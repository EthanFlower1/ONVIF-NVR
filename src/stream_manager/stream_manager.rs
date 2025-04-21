use crate::db::models::stream_models::StreamType;
use crate::db::repositories::cameras::CamerasRepository;
use anyhow::{anyhow, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use log::{info, warn};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type StreamId = String;

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
    db_pool: Arc<PgPool>,
}

impl StreamManager {
    /// Create a new StreamManager
    pub fn new(db_pool: Arc<PgPool>) -> Self {
        Self {
            streams: RwLock::new(HashMap::new()),
            db_pool,
        }
    }

    pub async fn connect(&self) -> Result<i32> {
        let cameras_with_streams = CamerasRepository::new(self.db_pool.clone())
            .get_all_with_streams()
            .await?;

        for camera_with_streams in cameras_with_streams.iter() {
            let username = camera_with_streams
                .camera
                .username
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Camera username is missing"))?;
            let password = camera_with_streams
                .camera
                .password
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Camera password is missing"))?;

            for stream in camera_with_streams.streams.iter() {
                // Parse the original URL to insert username and password
                let stream_uri = stream.url.to_string();

                // Check if URL already contains credentials
                let auth_uri = if stream_uri.contains('@') {
                    // URL already has credentials, use as is
                    stream_uri
                } else {
                    // URL doesn't have credentials, add them
                    if stream_uri.starts_with("rtsp://") {
                        format!("rtsp://{}:{}@{}", username, password, &stream_uri[7..])
                    } else {
                        // Handle non-RTSP URLs or malformed URLs
                        warn!("Invalid RTSP URL format: {}", stream_uri);
                        stream_uri
                    }
                };

                info!("Connecting to camera URL: {}", auth_uri.clone());

                let source = StreamSource {
                    stream_type: stream.stream_type,
                    uri: auth_uri,
                    name: stream.name.clone(),
                    description: Some("RTSP stream".to_string()),
                };

                let stream_id = self.add_stream(source, stream.id.to_string())?;
                println!("Created stream with ID: {}", stream_id);
            }
        }

        // Convert to i32 without needing to borrow
        let count: i32 = cameras_with_streams.len().try_into().unwrap();
        Ok(count)
    }

    /// Add a new stream from the given source
    pub fn add_stream(&self, source: StreamSource, stream_id: String) -> Result<StreamId> {
        // Initialize GStreamer if not already done
        if gst::init().is_err() {
            gst::init()?;
        }

        // Create a pipeline string based on the stream type
        let pipeline_str = match source.stream_type {
            StreamType::Rtsp => {
                // Create a minimal pipeline for RTSP sources
                format!("rtspsrc location={} latency=200 ! tee name=t", source.uri)
            }
            _ => {
                // Default pipeline - RTSP is the only type for now, but this handles any type
                format!("rtspsrc location={} latency=200 ! tee name=t", source.uri)
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
