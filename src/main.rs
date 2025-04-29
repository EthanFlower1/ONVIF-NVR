use crate::messaging::broker::MessageBrokerTrait;
use crate::security::auth::AuthService;
use anyhow::Result;
use db::migrations;
use db::repositories::recordings::RecordingsRepository;
use gst::prelude::*;
use gstreamer as gst;
use log::{error, info, warn};
use recorder::{RecordingManager, RecordingScheduler, StorageCleanupService};
use sqlx::postgres::PgPoolOptions;
use std::{sync::Arc, thread};
use stream_manager::StreamManager;

#[path = "./tutorial-common.rs"]
mod tutorials_common;

mod api;
mod config;
mod db;
mod device_manager;
mod error;
mod messaging;
mod recorder;
mod security;
mod stream_manager;
mod utils;

pub use error::Error;

async fn run_app() -> Result<()> {
    // Initialize logging
    env_logger::init();
    info!("Starting G-Streamer Stream Management System");

    // Initialize GStreamer
    gst::init()?;
    info!("GStreamer initialized successfully");

    // Store it for access by other threads
    // Run the main loop - this will block until quit() is called
    let config = config::load_config(None)?;

    info!("Configuration loaded");
    // Load configuration
    // let config = config::setup_config()?;
    // info!("Configuration loaded");

    // Create database connection pool
    let db_pool = PgPoolOptions::new()
        .max_connections(200)
        .connect(&config.database.url)
        .await?;

    match migrations::run_migrations(&db_pool).await {
        Ok(_) => {
            log::info!("Migrations completed successfully");
        }
        Err(err) => {
            log::error!("Failed to run migrations: {}", err);
        }
    }

    let db_pool = std::sync::Arc::new(db_pool);

    // Create auth service
    let auth_service = Arc::new(AuthService::new(db_pool.clone(), &config.security));

    // Create and initialize message broker
    let message_broker =
        messaging::broker::create_message_broker(config.message_broker.clone()).await?;
    info!("Message broker initialized");

    // Publish system startup event
    if let Err(e) = message_broker
        .publish(
            messaging::EventType::SystemStartup,
            None,
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }),
        )
        .await
    {
        warn!("Failed to publish system startup event: {}", e);
    }

    // Create and initialize stream manager
    let stream_manager = Arc::new(StreamManager::new(db_pool.clone()));
    let connected_cameras = &stream_manager.connect().await?;
    info!(
        "Stream manager initialized, Connected Cameras: {}",
        connected_cameras
    );

    // Setup recordings directory from config
    let recordings_dir = &config.recording.storage_path;
    std::fs::create_dir_all(recordings_dir)?;

    // Create the recording manager with configuration from settings
    let recording_manager = Arc::new(RecordingManager::new(
        db_pool.clone(),
        stream_manager.clone(),
        recordings_dir,
        config.recording.segment_duration as i64,
        &config.recording.format,
    ));

    // Pass the message broker to recording_manager so it can publish events
    recording_manager
        .set_message_broker(message_broker.clone())
        .await?;

    // Create and start recording scheduler
    let recording_scheduler = Arc::new(RecordingScheduler::new(
        db_pool.clone(),
        stream_manager.clone(),
        recording_manager.clone(),
        60, // Check for schedule changes every 60 seconds
    ));

    // Create storage cleanup service
    let storage_cleanup = Arc::new(StorageCleanupService::new(
        config.recording.cleanup.clone(),
        RecordingsRepository::new(db_pool.clone()),
        recordings_dir,
    ));

    // Pass the message broker to storage_cleanup service
    storage_cleanup
        .set_message_broker(message_broker.clone())
        .await?;

    // Start the recording scheduler
    recording_scheduler.clone().start().await?;
    info!("Recording scheduler started");

    // Start the storage cleanup service
    storage_cleanup.clone().start().await?;
    info!("Storage cleanup service started");

    // Start the REST API
    let http_server = api::rest::RestApi::new(
        &config.api,
        db_pool,
        stream_manager,
        auth_service,
        message_broker.clone(),
    )
    .unwrap();

    thread::spawn(move || {
        // Create a new main loop for this thread
        let main_loop = glib::MainLoop::new(None, false);
        // Run the main loop - this blocks until quit() is called
        main_loop.run();
    });

    let _ = http_server.run().await;
    info!("API servers started");

    // Wait for termination signals
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Shutdown recording scheduler and stop all recordings
    recording_scheduler.shutdown().await?;
    info!("Recording scheduler stopped");

    // Publish a system shutdown event
    if let Err(e) = message_broker
        .publish(
            messaging::EventType::SystemShutdown,
            None,
            serde_json::json!({"reason": "Normal shutdown"}),
        )
        .await
    {
        error!("Failed to publish shutdown event: {}", e);
    }

    // Allow time for the message to be sent before shutting down
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Finalize any pending storage operations
    std::thread::sleep(std::time::Duration::from_secs(2));

    Ok(())
}

// Helper function to try adding a real camera
// Returns Some(camera_id) if successful, None if no camera is available
// fn add_real_camera(camera_manager: &mut CameraManager) -> Result<Option<String>> {
//     #[cfg(target_os = "macos")]
//     let device_path = "0".to_string(); // Use device index 0 for macOS
//
//     let camera_id = camera_manager.add_camera(
//         "Real Camera".to_string(),
//         device_path,
//         Some("Physical camera device".to_string()),
//     )?;
//
//     match camera_manager.start_camera_stream(&camera_id) {
//         Ok(_) => {
//             // Camera started successfully, stop it for now
//             camera_manager.stop_camera_stream(&camera_id)?;
//             Ok(Some(camera_id))
//         }
//         Err(_) => {
//             // Camera failed to start, remove it
//             let _ = camera_manager.remove_camera(&camera_id);
//             Ok(None)
//         }
//     }
// }

fn main() {
    // For backward compatibility, we're using the tutorial_common's run wrapper
    // In a real app, you might want to use tokio::runtime directly
    tutorials_common::run(|| {
        // Create a tokio runtime in the current thread
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Run our async main function
        if let Err(e) = runtime.block_on(run_app()) {
            eprintln!("Application error: {}", e);
        }
    });
}

// Original tutorial code - kept for reference
fn tutorial_main() {
    // Initialize GStreamer
    gst::init().unwrap();

    // Build the pipeline
    let uri = "rtsp://admin:Gsd4life.@192.168.1.105:554/media/video1";
    let pipeline_str = format!(
    "playbin uri={} video-sink=\"videoconvert ! autovideosink sync=false\" audio-sink=\"audioconvert ! autoaudiosink sync=false\"",
    uri
    );
    let pipeline = gst::parse::launch(&pipeline_str).unwrap();

    // Start playing
    pipeline
        .set_state(gst::State::Playing)
        .expect("Unable to set the pipeline to the `Playing` state");

    // Wait until error or EOS
    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => (),
        }
    }

    // Shutdown pipeline
    pipeline
        .set_state(gst::State::Null)
        .expect("Unable to set the pipeline to the `Null` state");
}
