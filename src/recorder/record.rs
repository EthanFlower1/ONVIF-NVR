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
use log::{error, info, warn};
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

        // Get codec info from stream
        let video_codec = stream.codec.clone().unwrap_or_default();
        let audio_codec = stream.audio_codec.clone().unwrap_or_default();

        info!(
            "Starting recording with video codec: {}, audio codec: {}",
            video_codec, audio_codec
        );

        let recording_id = Uuid::new_v4();
        let now = Utc::now();

        // Create directory structure with date-based hierarchy
        let date_path = now.format("%Y/%m/%d/%H").to_string();
        let camera_path = format!("{}", stream.name);
        let mut dir_path = self.recording_base_path.join(&camera_path).join(&date_path);

        // Create parent directories with better error handling
        info!("Creating recording directory structure: {:?}", dir_path);
        match std::fs::create_dir_all(&dir_path) {
            Ok(_) => {
                info!("Successfully created directory: {:?}", dir_path);

                #[cfg(target_os = "macos")]
                {
                    // Ensure we have the absolute path on macOS to avoid path resolution issues
                    if let Ok(abs_path) = std::fs::canonicalize(&dir_path) {
                        info!("MacOS: Using absolute path: {:?}", abs_path);
                        // Replace dir_path with absolute path
                        dir_path = abs_path;
                    }
                }
            }
            Err(e) => {
                error!("Failed to create directory {:?}: {}", dir_path, e);
                return Err(anyhow!(
                    "Failed to create recording directory {:?}: {}",
                    dir_path,
                    e
                ));
            }
        };

        // Create filename with timestamp
        let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
        let file_name = format!("cam{}_{}.{}", stream.name, timestamp, self.format);
        let file_path = dir_path.join(&file_name);

        info!("Using recording file path: {:?}", file_path);

        // Get access to the MAIN PIPELINE and VIDEO TEE
        let (pipeline, video_tee, audio_tee) = self
            .stream_manager
            .get_stream_access(&stream.id.to_string())
            .map_err(|e| {
                error!("Failed to get video stream access: {}", e);
                anyhow!("Failed to get video stream access: {}", e)
            })?;

        // Generate unique key for this recording
        let element_suffix = recording_id.to_string().replace("-", "");

        //-----------------------------------------------------------------------------
        // MUXER & SPLITMUXSINK SETUP - Using the same format for all codecs
        //-----------------------------------------------------------------------------
        let muxer = {
            let factory_name = if self.format == "mp4" {
                "mp4mux"
            } else {
                "matroskamux"
            };

            let element = gst::ElementFactory::make(factory_name)
                .name(format!("record_muxer_{}", element_suffix))
                .property("faststart", true)
                .property("streamable", true);

            let element = element.property("fragment-duration", 3000_u32);

            element.build()?
        };

        // Use splitmuxsink for segment-based recording with platform optimizations
        let splitmuxsink = {
            let mut builder = gst::ElementFactory::make("splitmuxsink")
                .name(format!("record_splitmuxsink_{}", element_suffix))
                .property(
                    "location",
                    format!(
                        "{}/segment_%05d.{}",
                        dir_path.to_str().unwrap(),
                        self.format
                    ),
                )
                .property("max-size-bytes", 0u64)
                .property("muxer", &muxer);

            #[cfg(target_os = "macos")]
            {
                info!("macOS detected: using longer segment duration for stability");
                builder = builder.property(
                    "max-size-time",
                    gst::ClockTime::from_seconds((self.segment_duration * 3 / 2) as u64),
                );
                builder = builder.property("send-keyframe-requests", true);
                builder = builder.property("async-finalize", true);
                builder = builder.property("max-files", 0u32);
            }

            {
                builder = builder.property(
                    "max-size-time",
                    gst::ClockTime::from_seconds(self.segment_duration as u64),
                );
            }

            builder.build()?
        };

        // Setup segment location signal handler (for database tracking)
        // [Code for signal handler remains the same]

        //-----------------------------------------------------------------------------
        // VIDEO PROCESSING CHAIN SETUP
        //-----------------------------------------------------------------------------
        let video_queue = gst::ElementFactory::make("queue")
            .name(format!("record_queue_{}", element_suffix))
            .property("max-size-buffers", 0u32)
            .property("max-size-time", 0u64)
            .property("max-size-bytes", 0u32)
            .build()?;

        // Create appropriate video depayloaders based on codec
        let (video_depay, video_parse) = match video_codec.to_lowercase().as_str() {
            "h264" => {
                let depay = gst::ElementFactory::make("rtph264depay")
                    .name(format!("record_depay_{}", element_suffix))
                    .build()?;

                let parse = gst::ElementFactory::make("h264parse")
                    .name(format!("record_parse_{}", element_suffix))
                    .property("config-interval", -1)
                    .build()?;

                (depay, parse)
            }
            "h265" | "hevc" => {
                let depay = gst::ElementFactory::make("rtph265depay")
                    .name(format!("record_depay_{}", element_suffix))
                    .build()?;

                let parse = gst::ElementFactory::make("h265parse")
                    .name(format!("record_parse_{}", element_suffix))
                    .property("config-interval", -1)
                    .build()?;

                (depay, parse)
            }
            "jpeg" | "mjpeg" => {
                let depay = gst::ElementFactory::make("rtpjpegdepay")
                    .name(format!("record_depay_{}", element_suffix))
                    .build()?;

                let parse = gst::ElementFactory::make("jpegparse")
                    .name(format!("record_parse_{}", element_suffix))
                    .build()?;

                (depay, parse)
            }
            "vp8" => {
                let depay = gst::ElementFactory::make("rtpvp8depay")
                    .name(format!("record_depay_{}", element_suffix))
                    .build()?;

                // VP8 doesn't need a parser in the same way H.264 does
                let parse = gst::ElementFactory::make("identity")
                    .name(format!("record_parse_{}", element_suffix))
                    .build()?;

                (depay, parse)
            }
            "vp9" => {
                let depay = gst::ElementFactory::make("rtpvp9depay")
                    .name(format!("record_depay_{}", element_suffix))
                    .build()?;

                // VP9 doesn't need a parser in the same way H.264 does
                let parse = gst::ElementFactory::make("identity")
                    .name(format!("record_parse_{}", element_suffix))
                    .build()?;

                (depay, parse)
            }
            _ => {
                // Default to H264 if unknown
                warn!("Unknown video codec: {}. Defaulting to H264", video_codec);
                let depay = gst::ElementFactory::make("rtph264depay")
                    .name(format!("record_depay_{}", element_suffix))
                    .build()?;

                let parse = gst::ElementFactory::make("h264parse")
                    .name(format!("record_parse_{}", element_suffix))
                    .property("config-interval", -1)
                    .build()?;

                (depay, parse)
            }
        };

        //-----------------------------------------------------------------------------
        // AUDIO PROCESSING CHAIN SETUP
        //-----------------------------------------------------------------------------
        let audio_elements = if !audio_codec.is_empty() {
            info!("Setting up audio processing for codec: {}", audio_codec);

            info!("Successfully retrieved audio_tee from stream manager");

            // Create audio elements based on codec
            let audio_queue = gst::ElementFactory::make("queue")
                .name(format!("record_audio_queue_{}", element_suffix))
                .property("max-size-buffers", 0u32)
                .property("max-size-time", 0u64)
                .property("max-size-bytes", 0u32)
                .build()?;

            // Create appropriate depayloader for the codec
            info!("AUDIO CODEC ++++++++++++>>>>>>>>>>>>>> {}", audio_codec);
            let audio_depay = match audio_codec.to_lowercase().as_str() {
                "pcmu" => {
                    info!("Using rtppcmudepay for PCMU codec");
                    gst::ElementFactory::make("rtppcmudepay")
                        .name(format!("record_audio_depay_{}", element_suffix))
                        .build()?
                }
                "pcma" => {
                    info!("Using rtppcmadepay for PCMA codec");
                    gst::ElementFactory::make("rtppcmadepay")
                        .name(format!("record_audio_depay_{}", element_suffix))
                        .build()?
                }
                "G711" => {
                    info!("Using rtppcmudepay for PCMA codec");
                    gst::ElementFactory::make("rtppcmudepay")
                        .name(format!("record_audio_depay_{}", element_suffix))
                        .build()?
                }
                "aac" => {
                    info!("Using rtpaacdepay for AAC codec");
                    gst::ElementFactory::make("rtpaacdepay")
                        .name(format!("record_audio_depay_{}", element_suffix))
                        .build()?
                }
                "mp4a-latm" => {
                    info!("Using rtpmp4adepay for MP4A-LATM codec");
                    gst::ElementFactory::make("rtpmp4adepay")
                        .name(format!("record_audio_depay_{}", element_suffix))
                        .build()?
                }
                _ => {
                    warn!("Unknown audio codec: {}. Defaulting to PCMU", audio_codec);
                    gst::ElementFactory::make("rtppcmudepay")
                        .name(format!("record_audio_depay_{}", element_suffix))
                        .build()?
                }
            };

            // Create decoder based on the codec
            let audio_decoder = match audio_codec.to_lowercase().as_str() {
                "pcmu" => {
                    info!("Using mulawdec for PCMU codec");
                    gst::ElementFactory::make("mulawdec")
                        .name(format!("record_audio_decoder_{}", element_suffix))
                        .build()?
                }
                "G711" => {
                    info!("Using mulawdec for G711 codec");
                    gst::ElementFactory::make("mulawdec")
                        .name(format!("record_audio_decoder_{}", element_suffix))
                        .build()?
                }
                "pcma" => {
                    info!("Using alawdec for PCMA codec");
                    gst::ElementFactory::make("alawdec")
                        .name(format!("record_audio_decoder_{}", element_suffix))
                        .build()?
                }
                "aac" | "mp4a-latm" => {
                    info!("Using avdec_aac for AAC codec");
                    gst::ElementFactory::make("avdec_aac")
                        .name(format!("record_audio_decoder_{}", element_suffix))
                        .build()?
                }
                _ => {
                    info!("Using mulawdec as default audio decoder");
                    gst::ElementFactory::make("mulawdec")
                        .name(format!("record_audio_decoder_{}", element_suffix))
                        .build()?
                }
            };

            // Add standard audio processing elements
            let audio_convert = gst::ElementFactory::make("audioconvert")
                .name(format!("record_audio_convert_{}", element_suffix))
                .build()?;

            let audio_resample = gst::ElementFactory::make("audioresample")
                .name(format!("record_audio_resample_{}", element_suffix))
                .build()?;

            // Add audio encoder suitable for the container format
            let audio_encoder = gst::ElementFactory::make("avenc_aac")
                .name(format!("record_audio_encoder_{}", element_suffix))
                .property("bitrate", 128000i32)
                .build()?;

            // Add audio parser for the encoded format
            let audio_parse = gst::ElementFactory::make("aacparse")
                .name(format!("record_audio_parse_{}", element_suffix))
                .build()?;

            Some((
                audio_tee,
                audio_queue,
                audio_depay,
                audio_decoder,
                audio_convert,
                audio_resample,
                audio_encoder,
                audio_parse,
            ))
        } else {
            info!("No audio codec detected, skipping audio processing");
            None
        };

        //-----------------------------------------------------------------------------
        // PIPELINE ASSEMBLY
        //-----------------------------------------------------------------------------
        // Create a list to hold all elements for adding to pipeline
        let mut elements = vec![
            video_queue.clone(),
            video_depay.clone(),
            video_parse.clone(),
            splitmuxsink.clone(),
        ];

        // Add audio elements to pipeline if available
        if let Some((
            _,
            audio_queue,
            audio_depay,
            audio_decoder,
            audio_convert,
            audio_resample,
            audio_encoder,
            audio_parse,
        )) = &audio_elements
        {
            elements.push(audio_queue.clone());
            elements.push(audio_depay.clone());
            elements.push(audio_decoder.clone());
            elements.push(audio_convert.clone());
            elements.push(audio_resample.clone());
            elements.push(audio_encoder.clone());
            elements.push(audio_parse.clone());
        }

        // Add all elements to the pipeline
        pipeline.add_many(&elements)?;

        //-----------------------------------------------------------------------------
        // LINKING ELEMENTS
        //-----------------------------------------------------------------------------
        // 1. Link video processing chain
        info!(
            "Linking video elements: {} -> {} -> {}",
            video_queue.name(),
            video_depay.name(),
            video_parse.name()
        );
        if let Err(e) = gst::Element::link_many(&[&video_queue, &video_depay, &video_parse, &splitmuxsink]) {
            error!("Failed to link video elements: {}", e);
            return Err(anyhow!("Failed to link video elements: {}", e));
        }
        info!("Successfully linked video elements");

        // 2. Link audio processing chain if available
        if let Some((
            _audio_tee,
            _audio_queue,
            audio_depay,
            audio_decoder,
            audio_convert,
            audio_resample,
            audio_encoder,
            audio_parse,
        )) = &audio_elements
        {
            info!("Linking audio processing chain");

            // Link audio processing elements in sequence
            if let Err(e) = gst::Element::link_many(&[
                audio_depay,
                audio_decoder,
                audio_convert,
                audio_resample,
                audio_encoder,
                audio_parse,
            ]) {
                error!("Failed to link audio processing elements: {}", e);
                info!("Continuing without audio due to linking failure");
            } else {
                info!("Successfully linked audio processing elements");
            }
        }

        // 3. Link video to splitmuxsink
        // info!("Linking video parse to splitmuxsink");
        // if let Err(e) = video_parse.link_pads(Some("src"), &splitmuxsink, Some("video")) {
        //     error!("Failed to link video parse to splitmuxsink: {}", e);
        //
        //     // Try requesting a video pad directly
        //     info!("Trying with request_pad on splitmuxsink");
        //     if let Some(sink_pad) = splitmuxsink.request_pad_simple("video") {
        //         let src_pad = video_parse.static_pad("src").unwrap();
        //         if let Err(e) = src_pad.link(&sink_pad) {
        //             error!("Failed to link video with requested pad: {}", e);
        //             return Err(anyhow!("Failed to link video pipeline: {}", e));
        //         }
        //         info!("Successfully linked video using requested pad");
        //     } else {
        //         return Err(anyhow!("Failed to link video pipeline: {}", e));
        //     }
        // } else {
        //     info!("Successfully linked video parse to splitmuxsink");
        // }

        // 4. Link audio to splitmuxsink if available
        if let Some((_, _, _, _, _, _, _, audio_parse)) = &audio_elements {
            info!("Linking audio parse to splitmuxsink");

            if let Some(sink_pad) = splitmuxsink.request_pad_simple("audio_%u") {
                if let Some(src_pad) = audio_parse.static_pad("src") {
                    if let Err(e) = src_pad.link(&sink_pad) {
                        error!("Failed to link audio parse to splitmuxsink: {}", e);
                        info!("Continuing without audio due to sink pad linking failure");
                    } else {
                        info!("Successfully linked audio parse to splitmuxsink");
                    }
                }
            } else {
                error!("Failed to get audio sink pad from splitmuxsink");
                info!("Continuing without audio due to sink pad request failure");
            }
        }

        // 5. Connect the video tee to the video queue
        info!("Connecting video tee to video queue");
        let tee_src_pad = video_tee.request_pad_simple("src_%u").unwrap();
        tee_src_pad.link(&video_queue);

        // if let Some(tee_src_pad) = video_tee.request_pad_simple("src_%u") {
        //     if let Some(queue_sink_pad) = video_queue.static_pad("sink") {
        //         if let Err(e) = tee_src_pad.link(&queue_sink_pad) {
        //             error!("Failed to link video tee to queue: {}", e);
        //             return Err(anyhow!("Failed to link video tee to queue: {}", e));
        //         }
        //         info!("Successfully linked video tee to queue");
        //     } else {
        //         error!("Failed to get video queue sink pad");
        //         return Err(anyhow!("Failed to get video queue sink pad"));
        //     }
        // } else {
        //     error!("Failed to request video tee src pad");
        //     return Err(anyhow!("Failed to request video tee src pad"));
        // }

        // 6. Connect the audio tee to the audio queue if available
        if let Some((audio_tee, audio_queue, _, _, _, _, _, _)) = &audio_elements {
            info!("Connecting audio tee to audio queue");

            if let Some(tee_src_pad) = audio_tee.request_pad_simple("src_%u") {
                if let Some(queue_sink_pad) = audio_queue.static_pad("sink") {
                    if let Err(e) = tee_src_pad.link(&queue_sink_pad) {
                        error!("Failed to link audio tee to queue: {}", e);
                        info!("Continuing without audio due to tee linking failure");
                    } else {
                        info!("Successfully linked audio tee to queue");
                    }
                } else {
                    error!("Failed to get audio queue sink pad");
                    info!("Continuing without audio due to pad retrieval failure");
                }
            } else {
                error!("Failed to request audio tee src pad");
                info!("Continuing without audio due to tee pad request failure");
            }
        }

        //-----------------------------------------------------------------------------
        // ELEMENT STATE SYNCHRONIZATION
        //-----------------------------------------------------------------------------
        // Sync all elements with the pipeline state
        for element in &elements {
            info!("Syncing state for element: {}", element.name());
            if let Err(e) = element.sync_state_with_parent() {
                error!("Failed to sync state for element {}: {}", element.name(), e);
                // Continue anyway, the element might not be critical
            }
        }

        //-----------------------------------------------------------------------------
        // PIPELINE STATE MANAGEMENT
        //-----------------------------------------------------------------------------
        // Platform-specific preroll timing
        #[cfg(target_os = "macos")]
        {
            info!("macOS detected: extra wait time for preroll");
            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        // Set pipeline to Playing state
        info!(
            "Setting pipeline to Playing state for recording {}",
            recording_id
        );
        let _state_change_result = pipeline.set_state(gst::State::Playing);

        // Wait for pipeline to fully transition to Playing state
        sleep(Duration::from_secs(1)).await;

        // Verify the pipeline state
        let (_, current_state, _) = pipeline.state(gst::ClockTime::from_seconds(1));
        info!(
            "Pipeline state after waiting: {:?} for recording {}",
            current_state, recording_id
        );

        if current_state != gst::State::Playing {
            warn!(
                "Pipeline is not in Playing state, current state: {:?}",
                current_state
            );
            // Try one more time
            info!(
                "Retrying to set pipeline to Playing state for recording {}",
                recording_id
            );
            let _retry_result = pipeline.set_state(gst::State::Playing);
        }

        let bus = pipeline.bus().unwrap();
        let recordings_repo_clone = self.recordings_repo.clone();
        let recording_id_clone = recording_id;

        // Clone for bus watch
        let recordings_repo_for_watch = recordings_repo_clone.clone();
        
        let watch_id = bus.add_watch(move |_, msg| {
            match msg.view() {
                gst::MessageView::Element(element_msg) => {
                    if let Some(structure) = element_msg.structure() {
                        let name = structure.name();
                        
                        if name == "splitmuxsink-fragment-closed" {
                            // Extract values from the message
                            if let (Ok(fragment_id), Ok(location), Ok(fragment_duration)) = (
                                structure.get::<u32>("fragment-id"),
                                structure.get::<String>("location"),
                                structure.get::<u64>("fragment-duration")
                            ) {
                                info!(
                                    "Fragment {} closed: location={}, duration={}ns", 
                                    fragment_id, location, fragment_duration
                                );
                                
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
                                let recordings_repo_for_thread = recordings_repo_for_watch.clone();
                                let location_clone = location.clone();
                                let fragment_id_clone = fragment_id;
                                let fragment_duration_clone = fragment_duration;
                                let duration_ms_clone = duration_ms;
                                let file_size_clone = file_size;
                                
                                std::thread::spawn(move || {
                                    // Create a runtime for this thread
                                    let rt = tokio::runtime::Builder::new_current_thread()
                                        .enable_all()
                                        .build()
                                        .expect("Failed to create Tokio runtime");
                                    
                                    rt.block_on(async {
                                        // First, fetch the recording entry by parent_recording_id and segment_id
                                        match recordings_repo_for_thread.get_segment(&location_clone).await {
                                            Ok(Some(mut recording)) => {
                                                // Update the recording with final values
                                                recording.file_size = file_size_clone;
                                                recording.duration = duration_ms_clone;
                                                
                                                // Update the metadata to mark it as finalized
                                                if let Some(metadata) = recording.metadata {
                                                    let mut metadata_map = metadata.as_object().unwrap().clone();
                                                    metadata_map.insert("status".to_string(), json!("completed"));
                                                    metadata_map.insert("finalized".to_string(), json!(true));
                                                    metadata_map.insert("finalize_time".to_string(), 
                                                        json!(chrono::Utc::now().to_rfc3339()));
                                                    metadata_map.insert("fragment_duration_ns".to_string(), 
                                                        json!(fragment_duration_clone));
                                                    
                                                    recording.metadata = Some(json!(metadata_map));
                                                }
                                                
                                                // Calculate end time based on fragment duration
                                                recording.end_time = Some(
                                                    recording.start_time + chrono::Duration::milliseconds(duration_ms_clone as i64)
                                                );
                                                
                                                // Update the recording in the database
                                                if let Err(e) = recordings_repo_for_thread.update(&recording).await {
                                                    error!(
                                                        "Failed to update recording for fragment {}: {}", 
                                                        fragment_id_clone, e
                                                    );
                                                } else {
                                                    info!(
                                                        "Updated recording for fragment {}, duration={}ms, size={}bytes", 
                                                        fragment_id_clone, duration_ms_clone, file_size_clone
                                                    );
                                                }
                                            },
                                            Ok(None) => {
                                                error!(
                                                    "Could not find recording entry for fragment {}", 
                                                    fragment_id_clone
                                                );
                                            },
                                            Err(e) => {
                                                error!(
                                                    "Error finding recording for fragment {}: {}", 
                                                    fragment_id_clone, e
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
                _ => {}
            }
            glib::ControlFlow::Continue
        })?;

        // Create and store active recording info
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
            segment_id: None,
            parent_recording_id: None,
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
        // [Code for publishing event remains the same]

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

        info!(
            "Removed recording elements from pipeline for {}",
            active_recording.recording_id
        );

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

                // Additional validation that files exist
                for (i, path) in segment_files.iter().enumerate().take(5) {
                    if path.exists() {
                        if let Ok(metadata) = std::fs::metadata(path) {
                            info!(
                                "Segment file {} exists: {:?}, size: {} bytes",
                                i,
                                path,
                                metadata.len()
                            );
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
                    info!("... and {} more segment files", segment_files.len() - 5);
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

