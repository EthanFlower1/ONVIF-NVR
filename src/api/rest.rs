use crate::api::webrtc::{
    add_ice_candidate, close_webrtc_session, create_webrtc_session, process_webrtc_offer,
    WebRTCState,
};
use crate::db::models::camera_models::CameraWithStreams;
use crate::db::models::recording_schedule_models::RecordingSchedule;
use crate::db::models::stream_models::{ReferenceType, Stream, StreamReference, StreamType};
use crate::db::models::user_models::{AuthToken, LoginCredentials, User, UserRole};
use crate::db::repositories::cameras::CamerasRepository;
use crate::db::repositories::recordings::RecordingsRepository;
use crate::db::repositories::schedules::SchedulesRepository;
use crate::db::repositories::users::UsersRepository;
use crate::device_manager::onvif_client::{OnvifCameraBuilder, OnvifError};
use crate::error::Error;
use crate::recorder::record::RecordingManager;
use crate::security::auth::AuthService;
use crate::stream_manager::{StreamManager, StreamSource};
use crate::{config::ApiConfig, db::models::camera_models::Camera};
use crate::{device_manager, stream_manager};
use anyhow::Result;
use axum::routing::{delete, get, put};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use log::{info, warn};
use regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use uuid::Uuid;

// Import recording controller
pub mod recording_controller;

// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db_pool: Arc<PgPool>,
    pub cameras_repo: Arc<CamerasRepository>,
    pub stream_manager: Arc<StreamManager>,
    pub auth_service: Arc<AuthService>,
    pub recording_manager: Arc<RecordingManager>,
    pub recordings_repo: Arc<RecordingsRepository>,
    pub schedules_repo: Arc<SchedulesRepository>,
    pub message_broker: Arc<crate::messaging::MessageBroker>,
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub message: String,
    pub status: u16,
}

impl From<OnvifError> for ApiError {
    fn from(err: OnvifError) -> Self {
        ApiError {
            message: err.to_string(),
            status: StatusCode::UNAUTHORIZED.as_u16(),
        }
    }
}

impl From<OnvifError> for Error {
    fn from(err: OnvifError) -> Self {
        Error::Onvif(err.0)
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        match err {
            Error::Authentication(_) => ApiError {
                message: err.to_string(),
                status: StatusCode::UNAUTHORIZED.as_u16(),
            },
            Error::Authorization(_) => ApiError {
                message: err.to_string(),
                status: StatusCode::FORBIDDEN.as_u16(),
            },
            Error::NotFound(_) => ApiError {
                message: err.to_string(),
                status: StatusCode::NOT_FOUND.as_u16(),
            },
            Error::AlreadyExists(_) => ApiError {
                message: err.to_string(),
                status: StatusCode::CONFLICT.as_u16(),
            },
            Error::Onvif(_) | Error::Recording(_) | Error::Streaming(_) | Error::FFmpeg(_) => {
                ApiError {
                    message: err.to_string(),
                    status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                }
            }
            Error::Config(_) => ApiError {
                message: err.to_string(),
                status: StatusCode::BAD_REQUEST.as_u16(),
            },
            _ => ApiError {
                message: err.to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            },
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        if let Some(err) = err.downcast_ref::<Error>() {
            return err.clone().into();
        }

        ApiError {
            message: err.to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError {
            message: format!("JSON serialization error: {}", err),
            status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        }
    }
}

/// Implement IntoResponse for ApiError
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = Json(self);
        (status, body).into_response()
    }
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    email: String,
    password: String,
    role: Option<UserRole>,
}

pub struct RestApi {
    config: ApiConfig,
    db_pool: Arc<PgPool>,
    stream_manager: Arc<StreamManager>,
    auth_service: Arc<AuthService>,
    message_broker: Arc<crate::messaging::MessageBroker>,
}

