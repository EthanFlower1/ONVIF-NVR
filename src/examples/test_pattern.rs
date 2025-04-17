use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::thread;
use anyhow::Result;
use gstreamer as gst;

use g_streamer::{StreamManager, StreamSource, StreamType, BranchType, BranchConfig};

// Import the tutorials_common module for macOS compatibility
#[path = "../tutorial-common.rs"]
mod tutorials_common;

fn run_test_pattern() -> Result<()> {
    // Initialize GStreamer
    gst::init()?;
    
    println!("Test Pattern Example");
    println!("------------------");
    
    // Create Stream Manager
    let stream_manager = Arc::new(StreamManager::new());
    
    // Setup a test pattern source
    let source = StreamSource {
        stream_type: StreamType::TestSource,
        uri: "18".to_string(), // Pattern 18 (Ball pattern - more interesting to watch)
        name: "Test Pattern".to_string(),
        description: Some("Ball test pattern".to_string()),
    };
    
    // Add the stream to the manager
    let stream_id = stream_manager.add_stream(source)?;
    println!("Created test pattern stream with ID: {}", stream_id);
    
    // Create a viewing config
    let viewing_config = BranchConfig {
        branch_type: BranchType::LiveView,
        output_path: None,
        options: HashMap::new(),
    };
    
    // Add the viewing branch
    let viewing_branch_id = stream_manager.add_branch(&stream_id, viewing_config)?;
    println!("Created viewing branch with ID: {}", viewing_branch_id);
    println!("Displaying test pattern... (will run for 15 seconds)");
    
    // Run for 15 seconds
    for i in 1..=15 {
        thread::sleep(Duration::from_secs(1));
        println!("Running for {} seconds...", i);
    }
    
    // Clean up
    println!("Stopping stream...");
    
    // Remove the branch
    stream_manager.remove_branch(&stream_id, &viewing_branch_id)?;
    println!("Removed viewing branch");
    
    // Remove the stream
    stream_manager.remove_stream(&stream_id)?;
    println!("Removed stream");
    
    println!("Stream stopped successfully");
    
    Ok(())
}

// Main function that uses the tutorials_common wrapper for macOS compatibility
fn main() {
    // tutorials_common::run is required to set up the application environment on macOS
    tutorials_common::run(|| {
        if let Err(e) = run_test_pattern() {
            eprintln!("Error: {}", e);
        }
    });
}