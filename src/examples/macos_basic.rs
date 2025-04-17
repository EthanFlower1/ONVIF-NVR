use anyhow::Result;
use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread;

// Import the tutorials_common module using the path attribute
#[path = "../tutorial-common.rs"]
mod tutorials_common;

fn run_macos_basic() -> Result<()> {
    println!("GStreamer Basic macOS Example");
    println!("----------------------------");
    
    // Initialize GStreamer
    gst::init()?;
    
    // Create a minimal pipeline that should work on macOS
    // - videotestsrc: Generates a test pattern
    // - videoconvert: Converts between video formats (important for compatibility)
    // - osxvideosink: macOS-specific video sink that should work reliably
    let pipeline_str = "videotestsrc ! videoconvert ! osxvideosink";
    println!("Creating pipeline: {}", pipeline_str);
    
    let pipeline = gst::parse::launch(pipeline_str)?;
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
    }).expect("Error setting Ctrl-C handler");
    
    // Main loop
    let bus = pipeline.bus().expect("Pipeline without bus");
    
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
                    break;
                }
                gst::MessageView::StateChanged(state_changed) => {
                    if let Some(element) = state_changed.src() {
                        if element.downcast_ref::<gst::Pipeline>().is_some() {
                            println!("Pipeline state changed from {:?} to {:?}", 
                                     state_changed.old(), state_changed.current());
                        }
                    }
                }
                _ => (),
            }
        }
        
        thread::sleep(Duration::from_millis(100));
    }
    
    // Cleanup
    println!("Stopping pipeline...");
    pipeline.set_state(gst::State::Null)?;
    println!("Pipeline stopped");
    
    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    tutorials_common::run(|| {
        if let Err(e) = run_macos_basic() {
            eprintln!("Error: {}", e);
        }
    });
}