use crate::db::models::recording_models::{
    Recording, RecordingDb, RecordingEventType, RecordingUpdate,
};
use crate::db::models::recording_schedule_models::RecordingSchedule;
use crate::db::models::stream_models::Stream;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::messaging::broker::MessageBrokerTrait;
use crate::stream_manager::StreamManager;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_pbutils::{
    self, prelude::DiscovererStreamInfoExt, Discoverer, DiscovererInfo, DiscovererStreamInfo,
};
use log::{error, info, trace, warn};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
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
    segment_id: Option<u32>,
    parent_recording_id: Option<Uuid>,
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
    pub segment_id: Option<u32>,
    pub parent_recording_id: Option<Uuid>,
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
                serde_json::json!({"component": "recording_manager"}),
            )
            .await?;

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
            let active_recordings = self.active_recordings.lock().await;
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
            let active_recordings = self.active_recordings.lock().await;
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

        // Connect to format-location-full signal to get notified when a new segment is created
        // This allows us to create a database entry for each segment
        let recording_id_clone = recording_id;
        let stream_clone = stream.clone();
        let format_clone = self.format.clone();
        let event_type_clone = event_type;
        let schedule_id_clone = schedule_id;
        let recordings_repo_clone = self.recordings_repo.clone();
        let start_time_clone = now;
        let segment_duration_clone = self.segment_duration;

        // Instead of using format-location-full, use the regular format-location signal
        // which is more widely supported and compatible
        let dir_path_clone = dir_path.clone();

