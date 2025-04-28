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
    audio_tee: gst::Element,
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

    /// Add a new stream from the given source, with separate audio/video tees
    pub fn add_stream(&self, source: StreamSource, stream_id: String) -> Result<StreamId> {
        // 1) Init GStreamer
        gst::init()?;
        // 2) Create a new empty pipeline
        let pipeline = gst::Pipeline::with_name(&format!("pipeline_{}", stream_id));
        // 3) Create and add the RTSP source
        let rtspsrc = gst::ElementFactory::make("rtspsrc")
            .property("location", &source.uri)
            .property("latency", &200u32)
            .build()?;
        pipeline.add(&rtspsrc)?;
        // 4) Create two tees and add them
        let video_tee = gst::ElementFactory::make("tee")
            .name(&format!("video_tee_{}", stream_id))
            .build()?;
        let audio_tee = gst::ElementFactory::make("tee")
            .name(&format!("audio_tee_{}", stream_id))
            .build()?;
        pipeline.add_many(&[&video_tee, &audio_tee])?;
        // 5) Route incoming pads into the right tee
        //
        // We clone what we need into the closure:
        let pipeline_clone = pipeline.clone();
        let sid_clone = stream_id.clone();
        rtspsrc.connect_pad_added(move |_, src_pad| {
            // inspect caps to decide audio vs video
            if let Some(caps) = src_pad.current_caps() {
                if let Some(s) = caps.structure(0) {
                    if let Ok(media_type) = s.get::<String>("media") {
                        let tee_name = match media_type.as_str() {
                            "video" => format!("video_tee_{}", sid_clone),
                            "audio" => format!("audio_tee_{}", sid_clone),
                            _ => {
                                warn!("Unsupported media type: {}", media_type);
                                return;
                            }
                        };

                        // Look up the tee by name
                        let tee = match pipeline_clone.by_name(&tee_name) {
                            Some(t) => t,
                            None => {
                                eprintln!("Failed to find tee: {}", tee_name);
                                // Debug what tees are available
                                let elements = pipeline_clone.children();
                                eprintln!(
                                    "Available elements: {:?}",
                                    elements.iter().map(|e| e.name()).collect::<Vec<_>>()
                                );
                                return;
                            }
                        };

                        // Create a queue for this branch
                        let queue = match gst::ElementFactory::make("queue").build() {
                            Ok(q) => q,
                            Err(e) => {
                                eprintln!("Failed to create queue: {:?}", e);
                                return;
                            }
                        };

                        // Add the queue to the pipeline
                        if let Err(e) = pipeline_clone.add(&queue) {
                            eprintln!("Failed to add queue to pipeline: {:?}", e);
                            return;
                        }

                        if let Err(e) = queue.sync_state_with_parent() {
                            eprintln!("Failed to sync queue state: {:?}", e);
                            return;
                        }

                        // Link: src_pad → queue → tee
                        let sink_pad = match queue.static_pad("sink") {
                            Some(p) => p,
                            None => {
                                eprintln!("Failed to get sink pad from queue");
                                return;
                            }
                        };

                        if let Err(e) = src_pad.link(&sink_pad) {
                            eprintln!("Failed to link src_pad to queue: {:?}", e);
                            return;
                        }

                        if let Err(e) = queue.link(&tee) {
                            eprintln!("Failed to link queue to tee: {:?}", e);
                            return;
                        }

                        println!("Successfully linked {} pad to {}", media_type, tee_name);
                    }
                }
            }
        });
        // 6) Prevent tees from blocking when no real branches exist
        for (tee, tag) in [(&video_tee, "video"), (&audio_tee, "audio")] {
            let dummy_q = gst::ElementFactory::make("queue")
                .name(&format!("{}_dummy_q_{}", stream_id, tag))
                .build()?;
            let dummy_sink = gst::ElementFactory::make("fakesink")
                .name(&format!("{}_dummy_sink_{}", stream_id, tag))
                .property("sync", &false)
                .property("async", &false)
                .build()?;
            pipeline.add_many(&[&dummy_q, &dummy_sink])?;
            tee.link(&dummy_q)?;
            dummy_q.link(&dummy_sink)?;
        }
        // 7) Wrap into your Stream struct (you'll need to add audio_tee to it)
        let stream = Stream {
            source,
            pipeline: pipeline.clone(),
            tee: video_tee.clone(),
            audio_tee: audio_tee.clone(),
        };
        // 8) Store and set READY
        {
            let mut streams = self.streams.write().unwrap();
            streams.insert(stream_id.clone(), stream);
            streams
                .get_mut(&stream_id)
                .unwrap()
                .pipeline
                .set_state(gst::State::Ready)?;
        }
        Ok(stream_id)
    }

    pub fn get_stream_access(
        &self,
        stream_id: &str,
    ) -> Result<(gst::Pipeline, gst::Element, gst::Element)> {
        let streams = self.streams.read().unwrap();
        let stream = streams
            .get(stream_id)
            .ok_or_else(|| anyhow!("Stream not found: {}", stream_id))?;

        // Return clones of the pipeline and tee
        // This provides access without giving ownership or mutable access
        Ok((
            stream.pipeline.clone(),
            stream.tee.clone(),
            stream.audio_tee.clone(),
        ))
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
