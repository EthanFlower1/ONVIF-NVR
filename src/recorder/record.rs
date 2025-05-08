use crate::db::models::recording_models::{
    Recording, RecordingDb, RecordingEventType, RecordingUpdate,
};
use crate::db::models::recording_schedule_models::RecordingSchedule;
use crate::db::models::stream_models::Stream;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::messaging::broker::MessageBrokerTrait;
use crate::stream_manager::StreamManager;
use crate::utils::metadataparser::parse_onvif_event;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc, Datelike};
// use cocoa::appkit::NSEventType::NSCursorUpdate;
use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSinkCallbacks};
use log::{debug, error, info, warn};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[derive(Clone)]
pub struct RecordingManager {
    stream_manager: Arc<StreamManager>,
    recordings_repo: RecordingsRepository,
    active_recordings: Arc<Mutex<std::collections::HashMap<String, ActiveRecordingElements>>>,
    recording_base_path: PathBuf,
    segment_duration: i64,
    format: String,
    message_broker: Arc<Mutex<Option<Arc<crate::messaging::MessageBroker>>>>,
    // Track active events requiring recording to continue
    active_events: Arc<Mutex<HashMap<String, chrono::DateTime<Utc>>>>,
}

pub struct ActiveRecordingElements {
    // GStreamer components
    pipeline: gst::Pipeline, // Reference to the main pipeline these elements are part of
    video_tee_pad: gst::Pad, // The src pad on the main video_tee used by this recording
    video_queue: gst::Element,
    video_depay: gst::Element,
    video_parse: gst::Element,
    muxer: gst::Element,        // The mp4mux instance provided to splitmuxsink
    splitmuxsink: gst::Element,
    splitmuxsink_video_pad: gst::Pad, // The video sink pad on splitmuxsink

    audio_tee_pad: Option<gst::Pad>, // The src pad on the main audio_tee
    audio_elements_chain: Option<Vec<gst::Element>>, // Full chain from queue to parser/encoder for audio
    splitmuxsink_audio_pad: Option<gst::Pad>, // The audio sink pad on splitmuxsink

    // Data fields (merged from original ActiveRecording)
    pub recording_id: Uuid,          // Unique ID for this recording session (parent for segments)
    pub schedule_id: Option<Uuid>,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub event_type: RecordingEventType,
    pub file_path: PathBuf,          // Should be the directory where segments are stored (dir_path)
    pub pipeline_watch_id: Option<gst::bus::BusWatchGuard>, // For watching bus messages related to this recording
}

