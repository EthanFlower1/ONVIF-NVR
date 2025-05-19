use anyhow::{anyhow, Result};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{sink::SinkExt, stream::StreamExt};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gst::glib;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task;
use uuid::Uuid;

// Command struct for WebSocket communication
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientCommand {
    #[serde(rename = "play")]
    Play { recording_id: String, rate: f64 },
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "seek")]
    Seek { position: i64 },
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "setRate")]
    SetRate { rate: f64 },
}

// Alternative format for manual deserialization if needed
#[derive(Debug, Deserialize)]
pub struct ClientCommandRaw {
    #[serde(rename = "type")]
    pub type_: String,
    pub recording_id: Option<String>,
    pub position: Option<i64>,
    pub rate: Option<f64>,
}

// Response struct for WebSocket communication
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ServerResponse {
    #[serde(rename = "status")]
    Status { state: String, position: i64, duration: i64 },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "ready")]
    Ready { duration: i64 },
}

// Session to track active streams
struct PlaybackSession {
    pipeline: gst::Pipeline,
    recording_id: Uuid,
    sink: Arc<Mutex<gst_app::AppSink>>,
}

// Handle WebSocket connection upgrade
pub async fn handle_ws_upgrade(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

// Handle WebSocket connection
async fn handle_socket(socket: WebSocket) {
    // Ensure GStreamer is initialized
    if let Err(e) = gst::init() {
        error!("Failed to initialize GStreamer: {}", e);
        return;
    }

    // Split the socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();
    
    // Create a Mutex-wrapped sender to share between tasks
    let sender = Arc::new(tokio::sync::Mutex::new(sender));
    let sender_clone = sender.clone();

    // Create a channel for communication between tasks
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(32);

    // Spawn a task to handle messages from client
    let tx_clone = tx.clone();
    let mut playback_session: Option<PlaybackSession> = None;

    // Task to handle incoming messages from client
    let receiver_task = tokio::spawn(async move {
        
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    debug!("Received text message: {}", text);
                    
                    // Parse the command
                    match serde_json::from_str::<ClientCommand>(&text) {
                        Ok(command) => {
                            match command {
                                ClientCommand::Play { recording_id, rate } => {
                                    // If there's an existing session, clean it up first
                                    if let Some(session) = &playback_session {
                                        let _ = session.pipeline.set_state(gst::State::Null);
                                    }
                                    
                                    // Create new playback session
                                    match create_playback_pipeline(&recording_id, tx_clone.clone()).await {
                                        Ok(new_session) => {
                                            // Send Ready response with duration
                                            let pipeline = &new_session.pipeline;
                                            let duration = get_pipeline_duration(pipeline);
                                            
                                            // Convert to JSON and send to client
                                            let response = ServerResponse::Ready { duration };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                let _ = tx_clone.send(json.into_bytes()).await;
                                            }
                                            
                                            // Set playback rate if not 1.0
                                            if rate != 1.0 {
                                                set_playback_rate(pipeline, rate);
                                            }
                                            
                                            // Start playback
                                            let _ = pipeline.set_state(gst::State::Playing);
                                            playback_session = Some(new_session);
                                        },
                                        Err(e) => {
                                            error!("Failed to create playback pipeline: {}", e);
                                            // Send error to client
                                            let response = ServerResponse::Error { 
                                                message: format!("Failed to create playback: {}", e) 
                                            };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                let _ = tx_clone.send(json.into_bytes()).await;
                                            }
                                        }
                                    }
                                },
                                ClientCommand::Pause => {
                                    if let Some(session) = &playback_session {
                                        let _ = session.pipeline.set_state(gst::State::Paused);
                                    }
                                },
                                ClientCommand::Seek { position } => {
                                    if let Some(session) = &playback_session {
                                        // Convert position in nanoseconds to ClockTime
                                        let position_clock = gst::ClockTime::from_nseconds(position as u64);
                                        
                                        // Seek using the simplest possible approach
                                        debug!("Seeking to position: {} ns", position);
                                        
                                        // First, pause the pipeline
                                        let _ = session.pipeline.set_state(gst::State::Paused);
                                        
                                        // Execute the seek in a background thread
                                        let pipeline_clone = session.pipeline.clone();
                                        std::thread::spawn(move || {
                                            // Wait a bit for the pipeline to pause
                                            std::thread::sleep(std::time::Duration::from_millis(100));
                                            
                                            // We'll use element methods but avoid the GStreamer Seek API directly
                                            // Create a playbin element instead from the pipeline
                                            let elements = pipeline_clone.children();
                                            for element in elements {
                                                if let Some(playbin) = element.downcast_ref::<gst::Element>() {
                                                    // Set position property directly if possible
                                                    let has_prop = playbin.find_property("current-position").is_some();
                                                    if has_prop {
                                                        playbin.set_property("current-position", position_clock.nseconds());
                                                        debug!("Set position directly on playbin");
                                                        return;
                                                    }
                                                }
                                            }
                                            
                                            // If direct property setting failed, try a simpler approach
                                            warn!("Using fallback seek approach - pause/play cycle");
                                            let _ = pipeline_clone.set_state(gst::State::Paused);
                                            std::thread::sleep(std::time::Duration::from_millis(500));
                                            let _ = pipeline_clone.set_state(gst::State::Playing);
                                        });
                                    }
                                },
                                ClientCommand::Stop => {
                                    if let Some(session) = &playback_session {
                                        let _ = session.pipeline.set_state(gst::State::Null);
                                    }
                                    playback_session = None;
                                },
                                ClientCommand::SetRate { rate } => {
                                    if let Some(session) = &playback_session {
                                        set_playback_rate(&session.pipeline, rate);
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            error!("Failed to parse client command: {}. Message was: {}", e, text);
                            
                            // Try alternate parsing method using the raw struct
                            match serde_json::from_str::<ClientCommandRaw>(&text) {
                                Ok(raw_cmd) => {
                                    info!("Successfully parsed as raw command: type={}", raw_cmd.type_);
                                    
                                    // Convert raw command to proper enum command
                                    match raw_cmd.type_.as_str() {
                                        "play" => {
                                            if let (Some(recording_id), Some(rate)) = (raw_cmd.recording_id, raw_cmd.rate) {
                                                // If there's an existing session, clean it up first
                                                if let Some(session) = &playback_session {
                                                    let _ = session.pipeline.set_state(gst::State::Null);
                                                }
                                                
                                                // Create new playback session
                                                match create_playback_pipeline(&recording_id, tx_clone.clone()).await {
                                                    Ok(new_session) => {
                                                        // Send Ready response with duration
                                                        let pipeline = &new_session.pipeline;
                                                        let duration = get_pipeline_duration(pipeline);
                                                        
                                                        // Convert to JSON and send to client
                                                        let response = ServerResponse::Ready { duration };
                                                        if let Ok(json) = serde_json::to_string(&response) {
                                                            let _ = tx_clone.send(json.into_bytes()).await;
                                                        }
                                                        
                                                        // Set playback rate if not 1.0
                                                        if rate != 1.0 {
                                                            set_playback_rate(pipeline, rate);
                                                        }
                                                        
                                                        // Start playback
                                                        let _ = pipeline.set_state(gst::State::Playing);
                                                        playback_session = Some(new_session);
                                                    },
                                                    Err(e) => {
                                                        error!("Failed to create playback pipeline: {}", e);
                                                        // Send error to client
                                                        let response = ServerResponse::Error { 
                                                            message: format!("Failed to create playback: {}", e) 
                                                        };
                                                        if let Ok(json) = serde_json::to_string(&response) {
                                                            let _ = tx_clone.send(json.into_bytes()).await;
                                                        }
                                                    }
                                                }
                                            } else {
                                                let response = ServerResponse::Error { 
                                                    message: "Play command missing required parameters".to_string() 
                                                };
                                                if let Ok(json) = serde_json::to_string(&response) {
                                                    let _ = tx_clone.send(json.into_bytes()).await;
                                                }
                                            }
                                        },
                                        "pause" => {
                                            if let Some(session) = &playback_session {
                                                let _ = session.pipeline.set_state(gst::State::Paused);
                                            }
                                        },
                                        "seek" => {
                                            if let (Some(position), Some(session)) = (raw_cmd.position, &playback_session) {
                                                // Convert position in nanoseconds to ClockTime
                                                let position_clock = gst::ClockTime::from_nseconds(position as u64);
                                                
                                                // First, pause the pipeline
                                                let _ = session.pipeline.set_state(gst::State::Paused);
                                                
                                                // Execute the seek in a background thread
                                                let pipeline_clone = session.pipeline.clone();
                                                std::thread::spawn(move || {
                                                    // Wait a bit for the pipeline to pause
                                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                                    
                                                    // Similar seek logic as before...
                                                    let elements = pipeline_clone.children();
                                                    for element in elements {
                                                        if let Some(playbin) = element.downcast_ref::<gst::Element>() {
                                                            // Set position property directly if possible
                                                            let has_prop = playbin.find_property("current-position").is_some();
                                                            if has_prop {
                                                                playbin.set_property("current-position", position_clock.nseconds());
                                                                debug!("Set position directly on playbin");
                                                                return;
                                                            }
                                                        }
                                                    }
                                                    
                                                    warn!("Using fallback seek approach - pause/play cycle");
                                                    let _ = pipeline_clone.set_state(gst::State::Paused);
                                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                                    let _ = pipeline_clone.set_state(gst::State::Playing);
                                                });
                                            }
                                        },
                                        "stop" => {
                                            if let Some(session) = &playback_session {
                                                let _ = session.pipeline.set_state(gst::State::Null);
                                            }
                                            playback_session = None;
                                        },
                                        "setRate" => {
                                            if let (Some(rate), Some(session)) = (raw_cmd.rate, &playback_session) {
                                                set_playback_rate(&session.pipeline, rate);
                                            }
                                        },
                                        _ => {
                                            let response = ServerResponse::Error { 
                                                message: format!("Unknown command type: {}", raw_cmd.type_) 
                                            };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                let _ = tx_clone.send(json.into_bytes()).await;
                                            }
                                        }
                                    }
                                },
                                Err(_) => {
                                    // If both parsing methods fail, send an error response
                                    let response = ServerResponse::Error { 
                                        message: "Invalid command format".to_string() 
                                    };
                                    if let Ok(json) = serde_json::to_string(&response) {
                                        let _ = tx_clone.send(json.into_bytes()).await;
                                    }
                                }
                            }
                        }
                    }
                },
                Message::Binary(_) => {
                    // We don't expect binary messages from client
                    warn!("Unexpected binary message from client");
                },
                Message::Ping(ping) => {
                    // Respond to ping with pong
                    if let Err(e) = sender.lock().await.send(Message::Pong(ping)).await {
                        error!("Failed to send pong: {}", e);
                    }
                },
                Message::Pong(_) => {
                    // Ignore pong messages
                },
                Message::Close(_) => {
                    info!("Client closed the connection");
                    break;
                }
            }
        }
        
        // Clean up when loop exits
        if let Some(session) = &playback_session {
            let _ = session.pipeline.set_state(gst::State::Null);
        }
    });
    
    // Task to send status updates and video data to client
    let sender_task = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            // Lock the sender for the duration of this operation
            let mut ws_sender = sender_clone.lock().await;
            
            // Check if this is a JSON control message or video data
            if let Ok(text) = std::str::from_utf8(&data) {
                if text.starts_with('{') {
                    // This is a control message, send as text
                    if let Err(e) = ws_sender.send(Message::Text(text.to_string())).await {
                        error!("Failed to send control message: {}", e);
                        break;
                    }
                } else {
                    // This is binary video data
                    if let Err(e) = ws_sender.send(Message::Binary(data)).await {
                        error!("Failed to send video data: {}", e);
                        break;
                    }
                }
            } else {
                // Binary data
                if let Err(e) = ws_sender.send(Message::Binary(data)).await {
                    error!("Failed to send video data: {}", e);
                    break;
                }
            }
        }
    });
    
    // Wait for either task to finish
    tokio::select! {
        _ = receiver_task => debug!("Receiver task completed"),
        _ = sender_task => debug!("Sender task completed"),
    }
    
    info!("WebSocket connection closed");
}

