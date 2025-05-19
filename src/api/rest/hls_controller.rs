use crate::api::rest::AppState;
use crate::db::models::recording_models::Recording;
use axum::body::StreamBody;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use log::{error, info, warn};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path as FilePath, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;
use uuid::Uuid;
// GStreamer imports - used for GStreamer HLS generation approach
use gstreamer as gst;
use gstreamer::prelude::*;

/// Simple in-memory cache to avoid regenerating segments too frequently
#[derive(Default)]
struct HlsCache {
    /// Cache of initialization segments
    init_segments: HashMap<String, (PathBuf, DateTime<Utc>)>,
    /// Cache of video segments
    video_segments: HashMap<String, (PathBuf, DateTime<Utc>)>,
}

impl HlsCache {

    /// Clean expired cache entries (older than 10 minutes)
    fn clean_expired(&mut self) {
        let now = Utc::now();
        let max_age = Duration::minutes(10);

        // Clean init segments
        self.init_segments.retain(|_, (_, timestamp)| {
            now.signed_duration_since(*timestamp) < max_age
        });

        // Clean video segments
        self.video_segments.retain(|_, (_, timestamp)| {
            now.signed_duration_since(*timestamp) < max_age
        });
    }

    /// Get an init segment from cache if it exists and is fresh
    fn get_init_segment(&self, key: &str) -> Option<PathBuf> {
        if let Some((path, timestamp)) = self.init_segments.get(key) {
            let now = Utc::now();
            if now.signed_duration_since(*timestamp) < Duration::minutes(10) {
                return Some(path.clone());
            }
        }
        None
    }

    /// Store an init segment in cache
    fn store_init_segment(&mut self, key: String, path: PathBuf) {
        self.init_segments.insert(key, (path, Utc::now()));
    }

    /// Get a video segment from cache if it exists and is fresh
    fn get_video_segment(&self, key: &str) -> Option<PathBuf> {
        if let Some((path, timestamp)) = self.video_segments.get(key) {
            let now = Utc::now();
            if now.signed_duration_since(*timestamp) < Duration::minutes(10) {
                return Some(path.clone());
            }
        }
        None
    }

    /// Store a video segment in cache
    fn store_video_segment(&mut self, key: String, path: PathBuf) {
        self.video_segments.insert(key, (path, Utc::now()));
    }
}

/// Parameter for HLS segment requests
#[derive(Debug, Deserialize)]
pub struct HlsSegmentParams {
    pub start_time: Option<f64>,
    pub duration: Option<f64>,
}

/// Parameter for HLS playlist requests
#[derive(Debug, Deserialize, Default)]
pub struct HlsPlaylistParams {
    pub playlist_type: Option<String>,
    pub segment_duration: Option<f64>,
}

/// HLS controller state
#[derive(Clone)]
pub struct HlsControllerState {
    pub app_state: AppState,
    pub cache: Arc<Mutex<HlsCache>>,
    pub temp_dir: PathBuf,
}

impl HlsControllerState {
    pub fn new(app_state: AppState) -> Self {
        // Create temporary directory for on-the-fly generated segments
        let temp_dir = std::env::temp_dir().join("g-streamer-hls");
        if !temp_dir.exists() {
            std::fs::create_dir_all(&temp_dir).expect("Failed to create temporary HLS directory");
        }

        Self {
            app_state,
            cache: Arc::new(Mutex::new(HlsCache::default())),
            temp_dir,
        }
    }
}

