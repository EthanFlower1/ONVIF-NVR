use std::sync::Arc;
use anyhow::{Result, anyhow};
use log::{debug, info, warn, error};
use tokio::sync::Mutex;
use gstreamer as gst;
use gst::prelude::*;

#[path = "./tutorial-common.rs"]
mod tutorials_common;

mod stream_manager;
mod services;
mod api;
mod config;

use stream_manager::StreamManager;
use services::{CameraManager, RecordingService, StreamingService, AnalyticsService};
use config::AppConfig;

async fn run_app() -> Result<()> {
    // Initialize logging
    env_logger::init();
    info!("Starting G-Streamer Stream Management System");
    
    // Initialize GStreamer
    gst::init()?;
    info!("GStreamer initialized successfully");
    
    // Load configuration
    let config = config::setup_config()?;
    info!("Configuration loaded");
    
    // Create shared stream manager
    let stream_manager = Arc::new(StreamManager::new());
    info!("Stream manager initialized");
    
    // Initialize services with shared stream manager
    let camera_manager = Arc::new(Mutex::new(CameraManager::new(stream_manager.clone())));
    let recording_service = Arc::new(Mutex::new(RecordingService::new(stream_manager.clone())));
    let streaming_service = Arc::new(Mutex::new(StreamingService::new(stream_manager.clone())));
    let analytics_service = Arc::new(Mutex::new(AnalyticsService::new(stream_manager.clone())));
    info!("Services initialized");
    
    // Start API servers
    api::rest::setup_rest_api(
        camera_manager.clone(),
        recording_service.clone(),
        streaming_service.clone(),
        analytics_service.clone(),
    ).await?;
    
    let websocket_api = api::websocket::setup_websocket_api(
        camera_manager.clone(),
        recording_service.clone(),
        streaming_service.clone(),
        analytics_service.clone(),
    ).await?;
    info!("API servers started");
    
    // For demo purposes, add a sample camera
    {
        let mut camera_manager = camera_manager.lock().await;
        
        // Variable to hold the active stream ID
        let stream_id: String;
        
        // First, try to use a real camera
        let real_camera_id = add_real_camera(&mut camera_manager)?;
        
        // If successful, use the real camera
        if let Some(camera_id) = real_camera_id {
            info!("Added real camera with ID: {}", camera_id);
            
            // Start streaming from the camera
            stream_id = camera_manager.start_camera_stream(&camera_id)?;
            info!("Started real camera stream with ID: {}", stream_id);
        } else {
            // Fall back to a test source
            info!("No real camera available, using test source");
            
            let test_source = stream_manager::StreamSource {
                stream_type: stream_manager::StreamType::TestSource,
                uri: "0".to_string(), // Pattern 0 (SMPTE color bars)
                name: "Test Source".to_string(),
                description: Some("Fallback test pattern".to_string()),
            };
            
            stream_id = stream_manager.add_stream(test_source)?;
            info!("Started test source stream with ID: {}", stream_id);
        }
        
        // Start recording using the active stream
        let mut recording_service = recording_service.lock().await;
        let recording_id = recording_service.start_recording(
            services::recording::RecordingRequest {
                stream_id: stream_id,
                output_path: Some(config.recording_dir.to_string_lossy().to_string()),
                duration: Some(60), // 60 seconds
                quality: Some("medium".to_string()),
            },
        )?;
        info!("Started sample recording with ID: {}", recording_id);
    }
    
    info!("G-Streamer Stream Management System running");
    
    // In a real application, we would wait for termination signals
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    
    Ok(())
}

// Helper function to try adding a real camera
// Returns Some(camera_id) if successful, None if no camera is available
fn add_real_camera(camera_manager: &mut CameraManager) -> Result<Option<String>> {
    // Select appropriate camera device identifier based on platform
    #[cfg(target_os = "linux")]
    let device_path = "/dev/video0".to_string();
    
    #[cfg(target_os = "macos")]
    let device_path = "0".to_string(); // Use device index 0 for macOS
    
    #[cfg(target_os = "windows")]
    let device_path = "0".to_string(); // Use device index 0 for Windows
    
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let device_path = "0".to_string(); // Default device
    
    let camera_id = camera_manager.add_camera(
        "Real Camera".to_string(),
        device_path,
        Some("Physical camera device".to_string()),
    )?;
    
    // Try to start the camera to see if it works
    // Since we don't have a way to check if the camera is available before starting,
    // we'll try to start it and see if it fails
    match camera_manager.start_camera_stream(&camera_id) {
        Ok(_) => {
            // Camera started successfully, stop it for now
            camera_manager.stop_camera_stream(&camera_id)?;
            Ok(Some(camera_id))
        },
        Err(_) => {
            // Camera failed to start, remove it
            let _ = camera_manager.remove_camera(&camera_id);
            Ok(None)
        }
    }
}

fn main() {
    // For backward compatibility, we're using the tutorial_common's run wrapper
    // In a real app, you might want to use tokio::runtime directly
    tutorials_common::run(|| {
        // Create a tokio runtime in the current thread
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        
        // Run our async main function
        if let Err(e) = runtime.block_on(run_app()) {
            eprintln!("Application error: {}", e);
        }
    });
}

// Original tutorial code - kept for reference
fn tutorial_main() {
    // Initialize GStreamer
    gst::init().unwrap();

    // Build the pipeline
    let uri = "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1";
    let pipeline_str = format!(
    "playbin uri={} video-sink=\"videoconvert ! autovideosink sync=false\" audio-sink=\"audioconvert ! autoaudiosink sync=false\"",
    uri
    );
    let pipeline = gst::parse::launch(&pipeline_str).unwrap();

    // Start playing
    pipeline
        .set_state(gst::State::Playing)
        .expect("Unable to set the pipeline to the `Playing` state");

    // Wait until error or EOS
    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => (),
        }
    }

    // Shutdown pipeline
    pipeline
        .set_state(gst::State::Null)
        .expect("Unable to set the pipeline to the `Null` state");
}