use anyhow::Result;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Import the tutorials_common module using the path attribute
#[path = "../tutorial-common.rs"]
mod tutorials_common;

// A struct to keep track of frame statistics
struct FrameStats {
    frame_count: u32,
    last_fps_update: std::time::Instant,
    fps: f32,
}

impl FrameStats {
    fn new() -> Self {
        Self {
            frame_count: 0,
            last_fps_update: std::time::Instant::now(),
            fps: 0.0,
        }
    }

    fn update(&mut self) {
        self.frame_count += 1;

        let elapsed = self.last_fps_update.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.frame_count = 0;
            self.last_fps_update = std::time::Instant::now();
            println!("Stream FPS: {:.2}", self.fps);
        }
    }
}

fn run_stream() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    let rtsp_url = if args.len() > 1 {
        args[1].clone()
    } else {
        // Default RTSP URL - replace with your own
        "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1".to_string()
    };

    println!("RTSP Stream Example");
    println!("-------------------");
    println!("Connecting to: {}", rtsp_url);

    // Initialize GStreamer
    gst::init()?;

    // Create a custom pipeline for RTSP stream
    let pipeline_str = format!(
        "rtspsrc location={} latency=100 ! rtph264depay ! h264parse ! avdec_h264 ! videoconvert ! tee name=t ! queue ! appsink name=sink", 
        rtsp_url
    );

    println!("Creating pipeline: {}", pipeline_str);

    let pipeline = gst::parse::launch(&pipeline_str)?;
    let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();

    // Get the appsink element
    let appsink = pipeline
        .by_name("sink")
        .expect("Could not find appsink element")
        .dynamic_cast::<gst_app::AppSink>()
        .expect("Element is not an AppSink");

    // Create frame stats tracker
    let frame_stats = Arc::new(Mutex::new(FrameStats::new()));
    let frame_stats_clone = frame_stats.clone();

    // Configure the appsink
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |_appsink| {
                // Update frame stats
                let mut stats = frame_stats_clone.lock().unwrap();
                stats.update();

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    // Start the pipeline
    pipeline.set_state(gst::State::Playing)?;
    println!("Pipeline started. Press Ctrl+C to stop...");

    // Create a signal handler for ctrl+c
    let running = Arc::new(Mutex::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        let mut running = r.lock().unwrap();
        *running = false;
    })
    .expect("Error setting Ctrl-C handler");

    // Main loop
    while *running.lock().unwrap() {
        thread::sleep(Duration::from_millis(100));
    }

    // Stop the pipeline
    println!("Stopping pipeline...");
    pipeline.set_state(gst::State::Null)?;

    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    // (but not necessary in normal Cocoa applications where this is set up automatically)
    tutorials_common::run(|| {
        if let Err(e) = run_stream() {
            eprintln!("Error: {}", e);
        }
    });
}

