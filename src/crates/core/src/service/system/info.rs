//! System info utilities
//!
//! Provides system info retrieval.

use crate::util::process_manager;

/// System info
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemInfo {
    /// OS platform: "windows", "macos", "linux"
    pub platform: String,
    /// OS architecture: "x86_64", "aarch64", etc.
    pub arch: String,
    /// OS version
    pub os_version: Option<String>,
}

/// Gets system info.
///
/// # Returns
/// - `SystemInfo`: System info including platform and architecture
pub fn get_system_info() -> SystemInfo {
    SystemInfo {
        platform: detect_platform(),
        arch: std::env::consts::ARCH.to_string(),
        os_version: detect_os_version(),
    }
}

fn detect_platform() -> String {
    if cfg!(target_os = "windows") {
        "windows".to_string()
    } else if cfg!(target_os = "macos") {
        "macos".to_string()
    } else if cfg!(target_os = "linux") {
        "linux".to_string()
    } else {
        std::env::consts::OS.to_string()
    }
}

fn detect_os_version() -> Option<String> {
    let output = if cfg!(target_os = "macos") {
        process_manager::create_command("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
    } else if cfg!(target_os = "linux") {
        process_manager::create_command("uname")
            .arg("-r")
            .output()
            .ok()
    } else if cfg!(target_os = "windows") {
        process_manager::create_command("cmd")
            .args(["/C", "ver"])
            .output()
            .ok()
    } else {
        None
    }?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::get_system_info;

    #[test]
    fn returns_platform_and_arch_for_current_machine() {
        let info = get_system_info();

        assert!(!info.platform.trim().is_empty());
        assert!(!info.arch.trim().is_empty());
    }

    #[test]
    fn returns_os_version_for_supported_platforms() {
        let info = get_system_info();

        if cfg!(any(
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )) {
            assert!(
                info.os_version
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "expected os_version for supported platform, got {:?}",
                info.os_version
            );
        }
    }
}