#[derive(Debug, Clone)]
pub struct RecordingStatus {
    pub recording_id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub start_time: DateTime<Utc>,
    pub duration: i64,         // Current duration in seconds
    pub file_size: u64,        // For active splitmuxsink, this is tricky. Sum of segments or 0.
    pub pipeline_state: String,
    pub fps: i32,              // Currently hardcoded to 0, consider if it can be obtained
    pub event_type: RecordingEventType,
    pub segment_id: Option<u32>, // Should be None for the parent RecordingStatus
    pub parent_recording_id: Option<Uuid>, // Should be None for the parent itself
}
enum DatabaseOperation {
    UpdateSegment(
        RecordingsRepository, // Replace with your actual repository type
        String,               // location
        u32,                  // fragment_id
        u64,                  // fragment_duration
        u64,                  // duration_ms
        u64,                  // file_size
    ),
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
            active_recordings: Arc::new(Mutex::new(HashMap::new())),
            recording_base_path: recording_base_path.to_owned(),
            segment_duration,
            format: format.to_owned(),
            message_broker: Arc::new(Mutex::new(None)),
            active_events: Arc::new(Mutex::new(HashMap::new())),
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
                return Err(anyhow!("Already recording stream {} with key {}", stream.id, recording_key));
            }
        }

        // Get reported codec info from stream
        let reported_video_codec = stream.codec.clone().unwrap_or_default();
        let reported_audio_codec = stream.audio_codec.clone().unwrap_or_default();

        info!(
           "Initiating recording for stream {}. Reported video: [{}], Reported audio: [{}]",
           stream.id, reported_video_codec, reported_audio_codec
        );

        let recording_id = Uuid::new_v4(); // This is the parent recording ID for all segments
        let now = Utc::now();

        match self.log_metadata_stream(&stream.id.to_string()) {
            Ok(_) => info!( // Changed to info! from println! for consistency
                "Successfully started logging metadata for stream {}",
                stream.id
            ),
            Err(e) => error!("Failed to log metadata for stream {}: {}", stream.id, e), // Changed to error!
        }
        
        // Create directory structure
        let year = now.format("%Y").to_string();
        let month = now.format("%m").to_string();
        let day = now.format("%d").to_string();
        let camera_id_str = stream.camera_id.to_string(); // Renamed for clarity
        let stream_name_str = stream.name.clone(); // Renamed for clarity
        
        let mut dir_path = self.recording_base_path
            .join(&camera_id_str)
            .join(&stream_name_str)
            .join(&year)
            .join(&month)
            .join(&day);

        match std::fs::create_dir_all(&dir_path) {
            Ok(_) => {
                debug!("Successfully created directory: {:?}", dir_path);
                #[cfg(target_os = "macos")]
                {
                    if let Ok(abs_path) = std::fs::canonicalize(&dir_path) {
                        debug!("MacOS: Using absolute path for recording: {:?}", abs_path);
                        dir_path = abs_path;
                    }
                }
            }
            Err(e) => {
                error!("Failed to create directory {:?}: {}", dir_path, e);
                return Err(anyhow!("Failed to create recording directory {:?}: {}", dir_path, e));
            }
        };

        // Note: Original file_name and file_path for a single file are less relevant with splitmuxsink.
        // The `location` property of splitmuxsink defines the segment naming pattern.
        // However, keeping `element_suffix` for unique GStreamer element names is good.
        info!("Recording segments will be stored in: {:?}", dir_path);

        // Get access to the MAIN PIPELINE and TEEs
        let (pipeline, video_tee, audio_tee, _audio_source_element) = self
            .stream_manager
            .get_stream_access(&stream.id.to_string())
            .map_err(|e| {
                error!("Failed to get stream access for {}: {}", stream.id, e);
                anyhow!("Failed to get stream access for {}: {}", stream.id, e)
            })?;

        // Ensure pipeline is playing to get caps from live tees
        // This is crucial if the main pipeline isn't already playing or if elements are added to a PAUSED pipeline.
        if pipeline.current_state() != gst::State::Playing {
            info!("Pipeline not in PLAYING state. Setting to PLAYING for caps detection.");
            pipeline.set_state(gst::State::Playing)
                .map_err(|_| anyhow!("Failed to set pipeline to PLAYING before caps detection"))?;
            // Wait for state change, especially if it was NULL or READY
            let (state_res, _current, _pending) = pipeline.state(gst::ClockTime::from_seconds(2));
             state_res.map_err(|_| anyhow!("Pipeline did not reach PLAYING state in time for caps detection"))?;
            info!("Pipeline set to PLAYING for caps detection.");
        } else {
            info!("Pipeline already in PLAYING state for caps detection.");
        }
        
        // Generate unique suffix for GStreamer elements for this recording instance
        let element_suffix = recording_id.to_string().replace("-", "");

        //-----------------------------------------------------------------------------
        // DETERMINE ACTUAL VIDEO CODEC FROM CAPS
        //-----------------------------------------------------------------------------
        let detected_video_codec = { // Renamed to detected_video_codec for clarity
            let temp_video_queue = gst::ElementFactory::make("queue")
                .name(format!("temp_video_queue_{}", element_suffix))
                .build()?;
            pipeline.add(&temp_video_queue)?;
            // Sync state *before* linking if pipeline is already playing.
            temp_video_queue.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync temp_video_queue state"))?;


            let video_tee_src_pad = video_tee
                .request_pad_simple("src_%u")
                .ok_or_else(|| anyhow!("Failed to get temporary src pad from video_tee"))?;
            let temp_video_queue_sink_pad = temp_video_queue
                .static_pad("sink")
                .ok_or_else(|| anyhow!("Failed to get sink pad from temporary video_queue"))?;
            
            video_tee_src_pad.link(&temp_video_queue_sink_pad).map_err(|_| anyhow!("Failed to link video_tee to temp_video_queue"))?;
            
            sleep(Duration::from_millis(500)).await; // Give time for caps

            let temp_video_queue_src_pad = temp_video_queue
                .static_pad("src")
                .ok_or_else(|| anyhow!("Failed to get src pad from temporary video_queue"))?;
            
            let current_video_caps = temp_video_queue_src_pad.current_caps();
            let determined_codec = if let Some(caps) = current_video_caps {
                debug!("Raw video caps: {}", caps.to_string());
                if let Some(structure) = caps.structure(0) {
                    let mime_type = structure.name();
                    info!("Detected video caps structure: {}", structure.to_string());
                    if mime_type.contains("x-rtp") {
                        structure.get::<String>("encoding-name").map(|enc| enc.to_lowercase())
                            .unwrap_or_else(|_| {
                                warn!("Could not get 'encoding-name' from video RTP caps, using reported: {}", reported_video_codec);
                                reported_video_codec.to_lowercase()
                            })
                    } else { 
                        if mime_type.contains("h264") { "h264".to_string() }
                        else if mime_type.contains("h265") || mime_type.contains("hevc") { "h265".to_string() }
                        else if mime_type.contains("jpeg") { "jpeg".to_string() }
                        else {
                            warn!("Unknown non-RTP video mime_type: {}, using reported: {}", mime_type, reported_video_codec);
                            reported_video_codec.to_lowercase()
                        }
                    }
                } else {
                    warn!("No structure in video caps, using reported: {}", reported_video_codec);
                    reported_video_codec.to_lowercase()
                }
            } else {
                warn!("No video caps available from temp queue, using reported: {}", reported_video_codec);
                reported_video_codec.to_lowercase()
            };

            // Cleanup temporary video elements
            video_tee_src_pad.unlink(&temp_video_queue_sink_pad).map_err(|_| anyhow!("Failed to unlink video_tee_src_pad from temp_video_queue_sink_pad"))?;
            temp_video_queue.set_state(gst::State::Null).map_err(|_| anyhow!("Failed to set temp_video_queue to NULL"))?;
            pipeline.remove(&temp_video_queue).map_err(|_| anyhow!("Failed to remove temp_video_queue from pipeline"))?;
            video_tee.release_request_pad(&video_tee_src_pad);
            info!("Determined video codec: {}", determined_codec);
            determined_codec
        };


        //-----------------------------------------------------------------------------
        // DETERMINE ACTUAL AUDIO CODEC FROM CAPS (if audio is expected)
        //-----------------------------------------------------------------------------
        let detected_audio_codec = if !reported_audio_codec.is_empty() { // Renamed to detected_audio_codec
            info!("Attempting to determine actual audio codec (reported: {})...", reported_audio_codec);
            let temp_audio_queue = gst::ElementFactory::make("queue")
                .name(format!("temp_audio_queue_{}", element_suffix))
                .build()?;
            
            pipeline.add(&temp_audio_queue)?;
            temp_audio_queue.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync temp_audio_queue state"))?;

            let audio_tee_src_pad = audio_tee.request_pad_simple("src_%u")
                .ok_or_else(|| anyhow!("Failed to get temporary src pad from audio_tee. Is audio active on this stream?"))?;
            
            let temp_audio_queue_sink_pad = temp_audio_queue.static_pad("sink")
                .ok_or_else(|| anyhow!("Failed to get sink pad from temporary audio_queue"))?;

            let link_result = audio_tee_src_pad.link(&temp_audio_queue_sink_pad);
            
            let determined_codec = if link_result.is_err() {
                warn!("Failed to link audio_tee to temp_audio_queue. This might happen if audio_tee is not actively streaming. Falling back to reported audio codec: {}", reported_audio_codec);
                reported_audio_codec.to_lowercase()
            } else {
                sleep(Duration::from_millis(500)).await; 

                let temp_audio_queue_src_pad = temp_audio_queue.static_pad("src")
                    .ok_or_else(|| anyhow!("Failed to get src pad from temporary audio_queue"))?;

                let current_audio_caps = temp_audio_queue_src_pad.current_caps();
                let codec_from_caps = if let Some(caps) = current_audio_caps {
                    debug!("Raw audio caps: {}", caps.to_string());
                    if let Some(structure) = caps.structure(0) {
                        let mime_type = structure.name();
                        info!("Detected audio caps structure: {}", structure.to_string());
                        if mime_type.contains("x-rtp") {
                            let encoding_name = structure.get::<String>("encoding-name")
                                .map_err(|e| anyhow!("Failed to get encoding-name from audio RTP caps: {}", e))?
                                .to_lowercase();
                            match encoding_name.as_str() {
                                "pcmu" | "g711u" => "pcmu".to_string(),
                                "pcma" | "g711a" => "pcma".to_string(),
                                "mpeg4-generic" | "aac" => "aac".to_string(), // Common for AAC in RTP
                                _ => {
                                    warn!("Unknown RTP audio encoding-name: {}. Falling back to reported: {}", encoding_name, reported_audio_codec);
                                    reported_audio_codec.to_lowercase()
                                }
                            }
                        } else { 
                            if mime_type.contains("aac") || mime_type.contains("mp4a-latm") { "aac".to_string() }
                            else if mime_type.contains("mulaw") || mime_type.contains("pcmu") { "pcmu".to_string() }
                            else if mime_type.contains("alaw") || mime_type.contains("pcma") { "pcma".to_string() }
                            else {
                                warn!("Unknown non-RTP audio mime_type: {}. Falling back to reported: {}", mime_type, reported_audio_codec);
                                reported_audio_codec.to_lowercase()
                            }
                        }
                    } else {
                        warn!("No structure in audio caps. Falling back to reported: {}", reported_audio_codec);
                        reported_audio_codec.to_lowercase()
                    }
                } else {
                    warn!("No audio caps available from temp queue. Falling back to reported: {}", reported_audio_codec);
                    reported_audio_codec.to_lowercase()
                };
                codec_from_caps
            };
            
            // Cleanup temporary audio elements (regardless of link success for the queue itself)
            if link_result.is_ok() { // Only unlink if link was successful
                 audio_tee_src_pad.unlink(&temp_audio_queue_sink_pad).map_err(|_| anyhow!("Failed to unlink audio_tee_src_pad from temp_audio_queue_sink_pad"))?;
            }

            temp_audio_queue.set_state(gst::State::Null).map_err(|_| anyhow!("Failed to set temp_audio_queue to NULL"))?;
            pipeline.remove(&temp_audio_queue).map_err(|_| anyhow!("Failed to remove temp_audio_queue from pipeline"))?;
            audio_tee.release_request_pad(&audio_tee_src_pad); // Always release the pad
            info!("Determined audio codec: {}", determined_codec);
            determined_codec
        } else {
            info!("No reported audio codec for stream {}. Recording video-only.", stream.id);
            "".to_string() 
        };

        //-----------------------------------------------------------------------------
        // MUXER & SPLITMUXSINK SETUP
        //-----------------------------------------------------------------------------
        let muxer = gst::ElementFactory::make("mp4mux")
            .name(format!("mp4mux_{}", element_suffix))
            .property("faststart", true)
            .property("streamable", true)
            .property("fragment-duration", 1000_u32) 
            .property("movie-timescale", 90000_u32) 
            .property("trak-timescale", 90000_u32)
            .build()?;

        let splitmuxsink = gst::ElementFactory::make("splitmuxsink")
            .name(format!("splitmuxsink_{}", element_suffix))
            .property("muxer", &muxer) 
            .property("location",format!("{}/segment_%Y%m%d_%H%M%S_%%05d.{}", dir_path.to_str().ok_or_else(|| anyhow!("Dir path is not valid UTF-8"))?, self.format)) // Timestamped segment names
            .property("max-size-time", gst::ClockTime::from_seconds(self.segment_duration as u64)) 
            .property("max-size-bytes", 0u64) 
            .property("async-finalize", true)
            .property("max-files", 0u32) 
            .build()?;
        
        // Setup segment location signal handler
        let recording_id_clone = recording_id; // This is the parent recording ID
        let stream_clone = stream.clone();
        let format_clone = self.format.clone();
        let event_type_clone = event_type;
        let schedule_id_clone = schedule_id;
        let recordings_repo_clone = self.recordings_repo.clone();
        let start_time_clone = now; // Start time of the whole recording session
        let segment_duration_clone = self.segment_duration;
        let dir_path_clone_for_signal = dir_path.clone(); // dir_path for the signal handler

        let (tx_db, mut rx_db) = tokio::sync::mpsc::channel(100); 
        let tx_db_clone_for_signal = tx_db.clone(); 

        tokio::spawn(async move {
            while let Some((segment_rec, frag_id)) = rx_db.recv().await {
                if let Err(e) = recordings_repo_clone.create(&segment_rec).await {
                    error!("Failed to create DB entry for segment {} (frag_id {}): {}", segment_rec.id, frag_id, e);
                } else {
                    debug!("Successfully created DB entry for segment {} (frag_id {})", segment_rec.id, frag_id);
                }
            }
        });
        
        splitmuxsink.connect("format-location-full", false, move |args| {
            if args.len() < 3 {
                warn!("format-location-full signal: unexpected number of args: {}", args.len());
                // Fallback filename if something is wrong with args
                return Some(format!("{}/fallback_segment_%05d.{}", dir_path_clone_for_signal.to_str().unwrap_or("."), format_clone).to_value());
            }
        
            let fragment_id = args[1].get::<u32>().unwrap_or_else(|e| {
                warn!("Failed to get fragment_id from signal: {}. Defaulting to 0.", e); 0
            });
            
            // Use the timestamp from the buffer if available, otherwise generate one.
            // The filename pattern in splitmuxsink already includes a timestamp.
            // This handler primarily focuses on DB entry creation.
            let current_segment_timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
            let segment_filename = format!(
                "{}/segment_{}_{:05}.{}", // Consistent naming with splitmuxsink pattern (approx)
                dir_path_clone_for_signal.to_str().unwrap_or("."),
                current_segment_timestamp,
                fragment_id,
                format_clone
            );
        
            let mut width = 0;
            let mut height = 0;
            let mut fps_num = 0;
            let mut fps_den = 1;
            let mut mime = "unknown/unknown";
            let mut caps_string = "N/A".to_string();
            let mut pts_val: Option<u64> = None;
            let mut dts_val: Option<u64> = None;
            let mut duration_val: Option<u64> = None;

            if let Ok(first_sample) = args[2].get::<gst::Sample>() {
                if let Some(sample_caps) = first_sample.caps() {
                    caps_string = sample_caps.to_string();
                    if let Some(s) = sample_caps.structure(0) {
                        mime = s.name();
                        width = s.get("width").unwrap_or(0);
                        height = s.get("height").unwrap_or(0);
                        if let Ok(frac) = s.get::<gst::Fraction>("framerate") {
                            fps_num = frac.numer();
                            fps_den = frac.denom();
                        }
                    }
                }
                if let Some(buffer) = first_sample.buffer() {
                    pts_val = buffer.pts().map(|pts| pts.nseconds());
                    dts_val = buffer.dts().map(|dts| dts.nseconds());
                    duration_val = buffer.duration().map(|dur| dur.nseconds());
                }
            }

            let segment_start_time = if let Some(pts_ns) = pts_val {
                 start_time_clone + chrono::Duration::nanoseconds(pts_ns as i64) // More accurate if PTS is absolute
            } else {
                start_time_clone + chrono::Duration::seconds(fragment_id as i64 * segment_duration_clone)
            };

            let actual_fps = if fps_num > 0 && fps_den > 0 {
                (fps_num as f64 / fps_den as f64) as u32
            } else {
                stream_clone.framerate.unwrap_or(0) as u32
            };
            let actual_resolution = if width > 0 && height > 0 {
                format!("{}x{}", width, height)
            } else {
                stream_clone.resolution.clone().unwrap_or_else(|| "unknown".to_string())
            };

            let segment_metadata_json = json!({
                "status": "processing", "finalized": false, "creation_time": Utc::now().to_rfc3339(),
                "video_info": {
                    "mime_type": mime, "width": width, "height": height,
                    "framerate_num": fps_num, "framerate_den": fps_den,
                    "pts_ns": pts_val, "dts_ns": dts_val, "buffer_duration_ns": duration_val,
                    "caps_string": caps_string,
                }
            });
        
            let segment_recording_entry = Recording {
                id: Uuid::new_v4(), 
                camera_id: stream_clone.camera_id, stream_id: stream_clone.id,
                start_time: segment_start_time, end_time: None, 
                file_path: PathBuf::from(&segment_filename), // This path is for DB, actual path from splitmuxsink
                file_size: 0, duration: segment_duration_clone as u64, 
                format: format_clone.clone(), resolution: actual_resolution, fps: actual_fps,
                event_type: event_type_clone, metadata: Some(segment_metadata_json),
                schedule_id: schedule_id_clone, segment_id: Some(fragment_id),
                parent_recording_id: Some(recording_id_clone),
            };
        
            if let Err(e) = tx_db_clone_for_signal.try_send((segment_recording_entry.clone(), fragment_id)) {
                error!("Failed to send segment info to DB task for frag {}: {}", fragment_id, e);
            }
        
            // The filename returned here is what splitmuxsink will use.
            // It should match the pattern defined in the `location` property for consistency.
            // The `splitmuxsink` already creates timestamped names if `%Y%m%d_%H%M%S` is in `location`.
            // This signal is more for *knowing* the name it's about to use.
            let final_segment_path = PathBuf::from(dir_path_clone_for_signal.to_str().unwrap_or("."))
                .join(format!("segment_{}_{:05}.{}", current_segment_timestamp, fragment_id, format_clone));

            debug!("format-location-full: providing filename: {}", final_segment_path.display());
            Some(final_segment_path.to_str().unwrap_or("").to_value())
        });

        //-----------------------------------------------------------------------------
        // VIDEO PROCESSING CHAIN SETUP
        //-----------------------------------------------------------------------------
        let video_queue = gst::ElementFactory::make("queue")
            .name(format!("record_video_queue_{}", element_suffix))
            .build()?;

        let (video_depay, video_parse) = match detected_video_codec.as_str() {
            "h264" => (
                gst::ElementFactory::make("rtph264depay")
                    .name(format!("record_video_depay_{}", element_suffix)).build()?,
                gst::ElementFactory::make("h264parse")
                    .name(format!("record_video_parse_{}", element_suffix))
                    .build()?,
            ),
            "h265" | "hevc" => (
                gst::ElementFactory::make("rtph265depay")
                    .name(format!("record_video_depay_{}", element_suffix)).build()?,
                gst::ElementFactory::make("h265parse")
                    .name(format!("record_video_parse_{}", element_suffix))
                    .property("config-interval", -1i32) 
                    .build()?,
            ),
            "jpeg" | "mjpeg" => ( 
                gst::ElementFactory::make("rtpjpegdepay")
                    .name(format!("record_video_depay_{}", element_suffix)).build()?,
                gst::ElementFactory::make("jpegparse") 
                    .name(format!("record_video_parse_{}", element_suffix)).build()?,
            ),
            _ => {
                error!("Unsupported video codec for recording: {}. Aborting.", detected_video_codec);
                return Err(anyhow!("Unsupported video codec: {}", detected_video_codec));
            }
        };
        info!("Video chain for {}: queue ! {} ! {}", detected_video_codec, video_depay.name(), video_parse.name());

        //-----------------------------------------------------------------------------
        // AUDIO PROCESSING CHAIN SETUP (with G.711 to AAC transcoding)
        //-----------------------------------------------------------------------------
        let mut audio_elements_to_add: Vec<gst::Element> = Vec::new();
        let mut final_audio_processor_for_muxer: Option<gst::Element> = None;

        if !detected_audio_codec.is_empty() {
            info!("Setting up audio chain for determined codec: {}", detected_audio_codec);
            let current_audio_queue = gst::ElementFactory::make("queue")
                .name(format!("record_audio_queue_{}", element_suffix))
                .build()?;

            audio_elements_to_add.push(current_audio_queue.clone());

            match detected_audio_codec.as_str() {
                "aac" => {
                    let depay = gst::ElementFactory::make("rtpmp4gdepay")
                        .name(format!("record_audio_depay_aac_{}", element_suffix))
                        .build()?;
                    let parse = gst::ElementFactory::make("aacparse")
                        .name(format!("record_audio_parse_aac_{}", element_suffix))
                        .build()?;
                    
                    audio_elements_to_add.push(depay);
                    audio_elements_to_add.push(parse.clone()); 
                    final_audio_processor_for_muxer = Some(parse);
                    info!("Audio chain (AAC passthrough): queue ! rtpmp4gdepay ! aacparse -> muxer");
                }
                "pcmu" | "pcma" => {
                    let depay_name = if detected_audio_codec == "pcmu" { "rtppcmudepay" } else { "rtppcmadepay" };
                    let decode_name = if detected_audio_codec == "pcmu" { "mulawdec" } else { "alawdec" };

                    let depay = gst::ElementFactory::make(depay_name)
                        .name(format!("record_audio_depay_{}_{}", detected_audio_codec, element_suffix))
                        .build()?;
                    let decode = gst::ElementFactory::make(decode_name)
                        .name(format!("record_audio_decode_{}_{}", detected_audio_codec, element_suffix))
                        .build()?;
                    let audioconvert = gst::ElementFactory::make("audioconvert")
                        .name(format!("record_audio_convert_{}", element_suffix))
                        .build()?;
                    let audio_encoder_aac = gst::ElementFactory::make("avenc_aac")
                        .name(format!("record_audio_enc_aac_{}", element_suffix))
                        // .property("bitrate", 128000_i32) // Example bitrate
                        .build()?;
                    let aacparse = gst::ElementFactory::make("aacparse")
                        .name(format!("record_audio_transcoded_parse_aac_{}", element_suffix))
                        .build()?;

                    audio_elements_to_add.push(depay);
                    audio_elements_to_add.push(decode);
                    audio_elements_to_add.push(audioconvert);
                    audio_elements_to_add.push(audio_encoder_aac);
                    audio_elements_to_add.push(aacparse.clone()); 
                    final_audio_processor_for_muxer = Some(aacparse);
                    info!(
                        "Audio chain ({} to AAC): queue ! {} ! {} ! audioconvert ! avenc_aac ! aacparse -> muxer",
                        detected_audio_codec, depay_name, decode_name
                    );
                }
                _ => {
                    warn!("Unsupported audio codec for recording: {}. No audio will be recorded.", detected_audio_codec);
                    audio_elements_to_add.clear(); 
                    final_audio_processor_for_muxer = None;
                }
            }
        } else {
            info!("No audio codec detected or specified. Recording video only.");
        }

        //-----------------------------------------------------------------------------
        // ADD ELEMENTS TO PIPELINE & LINK THEM
        //-----------------------------------------------------------------------------
        pipeline.add_many(&[&video_queue, &video_depay, &video_parse, &muxer, &splitmuxsink])
            .map_err(|_| anyhow!("Failed to add core video/mux elements to pipeline"))?;
        info!("Added video elements, muxer, and splitmuxsink to pipeline.");

        for el in &audio_elements_to_add {
            pipeline.add(el).map_err(|_| anyhow!("Failed to add audio element {} to pipeline", el.name()))?;
        }
        if !audio_elements_to_add.is_empty() {
            info!("Added {} audio processing elements to pipeline.", audio_elements_to_add.len());
        }

        // Link video chain: video_tee -> video_queue -> video_depay -> video_parse -> splitmuxsink (video pad)
        let video_tee_src_pad_for_record = video_tee.request_pad_simple("src_%u") // Renamed to avoid conflict
            .ok_or_else(|| anyhow!("Failed to get src pad from video_tee for recording video"))?;
        
        gst::Element::link_many(&[&video_queue, &video_depay, &video_parse])
            .map_err(|_| anyhow!("Failed to link video_queue -> video_depay -> video_parse"))?;
        info!("Linked video_queue -> video_depay -> video_parse.");

        let video_queue_sink_pad = video_queue.static_pad("sink")
            .ok_or_else(|| anyhow!("Failed to get sink pad from video_queue"))?;
        video_tee_src_pad_for_record.link(&video_queue_sink_pad)
            .map_err(|_| anyhow!("Failed to link video_tee to video_queue"))?;
        info!("Linked video_tee to video_queue.");

        let splitmux_video_sink_pad = splitmuxsink.request_pad_simple("video") 
            .ok_or_else(|| anyhow!("Failed to get video sink pad from splitmuxsink"))?;
        let video_parse_src_pad = video_parse.static_pad("src")
            .ok_or_else(|| anyhow!("Failed to get src pad from video_parse"))?;

        video_parse_src_pad.link(&splitmux_video_sink_pad)
            .map_err(|_| anyhow!("Failed to link video_parse to splitmuxsink video pad"))?;
        info!("Linked video_parse to splitmuxsink video pad.");


        // Link audio chain
        let mut audio_tee_src_pad_for_record_opt: Option<gst::Pad> = None; // Renamed
        let mut splitmux_audio_sink_pad_opt: Option<gst::Pad> = None;

        if !audio_elements_to_add.is_empty() && final_audio_processor_for_muxer.is_some() {
            let audio_tee_src_pad = audio_tee.request_pad_simple("src_%u")
                .ok_or_else(|| anyhow!("Failed to get src pad from audio_tee for recording audio"))?;

            if audio_elements_to_add.len() > 1 {
                let elements_to_link_refs: Vec<&gst::Element> = audio_elements_to_add.iter().collect();
                gst::Element::link_many(&elements_to_link_refs)
                    .map_err(|_| anyhow!("Failed to link audio processing chain of {} elements", audio_elements_to_add.len()))?;
                info!("Linked audio processing chain: {} elements.", audio_elements_to_add.len());
            }
            
            let first_audio_element_sink_pad = audio_elements_to_add[0].static_pad("sink")
                .ok_or_else(|| anyhow!("Failed to get sink pad from the first audio element (queue)"))?;

            
            if let Some(final_processor) = &final_audio_processor_for_muxer {
                let splitmux_audio_sink_pad = splitmuxsink.request_pad_simple("audio_%u")
                    .ok_or_else(|| anyhow!("Failed to get audio sink pad from splitmuxsink"))?;

                let final_audio_processor_src_pad = final_processor.static_pad("src")
                    .ok_or_else(|| anyhow!("Failed to get src pad from the final audio processing element"))?;
                
                final_audio_processor_src_pad.link(&splitmux_audio_sink_pad)
                    .map_err(|_| anyhow!("Failed to link final audio processor to splitmuxsink audio pad"))?;
                info!("Linked final audio processor ({}) to splitmuxsink audio pad.", final_processor.name());
                
                splitmux_audio_sink_pad_opt = Some(splitmux_audio_sink_pad);

            }

            audio_tee_src_pad.link(&first_audio_element_sink_pad)
                .map_err(|_| anyhow!("Failed to link audio_tee to first audio element"))?;
            info!("Linked audio_tee to the first audio element: {}", audio_elements_to_add[0].name());

            audio_tee_src_pad_for_record_opt = Some(audio_tee_src_pad);

        }

        // Sync states of newly added elements with parent (pipeline)
        video_queue.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync video_queue state"))?;
        video_depay.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync video_depay state"))?;
        video_parse.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync video_parse state"))?;
        muxer.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync muxer state"))?;
        splitmuxsink.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync splitmuxsink state"))?;

        for el in &audio_elements_to_add {
            el.sync_state_with_parent().map_err(|_| anyhow!("Failed to sync audio element {} state", el.name()))?;
        }
        if !audio_elements_to_add.is_empty() {
            info!("Synced states of all new audio recording elements.");
        }



        
        // If pipeline was not playing before, set it to playing now that all elements are linked.
        // If it was already playing for caps detection, this confirms its state.
