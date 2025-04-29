use crate::config::StorageCleanupConfig;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::messaging::broker::MessageBrokerTrait;
use anyhow::{anyhow, Result};
use chrono::Utc;
use log::{error, info, warn};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

/// Storage cleanup service for managing recording retention
pub struct StorageCleanupService {
    config: StorageCleanupConfig,
    recordings_repo: RecordingsRepository,
    recordings_path: Arc<Path>,
    message_broker: Arc<Mutex<Option<Arc<crate::messaging::MessageBroker>>>>,
}

impl StorageCleanupService {
    /// Create a new storage cleanup service
    pub fn new(
        config: StorageCleanupConfig,
        recordings_repo: RecordingsRepository,
        recordings_path: &Path,
    ) -> Self {
        Self {
            config,
            recordings_repo,
            recordings_path: Arc::from(recordings_path),
            message_broker: Arc::new(Mutex::new(None)),
        }
    }

    /// Set message broker for event publishing
    pub async fn set_message_broker(
        &self,
        broker: Arc<crate::messaging::MessageBroker>,
    ) -> Result<()> {
        // Safely update the message broker through the mutex
        {
            let mut broker_guard = self.message_broker.lock().await;
            *broker_guard = Some(broker.clone());
        }

        // Publish a startup event
        broker
            .publish(
                crate::messaging::EventType::SystemStartup,
                None,
                serde_json::json!({"component": "storage_cleanup_service"}),
            )
            .await?;

        Ok(())
    }

    /// Start the cleanup service in the background
    pub async fn start(self: Arc<Self>) -> Result<()> {
        if !self.config.enabled {
            info!("Storage cleanup service is disabled");
            return Ok(());
        }

        info!(
            "Starting storage cleanup service with interval of {} seconds",
            self.config.check_interval_secs
        );

        // Create task to periodically check storage
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(self.config.check_interval_secs));