impl RestApi {
    pub fn new(
        config: &ApiConfig,
        db_pool: Arc<PgPool>,
        stream_manager: Arc<StreamManager>,
        auth_service: Arc<AuthService>,
        message_broker: Arc<crate::messaging::MessageBroker>,
    ) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            db_pool,
            stream_manager,
            auth_service,
            message_broker,
        })
    }

    pub async fn run(&self) -> Result<()> {
        // Create recording manager
        let recording_manager = Arc::new(RecordingManager::new(
            Arc::clone(&self.db_pool),
            Arc::clone(&self.stream_manager),
            &std::path::Path::new("./recordings"),
            300, // 5 minutes segment duration
            "mp4",
        ));

        let state = AppState {
            db_pool: Arc::clone(&self.db_pool),
            cameras_repo: Arc::new(CamerasRepository::new(self.db_pool.clone())),
            stream_manager: self.stream_manager.clone(),
            auth_service: self.auth_service.clone(),
            recording_manager: Arc::clone(&recording_manager),
            recordings_repo: Arc::new(RecordingsRepository::new(self.db_pool.clone())),
            schedules_repo: Arc::new(SchedulesRepository::new(self.db_pool.clone())),
            message_broker: self.message_broker.clone(),
        };

        let webrtc_state = Arc::new(WebRTCState::new(
            Arc::clone(&self.db_pool),
            Arc::clone(&self.stream_manager),
        ));

        // Create a CORS layer that allows all origins and preflight requests
        use std::time::Duration;
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(false)
            .max_age(Duration::from_secs(3600));

        // Build the API router with routes
        let app = Router::new()
            // Auth routes
            .route("/api/auth/login", post(login))
            .route("/api/auth/register", post(register))
            .route("/api/auth/me", get(get_current_user))
            .route("/api/auth/users/:id/change-password", post(change_password))
            .route("/api/auth/users/:id/reset-password", post(reset_password))
            .route("/api/auth/users/:id/role", put(update_role))
            .route("/api/auth/users/:id/status", put(set_user_active))
            // User routes
            .route("/api/users", get(get_all_users))
            .route("/api/users/:id", get(get_user_by_id))
            .route("/api/users/:id", delete(delete_user))
            // Camera routes
            .route("/api/cameras", get(get_cameras))
            // .route("/api/cameras", post(create_camera))
            .route("/api/cameras/discover", post(discover_cameras))
            .route("/api/cameras/connect", post(camera_connect))
            .route("/api/cameras/:id", get(get_camera_by_id))
            .route("/api/cameras/:id", put(update_camera))
            .route("/api/cameras/:id", delete(delete_camera))
            .route("/api/cameras/:id/status", put(update_camera_status))
            .route("/api/cameras/:id/refresh", post(refresh_camera_details))
            // .route("/api/cameras/:id/streams", get(get_camera_streams))
            // Schedule routes
            .route("/api/schedules", get(get_schedules))
            .route("/api/schedules", post(create_schedule))
            .route("/api/schedules/:id", get(get_schedule_by_id))
            .route("/api/schedules/:id", put(update_schedule))
            .route("/api/schedules/:id", delete(delete_schedule))
            .route("/api/schedules/:id/status", put(set_schedule_enabled))
            .route("/api/cameras/:id/schedules", get(get_schedules_by_camera))
            // Recording API routes
            .route("/api/recordings", get(search_recordings))
            .route("/api/recordings/:id", get(get_recording_by_id))
            .route("/api/recordings/:id", delete(delete_recording))
            .route("/api/recordings/:id/stream", get(stream_recording))
            .route("/api/recordings/:id/download", get(download_recording))
            .route("/api/cameras/:id/recordings", get(get_recordings_by_camera))
            // Create recording controller with routes using state
            .nest(
                "/recording",
                recording_controller::create_router(state.clone()),
            )
            // Regular routes with AppState
            .with_state(state)
            // Add WebRTC routes with their own state
            .nest(
                "/webrtc",
                Router::new()
                    .route("/session", post(create_webrtc_session))
                    .route("/offer", post(process_webrtc_offer))
                    .route("/ice", post(add_ice_candidate))
                    .route("/close/:session_id", get(close_webrtc_session))
                    .with_state(webrtc_state),
            )
            // Add WebSocket routes separately with their own state
            // Serve static files from the public directory
            .nest_service("/", ServeDir::new("public"))
            // Apply CORS middleware to all routes
            .layer(cors);

        // Build the server address
        let addr = self.config.address.clone() + ":" + &self.config.port.to_string();
        let addr: SocketAddr = addr.parse()?;

        // Log that we're starting
        info!("API server listening on {}", addr);

        // Create a listener and start the server
        let listener = TcpListener::bind(addr).await?;

        // Start serving (using axum's Server method)
        axum::Server::from_tcp(listener.into_std()?)?
            .serve(app.into_make_service())
            .await?;

        Ok(())
    }
}

// async fn get_cameras(State(state): State<AppState>) -> ApiResult<Json<Vec<Camera>>> {
//     let repo = CamerasRepository::new(Arc::clone(&state.db_pool));
//     let cameras = repo.get_all().await?;
//     Ok(Json(cameras))
// }