/// Generate a complete HLS playlist with segments for all recordings of a camera
async fn generate_camera_hls(
    camera_id: &Uuid,
    recordings: &[Recording],
    output_dir: &FilePath,
) -> Result<(), anyhow::Error> {
    info!("Generating complete HLS playlist for camera: {}", camera_id);
    
    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }
    
    // Determine the path for the main playlist and segments
    let playlist_path = output_dir.join("playlist.m3u8");
    let segments_pattern = output_dir.join("segment%03d.ts");
    
    // If we don't have any recordings, return an error
    if recordings.is_empty() {
        return Err(anyhow::anyhow!("No recordings found for camera {}", camera_id));
    }
    
    // Create a temporary file to list all recording files for FFmpeg
    let input_list_path = output_dir.join("input_list.txt");
    let mut input_list_content = String::new();
    
    // Sort recordings chronologically
    let mut sorted_recordings = recordings.to_vec();
    sorted_recordings.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    
    // Build the input list file with all recordings
    for recording in &sorted_recordings {
        input_list_content.push_str(&format!("file '{}'\n", recording.file_path.to_string_lossy().replace("'", "\\'")));
    }
    
    // Write the input list file
    std::fs::write(&input_list_path, input_list_content)?;
    
    // Use FFmpeg to concatenate all recordings and create HLS playlist
    let status = Command::new("ffmpeg")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")  // Allow absolute paths
        .arg("-i")
        .arg(&input_list_path) // Input file list
        // Try to copy codecs if possible for better performance
        .arg("-c")
        .arg("copy")
        // HLS output settings
        .arg("-f")
        .arg("hls") // Output format is HLS
        .arg("-hls_time")
        .arg("4") // 4-second segments
        .arg("-hls_list_size")
        .arg("0") // Keep all segments in the playlist
        .arg("-hls_segment_type")
        .arg("mpegts") // Use MPEG-TS for segments
        .arg("-hls_segment_filename")
        .arg(&segments_pattern) // Pattern for segment files
        // Output path for the playlist
        .arg(&playlist_path)
        .stderr(Stdio::inherit())
        .status()?;
        
    if !status.success() {
        error!("Failed to generate HLS with concat+copy, trying with re-encoding");
        
        // If direct concatenation fails, try with re-encoding
        let fallback_status = Command::new("ffmpeg")
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")  // Allow absolute paths
            .arg("-i")
            .arg(&input_list_path) // Input file list
            // Explicit transcoding settings
            .arg("-c:v")
            .arg("libx264") // H.264 video codec
            .arg("-profile:v")
            .arg("baseline") // Use baseline profile for maximum compatibility
            .arg("-level")
            .arg("3.0")
            .arg("-preset")
            .arg("superfast") // Fast encoding at slight quality cost
            .arg("-c:a")
            .arg("aac") // AAC audio codec
            .arg("-b:a")
            .arg("128k") // 128kbps audio bitrate
            .arg("-pix_fmt")
            .arg("yuv420p") // Standard pixel format
            // HLS output settings
            .arg("-f")
            .arg("hls") // Output format is HLS
            .arg("-hls_time")
            .arg("4") // 4-second segments
            .arg("-hls_list_size")
            .arg("0") // Keep all segments in the playlist
            .arg("-hls_segment_type")
            .arg("mpegts") // Use MPEG-TS for segments
            .arg("-hls_segment_filename")
            .arg(&segments_pattern) // Pattern for segment files
            // Output path for the playlist
            .arg(&playlist_path)
            .stderr(Stdio::inherit())
            .status()?;
            
        if !fallback_status.success() {
            return Err(anyhow::anyhow!("Failed to generate HLS playlist with multiple methods"));
        }
    }
    
    // Verify the playlist was created
    if !playlist_path.exists() || std::fs::metadata(&playlist_path)?.len() == 0 {
        return Err(anyhow::anyhow!("Failed to create a valid HLS playlist"));
    }
    
    // Clean up the input list file
    let _ = std::fs::remove_file(&input_list_path);

    // Create a master playlist that references the main playlist
    let master_playlist_path = output_dir.join("master.m3u8");
    let master_content = format!(
        "#EXTM3U\n\
        #EXT-X-VERSION:3\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
        playlist.m3u8\n"
    );
    std::fs::write(&master_playlist_path, master_content)?;

    info!("Successfully generated HLS playlists for camera {} at: {}", camera_id, output_dir.display());
    Ok(())
}