// Create a GStreamer pipeline for playback with playbin
async fn create_playback_pipeline(recording_id: &str, tx: mpsc::Sender<Vec<u8>>) -> Result<PlaybackSession> {
    let recording_uuid = Uuid::parse_str(recording_id)
        .map_err(|e| anyhow!("Invalid recording ID: {}", e))?;
    
    // Get recording file path from database
    let db_pool = crate::db::get_connection_pool().await?;
    let recordings_repo = crate::db::repositories::recordings::RecordingsRepository::new(Arc::new(db_pool));
    let recording = recordings_repo.get_by_id(&recording_uuid).await?
        .ok_or_else(|| anyhow!("Recording not found with ID: {}", recording_id))?;
    
    // Make sure the file exists
    let file_path = recording.file_path.to_str()
        .ok_or_else(|| anyhow!("Invalid file path"))?;
    
    if !std::path::Path::new(file_path).exists() {
        return Err(anyhow!("Recording file not found: {}", file_path));
    }
    
    // Create a playbin pipeline with a unique name
    let pipeline = gst::Pipeline::builder()
        .name(format!("playback-{}", recording_id))
        .build();
    
    // Create playbin element
    let playbin = gst::ElementFactory::make("playbin")
        .name(&format!("playbin-{}", recording_id))
        .property("uri", &format!("file://{}", file_path))
        .build()?;
    
    // Create appsink for video
    let video_sink = gst_app::AppSink::builder()
        .name(&format!("videosink-{}", recording_id))
        .max_buffers(5)
        .drop(true)
        .wait_on_eos(false)
        .sync(false)
        .enable_last_sample(false)
        .build();
    
    // Set caps for appsink to receive raw video frames
    let video_caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGB")
        .build();
    video_sink.set_caps(Some(&video_caps));
    
    // Add elements to pipeline
    pipeline.add(&playbin)?;
    
    // Set appsink as video-sink
    playbin.set_property("video-sink", &video_sink);
    
    // Create an audio sink (fakesink for now, could be replaced with audio streaming later)
    let audio_sink = gst::ElementFactory::make("fakesink")
        .name(&format!("audiosink-{}", recording_id))
        .property("sync", true)
        .build()?;
    
    // Set audio-sink
    playbin.set_property("audio-sink", &audio_sink);
    
    // Set up appsink callbacks to handle video frames
    let sink_clone = Arc::new(Mutex::new(video_sink.clone()));
    let tx_clone = tx.clone();
    
    // This will run in the GStreamer thread
    video_sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                let sample = match appsink.pull_sample() {
                    Ok(sample) => sample,
                    Err(e) => {
                        error!("Failed to pull sample: {:?}", e);
                        return Err(gst::FlowError::Error);
                    }
                };
                
                let buffer = match sample.buffer() {
                    Some(buffer) => buffer,
                    None => {
                        error!("No buffer in sample");
                        return Ok(gst::FlowSuccess::Ok);
                    }
                };
                
                let map = match buffer.map_readable() {
                    Ok(map) => map,
                    Err(e) => {
                        error!("Failed to map buffer: {:?}", e);
                        return Ok(gst::FlowSuccess::Ok);
                    }
                };
                
                // Clone the frame data and send it through the channel
                let data = map.as_slice().to_vec();
                let tx = tx_clone.clone();
                
                // We can't use tokio::spawn directly in the GStreamer thread as it's not inside the Tokio runtime
                // Instead, spawn a std::thread to bridge GStreamer and Tokio
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.spawn(async move {
                        if let Err(e) = tx.send(data).await {
                            warn!("Failed to send video data: {}", e);
                        }
                    });
                });
                
                Ok(gst::FlowSuccess::Ok)
            })
            .build()
    );
    
    // Set up a bus watch to handle pipeline messages
    let tx_state = tx.clone();
    let pipeline_clone = pipeline.clone();
    let bus = pipeline.bus().ok_or_else(|| anyhow!("Failed to get pipeline bus"))?;
    bus.add_watch(move |_, msg| {
        match msg.view() {
            gst::MessageView::Eos(_) => {
                info!("End of stream");
                
                // Notify client about EOS
                let status = ServerResponse::Status {
                    state: "eos".to_string(),
                    position: get_pipeline_position(&pipeline_clone),
                    duration: get_pipeline_duration(&pipeline_clone),
                };
                
                if let Ok(json) = serde_json::to_string(&status) {
                    // Convert to bytes
                    let data = json.into_bytes();
                    
                    // Clone the sender before moving it into the closure
                    let tx_eos = tx_state.clone();
                    
                    // We need to run this on the Tokio runtime
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Handle::current();
                        rt.spawn(async move {
                            if let Err(e) = tx_eos.send(data).await {
                                warn!("Failed to send EOS status: {}", e);
                            }
                        });
                    });
                }
                
                let _ = pipeline_clone.set_state(gst::State::Ready);
            },
            gst::MessageView::Error(err) => {
                error!(
                    "Error from {}: {} ({})",
                    err.src().map(|s| s.name()).unwrap_or_else(|| "unknown".into()),
                    err.error(),
                    err.debug().unwrap_or_else(|| "no debug info".into())
                );
                
                // Notify client about error
                let error_msg = format!("{}", err.error());
                let status = ServerResponse::Error {
                    message: error_msg,
                };
                
                if let Ok(json) = serde_json::to_string(&status) {
                    // Convert to bytes
                    let data = json.into_bytes();
                    
                    // Clone the sender before moving it into the closure
                    let tx_error = tx_state.clone();
                    
                    // We need to run this on the Tokio runtime
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Handle::current();
                        rt.spawn(async move {
                            if let Err(e) = tx_error.send(data).await {
                                warn!("Failed to send error status: {}", e);
                            }
                        });
                    });
                }
            },
            gst::MessageView::StateChanged(state_changed) => {
                if let Some(element) = state_changed.src() {
                    if element.type_() == pipeline_clone.type_() {
                        debug!(
                            "Pipeline state changed from {:?} to {:?}",
                            state_changed.old(),
                            state_changed.current()
                        );
                        
                        // Send status update to client
                        let status = ServerResponse::Status {
                            state: format!("{:?}", state_changed.current()).to_lowercase(),
                            position: get_pipeline_position(&pipeline_clone),
                            duration: get_pipeline_duration(&pipeline_clone),
                        };
                        
                        if let Ok(json) = serde_json::to_string(&status) {
                            // Convert to bytes
                            let data = json.into_bytes();
                            
                            // Clone the sender before moving it into the closure
                            let tx_status = tx_state.clone();
                            
                            // We need to run this on the Tokio runtime
                            std::thread::spawn(move || {
                                let rt = tokio::runtime::Handle::current();
                                rt.spawn(async move {
                                    if let Err(e) = tx_status.send(data).await {
                                        warn!("Failed to send state changed status: {}", e);
                                    }
                                });
                            });
                        }
                    }
                }
            },
            _ => {},
        }
        
        gst::glib::ControlFlow::Continue
    })?;
    
    // Prepare pipeline (moves to PAUSED state)
    let ret = pipeline.set_state(gst::State::Paused)?;
    
    // Check the state change result
    match ret {
        gst::StateChangeSuccess::Success => {
            debug!("Pipeline state change to PAUSED succeeded immediately");
        },
        gst::StateChangeSuccess::Async => {
            debug!("Pipeline state change to PAUSED in progress (async)");
            
            // Wait for a moment to allow the change to complete
            std::thread::sleep(std::time::Duration::from_millis(500));
        },
        gst::StateChangeSuccess::NoPreroll => {
            debug!("Pipeline state change to PAUSED succeeded (no preroll)");
        },
    }
    
    // Create playback session
    let session = PlaybackSession {
        pipeline,
        recording_id: recording_uuid,
        sink: sink_clone,
    };
    
    Ok(session)
}

