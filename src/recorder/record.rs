
pub async fn record() -> { 
    info!("Processing WebRTC offer for session: {}", request.session_id);

    // Get the existing stream using stream_id from StreamManager
    let stream_id = request.stream_id.to_string();
    let (pipeline, tee) = state.stream_manager.get_stream_access(&stream_id)
        .map_err(|e| {
            error!("Failed to get stream access: {}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Check pipeline state
    let pipeline_state = pipeline.state(Some(gst::ClockTime::from_seconds(2)));
    info!("Pipeline current state: {:?}", pipeline_state);

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
        let element_state = element.state(Some(gst::ClockTime::from_mseconds(100)));
        info!("Element {} is in state: {:?}", element.name(), element_state);
    }

    let _pipeline_state = pipeline.set_state(gst::State::Playing);
}
