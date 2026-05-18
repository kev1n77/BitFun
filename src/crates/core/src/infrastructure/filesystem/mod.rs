//! Filesystem infrastructure
//!
//! File operations and file tree building.

pub mod file_operations;
pub mod file_tree;

pub use file_operations::{
    normalize_text_for_editor_disk_sync, FileInfo, FileOperationOptions, FileOperationService,
    FileReadResult, FileWriteResult,
};
pub use file_tree::{
    BatchedFileSearchProgressSink, FileContentSearchOptions, FileNameSearchOptions,
    FileSearchOutcome, FileSearchProgressSink, FileSearchResult, FileSearchResultGroup,
    FileTreeNode, FileTreeOptions, FileTreeService, FileTreeStatistics, SearchMatchType,
};
