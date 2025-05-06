use crate::db::models::recording_models::{Recording, RecordingEventType};
use crate::db::repositories::recordings::RecordingsRepository;
use anyhow::{anyhow, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use log::{debug, error, info, warn};
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

/// Service that prepares HLS streams from existing MP4 recordings
pub struct HlsPreparationService {
    recordings_repo: RecordingsRepository,
    base_hls_path: PathBuf,
    active_preparations: Arc<Mutex<HashMap<String, HlsPreparationStatus>>>,
    // Channel for queueing preparation requests
    prep_tx: mpsc::Sender<HlsPreparationRequest>,
}

struct HlsPreparationStatus {
    camera_id: Uuid,
    start_time: chrono::DateTime<chrono::Utc>,
    status: String,
    hls_directory: PathBuf,
    // Store the latest segment timestamp for resuming
    last_segment_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

enum HlsPreparationRequest {
    PrepareCamera { camera_id: Uuid, max_age_days: u32 },
    PrepareRecording { recording_id: Uuid },
}

impl HlsPreparationService {
    /// Create a new HLS preparation service
    pub fn new(db_pool: Arc<PgPool>, hls_output_path: &Path) -> Self {
        // Create the HLS directory if it doesn't exist
        if !hls_output_path.exists() {
            std::fs::create_dir_all(hls_output_path)
                .expect("Failed to create HLS output directory");
        }

        // Create a channel for preparation requests
        let (prep_tx, mut prep_rx) = mpsc::channel::<HlsPreparationRequest>(100);

        let recordings_repo = RecordingsRepository::new(Arc::clone(&db_pool));
        let base_hls_path = hls_output_path.to_owned();
        let active_preparations = Arc::new(Mutex::new(HashMap::new()));

        // Clone values needed for the worker task
        let recordings_repo_clone = recordings_repo.clone();
        let base_hls_path_clone = base_hls_path.clone();
        let active_preparations_clone = Arc::clone(&active_preparations);

        // Spawn a worker task to process HLS preparation requests in the background
        tokio::spawn(async move {
            info!("HLS preparation service worker started");

            while let Some(request) = prep_rx.recv().await {
                match request {
                    HlsPreparationRequest::PrepareCamera {
                        camera_id,
                        max_age_days,
                    } => {
                        if let Err(e) = Self::prepare_camera_hls(
                            &recordings_repo_clone,
                            &base_hls_path_clone,
                            &active_preparations_clone,
                            camera_id,
                            max_age_days,
                        )
                        .await
                        {
                            error!("Error preparing HLS for camera {}: {}", camera_id, e);
                        }
                    }
                    HlsPreparationRequest::PrepareRecording { recording_id } => {
                        if let Err(e) = Self::prepare_recording_hls(
                            &recordings_repo_clone,
                            &base_hls_path_clone,
                            &active_preparations_clone,
                            recording_id,
                        )
                        .await
                        {
                            error!("Error preparing HLS for recording {}: {}", recording_id, e);
                        }
                    }
                }
            }

            error!("HLS preparation service worker channel closed");
        });

        Self {
            recordings_repo,
            base_hls_path,
            active_preparations,
            prep_tx,
        }
    }

    /// Queue a request to prepare HLS streams for all recordings of a camera
    pub async fn queue_camera_preparation(&self, camera_id: Uuid, max_age_days: u32) -> Result<()> {
        self.prep_tx
            .send(HlsPreparationRequest::PrepareCamera {
                camera_id,
                max_age_days,
            })
            .await
            .map_err(|e| anyhow!("Failed to queue camera preparation: {}", e))
    }

    /// Queue a request to prepare HLS stream for a specific recording
    pub async fn queue_recording_preparation(&self, recording_id: Uuid) -> Result<()> {
        self.prep_tx
            .send(HlsPreparationRequest::PrepareRecording { recording_id })
            .await
            .map_err(|e| anyhow!("Failed to queue recording preparation: {}", e))
    }

    /// Prepare HLS streams for all recordings of a camera
    async fn prepare_camera_hls(
        recordings_repo: &RecordingsRepository,
        base_hls_path: &Path,
        active_preparations: &Arc<Mutex<HashMap<String, HlsPreparationStatus>>>,
        camera_id: Uuid,
        max_age_days: u32,
    ) -> Result<()> {
        // Create a preparation key for tracking
        let prep_key = format!("camera-{}", camera_id);

        // Check if we're already preparing this camera
        {
            let active_preps = active_preparations.lock().await;
            if active_preps.contains_key(&prep_key) {
                info!(
                    "HLS preparation for camera {} is already in progress",
                    camera_id
                );
                return Ok(());
            }
        }

        // Register the preparation
        {
            let mut active_preps = active_preparations.lock().await;
            let hls_dir = base_hls_path.join("cameras").join(camera_id.to_string());

            // Create the HLS directory
            fs::create_dir_all(&hls_dir)?;

            active_preps.insert(
                prep_key.clone(),
                HlsPreparationStatus {
                    camera_id,
                    start_time: chrono::Utc::now(),
                    status: "in_progress".to_string(),
                    hls_directory: hls_dir,
                    last_segment_timestamp: None,
                },
            );
        }

        // Get recordings for this camera within the time range that have an end_time
        let max_age = chrono::Duration::days(max_age_days as i64);
        let start_time = chrono::Utc::now() - max_age;

        let query = crate::db::models::recording_models::RecordingSearchQuery {
            camera_ids: Some(vec![camera_id]),
            stream_ids: None,
            start_time: Some(start_time),
            end_time: None, // Get all recordings up to now
            event_types: None,
            schedule_id: None,
            min_duration: Some(1), // Exclude 0-duration recordings
            segment_id: None,
            parent_recording_id: None,
            is_segment: None, // Get all recordings regardless of segment status
            limit: Some(1000),
            offset: Some(0),
        };

        let all_recordings = recordings_repo.search(&query).await?;
        
        // Filter recordings to only include those that have an end_time
        let recordings: Vec<_> = all_recordings
            .into_iter()
            .filter(|r| r.end_time.is_some())
            .collect();

        // Update status with recording count
        {
            let mut active_preps = active_preparations.lock().await;
            if let Some(status) = active_preps.get_mut(&prep_key) {
                status.status = format!("Processing {} recordings", recordings.len());
            }
        }
        
        info!("Found {} recordings with end_time for camera {}", recordings.len(), camera_id);

        // Process each recording individually
        for recording in recordings {
            // Prepare HLS for this recording directly - no need to check segments
            let recording_id = recording.id;
            
            // Each recording is self-contained, so we can process it directly
            if let Err(e) = Self::prepare_recording_hls_internal(
                recordings_repo,
                base_hls_path,
                active_preparations,
                &recording,
                &[recording.clone()], // Use the recording itself as its own "segment"
            )
            .await
            {
                error!("Error preparing HLS for recording {}: {}", recording_id, e);
                // Continue to the next recording
                continue;
            }
        }

        // Update status to completed
        {
            let mut active_preps = active_preparations.lock().await;
            active_preps.remove(&prep_key);
        }

        info!("Completed HLS preparation for camera {}", camera_id);
        Ok(())
    }

    /// Prepare HLS stream for a specific recording
    async fn prepare_recording_hls(
        recordings_repo: &RecordingsRepository,
        base_hls_path: &Path,
        active_preparations: &Arc<Mutex<HashMap<String, HlsPreparationStatus>>>,
        recording_id: Uuid,
    ) -> Result<()> {
        // Get the recording details
        let recording = match recordings_repo.get_by_id(&recording_id).await? {
            Some(r) => r,
            None => return Err(anyhow!("Recording not found: {}", recording_id)),
        };

        // Check if we're already preparing this recording
        let prep_key = format!("recording-{}", recording_id);
        {
            let active_preps = active_preparations.lock().await;
            if active_preps.contains_key(&prep_key) {
                info!(
                    "HLS preparation for recording {} is already in progress",
                    recording_id
                );
                return Ok(());
            }
        }

        // Make sure the recording has an end_time
        if recording.end_time.is_none() {
            return Err(anyhow!("Recording {} is still in progress and doesn't have an end_time. Cannot prepare HLS yet.", recording_id));
        }

        // Prepare this recording's HLS - use the recording itself as its own "segment"
        Self::prepare_recording_hls_internal(
            recordings_repo,
            base_hls_path,
            active_preparations,
            &recording,
            &[recording.clone()], // Use the recording itself as its own "segment"
        )
        .await
    }

    /// Internal method to prepare HLS for a recording (using the recording as its own segment)
    async fn prepare_recording_hls_internal(
        _recordings_repo: &RecordingsRepository,
        base_hls_path: &Path,
        active_preparations: &Arc<Mutex<HashMap<String, HlsPreparationStatus>>>,
        recording: &Recording,
        recordings: &[Recording],
    ) -> Result<()> {
        // Create a preparation key for tracking
        let recording_id = recording.id;
        let prep_key = format!("recording-{}", recording_id);

        // Register the preparation
        {
            let mut active_preps = active_preparations.lock().await;
            let hls_dir = base_hls_path
                .join("recordings")
                .join(recording_id.to_string());

            // Create the HLS directory
            fs::create_dir_all(&hls_dir)?;

            active_preps.insert(
                prep_key.clone(),
                HlsPreparationStatus {
                    camera_id: recording.camera_id,
                    start_time: chrono::Utc::now(),
                    status: "in_progress".to_string(),
                    hls_directory: hls_dir.clone(),
                    last_segment_timestamp: None,
                },
            );
        }

        // Get the HLS directory path
        let hls_dir = base_hls_path
            .join("recordings")
            .join(recording_id.to_string());

        // Filter for recordings that exist and have an end_time
        let valid_recordings: Vec<&Recording> = recordings
            .iter()
            .filter(|r| r.file_path.exists() && r.end_time.is_some())
            .collect();

        if valid_recordings.is_empty() {
            // Remove this preparation
            {
                let mut active_preps = active_preparations.lock().await;
                active_preps.remove(&prep_key);
            }
            return Err(anyhow!(
                "No valid recordings found for ID {}",
                recording_id
            ));
        }

        // Update status
        {
            let mut active_preps = active_preparations.lock().await;
            if let Some(status) = active_preps.get_mut(&prep_key) {
                status.status = format!("Processing recording {}", recording_id);
            }
        }

        // Create a pipeline to convert the recording to HLS
        Self::create_hls_from_segments(&valid_recordings, &hls_dir, &prep_key, active_preparations)
            .await?;

        // Once the HLS conversion is complete, also update the camera's HLS directory
        // This ensures that the camera's HLS content is always up to date
        let camera_id = recording.camera_id;
        let camera_hls_dir = base_hls_path.join("cameras").join(camera_id.to_string());
        
        // Make sure the camera directory exists
        if !camera_hls_dir.exists() {
            fs::create_dir_all(&camera_hls_dir)?;
        }
        
        // Copy the recording's master playlist to the camera directory
        let recording_master = hls_dir.join("master.m3u8");
        let camera_master = camera_hls_dir.join("master.m3u8");
        
        if recording_master.exists() {
            // For camera directory, we'll create a new master playlist that references
            // all processed recordings
            if !camera_master.exists() {
                // Create initial master playlist for the camera
                let master_content = format!(
                    "#EXTM3U\n\
                    #EXT-X-VERSION:7\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
                    /playback/cameras/{}/hls?playlist_type=variant\n",
                    camera_id
                );
                fs::write(&camera_master, master_content)?;
            }
        }

        // Update status to completed
        {
            let mut active_preps = active_preparations.lock().await;
            active_preps.remove(&prep_key);
        }

        info!("Completed HLS preparation for recording {}", recording_id);
        Ok(())
    }

    /// Create HLS playlists from MP4 segments using GStreamer
    async fn create_hls_from_segments(
        segments: &[&Recording],
        hls_dir: &Path,
        prep_key: &str,
        active_preparations: &Arc<Mutex<HashMap<String, HlsPreparationStatus>>>,
    ) -> Result<()> {
        // Ensure GStreamer is initialized
        if let Err(e) = gst::init() {
            return Err(anyhow!("Failed to initialize GStreamer: {}", e));
        }

        // Create a list of file paths for the segments
        let segment_paths: Vec<String> = segments
            .iter()
            .map(|s| s.file_path.to_string_lossy().to_string())
            .collect();

        // Create the pipeline in a separate thread
        let (ready_tx, mut ready_rx) = mpsc::channel::<Result<gst::Pipeline>>(1);
        let hls_dir_clone = hls_dir.to_path_buf();
        let segment_paths_clone = segment_paths.clone();

        // Create the pipeline in a separate thread because GStreamer API is not async
        std::thread::spawn(move || {
            let result = || -> Result<gst::Pipeline> {
                // Create main pipeline
                let pipeline = gst::Pipeline::new();

                // Create a splitmuxsrc to read the MP4 files
                let src = gst::ElementFactory::make("splitmuxsrc")
                    .name("hls_src")
                    .build()
                    .map_err(|e| anyhow!("Failed to create splitmuxsrc: {:?}", e))?;

                // Set location of the first segment - we'll handle the rest programmatically
                src.set_property("location", &segment_paths_clone[0]);

                // Create queue for buffering
                let queue = gst::ElementFactory::make("queue")
                    .name("hls_queue")
                    .property("max-size-buffers", 1000u32)
                    .property("max-size-time", 0u64)
                    .property("max-size-bytes", 0u32)
                    .build()
                    .map_err(|e| anyhow!("Failed to create queue: {:?}", e))?;

                // Create hlssink2 element
                let sink = gst::ElementFactory::make("hlssink2")
                    .name("hls_sink")
                    .property(
                        "playlist-location",
                        &hls_dir_clone
                            .join("playlist.m3u8")
                            .to_string_lossy()
                            .to_string(),
                    )
                    .property(
                        "location",
                        &hls_dir_clone
                            .join("segment_%05d.ts")
                            .to_string_lossy()
                            .to_string(),
                    )
                    .property("playlist-length", 0u32) // All segments in playlist (infinite)
                    .property("target-duration", 4u32) // 4-second target
                    .property("max-files", 0u32) // Keep all files
                    // The "playlist-type" property is not supported in this version of hlssink2
                    .build()
                    .map_err(|e| anyhow!("Failed to create hlssink2: {:?}", e))?;

                // Add all elements to the pipeline
                pipeline
                    .add_many(&[&src, &queue, &sink])
                    .map_err(|e| anyhow!("Failed to add elements to pipeline: {}", e))?;

                // Create video conversion elements to ensure proper format for HLS
                let videoconvert = gst::ElementFactory::make("videoconvert")
                    .name("video_convert")
                    .build()
                    .map_err(|e| anyhow!("Failed to create videoconvert: {:?}", e))?;
                
                // Try x264enc first, fall back to platform-specific encoders if not available
                let h264enc = match gst::ElementFactory::make("x264enc").name("h264_encoder").build() {
                    Ok(enc) => {
                        // Configure encoder for low-latency
                        enc.set_property("tune", "zerolatency");
                        enc.set_property("speed-preset", "superfast");
                        enc
                    },
                    Err(_) => {
                        // Try alternative encoders based on platform
                        match gst::ElementFactory::make("avenc_h264").name("h264_encoder").build() {
                            Ok(enc) => {
                                debug!("Using avenc_h264 instead of x264enc");
                                enc
                            },
                            Err(_) => {
                                // Try one more fallback - nvenc on systems with NVIDIA
                                match gst::ElementFactory::make("nvh264enc").name("h264_encoder").build() {
                                    Ok(enc) => {
                                        debug!("Using nvh264enc as fallback encoder");
                                        enc
                                    },
                                    Err(e) => return Err(anyhow!("Could not find a suitable H.264 encoder: {:?}", e)),
                                }
                            }
                        }
                    }
                };
                
                let h264parse = gst::ElementFactory::make("h264parse")
                    .name("h264_parser")
                    .build()
                    .map_err(|e| anyhow!("Failed to create h264parse: {:?}", e))?;
                
                // Add conversion elements to the pipeline
                pipeline
                    .add_many(&[&videoconvert, &h264enc, &h264parse])
                    .map_err(|e| anyhow!("Failed to add conversion elements to pipeline: {}", e))?;
                
                // Link the conversion elements
                gst::Element::link_many(&[&queue, &videoconvert, &h264enc, &h264parse, &sink])
                    .map_err(|e| anyhow!("Failed to link conversion elements: {}", e))?;

                // We can't directly link splitmuxsrc since it has dynamic pads
                // Instead, set up a pad-added handler to link when the pads become available
                let queue_weak = queue.downgrade();
                src.connect_pad_added(move |_, src_pad| {
                    let queue = match queue_weak.upgrade() {
                        Some(queue) => queue,
                        None => return,
                    };

                    // Only connect video streams for now
                    if src_pad.name().starts_with("video") {
                        debug!("Linking video pad: {}", src_pad.name());
                        let sink_pad = queue.static_pad("sink").expect("Failed to get sink pad from queue");
                        
                        // Attempt to link the pads
                        if let Err(err) = src_pad.link(&sink_pad) {
                            error!("Failed to link pads: {}", err);
                        }
                    }
                });

                Ok(pipeline)
            }();

            // Send the result back to the async task
            let _ = ready_tx.blocking_send(result);
        });

        // Wait for the pipeline to be created
        let pipeline = match ready_rx.recv().await {
            Some(Ok(p)) => p,
            Some(Err(e)) => return Err(anyhow!("Failed to create GStreamer pipeline: {}", e)),
            None => return Err(anyhow!("Pipeline creation thread exited unexpectedly")),
        };

        // Update status
        {
            let mut active_preps = active_preparations.lock().await;
            if let Some(status) = active_preps.get_mut(prep_key) {
                status.status = "Starting HLS conversion pipeline".to_string();
            }
        }

        // Create message handling channel
        let (msg_tx, mut msg_rx) = mpsc::channel::<gst::Message>(100);

        // Start the pipeline
        let pipeline_clone = pipeline.clone();
        let start_result = pipeline.set_state(gst::State::Playing);

        if let Err(e) = start_result {
            pipeline.set_state(gst::State::Null)?;
            return Err(anyhow!("Failed to start pipeline: {}", e));
        }

        // Set up bus watch in a separate thread
        let msg_tx_clone = msg_tx.clone();
        let _bus_watch_thread = std::thread::spawn(move || {
            let bus = pipeline_clone.bus().unwrap();
            for msg in bus.iter() {
                if let Err(_) = msg_tx_clone.blocking_send(msg) {
                    // Channel closed, exit thread
                    break;
                }
            }
        });

        // Track progress
        let mut segment_index = 0;
        let total_segments = segments.len();

        // Process bus messages
        while let Some(msg) = msg_rx.recv().await {
            match msg.view() {
                gst::MessageView::Eos(_) => {
                    info!("End of stream, HLS conversion complete");
                    break;
                }
                gst::MessageView::Error(err) => {
                    error!(
                        "Error from {}: {}",
                        err.src()
                            .map(|s| s.name())
                            .unwrap_or_else(|| "unknown".into()),
                        err.error()
                    );

                    // If we're not at the end, try continuing with the next segment
                    if segment_index < total_segments - 1 {
                        segment_index += 1;

                        // Get src element and set next location
                        if let Some(src) = pipeline.by_name("hls_src") {
                            src.set_property("location", &segment_paths[segment_index]);

                            // Update status
                            {
                                let mut active_preps = active_preparations.lock().await;
                                if let Some(status) = active_preps.get_mut(prep_key) {
                                    status.status = format!(
                                        "Processing segment {}/{}",
                                        segment_index + 1,
                                        total_segments
                                    );
                                }
                            }

                            continue;
                        }
                    }

                    // If we can't continue, stop the pipeline
                    pipeline.set_state(gst::State::Null)?;
                    return Err(anyhow!("Pipeline error: {}", err.error()));
                }
                gst::MessageView::StateChanged(state_changed) => {
                    // Check if this state change is for our pipeline
                    if let Some(src) = state_changed.src() {
                        if src.name() == pipeline.name() {
                            let (old, new, _) = (
                                state_changed.old(),
                                state_changed.current(),
                                state_changed.pending(),
                            );
                            debug!("Pipeline state changed from {:?} to {:?}", old, new);
                        }
                    }
                }
                _ => {}
            }
        }

        // Clean up
        pipeline.set_state(gst::State::Null)?;

        // Final step: create a master playlist if it doesn't exist
        let master_playlist_path = hls_dir.join("master.m3u8");
        if !master_playlist_path.exists() {
            let master_content = format!(
                "#EXTM3U\n\
                #EXT-X-VERSION:7\n\
                #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
                playlist.m3u8\n"
            );

            fs::write(&master_playlist_path, master_content)?;
        }

        Ok(())
    }

    /// Get the HLS directory for a specific recording
    pub fn get_hls_dir_for_recording(&self, recording_id: &Uuid) -> PathBuf {
        self.base_hls_path
            .join("recordings")
            .join(recording_id.to_string())
    }

    /// Get the HLS directory for a specific camera
    pub fn get_hls_dir_for_camera(&self, camera_id: &Uuid) -> PathBuf {
        self.base_hls_path
            .join("cameras")
            .join(camera_id.to_string())
    }

    /// Check if HLS content is available for a recording
    pub async fn is_hls_available_for_recording(&self, recording_id: &Uuid) -> bool {
        let hls_dir = self.get_hls_dir_for_recording(recording_id);
        let master_playlist = hls_dir.join("master.m3u8");
        master_playlist.exists()
    }

    /// Check if HLS content is available for a camera
    pub async fn is_hls_available_for_camera(&self, camera_id: &Uuid) -> bool {
        let hls_dir = self.get_hls_dir_for_camera(camera_id);
        let master_playlist = hls_dir.join("master.m3u8");
        master_playlist.exists()
    }
}

