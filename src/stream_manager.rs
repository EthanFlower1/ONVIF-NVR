use anyhow::{anyhow, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

pub type StreamId = String;
pub type BranchId = String;

// Stream types supported by our system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamType {
    RTSP,       // RTSP video streams
    TestSource, // Test video pattern for development
}

// Source configuration for a stream
#[derive(Debug, Clone)]
pub struct StreamSource {
    pub stream_type: StreamType,
    pub uri: String,  // RTSP URL or test pattern number
    pub name: String, // Human-readable name
    pub description: Option<String>,
}

// Types of branch we can create from a stream
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchType {
    Recording, // Record to disk
    LiveView,  // Display to screen
}

// Configuration for a branch
#[derive(Debug, Clone)]
pub struct BranchConfig {
    pub branch_type: BranchType,
    pub output_path: Option<String>, // For recording: path to save file
    pub options: HashMap<String, String>,
}

// Internal stream representation
struct Stream {
    source: StreamSource,
    pipeline: gst::Pipeline,
    tee: gst::Element,
    branches: HashMap<BranchId, Branch>,
}

// Internal branch representation
struct Branch {
    bin: gst::Bin,
    queue: gst::Element,
}

/// StreamManager: Core class that manages video streams and their branches
pub struct StreamManager {
    streams: RwLock<HashMap<StreamId, Stream>>,
}

impl StreamManager {
    /// Create a new StreamManager
    pub fn new() -> Self {
        Self {
            streams: RwLock::new(HashMap::new()),
        }
    }

    /// Add a new stream from the given source
    pub fn add_stream(&self, source: StreamSource) -> Result<StreamId> {
        // Generate a unique ID for this stream
        let stream_id = Uuid::new_v4().to_string();

        // Initialize GStreamer if not already done
        if gst::init().is_err() {
            gst::init()?;
        }

        // Create a pipeline string based on the stream type
        let pipeline_str = match source.stream_type {
            StreamType::RTSP => {
                // Create a minimal pipeline for RTSP sources
                format!("rtspsrc location={} latency=200 ! tee name=t", source.uri)
            }
            StreamType::TestSource => {
                // Create a minimal pipeline for test patterns
                let pattern = source.uri.parse::<u32>().unwrap_or(0);
                format!("videotestsrc pattern={} ! tee name=t", pattern)
            }
        };
        println!("Creating pipeline: {}", pipeline_str);

        // Create the GStreamer pipeline
        let pipeline = gst::parse::launch(&pipeline_str)?
            .downcast::<gst::Pipeline>()
            .unwrap();

        // Get the tee element that will be used to create branches
        let tee = pipeline
            .by_name("t")
            .ok_or_else(|| anyhow!("Failed to get tee element"))?;

        // Create the Stream object and add it to our collection
        let stream = Stream {
            source,
            pipeline,
            tee,
            branches: HashMap::new(),
        };

        let mut streams = self.streams.write().unwrap();
        streams.insert(stream_id.clone(), stream);

        // Put the pipeline in READY state so we can add branches
        streams
            .get_mut(&stream_id)
            .unwrap()
            .pipeline
            .set_state(gst::State::Ready)?;

        Ok(stream_id)
    }

    /// Remove a stream and all its branches
    pub fn remove_stream(&self, stream_id: &str) -> Result<()> {
        let mut streams = self.streams.write().unwrap();

        if let Some(stream) = streams.get_mut(stream_id) {
            // Stop the pipeline
            stream.pipeline.set_state(gst::State::Null)?;
            streams.remove(stream_id);
            Ok(())
        } else {
            Err(anyhow!("Stream not found: {}", stream_id))
        }
    }

    /// Add a branch to an existing stream
    pub fn add_branch(&self, stream_id: &str, config: BranchConfig) -> Result<BranchId> {
        let branch_id = Uuid::new_v4().to_string();
        let mut streams = self.streams.write().unwrap();

        let stream = streams
            .get_mut(stream_id)
            .ok_or_else(|| anyhow!("Stream not found: {}", stream_id))?;

        // Create the appropriate type of branch
        let (bin, queue) = match config.branch_type {
            BranchType::Recording => self.create_recording_branch(&config)?,
            BranchType::LiveView => self.create_live_view_branch(&config)?,
        };

        // Get a sink pad for the branch
        let queue_sink_pad = queue.static_pad("sink").unwrap();
        let bin_sink_pad = bin.static_pad("sink").unwrap();

        // Get a source pad from the tee
        let tee_src_pad = stream.tee.request_pad_simple("src_%u").unwrap();

        // Add bin to pipeline
        stream.pipeline.add(&bin)?;

        // Link the tee to the branch bin
        tee_src_pad.link(&bin_sink_pad)?;

        // Add bin to pipeline
        bin.sync_state_with_parent()?;

        // Store branch
        let branch = Branch { bin, queue };

        stream.branches.insert(branch_id.clone(), branch);

        // If this is the first branch, start the pipeline
        if stream.branches.len() == 1 {
            stream.pipeline.set_state(gst::State::Playing)?;
        }

        Ok(branch_id)
    }