async fn discover_cameras(State(_state): State<AppState>) -> ApiResult<Json<Vec<Camera>>> {
    info!("Starting camera discovery");

    let discovered_cameras = device_manager::discovery::discover().await?;

    Ok(Json(discovered_cameras))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConnectRequest {
    pub username: String,
    pub password: String,
    pub ip_address: String,
}
async fn camera_connect(
    State(state): State<AppState>,
    Json(req): Json<CameraConnectRequest>,
) -> ApiResult<Json<CameraWithStreams>> {
    info!("Connection to Camera");
    let mut camera = Camera::default();
    camera.username = Some(req.username.clone());
    camera.password = Some(req.password.clone());

    let client = OnvifCameraBuilder::new()
        .uri(&format!("http://{}", &req.ip_address))?
        .credentials(&req.username, &req.password)
        .service_path("onvif/device_service")
        .fix_time(true)
        .auth_type("digest")
        .build()
        .await?;

    let device_info = client.get_device_information().await?;
    camera.manufacturer = Some(device_info.manufacturer);
    camera.model = Some(device_info.model);
    camera.ip_address = req.ip_address;
    camera.firmware_version = Some(device_info.firmware_version);
    camera.serial_number = Some(device_info.serial_number);
    camera.hardware_id = Some(device_info.hardware_id);

    let stream_uris = client.get_stream_uris().await?;
    let mut streams: Vec<Stream> = vec![];
    let mut stream_references: Vec<StreamReference> = vec![];

    for (i, stream_response) in stream_uris.iter().enumerate() {
        let now = Utc::now();
        let mut stream = Stream::default();
        if let Some((width, height)) = stream_response.video_resolution {
            stream.width = Some(width as i32);
            stream.height = Some(height as i32);
        }
        stream.camera_id = camera.id;
        stream.name = stream_response.name.clone();
        stream.url = stream_response.uri.clone();
        stream.codec = stream_response.video_encoding.clone();
        stream.framerate = stream_response.framerate.map(|value| value as i32);
        stream.bitrate = stream_response.bitrate.map(|value| value as i32);
        stream.audio_bitrate = stream_response.audio_bitrate.map(|value| value as i32);
        stream.audio_sample_rate = stream_response.audio_samplerate.map(|value| value as i32);
        stream.audio_codec = stream_response.audio_encoding.clone();
        stream.stream_type = StreamType::Rtsp;
        stream.is_active = Some(false);
        stream.is_primary = Some(i == 0);
        stream.updated_at = now;
        stream.created_at = now;

        let stream_ref = StreamReference {
            id: Uuid::new_v4(),
            camera_id: camera.id,
            stream_id: stream.id,
            reference_type: match i {
                0 => ReferenceType::Primary,
                1 => ReferenceType::Sub,
                2 => ReferenceType::Tertiary,
                3 => ReferenceType::Lowres,
                4 => ReferenceType::Mobile,
                5 => ReferenceType::Analytics,
                _ => ReferenceType::Unknown, // Default for any index beyond 5
            },
            display_order: Some(i as i32),
            is_default: Some(i == 0),
            created_at: now,
            updated_at: now,
        };

        streams.push(stream);
        stream_references.push(stream_ref);
    }

    let camera_with_streams = CameraWithStreams {
        camera,
        streams,
        stream_references,
    };

    // Clone what we need to pass to the thread
    let stream_manager = state.stream_manager.clone();
    let streams_for_thread = camera_with_streams.streams.clone();
    let username = camera_with_streams
        .camera
        .username
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Camera username is missing"))?;
    let password = camera_with_streams
        .camera
        .password
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Camera password is missing"))?;

    // Spawn a new thread to handle stream connections
    tokio::spawn(async move {
        for stream in streams_for_thread {
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

            match stream_manager.add_stream(source, stream.id.to_string()) {
                Ok(stream_id) => {
                    println!("Created stream with ID: {}", stream_id);
                }
                Err(e) => {
                    warn!("Failed to add stream: {}", e);
                }
            }
        }
    });

    let db_response = state
        .cameras_repo
        .create_with_streams(&camera_with_streams)
        .await?;

    Ok(Json(db_response))
}

async fn get_cameras(State(state): State<AppState>) -> ApiResult<Json<Vec<CameraWithStreams>>> {
    info!("Getting cameras with streams...");
    let cameras = state.cameras_repo.get_all_with_streams().await?;

    Ok(Json(cameras))
}

async fn get_camera_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Camera>> {
    let camera = state
        .cameras_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Camera not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    Ok(Json(camera))
}

#[derive(Debug, Deserialize)]
struct CameraUpdateRequest {
    name: Option<String>,
    model: Option<String>,
    manufacturer: Option<String>,
    ip_address: Option<String>,
    username: Option<String>,
    password: Option<String>,
    onvif_endpoint: Option<String>,
    status: Option<String>,
    ptz_supported: Option<bool>,
    audio_supported: Option<bool>,
    analytics_supported: Option<bool>,
    recording_mode: Option<String>,
    retention_days: Option<i32>,
}

async fn update_camera(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CameraUpdateRequest>,
) -> ApiResult<Json<Camera>> {
    let mut camera = state
        .cameras_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Camera not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    // Track if credentials are being updated
    let mut credentials_updated = false;
    let old_username = camera.username.clone();
    let old_password = camera.password.clone();

    if let Some(name) = req.name {
        camera.name = name;
    }

    if let Some(model) = req.model {
        camera.model = Some(model);
    }

    if let Some(manufacturer) = req.manufacturer {
        camera.manufacturer = Some(manufacturer);
    }

    if let Some(ip_address) = req.ip_address {
        camera.ip_address = ip_address;
    }

    if let Some(username) = req.username {
        if old_username != Some(username.clone()) {
            credentials_updated = true;
        }
        camera.username = Some(username);
    }

    if let Some(password) = req.password {
        if old_password != Some(password.clone()) {
            credentials_updated = true;
        }
        camera.password = Some(password);
    }

    if let Some(onvif_endpoint) = req.onvif_endpoint {
        camera.onvif_endpoint = Some(onvif_endpoint);
    }

    if let Some(status) = req.status {
        camera.status = status;
    }

    if let Some(ptz_supported) = req.ptz_supported {
        camera.ptz_supported = Some(ptz_supported);
    }

    if let Some(audio_supported) = req.audio_supported {
        camera.audio_supported = Some(audio_supported);
    }

    if let Some(analytics_supported) = req.analytics_supported {
        camera.analytics_supported = Some(analytics_supported);
    }

    if let Some(recording_mode) = req.recording_mode {
        camera.recording_mode = Some(recording_mode);
    }

    if let Some(retention_days) = req.retention_days {
        camera.retention_days = Some(retention_days);
    }

    // Update the camera with the new info
    let updated = state.cameras_repo.update(&camera).await?;

    Ok(Json(updated))
}

#[derive(Debug, Deserialize)]
struct CameraStatusUpdateRequest {
    status: String,
}

async fn update_camera_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CameraStatusUpdateRequest>,
) -> ApiResult<Json<Camera>> {
    // Validate status value
    let valid_statuses = [
        "discovered",
        "connected",
        "active",
        "inactive",
        "error",
        "offline",
    ];
    if !valid_statuses.contains(&req.status.as_str()) {
        return Err(ApiError {
            message: format!(
                "Invalid status. Must be one of: {}",
                valid_statuses.join(", ")
            ),
            status: StatusCode::BAD_REQUEST.as_u16(),
        });
    }

    // Get the camera to update
    let mut camera = state
        .cameras_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Camera not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    // Update the status
    camera.status = req.status;

    // Use the repository method specifically for status update if complex logic needed,
    // or just update the camera object
    state
        .cameras_repo
        .update_status(&id, &camera.status)
        .await?;

    // Fetch the updated camera to return the latest state
    let updated_camera = state
        .cameras_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Camera not found after update: {}", id),
            status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        })?;

    Ok(Json(updated_camera))
}

