use crate::db::models::recording_models::{Recording, RecordingEventType};
use crate::db::models::recording_schedule_models::RecordingSchedule;
use crate::db::models::stream_models::Stream;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::stream_manager::StreamManager;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use log::{error, info, warn};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

/// Manages the recording of streams from cameras
pub struct RecordingManager {
    stream_manager: Arc<StreamManager>,
    recordings_repo: RecordingsRepository,
    active_recordings: Mutex<HashMap<String, ActiveRecording>>,
    recording_base_path: PathBuf,
    segment_duration: i64,
    format: String,
    message_broker: Arc<Mutex<Option<Arc<crate::messaging::MessageBroker>>>>,
}

/// Represents an active recording session
struct ActiveRecording {
    recording_id: Uuid,
    schedule_id: Option<Uuid>,
    camera_id: Uuid,
    stream_id: Uuid,
    pipeline: gst::Pipeline,
    start_time: DateTime<Utc>,
    event_type: RecordingEventType,
    file_path: PathBuf,
    pipeline_watch_id: Option<gst::bus::BusWatchGuard>,
}

/// Recording pipeline status information
#[derive(Debug, Clone)]
pub struct RecordingStatus {
    pub recording_id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub duration: i64,
    pub file_size: u64,
    pub pipeline_state: String,
    pub fps: i32,
    pub error_count: i32,
    pub event_type: RecordingEventType,
}

