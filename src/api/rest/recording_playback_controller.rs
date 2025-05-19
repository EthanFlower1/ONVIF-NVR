use crate::api::rest::AppState;
use crate::db::models::recording_models::{RecordingEventType, RecordingSearchQuery};
use crate::db::repositories::cameras::CamerasRepository;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::security::auth::AuthService;
use axum::body::StreamBody;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Duration, TimeZone, Utc};
use gstreamer as gst;
use gstreamer::parse::launch;
use gstreamer::prelude::*;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

/// Timeline API state
#[derive(Clone)]
pub struct TimelineApiState {
    pub recordings_repo: RecordingsRepository,
    pub cameras_repo: CamerasRepository,
    pub auth_service: Arc<AuthService>,
}

/// Timeline query parameters
#[derive(Debug, Deserialize)]
pub struct TimelineParams {
    pub camera_id: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub event_type: Option<String>,
    pub include_segments: Option<bool>,
}

/// Timeline segment response
#[derive(Debug, Serialize)]
pub struct TimelineSegment {
    pub id: String,
    pub camera_id: String,
    pub stream_id: String,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration: u64,
    pub event_type: String,
    pub file_path: String,
    pub is_segment: bool,
    pub parent_id: Option<String>,
    pub segment_id: Option<u32>,
}

/// Timeline response with all segments
#[derive(Debug, Serialize)]
pub struct TimelineResponse {
    pub start_time: String,
    pub end_time: String,
    pub total_duration: u64,
    pub camera_id: String,
    pub camera_name: String,
    pub segments: Vec<TimelineSegment>,
}

/// Helper function to convert AppState to TimelineApiState
pub fn app_state_to_timeline_state(app_state: &AppState) -> TimelineApiState {
    TimelineApiState {
        recordings_repo: RecordingsRepository::new(Arc::clone(&app_state.db_pool)),
        cameras_repo: CamerasRepository::new(Arc::clone(&app_state.db_pool)),
        auth_service: Arc::clone(&app_state.auth_service),
    }
}

/// Create recording timeline router
pub fn create_router<S: Clone + Send + Sync + 'static>(state: S) -> Router<AppState> {
    Router::new()
        .route("/timeline", get(get_recording_timeline))
        .route("/:recording_id", get(get_recording_playback_info))
        .route("/recordings_by_date", get(get_recordings_by_date))
        .route("/segments/:parent_id", get(get_recording_segments))
        .route("/video/:recording_id", get(get_video_recording))
        // HLS playlist endpoints
        .route("/cameras/:id/hls", get(get_hls_playlist))
        // HLS segment endpoint
        .route("/:id/hls", get(get_hls_segment))
        // HLS init segment endpoint
        .route("/:id/init.mp4", get(get_init_segment))
}

pub async fn get_video_recording(
    Path(recording_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("TRiggering video recordings...........................................................................");
    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    let recording = match state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
        }
    };

    // Here you would load the file and return it
    let path = recording.file_path;
    match tokio::fs::File::open(&path).await {
        Ok(file) => {
            let stream = ReaderStream::new(file);
            let body = StreamBody::new(stream);

            let headers = HeaderMap::from_iter([
                (header::CONTENT_TYPE, "video/mp4".parse().unwrap()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}.mp4\"", recording_id)
                        .parse()
                        .unwrap(),
                ),
            ]);

            (StatusCode::OK, headers, body).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Video recording not found").into_response(),
    }
}

// Define the HlsQuery struct for query parameters
#[derive(Deserialize, Default)]
pub struct HlsQuery {
    #[serde(default)]
    playlist_type: String,
}