async fn refresh_camera_details(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CameraWithStreams>> {
    // Get existing camera
    let camera = state
        .cameras_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Camera not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    // Ensure we have credentials
    let username = camera.username.clone().ok_or_else(|| ApiError {
        message: "Camera username is missing".to_string(),
        status: StatusCode::BAD_REQUEST.as_u16(),
    })?;

    let password = camera.password.clone().ok_or_else(|| ApiError {
        message: "Camera password is missing".to_string(),
        status: StatusCode::BAD_REQUEST.as_u16(),
    })?;

    // Create ONVIF client to get fresh device information
    let client = OnvifCameraBuilder::new()
        .uri(&format!("http://{}", &camera.ip_address))?
        .credentials(&username, &password)
        .service_path(
            camera
                .onvif_endpoint
                .as_deref()
                .unwrap_or("onvif/device_service"),
        )
        .fix_time(true)
        .auth_type("digest")
        .build()
        .await?;

    // Get updated device information
    let device_info = client.get_device_information().await?;

    // Get stream URIs
    let stream_uris = client.get_stream_uris().await?;

    // Create an updated camera with streams object
    let mut updated_camera = camera.clone();
    updated_camera.manufacturer = Some(device_info.manufacturer);
    updated_camera.model = Some(device_info.model);
    updated_camera.firmware_version = Some(device_info.firmware_version);
    updated_camera.serial_number = Some(device_info.serial_number);
    updated_camera.hardware_id = Some(device_info.hardware_id);
    updated_camera.updated_at = Utc::now();
    updated_camera.last_updated = Some(Utc::now());

    // Get existing streams for this camera
    let existing_streams = state.cameras_repo.get_streams(&id).await?;
    let mut streams = Vec::new();
    let mut stream_references = Vec::new();

    // Update existing streams or create new ones
    for (i, stream_response) in stream_uris.iter().enumerate() {
        let now = Utc::now();

        // Try to find an existing stream to update
        let stream_exists = i < existing_streams.len();

        let mut stream = if stream_exists {
            let mut existing = existing_streams[i].clone();
            existing.camera_id = updated_camera.id;
            existing.name = stream_response.name.clone();
            existing.url = stream_response.uri.clone();
            existing.codec = stream_response.video_encoding.clone();
            existing.framerate = stream_response.framerate.map(|value| value as i32);
            existing.bitrate = stream_response.bitrate.map(|value| value as i32);
            existing.audio_bitrate = stream_response.audio_bitrate.map(|value| value as i32);
            existing.audio_sample_rate = stream_response.audio_samplerate.map(|value| value as i32);
            existing.audio_codec = stream_response.audio_encoding.clone();
            existing
        } else {
            let mut new_stream = Stream::default();
            new_stream.camera_id = updated_camera.id;
            new_stream.name = stream_response.name.clone();
            new_stream.url = stream_response.uri.clone();
            new_stream.codec = stream_response.video_encoding.clone();
            new_stream.framerate = stream_response.framerate.map(|value| value as i32);
            new_stream.bitrate = stream_response.bitrate.map(|value| value as i32);
            new_stream.audio_bitrate = stream_response.audio_bitrate.map(|value| value as i32);
            new_stream.audio_sample_rate =
                stream_response.audio_samplerate.map(|value| value as i32);
            new_stream.audio_codec = stream_response.audio_encoding.clone();
            new_stream
        };

        if let Some((width, height)) = stream_response.video_resolution {
            stream.width = Some(width as i32);
            stream.height = Some(height as i32);
            stream.resolution = Some(format!("{}x{}", width, height));
        }

        // Set stream type and primary flag
        stream.is_primary = Some(i == 0);
        stream.updated_at = now;

        // Add stream reference if it's a new stream
        if !stream_exists {
            let stream_ref = StreamReference {
                id: Uuid::new_v4(),
                camera_id: updated_camera.id,
                stream_id: stream.id,
                reference_type: match i {
                    0 => ReferenceType::Primary,
                    1 => ReferenceType::Sub,
                    2 => ReferenceType::Tertiary,
                    3 => ReferenceType::Lowres,
                    4 => ReferenceType::Mobile,
                    5 => ReferenceType::Analytics,
                    _ => ReferenceType::Unknown,
                },
                display_order: Some(i as i32),
                is_default: Some(i == 0),
                created_at: now,
                updated_at: now,
            };
            stream_references.push(stream_ref);
        }

        streams.push(stream);
    }

    // Create camera with streams object for update
    let camera_with_streams = CameraWithStreams {
        camera: updated_camera,
        streams,
        stream_references,
    };

    // Update camera and streams in database
    let updated = state
        .cameras_repo
        .update_with_streams(&camera_with_streams)
        .await?;

    info!("Successfully refreshed camera details for {}", id);
    Ok(Json(updated))
}

async fn delete_camera(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    // Check if camera exists first
    let camera = state
        .cameras_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Camera not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    // Get existing streams and stop any active recordings
    let streams = state.cameras_repo.get_streams(&id).await?;

    // Stop any active recordings for this camera
    for stream in &streams {
        if let Some(is_active) = stream.is_active {
            if is_active {
                // Try to stop recording, but don't fail if it doesn't work
                match state
                    .recording_manager
                    .stop_recording(&camera.id, &stream.id)
                    .await
                {
                    Ok(_) => info!(
                        "Stopped recording for stream {} before camera deletion",
                        stream.id
                    ),
                    Err(e) => info!("Failed to stop recording for stream {}: {}", stream.id, e),
                }
            }
        }
    }

    // Delete camera and all related data
    let result = state.cameras_repo.delete(&id).await?;

    // Publish camera deleted event
    let camera_events = crate::messaging::CameraEvents::new(state.message_broker.clone());
    if let Err(e) = camera_events.camera_deleted(id, &camera.name).await {
        warn!("Failed to publish camera deleted event: {}", e);
    } else {
        info!("Published camera deleted event for {}", id);
    }

    Ok(Json(serde_json::json!({
        "success": result,
        "id": id.to_string(),
        "message": format!("Camera '{}' deleted successfully", camera.name)
    })))
}

// Auth API Handlers
async fn login(
    State(state): State<AppState>,
    Json(credentials): Json<LoginCredentials>,
) -> ApiResult<Json<(User, AuthToken)>> {
    let (user, token) = state.auth_service.login(&credentials).await?;
    Ok(Json((user, token)))
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<Json<(User, AuthToken)>> {
    let role = req.role.unwrap_or(UserRole::Viewer);
    let user = state
        .auth_service
        .register(&req.username, &req.email, &req.password, role)
        .await?;

    // After registration, log the user in
    let credentials = LoginCredentials {
        username: user.username,
        password: req.password,
    };
    let (user, token) = state.auth_service.login(&credentials).await?;

    Ok(Json((user, token)))
}

async fn get_current_user(
    State(state): State<AppState>,
    // TODO: Add authentication middleware to extract user from token
) -> ApiResult<Json<User>> {
    // For now, return a mock user
    let repo = UsersRepository::new(Arc::clone(&state.db_pool));
    let users = repo.get_all().await?;
    if let Some(user) = users.first() {
        Ok(Json(user.clone()))
    } else {
        Err(ApiError {
            message: "No users found".to_string(),
            status: StatusCode::NOT_FOUND.as_u16(),
        })
    }
}

async fn change_password(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> ApiResult<Json<()>> {
    let current_password = payload
        .get("current_password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError {
            message: "Missing current_password".to_string(),
            status: StatusCode::BAD_REQUEST.as_u16(),
        })?;

    let new_password = payload
        .get("new_password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError {
            message: "Missing new_password".to_string(),
            status: StatusCode::BAD_REQUEST.as_u16(),
        })?;

    state
        .auth_service
        .change_password(&user_id, current_password, new_password)
        .await?;
    Ok(Json(()))
}

async fn reset_password(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let new_password = state.auth_service.reset_password(&user_id).await?;
    Ok(Json(serde_json::json!({ "password": new_password })))
}

async fn update_role(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> ApiResult<Json<User>> {
    let role_str = payload
        .get("role")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError {
            message: "Missing role".to_string(),
            status: StatusCode::BAD_REQUEST.as_u16(),
        })?;

    let role = match role_str {
        "admin" => UserRole::Admin,
        "operator" => UserRole::Operator,
        "viewer" => UserRole::Viewer,
        _ => {
            return Err(ApiError {
                message: format!("Invalid role: {}", role_str),
                status: StatusCode::BAD_REQUEST.as_u16(),
            })
        }
    };

    let user = state.auth_service.update_role(&user_id, role).await?;
    Ok(Json(user))
}

async fn set_user_active(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> ApiResult<Json<User>> {
    let active = payload
        .get("active")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| ApiError {
            message: "Missing active status".to_string(),
            status: StatusCode::BAD_REQUEST.as_u16(),
        })?;

    let user = state.auth_service.set_active(&user_id, active).await?;
    Ok(Json(user))
}

// User API Handlers
async fn get_all_users(State(state): State<AppState>) -> ApiResult<Json<Vec<User>>> {
    let repo = UsersRepository::new(Arc::clone(&state.db_pool));
    let users = repo.get_all().await?;
    Ok(Json(users))
}

async fn get_user_by_id(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<User>> {
    let repo = UsersRepository::new(Arc::clone(&state.db_pool));
    let user = repo.get_by_id(&user_id).await?.ok_or_else(|| ApiError {
        message: format!("User not found: {}", user_id),
        status: StatusCode::NOT_FOUND.as_u16(),
    })?;

    Ok(Json(user))
}

async fn delete_user(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<()>> {
    let repo = UsersRepository::new(Arc::clone(&state.db_pool));
    repo.delete(&user_id).await?;
    Ok(Json(()))
}

// Recording API handlers
async fn search_recordings(
    State(state): State<AppState>,
    Query(params): Query<recording_controller::SearchParams>,
) -> ApiResult<Json<HashMap<String, serde_json::Value>>> {
    // Convert search parameters to the internal search query format
    let mut query = crate::db::models::recording_models::RecordingSearchQuery {
        camera_ids: None,
        stream_ids: None,
        start_time: None,
        end_time: None,
        event_types: None,
        schedule_id: None,
        min_duration: None,
        segment_id: params.segment_id,
        parent_recording_id: params
            .parent_recording_id
            .as_ref()
            .and_then(|id| Uuid::parse_str(id).ok()),
        is_segment: params.is_segment,
        limit: params.limit,
        offset: params.offset,
    };

    // Parse camera ID if provided
    if let Some(camera_id_str) = &params.camera_id {
        if let Ok(camera_id) = Uuid::parse_str(camera_id_str) {
            query.camera_ids = Some(vec![camera_id]);
        }
    }

    // Parse stream ID if provided
    if let Some(stream_id_str) = &params.stream_id {
        if let Ok(stream_id) = Uuid::parse_str(stream_id_str) {
            query.stream_ids = Some(vec![stream_id]);
        }
    }

    // Parse start time if provided
    if let Some(start_time_str) = &params.start_time {
        if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(start_time_str) {
            query.start_time = Some(start_time.with_timezone(&Utc));
        }
    }

    // Parse end time if provided
    if let Some(end_time_str) = &params.end_time {
        if let Ok(end_time) = chrono::DateTime::parse_from_rfc3339(end_time_str) {
            query.end_time = Some(end_time.with_timezone(&Utc));
        }
    }

    // Parse event type if provided
    if let Some(event_type_str) = &params.event_type {
        let event_type = match event_type_str.to_lowercase().as_str() {
            "continuous" => crate::db::models::recording_models::RecordingEventType::Continuous,
            "motion" => crate::db::models::recording_models::RecordingEventType::Motion,
            "audio" => crate::db::models::recording_models::RecordingEventType::Audio,
            "external" => crate::db::models::recording_models::RecordingEventType::External,
            "manual" => crate::db::models::recording_models::RecordingEventType::Manual,
            "analytics" => crate::db::models::recording_models::RecordingEventType::Analytics,
            _ => {
                return Err(ApiError {
                    message: format!("Invalid event type: {}", event_type_str),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                })
            }
        };

        query.event_types = Some(vec![event_type]);
    }

    // Execute search query
    let recordings = state.recordings_repo.search(&query).await?;

    // Convert to response format
    let mut response = HashMap::new();
    response.insert("count".to_string(), serde_json::json!(recordings.len()));
    response.insert("recordings".to_string(), serde_json::to_value(&recordings)?);

    Ok(Json(response))
}

async fn get_recording_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let recording = state
        .recordings_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Recording not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    Ok(Json(serde_json::to_value(recording)?))
}

async fn delete_recording(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<()>> {
    state.recordings_repo.delete(&id).await?;
    Ok(Json(()))
}

async fn stream_recording(State(_state): State<AppState>, Path(_id): Path<Uuid>) -> ApiResult<()> {
    // Implement streaming logic - for now just return not implemented
    Err(ApiError {
        message: "Streaming not yet implemented".to_string(),
        status: StatusCode::NOT_IMPLEMENTED.as_u16(),
    })
}

async fn download_recording(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> ApiResult<()> {
    // Implement download logic - for now just return not implemented
    Err(ApiError {
        message: "Download not yet implemented".to_string(),
        status: StatusCode::NOT_IMPLEMENTED.as_u16(),
    })
}

async fn get_recordings_by_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<Uuid>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    // Create a search query for this camera's recordings
    let query = crate::db::models::recording_models::RecordingSearchQuery {
        camera_ids: Some(vec![camera_id]),
        stream_ids: None,
        start_time: None,
        end_time: None,
        event_types: None,
        schedule_id: None,
        min_duration: None,
        segment_id: None,
        parent_recording_id: None,
        is_segment: Some(false), // Only return parent recordings
        limit: Some(100),
        offset: Some(0),
    };

    // Execute search query
    let recordings = state.recordings_repo.search(&query).await?;

    // Convert to JSON value array
    let recordings_json = serde_json::to_value(recordings)?;

    if let serde_json::Value::Array(recordings_array) = recordings_json {
        Ok(Json(recordings_array))
    } else {
        Err(ApiError {
            message: "Failed to convert recordings to JSON array".to_string(),
            status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        })
    }
}

// Handler for getting schedules by camera ID
async fn get_schedules_by_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<Uuid>,
) -> ApiResult<Json<Vec<RecordingSchedule>>> {
    // Get schedules for the camera from repository
    let schedules = state.schedules_repo.get_by_camera(&camera_id).await?;
    Ok(Json(schedules))
}

// Schedule API handlers
async fn get_schedules(State(state): State<AppState>) -> ApiResult<Json<Vec<RecordingSchedule>>> {
    // Get all schedules from repository
    let schedules = state.schedules_repo.get_all().await?;
    Ok(Json(schedules))
}

#[derive(Debug, Deserialize)]
struct CreateScheduleRequest {
    camera_id: Uuid,
    stream_id: Uuid,
    name: String,
    enabled: bool,
    days_of_week: Vec<i32>,
    start_time: String,
    end_time: String,
    retention_days: i32,
}

async fn create_schedule(
    State(state): State<AppState>,
    Json(req): Json<CreateScheduleRequest>,
) -> ApiResult<Json<RecordingSchedule>> {
    // Validate time format (HH:MM)
    let time_regex = regex::Regex::new(r"^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$").unwrap();
    if !time_regex.is_match(&req.start_time) || !time_regex.is_match(&req.end_time) {
        return Err(ApiError {
            message: "Invalid time format. Use HH:MM format (24-hour)".to_string(),
            status: StatusCode::BAD_REQUEST.as_u16(),
        });
    }

    // Validate days of week (0-6)
    for day in &req.days_of_week {
        if *day < 0 || *day > 6 {
            return Err(ApiError {
                message: "Days of week must be between 0 (Sunday) and 6 (Saturday)".to_string(),
                status: StatusCode::BAD_REQUEST.as_u16(),
            });
        }
    }

    // Create schedule object
    let now = Utc::now();
    let schedule = RecordingSchedule {
        id: Uuid::new_v4(),
        camera_id: req.camera_id,
        stream_id: req.stream_id,
        name: req.name,
        enabled: req.enabled,
        days_of_week: req.days_of_week,
        start_time: req.start_time,
        end_time: req.end_time,
        created_at: now,
        updated_at: now,
        retention_days: req.retention_days,
    };

    // Create schedule in repository
    let created_schedule = state.schedules_repo.create(&schedule).await?;
    Ok(Json(created_schedule))
}

async fn get_schedule_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RecordingSchedule>> {
    // Get schedule by ID
    let schedule = state
        .schedules_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Schedule not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    Ok(Json(schedule))
}

#[derive(Debug, Deserialize)]
struct UpdateScheduleRequest {
    camera_id: Option<Uuid>,
    stream_id: Option<Uuid>,
    name: Option<String>,
    enabled: Option<bool>,
    days_of_week: Option<Vec<i32>>,
    start_time: Option<String>,
    end_time: Option<String>,
    retention_days: Option<i32>,
}

async fn update_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateScheduleRequest>,
) -> ApiResult<Json<RecordingSchedule>> {
    // First get the existing schedule
    let mut schedule = state
        .schedules_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Schedule not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    // Update fields if provided
    if let Some(camera_id) = req.camera_id {
        schedule.camera_id = camera_id;
    }

    if let Some(stream_id) = req.stream_id {
        schedule.stream_id = stream_id;
    }

    if let Some(name) = req.name {
        schedule.name = name;
    }

    if let Some(enabled) = req.enabled {
        schedule.enabled = enabled;
    }

    if let Some(days_of_week) = req.days_of_week {
        // Validate days of week (0-6)
        for day in &days_of_week {
            if *day < 0 || *day > 6 {
                return Err(ApiError {
                    message: "Days of week must be between 0 (Sunday) and 6 (Saturday)".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                });
            }
        }
        schedule.days_of_week = days_of_week;
    }

    if let Some(start_time) = req.start_time {
        // Validate time format (HH:MM)
        let time_regex = regex::Regex::new(r"^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$").unwrap();
        if !time_regex.is_match(&start_time) {
            return Err(ApiError {
                message: "Invalid start time format. Use HH:MM format (24-hour)".to_string(),
                status: StatusCode::BAD_REQUEST.as_u16(),
            });
        }
        schedule.start_time = start_time;
    }

    if let Some(end_time) = req.end_time {
        // Validate time format (HH:MM)
        let time_regex = regex::Regex::new(r"^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$").unwrap();
        if !time_regex.is_match(&end_time) {
            return Err(ApiError {
                message: "Invalid end time format. Use HH:MM format (24-hour)".to_string(),
                status: StatusCode::BAD_REQUEST.as_u16(),
            });
        }
        schedule.end_time = end_time;
    }

    if let Some(retention_days) = req.retention_days {
        schedule.retention_days = retention_days;
    }

    // Update timestamp
    schedule.updated_at = Utc::now();

    // Update schedule in repository
    let updated_schedule = state.schedules_repo.update(&schedule).await?;
    Ok(Json(updated_schedule))
}

async fn delete_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<bool>> {
    // Delete schedule by ID
    let result = state.schedules_repo.delete(&id).await?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct ScheduleEnabledRequest {
    enabled: bool,
}

async fn set_schedule_enabled(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<ScheduleEnabledRequest>,
) -> ApiResult<Json<()>> {
    // Check if schedule exists first
    let _ = state
        .schedules_repo
        .get_by_id(&id)
        .await?
        .ok_or_else(|| ApiError {
            message: format!("Schedule not found: {}", id),
            status: StatusCode::NOT_FOUND.as_u16(),
        })?;

    // Set schedule enabled status
    state.schedules_repo.set_enabled(&id, req.enabled).await?;

    Ok(Json(()))
}