// Set playback rate (including reverse playback support)
fn set_playback_rate(pipeline: &gst::Pipeline, rate: f64) {
    debug!("Setting playback rate to: {}", rate);
    
    // Find all elements in the pipeline to try setting properties
    let elements = pipeline.children();
    let mut rate_set = false;
    
    for element in elements {
        // Try to find playbin or other elements with rate property
        if element.find_property("playback-rate").is_some() {
            element.set_property("playback-rate", rate);
            rate_set = true;
            debug!("Set playback-rate property on {}", element.name());
        } else if element.find_property("rate").is_some() {
            element.set_property("rate", rate);
            rate_set = true;
            debug!("Set rate property on {}", element.name());
        }
    }
    
    if !rate_set {
        warn!("Could not find any element to set playback rate");
    }
    
    // Also control playback state based on rate
    if rate != 0.0 {
        let _ = pipeline.set_state(gst::State::Playing);
    } else {
        let _ = pipeline.set_state(gst::State::Paused);
    }
}

// Get current position in nanoseconds
fn get_pipeline_position(pipeline: &gst::Pipeline) -> i64 {
    let position = pipeline
        .query_position::<gst::ClockTime>()
        .unwrap_or(gst::ClockTime::ZERO);
    
    // Convert to i64 nanoseconds
    position.nseconds() as i64
}

