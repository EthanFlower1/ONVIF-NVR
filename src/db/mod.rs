use crate::config::DatabaseConfig;
use crate::error::Error;
use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};
use once_cell::sync::OnceCell;

pub mod migrations;
pub mod models;
pub mod repositories;

// Global database pool for use throughout the application
static DB_POOL: OnceCell<Arc<PgPool>> = OnceCell::new();

/// Database service for handling connections and migrations
pub struct DatabaseService {
    pub pool: Arc<PgPool>,
    config: DatabaseConfig,
}

impl DatabaseService {
    /// Create a new database service
    pub async fn new(config: &DatabaseConfig) -> Result<Self> {
        info!("Initializing Database service");

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.url)
            .await
            .map_err(|e| Error::Database(format!("Failed to connect to database: {}", e)))?;

        info!("Connected to PostgreSQL database");

        // Create a shared instance and store it in the global pool
        let pool_arc = Arc::new(pool);
        
        // Initialize the global DB_POOL if it hasn't been set yet
        let _ = DB_POOL.set(pool_arc.clone());

        let service = Self {
            pool: pool_arc,
            config: config.clone(),
        };

        // Run migrations if configured
        if config.auto_migrate {
            service.run_migrations().await?;
        }

        Ok(service)
    }

    /// Run database migrations
    pub async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations");

        migrations::run_migrations(&self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to run migrations: {}", e)))?;

        info!("Database migrations completed successfully");

        Ok(())
    }

    /// Health check for database
    pub async fn health_check(&self) -> Result<bool> {
        match sqlx::query("SELECT 1").execute(&*self.pool).await {
            Ok(_) => Ok(true),
            Err(e) => {
                error!("Database health check failed: {}", e);
                Ok(false)
            }
        }
    }
}

/// Get the global database connection pool
/// Returns an error if the pool has not been initialized
pub async fn get_connection_pool() -> Result<PgPool> {
    match DB_POOL.get() {
        Some(pool) => Ok(pool.as_ref().clone()),
        None => Err(Error::Database("Database pool not initialized".to_string()).into()),
    }
}

