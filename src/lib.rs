pub mod stream_manager;
pub mod services;
pub mod api;
pub mod config;

// Re-export main components for easier use
pub use stream_manager::{
    StreamManager,
    StreamSource, 
    StreamType,
    BranchType,
    BranchConfig,
    StreamId,
    BranchId
};