#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::OnceLock, time::Duration};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};

type PendingPackage = Option<(Update, Vec<u8>)>;
static PENDING: OnceLock<tokio::sync::Mutex<PendingPackage>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Channel {
    #[default]
    Legacy,
    Control,
}

#[derive(Default, Deserialize)]
struct CheckRequest {
    #[serde(default)]
    channel: Channel,
}

#[derive(Deserialize)]
struct InstallRequest {
    version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckReport {
    current_version: String,
    endpoint: String,
    update_available: bool,
    latest_version: Option<String>,
    notes: Option<String>,
    public_key_sha256: String,
    download_verified: bool,
    package_sha256: Option<String>,
    download_page: String,
}

fn endpoint(app: &AppHandle, channel: Channel) -> Result<String, String> {
    let config = &app.config().plugins.0["updater"];
    let legacy = config["endpoints"][0]
        .as_str()
        .ok_or("Missing lab channel endpoint")?;
    Ok(match channel {
        Channel::Legacy => legacy.to_string(),
        Channel::Control => legacy.replace("channel-legacy.json", "channel-control.json"),
    })
}

fn download_page(app: &AppHandle) -> Result<String, String> {
    Ok(endpoint(app, Channel::Legacy)?
        .trim_end_matches("/channel-legacy.json")
        .replace("/releases/download/", "/releases/tag/")
        .trim_end_matches("0.2.19")
        .to_string()
        + "0.2.20")
}

async fn check(app: &AppHandle, channel: Channel) -> Result<(CheckReport, Option<Update>), String> {
    let url = endpoint(app, channel)?;
    let exit_app = app.clone();
    let updater = app
        .updater_builder()
        .endpoints(vec![url
            .parse()
            .map_err(|e| format!("Invalid URL: {e}"))?])
        .map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(25))
        .on_before_exit(move || exit_app.cleanup_before_exit())
        .build()
        .map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let key = app.config().plugins.0["updater"]["pubkey"]
        .as_str()
        .ok_or("Missing lab public key")?;
    let report = CheckReport {
        current_version: app.package_info().version.to_string(),
        endpoint: url,
        update_available: update.is_some(),
        latest_version: update.as_ref().map(|u| u.version.clone()),
        notes: update.as_ref().and_then(|u| u.body.clone()),
        public_key_sha256: format!("{:x}", Sha256::digest(key.trim().as_bytes())),
        download_verified: false,
        package_sha256: None,
        download_page: download_page(app)?,
    };
    Ok((report, update))
}

async fn verified_download(mut update: Update) -> Result<(Update, Vec<u8>), String> {
    // This fixture has an explicit ceiling so a CI failure cannot hang a runner.
    update.timeout = Some(Duration::from_secs(180));
    // Use the real plugin path. It verifies minisign before returning these bytes.
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    Ok((update, bytes))
}

#[tauri::command]
async fn check_for_updates(app: AppHandle, request: CheckRequest) -> Result<CheckReport, String> {
    check(&app, request.channel).await.map(|(report, _)| report)
}

#[tauri::command]
async fn download_update(app: AppHandle, request: CheckRequest) -> Result<CheckReport, String> {
    let mut pending = PENDING
        .get_or_init(Default::default)
        .try_lock()
        .map_err(|_| "A lab update is already in progress")?;
    let (mut report, update) = check(&app, request.channel).await?;
    let (update, bytes) = verified_download(update.ok_or("No update available")?).await?;
    report.download_verified = true;
    report.package_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
    *pending = Some((update, bytes));
    Ok(report)
}

#[tauri::command]
async fn install_update(request: InstallRequest) -> Result<(), String> {
    let mut pending = PENDING
        .get_or_init(Default::default)
        .try_lock()
        .map_err(|_| "A lab update is already in progress")?;
    let (update, bytes) = pending
        .as_ref()
        .ok_or("Download and verify the package first")?;
    if update.version != request.version {
        return Err("The downloaded version changed; check again".into());
    }
    let (update, bytes) = (update.clone(), bytes.clone());
    let result = tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string());
    if result.is_ok() {
        *pending = None;
    }
    result
}

#[tauri::command]
fn open_download_page(app: AppHandle, request: CheckRequest) -> Result<(), String> {
    let _ = request;
    app.opener()
        .open_url(download_page(&app)?, None::<&str>)
        .map_err(|e| e.to_string())
}

fn argument(name: &str) -> Option<String> {
    let args = std::env::args().collect::<Vec<_>>();
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn has_argument(name: &str) -> bool {
    std::env::args().any(|arg| arg == name)
}

fn write_report(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let temp = path.with_extension("part");
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}

async fn smoke(app: &AppHandle, report_path: &Path) -> Result<(), String> {
    let channel = match argument("--channel").as_deref() {
        None | Some("legacy") => Channel::Legacy,
        Some("control") => Channel::Control,
        _ => return Err("Invalid lab channel".into()),
    };
    let (mut report, update) = check(app, channel).await?;
    let is_upgrade = has_argument("--lab-upgrade");
    if is_upgrade && report.current_version == "0.2.20" {
        // NSIS forwards the original arguments to the restarted application.
        return write_report(report_path, &report);
    }
    if has_argument("--verify-download") || is_upgrade {
        let (update, bytes) = verified_download(update.ok_or("No update available")?).await?;
        report.download_verified = true;
        report.package_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        if is_upgrade {
            if report.current_version != "0.2.19" || update.version != "0.2.20" {
                return Err("The lab only installs 0.2.19 -> 0.2.20".into());
            }
            write_report(&report_path.with_extension("before.json"), &report)?;
            return tauri::async_runtime::spawn_blocking(move || update.install(bytes))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string());
        }
    }
    write_report(report_path, &report)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            check_for_updates,
            download_update,
            install_update,
            open_download_page
        ])
        .setup(|app| {
            if has_argument("--lab-smoke") || has_argument("--lab-upgrade") {
                let report = argument("--report").ok_or("A smoke run requires --report")?;
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let result = smoke(&handle, Path::new(&report)).await;
                    if let Err(error) = &result {
                        let _ =
                            write_report(Path::new(&report), &serde_json::json!({"error": error}));
                    }
                    handle.exit(if result.is_ok() { 0 } else { 1 });
                });
            } else if let Some(window) = app.get_webview_window("main") {
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Failed to run the isolated updater lab");
}