/// Generate a complete HLS playlist with segments for a single recording
async fn generate_recording_hls(
    recording: &Recording,
    output_dir: &FilePath,
) -> Result<(), anyhow::Error> {
    info!("Generating HLS playlist for recording: {}", recording.id);
    
    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }
    
    // Determine the path for the main playlist and segments
    let playlist_path = output_dir.join("playlist.m3u8");
    let segments_pattern = output_dir.join("segment%03d.ts");
    
    // Use FFmpeg's direct HLS generation capabilities
    // This will create the master playlist and all segments in one operation
    let status = Command::new("ffmpeg")
        .arg("-i")
        .arg(&recording.file_path) // Input file
        // Try to copy codecs if possible for better performance
        .arg("-c")
        .arg("copy")
        // HLS output settings
        .arg("-f")
        .arg("hls") // Output format is HLS
        .arg("-hls_time")
        .arg("4") // 4-second segments
        .arg("-hls_list_size")
        .arg("0") // Keep all segments in the playlist
        .arg("-hls_segment_type")
        .arg("mpegts") // Use MPEG-TS for segments
        .arg("-hls_segment_filename")
        .arg(&segments_pattern) // Pattern for segment files
        // Output path for the playlist
        .arg(&playlist_path)
        .stderr(Stdio::inherit())
        .status()?;
        
    if !status.success() {
        error!("Failed to generate HLS with codec copy, trying with transcoding");
        
        // If direct copy fails, try with explicit transcoding
        let fallback_status = Command::new("ffmpeg")
            .arg("-i")
            .arg(&recording.file_path) // Input file
            // Explicit transcoding settings
            .arg("-c:v")
            .arg("libx264") // H.264 video codec
            .arg("-profile:v")
            .arg("baseline") // Use baseline profile for maximum compatibility
            .arg("-level")
            .arg("3.0")
            .arg("-preset")
            .arg("superfast") // Fast encoding at slight quality cost
            .arg("-c:a")
            .arg("aac") // AAC audio codec
            .arg("-b:a")
            .arg("128k") // 128kbps audio bitrate
            .arg("-pix_fmt")
            .arg("yuv420p") // Standard pixel format
            // HLS output settings
            .arg("-f")
            .arg("hls") // Output format is HLS
            .arg("-hls_time")
            .arg("4") // 4-second segments
            .arg("-hls_list_size")
            .arg("0") // Keep all segments in the playlist
            .arg("-hls_segment_type")
            .arg("mpegts") // Use MPEG-TS for segments
            .arg("-hls_segment_filename")
            .arg(&segments_pattern) // Pattern for segment files
            // Output path for the playlist
            .arg(&playlist_path)
            .stderr(Stdio::inherit())
            .status()?;
            
        if !fallback_status.success() {
            return Err(anyhow::anyhow!("Failed to generate HLS playlist with multiple methods"));
        }
    }
    
    // Verify the playlist was created
    if !playlist_path.exists() || std::fs::metadata(&playlist_path)?.len() == 0 {
        return Err(anyhow::anyhow!("Failed to create a valid HLS playlist"));
    }

    // Create a master playlist that references the main playlist
    let master_playlist_path = output_dir.join("master.m3u8");
    let master_content = format!(
        "#EXTM3U\n\
        #EXT-X-VERSION:3\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
        playlist.m3u8\n"
    );
    std::fs::write(&master_playlist_path, master_content)?;

    info!("Successfully generated HLS playlist for recording {} at: {}", recording.id, playlist_path.display());
    Ok(())
}