// Get the current state before any action
    let (initial_state_result, initial_current_state, initial_pending_state) = pipeline.state(gst::ClockTime::ZERO); // Use ZERO for immediate state query

    match initial_state_result {
        Ok(_) => {
            info!(
                "Before final check: Pipeline current state is {:?}, pending state is {:?}.",
                initial_current_state, initial_pending_state
            );
        }
        Err(_) => {
            warn!("Before final check: Failed to query initial pipeline state. Proceeding with state check.");
        }
    }

if initial_current_state != gst::State::Playing {
        info!(
            "Pipeline is in state {:?} (pending {:?}) after linking all recording elements. Attempting to set to PLAYING.",
            initial_current_state, initial_pending_state
        );

        match pipeline.set_state(gst::State::Playing) {
            Ok(state_change_ret) => {
                info!("Call to pipeline.set_state(PLAYING) returned: {:?}", state_change_ret);

                // Use a match statement to handle the StateChangeReturn variants
                match state_change_ret {

                    gst::StateChangeSuccess::Success => {
                        info!("Pipeline state change to PLAYING was immediately successful (Success).");
                        // Pipeline is PLAYING, further waiting might only be for confirmation or if other async operations are involved.
                    }
                    gst::StateChangeSuccess::Async => {
                        info!("Pipeline state change to PLAYING is asynchronous (Async). Waiting for completion...");
                        // The waiting logic below will handle this.
                    }
                    gst::StateChangeSuccess::NoPreroll => {
                        info!("Pipeline state change to PLAYING was successful without preroll (NoPreroll).");
                        // Pipeline is PLAYING, similar to Success.
                    }
                    // Use a wildcard for any other variants if the enum might be non-exhaustive in the future,
                    // though for StateChangeReturn it's usually exhaustive with these main four.
                    _ => {
                        warn!("set_state returned an unexpected or unhandled StateChangeReturn variant: {:?}", state_change_ret);
                        // Treat as needing to wait and verify, similar to Async.
                    }
                }

                // Proceed to wait and verify the state, especially if it was Async or if you want to be certain.
                info!("Verifying pipeline reaches PLAYING state (timeout: 2 seconds)...");
                let (state_query_res, current_final_state, pending_final_state) =
                    pipeline.state(gst::ClockTime::from_seconds(2));

                match state_query_res {
                    Ok(_) => {
                        info!(
                            "After waiting/verification: Pipeline current state is {:?}, pending state is {:?}.",
                            current_final_state, pending_final_state
                        );
                        if current_final_state == gst::State::Playing && (pending_final_state == gst::State::VoidPending || pending_final_state == gst::State::Playing) {
                            info!("Pipeline successfully reached and settled in PLAYING state.");
                        } else {
                            error!(
                                "Pipeline did not reach/settle in PLAYING state. Current state: {:?}, Pending: {:?}. The recording may not start correctly.",
                                current_final_state, pending_final_state
                            );
                            // Optionally, return an error here if not reaching PLAYING is critical:
                            // return Err(anyhow!("Pipeline did not reach/settle in PLAYING state. Final state: current={:?}, pending={:?}", current_final_state, pending_final_state));
                        }
                    }
                    Err(e) => {
                        error!("Error while querying pipeline state to verify PLAYING: {:?}.", e);
                        return Err(anyhow!("Failed to query pipeline state while waiting for PLAYING after linking all elements: {:?}", e));
                    }
                }
            }
            Err(e) => { // This outer Err is for the pipeline.set_state() call itself, e.g., if the pipeline element was invalid.
                error!("Critical error calling pipeline.set_state(PLAYING): {:?}", e);
                return Err(anyhow!("Failed to initiate pipeline state change to PLAYING after linking all elements: {:?}", e));
            }
        }
    } else {
        // If the pipeline was already PLAYING (and no pending state away from PLAYING)
        if initial_pending_state == gst::State::VoidPending || initial_pending_state == gst::State::Playing {
             info!("Pipeline was already in PLAYING state after linking all recording elements. No state change needed.");
        } else {
            info!("Pipeline current state is PLAYING, but has a pending state of {:?}. Waiting for it to settle (timeout: 2 seconds)...", initial_pending_state);
            let (state_query_res, current_final_state, pending_final_state) =
                pipeline.state(gst::ClockTime::from_seconds(2));
            match state_query_res {
                Ok(_) => info!("After waiting for pending state: Pipeline current state is {:?}, pending state is {:?}.", current_final_state, pending_final_state),
                Err(e) => warn!("Error querying state while waiting for pending state to resolve: {:?}", e),
            }
            if current_final_state != gst::State::Playing {
                 warn!("Pipeline was PLAYING but transitioned or did not settle to PLAYING. Final state: {:?}, pending: {:?}", current_final_state, pending_final_state);
            } else {
                 info!("Pipeline settled in PLAYING state.");
            }
        }
    }

        // Store active recording elements
