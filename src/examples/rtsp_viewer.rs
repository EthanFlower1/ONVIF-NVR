use anyhow::Result;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// Import the tutorials_common module using the path attribute
#[path = "../tutorial-common.rs"]
mod tutorials_common;

fn run_viewer() -> Result<()> {
    // Parse command line arguments
    // let args: Vec<String> = std::env::args().collect();

    let rtsp_url = "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1".to_string();
    println!("RTSP Viewer Example");
    println!("-------------------");
    println!("Connecting to: {}", rtsp_url);

    // Initialize GStreamer
    gst::init()?;

    // Create a cross-platform display pipeline
    let display_element = if cfg!(target_os = "linux") {
        "xvimagesink"
    } else if cfg!(target_os = "macos") {
        "osxvideosink"
    } else if cfg!(target_os = "windows") {
        "d3dvideosink"
    } else {
        "autovideosink" // Default fallback
    };

    // Create a pipeline for RTSP stream with display
    let pipeline_str = format!(
        "rtspsrc location={} latency=100 ! rtph264depay ! h264parse ! avdec_h264 ! videoconvert ! {} sync=false",
        rtsp_url, display_element
    );

    println!("Creating pipeline: {}", pipeline_str);

    let pipeline = gst::parse::launch(&pipeline_str)?;
    let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();

    // Start playing
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

    // Track and display stats
    let start_time = Instant::now();
    let mut last_report = Instant::now();

    // Main loop
    while *running.lock().unwrap() {
        // Report every 5 seconds
        if last_report.elapsed() >= Duration::from_secs(5) {
            let position = pipeline
                .query_position::<gst::ClockTime>()
                .unwrap_or(gst::ClockTime::ZERO);

            println!(
                "Stream running for {:.1} seconds, position: {:.1} seconds",
                start_time.elapsed().as_secs_f32(),
                position.seconds() as f32
            );

            last_report = Instant::now();
        }

        thread::sleep(Duration::from_millis(100));

        // Check for messages on the bus
        let bus = pipeline.bus().unwrap();
        while let Some(msg) = bus.pop() {
            use gst::MessageView;
            match msg.view() {
                MessageView::Error(err) => {
                    eprintln!(
                        "Error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );

                    // Try to restart pipeline on error
                    pipeline.set_state(gst::State::Null)?;
                    pipeline.set_state(gst::State::Playing)?;
                    println!("Restarting pipeline after error...");
                }
                MessageView::Eos(..) => {
                    println!("End of stream");

                    // For VOD streams, let's restart the stream
                    pipeline.set_state(gst::State::Null)?;
                    pipeline.set_state(gst::State::Playing)?;
                    println!("Restarting stream after EOS...");
                }
                _ => (),
            }
        }
    }

    // Clean up
    pipeline.set_state(gst::State::Null)?;
    println!("Stream stopped. Exiting...");

    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    // (but not necessary in normal Cocoa applications where this is set up automatically)
    tutorials_common::run(|| {
        if let Err(e) = run_viewer() {
            eprintln!("Error: {}", e);
        }
    });
}

