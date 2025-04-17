use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread;
use anyhow::Result;
use g_streamer::{
    StreamManager, StreamSource, StreamType
};
use g_streamer::services::{
    CameraManager, AnalyticsService
};
use g_streamer::services::analytics::{AnalyticsRequest, AnalyticsType};
use gstreamer as gst;

// Import the tutorials_common module using the path attribute
#[path = "../tutorial-common.rs"]
mod tutorials_common;

// A simple analytics demo result
#[derive(Debug)]
struct AnalyticsResult {
    timestamp: std::time::SystemTime,
    frame_number: u32,
    objects_detected: u32,
    confidence: f32,
}

fn run_analytics() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    
    let rtsp_url = if args.len() > 1 {
        args[1].clone()
    } else {
        // Default RTSP URL - replace with your own
        "rtsp://wowzaec2demo.streamlock.net/vod/mp4:BigBuckBunny_115k.mp4".to_string()
    };
    
    println!("RTSP Analytics Service Example");
    println!("-----------------------------");
    println!("Connecting to: {}", rtsp_url);
    
    // Initialize GStreamer
    gst::init()?;
    
    // Create the core stream manager
    let stream_manager = Arc::new(StreamManager::new());
    println!("Stream manager initialized");
    
    // Create our services
    let mut camera_manager = CameraManager::new(stream_manager.clone());
    let mut analytics_service = AnalyticsService::new(stream_manager.clone());
    println!("Services initialized");
    
    // Add a network camera
    let camera_id = camera_manager.add_camera(
        "RTSP Camera".to_string(),
        rtsp_url,
        Some("Network camera via RTSP".to_string())
    )?;
    println!("Added network camera with ID: {}", camera_id);
    
    // Create a network stream
    let source = StreamSource {
        stream_type: StreamType::Network,
        uri: camera_manager.get_camera(&camera_id).unwrap().device_path.clone(),
        name: "RTSP Stream".to_string(),
        description: Some("Network camera stream".to_string()),
    };
    
    let stream_id = stream_manager.add_stream(source)?;
    println!("Created stream with ID: {}", stream_id);
    
    // Configure analytics
    let analytics_config = HashMap::from([
        ("detection_threshold".to_string(), "0.5".to_string()),
        ("frame_interval".to_string(), "5".to_string()),
        ("object_classes".to_string(), "person,car,dog".to_string()),
    ]);
    
    // Start object detection analytics
    let analytics_id = analytics_service.start_analytics(
        AnalyticsRequest {
            stream_id: stream_id.clone(),
            analytics_type: AnalyticsType::ObjectDetection,
            config: analytics_config,
        }
    )?;
    println!("Started analytics with ID: {}", analytics_id);
    
    // Create a signal handler for ctrl+c
    let running = Arc::new(Mutex::new(true));
    let r = running.clone();
    
    ctrlc::set_handler(move || {
        let mut running = r.lock().unwrap();
        *running = false;
    }).expect("Error setting Ctrl-C handler");
    
    println!("Press Ctrl+C to stop...");
    
    // Simulate analytics results in a loop
    let mut frame_number = 0;
    let start_time = std::time::Instant::now();
    let mut rng = rand::thread_rng();
    
    while *running.lock().unwrap() && start_time.elapsed() < Duration::from_secs(60) {
        // Generate a simulated analytics result every second
        if frame_number % 30 == 0 {  // Assuming 30fps
            let objects_detected = rand::Rng::gen_range(&mut rng, 0..5);
            let confidence = rand::Rng::gen_range(&mut rng, 0.5..0.99);
            
            let result = AnalyticsResult {
                timestamp: std::time::SystemTime::now(),
                frame_number,
                objects_detected,
                confidence,
            };
            
            println!("Analytics result: {:?}", result);
            
            // Every 10 seconds, query results from the service
            if start_time.elapsed().as_secs() % 10 == 0 && start_time.elapsed().subsec_millis() < 100 {
                let results = analytics_service.get_analytics_results(&analytics_id)?;
                println!("Results from service: {:?}", results);
            }
        }
        
        frame_number += 1;
        thread::sleep(Duration::from_millis(33));  // ~30fps
    }
    
    // Clean up
    println!("Stopping analytics...");
    analytics_service.stop_analytics(&analytics_id)?;
    
    println!("Removing camera...");
    camera_manager.remove_camera(&camera_id)?;
    
    println!("Clean exit");
    
    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    // (but not necessary in normal Cocoa applications where this is set up automatically)
    tutorials_common::run(|| {
        if let Err(e) = run_analytics() {
            eprintln!("Error: {}", e);
        }
    });
}