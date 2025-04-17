use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::thread;
use anyhow::Result;
use gstreamer as gst;

use g_streamer::{StreamManager, StreamSource, StreamType, BranchType, BranchConfig};

fn main() -> Result<()> {
    // Initialize GStreamer
    gst::init()?;
    
    // Create Stream Manager
    let stream_manager = Arc::new(StreamManager::new());
    
    // Use a test source which works on all platforms
    let source = StreamSource {
        stream_type: StreamType::TestSource,
        uri: "0".to_string(), // Pattern 0 (SMPTE color bars)
        name: "Test Source".to_string(),
        description: Some("SMPTE color bars test pattern".to_string()),
    };
    
    // Add the stream to the manager
    let stream_id = stream_manager.add_stream(source)?;
    println!("Created stream with ID: {}", stream_id);
    
    // Add a recording branch
    let recording_config = BranchConfig {
        branch_type: BranchType::Recording,
        output_path: Some("/tmp/recording.mp4".to_string()),
        options: HashMap::new(),
    };
    
    let recording_branch_id = stream_manager.add_branch(&stream_id, recording_config)?;
    println!("Created recording branch with ID: {}", recording_branch_id);
    
    // Add a live viewing branch
    let viewing_config = BranchConfig {
        branch_type: BranchType::LiveView,
        output_path: None,
        options: HashMap::new(),
    };
    
    let viewing_branch_id = stream_manager.add_branch(&stream_id, viewing_config)?;
    println!("Created live viewing branch with ID: {}", viewing_branch_id);
    
    // Record for 10 seconds
    println!("Recording for 10 seconds...");
    thread::sleep(Duration::from_secs(10));
    
    // Remove branches
    stream_manager.remove_branch(&stream_id, &recording_branch_id)?;
    println!("Removed recording branch");
    
    stream_manager.remove_branch(&stream_id, &viewing_branch_id)?;
    println!("Removed viewing branch");
    
    // Remove stream
    stream_manager.remove_stream(&stream_id)?;
    println!("Removed stream");
    
    Ok(())
}