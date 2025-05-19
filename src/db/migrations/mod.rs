use std::{fs, path::Path};

use anyhow::Result;
use sqlx::{Executor, PgPool};
use tracing::{info, warn};

pub async fn run_migrations(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let migrations_dir = "/Users/ethanflower/projects/g-streamer/src/db/migrations/sql";

    // Get all SQL files from the directory
    // info!("Gathering all migration files...");
    let mut entries = fs::read_dir(migrations_dir)?
        .filter_map(Result::ok)
        .filter(|entry| {
            let path = entry.path();
            path.extension().map(|ext| ext == "sql").unwrap_or(false)
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    // info!("Files collection: {:?}", entries);
    // Custom sorting logic to handle special files
    entries.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Helper function to determine file order
        fn get_order_value(name: &str) -> usize {
            if name.starts_with("add_foreign_keys") {
                // Foreign keys should be added after tables
                return 1000;
            } else if name.starts_with("add_indexes") {
                // Indexes should be added after foreign keys
                return 2000;
            } else {
                // For numbered files, use their numeric prefix
                name.split('_')
                    .next()
                    .and_then(|prefix| prefix.parse::<usize>().ok())
                    .unwrap_or(usize::MAX)
            }
        }

        get_order_value(a_name).cmp(&get_order_value(b_name))
    });

    // Execute each file in order
    for path in entries {
        execute_migration_file(pool, &path).await?;
        println!("Applied migration: {}", path.display());
    }

    Ok(())
}

/// Run a specific migration file by name
pub async fn run_single_migration(
    pool: &PgPool,
    migration_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let migrations_dir = "/Users/ethanflower/projects/g-streamer/src/db/migrations/sql";
    let migration_path = Path::new(migrations_dir).join(migration_name);

    if !migration_path.exists() || !migration_path.is_file() {
        return Err(format!("Migration file {} not found", migration_name).into());
    }

    // info!("Running single migration: {}", migration_name);
    execute_migration_file(pool, &migration_path).await?;
    println!("Applied migration: {}", migration_path.display());

    Ok(())
}

async fn execute_migration_file(
    pool: &PgPool,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let sql = fs::read_to_string(path)?;

    // Execute the SQL script
    // info!("Executing migration: {:?}", path.file_name());
    pool.execute(&*sql).await?;

    Ok(())
}

/// Create default admin user if no users exist
async fn create_default_admin(pool: &PgPool) -> Result<()> {
    // Check if any users exist
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    if user_count == 0 {
        // info!("Creating default admin user...");

        // Generate a secure password hash for "admin" (should be changed immediately)
        let password_hash = bcrypt::hash("admin", 10)?;

        // Insert default admin user with direct SQL to avoid type issues
        let id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();

        sqlx::query(&format!(
            r#"
            INSERT INTO users (id, username, email, password_hash, role, created_at, updated_at, active)
            VALUES ('{}', 'admin', 'admin@localhost', '{}', 'admin', '{}', '{}', true)
            "#,
            id, password_hash, now, now
        ))
        .execute(pool)
        .await?;

        // info!("Default admin user created with username 'admin' and password 'admin'");
        warn!("Please change the default admin password immediately!");
    }

    Ok(())
}
