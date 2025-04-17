use anyhow::Result;
use g_streamer::{
    BranchConfig, BranchId, BranchType, StreamId, StreamManager, StreamSource, StreamType,
};
use gstreamer as gst;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Import the tutorials_common module using the path attribute
#[path = "../tutorial-common.rs"]
mod tutorials_common;

// Import our service modules
use g_streamer::services::{CameraManager, RecordingService, StreamingService};

// A simple frame counter for demo purposes
struct FrameCounter {
    count: u32,
    last_report: std::time::Instant,
}

impl FrameCounter {
    fn new() -> Self {
        Self {
            count: 0,
            last_report: std::time::Instant::now(),
        }
    }

    fn increment(&mut self) {
        self.count += 1;

        // Report every second
        if self.last_report.elapsed() >= Duration::from_secs(1) {
            println!(
                "Frames processed: {} (FPS: {:.2})",
                self.count,
                self.count as f32 / self.last_report.elapsed().as_secs_f32()
            );
            self.count = 0;
            self.last_report = std::time::Instant::now();
        }
    }
}

fn run_service() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    let rtsp_url = if args.len() > 1 {
        args[1].clone()
    } else {
        // Default RTSP URL - replace with your own
        "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1".to_string()
    };

    println!("RTSP Service Example");
    println!("-------------------");
    println!("Connecting to: {}", rtsp_url);

    // Initialize GStreamer (required for our services)
    gst::init()?;

    // 1. Create the stream manager (central service)
    let stream_manager = Arc::new(StreamManager::new());
    println!("Stream manager initialized");

    // 2. Create services using the stream manager
    let mut camera_manager = CameraManager::new(stream_manager.clone());
    let mut recording_service = RecordingService::new(stream_manager.clone());
    let mut streaming_service = StreamingService::new(stream_manager.clone());
    println!("Services initialized");

    // 3. Add a virtual network camera representing the RTSP stream
    let camera_id = camera_manager.add_camera(
        "RTSP Camera".to_string(),
        rtsp_url,
        Some("Network camera via RTSP".to_string()),
    )?;
    println!("Added network camera with ID: {}", camera_id);

    // 4. Start streaming from the camera, using our network stream type
    let source = StreamSource {
        stream_type: StreamType::Network,
        uri: camera_manager
            .get_camera(&camera_id)
            .unwrap()
            .device_path
            .clone(),
        name: "RTSP Stream".to_string(),
        description: Some("Network camera stream".to_string()),
    };

    let stream_id = stream_manager.add_stream(source)?;
    println!("Created stream with ID: {}", stream_id);

    // 5. Add a recording branch with specific options
    let recording_config = BranchConfig {
        branch_type: BranchType::Recording,
        output_path: Some(
            "/Users/ethanflower/projects/g-streamer/test-recordings/rtsp_recording.mp4".to_string(),
        ),
        options: {
            let mut options = HashMap::new();
            options.insert("bitrate".to_string(), "1000".to_string());
            options.insert("preset".to_string(), "medium".to_string());
            options
        },
    };

    let recording_id =
        recording_service.start_recording(g_streamer::services::recording::RecordingRequest {
            stream_id: stream_id.clone(),
            output_path: Some("/tmp".to_string()),
            duration: Some(60), // 60 seconds duration
            quality: Some("medium".to_string()),
        })?;
    println!("Started recording with ID: {}", recording_id);

    // Create a signal handler for ctrl+c
    let running = Arc::new(Mutex::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        let mut running = r.lock().unwrap();
        *running = false;
    })
    .expect("Error setting Ctrl-C handler");

    // Set up a frame counter to show processing is happening
    let mut frame_counter = FrameCounter::new();

    println!("Services running. Press Ctrl+C to stop...");

    // Main loop - run for 60 seconds or until Ctrl+C
    let start_time = std::time::Instant::now();
    while *running.lock().unwrap() && start_time.elapsed() < Duration::from_secs(60) {
        // Simulate frame processing (in a real app, this would come from callbacks)
        frame_counter.increment();

        // Sleep a bit to not hog CPU
        thread::sleep(Duration::from_millis(33)); // ~30 fps
    }

    // Clean up - stop recording
    println!("\nStopping recording...");
    recording_service.stop_recording(&recording_id)?;

    // Remove camera
    println!("Removing camera...");
    camera_manager.remove_camera(&camera_id)?;

    println!("Recording saved to /tmp/rtsp_recording_*.mp4");

    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    // (but not necessary in normal Cocoa applications where this is set up automatically)
    tutorials_common::run(|| {
        if let Err(e) = run_service() {
            eprintln!("Error: {}", e);
        }
    });
}