splitmuxsink.connect("format-location-full", false, move |args| {
    // This is called when a new segment file is about to be created
    if args.len() < 3 {
        warn!(
            "format-location-full signal has fewer than expected arguments: {}",
            args.len()
        );
        let fallback_name = format!(
            "{}/segment_emergency.{}",
            dir_path_clone.to_str().unwrap(),
            format_clone
        );
        return Some(fallback_name.to_value());
    }

    // Log argument details for debugging
    info!("format-location-full signal: got {} args", args.len());
    for (i, arg) in args.iter().enumerate() {
        info!("  Arg {}: type = {}", i, arg.type_().name());
    }

    // Get the fragment ID (index)
    let fragment_id = match args[1].get::<u32>() {
        Ok(id) => id,
        Err(e) => {
            warn!("Failed to get fragment ID: {}", e);
            0 // Default to 0 if we can't get the ID
        }
    };
    let now = Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();

    // Extract the GstSample containing the first buffer that will go into this file
    let first_sample = match args[2].get::<gst::Sample>() {
        Ok(sample) => sample,
        Err(e) => {
            warn!("Failed to get first sample: {}", e);
            // Continue with less information
            let segment_filename = format!(
                "{}/{}.{}",
                dir_path_clone.to_str().unwrap(),
                timestamp,
                format_clone
            );
            info!("Generated segment filename (without sample data): {}", segment_filename);
            return Some(segment_filename.to_value());
        }
    };

    // Get buffer from sample
    let buffer = match first_sample.buffer() {
        Some(buf) => buf,
        None => {
            warn!("No buffer in first sample");
            let segment_filename = format!(
                "{}/{}.{}",
                dir_path_clone.to_str().unwrap(),
                timestamp,
                format_clone
            );
            return Some(segment_filename.to_value());
        }
    };

    // Extract timing information from the buffer
    let pts = buffer.pts();
    let dts = buffer.dts();
    let duration = buffer.duration();
    
    // Get caps information (format, resolution, etc.)
// Get caps information (format, resolution, etc.)
// Get caps information (format, resolution, etc.)
let caps = match first_sample.caps() {
    Some(c) => c,
    None => {
        warn!("No caps in first sample");
        // Create a basic empty caps
        &gst::Caps::new_empty()
    }
};

    // Extract video-specific information from caps
    let caps_str = caps.to_string();
    let mut width = 0;
    let mut height = 0;
    let mut framerate_num = 0;
    let mut framerate_den = 1;
    let mut mime_type = "unknown";
    
    if let Some(structure) = caps.structure(0) {
        mime_type = structure.name();
        width = structure.get::<i32>("width").unwrap_or(0);
        height = structure.get::<i32>("height").unwrap_or(0);
        
if let Ok(fraction) = structure.get::<gst::Fraction>("framerate") {
        framerate_num = fraction.numer();
        framerate_den = fraction.denom();
    }
    }

    info!("Fragment ID: {}, PTS: {:?}, Width: {}, Height: {}", 
          fragment_id, pts, width, height);

    // Create a filename using our own format
    let segment_filename = format!(
        "{}/{}.{}",
        dir_path_clone.to_str().unwrap(),
        timestamp,
        format_clone
    );

    info!("Generated segment filename: {}", segment_filename);

    // Create a unique ID for this segment
    let segment_id = Uuid::new_v4();

    // Calculate precise segment start time using PTS if available
    let segment_start = if let Some(pts_time) = pts {
        // Convert PTS to milliseconds and add to the base start time
        let pts_ms = pts_time.mseconds();
        start_time_clone + chrono::Duration::milliseconds(pts_ms as i64)
    } else {
        // Fall back to our estimate based on fragment ID
        start_time_clone + chrono::Duration::seconds((fragment_id as i64) * segment_duration_clone)
    };

    // Calculate actual frame rate from caps if available
    let fps = if framerate_num > 0 && framerate_den > 0 {
        (framerate_num as f64 / framerate_den as f64) as u32
    } else {
        stream_clone.framerate.unwrap_or(30) as u32
    };

    // Create segment metadata with detailed information from sample
    let segment_metadata = serde_json::json!({
        "status": "processing",
        "finalized": false,
        "segment_type": "time_based",
        "creation_time": chrono::Utc::now().to_rfc3339(),
        "video_info": {
            "mime_type": mime_type,
            "width": width,
            "height": height,
            "framerate_num": framerate_num,
            "framerate_den": framerate_den,
            "has_pts": pts.is_some(),
            "has_dts": dts.is_some(),
            "buffer_duration_ns": duration.map(|d| d.nseconds()).unwrap_or(0),
            "caps_string": caps_str,
        }
    });

    // Create a segment recording entry with more accurate information
    let resolution = if width > 0 && height > 0 {
        format!("{}x{}", width, height)
    } else {
        stream_clone.resolution.clone().unwrap_or_else(|| "unknown".to_string())
    };

    let segment_recording = Recording {
        id: segment_id,
        camera_id: stream_clone.camera_id,
        stream_id: stream_clone.id,
        start_time: segment_start,
        end_time: None,
        file_path: std::path::PathBuf::from(&segment_filename),
        file_size: 0,
        duration: 0,
        format: format_clone.clone(),
        resolution,
        fps,
        event_type: event_type_clone,
        metadata: Some(segment_metadata),
        schedule_id: schedule_id_clone,
        segment_id: Some(fragment_id), // Store the segment ID directly
        parent_recording_id: Some(recording_id_clone), // Store parent recording ID directly
    };

    // We can't use tokio::spawn directly in the GStreamer thread as it's not inside the Tokio runtime
    // Instead, we'll use a std::thread to bridge between GStreamer and Tokio
    let recording_clone = segment_recording.clone();
    let recordings_repo = recordings_repo_clone.clone();
    let segment_id_clone = segment_id;
    let fragment_id_clone = fragment_id;

    // Create the initial database entry immediately
    std::thread::spawn(move || {
        // Create a simple runtime for this operation
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");

        // Run the async operation to create the recording entry
        rt.block_on(async {
            match recordings_repo.create(&recording_clone).await {
                Ok(_) => {
                    info!(
                        "Created database entry for segment {}: {}",
                        fragment_id_clone, segment_id_clone
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to create database entry for segment {}: {}",
                        fragment_id_clone, e
                    );
                }
            }
        });
    });

    Some(segment_filename.to_value())
});
        // Don't unwrap the signal handler ID - it's already properly connected

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

        // Set pipeline to Playing state
        pipeline.set_state(gst::State::Playing)?;
        info!(
            "Set recording pipeline to Playing state for recording {}",
            recording_id
        );

