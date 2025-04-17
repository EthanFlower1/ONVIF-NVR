use anyhow::Result;
use gstreamer as gst;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use g_streamer::{BranchConfig, BranchType, StreamManager, StreamSource, StreamType};

// Import the tutorials_common module for macOS compatibility
#[path = "../tutorial-common.rs"]
mod tutorials_common;

fn run_rtsp_live_view() -> Result<()> {
    // Initialize GStreamer
    gst::init()?;

    println!("RTSP Live View with Recording Example");
    println!("-----------------------------------");

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    // Create Stream Manager
    let stream_manager = Arc::new(StreamManager::new());

    // Decide which source to use - RTSP or test pattern
    let rtsp_url = if args.len() > 1 {
        println!("Connecting to RTSP URL: {}", args[1]);
        args[1].clone()
    } else {
        // Use a default URL or fall back to test pattern if no URL provided
        let default_url = "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1";
        println!("No RTSP URL provided, using public test stream");
        println!("Usage: cargo run --example rtsp_live_view [rtsp_url]");
        default_url.to_string()
    };

    // Create the source
    let source = StreamSource {
        stream_type: StreamType::RTSP,
        uri: rtsp_url,
        name: "RTSP Stream".to_string(),
        description: Some("RTSP stream with live view and recording".to_string()),
    };

    // Add the stream to the manager
    let stream_id = stream_manager.add_stream(source)?;
    println!("Created stream with ID: {}", stream_id);

    // // Create a timestamp-based filename for the recording
    // let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    // let recording_path = format!("/tmp/recording_{}.mp4", timestamp);
    //
    // // Add a recording branch with higher quality settings
    // let mut recording_options = HashMap::new();
    // recording_options.insert("bitrate".to_string(), "2000".to_string());
    // recording_options.insert("tune".to_string(), "film".to_string());
    //
    // let recording_config = BranchConfig {
    //     branch_type: BranchType::Recording,
    //     output_path: Some(recording_path.clone()),
    //     options: recording_options,
    // };
    //
    // let recording_branch_id = stream_manager.add_branch(&stream_id, recording_config)?;
    // println!("Created recording branch with ID: {}", recording_branch_id);
    // println!("Recording to: {}", recording_path);
    //
    // Add a live viewing branch with low-latency settings
    let mut viewing_options = HashMap::new();
    viewing_options.insert("sync".to_string(), "false".to_string()); // Disable sync for lower latency

    let viewing_config = BranchConfig {
        branch_type: BranchType::LiveView,
        output_path: None,
        options: viewing_options,
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
    println!("\nStream running with BOTH recording and live view active.");
    println!("You should see the video playing in a window.");
    println!("Press Ctrl+C to stop...");

    let start_time = Instant::now();
    let mut last_status = Instant::now();

    while *running.lock().unwrap() {
        // Print status every 5 seconds
        if last_status.elapsed() >= Duration::from_secs(5) {
            println!(
                "Running for {:.1}s, recording to {}",
                start_time.elapsed().as_secs_f32(),
                "recording_path"
            );
            last_status = Instant::now();
        }

        thread::sleep(Duration::from_millis(100));
    }

    // Clean up
    println!("\nStopping stream...");

    // First remove branches
    stream_manager.remove_branch(&stream_id, &viewing_branch_id)?;
    println!("Removed viewing branch");

    // stream_manager.remove_branch(&stream_id, &recording_branch_id)?;
    // println!("Removed recording branch");

    // Then remove the stream
    stream_manager.remove_stream(&stream_id)?;
    println!("Removed stream");

    // println!("Recording saved to: {}", recording_path);
    println!("Stream stopped successfully");

    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    tutorials_common::run(|| {
        if let Err(e) = run_rtsp_live_view() {
            eprintln!("Error: {}", e);
        }
    });
}

