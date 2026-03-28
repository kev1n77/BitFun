//! Infrastructure module
//!
//! Provides low-level services: AI clients, storage, event system

pub mod ai;
pub mod debug_log;
pub mod events;
pub mod filesystem;
pub mod storage;
pub mod telemetry;

pub use ai::AIClient;
pub use events::BackendEventManager;
pub use filesystem::{
    file_watcher, get_path_manager_arc, initialize_file_watcher, try_get_path_manager_arc,
    FileInfo, FileOperationOptions, FileOperationService, FileReadResult, FileSearchResult,
    FileTreeNode, FileTreeOptions, FileTreeService, FileTreeStatistics, FileWriteResult,
    PathManager, SearchMatchType,
};
pub use telemetry::{
    flush_and_shutdown_global_telemetry, get_global_telemetry, get_telemetry_identity,
    initialize_global_telemetry, shutdown_global_telemetry,
    shutdown_global_telemetry_with_timeout, with_telemetry_request_context, ConfiguredTelemetry,
    TelemetryEventSubscriber, TelemetryIdentity, TelemetryInitConfig, TelemetryRequestContext,
    TelemetryService,
};
// pub use storage::{};