/// Generate HLS playlist and segments using GStreamer
async fn generate_hls_with_gstreamer(
    recording: &Recording, 
    output_dir: &FilePath
) -> Result<(), anyhow::Error> {
    // Initialize GStreamer if needed
    gst::init()?;
    
    let playlist_path = output_dir.join("playlist.m3u8");
    let segments_pattern = output_dir.join("segment_%05d.ts").to_string_lossy().to_string();
    
    // Create a channel for communication between the GStreamer thread and our async function
    let (tx, mut rx) = mpsc::channel::<Result<(), anyhow::Error>>(1);
    
    // Clone values needed for the thread
    let input_path = recording.file_path.to_string_lossy().to_string();
    
    // Create and run the pipeline in a separate thread since GStreamer API is not async
    std::thread::spawn(move || {
        // Build the GStreamer pipeline
        let result = (|| {
            // Create a pipeline with a name
            let pipeline = gst::Pipeline::new();
            
            // Create elements with a more flexible demuxing approach
            let filesrc = gst::ElementFactory::make("filesrc")
                .name("src")
                .property("location", &input_path)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create filesrc: {:?}", e))?;
                
            // Use decodebin to handle any input format automatically
            let decodebin = gst::ElementFactory::make("decodebin")
                .name("decoder")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create decodebin: {:?}", e))?;
                
            // Video encoding elements
            let videoconvert = gst::ElementFactory::make("videoconvert")
                .name("videoconvert")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create videoconvert: {:?}", e))?;
                
            // Try different H.264 encoders
            let h264enc = match gst::ElementFactory::make("x264enc").name("h264enc").build() {
                Ok(encoder) => {
                    // Configure for streaming
                    encoder.set_property("tune", "zerolatency");
                    encoder.set_property("speed-preset", "superfast");
                    encoder.set_property("bitrate", 2000u32); // 2 Mbps
                    encoder
                },
                Err(_) => {
                    // Try alternative encoder (avenc_h264 or nvh264enc)
                    match gst::ElementFactory::make("avenc_h264").name("h264enc").build() {
                        Ok(encoder) => encoder,
                        Err(_) => gst::ElementFactory::make("nvh264enc")
                            .name("h264enc")
                            .build()
                            .map_err(|e| anyhow::anyhow!("Could not find a suitable H.264 encoder: {:?}", e))?
                    }
                }
            };
                
            let h264parse = gst::ElementFactory::make("h264parse")
                .name("h264parse")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create h264parse: {:?}", e))?;
                
            // Audio processing elements
            let audioconvert = gst::ElementFactory::make("audioconvert")
                .name("audioconvert")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create audioconvert: {:?}", e))?;
                
            let audioresample = gst::ElementFactory::make("audioresample")
                .name("audioresample")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create audioresample: {:?}", e))?;
                
            let aacenc = gst::ElementFactory::make("avenc_aac")
                .name("aacenc")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create aac encoder: {:?}", e))?;
                
            let aacparse = gst::ElementFactory::make("aacparse")
                .name("aacparse")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create aacparse: {:?}", e))?;
                
            // Create queues for buffering
            let video_queue = gst::ElementFactory::make("queue")
                .name("video_queue")
                .property("max-size-buffers", 1000u32)
                .property("max-size-time", 0u64)
                .property("max-size-bytes", 0u32)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create video queue: {:?}", e))?;
                
            let audio_queue = gst::ElementFactory::make("queue")
                .name("audio_queue")
                .property("max-size-buffers", 1000u32)
                .property("max-size-time", 0u64)
                .property("max-size-bytes", 0u32)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create audio queue: {:?}", e))?;
                
            // MPEG-TS muxer for segment compatibility with all players
            let mpegtsmux = gst::ElementFactory::make("mpegtsmux")
                .name("mpegtsmux")
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create mpegtsmux: {:?}", e))?;
            
            // HLS sink element to create HLS playlist and segments
            let hlssink = gst::ElementFactory::make("hlssink2")
                .name("hlssink")
                .property("playlist-location", &playlist_path.to_string_lossy().to_string())
                .property("location", &segments_pattern)
                .property("playlist-length", 0u32) // Include all segments
                .property("target-duration", 4u32) // 4-second segments
                .property("max-files", 0u32) // Keep all files
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create hlssink2: {:?}", e))?;
            
            // Add all elements to the pipeline
            pipeline.add_many(&[&filesrc, &decodebin])
                .map_err(|e| anyhow::anyhow!("Failed to add source elements to pipeline: {}", e))?;
                
            pipeline.add_many(&[&videoconvert, &h264enc, &h264parse, &video_queue])
                .map_err(|e| anyhow::anyhow!("Failed to add video elements to pipeline: {}", e))?;
                
            pipeline.add_many(&[&audioconvert, &audioresample, &aacenc, &aacparse, &audio_queue])
                .map_err(|e| anyhow::anyhow!("Failed to add audio elements to pipeline: {}", e))?;
                
            pipeline.add_many(&[&mpegtsmux, &hlssink])
                .map_err(|e| anyhow::anyhow!("Failed to add muxer elements to pipeline: {}", e))?;
                
            // Set all elements to PAUSED state so they're ready when we start playing
            for element in [&videoconvert, &h264enc, &h264parse, &video_queue, 
                           &audioconvert, &audioresample, &aacenc, &aacparse, &audio_queue,
                           &mpegtsmux, &hlssink].iter() {
                let _ = element.set_state(gst::State::Paused);
            }
            
            // Link elements that can be statically linked
            gst::Element::link_many(&[&filesrc, &decodebin])
                .map_err(|e| anyhow::anyhow!("Failed to link filesrc -> decodebin: {}", e))?;
                
            // Link video pipeline (processed separately due to muxer having limited sink pads)
            gst::Element::link_many(&[&videoconvert, &h264enc, &h264parse, &video_queue])
                .map_err(|e| anyhow::anyhow!("Failed to link video elements: {}", e))?;
                
            // Connect video queue to muxer separately
            video_queue.link(&mpegtsmux)
                .map_err(|e| anyhow::anyhow!("Failed to link video_queue to mpegtsmux: {}", e))?;
                
            // Link audio pipeline to a tee (we'll connect the tee to mpegtsmux)
            gst::Element::link_many(&[&audioconvert, &audioresample, &aacenc, &aacparse, &audio_queue])
                .map_err(|e| anyhow::anyhow!("Failed to link audio elements: {}", e))?;
                
            // Connect audio queue to muxer
            audio_queue.link(&mpegtsmux)
                .map_err(|e| anyhow::anyhow!("Failed to link audio_queue to mpegtsmux: {}", e))?;
                
            // Link muxer to HLS sink
            mpegtsmux.link(&hlssink)
                .map_err(|e| anyhow::anyhow!("Failed to link mpegtsmux -> hlssink: {}", e))?;
            
            // Set up dynamic pad connection for decodebin
            let video_convert_weak = videoconvert.downgrade();
            let audio_convert_weak = audioconvert.downgrade();
            let pipeline_weak = pipeline.downgrade();
            
            decodebin.connect_pad_added(move |_, src_pad| {
                let pipeline = match pipeline_weak.upgrade() {
                    Some(pipeline) => pipeline,
                    None => return,
                };
            
                let caps = match src_pad.current_caps() {
                    Some(caps) => caps,
                    None => return,
                };
                
                let structure = match caps.structure(0) {
                    Some(structure) => structure,
                    None => return,
                };
                
                let media_type = structure.name();
                
                // Handle video stream
                if media_type.starts_with("video/") {
                    if let Some(videoconvert) = video_convert_weak.upgrade() {
                        let sink_pad = match videoconvert.static_pad("sink") {
                            Some(pad) => pad,
                            None => {
                                eprintln!("Failed to get sink pad from videoconvert");
                                return;
                            }
                        };
                        
                        if src_pad.link(&sink_pad).is_err() {
                            eprintln!("Failed to link video decoder pad to videoconvert");
                        } else {
                            eprintln!("Successfully linked video stream to processing pipeline");
                            
                            // Ensure pad state is properly set
                            let _ = videoconvert.sync_state_with_parent();
                        }
                    }
                }
                // Handle audio stream
                else if media_type.starts_with("audio/") {
                    if let Some(audioconvert) = audio_convert_weak.upgrade() {
                        let sink_pad = match audioconvert.static_pad("sink") {
                            Some(pad) => pad,
                            None => {
                                eprintln!("Failed to get sink pad from audioconvert");
                                return;
                            }
                        };
                        
                        if src_pad.link(&sink_pad).is_err() {
                            eprintln!("Failed to link audio decoder pad to audioconvert");
                        } else {
                            eprintln!("Successfully linked audio stream to processing pipeline");
                            
                            // Ensure pad state is properly set
                            let _ = audioconvert.sync_state_with_parent();
                        }
                    }
                }
            });
            
            // Create a bus to watch for messages
            let bus = pipeline.bus().unwrap();
            
            // Start the pipeline
            pipeline.set_state(gst::State::Playing)
                .map_err(|e| anyhow::anyhow!("Failed to set pipeline to Playing: {}", e))?;
                
            // Wait for EOS or error
            let mut success = true;
            for msg in bus.iter_timed(gst::ClockTime::NONE) {
                match msg.view() {
                    gst::MessageView::Eos(_) => break,
                    gst::MessageView::Error(err) => {
                        eprintln!("Error from {:?}: {} ({:?})", 
                            err.src().map(|s| s.name()),
                            err.error(),
                            err.debug()
                        );
                        success = false;
                        break;
                    },
                    _ => {}
                }
            }
            
            // Stop the pipeline
            pipeline.set_state(gst::State::Null)
                .map_err(|e| anyhow::anyhow!("Failed to set pipeline to Null: {}", e))?;
                
            if !success {
                return Err(anyhow::anyhow!("GStreamer pipeline failed"));
            }
            
            // Create master playlist if not already created
            let master_playlist_path = playlist_path.parent().unwrap().join("master.m3u8");
            if !master_playlist_path.exists() {
                let master_content = format!(
                    "#EXTM3U\n\
                    #EXT-X-VERSION:3\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
                    playlist.m3u8\n"
                );
                std::fs::write(&master_playlist_path, master_content)
                    .map_err(|e| anyhow::anyhow!("Failed to write master playlist: {}", e))?;
            }
            
            Ok(())
        })();
        
        // Send the result back to the async task
        let _ = tx.blocking_send(result);
    });
    
    // Wait for the GStreamer processing to complete
    match rx.recv().await {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(e),
        None => Err(anyhow::anyhow!("GStreamer thread exited unexpectedly")),
    }
}

