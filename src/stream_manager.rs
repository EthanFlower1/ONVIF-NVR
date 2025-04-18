use anyhow::{anyhow, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

pub type StreamId = String;

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

// Internal stream representation
struct Stream {
    source: StreamSource,
    pipeline: gst::Pipeline,
    tee: gst::Element,
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

    pub fn get_stream_access(&self, stream_id: &str) -> Result<(gst::Pipeline, gst::Element)> {
        let streams = self.streams.read().unwrap();
        let stream = streams
            .get(stream_id)
            .ok_or_else(|| anyhow!("Stream not found: {}", stream_id))?;

        // Return clones of the pipeline and tee
        // This provides access without giving ownership or mutable access
        Ok((stream.pipeline.clone(), stream.tee.clone()))
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
}

impl Default for StreamManager {
    fn default() -> Self {
        Self::new()
    }
}

