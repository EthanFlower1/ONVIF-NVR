pub mod api;
pub mod config;
pub mod services;
pub mod stream_manager;

// Re-export main components for easier use
pub use stream_manager::{
    // BranchType,
    // BranchConfig,
    StreamId,
    // BranchId
    StreamManager,
    StreamSource,
    StreamType,
};