/// Get initialization segment for HLS playback
pub async fn get_init_segment(
    Path(recording_id): Path<String>,
    State(state): State<HlsControllerState>,
) -> impl IntoResponse {
    info!("On-the-fly HLS init segment request for recording: {}", recording_id);

    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    // Check if we have this init segment cached
    let cache_key = format!("init_{}", recording_id);
    let cached_path = {
        let cache = state.cache.lock().await;
        cache.get_init_segment(&cache_key)
    };

    if let Some(path) = cached_path {
        // Use cached segment if it exists
        if path.exists() {
            return serve_file(path).await;
        }
    }

    // Get recording details
    let recording = match state.app_state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
        }
    };

    // Create output path for the init segment
    let output_dir = state.temp_dir.join("init");
    if !output_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            error!("Failed to create init directory: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create directory").into_response();
        }
    }

    let output_path = output_dir.join(format!("{}.mp4", recording_id));

    // Generate init segment
    if let Err(e) = generate_init_segment(&recording, &output_path).await {
        error!("Failed to generate init segment: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate init segment").into_response();
    }

    // Store in cache
    {
        let mut cache = state.cache.lock().await;
        cache.store_init_segment(cache_key, output_path.clone());
        cache.clean_expired();
    }

    // Serve the generated file
    serve_file(output_path).await
}

