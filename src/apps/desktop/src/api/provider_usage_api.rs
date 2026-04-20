//! Provider Usage Statistics API
//!
//! Proxies API usage statistics from internal provider dashboard (http://7.242.99.159:8888)

use crate::api::app_state::AppState;
use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use std::time::Duration;

const PROVIDER_BASE_URL: &str = "http://7.242.99.159:8888";
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Request for usage statistics
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderUsageRequest {
    pub api_key: String,
}

/// Request for usage logs
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderLogsRequest {
    pub api_key: String,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

/// Response from /api/user/key-info (contains usage data)
#[derive(Debug, Serialize, Deserialize)]
pub struct UsageStats {
    pub spend: f64,
    #[serde(rename = "total_input_tokens")]
    pub total_input_tokens: u64,
    #[serde(rename = "total_output_tokens")]
    pub total_output_tokens: u64,
    #[serde(rename = "created_at")]
    pub created_at: Option<String>,
    #[serde(rename = "key_alias")]
    pub key_alias: Option<String>,
    #[serde(rename = "key_name")]
    pub key_name: Option<String>,
    #[serde(rename = "rpm_limit")]
    pub rpm_limit: u32,
    #[serde(rename = "tpm_limit")]
    pub tpm_limit: Option<u32>,
    #[serde(rename = "max_budget")]
    pub max_budget: Option<f64>,
    #[serde(rename = "budget_duration")]
    pub budget_duration: Option<String>,
    pub expires: Option<String>,
    pub blocked: bool,
    #[serde(rename = "token_prefix")]
    pub token_prefix: Option<String>,
}

/// Response from /api/user/usage (contains plan/rate limit data)
#[derive(Debug, Serialize, Deserialize)]
pub struct PlanInfo {
    pub concurrency: Option<u32>,
    pub concurrency_limit: u32,
    pub plan_name: String,
    pub windows: Vec<UsageWindow>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageWindow {
    pub cache_key: String,
    pub count: u32,
    pub elapsed_secs: u64,
    pub limit: u64,
    pub window_secs: u64,
}

/// Response from /api/user/plan (contains key info with rate limits)
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyInfo {
    pub concurrency_limit: u32,
    pub plan_name: String,
    pub rpm_limit: u32,
    pub schedule: Vec<ScheduleItem>,
    pub window_limits: Vec<Vec<u32>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScheduleItem {
    pub hours: String,
    pub rpm_limit: u32,
    pub concurrency_limit: u32,
    pub window_limits: Vec<Vec<u32>>,
}

/// Response from /api/user/logs
#[derive(Debug, Serialize, Deserialize)]
pub struct UsageLogs {
    pub logs: Vec<UsageLogEntry>,
    pub page: u32,
    #[serde(rename = "per_page")]
    pub per_page: u32,
    pub total: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageLogEntry {
    #[serde(rename = "created_at")]
    pub created_at: String,
    pub model: String,
    #[serde(rename = "api_path")]
    pub api_path: String,
    #[serde(rename = "status_code")]
    pub status_code: u32,
    #[serde(rename = "error_type")]
    pub error_type: Option<String>,
    #[serde(rename = "error_message")]
    pub error_message: Option<String>,
    #[serde(rename = "input_tokens")]
    pub input_tokens: Option<u32>,
    #[serde(rename = "output_tokens")]
    pub output_tokens: Option<u32>,
    #[serde(rename = "duration_ms")]
    pub duration_ms: Option<u32>,
    #[serde(rename = "is_stream")]
    pub is_stream: bool,
}

/// Combined usage statistics response
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CombinedUsageStats {
    pub usage: UsageStats,
    pub plan: PlanInfo,
    pub key_info: KeyInfo,
}

fn create_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))
}

/// Check if a base_url matches the internal provider
pub fn is_internal_provider(base_url: &str) -> bool {
    base_url.contains("7.242.99.159") || base_url.contains("internal")
}

async fn login_and_get_session(client: &Client, api_key: &str) -> Result<String, String> {
    let login_url = format!("{}/dashboard/api/auth/login", PROVIDER_BASE_URL);

    let body = serde_json::json!({
        "user_id": "",
        "api_key": api_key
    });

    let resp = client
        .post(&login_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Login request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("Login failed: {} - {}", status, body_text));
    }

    // Extract session cookie from Set-Cookie header (case-insensitive via manual check)
    // The server may use "session", "boom_session", or other names
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .or_else(|| resp.headers().get("SET-COOKIE"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "No Set-Cookie header in login response".to_string())?;

    log::info!("Set-Cookie header: {}", set_cookie);

    // Parse cookie: extract name=value from "name=value; options" format
    let cookie_value = set_cookie
        .split(';')
        .next()
        .and_then(|part| {
            let part = part.trim();
            // Find the first = sign and use everything after it as value
            part.find('=').map(|i| (i, &part[i + 1..]))
        })
        .ok_or_else(|| format!("Failed to parse cookie from: {}", set_cookie))?;

    // Validate it's not empty
    if cookie_value.1.is_empty() {
        return Err(format!("Empty cookie value from: {}", set_cookie));
    }

    log::info!("Parsed cookie value length: {}", cookie_value.1.len());

    Ok(format!("{}", set_cookie.split(';').next().unwrap().trim()))
}

async fn fetch_with_session<T: for<'de> Deserialize<'de>>(
    client: &Client,
    session_cookie: &str,
    path: &str,
) -> Result<T, String> {
    let url = format!("{}{}", PROVIDER_BASE_URL, path);

    let resp = client
        .get(&url)
        .header("Cookie", session_cookie)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("API request failed: {} - {}", status, body_text));
    }

    let body_text = resp.text().await.map_err(|e| format!("Failed to read response body: {}", e))?;

    log::info!("Response from {}: {}", path, &body_text[..body_text.len().min(500)]);

    serde_json::from_str::<T>(&body_text)
        .map_err(|e| format!("Failed to parse response: {} | Body: {}", e, &body_text[..body_text.len().min(200)]))
}