pub async fn get_hls_playlist(
    Path(camera_id): Path<String>, // This could be camera ID or any grouping ID
    Query(params): Query<HlsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("HLS playlist request for camera: {}", camera_id);

    let uuid = match Uuid::parse_str(&camera_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    // Get all recordings for this camera, ordered by timestamp
    let recordings = match state
        .recordings_repo
        .get_by_camera(&uuid, Some(10000))
        .await
    {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error fetching recordings: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
        }
    };

    if recordings.is_empty() {
        return (StatusCode::NOT_FOUND, "No recordings found").into_response();
    }

    match params.playlist_type.as_str() {
        // Master playlist
        "master" => {
            // Create master playlist with one quality level
            let playlist = format!(
                "#EXTM3U\n\
                 #EXT-X-VERSION:7\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
                 /playback/cameras/{}/hls?playlist_type=variant\n",
                camera_id
            );

            let headers = HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                "application/vnd.apple.mpegurl".parse().unwrap(),
            )]);

            (StatusCode::OK, headers, playlist).into_response()
        }

        // Variant playlist
        "variant" => {
            // Create a variant playlist that references all segments
            let mut playlist = String::from(
                "#EXTM3U\n\
                #EXT-X-VERSION:7\n\
                #EXT-X-TARGETDURATION:4\n\
                #EXT-X-MEDIA-SEQUENCE:0\n",
            );

            // Use first recording's ID to create init.mp4 URL
            let first_recording = &recordings[0];

            let recordings_base =
                std::env::var("RECORDINGS_PATH").unwrap_or_else(|_| "/app/recordings".to_string());
            let init_path = format!("{}/{}/init.mp4", recordings_base, first_recording.id);

            // Create directory if it doesn't exist
            if let Some(parent) = std::path::Path::new(&init_path).parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    error!("Failed to create directory: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create output directory",
                    )
                        .into_response();
                }
            } else {
                // This else clause belongs to the parent check, not the error check
                error!("Invalid path: no parent directory");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid output path").into_response();
            }

            info!(
                "First Recording Path: {}",
                first_recording.file_path.display()
            );
            // Try a simpler pipeline first - just extract the MP4 headers
            let pipeline = match launch(&format!(
                "filesrc location=\"{}\" ! \
                qtdemux name=demux ! \
                mp4mux fragment-duration=1000 ! \
                filesink location=\"{}\"",
                first_recording.file_path.display(),
                init_path
            )) {
                Ok(p) => p,
                Err(e) => {
                    error!("GStreamer pipeline launch error: {}", e);
                    // Try fallback pipeline if the first one fails
                    match launch(&format!(
                        "filesrc location=\"{}\" ! \
                        decodebin ! \
                        mp4mux ! \
                        filesink location=\"{}\"",
                        first_recording.file_path.display(),
                        init_path
                    )) {
                        Ok(p) => p,
                        Err(e) => {
                            error!("Fallback GStreamer pipeline launch error: {}", e);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to create init.mp4",
                            )
                                .into_response();
                        }
                    }
                }
            };

            // Add timeout for pipeline operation
            let timeout = std::time::Duration::from_secs(10);
            let start_time = std::time::Instant::now();

            // Replace ? operator with explicit match handling
            match pipeline.set_state(gst::State::Playing) {
                Ok(_) => {}
                Err(e) => {
                    error!("GStreamer state change error: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to start GStreamer pipeline",
                    )
                        .into_response();
                }
            }

            let bus = pipeline.bus().unwrap();
            let mut success = false;

            // Wait for EOS or timeout
            for msg in bus.iter_timed(gst::ClockTime::from_seconds(5)) {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        success = true;
                        break;
                    }
                    gst::MessageView::Error(err) => {
                        error!("GStreamer error: {}", err.error());
                        break;
                    }
                    _ => {}
                }

                if start_time.elapsed() > timeout {
                    error!("GStreamer pipeline timeout");
                    break;
                }
            }

            // Always clean up the pipeline state
            if let Err(e) = pipeline.set_state(gst::State::Null) {
                error!("Failed to stop GStreamer pipeline: {}", e);
            }

            // Check if init file was created
            if !success || !std::path::Path::new(&init_path).exists() {
                error!("Failed to create initialization segment");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to create initialization segment",
                )
                    .into_response();
            }

            // Continue with playlist generation - properly reference init.mp4
            // Using both absolute and relative URLs for better compatibility
            playlist.push_str(&format!(
                "#EXT-X-MAP:URI=\"/playback/{}/init.mp4\"\n",
                first_recording.id
            ));

            // Check if there are discontinuities between segments
            let mut previous_end_time = None;

            for recording in recordings {
                // Skip recordings without an end_time
                if recording.end_time.is_none() {
                    continue; // Skip to the next recording
                }

                // Add discontinuity marker if there's a gap between segments
                if let Some(prev_end) = previous_end_time {
                    // Assuming recording has start_time field
                    if recording.start_time > prev_end + Duration::seconds(1) {
                        // 1 second tolerance
                        playlist.push_str("#EXT-X-DISCONTINUITY\n");
                    }
                }

                // Add segment to playlist - using relative URL for better compatibility
                playlist.push_str(&format!(
                    "#EXTINF:4.0,\n\
                    /playback/{}/hls?playlist_type=segment\n",
                    recording.id
                ));

                // Update previous_end_time using the actual end_time from recording
                // instead of calculating based on start_time
                previous_end_time = recording.end_time;
                // Assuming 30s segments
            }

            playlist.push_str("#EXT-X-ENDLIST\n");

            let headers = HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                "application/vnd.apple.mpegurl".parse().unwrap(),
            )]);

            (StatusCode::OK, headers, playlist).into_response()
        }

        _ => (StatusCode::BAD_REQUEST, "Invalid playlist type").into_response(),
    }
}
// Function to serve individual segments
// Get initialization segment for HLS
pub async fn get_init_segment(
    Path(recording_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("HLS init.mp4 request for recording: {}", recording_id);

    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    // Try to find existing init.mp4 file
    let recordings_base =
        std::env::var("RECORDINGS_PATH").unwrap_or_else(|_| "/app/recordings".to_string());
    let init_path = format!("{}/{}/init.mp4", recordings_base, recording_id);

    // Check if init.mp4 exists
    match tokio::fs::File::open(&init_path).await {
        Ok(file) => {
            info!("Found existing init.mp4 for {}", recording_id);
            let stream = ReaderStream::new(file);
            let body = StreamBody::new(stream);

            // For MP4 segments, use video/mp4 content type
            let headers = HeaderMap::from_iter([
                (header::CONTENT_TYPE, "video/mp4".parse().unwrap()),
                (header::CACHE_CONTROL, "max-age=31536000".parse().unwrap()), // Cache for a year
            ]);

            (StatusCode::OK, headers, body).into_response()
        }
        Err(_) => {
            info!("Init.mp4 not found at {}, generating new one", init_path);

            // Get recording details to generate init.mp4
            let recording = match state.recordings_repo.get_by_id(&uuid).await {
                Ok(Some(recording)) => recording,
                Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
                Err(e) => {
                    error!("Error fetching recording: {}", e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
                }
            };

            // Create directory if it doesn't exist
            if let Some(parent) = std::path::Path::new(&init_path).parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    error!("Failed to create directory: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create output directory",
                    )
                        .into_response();
                }
            }

            // Try to create initialization segment with GStreamer using multiple approaches
            let mut success = false;
            let mut pipeline_opt = None;

            // Try the primary pipeline approach first
            let primary_pipeline = format!(
                "filesrc location=\"{}\" ! \
                qtdemux name=demux ! \
                mp4mux faststart=true streamable=true movie-timescale=90000 trak-timescale=90000 ! \
                filesink location=\"{}\"",
                recording.file_path.display(),
                init_path
            );

            match launch(&primary_pipeline) {
                Ok(p) => {
                    info!("Successfully created primary pipeline for init.mp4");
                    pipeline_opt = Some(p);
                    success = true;
                }
                Err(e) => {
                    error!("GStreamer primary pipeline launch error: {}", e);

                    // Define fallback pipelines in order of preference
                    let fallback_pipelines = vec![
                        // Fallback 1: Try with decodebin
                        format!(
                            "filesrc location=\"{}\" ! \
                            decodebin ! \
                            mp4mux faststart=true streamable=true movie-timescale=90000 trak-timescale=90000 ! \
                            filesink location=\"{}\"",
                            recording.file_path.display(),
                            init_path
                        ),
                        // Fallback 2: Try with full demux/decode/encode cycle
                        format!(
                            "filesrc location=\"{}\" ! \
                            decodebin name=dec ! queue ! videoconvert ! x264enc ! \
                            mp4mux faststart=true streamable=true ! \
                            filesink location=\"{}\"",
                            recording.file_path.display(),
                            init_path
                        ),
                        // Fallback 3: Just create a minimal valid MP4 file
                        format!(
                            "videotestsrc num-buffers=1 ! video/x-raw,width=320,height=240 ! \
                            videoconvert ! x264enc ! mp4mux ! \
                            filesink location=\"{}\"",
                            init_path
                        )
                    ];

                    // Try each fallback pipeline in sequence
                    for (i, pipeline_str) in fallback_pipelines.iter().enumerate() {
                        match launch(pipeline_str) {
                            Ok(p) => {
                                info!(
                                    "Successfully created fallback pipeline {} for init.mp4",
                                    i + 1
                                );
                                pipeline_opt = Some(p);
                                success = true;
                                break;
                            }
                            Err(e) => {
                                error!("Fallback pipeline {} launch error: {}", i + 1, e);
                                // Continue to next fallback
                            }
                        }
                    }

                    // If all GStreamer pipelines failed, try creating a minimal file manually
                    if !success {
                        error!("All GStreamer pipelines failed. Attempting to manually create a minimal init.mp4");

                        // Create a very basic standard MP4 initialization segment
                        let init_contents: &[u8] = &[
                            // MP4 file signature (ftyp box)
                            0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79,
                            0x70, // size=24, type="ftyp"
                            0x69, 0x73, 0x6F, 0x6D, 0x00, 0x00, 0x00,
                            0x01, // brand="isom", minor_version=1
                            0x69, 0x73, 0x6F, 0x6D, 0x61, 0x76, 0x63,
                            0x31, // compatible_brands="isomavc1"
                            // Movie box header
                            0x00, 0x00, 0x00, 0x08, 0x6D, 0x6F, 0x6F,
                            0x76, // size=8, type="moov"
                        ];

                        // Write the minimal init segment to file
                        if let Ok(mut file) = std::fs::File::create(&init_path) {
                            if let Ok(_) = file.write_all(init_contents) {
                                info!("Created minimal MP4 initialization file at {}", init_path);

                                // Create a dummy pipeline for consistency
                                if let Ok(p) = launch("fakesrc ! fakesink") {
                                    pipeline_opt = Some(p);
                                    success = true;
                                }
                            }
                        }
                    }
                }
            }

            // If all approaches failed
            if !success {
                error!("Failed to create init.mp4 file through any method");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to create init.mp4 - all approaches failed",
                )
                    .into_response();
            }

            // Get the pipeline from the option
            let pipeline = pipeline_opt.unwrap();

            // Run the pipeline
            if let Err(e) = pipeline.set_state(gst::State::Playing) {
                error!("GStreamer state change error: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to start GStreamer pipeline",
                )
                    .into_response();
            }

            let bus = pipeline.bus().unwrap();
            let mut success = false;

            // Wait for EOS or timeout
            for msg in bus.iter_timed(gst::ClockTime::from_seconds(5)) {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        success = true;
                        break;
                    }
                    gst::MessageView::Error(err) => {
                        error!("GStreamer error: {}", err.error());
                        break;
                    }
                    _ => {}
                }
            }

            // Clean up the pipeline state
            if let Err(e) = pipeline.set_state(gst::State::Null) {
                error!("Failed to stop GStreamer pipeline: {}", e);
            }

            // Check if init file was created
            if !success || !std::path::Path::new(&init_path).exists() {
                error!("Failed to create initialization segment");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to create initialization segment",
                )
                    .into_response();
            }

            // Now serve the newly created init.mp4
            match tokio::fs::File::open(&init_path).await {
                Ok(file) => {
                    let stream = ReaderStream::new(file);
                    let body = StreamBody::new(stream);

                    // For MP4 segments, use video/mp4 content type
                    let headers = HeaderMap::from_iter([
                        (header::CONTENT_TYPE, "video/mp4".parse().unwrap()),
                        (header::CACHE_CONTROL, "max-age=31536000".parse().unwrap()), // Cache for a year
                    ]);

                    (StatusCode::OK, headers, body).into_response()
                }
                Err(_) => {
                    (StatusCode::NOT_FOUND, "Failed to open generated init.mp4").into_response()
                }
            }
        }
    }
}