/// Get an HLS segment for a recording at the specified time
pub async fn get_segment(
    Path(recording_id): Path<String>,
    Query(params): Query<HlsSegmentParams>,
    State(state): State<HlsControllerState>,
) -> impl IntoResponse {
    info!("On-the-fly HLS segment request for recording: {}", recording_id);

    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    // Get start time and duration from query params
    let start_time = params.start_time.unwrap_or(0.0);
    let duration = params.duration.unwrap_or(4.0); // Default to 4-second segments

    // Create cache key based on recording ID and segment parameters
    let cache_key = format!("segment_{}_{}_{}", recording_id, start_time, duration);
    
    // Check if we have this segment cached
    let cached_path = {
        let cache = state.cache.lock().await;
        cache.get_video_segment(&cache_key)
    };

    if let Some(path) = cached_path {
        // Use cached segment if it exists
        if path.exists() {
            return serve_file(path).await;
        }
    }

    // Get recording details
    let recording = match state.app_state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
        }
    };

    // Create output path for the segment
    let output_dir = state.temp_dir.join("segments");
    if !output_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            error!("Failed to create segments directory: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create directory").into_response();
        }
    }

    // Always use .ts extension for MPEG-TS segments (most compatible for HLS)
    let output_path = output_dir.join(format!("{}_{}_{}s.ts", recording_id, start_time, duration));

    // Generate standard MPEG-TS segment for maximum compatibility
    if let Err(e) = generate_segment(&recording, &output_path, start_time, duration).await {
        error!("Failed to generate TS segment: {}", e);
            
        // Try direct file streaming as final fallback
        if recording.file_path.exists() {
            info!("Attempting direct file streaming as final fallback for {}", recording_id);
            return match tokio::fs::File::open(&recording.file_path).await {
                Ok(file) => {
                    let stream = ReaderStream::new(file);
                    let body = StreamBody::new(stream);
                    
                    let headers = HeaderMap::from_iter([
                        (header::CONTENT_TYPE, "video/mp4".parse().unwrap()),
                        (header::CACHE_CONTROL, "max-age=3600".parse().unwrap()),
                        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap()),
                        (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS".parse().unwrap()),
                        (header::ACCESS_CONTROL_ALLOW_HEADERS, "Origin, Content-Type, Accept, Range".parse().unwrap()),
                        (header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Length, Content-Range, Content-Type".parse().unwrap()),
                        (header::ACCESS_CONTROL_MAX_AGE, "86400".parse().unwrap()),
                    ]);
                    
                    (StatusCode::OK, headers, body).into_response()
                }
                Err(e) => {
                    error!("Failed to open recording file for direct streaming: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate or stream segment").into_response();
                }
            };
        }
        
        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate segment").into_response();
    }

    // Store in cache
    {
        let mut cache = state.cache.lock().await;
        cache.store_video_segment(cache_key, output_path.clone());
        cache.clean_expired();
    }

    // Serve the generated file
    serve_file(output_path).await
}

