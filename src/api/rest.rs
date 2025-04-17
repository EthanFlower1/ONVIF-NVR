use crate::services::analytics::AnalyticsRequest;
use crate::services::recording::RecordingRequest;
use crate::services::streaming::StreamRequest;
use crate::services::{AnalyticsService, CameraManager, RecordingService, StreamingService};
use anyhow::{Result, Error};
use serde_json;
use std::sync::Arc;
use tokio::sync::Mutex;

// Note: In a real application, this would use a proper HTTP server like warp, axum, or actix-web
// This is a simplified version for demonstration purposes

pub struct RestApi {
    camera_manager: Arc<Mutex<CameraManager>>,
    recording_service: Arc<Mutex<RecordingService>>,
    streaming_service: Arc<Mutex<StreamingService>>,
    analytics_service: Arc<Mutex<AnalyticsService>>,
}

impl RestApi {
    pub fn new(
        camera_manager: Arc<Mutex<CameraManager>>,
        recording_service: Arc<Mutex<RecordingService>>,
        streaming_service: Arc<Mutex<StreamingService>>,
        analytics_service: Arc<Mutex<AnalyticsService>>,
    ) -> Self {
        Self {
            camera_manager,
            recording_service,
            streaming_service,
            analytics_service,
        }
    }

    // Camera management endpoints

    pub async fn add_camera(
        &self,
        name: String,
        device_path: String,
        description: Option<String>,
    ) -> Result<String> {
        let mut camera_manager = self.camera_manager.lock().await;
        camera_manager.add_camera(name, device_path, description)
    }

    pub async fn remove_camera(&self, camera_id: String) -> Result<()> {
        let mut camera_manager = self.camera_manager.lock().await;
        camera_manager.remove_camera(&camera_id)
    }

    pub async fn list_cameras(&self) -> Result<Vec<serde_json::Value>> {
        let camera_manager = self.camera_manager.lock().await;
        let cameras = camera_manager.list_cameras();

        // In a real application, you would serialize to proper JSON
        let result = cameras
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "device_path": c.device_path,
                    "description": c.description,
                    "streaming": c.stream_id.is_some(),
                })
            })
            .collect();

        Ok(result)
    }

    pub async fn start_camera_stream(&self, camera_id: String) -> Result<String> {
        let mut camera_manager = self.camera_manager.lock().await;
        camera_manager.start_camera_stream(&camera_id)
    }

    pub async fn stop_camera_stream(&self, camera_id: String) -> Result<()> {
        let mut camera_manager = self.camera_manager.lock().await;
        camera_manager.stop_camera_stream(&camera_id)
    }

    // Recording endpoints

    pub async fn start_recording(&self, request: RecordingRequest) -> Result<String> {
        let mut recording_service = self.recording_service.lock().await;
        recording_service.start_recording(request)
    }

    pub async fn stop_recording(&self, recording_id: String) -> Result<()> {
        let mut recording_service = self.recording_service.lock().await;
        recording_service.stop_recording(&recording_id)
    }

    pub async fn list_recordings(&self) -> Result<Vec<serde_json::Value>> {
        let recording_service = self.recording_service.lock().await;
        let recordings = recording_service.list_recordings();

        // In a real application, you would serialize to proper JSON
        let result = recordings
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "stream_id": r.stream_id,
                    "output_path": r.output_path,
                    "start_time": r.start_time.elapsed().unwrap_or_default().as_secs(),
                    "status": format!("{:?}", r.status),
                })
            })
            .collect();

        Ok(result)
    }

    // Live streaming endpoints

    pub async fn start_stream(&self, request: StreamRequest) -> Result<String> {
        let mut streaming_service = self.streaming_service.lock().await;
        streaming_service.start_stream(request)
    }

    pub async fn stop_stream(&self, stream_id: String) -> Result<()> {
        let mut streaming_service = self.streaming_service.lock().await;
        streaming_service.stop_stream(&stream_id)
    }

    // Analytics endpoints

    pub async fn start_analytics(&self, request: AnalyticsRequest) -> Result<String> {
        let mut analytics_service = self.analytics_service.lock().await;
        analytics_service.start_analytics(request)
    }

    pub async fn stop_analytics(&self, analytics_id: String) -> Result<()> {
        let mut analytics_service = self.analytics_service.lock().await;
        analytics_service.stop_analytics(&analytics_id)
    }

    pub async fn get_analytics_results(&self, analytics_id: String) -> Result<Vec<String>> {
        let analytics_service = self.analytics_service.lock().await;
        analytics_service.get_analytics_results(&analytics_id)
    }
}

pub async fn setup_rest_api(
    camera_manager: Arc<Mutex<CameraManager>>,
    recording_service: Arc<Mutex<RecordingService>>,
    streaming_service: Arc<Mutex<StreamingService>>,
    analytics_service: Arc<Mutex<AnalyticsService>>,
) -> Result<()> {
    let _rest_api = RestApi::new(
        camera_manager,
        recording_service,
        streaming_service,
        analytics_service,
    );

    // In a real application, you would set up routes for a web framework here
    // For example, with warp:
    /*
    let api = warp::path("api")
        .and(
            // Camera routes
            warp::path("cameras")
                .and(warp::post())
                .and(warp::body::json())
                .and(with_rest_api(rest_api.clone()))
                .and_then(add_camera_handler)
                .or(warp::path("cameras")
                    .and(warp::get())
                    .and(with_rest_api(rest_api.clone()))
                    .and_then(list_cameras_handler))
                .or(warp::path!("cameras" / String)
                    .and(warp::delete())
                    .and(with_rest_api(rest_api.clone()))
                    .and_then(remove_camera_handler))
            // ... and so on for all other routes
        );

    warp::serve(api).run(([127, 0, 0, 1], 3030)).await;
    */

    Ok(())
}

