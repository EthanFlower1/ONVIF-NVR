use crate::db::repositories::cameras::CamerasRepository;
use crate::db::repositories::schedules::SchedulesRepository;
use crate::recorder::record::RecordingManager;
use anyhow::Result;
use log::{error, info, warn};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{interval, Duration};

/// Manages recording schedules and starts/stops recordings based on schedule times
pub struct RecordingScheduler {
    schedules_repo: SchedulesRepository,
    cameras_repo: CamerasRepository,
    recording_manager: Arc<RecordingManager>,
    check_interval: Duration,
}

impl RecordingScheduler {
    /// Create a new recording scheduler
    pub fn new(
        db_pool: Arc<PgPool>,
        _stream_manager: Arc<crate::stream_manager::StreamManager>,
        recording_manager: Arc<RecordingManager>,
        check_interval_secs: u64,
    ) -> Self {
        Self {
            schedules_repo: SchedulesRepository::new(db_pool.clone()),
            cameras_repo: CamerasRepository::new(db_pool.clone()),
            recording_manager,
            check_interval: Duration::from_secs(check_interval_secs),
        }
    }

    /// Start the recording scheduler service
    pub async fn start(self: Arc<Self>) -> Result<()> {
        info!("Starting recording scheduler service");

        // Create task to periodically check schedules
        tokio::spawn(async move {
            let mut interval = interval(self.check_interval);

            loop {
                interval.tick().await;

                if let Err(e) = self.process_schedules().await {
                    error!("Error processing recording schedules: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Process recording schedules
    async fn process_schedules(&self) -> Result<()> {
        info!("Processing recording schedules");

        // Get all currently active schedules
        let active_schedules = self.schedules_repo.get_active_schedules().await?;
        info!("Found {} active schedules", active_schedules.len());

        // Track streams that should be recording now
        let mut should_be_recording = HashMap::new();

        // Start recording for all active schedules
        for schedule in &active_schedules {
            // Get the camera and associated stream
            let stream = match self
                .cameras_repo
                .get_stream_by_id(&schedule.stream_id)
                .await?
            {
                Some(stream) => stream,
                None => {
                    warn!(
                        "Stream {} for schedule {} not found",
                        schedule.stream_id, schedule.id
                    );
                    continue;
                }
            };

            // Check if already recording this schedule
            if self
                .recording_manager
                .is_recording_active(&schedule.id, &stream.id).await
            {
                // Already recording, mark as should be recording
                should_be_recording.insert(format!("{}-{}", schedule.id, stream.id), true);
                continue;
            }

            // Start new recording
            match self
                .recording_manager
                .start_recording(schedule, &stream)
                .await
            {
                Ok(recording_id) => {
                    info!(
                        "Started recording {} for schedule {}",
                        recording_id, schedule.id
                    );

                    // Mark as should be recording
                    should_be_recording.insert(format!("{}-{}", schedule.id, stream.id), true);
                }
                Err(e) => {
                    error!(
                        "Failed to start recording for schedule {}: {}",
                        schedule.id, e
                    );
                }
            }
        }

        // Get all enabled schedules to check for ones that should be stopped
        let all_enabled_schedules = self.schedules_repo.get_all_enabled().await?;

        // Check for recordings that should be stopped
        for schedule in &all_enabled_schedules {
            // Skip if not active and should be recording
            let key = format!("{}-{}", schedule.id, schedule.stream_id);
            if should_be_recording.contains_key(&key) {
                continue;
            }

            // Check if currently recording
            if self
                .recording_manager
                .is_recording_active(&schedule.id, &schedule.stream_id).await
            {
                // Stop recording
                match self
                    .recording_manager
                    .stop_recording(&schedule.id, &schedule.stream_id)
                    .await
                {
                    Ok(_) => {
                        info!("Stopped recording for schedule {}", schedule.id);
                    }
                    Err(e) => {
                        error!(
                            "Failed to stop recording for schedule {}: {}",
                            schedule.id, e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Properly shut down the scheduler and stop all recordings
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down recording scheduler");

        // Stop all active recordings
        self.recording_manager.stop_all_recordings().await?;

        Ok(())
    }
}