/// Generate and serve an HLS playlist for a recording or camera
pub async fn get_playlist(
    Path(recording_id): Path<String>,
    Query(params): Query<HlsPlaylistParams>,
    State(state): State<HlsControllerState>,
) -> impl IntoResponse {
    // Check if this is a camera ID or recording ID
    let is_camera_request = recording_id.starts_with("camera-");
    
    if is_camera_request {
        // This is a camera HLS request
        let camera_id_str = recording_id.trim_start_matches("camera-");
        info!("HLS playlist request for camera: {}", camera_id_str);
        
        // Parse camera ID
        let camera_id = match Uuid::parse_str(camera_id_str) {
            Ok(id) => id,
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid camera ID").into_response(),
        };
        
        // Create a directory for this camera's HLS files
        let hls_dir = state.temp_dir.join("cameras").join(camera_id_str);
        let playlist_path = hls_dir.join("playlist.m3u8");
        let master_path = hls_dir.join("master.m3u8");
        
        // Check if we already have a generated playlist
        if !master_path.exists() || !playlist_path.exists() {
            info!("No pre-generated HLS playlist found, generating one now for camera {}", camera_id);
            
            // Get all recordings for this camera
            let query = crate::db::models::recording_models::RecordingSearchQuery {
                camera_ids: Some(vec![camera_id]),
                stream_ids: None,
                start_time: None,
                end_time: None,
                event_types: None,
                schedule_id: None,
                min_duration: Some(1), // Exclude 0-duration recordings
                segment_id: None,
                parent_recording_id: None,
                is_segment: None,
                limit: None, // Get all recordings
                offset: None,
            };
            
            let recordings = match state.app_state.recordings_repo.search(&query).await {
                Ok(recs) => recs,
                Err(e) => {
                    error!("Error fetching recordings for camera {}: {}", camera_id, e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch recordings").into_response();
                }
            };
            
            // Filter recordings with existing files
            let valid_recordings: Vec<_> = recordings
                .into_iter()
                .filter(|r| r.file_path.exists() && r.end_time.is_some())
                .collect();
                
            info!("Found {} valid recordings for camera {}", valid_recordings.len(), camera_id);
                
            // Generate the HLS playlist and segments for all recordings
            if let Err(e) = generate_camera_hls(&camera_id, &valid_recordings, &hls_dir).await {
                error!("Failed to generate HLS for camera {}: {}", camera_id, e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate camera HLS").into_response();
            }
        }
        
        // Determine which playlist to serve
        let playlist_type = params.playlist_type.as_deref().unwrap_or("master");
        let file_path = if playlist_type == "master" {
            master_path
        } else {
            playlist_path
        };
        
        // Verify that file exists
        if !file_path.exists() {
            error!("HLS playlist file not found: {}", file_path.display());
            return (StatusCode::NOT_FOUND, "HLS playlist not found").into_response();
        }
        
        // Serve the playlist file
        match tokio::fs::read(&file_path).await {
            Ok(content) => {
                info!("Serving HLS playlist: {}", file_path.display());
                
                // Set appropriate CORS headers for streaming
                let headers = HeaderMap::from_iter([
                    (header::CONTENT_TYPE, "application/vnd.apple.mpegurl".parse().unwrap()),
                    (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap()),
                    (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS".parse().unwrap()),
                    (header::ACCESS_CONTROL_ALLOW_HEADERS, "Origin, Content-Type, Accept, Range".parse().unwrap()),
                    (header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Length, Content-Range, Content-Type".parse().unwrap()),
                    (header::CACHE_CONTROL, "max-age=3600".parse().unwrap()), // Cache for an hour
                ]);
                
                // Return the playlist
                (StatusCode::OK, headers, content).into_response()
            },
            Err(e) => {
                error!("Failed to read HLS playlist: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read HLS playlist").into_response()
            }
        }
    } else {
        // This is a recording HLS request
        info!("HLS playlist request for recording: {}", recording_id);

        // Parse recording ID
        let uuid = match Uuid::parse_str(&recording_id) {
            Ok(id) => id,
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
        };

        // Get recording details
        let recording = match state.app_state.recordings_repo.get_by_id(&uuid).await {
            Ok(Some(recording)) => recording,
            Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
            Err(e) => {
                error!("Error fetching recording: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
            }
        };

        // Create a directory for this recording's HLS files
        let hls_dir = state.temp_dir.join("recordings").join(&recording_id);
        let playlist_path = hls_dir.join("playlist.m3u8");
        let master_path = hls_dir.join("master.m3u8");
        
        // Check if we already have a generated playlist
        if !master_path.exists() || !playlist_path.exists() {
            info!("No pre-generated HLS playlist found, generating one now for recording {}", recording_id);
            
            // Generate the HLS playlist and segments
            if let Err(e) = generate_recording_hls(&recording, &hls_dir).await {
                error!("Failed to generate HLS: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate HLS").into_response();
            }
        }
        
        // Determine which playlist to serve
        let playlist_type = params.playlist_type.as_deref().unwrap_or("master");
        let file_path = if playlist_type == "master" {
            master_path
        } else {
            playlist_path
        };
        
        // Verify that file exists
        if !file_path.exists() {
            error!("HLS playlist file not found: {}", file_path.display());
            return (StatusCode::NOT_FOUND, "HLS playlist not found").into_response();
        }
        
        // Serve the playlist file
        match tokio::fs::read(&file_path).await {
            Ok(content) => {
                info!("Serving HLS playlist: {}", file_path.display());
                
                // Set appropriate CORS headers for streaming
                let headers = HeaderMap::from_iter([
                    (header::CONTENT_TYPE, "application/vnd.apple.mpegurl".parse().unwrap()),
                    (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap()),
                    (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS".parse().unwrap()),
                    (header::ACCESS_CONTROL_ALLOW_HEADERS, "Origin, Content-Type, Accept, Range".parse().unwrap()),
                    (header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Length, Content-Range, Content-Type".parse().unwrap()),
                    (header::CACHE_CONTROL, "max-age=3600".parse().unwrap()), // Cache for an hour
                ]);
                
                // Return the playlist
                (StatusCode::OK, headers, content).into_response()
            },
            Err(e) => {
                error!("Failed to read HLS playlist: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read HLS playlist").into_response()
            }
        }
    }
}

/// Generate an initialization segment for HLS streaming
async fn generate_init_segment(
    recording: &Recording,
    output_path: &FilePath,
) -> Result<(), anyhow::Error> {
    info!("Generating init segment for recording: {}", recording.id);
    
    // Use FFmpeg to extract the initialization segment (first few frames without keyframes)
    let status = Command::new("ffmpeg")
        .arg("-i")
        .arg(&recording.file_path) // Input file
        .arg("-c")
        .arg("copy") // Copy codecs
        .arg("-map")
        .arg("0") // Map all streams
        .arg("-f")
        .arg("mp4") // Use MP4 format
        .arg("-y") // Overwrite existing file
        .arg("-t")
        .arg("0.5") // Just get a small portion for initialization
        .arg(output_path) // Output path
        .stderr(Stdio::inherit())
        .status()?;
        
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to generate init segment"));
    }
    
    // Verify file was created successfully
    if !output_path.exists() || std::fs::metadata(output_path)?.len() == 0 {
        return Err(anyhow::anyhow!("Failed to create a valid init segment"));
    }
    
    Ok(())
}

/// Generate a segment for HLS streaming at a specific timestamp
async fn generate_segment(
    recording: &Recording,
    output_path: &FilePath,
    start_time: f64,
    duration: f64,
) -> Result<(), anyhow::Error> {
    info!("Generating segment for recording {} at {}s for {}s", recording.id, start_time, duration);
    
    // Use FFmpeg to extract the segment
    let status = Command::new("ffmpeg")
        .arg("-i")
        .arg(&recording.file_path) // Input file
        .arg("-ss")
        .arg(start_time.to_string()) // Start time
        .arg("-t")
        .arg(duration.to_string()) // Duration
        .arg("-c")
        .arg("copy") // Copy codecs
        .arg("-map")
        .arg("0") // Map all streams
        .arg("-f")
        .arg("mpegts") // Use MPEG-TS format for better compatibility
        .arg("-y") // Overwrite existing file
        .arg(output_path) // Output path
        .stderr(Stdio::inherit())
        .status()?;
        
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to generate segment"));
    }
    
    // Verify file was created successfully
    if !output_path.exists() || std::fs::metadata(output_path)?.len() == 0 {
        return Err(anyhow::anyhow!("Failed to create a valid segment"));
    }
    
    Ok(())
}

/// Helper function to serve a file with appropriate headers
async fn serve_file(path: PathBuf) -> Response {
    match tokio::fs::File::open(&path).await {
        Ok(file) => {
            let stream = ReaderStream::new(file);
            let body = StreamBody::new(stream);

            // Determine content type based on file extension and path
            let content_type = if path.extension().and_then(|e| e.to_str()) == Some("ts") {
                "video/mp2t"  // MPEG-2 Transport Stream
            } else if path.extension().and_then(|e| e.to_str()) == Some("mp4") {
                "video/mp4"   // MP4 file
            } else if path.to_string_lossy().contains("init") {
                if path.to_string_lossy().ends_with("init.mp4") {
                    "video/mp4"  // MP4 init segment
                } else {
                    "video/mp2t"  // TS init segment
                }
            } else {
                "application/octet-stream"  // Default binary type
            };

            let headers = HeaderMap::from_iter([
                (header::CONTENT_TYPE, content_type.parse().unwrap()),
                (header::CACHE_CONTROL, "max-age=3600".parse().unwrap()), // Cache for an hour
                // Add comprehensive Cross-Origin headers for better browser compatibility
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap()),
                (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS".parse().unwrap()),
                (header::ACCESS_CONTROL_ALLOW_HEADERS, "Origin, Content-Type, Accept, Range".parse().unwrap()),
                (header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Length, Content-Range, Content-Type".parse().unwrap()),
                (header::ACCESS_CONTROL_MAX_AGE, "86400".parse().unwrap()), // 24 hours
            ]);

            (StatusCode::OK, headers, body).into_response()
        }
        Err(e) => {
            error!("Failed to serve file {}: {}", path.display(), e);
            (StatusCode::NOT_FOUND, "File not found").into_response()
        }
    }
}