let active_elements_struct = ActiveRecordingElements {
        // GStreamer pipeline and element references
        pipeline: pipeline.clone(), // Main pipeline instance
        video_tee_pad: video_tee_src_pad_for_record, // The Pad object from video_tee
        video_queue: video_queue.clone(), // Cloned GStreamer elements
        video_depay: video_depay.clone(),
        video_parse: video_parse.clone(),
        muxer: muxer.clone(), // The mp4mux specifically for this recording's splitmuxsink
        splitmuxsink: splitmuxsink.clone(),
        splitmuxsink_video_pad: splitmux_video_sink_pad, // The Pad object from splitmuxsink

        audio_tee_pad: audio_tee_src_pad_for_record_opt, // Option<gst::Pad>
        audio_elements_chain: if !audio_elements_to_add.is_empty() { Some(audio_elements_to_add) } else { None },
        splitmuxsink_audio_pad: splitmux_audio_sink_pad_opt, // Option<gst::Pad>

        // Data fields (previously from ActiveRecording struct)
        recording_id, // This is the Uuid generated earlier for this recording session
        schedule_id,  // Passed as an argument to this function
        camera_id: stream.camera_id,
        stream_id: stream.id,
        start_time: now, // This was `let now = Utc::now();` earlier in the function
        event_type,   // Passed as an argument to this function
        file_path: dir_path.clone(), // **IMPORTANT**: Store the directory path for segments
        pipeline_watch_id: None, // Store the bus watch guard if implemented
    };
        
        {
            let mut active_recordings_map = self.active_recordings.lock().await;
            active_recordings_map.insert(recording_key.clone(), active_elements_struct);
        }

        info!("Successfully started recording for stream {} (key: {}). Video: {}, Audio (to muxer): {}", 
            stream.id, recording_key, detected_video_codec, 
            if final_audio_processor_for_muxer.is_some() { "AAC" } else { "none" });

        Ok(recording_id) // Return the parent recording ID
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

            // info!(
            //     "Set splitmuxsink to Null state for recording {}",
            //     active_recording.recording_id
            // );
        } else {
            warn!(
                "Could not find splitmuxsink element for recording {}",
                active_recording.recording_id
            );
        }

        // Find and set video elements to NULL
        let queue_name = format!("record_queue_{}", element_suffix);
        if let Some(queue) = pipeline.by_name(&queue_name) {
            let _ = queue.set_state(gst::State::Null);
        }

        let depay_name = format!("record_depay_{}", element_suffix);
        if let Some(depay) = pipeline.by_name(&depay_name) {
            let _ = depay.set_state(gst::State::Null);
        }

        let parse_name = format!("record_parse_{}", element_suffix);
        if let Some(parse) = pipeline.by_name(&parse_name) {
            let _ = parse.set_state(gst::State::Null);
        }

        // Find and set audio elements to NULL
        let audio_queue_name = format!("record_audio_queue_{}", element_suffix);
        if let Some(audio_queue) = pipeline.by_name(&audio_queue_name) {
            let _ = audio_queue.set_state(gst::State::Null);
        }

        let audio_depay_name = format!("record_audio_depay_{}", element_suffix);
        if let Some(audio_depay) = pipeline.by_name(&audio_depay_name) {
            let _ = audio_depay.set_state(gst::State::Null);
        }

        // Audio parse no longer used

        let audio_decoder_name = format!("record_audio_decoder_{}", element_suffix);
        if let Some(audio_decoder) = pipeline.by_name(&audio_decoder_name) {
            let _ = audio_decoder.set_state(gst::State::Null);
        }

        let audio_convert_name = format!("record_audio_convert_{}", element_suffix);
        if let Some(audio_convert) = pipeline.by_name(&audio_convert_name) {
            let _ = audio_convert.set_state(gst::State::Null);
        }

        let audio_resample_name = format!("record_audio_resample_{}", element_suffix);
        if let Some(audio_resample) = pipeline.by_name(&audio_resample_name) {
            let _ = audio_resample.set_state(gst::State::Null);
        }

        let audio_encoder_name = format!("record_audio_encoder_{}", element_suffix);
        if let Some(audio_encoder) = pipeline.by_name(&audio_encoder_name) {
            let _ = audio_encoder.set_state(gst::State::Null);
        }

        let audio_format_parse_name = format!("record_audio_format_parse_{}", element_suffix);
        if let Some(audio_format_parse) = pipeline.by_name(&audio_format_parse_name) {
            let _ = audio_format_parse.set_state(gst::State::Null);
        }

        // Wait for file to be fully written
        sleep(Duration::from_secs(1)).await;

        // Now remove all video elements from the pipeline
        if let Some(queue) = pipeline.by_name(&queue_name) {
            pipeline.remove(&queue).ok();
        }

        if let Some(depay) = pipeline.by_name(&depay_name) {
            pipeline.remove(&depay).ok();
        }

        if let Some(parse) = pipeline.by_name(&parse_name) {
            pipeline.remove(&parse).ok();
        }

        // Remove all audio elements from the pipeline
        if let Some(audio_queue) = pipeline.by_name(&audio_queue_name) {
            pipeline.remove(&audio_queue).ok();
        }

        if let Some(audio_depay) = pipeline.by_name(&audio_depay_name) {
            pipeline.remove(&audio_depay).ok();
        }

        // Audio parse no longer used

        if let Some(audio_decoder) = pipeline.by_name(&audio_decoder_name) {
            pipeline.remove(&audio_decoder).ok();
        }

        if let Some(audio_convert) = pipeline.by_name(&audio_convert_name) {
            pipeline.remove(&audio_convert).ok();
        }

        if let Some(audio_resample) = pipeline.by_name(&audio_resample_name) {
            pipeline.remove(&audio_resample).ok();
        }

        if let Some(audio_encoder) = pipeline.by_name(&audio_encoder_name) {
            pipeline.remove(&audio_encoder).ok();
        }

        if let Some(audio_format_parse) = pipeline.by_name(&audio_format_parse_name) {
            pipeline.remove(&audio_format_parse).ok();
        }

        if let Some(splitmuxsink) = pipeline.by_name(&splitmuxsink_name) {
            pipeline.remove(&splitmuxsink).ok();
        }

        // info!(
        //     "Removed recording elements from pipeline for {}",
        //     active_recording.recording_id
        // );

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

        // info!("Found {} segment files to finalize", segment_files.len());

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
                // info!("Found {} segment recordings in database", recordings.len());
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
                    // info!(
                    //     "Finalized segment {} with size {}B",
                    //     segment_recording.id, segment_file_size
                    // );
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
                // info!("Successfully finalized parent recording {} with {} segments, duration {}s and total size {}B",
                //     parent_recording_id, segment_files.len(), duration, total_file_size);

                // Additional validation that files exist
                for (i, path) in segment_files.iter().enumerate().take(5) {
                    if path.exists() {
                        if let Ok(_metadata) = std::fs::metadata(path) {
                            // info!(
                            //     "Segment file {} exists: {:?}, size: {} bytes",
                            //     i,
                            //     path,
                            //     metadata.len()
                            // );
                        } else {
                            warn!(
                                "Segment file {} exists but cannot read metadata: {:?}",
                                i, path
                            );
                        }
                    } else {
                        warn!("Segment file {} does not exist: {:?}", i, path);
                    }
                }

                if segment_files.len() > 5 {
                    // info!("... and {} more segment files", segment_files.len() - 5);
                }
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
    
    /// Register an event that requires recording
    pub async fn register_event(&self, stream_id: &Uuid, event_type: RecordingEventType) -> Result<()> {
        let stream_key = stream_id.to_string();
        let now = Utc::now();
        
        // Update the event time in the active events map
        {
            let mut active_events = self.active_events.lock().await;
            active_events.insert(format!("{}-{}", stream_key, event_type.to_string()), now);
        }
        
        // Check if we're already recording this stream
        if self.is_stream_recording(stream_id).await {
            // Already recording, no need to start a new recording
            info!("Event received but already recording stream {}", stream_id);
            return Ok(());
        }
        
        // Get the stream info from the database
        let stream = match sqlx::query_as::<_, crate::db::models::stream_models::Stream>(
            "SELECT * FROM streams WHERE id = $1",
        )
        .bind(stream_id)
        .fetch_optional(&*self.recordings_repo.pool)
        .await? {
            Some(s) => s,
            None => {
                return Err(anyhow!("Stream not found: {}", stream_id));
            }
        };
        
        // Check for any active schedules that allow recording this event type
        let schedules = self.get_event_schedules(stream_id, &event_type).await?;
        
        if !schedules.is_empty() {
            // Use the first matching schedule to start recording
            let schedule = &schedules[0];
            info!("Starting event recording for stream {} using schedule {}", stream_id, schedule.id);
            
            let recording_id = self.start_recording(schedule, &stream).await?;
            info!("Started scheduled event recording {} for event type {}", recording_id, event_type.to_string());
        } else {
            // No matching schedule, start a standalone event recording
            let recording_id = self.start_event_recording(&stream, event_type).await?;
            info!("Started standalone event recording {} for event type {}", recording_id, event_type.to_string());
        }
        
        Ok(())
    }
    
    /// Get schedules that match this event type and are currently active
    async fn get_event_schedules(&self, stream_id: &Uuid, event_type: &RecordingEventType) -> Result<Vec<RecordingSchedule>> {
        // Get the current time
        let now = Utc::now();
        let day_of_week = now.weekday().num_days_from_sunday() as i32;
        let current_time = now.format("%H:%M").to_string();
        
        // Query for schedules that are active now and support this event type
        let event_field = match event_type {
            RecordingEventType::Motion => "record_on_motion",
            RecordingEventType::Audio => "record_on_audio",
            RecordingEventType::Analytics => "record_on_analytics",
            RecordingEventType::External => "record_on_external",
            _ => return Ok(Vec::new()), // Continuous and Manual aren't event types
        };
        
        let query = format!(
            r#"
            SELECT id, camera_id, stream_id, name, enabled, days_of_week, start_time, end_time,
                   created_at, updated_at, retention_days, record_on_motion, record_on_audio,
                   record_on_analytics, record_on_external, continuous_recording
            FROM recording_schedules
            WHERE enabled = true
            AND stream_id = $1
            AND {} = true
            AND $2 = ANY(days_of_week)
            AND start_time <= $3
            AND end_time >= $3
            "#,
            event_field
        );
        
        let schedules = sqlx::query_as::<_, crate::db::models::recording_schedule_models::RecordingScheduleDb>(&query)
            .bind(stream_id)
            .bind(day_of_week)
            .bind(current_time)
            .fetch_all(&*self.recordings_repo.pool)
            .await?
            .into_iter()
            .map(crate::db::models::recording_schedule_models::RecordingSchedule::from)
            .collect();
        
        Ok(schedules)
    }
    
    /// Mark an event as completed
    pub async fn event_completed(&self, stream_id: &Uuid, event_type: RecordingEventType) -> Result<()> {
        let stream_key = stream_id.to_string();
        let now = Utc::now();
        
        // Update the event time in the active events map with expiration time (now + 5 seconds)
        let expiration_time = now + chrono::Duration::seconds(5);
        {
            let mut active_events = self.active_events.lock().await;
            active_events.insert(format!("{}-{}", stream_key, event_type.to_string()), expiration_time);
        }
        
        info!("Event {} completed for stream {}, recording will continue for 5 more seconds", 
              event_type.to_string(), stream_id);
        
        Ok(())
    }
    
    /// Check if there are active events requiring recording for this stream
    pub async fn has_active_events(&self, stream_id: &Uuid) -> bool {
        let stream_key = stream_id.to_string();
        let now = Utc::now();
        
        let active_events = self.active_events.lock().await;
        
        // Check if there's any unexpired event for this stream
        for (key, expiration_time) in active_events.iter() {
            if key.starts_with(&format!("{}-", stream_key)) && expiration_time > &now {
                return true;
            }
        }
        
        false
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
                    fps: 0, // Not available from pipeline
                    event_type: recording.event_type,
                    segment_id: None,
                    parent_recording_id: None,
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
                    event_type: recording.event_type,
                    segment_id: None,
                    parent_recording_id: None,
                }
            })
    }

    pub fn log_metadata_stream(&self, stream_id: &str) -> Result<()> {
        // Get access to the pipeline and tees
        let (pipeline, _video_tee, _audio_tee, metadata_tee) = self
            .stream_manager
            .get_stream_access(stream_id)
            .map_err(|e| {
                error!("Failed to get video stream access: {}", e);
                anyhow!("Failed to get video stream access: {}", e)
            })?;

        // Create elements for the metadata branch
        let queue = gst::ElementFactory::make("queue")
            .name(&format!("metadata_logger_queue_{}", stream_id))
            .build()?;

        let depay = gst::ElementFactory::make("rtponvifmetadatadepay")
            .name(&format!("metadata_logger_depay_{}", stream_id))
            .build()?;

        // Create a sink that will handle the metadata
        let sink = gst::ElementFactory::make("appsink")
            .name(&format!("metadata_sink_{}", stream_id))
            .property("emit-signals", &true)
            .property("sync", &false)
            .build()?;

        // Add all elements to the pipeline
        pipeline.add_many(&[&queue, &depay, &sink])?;

        // Link the elements together
        gst::Element::link_many(&[&queue, &depay, &sink])?;

        // Request a pad from the tee and link it to our queue
        let tee_src_pad = metadata_tee
            .request_pad_simple("src_%u")
            .ok_or_else(|| anyhow!("Failed to get source pad from metadata tee"))?;

        let queue_sink_pad = queue
            .static_pad("sink")
            .ok_or_else(|| anyhow!("Failed to get sink pad from queue"))?;

        tee_src_pad.link(&queue_sink_pad)?;

        // Get the appsink element and connect to new-sample signal
        let appsink = sink.dynamic_cast::<AppSink>().unwrap();

        // Import the necessary types for metadata processing
        // Create clones of necessary data that will be moved into the callback
        let recording_manager = self.clone();
        let stream_id_clone = stream_id.to_string();

        appsink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    // Pull the sample from the sink
                    let sample = match appsink.pull_sample() {
                        Ok(sample) => sample,
                        Err(e) => {
                            error!("Error pulling sample: {}", e);
                            return Err(gst::FlowError::Eos);
                        }
                    };

                    // Extract the buffer from the sample
                    let buffer = match sample.buffer() {
                        Some(buffer) => buffer,
                        None => {
                            error!("Received sample with no buffer");
                            return Ok(gst::FlowSuccess::Ok);
                        }
                    };

                    // Map the buffer to read its contents
                    let map = match buffer.map_readable() {
                        Ok(map) => map,
                        Err(_) => {
                            error!("Failed to map buffer");
                            return Ok(gst::FlowSuccess::Ok);
                        }
                    };

                    // Convert the buffer data to a string if it's XML
                    match std::str::from_utf8(&map) {
                        Ok(metadata_str) => {
                            info!("Received metadata: {}", metadata_str);
                            
                            // Write metadata to file in a proper location
                            let metadata_dir = crate::utils::metadataparser::get_metadata_path();
                            std::fs::create_dir_all(&metadata_dir).map_err(|_e| {
                                println!("Error creating metadata directory");
                                gstreamer::FlowError::Error
                            })?;
                            
                            let metadata_file = metadata_dir.join(format!("{}-metadata.xml", stream_id_clone));
                            let file = OpenOptions::new()
                                .write(true)
                                .append(true)
                                .create(true)
                                .open(metadata_file)
                                .map_err(|e| {
                                    println!("Error creating file for onvif-metadata: {}", e);
                                    gstreamer::FlowError::Error
                                })?;
                            let mut buf_writer = BufWriter::new(file);

                            buf_writer.write_all(&map).map_err(|_e| {
                                println!("Error creating file for onvif-metadata");
                                gstreamer::FlowError::Error
                            })?;

                            // Parse the ONVIF event metadata
                            match parse_onvif_event(metadata_str) {
                                Ok(metadata) => {
                                    println!(
                                        "Parsed Event: {:#?}, active: {:#?}",
                                        metadata.event_type,
                                        metadata.is_active.unwrap_or(false)
                                    );
                                    
                                    // Handle specific event types from camera
                                    if let Some(is_active) = metadata.is_active {
                                        if let Some(_camera_id) = metadata.camera_id.clone() {
                                            if let Some(stream_id) = metadata.stream_id.clone() {
                                                let stream_uuid = uuid::Uuid::parse_str(&stream_id).unwrap_or_default();
                                                let recording_manager_clone = recording_manager.clone();
                                                
                                                // Handle motion events
                                                if matches!(metadata.event_type, crate::utils::metadataparser::EventType::MotionDetected) {
                                                    if is_active {
                                                        // Motion started
                                                        tokio::spawn(async move {
                                                            if let Err(e) = recording_manager_clone.register_event(&stream_uuid, RecordingEventType::Motion).await {
                                                                eprintln!("Failed to register motion event: {}", e);
                                                            }
                                                        });
                                                    } else {
                                                        // Motion ended
                                                        tokio::spawn(async move {
                                                            if let Err(e) = recording_manager_clone.event_completed(&stream_uuid, RecordingEventType::Motion).await {
                                                                eprintln!("Failed to complete motion event: {}", e);
                                                            }
                                                        });
                                                    }
                                                } 
                                                // Handle audio events
                                                else if matches!(metadata.event_type, crate::utils::metadataparser::EventType::AudioDetected) {
                                                    let recording_manager_clone = recording_manager.clone();
                                                    if is_active {
                                                        // Audio started
                                                        tokio::spawn(async move {
                                                            if let Err(e) = recording_manager_clone.register_event(&stream_uuid, RecordingEventType::Audio).await {
                                                                eprintln!("Failed to register audio event: {}", e);
                                                            }
                                                        });
                                                    } else {
                                                        // Audio ended
                                                        tokio::spawn(async move {
                                                            if let Err(e) = recording_manager_clone.event_completed(&stream_uuid, RecordingEventType::Audio).await {
                                                                eprintln!("Failed to complete audio event: {}", e);
                                                            }
                                                        });
                                                    }
                                                } 
                                                // Handle analytics events
                                                else if matches!(metadata.event_type, 
                                                     crate::utils::metadataparser::EventType::LineDetected |
                                                     crate::utils::metadataparser::EventType::FieldDetected |
                                                     crate::utils::metadataparser::EventType::FaceDetected |
                                                     crate::utils::metadataparser::EventType::ObjectDetected) {
                                                    let recording_manager_clone = recording_manager.clone();
                                                    if is_active {
                                                        // Analytics event started
                                                        tokio::spawn(async move {
                                                            if let Err(e) = recording_manager_clone.register_event(&stream_uuid, RecordingEventType::Analytics).await {
                                                                eprintln!("Failed to register analytics event: {}", e);
                                                            }
                                                        });
                                                    } else {
                                                        // Analytics event ended
                                                        tokio::spawn(async move {
                                                            if let Err(e) = recording_manager_clone.event_completed(&stream_uuid, RecordingEventType::Analytics).await {
                                                                eprintln!("Failed to complete analytics event: {}", e);
                                                            }
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                                Err(e) => {
                                    println!("Failed to parse ONVIF event: {}", e);
                                }
                            }
                        }
                        Err(_) => {
                            // If it's not UTF-8 (could be binary format like KLV)
                            debug!("Received binary metadata of size: {} bytes", map.len());
                        }
                    }

                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        // Make sure these elements start in the right state
        queue.sync_state_with_parent()?;
        depay.sync_state_with_parent()?;
        appsink.sync_state_with_parent()?;

        info!("Metadata logging started for stream {}", stream_id);

        // Return success
        Ok(())
    }
}