/// Get combined usage statistics for a provider
#[tauri::command]
pub async fn get_provider_usage_stats(
    _state: State<'_, AppState>,
    request: GetProviderUsageRequest,
) -> Result<CombinedUsageStats, String> {
    info!("Fetching provider usage stats");

    let client = create_http_client()?;

    // First login to get session
    let session_cookie = login_and_get_session(&client, &request.api_key).await?;

    // Fetch all stats in parallel
    // Note: API endpoints have swapped content:
    // - /usage returns PlanInfo (rate limit data)
    // - /plan returns KeyInfo (rpm_limit, schedule)
    // - /key-info returns UsageStats (spend, tokens)
    let (plan_data, key_info_data, usage_data) = tokio::join!(
        fetch_with_session::<PlanInfo>(&client, &session_cookie, "/dashboard/api/user/usage"),
        fetch_with_session::<KeyInfo>(&client, &session_cookie, "/dashboard/api/user/plan"),
        fetch_with_session::<UsageStats>(&client, &session_cookie, "/dashboard/api/user/key-info"),
    );

    let usage = usage_data.map_err(|e| format!("Failed to get usage: {}", e))?;
    let plan = plan_data.map_err(|e| format!("Failed to get plan: {}", e))?;
    let key_info = key_info_data.map_err(|e| format!("Failed to get key info: {}", e))?;

    Ok(CombinedUsageStats {
        usage,
        plan,
        key_info,
    })
}

/// Get usage logs for a provider
#[tauri::command]
pub async fn get_provider_usage_logs(
    _state: State<'_, AppState>,
    request: GetProviderLogsRequest,
) -> Result<UsageLogs, String> {
    info!(
        "Fetching provider usage logs: page={}, per_page={}",
        request.page.unwrap_or(1),
        request.per_page.unwrap_or(50)
    );

    let client = create_http_client()?;

    // First login to get session
    let session_cookie = login_and_get_session(&client, &request.api_key).await?;

    let page = request.page.unwrap_or(1);
    let per_page = request.per_page.unwrap_or(50);

    let path = format!(
        "/dashboard/api/user/logs?page={}&per_page={}",
        page, per_page
    );

    fetch_with_session::<UsageLogs>(&client, &session_cookie, &path)
        .await
        .map_err(|e| format!("Failed to get usage logs: {}", e))
}