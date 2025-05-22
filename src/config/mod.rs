use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Top-level configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub api: ApiConfig,
    pub onvif: OnvifConfig,
    pub recording: RecordingConfig,
    pub streaming: StreamingConfig,
    pub database: DatabaseConfig,
    pub security: SecurityConfig,
    pub message_broker: MessageBrokerConfig,
}

/// API server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ApiConfig {
    /// API server address
    pub address: String,
    /// API server port
    pub port: u16,
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_buffer_size_mb() -> usize {
    32 // Default to 32MB buffer capacity
}

fn default_buffer_duration() -> u64 {
    10 // Default to 10 seconds of buffer
}

/// ONVIF service configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnvifConfig {
    /// ONVIF discovery broadcast address
    pub discovery_address: String,
    /// ONVIF discovery port
    pub discovery_port: u16,
    /// ONVIF discovery timeout (seconds)
    pub discovery_timeout: u64,
    /// Database pool for accessing camera information
    #[serde(skip)]
    pub db_pool: Option<Arc<sqlx::PgPool>>,
}

/// Recording service configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecordingConfig {
    /// Storage path for recordings
    pub storage_path: PathBuf,
    /// Maximum storage size in gigabytes
    pub max_storage_gb: u64,
    /// Default recording segment duration in seconds
    pub segment_duration: u64,
    /// Recording file format (mp4, mkv)
    pub format: String,
    /// Default retention period in days
    pub retention_days: i32,
    /// Storage cleanup configuration
    #[serde(default)]
    pub cleanup: StorageCleanupConfig,
}

/// Storage cleanup configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageCleanupConfig {
    /// Whether cleanup is enabled
    pub enabled: bool,
    /// Maximum retention period in days
    pub max_retention_days: i32,
    /// Maximum disk usage percentage before cleanup
    pub max_disk_usage_percent: u8,
    /// Interval in seconds to check for cleanup
    pub check_interval_secs: u64,
}

/// Streaming service configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StreamingConfig {
    /// Multicast address range base
    pub multicast_address_base: String,
    /// Multicast port range start
    pub multicast_port_start: u16,
    /// Streaming buffer size in milliseconds
    pub buffer_ms: u64,
    /// Shared buffer capacity in megabytes
    #[serde(default = "default_buffer_size_mb")]
    pub buffer_size_mb: usize,
    /// Shared buffer duration in seconds
    #[serde(default = "default_buffer_duration")]
    pub buffer_duration: u64,
}

/// Database configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DatabaseConfig {
    /// Database URL
    #[serde(default = "default_db_url")]
    pub url: String,
    /// Connection pool max size
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    /// Automatic migration on startup
    #[serde(default)]
    pub auto_migrate: bool,
}

fn default_db_url() -> String {
    // Get database connection parameters from environment variables or use defaults
    let host = std::env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "postgres".to_string());
    let password = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
    let db = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "server".to_string());

    format!("postgres://{}:{}@{}:{}/{}", user, password, host, port, db)
}

fn default_max_connections() -> u32 {
    5
}

/// Security configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SecurityConfig {
    /// JWT secret key
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    /// JWT token expiration time in minutes
    #[serde(default = "default_jwt_expiration")]
    pub jwt_expiration_minutes: u64,
    /// Password hashing cost (higher is more secure but slower)
    #[serde(default = "default_password_hash_cost")]
    pub password_hash_cost: u32,
}

fn default_jwt_secret() -> String {
    "default_secret_change_in_production".to_string()
}

fn default_jwt_expiration() -> u64 {
    60 // 60 minutes
}

fn default_password_hash_cost() -> u32 {
    10 // reasonable default for bcrypt
}

/// Message broker (RabbitMQ) configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageBrokerConfig {
    /// RabbitMQ connection URI
    #[serde(default = "default_rabbitmq_uri")]
    pub uri: String,
    /// Connection pool size
    #[serde(default = "default_rabbitmq_pool_size")]
    pub pool_size: u32,
    /// Exchange name for event publishing
    #[serde(default = "default_rabbitmq_exchange")]
    pub exchange: String,
    /// Dead letter exchange name
    #[serde(default = "default_rabbitmq_dlx")]
    pub dead_letter_exchange: String,
    /// Default message timeout in milliseconds
    #[serde(default = "default_rabbitmq_timeout")]
    pub timeout_ms: u64,
    /// Connection retry attempts
    #[serde(default = "default_rabbitmq_retry_attempts")]
    pub retry_attempts: u32,
    /// Connection retry delay in milliseconds
    #[serde(default = "default_rabbitmq_retry_delay")]
    pub retry_delay_ms: u64,
}

