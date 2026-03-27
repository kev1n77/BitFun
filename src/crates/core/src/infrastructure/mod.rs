//! Infrastructure module
//!
//! Provides low-level services: AI clients, storage, event system

pub mod ai;
pub mod app_paths;
pub mod debug_log;
pub mod events;
pub mod filesystem;
pub mod storage;
pub mod telemetry;

pub use ai::AIClient;
pub use app_paths::{
    get_path_manager_arc, try_get_path_manager_arc, CacheType, PathManager, StorageLevel,
};
pub use events::BackendEventManager;
pub use filesystem::{
    BatchedFileSearchProgressSink, FileContentSearchOptions, FileInfo, FileNameSearchOptions,
    FileOperationOptions, FileOperationService, FileReadResult, FileSearchOutcome,
    FileSearchProgressSink, FileSearchResult, FileSearchResultGroup, FileTreeNode, FileTreeOptions,
    FileTreeService, FileTreeStatistics, FileWriteResult, SearchMatchType,
};
pub use telemetry::{
    get_global_telemetry, get_telemetry_identity, initialize_global_telemetry,
    shutdown_global_telemetry, with_telemetry_request_context, ConfiguredTelemetry,
    TelemetryEventSubscriber, TelemetryIdentity, TelemetryInitConfig, TelemetryRequestContext,
    TelemetryService,
};
// pub use storage::{};
