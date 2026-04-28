//! Desktop-only gate for Computer use (set from BitFun desktop at startup).

use std::sync::atomic::{AtomicBool, Ordering};

static COMPUTER_USE_DESKTOP_AVAILABLE: AtomicBool = AtomicBool::new(false);

const COMPUTER_USE_FEATURE_ENABLED: bool = false;

pub fn computer_use_feature_enabled() -> bool {
    COMPUTER_USE_FEATURE_ENABLED
}

/// Mark whether this process is BitFun desktop with OS automation wired up.
pub fn set_computer_use_desktop_available(available: bool) {
    COMPUTER_USE_DESKTOP_AVAILABLE
        .store(available && COMPUTER_USE_FEATURE_ENABLED, Ordering::SeqCst);
}

pub fn computer_use_desktop_available() -> bool {
    COMPUTER_USE_FEATURE_ENABLED && COMPUTER_USE_DESKTOP_AVAILABLE.load(Ordering::SeqCst)
}