    /// Remove a branch from a stream
    pub fn remove_branch(&self, stream_id: &str, branch_id: &str) -> Result<()> {
        let mut streams = self.streams.write().unwrap();

        if let Some(stream) = streams.get_mut(stream_id) {
            if let Some(branch) = stream.branches.remove(branch_id) {
                // Get bin's sink pad
                let bin_sink_pad = branch.bin.static_pad("sink").unwrap();

                // Get the tee src pad connected to this branch
                let tee_src_pad = bin_sink_pad.peer().unwrap();

                // Unlink
                tee_src_pad.unlink(&bin_sink_pad)?;

                // Release tee pad
                stream.tee.release_request_pad(&tee_src_pad);

                // Remove bin from pipeline
                branch.bin.set_state(gst::State::Null)?;
                stream.pipeline.remove(&branch.bin)?;

                // If no branches left, put pipeline back to READY
                if stream.branches.is_empty() {
                    stream.pipeline.set_state(gst::State::Ready)?;
                }

                Ok(())
            } else {
                Err(anyhow!("Branch not found: {}", branch_id))
            }
        } else {
            Err(anyhow!("Stream not found: {}", stream_id))
        }
    }

    /// Get information about a stream
    pub fn get_stream_info(&self, stream_id: &str) -> Result<StreamSource> {
        let streams = self.streams.read().unwrap();

        if let Some(stream) = streams.get(stream_id) {
            Ok(stream.source.clone())
        } else {
            Err(anyhow!("Stream not found: {}", stream_id))
        }
    }

    /// List all streams
    pub fn list_streams(&self) -> Vec<(StreamId, StreamSource)> {
        let streams = self.streams.read().unwrap();

        streams
            .iter()
            .map(|(id, stream)| (id.clone(), stream.source.clone()))
            .collect()
    }

    // Helper to create a recording branch
    fn create_recording_branch(&self, config: &BranchConfig) -> Result<(gst::Bin, gst::Element)> {
        let bin = gst::Bin::new();

        // Create queue element to buffer data from the tee
        let queue = gst::ElementFactory::make("queue").build()?;
        queue.set_property("max-size-buffers", 100u32);

        // Create elements for the recording pipeline
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
        let encoder = gst::ElementFactory::make("x264enc").build()?;
        let muxer = gst::ElementFactory::make("mp4mux").build()?;

        // Get output path from config or use default
        let output_path = config
            .output_path
            .as_ref()
            .map(|p| p.as_str())
            .unwrap_or("/tmp/recording.mp4");

        let sink = gst::ElementFactory::make("filesink").build()?;
        sink.set_property("location", output_path);

        // Add all elements to the bin
        bin.add_many(&[&queue, &videoconvert, &encoder, &muxer, &sink])?;

        // Link elements together
        gst::Element::link_many(&[&queue, &videoconvert, &encoder, &muxer, &sink])?;

        // Create ghost pad to expose queue's sink pad as bin's sink pad
        let queue_sink_pad = queue.static_pad("sink").unwrap();
        let ghost_pad = gst::GhostPad::with_target(&queue_sink_pad)?;
        ghost_pad.set_active(true)?;
        bin.add_pad(&ghost_pad)?;

        Ok((bin, queue))
    }

    // Helper to create a live viewing branch
    fn create_live_view_branch(&self, _config: &BranchConfig) -> Result<(gst::Bin, gst::Element)> {
        let bin = gst::Bin::new();

        // Create queue element to buffer data from the tee
        let queue = gst::ElementFactory::make("queue").build()?;
        queue.set_property("max-size-buffers", 3u32);

        // For RTSP streams, we need to handle the specific format
        // This will depend on your specific stream format
        let rtpdepay = gst::ElementFactory::make("rtph264depay").build()?; // For H.264 over RTP
        let parser = gst::ElementFactory::make("h264parse").build()?;
        let decoder = gst::ElementFactory::make("avdec_h264").build()?; // Hardware decoder if available
        let convert = gst::ElementFactory::make("videoconvert").build()?;

        // Platform-specific sink
        #[cfg(target_os = "macos")]
        let sink = gst::ElementFactory::make("osxvideosink").build()?;
        #[cfg(target_os = "linux")]
        let sink = gst::ElementFactory::make("autovideosink").build()?;
        #[cfg(target_os = "windows")]
        let sink = gst::ElementFactory::make("directdrawsink").build()?;
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        let sink = gst::ElementFactory::make("autovideosink").build()?;

        sink.set_property("sync", false);

        // Add all elements to the bin
        bin.add_many(&[&queue, &rtpdepay, &parser, &decoder, &convert, &sink])?;

        // Link elements together
        gst::Element::link_many(&[&queue, &rtpdepay, &parser, &decoder, &convert, &sink])?;

        // Create ghost pad
        let queue_sink_pad = queue.static_pad("sink").unwrap();
        let ghost_pad = gst::GhostPad::with_target(&queue_sink_pad)?;
        ghost_pad.set_active(true)?;
        bin.add_pad(&ghost_pad)?;

        Ok((bin, queue))
    }
}

impl Default for StreamManager {
    fn default() -> Self {
        Self::new()
    }
}