pub async fn get_hls_segment(
    Path(recording_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    info!("HLS segment request for recording: {}", recording_id);

    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid recording ID").into_response(),
    };

    // Get recording details
    let recording = match state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return (StatusCode::NOT_FOUND, "Recording not found").into_response(),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Server error").into_response();
        }
    };

    // Serve the MP4 file directly as an HLS segment
    let path = recording.file_path;
    match tokio::fs::File::open(&path).await {
        Ok(file) => {
            let stream = ReaderStream::new(file);
            let body = StreamBody::new(stream);

            // For MP4 segments, use video/mp4 content type with caching directives
            let headers = HeaderMap::from_iter([
                (header::CONTENT_TYPE, "video/mp4".parse().unwrap()),
                (header::CACHE_CONTROL, "max-age=86400".parse().unwrap()), // Cache for a day
            ]);

            (StatusCode::OK, headers, body).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "Video segment not found").into_response(),
    }
}
/// Get timeline data for a camera over a specific time period
pub async fn get_recording_timeline(
    Query(params): Query<TimelineParams>,
    State(state): State<AppState>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse camera ID
    let camera_id = match Uuid::parse_str(&params.camera_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get the camera info
    let camera = match state.cameras_repo.get_by_id(&camera_id).await {
        Ok(Some(camera)) => camera,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Error fetching camera: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Determine time range (default to last 24 hours if not specified)
    let end_time = match params.end_time {
        Some(end_str) => match DateTime::parse_from_rfc3339(&end_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => Utc::now(),
        },
        None => Utc::now(),
    };

    let start_time = match params.start_time {
        Some(start_str) => match DateTime::parse_from_rfc3339(&start_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => end_time - Duration::days(1),
        },
        None => end_time - Duration::days(1),
    };

    // Parse event type if provided
    let event_types = match params.event_type {
        Some(ref type_str) => match type_str.to_lowercase().as_str() {
            "continuous" => Some(vec![RecordingEventType::Continuous]),
            "motion" => Some(vec![RecordingEventType::Motion]),
            "audio" => Some(vec![RecordingEventType::Audio]),
            "external" => Some(vec![RecordingEventType::External]),
            "manual" => Some(vec![RecordingEventType::Manual]),
            "analytics" => Some(vec![RecordingEventType::Analytics]),
            _ => None,
        },
        None => None,
    };

    // Create search query
    let mut query = RecordingSearchQuery {
        camera_ids: Some(vec![camera_id]),
        stream_ids: None,
        start_time: Some(start_time),
        end_time: Some(end_time),
        event_types,
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: None,
        is_segment: match params.include_segments {
            Some(true) => None,                // Include both segments and parents
            Some(false) | None => Some(false), // Only parent recordings by default
        },
        limit: Some(1000), // Higher limit for timeline view
        offset: Some(0),
    };

    // Execute search
    let recordings = match state.recordings_repo.search(&query).await {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error searching recordings: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert to timeline segments
    let mut segments = Vec::new();
    let mut total_duration: u64 = 0;
    for recording in recordings {
        // Skip invalid recordings
        if recording.duration == 0 || recording.file_path.to_str().is_none() {
            continue;
        }

        let is_segment = recording.parent_recording_id.is_some();

        // Exclude segments if requested
        if params.include_segments == Some(false) && is_segment {
            continue;
        }

        let segment = TimelineSegment {
            id: recording.id.to_string(),
            camera_id: recording.camera_id.to_string(),
            stream_id: recording.stream_id.to_string(),
            start_time: recording.start_time.to_rfc3339(),
            end_time: recording.end_time.map(|dt| dt.to_rfc3339()),
            duration: recording.duration as u64,
            event_type: recording.event_type.to_string(),
            file_path: recording.file_path.to_string_lossy().to_string(),
            is_segment,
            parent_id: recording.parent_recording_id.map(|id| id.to_string()),
            segment_id: recording.segment_id,
        };

        total_duration += recording.duration as u64;
        segments.push(segment);
    }

    // Create timeline response
    let response = TimelineResponse {
        start_time: start_time.to_rfc3339(),
        end_time: end_time.to_rfc3339(),
        total_duration,
        camera_id: camera_id.to_string(),
        camera_name: camera.name.clone(),
        segments,
    };

    Ok(Json(response))
}

/// Get detailed information about a specific recording for playback
pub async fn get_recording_playback_info(
    Path(recording_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<HashMap<String, serde_json::Value>>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse recording ID
    let uuid = match Uuid::parse_str(&recording_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Get recording details
    let recording = match state.recordings_repo.get_by_id(&uuid).await {
        Ok(Some(recording)) => recording,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Error fetching recording: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get camera details
    let camera = match state.cameras_repo.get_by_id(&recording.camera_id).await {
        Ok(Some(camera)) => camera,
        Ok(None) => {
            error!("Camera not found for recording: {}", recording.id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("Error fetching camera: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Check if file exists
    let file_path = recording.file_path.clone();
    if !file_path.exists() {
        error!("Recording file not found: {}", file_path.display());
        return Err(StatusCode::NOT_FOUND);
    }

    // Build response with all needed info
    let mut response = HashMap::new();
    response.insert(
        "id".to_string(),
        serde_json::json!(recording.id.to_string()),
    );
    response.insert(
        "camera_id".to_string(),
        serde_json::json!(recording.camera_id.to_string()),
    );
    response.insert("camera_name".to_string(), serde_json::json!(camera.name));
    response.insert(
        "stream_id".to_string(),
        serde_json::json!(recording.stream_id.to_string()),
    );
    response.insert(
        "start_time".to_string(),
        serde_json::json!(recording.start_time.to_rfc3339()),
    );

    if let Some(end_time) = recording.end_time {
        response.insert(
            "end_time".to_string(),
            serde_json::json!(end_time.to_rfc3339()),
        );
    }

    response.insert(
        "duration".to_string(),
        serde_json::json!(recording.duration),
    );
    response.insert(
        "file_size".to_string(),
        serde_json::json!(recording.file_size),
    );
    response.insert("format".to_string(), serde_json::json!(recording.format));
    response.insert(
        "resolution".to_string(),
        serde_json::json!(recording.resolution),
    );
    response.insert("fps".to_string(), serde_json::json!(recording.fps));
    response.insert(
        "event_type".to_string(),
        serde_json::json!(recording.event_type.to_string()),
    );
    response.insert(
        "file_path".to_string(),
        serde_json::json!(recording.file_path.to_string_lossy().to_string()),
    );

    if let Some(segment_id) = recording.segment_id {
        response.insert("segment_id".to_string(), serde_json::json!(segment_id));
    }

    if let Some(parent_id) = recording.parent_recording_id {
        response.insert(
            "parent_recording_id".to_string(),
            serde_json::json!(parent_id.to_string()),
        );
    }

    // Include metadata if available
    if let Some(metadata) = recording.metadata {
        response.insert("metadata".to_string(), metadata);
    }

    Ok(Json(response))
}

/// Get recordings grouped by date (for calendar view)
pub async fn get_recordings_by_date(
    Query(params): Query<TimelineParams>,
    State(state): State<AppState>,
) -> Result<Json<HashMap<String, serde_json::Value>>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse camera ID
    let camera_id = match Uuid::parse_str(&params.camera_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Determine time range (default to last 30 days if not specified)
    let end_time = match params.end_time {
        Some(end_str) => match DateTime::parse_from_rfc3339(&end_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => Utc::now(),
        },
        None => Utc::now(),
    };

    let start_time = match params.start_time {
        Some(start_str) => match DateTime::parse_from_rfc3339(&start_str) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => end_time - Duration::days(30),
        },
        None => end_time - Duration::days(30),
    };

    // Create search query for non-segment recordings (parent recordings only)
    let query = RecordingSearchQuery {
        camera_ids: Some(vec![camera_id]),
        stream_ids: None,
        start_time: Some(start_time),
        end_time: Some(end_time),
        event_types: None, // All event types
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: None,
        is_segment: Some(false), // Only parent recordings
        limit: Some(1000),
        offset: Some(0),
    };

    // Execute search
    let recordings = match state.recordings_repo.search(&query).await {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error searching recordings: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Group recordings by date (YYYY-MM-DD)
    let mut recordings_by_date: HashMap<String, Vec<serde_json::Value>> = HashMap::new();

    for recording in recordings {
        let date_key = recording.start_time.format("%Y-%m-%d").to_string();

        let recording_json = serde_json::json!({
            "id": recording.id.to_string(),
            "start_time": recording.start_time.to_rfc3339(),
            "end_time": recording.end_time.map(|dt| dt.to_rfc3339()),
            "duration": recording.duration,
            "event_type": recording.event_type.to_string(),
            "file_size": recording.file_size,
        });

        recordings_by_date
            .entry(date_key)
            .or_insert_with(Vec::new)
            .push(recording_json);
    }

    // Build final response
    let mut response = HashMap::new();
    response.insert(
        "camera_id".to_string(),
        serde_json::json!(camera_id.to_string()),
    );
    response.insert(
        "start_date".to_string(),
        serde_json::json!(start_time.format("%Y-%m-%d").to_string()),
    );
    response.insert(
        "end_date".to_string(),
        serde_json::json!(end_time.format("%Y-%m-%d").to_string()),
    );

    // Convert recordings_by_date into JSON
    let dates_json = serde_json::json!(recordings_by_date);
    response.insert("dates".to_string(), dates_json);

    Ok(Json(response))
}

/// Get all segments for a parent recording
pub async fn get_recording_segments(
    Path(parent_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<TimelineSegment>>, StatusCode> {
    // Convert AppState to TimelineApiState
    let state = app_state_to_timeline_state(&state);

    // Parse parent recording ID
    let parent_uuid = match Uuid::parse_str(&parent_id) {
        Ok(id) => id,
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // Create search query for segments of this parent
    let query = RecordingSearchQuery {
        camera_ids: None,
        stream_ids: None,
        start_time: None,
        end_time: None,
        event_types: None,
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: Some(parent_uuid),
        is_segment: Some(true), // Only segments
        limit: Some(1000),
        offset: Some(0),
    };

    // Execute search
    let segments = match state.recordings_repo.search(&query).await {
        Ok(recordings) => recordings,
        Err(e) => {
            error!("Error searching for segments: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert to timeline segments
    let segments = segments
        .into_iter()
        .map(|recording| TimelineSegment {
            id: recording.id.to_string(),
            camera_id: recording.camera_id.to_string(),
            stream_id: recording.stream_id.to_string(),
            start_time: recording.start_time.to_rfc3339(),
            end_time: recording.end_time.map(|dt| dt.to_rfc3339()),
            duration: recording.duration as u64,
            event_type: recording.event_type.to_string(),
            file_path: recording.file_path.to_string_lossy().to_string(),
            is_segment: true,
            parent_id: Some(parent_uuid.to_string()),
            segment_id: recording.segment_id,
        })
        .collect();

    Ok(Json(segments))
}