let bus = pipeline.bus().unwrap();
let recordings_repo_clone = self.recordings_repo.clone();
let recording_id_clone = recording_id;

let watch_id = bus.add_watch(move |_, msg| {
    match msg.view() {
        gst::MessageView::Element(element_msg) => {
            if let Some(structure) = element_msg.structure() {
                let name = structure.name();
                
                if name == "splitmuxsink-fragment-closed" {
                    // Log the complete structure for debugging
                    // info!("Fragment closed message: {:?}", structure);
                    
                    // Extract values from the message
                    if let (Ok(fragment_id), Ok(location), Ok(fragment_duration)) = (
                        structure.get::<u32>("fragment-id"),
                        structure.get::<String>("location"),
                        structure.get::<u64>("fragment-duration")
                    ) {
                        // info!(
                        //     "Fragment {} closed: location={}, duration={}ns", 
                        //     fragment_id, location, fragment_duration
                        // );
                        
                        // Convert duration from nanoseconds to milliseconds for database
                        let duration_ms = fragment_duration / 1_000_000;
                        
                        // Get the file size
                        let file_size = match std::fs::metadata(&location) {
                            Ok(metadata) => metadata.len(),
                            Err(e) => {
                                warn!("Failed to get file size for {}: {}", location, e);
                                0
                            }
                        };
                        
                        // Update the segment recording in the database
                        // We need to use a thread to bridge between GStreamer and Tokio
                        let recordings_repo = recordings_repo_clone.clone();
                        let recording_id = recording_id_clone;
                        
                        std::thread::spawn(move || {
                            // Create a runtime for this thread
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                                .expect("Failed to create Tokio runtime");
                            
                            rt.block_on(async {
                                // First, fetch the recording entry by parent_recording_id and segment_id
                                match recordings_repo.get_segment(&location).await {
                                    Ok(Some(mut recording)) => {
                                        // Update the recording with final values
                                        recording.file_size = file_size;
                                        recording.duration = duration_ms;
                                        
                                        // Update the metadata to mark it as finalized
                                        if let Some(metadata) = recording.metadata {
                                            let mut metadata_map = metadata.as_object().unwrap().clone();
                                            metadata_map.insert("status".to_string(), json!("completed"));
                                            metadata_map.insert("finalized".to_string(), json!(true));
                                            metadata_map.insert("finalize_time".to_string(), 
                                                json!(chrono::Utc::now().to_rfc3339()));
                                            metadata_map.insert("fragment_duration_ns".to_string(), 
                                                json!(fragment_duration));
                                            
                                            recording.metadata = Some(json!(metadata_map));
                                        }
                                        
                                        // Calculate end time based on fragment duration
                                            recording.end_time = Some(
                                                recording.start_time + chrono::Duration::milliseconds(duration_ms as i64)
                                            );
                                        
                                        
                                        // Update the recording in the database
                                        if let Err(e) = recordings_repo.update(&recording).await {
                                            error!(
                                                "Failed to update recording for fragment {}: {}", 
                                                fragment_id, e
                                            );
                                        } else {
                                            info!(
                                                "Updated recording for fragment {}, duration={}ms, size={}bytes", 
                                                fragment_id, duration_ms, file_size
                                            );
                                        }
                                    },
                                    Ok(None) => {
                                        error!(
                                            "Could not find recording entry for fragment {} of recording {}", 
                                            fragment_id, recording_id
                                        );
                                    },
                                    Err(e) => {
                                        error!(
                                            "Error finding recording for fragment {}: {}", 
                                            fragment_id, e
                                        );
                                    }
                                }
                            });
                        });
                    } else {
                        warn!("Missing fields in fragment-closed message: {:?}", structure);
                    }
                }
            }
        },
        gst::MessageView::Eos(_) => {
            info!("End of stream received for recording {}", recording_id_clone);
        },
        gst::MessageView::Error(err) => {
            error!(
                "Error from {}: {} ({})", 
                err.src().map(|s| s.name()).unwrap_or_else(|| "unknown".into()),
                err.error(),
                err.debug().unwrap_or_else(|| "no debug info".into())
            );
        },
        _ => {
            // Only log important messages to reduce noise
            trace!("Other message type: {:?}", msg.type_());
        }
    }
    glib::ControlFlow::Continue
})?;

        // Create recording entry in database with initial metadata
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
            segment_id: None,          // This is a parent recording, not a segment
            parent_recording_id: None, // Parent recording has no parent
        };

        // Add to active recordings
        {
            let mut active_recordings = self.active_recordings.lock().await;
            active_recordings.insert(recording_key, active_recording);
        }

        info!(
            "Started {} recording for camera {} with stream {}",
            event_type.to_string(),
            stream.camera_id,
            stream.id
        );

        // Publish recording started event
        if let Some(broker) = self.message_broker.lock().await.as_ref() {
            if let Err(e) = broker
                .publish(
                    crate::messaging::EventType::RecordingStarted,
                    Some(stream.camera_id),
                    serde_json::json!({
                        "recording_id": recording_id.to_string(),
                        "stream_id": stream.id.to_string(),
                        "event_type": event_type.to_string(),
                        "schedule_id": schedule_id.map(|id| id.to_string())
                    }),
                )
                .await
            {
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
            let active_recordings = self.active_recordings.lock().await;
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
            let mut active_recordings = self.active_recordings.lock().await;

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

        // Find the splitmuxsink element we added for this recording
        let pipeline = &active_recording.pipeline;
        let element_suffix = active_recording.recording_id.to_string().replace("-", "");
        let splitmuxsink_name = format!("record_splitmuxsink_{}", element_suffix);

        if let Some(splitmuxsink) = pipeline.by_name(&splitmuxsink_name) {
            // Send EOS just to the splitmuxsink element to finalize it properly
            let _ = splitmuxsink.send_event(gst::event::Eos::new());

            // Set only the splitmuxsink to NULL state
            let _ = splitmuxsink.set_state(gst::State::Null);

            info!(
                "Set splitmuxsink to Null state for recording {}",
                active_recording.recording_id
            );
        } else {
            warn!(
                "Could not find splitmuxsink element for recording {}",
                active_recording.recording_id
            );
        }

        // Find queue element
        let queue_name = format!("record_queue_{}", element_suffix);
        if let Some(queue) = pipeline.by_name(&queue_name) {
            // Set state to NULL
            let _ = queue.set_state(gst::State::Null);
        }

        // Find depay element
        let depay_name = format!("record_depay_{}", element_suffix);
        if let Some(depay) = pipeline.by_name(&depay_name) {
            // Set state to NULL
            let _ = depay.set_state(gst::State::Null);
        }

        // Find parse element
        let parse_name = format!("record_parse_{}", element_suffix);
        if let Some(parse) = pipeline.by_name(&parse_name) {
            // Set state to NULL
            let _ = parse.set_state(gst::State::Null);
        }

        // Wait for file to be fully written
        sleep(Duration::from_secs(1)).await;

        // Now unlink and remove elements from the pipeline - first find the source pad from tee
        // The tee element may have a generic name, look for it
        let mut tee_element = None;
        for element in pipeline.iterate_elements() {
            if let Ok(element) = element {
                if let Some(factory) = element.factory() {
                    if factory.name() == "tee" {
                        tee_element = Some(element);
                        break;
                    }
                }
            }
        }

        if let Some(tee) = tee_element {
            // Look for our queue element's sink pad
            if let Some(queue) = pipeline.by_name(&queue_name) {
                if let Some(queue_sink) = queue.static_pad("sink") {
                    // Find the tee src pad connected to our queue
                    let tee_pads = tee.pads();
                    for pad in tee_pads {
                        if pad.direction() == gst::PadDirection::Src {
                            if let Some(peer) = pad.peer() {
                                if peer == queue_sink {
                                    // Unlink this pad from the queue
                                    pad.unlink(&queue_sink).ok();
                                    // Release the pad back to the tee
                                    tee.release_request_pad(&pad);
                                    break;
                                }
                            }
                        }
                    }
                }

                // Remove elements from the pipeline
                // First unlink all elements from each other
                if let Some(parse) = pipeline.by_name(&parse_name) {
                    if let Some(splitmuxsink) = pipeline.by_name(&splitmuxsink_name) {
                        parse.unlink(&splitmuxsink);
                    }
                }

                if let Some(depay) = pipeline.by_name(&depay_name) {
                    if let Some(parse) = pipeline.by_name(&parse_name) {
                        depay.unlink(&parse);
                    }
                }

                if let Some(queue) = pipeline.by_name(&queue_name) {
                    if let Some(depay) = pipeline.by_name(&depay_name) {
                        queue.unlink(&depay);
                    }
                }

                // Now remove all elements from the pipeline
                if let Some(queue) = pipeline.by_name(&queue_name) {
                    pipeline.remove(&queue).ok();
                }

                if let Some(depay) = pipeline.by_name(&depay_name) {
                    pipeline.remove(&depay).ok();
                }

                if let Some(parse) = pipeline.by_name(&parse_name) {
                    pipeline.remove(&parse).ok();
                }

                if let Some(splitmuxsink) = pipeline.by_name(&splitmuxsink_name) {
                    pipeline.remove(&splitmuxsink).ok();
                }

                info!(
                    "Removed recording elements from pipeline for {}",
                    active_recording.recording_id
                );
            }
        }

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

        // Determine segments directory
        let end_time = Utc::now();
        let segments_dir = active_recording
            .file_path
            .parent()
            .unwrap_or_else(|| Path::new("."));

        // Find all segment files
        let segment_pattern = format!("segment_*.{}", self.format);

        // Get list of all segment files
        let mut segment_files = Vec::new();
        if segments_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(segments_dir) {
                for entry in entries.filter_map(Result::ok) {
                    if let Some(name) = entry.file_name().to_str() {
                        let pattern = glob::Pattern::new(&segment_pattern).unwrap_or_default();
                        if pattern.matches(name) {
                            segment_files.push(entry.path());
                        }
                    }
                }
            }
        }

        info!("Found {} segment files to finalize", segment_files.len());

        // Query for all segments associated with this parent recording
        let parent_recording_id = active_recording.recording_id;

        // Get all segments for this parent recording directly with SQL
        let segment_recordings = match sqlx::query_as::<_, RecordingDb>(
            r#"
            SELECT id, camera_id, stream_id, schedule_id, start_time, end_time, file_path, file_size,
                   duration, format, resolution, fps, event_type, metadata, segment_id, parent_recording_id
            FROM recordings
            WHERE parent_recording_id = $1
            "#
        )
        .bind(parent_recording_id)
        .fetch_all(&*self.recordings_repo.pool)
        .await
        {
            Ok(recordings) => {
                let recordings = recordings.into_iter().map(Recording::from).collect::<Vec<_>>();
                info!("Found {} segment recordings in database", recordings.len());
                recordings
            }
            Err(e) => {
                error!("Failed to query segment recordings: {}", e);
                Vec::new()
            }
        };

        // Track total file size for parent recording
        let mut total_file_size: u64 = 0;

        // First update all segment recordings to finalized state
        for segment_recording in segment_recordings {
            // Get segment index directly from the segment_id field
            let segment_idx = segment_recording.segment_id.unwrap_or(0) as usize;

            // Find corresponding file (if available)
            let segment_path = segment_recording.file_path.clone();
            let segment_file_size = if segment_path.exists() {
                if let Ok(metadata) = std::fs::metadata(&segment_path) {
                    metadata.len()
                } else {
                    0
                }
            } else {
                0
            };

            total_file_size += segment_file_size;

            // Create segment metadata update
            let segment_metadata = serde_json::json!({
                "finalized": true,
                "status": "completed",
                "completion_time": end_time.to_rfc3339(),
                "file_size_bytes": segment_file_size
            });

            // Create update object for segment
            let segment_update = RecordingUpdate {
                file_path: None, // Don't update path
                duration: None,  // Use whatever duration was already recorded
                file_size: Some(segment_file_size),
                end_time: Some(end_time),
                metadata: Some(segment_metadata),
                segment_id: Some(segment_idx as u32), // Keep the segment ID
                parent_recording_id: Some(parent_recording_id), // Keep the parent recording ID
            };

            // Save finalized segment to database using the new update_with_data method
            match self
                .recordings_repo
                .update_with_data(&segment_recording.id, segment_update)
                .await
            {
                Ok(_) => {
                    info!(
                        "Finalized segment {} with size {}B",
                        segment_recording.id, segment_file_size
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to update segment recording {}: {}",
                        segment_recording.id, e
                    );
                }
            }
        }

        // Now update the parent recording as well
        let parent_recording_id = active_recording.recording_id;

        // Create final metadata for parent recording
        let final_metadata = serde_json::json!({
            "finalized": true,
            "status": "completed",
            "completion_time": end_time.to_rfc3339(),
            "segment_count": segment_files.len(),
            "total_size_bytes": total_file_size,
            "recording_type": "segmented"
        });

        // Create update object for parent recording
        let parent_update = RecordingUpdate {
            file_path: None, // Don't update path
            duration: Some(duration),
            file_size: Some(total_file_size),
            end_time: Some(end_time),
            metadata: Some(final_metadata),
            segment_id: None,          // Parent recording is not a segment
            parent_recording_id: None, // Parent recording has no parent
        };

        // Save finalized parent recording to database using the new update_with_data method
        match self
            .recordings_repo
            .update_with_data(&parent_recording_id, parent_update)
            .await
        {
            Ok(_) => {
                info!("Successfully finalized parent recording {} with {} segments, duration {}s and total size {}B",
                    parent_recording_id, segment_files.len(), duration, total_file_size);
            }
            Err(e) => {
                error!(
                    "Failed to update parent recording {}: {}",
                    parent_recording_id, e
                );
            }
        }

        info!(
            "Stopped recording {} for camera {}",
            active_recording.recording_id, active_recording.camera_id
        );

        // Publish recording stopped event
        if let Some(broker) = self.message_broker.lock().await.as_ref() {
            if let Err(e) = broker
                .publish(
                    crate::messaging::EventType::RecordingStopped,
                    Some(active_recording.camera_id),
                    serde_json::json!({
                        "recording_id": active_recording.recording_id.to_string(),
                        "stream_id": active_recording.stream_id.to_string(),
                        "duration_seconds": duration,
                        "file_size_bytes": file_size,
                        "event_type": active_recording.event_type.to_string(),
                        "schedule_id": active_recording.schedule_id.map(|id| id.to_string())
                    }),
                )
                .await
            {
                warn!("Failed to publish recording stopped event: {}", e);
            }
        }

        Ok(())
    }

    /// Stop all active recordings
    pub async fn stop_all_recordings(&self) -> Result<()> {
        // Get all active recordings
        let active_recordings = {
            let active_recordings = self.active_recordings.lock().await;
            active_recordings.keys().cloned().collect::<Vec<_>>()
        };

        // Stop each recording
        for key in active_recordings {
            let _ = self.stop_recording_by_key(&key).await;
        }

        Ok(())
    }

    /// Check if a recording is currently active for schedule and stream
    pub async fn is_recording_active(&self, schedule_id: &Uuid, stream_id: &Uuid) -> bool {
        let recording_key = format!("{}-{}", schedule_id, stream_id);
        let active_recordings = self.active_recordings.lock().await;
        active_recordings.contains_key(&recording_key)
    }

    /// Check if any recording is active for a stream
    pub async fn is_stream_recording(&self, stream_id: &Uuid) -> bool {
        let active_recordings = self.active_recordings.lock().await;
        active_recordings
            .values()
            .any(|r| &r.stream_id == stream_id)
    }

    /// Get status of all active recordings
    pub async fn get_recording_status(&self) -> Vec<RecordingStatus> {
        let active_recordings = self.active_recordings.lock().await;

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
                    segment_id: recording.segment_id,
                    parent_recording_id: recording.parent_recording_id,
                }
            })
            .collect()
    }

    /// Get status of a specific recording
    pub async fn get_recording_status_by_id(&self, recording_id: &Uuid) -> Option<RecordingStatus> {
        let active_recordings = self.active_recordings.lock().await;

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
                    segment_id: recording.segment_id,
                    parent_recording_id: recording.parent_recording_id,
                }
            })
    }
}
