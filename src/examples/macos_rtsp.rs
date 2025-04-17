use anyhow::Result;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// Import the tutorials_common module using the path attribute
#[path = "../tutorial-common.rs"]
mod tutorials_common;

fn run_rtsp_playback() -> Result<()> {
    // Parse command line arguments
    let args: std::env::Args = std::env::args();
    let args: Vec<String> = args.collect();

    let rtsp_url = if args.len() > 1 {
        args[1].clone()
    } else {
        // Default sample RTSP stream that is publicly available
        "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1".to_string()
    };

    println!("macOS RTSP Viewer Example");
    println!("------------------------");
    println!("Connecting to: {}", rtsp_url);

    // Initialize GStreamer
    gst::init()?;

    // Creating a simpler pipeline for macOS
    let pipeline_str = format!(
        "rtspsrc location={} latency=200 ! decodebin ! videoconvert ! autovideosink sync=false",
        rtsp_url
    );

    println!("Creating pipeline: {}", pipeline_str);

    // Create the pipeline (using gst::parse::launch instead of gst::parse_launch)
    let pipeline = gst::parse::launch(&pipeline_str)?;
    let pipeline = pipeline.dynamic_cast::<gst::Pipeline>().unwrap();

    // Create a bus to watch for messages
    let bus = pipeline.bus().expect("Pipeline without bus");

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

    // Main loop with error handling
    let start_time = Instant::now();
    let mut last_status = Instant::now();

    while *running.lock().unwrap() {
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
            match msg.view() {
                gst::MessageView::Eos(..) => {
                    println!("End of stream");
                    break;
                }
                gst::MessageView::Error(err) => {
                    eprintln!(
                        "Error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );
                    println!("Stopping due to error...");
                    break;
                }
                gst::MessageView::StateChanged(state_changed) => {
                    if let Some(element) = state_changed.src() {
                        if element.downcast_ref::<gst::Pipeline>().is_some() {
                            println!(
                                "Pipeline state changed from {:?} to {:?}",
                                state_changed.old(),
                                state_changed.current()
                            );
                        }
                    }
                }
                _ => (),
            }
        }

        // Print status every 5 seconds
        if last_status.elapsed() >= Duration::from_secs(5) {
            if let Some(position) = pipeline.query_position::<gst::ClockTime>() {
                println!(
                    "Running for {:.1}s, stream position: {:.1}s",
                    start_time.elapsed().as_secs_f32(),
                    position.seconds() as f32 // Already in seconds
                );
            }
            last_status = Instant::now();
        }
    }

    // Cleanup
    println!("Stopping playback...");
    let _ = pipeline.set_state(gst::State::Null);
    println!("Playback stopped");

    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    tutorials_common::run(|| {
        if let Err(e) = run_rtsp_playback() {
            eprintln!("Error: {}", e);
        }
    });
}