fn default_rabbitmq_uri() -> String {
    "amqp://guest:guest@localhost:5672/%2f".to_string()
}

fn default_rabbitmq_pool_size() -> u32 {
    5
}

fn default_rabbitmq_exchange() -> String {
    "gstreamer.events".to_string()
}

fn default_rabbitmq_dlx() -> String {
    "gstreamer.events.dlx".to_string()
}

fn default_rabbitmq_timeout() -> u64 {
    30000 // 30 seconds
}

fn default_rabbitmq_retry_attempts() -> u32 {
    3
}

fn default_rabbitmq_retry_delay() -> u64 {
    1000 // 1 second
}

impl Default for StorageCleanupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retention_days: 30,
            max_disk_usage_percent: 80,
            check_interval_secs: 3600,
        }
    }
}

impl Default for MessageBrokerConfig {
    fn default() -> Self {
        Self {
            uri: default_rabbitmq_uri(),
            pool_size: default_rabbitmq_pool_size(),
            exchange: default_rabbitmq_exchange(),
            dead_letter_exchange: default_rabbitmq_dlx(),
            timeout_ms: default_rabbitmq_timeout(),
            retry_attempts: default_rabbitmq_retry_attempts(),
            retry_delay_ms: default_rabbitmq_retry_delay(),
        }
    }
}

/// Helper to get environment variables with defaults
fn get_env_var<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|val| val.parse::<T>().ok())
        .unwrap_or(default)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api: ApiConfig {
                address: std::env::var("API_ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: get_env_var("RUST_SERVER_PORT", 4750),
                log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            },
            onvif: OnvifConfig {
                discovery_address: "239.255.255.250".to_string(),
                discovery_port: 3702,
                discovery_timeout: 3,
                db_pool: None,
            },
            recording: RecordingConfig {
                storage_path: {
                    let recordings_dir = PathBuf::from(
                        std::env::var("RECORDINGS_PATH")
                            .unwrap_or_else(|_| "./recordings".to_string()),
                    );
                    // Create the directory if it doesn't exist
                    if !recordings_dir.exists() {
                        let _ = std::fs::create_dir_all(&recordings_dir);
                    }
                    // Use absolute path if possible, otherwise use relative
                    std::fs::canonicalize(&recordings_dir).unwrap_or(recordings_dir)
                },
                max_storage_gb: get_env_var("MAX_STORAGE_GB", 500),
                segment_duration: get_env_var("SEGMENT_DURATION", 30), // 30 seconds
                format: std::env::var("RECORDING_FORMAT").unwrap_or_else(|_| "mp4".to_string()),
                retention_days: get_env_var("RETENTION_DAYS", 30),
                cleanup: StorageCleanupConfig::default(),
            },
            streaming: StreamingConfig {
                multicast_address_base: "239.0.0.0".to_string(),
                multicast_port_start: 5000,
                buffer_ms: 500,
                buffer_size_mb: 32,
                buffer_duration: 10,
            },
            database: DatabaseConfig {
                url: "postgres://postgres:postgres@localhost:5432/server".to_string(),
                max_connections: 5,
                auto_migrate: true,
            },
            security: SecurityConfig {
                jwt_secret: "change_this_to_a_secure_random_string_in_production".to_string(),
                jwt_expiration_minutes: 60,
                password_hash_cost: 10,
            },
            message_broker: MessageBrokerConfig::default(),
        }
    }
}

/// Load configuration from a file or use default
pub fn load_config(config_path: Option<&Path>) -> Result<Config> {
    match config_path {
        Some(path) => {
            let config_str = std::fs::read_to_string(path)
                .context(format!("Failed to read config file: {:?}", path))?;

            let config = if path.extension().map_or(false, |ext| ext == "json") {
                serde_json::from_str(&config_str).context("Failed to parse JSON config")?
            } else if path.extension().map_or(false, |ext| ext == "toml") {
                toml::from_str(&config_str).context("Failed to parse TOML config")?
            } else {
                return Err(anyhow::anyhow!("Unsupported config file format"));
            };

            Ok(config)
        }
        None => Ok(Config::default()),
    }
}