// Get duration in nanoseconds
fn get_pipeline_duration(pipeline: &gst::Pipeline) -> i64 {
    let duration = pipeline
        .query_duration::<gst::ClockTime>()
        .unwrap_or(gst::ClockTime::ZERO);
    
    // Convert to i64 nanoseconds
    duration.nseconds() as i64
}

// Spawn a task to send periodic status updates
async fn start_status_update_task(
    pipeline: Arc<gst::Pipeline>,
    tx: mpsc::Sender<Vec<u8>>,
    interval_ms: u64,
) -> task::JoinHandle<()> {
    task::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        
        loop {
            interval.tick().await;
            
            // Check if pipeline is still active
            let timeout = gst::ClockTime::from_mseconds(100);
            let (ret, state, _pending) = pipeline.state(timeout);
            
            // If pipeline is in NULL state, exit the loop
            if state == gst::State::Null {
                debug!("Pipeline is no longer active (state: NULL)");
                break;
            }
            
            // Send status update
            let status = ServerResponse::Status {
                state: format!("{:?}", state).to_lowercase(),
                position: get_pipeline_position(&pipeline),
                duration: get_pipeline_duration(&pipeline),
            };
            
            if let Ok(json) = serde_json::to_string(&status) {
                if let Err(e) = tx.send(json.into_bytes()).await {
                    error!("Failed to send status update: {}", e);
                    break;
                }
            }
        }
    })
}