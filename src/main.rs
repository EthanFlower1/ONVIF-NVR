use anyhow::Result;
use db::migrations;
use gst::prelude::*;
use gstreamer as gst;
use log::info;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

#[path = "./tutorial-common.rs"]
mod tutorials_common;

mod api;
mod config;
mod db;
mod device_manager;
mod error;
mod stream_manager;
pub use error::Error;

use stream_manager::StreamManager;

async fn run_app() -> Result<()> {
    // Initialize logging
    env_logger::init();
    info!("Starting G-Streamer Stream Management System");

    // Initialize GStreamer
    gst::init()?;
    info!("GStreamer initialized successfully");

    let config = config::load_config(None)?;
    info!("Configuration loaded");
    // Load configuration
    // let config = config::setup_config()?;
    // info!("Configuration loaded");

    // Create shared stream manager
    let _stream_manager = Arc::new(StreamManager::new());
    info!("Stream manager initialized");

    let db_pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database.url)
        .await?;

    info!("Running migration...");
    match migrations::run_migrations(&db_pool).await {
        Ok(_) => {
            log::info!("Migrations completed successfully");
        }
        Err(err) => {
            log::error!("Failed to run migrations: {}", err);
            // You might want to panic or return the error depending on your application needs
            // panic!("Migration failed: {}", err);
            // Or return the error if this is inside a function
            // return Err(err.into());
        }
    }
    //
    let db_pool = std::sync::Arc::new(db_pool);

    // Start API servers
    let http_server = api::rest::RestApi::new(&config.api, db_pool).unwrap();

    let _ = http_server.run().await;

    //
    // let websocket_api = api::websocket::setup_websocket_api(
    //     camera_manager.clone(),
    //     recording_service.clone(),
    //     streaming_service.clone(),
    //     analytics_service.clone(),
    // )
    // .await?;
    info!("API servers started");

    // In a real application, we would wait for termination signals
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

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