impl RecordingManager {
    /// Create a new recording manager
    pub fn new(
        db_pool: Arc<PgPool>,
        stream_manager: Arc<StreamManager>,
        recording_base_path: &Path,
        segment_duration: i64,
        format: &str,
    ) -> Self {
        Self {
            stream_manager,
            recordings_repo: RecordingsRepository::new(db_pool),
            active_recordings: Mutex::new(HashMap::new()),
            recording_base_path: recording_base_path.to_owned(),
            segment_duration,
            format: format.to_owned(),
            message_broker: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Set message broker for event publishing
    pub async fn set_message_broker(&self, broker: Arc<crate::messaging::MessageBroker>) -> Result<()> {
        // Safely update the message broker through the mutex
        {
            let mut broker_guard = self.message_broker.lock().unwrap();
            *broker_guard = Some(broker.clone());
        }
        
        // Publish a startup event
        broker.publish(
            crate::messaging::EventType::SystemStartup, 
            None, 
            serde_json::json!({"component": "recording_manager"})
        ).await?;
        
        Ok(())
    }

    /// Start recording a stream
    pub async fn start_recording(
        &self,
        schedule: &RecordingSchedule,
        stream: &Stream,
    ) -> Result<Uuid> {
        let recording_key = format!("{}-{}", schedule.id, stream.id);

        // Check if already recording this stream for this schedule
        {
            let active_recordings = self.active_recordings.lock().unwrap();
            if active_recordings.contains_key(&recording_key) {
                return Err(anyhow!(
                    "Already recording stream {} for schedule {}",
                    stream.id,
                    schedule.id
                ));
            }
        }

        // Generate unique recording ID and create initial recording entry
        self.start_recording_with_type(stream, Some(schedule.id), RecordingEventType::Continuous)
            .await
    }

    /// Start manual recording for a stream
    pub async fn start_manual_recording(&self, stream: &Stream) -> Result<Uuid> {
        self.start_recording_with_type(
            stream,
            None, // No schedule
            RecordingEventType::Manual,
        )
        .await
    }

    /// Start event-triggered recording for a stream
    pub async fn start_event_recording(
        &self,
        stream: &Stream,
        event_type: RecordingEventType,
    ) -> Result<Uuid> {
        if event_type == RecordingEventType::Continuous || event_type == RecordingEventType::Manual
        {
            return Err(anyhow!("Invalid event type for event recording"));
        }

        self.start_recording_with_type(stream, None, event_type)
            .await
    }

    async fn start_recording_with_type(
        &self,
        stream: &Stream,
        schedule_id: Option<Uuid>,
        event_type: RecordingEventType,
    ) -> Result<Uuid> {
        // Generate a recording key
        let recording_key = match schedule_id {
            Some(id) => format!("{}-{}", id, stream.id),
            None => format!("{}-{}", event_type.to_string(), stream.id),
        };

        // Check if already recording this combination
        {
            let active_recordings = self.active_recordings.lock().unwrap();
            if active_recordings.contains_key(&recording_key) {
                return Err(anyhow!("Already recording stream {}", stream.id));
            }
        }

        // Generate unique recording ID and organization paths
        let recording_id = Uuid::new_v4();
        let now = Utc::now();

        // Create directory structure with date-based hierarchy: YYYY/MM/DD/HH
        let date_path = now.format("%Y/%m/%d/%H").to_string();
        let camera_path = format!("{}", stream.camera_id);
        let dir_path = self.recording_base_path.join(&camera_path).join(&date_path);

        // Create parent directories
        std::fs::create_dir_all(&dir_path)?;

        // Create filename with timestamp
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let file_name = format!("cam{}_{}.{}", stream.camera_id, timestamp, self.format);
        let file_path = dir_path.join(&file_name);

        // Get access to the stream
        let (pipeline, tee) = self
            .stream_manager
            .get_stream_access(&stream.id.to_string())
            .map_err(|e| {
                error!("Failed to get stream access: {}", e);
                anyhow!("Failed to get stream access: {}", e)
            })?;

        // Generate unique key for this recording
        let element_suffix = recording_id.to_string().replace("-", "");

        // Create GStreamer elements for recording
        let queue = gst::ElementFactory::make("queue")
            .name(format!("record_queue_{}", element_suffix))
            .property("max-size-buffers", 0u32)
            .property("max-size-time", 0u64)
            .property("max-size-bytes", 0u32)
            .build()?;

        let depay = gst::ElementFactory::make("rtph264depay")
            .name(format!("record_depay_{}", element_suffix))
            .build()?;

        let parse = gst::ElementFactory::make("h264parse")
            .name(format!("record_parse_{}", element_suffix))
            .property("config-interval", -1) // Insert SPS/PPS with each key frame
            .build()?;

        // Use splitmuxsink for segment-based recording
        let splitmuxsink = gst::ElementFactory::make("splitmuxsink")
            .name(format!("record_splitmuxsink_{}", element_suffix))
            .property(
                "location",
                format!(
                    "{}/segment_%05d.{}",
                    dir_path.to_str().unwrap(),
                    self.format
                ),
            )
            .property(
                "max-size-time",
                gst::ClockTime::from_seconds(self.segment_duration as u64),
            )
            .property("max-size-bytes", 0u64)
            .build()?;

        // Configure the muxer for fragmented MP4
        let muxer = gst::ElementFactory::make(if self.format == "mp4" {
            "mp4mux"
        } else {
            "matroskamux"
        })
        .name(format!("record_muxer_{}", element_suffix))
        .property("faststart", true) // Put MOOV atom at the beginning
        .property("streamable", true) // Make it seekable during recording
        .property("fragment-duration", 1000_u32) // 1 second fragments
        .build()?;

        // Set the muxer on the splitmuxsink
        splitmuxsink.set_property("muxer", &muxer);

        // Create a filesink for storing the video
        let sink = gst::ElementFactory::make("filesink")
            .name(format!("record_sink_{}", element_suffix))
            .property("location", file_path.to_str().unwrap())
            .property("sync", false)
            .property("async", false)
            .build()?;

        // Set the sink on the splitmuxsink
        splitmuxsink.set_property("sink", &sink);

        // Add elements to pipeline
        pipeline.add_many(&[&queue, &depay, &parse, &splitmuxsink])?;

        // Link GStreamer elements
        gst::Element::link_many(&[&queue, &depay, &parse, &splitmuxsink])?;

        // Connect to tee
        let tee_src_pad = tee
            .request_pad_simple("src_%u")
            .ok_or_else(|| anyhow!("Failed to get tee src pad"))?;

        let queue_sink_pad = queue
            .static_pad("sink")
            .ok_or_else(|| anyhow!("Failed to get queue sink pad"))?;

        // Link the tee to the queue
        tee_src_pad.link(&queue_sink_pad)?;

        // Sync all elements with the pipeline state
        for element in [&queue, &depay, &parse, &splitmuxsink] {
            element.sync_state_with_parent()?;
        }

        // Set up a bus watch for monitoring
        let pipeline_clone = pipeline.clone();
        let recording_id_clone = recording_id;
        let watch_id = pipeline
            .bus()
            .unwrap()
            .add_watch(move |_, msg| {
                match msg.view() {
                    gst::MessageView::Error(err) => {
                        error!(
                            "Error from recording pipeline {}: {} ({})",
                            recording_id_clone,
                            err.error(),
                            err.debug().unwrap_or_default()
                        );
                    }
                    gst::MessageView::Eos(_) => {
                        info!("End of stream for recording {}", recording_id_clone);
                    }
                    gst::MessageView::StateChanged(state) => {
                        if state.src().map(|s| s.name() == pipeline_clone.name()) == Some(true) {
                            info!(
                                "Pipeline state changed for recording {}: {:?} -> {:?}",
                                recording_id_clone,
                                state.old(),
                                state.current()
                            );
                        }
                    }
                    _ => (),
                }
                glib::ControlFlow::Continue
            })
            .expect("Failed to add bus watch");

        // Create recording entry in database
        let recording = Recording {
            id: recording_id,
            camera_id: stream.camera_id,
            stream_id: stream.id,
            start_time: now,
            end_time: None,
            file_path: file_path.clone(),
            file_size: 0,
            duration: 0,
            format: self.format.clone(),
            resolution: stream
                .resolution
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            fps: stream.framerate.unwrap_or(30) as u32,
            event_type,
            metadata: None,
            schedule_id,
        };

        // Store in database
        self.recordings_repo.create(&recording).await?;

        // Store active recording info
        let active_recording = ActiveRecording {
            recording_id,
            schedule_id,
            camera_id: stream.camera_id,
            stream_id: stream.id,
            pipeline: pipeline.clone(),
            start_time: now,
            event_type,
            file_path,
            pipeline_watch_id: Some(watch_id),
        };

        // Add to active recordings
        {
            let mut active_recordings = self.active_recordings.lock().unwrap();
            active_recordings.insert(recording_key, active_recording);
        }

        info!(
            "Started {} recording for camera {} with stream {}",
            event_type.to_string(),
            stream.camera_id,
            stream.id
        );
        
        // Publish recording started event
        if let Some(broker) = self.message_broker.lock().unwrap().as_ref() {
            if let Err(e) = broker.publish(
                crate::messaging::EventType::RecordingStarted,
                Some(stream.camera_id),
                serde_json::json!({
                    "recording_id": recording_id.to_string(),
                    "stream_id": stream.id.to_string(),
                    "event_type": event_type.to_string(),
                    "schedule_id": schedule_id.map(|id| id.to_string())
                })
            ).await {
                warn!("Failed to publish recording started event: {}", e);
            }
        }

        Ok(recording_id)
    }

    /// Stop recording a specific schedule
    pub async fn stop_recording(&self, schedule_id: &Uuid, stream_id: &Uuid) -> Result<()> {
        let recording_key = format!("{}-{}", schedule_id, stream_id);
        self.stop_recording_by_key(&recording_key).await
    }

    /// Stop a manual or event-triggered recording
    pub async fn stop_event_recording(
        &self,
        event_type: RecordingEventType,
        stream_id: &Uuid,
    ) -> Result<()> {
        let recording_key = format!("{}-{}", event_type.to_string(), stream_id);
        self.stop_recording_by_key(&recording_key).await
    }

    /// Stop recording by ID
    pub async fn stop_recording_by_id(&self, recording_id: &Uuid) -> Result<()> {
        // Find the recording key from the ID
        let recording_key = {
            let active_recordings = self.active_recordings.lock().unwrap();
            let mut key = None;

            for (k, recording) in active_recordings.iter() {
                if &recording.recording_id == recording_id {
                    key = Some(k.clone());
                    break;
                }
            }

            key.ok_or_else(|| anyhow!("Recording not found"))?
        };

        self.stop_recording_by_key(&recording_key).await
    }

    /// Internal method to stop recording by key
    async fn stop_recording_by_key(&self, recording_key: &str) -> Result<()> {
        // Get the active recording
        let active_recording = {
            let mut active_recordings = self.active_recordings.lock().unwrap();

            if !active_recordings.contains_key(recording_key) {
                return Err(anyhow!(
                    "No active recording found for key {}",
                    recording_key
                ));
            }

            active_recordings
                .remove(recording_key)
                .ok_or_else(|| anyhow!("Failed to remove active recording"))?
        };

        // Drop the pipeline watch guard to deregister it
        if let Some(watch_id) = active_recording.pipeline_watch_id {
            drop(watch_id);
        }

        // Send EOS to the pipeline
        // This will make splitmuxsink finalize properly
        let _ = active_recording.pipeline.send_event(gst::event::Eos::new());

        // Wait a moment for EOS to propagate
        sleep(Duration::from_millis(500)).await;

        // Stop the pipeline gracefully
        let _ = active_recording.pipeline.set_state(gst::State::Null);

        // Wait for file to be fully written
        sleep(Duration::from_secs(1)).await;

        // Get file info
        let metadata = match std::fs::metadata(&active_recording.file_path) {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to get file metadata: {}", e);
                // Continue with update even if file info isn't available
                return Ok(());
            }
        };

        let file_size = metadata.len();
        let duration = Utc::now()
            .signed_duration_since(active_recording.start_time)
            .num_seconds() as u64;

        // Update recording in database
        let mut recording = match self
            .recordings_repo
            .get_by_id(&active_recording.recording_id)
            .await
        {
            Ok(Some(rec)) => rec,
            Ok(None) => {
                error!("Recording not found in database");
                return Ok(());
            }
            Err(e) => {
                error!("Failed to get recording from database: {}", e);
                return Ok(());
            }
        };

        recording.end_time = Some(Utc::now());
        recording.file_size = file_size;
        recording.duration = duration;

        if let Err(e) = self.recordings_repo.update(&recording).await {
            error!("Failed to update recording in database: {}", e);
        }

        info!(
            "Stopped recording {} for camera {}",
            active_recording.recording_id, active_recording.camera_id
        );
        
        // Publish recording stopped event
        if let Some(broker) = self.message_broker.lock().unwrap().as_ref() {
            if let Err(e) = broker.publish(
                crate::messaging::EventType::RecordingStopped,
                Some(active_recording.camera_id),
                serde_json::json!({
                    "recording_id": active_recording.recording_id.to_string(),
                    "stream_id": active_recording.stream_id.to_string(),
                    "duration_seconds": duration,
                    "file_size_bytes": file_size,
                    "event_type": active_recording.event_type.to_string(),
                    "schedule_id": active_recording.schedule_id.map(|id| id.to_string())
                })
            ).await {
                warn!("Failed to publish recording stopped event: {}", e);
            }
        }

        Ok(())
    }

    /// Stop all active recordings
    pub async fn stop_all_recordings(&self) -> Result<()> {
        // Get all active recordings
        let active_recordings = {
            let active_recordings = self.active_recordings.lock().unwrap();
            active_recordings.keys().cloned().collect::<Vec<_>>()
        };

        // Stop each recording
        for key in active_recordings {
            let _ = self.stop_recording_by_key(&key).await;
        }

        Ok(())
    }

    /// Check if a recording is currently active for schedule and stream
    pub fn is_recording_active(&self, schedule_id: &Uuid, stream_id: &Uuid) -> bool {
        let recording_key = format!("{}-{}", schedule_id, stream_id);
        let active_recordings = self.active_recordings.lock().unwrap();
        active_recordings.contains_key(&recording_key)
    }

    /// Check if any recording is active for a stream
    pub fn is_stream_recording(&self, stream_id: &Uuid) -> bool {
        let active_recordings = self.active_recordings.lock().unwrap();
        active_recordings
            .values()
            .any(|r| &r.stream_id == stream_id)
    }

    /// Get status of all active recordings
    pub fn get_recording_status(&self) -> Vec<RecordingStatus> {
        let active_recordings = self.active_recordings.lock().unwrap();

        active_recordings
            .values()
            .map(|recording| {
                // Get pipeline state
                let state = recording.pipeline.state(None);
                let state_str = format!("{:?}", state.1);

                // Get file size if possible
                let file_size = std::fs::metadata(&recording.file_path)
                    .map(|m| m.len())
                    .unwrap_or(0);

                // Calculate current duration
                let duration = Utc::now()
                    .signed_duration_since(recording.start_time)
                    .num_seconds();

                RecordingStatus {
                    recording_id: recording.recording_id,
                    camera_id: recording.camera_id,
                    stream_id: recording.stream_id,
                    start_time: recording.start_time,
                    duration,
                    file_size,
                    pipeline_state: state_str,
                    fps: 0,         // Not available from pipeline
                    error_count: 0, // Not tracked currently
                    event_type: recording.event_type,
                }
            })
            .collect()
    }

    /// Get status of a specific recording
    pub fn get_recording_status_by_id(&self, recording_id: &Uuid) -> Option<RecordingStatus> {
        let active_recordings = self.active_recordings.lock().unwrap();

        active_recordings
            .values()
            .find(|r| &r.recording_id == recording_id)
            .map(|recording| {
                // Get pipeline state
                let state = recording.pipeline.state(None);
                let state_str = format!("{:?}", state.1);

                // Get file size if possible
                let file_size = std::fs::metadata(&recording.file_path)
                    .map(|m| m.len())
                    .unwrap_or(0);

                // Calculate current duration
                let duration = Utc::now()
                    .signed_duration_since(recording.start_time)
                    .num_seconds();

                RecordingStatus {
                    recording_id: recording.recording_id,
                    camera_id: recording.camera_id,
                    stream_id: recording.stream_id,
                    start_time: recording.start_time,
                    duration,
                    file_size,
                    pipeline_state: state_str,
                    fps: 0,
                    error_count: 0,
                    event_type: recording.event_type,
                }
            })
    }
}
