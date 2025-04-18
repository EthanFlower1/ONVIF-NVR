use sqlx::PgPool;
use std::sync::Arc;

pub mod camera_event_settings;
pub mod cameras;
pub mod events;
pub mod recordings;
pub mod schedules;
pub mod users;

/// Base repository with shared functionality
pub struct Repository {
    /// Database connection pool
    pub pool: Arc<PgPool>,
}

impl Repository {
    /// Create a new repository
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

