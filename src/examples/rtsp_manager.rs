use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::thread;
use anyhow::Result;
use gstreamer as gst;

use g_streamer::{StreamManager, StreamSource, StreamType, BranchType, BranchConfig};

// Import the tutorials_common module for macOS compatibility
#[path = "../tutorial-common.rs"]
mod tutorials_common;

fn run_rtsp_manager() -> Result<()> {
    // Initialize GStreamer
    gst::init()?;
    
    println!("RTSP Stream Manager Example");
    println!("-------------------------");
    
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    // Create Stream Manager
    let stream_manager = Arc::new(StreamManager::new());
    
    // Decide which source to use - RTSP or test pattern
    let source = if args.len() > 1 {
        println!("Connecting to RTSP URL: {}", args[1]);
        StreamSource {
            stream_type: StreamType::RTSP,
            uri: args[1].clone(),
            name: "RTSP Camera".to_string(),
            description: Some("Live RTSP stream from camera".to_string()),
        }
    } else {
        println!("No RTSP URL provided, using test pattern instead");
        println!("Usage: cargo run --example rtsp_manager [rtsp_url]");
        
        StreamSource {
            stream_type: StreamType::TestSource,
            uri: "0".to_string(), // Pattern 0 (SMPTE color bars)
            name: "Test Pattern".to_string(),
            description: Some("SMPTE color bars test pattern".to_string()),
        }
    };
    
    // Add the stream to the manager
    let stream_id = stream_manager.add_stream(source)?;
    println!("Created stream with ID: {}", stream_id);
    
    // Create a timestamp-based filename for the recording
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let recording_path = format!("/tmp/recording_{}.mp4", timestamp);
    
    // Add a recording branch
    let recording_config = BranchConfig {
        branch_type: BranchType::Recording,
        output_path: Some(recording_path.clone()),
        options: HashMap::new(),
    };
    
    let recording_branch_id = stream_manager.add_branch(&stream_id, recording_config)?;
    println!("Created recording branch with ID: {}", recording_branch_id);
    println!("Recording to: {}", recording_path);
    
    // Add a live viewing branch
    let viewing_config = BranchConfig {
        branch_type: BranchType::LiveView,
        output_path: None,
        options: HashMap::new(),
    };
    
    let viewing_branch_id = stream_manager.add_branch(&stream_id, viewing_config)?;
    println!("Created live viewing branch with ID: {}", viewing_branch_id);
    
    // Create a signal handler for ctrl+c
    let running = Arc::new(std::sync::Mutex::new(true));
    let r = running.clone();
    
    ctrlc::set_handler(move || {
        let mut running = r.lock().unwrap();
        *running = false;
    })
    .expect("Error setting Ctrl-C handler");
    
    // Main loop with status updates
    println!("Stream running. Press Ctrl+C to stop...");
    let start_time = Instant::now();
    let mut last_status = Instant::now();
    
    while *running.lock().unwrap() {
        // Print status every 5 seconds
        if last_status.elapsed() >= Duration::from_secs(5) {
            println!("Running for {:.1}s, recording to {}", 
                start_time.elapsed().as_secs_f32(), 
                recording_path);
            last_status = Instant::now();
        }
        
        thread::sleep(Duration::from_millis(100));
    }
    
    // Clean up
    println!("\nStopping stream...");
    
    // First remove branches
    stream_manager.remove_branch(&stream_id, &viewing_branch_id)?;
    println!("Removed viewing branch");
    
    stream_manager.remove_branch(&stream_id, &recording_branch_id)?;
    println!("Removed recording branch");
    
    // Then remove the stream
    stream_manager.remove_stream(&stream_id)?;
    println!("Removed stream");
    
    println!("Recording saved to: {}", recording_path);
    println!("Stream stopped successfully");
    
    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    tutorials_common::run(|| {
        if let Err(e) = run_rtsp_manager() {
            eprintln!("Error: {}", e);
        }
    });
}