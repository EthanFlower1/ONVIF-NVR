use std::env;
use std::error::Error;
use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Set up tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("failed to set subscriber");

    // Get database URL from environment variable or use default
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/g_streamer".to_string());

    info!("Connecting to database at {}", database_url);

    // Create database connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    // Run the specific recording field migration
    info!("Running recording fields migration...");
    match g_streamer::db::migrations::run_single_migration(&pool, "add_segment_fields.sql").await {
        Ok(_) => {
            info!("Migration completed successfully!");
            
            // Query the database to verify the new columns exist
            let result = sqlx::query!(
                r#"
                SELECT column_name 
                FROM information_schema.columns 
                WHERE table_name = 'recordings' AND 
                    (column_name = 'segment_id' OR column_name = 'parent_recording_id')
                "#
            )
            .fetch_all(&pool)
            .await?;
            
            info!("Verification: found {} new columns", result.len());
            for row in result {
                info!("- Column: {}", row.column_name);
            }
            
            // Show count of migrated recordings with segment IDs
            let segment_count = sqlx::query!(
                r#"SELECT COUNT(*) as count FROM recordings WHERE segment_id IS NOT NULL"#
            )
            .fetch_one(&pool)
            .await?;
            
            info!("Migrated {} recordings with segment IDs", segment_count.count.unwrap_or(0));
        }
        Err(e) => {
            error!("Migration failed: {}", e);
            return Err(e);
        }
    }

    Ok(())
}