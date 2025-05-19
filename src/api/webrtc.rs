use anyhow::Result;
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use sqlx::PgPool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use webrtc::api::{
    APIBuilder, 
    media_engine::MediaEngine,
};
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy;
use webrtc::peer_connection::policy::bundle_policy::RTCBundlePolicy;
use webrtc::peer_connection::policy::rtcp_mux_policy::RTCRtcpMuxPolicy;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::media::Sample;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

// Import your custom types (make sure these paths match your project structure)
use crate::stream_manager::stream_manager::StreamManager;

pub struct WebRTCState {
    pub pool: Arc<PgPool>,
    pub stream_manager: Arc<StreamManager>,
    // Track active peer connections
    peer_connections: Arc<tokio::sync::Mutex<HashMap<String, Arc<RTCPeerConnection>>>>,
}

impl WebRTCState {
    pub fn new(pool: Arc<PgPool>, stream_manager: Arc<StreamManager>) -> Self {
        Self {
            pool,
            stream_manager,
            peer_connections: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRTCSessionRequest {
    stream_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRTCSessionResponse {
    session_id: String,
    ice_servers: Vec<WebRTCIceServer>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRTCIceServer {
    urls: Vec<String>,
    username: Option<String>,
    credential: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRTCOfferRequest {
    session_id: String,
    stream_id: Uuid,
    sdp: String,
    type_field: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRTCAnswerResponse {
    sdp: String,
    type_field: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRTCIceCandidateRequest {
    session_id: String,
    candidate: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u16>,
}

// Create a new WebRTC session
pub async fn create_webrtc_session(
    State(_state): State<Arc<WebRTCState>>,
    Json(request): Json<WebRTCSessionRequest>,
) -> Json<WebRTCSessionResponse> {
    info!("Creating WebRTC session for camera: {}", request.stream_id);
    
    // Generate a unique session ID
    let session_id = Uuid::new_v4().to_string();
    
    // Define ICE servers (STUN and TURN configurations)
    let ice_servers = vec![
        WebRTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            username: None,
            credential: None,
        },
    ];
    
    // Return the session information
    Json(WebRTCSessionResponse {
        session_id,
        ice_servers,
    })
}

// Process an SDP offer from the client
pub async fn process_webrtc_offer(
    State(state): State<Arc<WebRTCState>>,
    Json(request): Json<WebRTCOfferRequest>,
) -> Result<Json<WebRTCAnswerResponse>, axum::http::StatusCode> {
    info!("Processing WebRTC offer for session: {}", request.session_id);

    // Get the existing stream using stream_id from StreamManager
    let stream_id = request.stream_id.to_string();
    let (pipeline, tee, _, _) = state.stream_manager.get_stream_access(&stream_id)
        .map_err(|e| {
            error!("Failed to get stream access: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Check pipeline state
    // let pipeline_state = pipeline.state(Some(gst::ClockTime::from_seconds(2)));
    // info!("Pipeline current state: {:?}", pipeline_state);

    // Generate unique names for elements based on session ID
    let element_suffix = &request.session_id.replace("-", "");
    
    // Create GStreamer elements for WebRTC processing with unique names
    let queue = gst::ElementFactory::make("queue")
        .name(&format!("webrtc_queue_{}", element_suffix))
        .property("max-size-buffers", 0u32)  // Try unlimited buffers
        .property("max-size-time", 0u64)     // Unlimited time
        .property("max-size-bytes", 0u32)    // Unlimited bytes
        .build()
        .map_err(|e| {
            error!("Failed to create queue: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let depay = gst::ElementFactory::make("rtph264depay")
        .name(&format!("webrtc_depay_{}", element_suffix))
        .build()
        .map_err(|e| {
            error!("Failed to create depay: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let parse = gst::ElementFactory::make("h264parse")
        .name(&format!("webrtc_parse_{}", element_suffix))
        .build()
        .map_err(|e| {
            error!("Failed to create parse: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Create appsink to capture H.264 packets
    let appsink = gst_app::AppSink::builder()
        .name(&format!("webrtc_appsink_{}", element_suffix))
        .max_buffers(1)
        .drop(true)
        .buffer_list(false)  // Set to false
        .wait_on_eos(false)
        .sync(false)
        .enable_last_sample(false)
        .build();
    
    // Set caps on appsink
    let caps = gst::Caps::builder("video/x-h264")
        .field("stream-format", "byte-stream")
        .field("alignment", "au")
        .build();
    appsink.set_caps(Some(&caps));

    // Add elements to pipeline
    pipeline.add_many(&[&queue, &depay, &parse, appsink.upcast_ref()])
        .map_err(|e| {
            error!("Failed to add elements to pipeline: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Link GStreamer elements
    gst::Element::link_many(&[&queue, &depay, &parse, appsink.upcast_ref()])
        .map_err(|e| {
            error!("Failed to link elements: {}", e);
            // If linking fails, remove the elements we added
            let _ = pipeline.remove_many(&[&queue, &depay, &parse, appsink.upcast_ref()]);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Connect to tee
    let tee_src_pad = tee.request_pad_simple("src_%u")
        .ok_or_else(|| {
            error!("Failed to get tee src pad");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let queue_sink_pad = queue.static_pad("sink")
        .ok_or_else(|| {
            error!("Failed to get queue sink pad");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Link the tee to the queue
    tee_src_pad.link(&queue_sink_pad)
        .map_err(|e| {
            error!("Failed to link tee to queue: {:?}", e);
            // Clean up on error
            let _ = pipeline.remove_many(&[&queue, &depay, &parse, appsink.upcast_ref()]);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Sync state with parent
    for element in [&queue, &depay, &parse, appsink.upcast_ref()] {
        element.sync_state_with_parent()
            .map_err(|e| {
                error!("Failed to sync element state: {}", e);
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            })?;
        
        // Debug element state
        // let element_state = element.state(Some(gst::ClockTime::from_mseconds(100)));
        // info!("Element {} is in state: {:?}", element.name(), element_state);
    }

    let _pipeline_state = pipeline.set_state(gst::State::Playing);

    // Create media engine and API
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs()
        .map_err(|e| {
            error!("Failed to register codecs: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .build();
    
    // Create ICE server configuration
    let config = RTCConfiguration {
        ice_servers: vec![
            RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            },
        ],
        ice_transport_policy: RTCIceTransportPolicy::All,
        bundle_policy: RTCBundlePolicy::MaxBundle,
        rtcp_mux_policy: RTCRtcpMuxPolicy::Require,
        ..Default::default()
    };
    
    // Create a new RTCPeerConnection
    let peer_connection = Arc::new(api.new_peer_connection(config).await
        .map_err(|e| {
            error!("Failed to create peer connection: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?);
    
    // Create a video track for the camera stream
    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: "video/h264".to_owned(),
            clock_rate: 90000,
            channels: 1,
            sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".to_owned(),
            ..Default::default()
        },
        format!("video-{}", request.session_id),
        "camera-stream-video".to_owned(),
    ));
    
    // Add the video track to the peer connection
    let _rtp_sender = peer_connection
        .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .map_err(|e| {
            error!("Failed to add video track: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Parse and set the remote SDP
    let offer_sdp_type = match request.type_field.as_str() {
        "offer" => RTCSdpType::Offer,
        _ => return Err(axum::http::StatusCode::BAD_REQUEST),
    };
    
    let offer = match offer_sdp_type {
        RTCSdpType::Offer => RTCSessionDescription::offer(request.sdp)
            .map_err(|e| {
                error!("Failed to create offer: {}", e);
                axum::http::StatusCode::BAD_REQUEST
            })?,
        _ => return Err(axum::http::StatusCode::BAD_REQUEST),
    };
    
    peer_connection.set_remote_description(offer).await
        .map_err(|e| {
            error!("Failed to set remote description: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Create and set answer
    let answer = peer_connection.create_answer(None).await
        .map_err(|e| {
            error!("Failed to create answer: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    peer_connection.set_local_description(answer.clone()).await
        .map_err(|e| {
            error!("Failed to set local description: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Store the peer connection
    {
        let mut peer_connections = state.peer_connections.lock().await;
        peer_connections.insert(request.session_id.clone(), Arc::clone(&peer_connection));
    }
    
    // Create a channel for sending samples from the appsink to the WebRTC writing task
    let (sample_sender, mut sample_receiver) = tokio::sync::mpsc::channel::<Sample>(100);

    // Spawn a task to receive samples and write them to the track
    let track_clone_for_receiver = Arc::clone(&video_track);
    let session_id_for_receiver = request.session_id.clone();
    tokio::spawn(async move {
        while let Some(sample) = sample_receiver.recv().await {
            if let Err(err) = track_clone_for_receiver.write_sample(&sample).await {
                warn!("Failed to write sample to WebRTC track for session {}: {}", session_id_for_receiver, err);
            }
        }
        info!("Sample receiver task ended for session {}", session_id_for_receiver);
    });
    
    // Set up appsink to send H.264 packets to WebRTC via the channel
    let session_id_for_debug = request.session_id.clone();
    let session_id_for_debug_2 = request.session_id.clone();
    let mut sample_count = 0u64;
    
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(move |appsink| {
                sample_count += 1;
                
                // Debug log every 30 samples (roughly once per second at 30fps)
                // if sample_count % 120 == 0 {
                //     info!("AppSink received {} samples for session {}", sample_count, session_id_for_debug.clone());
                // }
                
                let sample = match appsink.pull_sample() {
                    Ok(sample) => sample,
                    Err(e) => {
                        error!("Failed to pull sample: {:?}", e);
                        return Err(gst::FlowError::Error);
                    }
                };
                
                let buffer = match sample.buffer() {
                    Some(buffer) => buffer,
                    None => {
                        error!("No buffer in sample");
                        return Err(gst::FlowError::Error);
                    }
                };
                
                let map = match buffer.map_readable() {
                    Ok(map) => map,
                    Err(e) => {
                        error!("Failed to map buffer: {:?}", e);
                        return Err(gst::FlowError::Error);
                    }
                };
                
                // Debug log every 30 samples
                // if sample_count % 30 == 0 {
                //     info!("Buffer size: {} bytes for session {}", map.size(), session_id_for_debug.clone());
                // }
                
                // Create WebRTC sample
                let webrtc_sample = Sample {
                    data: map.as_slice().to_vec().into(),
                    duration: Duration::from_millis(33), // ~30fps
                    timestamp: SystemTime::now(),
                    packet_timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u32,
                    prev_dropped_packets: 0,
                    prev_padding_packets: 0,
                };
                
                // Send the sample through the channel
                let tx = sample_sender.clone();
                match tx.try_send(webrtc_sample) {
                    Ok(_) => Ok(gst::FlowSuccess::Ok),
                    Err(e) => {
                        warn!("Failed to send sample to channel: {} for session {}", e, session_id_for_debug.clone());
                        // We continue even if we failed to send to avoid blocking the pipeline
                        Ok(gst::FlowSuccess::Ok)
                    }
                }
            })
            .eos(move |_appsink| {
                info!("AppSink received EOS for session {}", session_id_for_debug_2.clone());
            })
            .build()
    );
    
    // Set up connection state monitoring
    let session_id_mon = request.session_id.clone();
    let state_mon = Arc::clone(&state);
    let pc_mon = Arc::clone(&peer_connection);
    
    peer_connection.on_peer_connection_state_change(Box::new(move |connection_state| {
        let pc_clone = Arc::clone(&pc_mon);
        let session_id = session_id_mon.clone();
        let state_clone = Arc::clone(&state_mon);
        
        Box::pin(async move {
            match connection_state {
                webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected => {
                    info!("WebRTC connection established for session: {}", session_id);
                },
                webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Disconnected => {
                    info!("WebRTC connection disconnected for session: {}", session_id);
                    let peer_connections_handle = Arc::clone(&state_clone.peer_connections);
                    let session_id_for_disconnect = session_id.clone();
                    
                    tokio::spawn(async move {
                        let should_terminate = {
                            let peer_connections = peer_connections_handle.lock().await;
                            peer_connections.contains_key(&session_id_for_disconnect)
                        };
                        
                        if should_terminate {
                            info!("Connection did not recover after timeout for session: {}", session_id_for_disconnect);
                            let mut peer_connections = peer_connections_handle.lock().await;
                            if peer_connections.remove(&session_id_for_disconnect).is_some() {
                                info!("Removed peer connection for session: {}", session_id_for_disconnect);
                            }
                        }
                    });
                },
                webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Failed
                | webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Closed => {
                    info!("WebRTC connection ended (state: {:?}) for session: {}", connection_state, session_id);
                    
                    let peer_connections_handle = Arc::clone(&state_clone.peer_connections);
                    let session_id_for_close = session_id.clone();
                    
                    tokio::spawn(async move {
                        let mut peer_connections = peer_connections_handle.lock().await;
                        if peer_connections.remove(&session_id_for_close).is_some() {
                            info!("Removed peer connection for session: {}", session_id_for_close);
                        }

                        // close_webrtc_session(state, ).await
                    });
                    
                    if connection_state != webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Closed {
                        if let Err(e) = pc_clone.close().await {
                            warn!("Error closing peer connection: {}", e);
                        }
                    }
                },
                _ => debug!("WebRTC connection state changed to {:?} for session: {}", connection_state, session_id),
            }
        })
    }));

    
    // Return the SDP answer
    Ok(Json(WebRTCAnswerResponse {
        sdp: answer.sdp,
        type_field: "answer".to_string(),
    }))
}
// Add an ICE candidate from the client
pub async fn add_ice_candidate(
    State(state): State<Arc<WebRTCState>>,
    Json(request): Json<WebRTCIceCandidateRequest>,
) -> Result<Json<JsonValue>, axum::http::StatusCode> {
    info!("Adding ICE candidate for session: {}", request.session_id);
    
    let peer_connection = {
        let peer_connections = state.peer_connections.lock().await;
        
        match peer_connections.get(&request.session_id) {
            Some(pc) => Arc::clone(pc),
            None => {
                info!("ICE candidate received before peer connection setup for session {}", 
                     request.session_id);
                return Ok(Json(json!({ "success": true, "queued": true })));
            }
        }
    };
    
    let candidate_init = RTCIceCandidateInit {
        candidate: request.candidate,
        sdp_mid: request.sdp_mid,
        sdp_mline_index: request.sdp_mline_index,
        username_fragment: None,
    };
    
    peer_connection.add_ice_candidate(candidate_init).await
        .map_err(|e| {
            error!("Failed to add ICE candidate: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(Json(json!({ "success": true })))
}

// Close a WebRTC session
pub async fn close_webrtc_session(
    State(state): State<Arc<WebRTCState>>,
    Path(session_id): Path<String>,
) -> Json<JsonValue> {
    info!("Closing WebRTC session: {}", session_id);
    
    let peer_connection = {
        let mut peer_connections = state.peer_connections.lock().await;
        peer_connections.remove(&session_id)
    };
    
    if let Some(pc) = peer_connection {
        for sender in pc.get_senders().await {
            if let Some(track) = sender.track().await {
                info!("Stopping track: {}", track.id());
                if let Err(err) = sender.stop().await {
                    warn!("Error stopping RTP sender: {}", err);
                }
            }
        }
        
        if let Err(err) = pc.close().await {
            error!("Error closing peer connection: {}", err);
        }
        
        tokio::time::sleep(Duration::from_millis(100)).await;
    } else {
        warn!("No peer connection found for session: {}", session_id);
    }
     clean_up_gstreamer_elements(&session_id, &state).await;

    info!("WebRTC session closed: {}", session_id);
    Json(json!({ "success": true }))
}

async fn clean_up_gstreamer_elements(session_id: &str, state: &Arc<WebRTCState>) {
    // Generate unique element suffix
    let element_suffix = session_id.replace("-", "");

    // Get all streams to check for elements to clean up
    let stream_list = state.stream_manager.list_streams();

    for (stream_id, _) in stream_list {
        if let Ok((pipeline, tee, _, _)) = state.stream_manager.get_stream_access(&stream_id) {
            info!(
                "Cleaning up GStreamer elements for session {} in stream {}",
                session_id, stream_id
            );

            // Element names for this session
            let queue_name = format!("webrtc_queue_{}", element_suffix);
            let depay_name = format!("webrtc_depay_{}", element_suffix);
            let parse_name = format!("webrtc_parse_{}", element_suffix);
            let appsink_name = format!("webrtc_appsink_{}", element_suffix);

            // Find the elements
            let queue_opt = pipeline.by_name(&queue_name);
            let depay_opt = pipeline.by_name(&depay_name);
            let parse_opt = pipeline.by_name(&parse_name);
            let appsink_opt = pipeline.by_name(&appsink_name);

            // Check if we found any elements
            if queue_opt.is_none()
                && depay_opt.is_none()
                && parse_opt.is_none()
                && appsink_opt.is_none()
            {
                debug!(
                    "No elements found for session {} in stream {}",
                    session_id, stream_id
                );
                continue;
            }

            // First handle unlinking from tee if queue exists
            if let Some(queue) = &queue_opt {
                if let Some(queue_sink_pad) = queue.static_pad("sink") {
                    if let Some(tee_src_pad) = queue_sink_pad.peer() {
                        // Block the pad before unlinking
                        if let Some(probe_id) = tee_src_pad
                            .add_probe(gst::PadProbeType::BLOCK_DOWNSTREAM, |_pad, _info| {
                                gst::PadProbeReturn::Ok
                            })
                        {
                            // Unlink the pads
                            let _ = tee_src_pad.unlink(&queue_sink_pad);

                            // Release the tee request pad
                            tee.release_request_pad(&tee_src_pad);

                            // Remove the probe
                            tee_src_pad.remove_probe(probe_id);
                        }
                    }
                }
            }

            // Gather all found elements
            let mut elements = Vec::new();
            if let Some(e) = queue_opt {
                elements.push(e);
            }
            if let Some(e) = depay_opt {
                elements.push(e);
            }
            if let Some(e) = parse_opt {
                elements.push(e);
            }
            if let Some(e) = appsink_opt {
                elements.push(e);
            }

            // Send EOS to elements
            for element in &elements {
                let _ = element.send_event(gst::event::Eos::new());
            }

            // Set elements to NULL state
            for element in &elements {
                let _ = element.set_state(gst::State::Null);
            }

            // Remove elements from pipeline
            if !elements.is_empty() {
                match pipeline.remove_many(&elements) {
                    Ok(_) => info!(
                        "Successfully removed {} elements for session {}",
                        elements.len(),
                        session_id
                    ),
                    Err(e) => warn!("Failed to remove elements from pipeline: {:?}", e),
                }
            }

            // Check if this was the last connection for this stream
            let has_other_connections = {
                let connections = state.peer_connections.lock().await;
                !connections.is_empty()
            };

            // If no other connections, set the pipeline to NULL state
            if !has_other_connections {
                info!(
                    "No more active connections for stream {}, stopping pipeline",
                    stream_id
                );
                // if let Err(e) = pipeline.set_state(gst::State::Paused) {
                //     warn!("Failed to set pipeline to NULL state: {:?}", e);
                // } else {
                //     info!("Successfully set pipeline to NULL state");
                //
                //     // Remove the stream entirely
                //     // if let Err(e) = state.stream_manager.remove_stream(&stream_id) {
                //     //     warn!("Failed to remove stream: {:?}", e);
                //     // } else {
                //     //     info!("Successfully removed stream: {}", stream_id);
                //     // }
                // }
            }
        }
    }
}