            loop {
                interval.tick().await;

                if let Err(e) = self.run_cleanup().await {
                    error!("Error running storage cleanup: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Run the cleanup process
    async fn run_cleanup(&self) -> Result<()> {
        info!("Running storage cleanup process");

        // Publish cleanup started event
        if let Some(broker) = self.message_broker.lock().await.as_ref() {
            if let Err(e) = broker
                .publish(
                    crate::messaging::EventType::StorageCleanupStarted,
                    None,
                    serde_json::json!({}),
                )
                .await
            {
                warn!("Failed to publish cleanup started event: {}", e);
            }
        }

        // First check age-based retention
        let age_cleanup_count = self.cleanup_by_age().await?;

        // Then check storage usage
        let storage_cleanup_count = if age_cleanup_count == 0 {
            // Only check storage if we didn't already delete files by age
            self.cleanup_by_storage_usage().await?
        } else {
            0
        };

        // Publish cleanup completed event
        if let Some(broker) = self.message_broker.lock().await.as_ref() {
            if let Err(e) = broker
                .publish(
                    crate::messaging::EventType::StorageCleanupCompleted,
                    None,
                    serde_json::json!({
                        "age_based_deletions": age_cleanup_count,
                        "storage_based_deletions": storage_cleanup_count,
                        "total_deletions": age_cleanup_count + storage_cleanup_count
                    }),
                )
                .await
            {
                warn!("Failed to publish cleanup completed event: {}", e);
            }
        }

        Ok(())
    }

    /// Clean up recordings based on age
    async fn cleanup_by_age(&self) -> Result<u64> {
        info!(
            "Cleaning up recordings older than {} days",
            self.config.max_retention_days
        );

        // Calculate cutoff date
        let cutoff_date =
            Utc::now() - chrono::Duration::days(self.config.max_retention_days as i64);

        // Get recordings to delete
        let recordings = self
            .recordings_repo
            .get_recordings_to_prune(None, Some(cutoff_date))
            .await?;

        if recordings.is_empty() {
            info!("No expired recordings found");
            return Ok(0);
        }

        info!("Found {} expired recordings to clean up", recordings.len());

        let mut delete_count = 0;
        for recording in recordings {
            // Delete the file
            if let Err(e) = std::fs::remove_file(&recording.file_path) {
                warn!(
                    "Failed to delete recording file {}: {}",
                    recording.file_path.display(),
                    e
                );
            }

            // Delete from database
            if let Ok(deleted) = self.recordings_repo.delete(&recording.id).await {
                if deleted {
                    delete_count += 1;
                }
            }
        }

        info!("Cleaned up {} expired recordings", delete_count);
        Ok(delete_count)
    }

    /// Clean up recordings based on storage usage
    async fn cleanup_by_storage_usage(&self) -> Result<u64> {
        // Get current disk usage
        let disk_usage = self.get_disk_usage()?;

        // Check if we need to clean up
        if disk_usage.percentage < self.config.max_disk_usage_percent as f64 {
            info!(
                "Current disk usage is {}%, below threshold of {}%. No cleanup needed.",
                disk_usage.percentage, self.config.max_disk_usage_percent
            );
            return Ok(0);
        }

        info!(
            "Current disk usage is {}%, above threshold of {}%. Cleaning up oldest recordings.",
            disk_usage.percentage, self.config.max_disk_usage_percent
        );

        // Get total recording stats
        let stats = self.recordings_repo.get_stats(None).await?;

        if stats.total_count == 0 {
            info!("No recordings found to clean up");
            return Ok(0);
        }

        // Calculate how many bytes to free
        let target_usage = (disk_usage.total_bytes as f64
            * (self.config.max_disk_usage_percent as f64 - 5.0)
            / 100.0) as u64;
        let bytes_to_free = disk_usage.used_bytes.saturating_sub(target_usage);

        if bytes_to_free == 0 {
            info!("No need to free disk space");
            return Ok(0);
        }

        info!(
            "Need to free approximately {} MB",
            bytes_to_free / 1024 / 1024
        );

        // Get oldest recordings first, limited to a reasonable batch size
        let mut deleted_bytes = 0;
        let mut delete_count = 0;
        let batch_size = 100;
        let mut processed = 0;

        while deleted_bytes < bytes_to_free && processed < 1000 {
            // Safety limit
            // Get a batch of oldest recordings
            let recordings = self
                .recordings_repo
                .get_recordings_to_prune(None, None)
                .await?;

            if recordings.is_empty() {
                break;
            }

            for recording in recordings.iter().take(batch_size) {
                // Delete the file
                if let Err(e) = std::fs::remove_file(&recording.file_path) {
                    warn!(
                        "Failed to delete recording file {}: {}",
                        recording.file_path.display(),
                        e
                    );
                    continue;
                }

                // Delete from database
                if let Ok(deleted) = self.recordings_repo.delete(&recording.id).await {
                    if deleted {
                        deleted_bytes += recording.file_size;
                        delete_count += 1;

                        if deleted_bytes >= bytes_to_free {
                            break;
                        }
                    }
                }
            }

            processed += batch_size;
        }

        info!(
            "Cleaned up {} recordings, freed {} MB",
            delete_count,
            deleted_bytes / 1024 / 1024
        );

        Ok(delete_count)
    }

    /// Get disk usage information
    fn get_disk_usage(&self) -> Result<DiskUsage> {
        #[cfg(target_os = "linux")]
        {
            let path = self.recordings_path.to_string_lossy().to_string();
            let out = std::process::Command::new("df")
                .args(&["--output=size,used,avail", "-k", &path])
                .output()?;

            if !out.status.success() {
                return Err(anyhow!("Failed to get disk usage"));
            }

            let output = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = output.lines().collect();

            if lines.len() < 2 {
                return Err(anyhow!("Invalid df output"));
            }

            let values: Vec<&str> = lines[1].split_whitespace().collect();
            if values.len() < 3 {
                return Err(anyhow!("Invalid df output format"));
            }

            let total_kb: u64 = values[0].parse()?;
            let used_kb: u64 = values[1].parse()?;

            let total_bytes = total_kb * 1024;
            let used_bytes = used_kb * 1024;
            let percentage = (used_bytes as f64 / total_bytes as f64) * 100.0;

            Ok(DiskUsage {
                total_bytes,
                used_bytes,
                percentage,
            })
        }

        #[cfg(target_os = "macos")]
        {
            let path = self.recordings_path.to_string_lossy().to_string();
            let out = std::process::Command::new("df")
                .args(&["-k", &path])
                .output()?;

            if !out.status.success() {
                return Err(anyhow!("Failed to get disk usage"));
            }

            let output = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = output.lines().collect();

            if lines.len() < 2 {
                return Err(anyhow!("Invalid df output"));
            }

            let values: Vec<&str> = lines[1].split_whitespace().collect();
            if values.len() < 5 {
                return Err(anyhow!("Invalid df output format"));
            }

            let total_kb: u64 = values[1].parse()?;
            let used_kb: u64 = values[2].parse()?;
            let percentage: f64 = values[4].trim_end_matches('%').parse()?;

            let total_bytes = total_kb * 1024;
            let used_bytes = used_kb * 1024;

            Ok(DiskUsage {
                total_bytes,
                used_bytes,
                percentage,
            })
        }

        #[cfg(target_os = "windows")]
        {
            // On Windows, use GetDiskFreeSpaceEx
            // For simplicity, we'll use a temporary implementation here
            let total_bytes = 1_000_000_000_000; // 1 TB
            let used_bytes = 500_000_000_000; // 500 GB
            let percentage = 50.0;

            Ok(DiskUsage {
                total_bytes,
                used_bytes,
                percentage,
            })
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err(anyhow!("Unsupported operating system"))
        }
    }
}

/// Disk usage information
struct DiskUsage {
    total_bytes: u64,
    used_bytes: u64,
    percentage: f64,
}

