use crate::api::webrtc::{
    add_ice_candidate, close_webrtc_session, create_webrtc_session, process_webrtc_offer,
    WebRTCState,
};
use crate::db::models::camera_models::CameraWithStreams;
use crate::db::models::stream_models::{ReferenceType, Stream, StreamReference, StreamType};
use crate::db::models::user_models::{AuthToken, LoginCredentials, User, UserRole};
use crate::db::repositories::cameras::CamerasRepository;
use crate::db::repositories::users::UsersRepository;
use crate::device_manager;
use crate::device_manager::onvif_client::{OnvifCameraBuilder, OnvifError};
use crate::error::Error;
use crate::security::auth::AuthService;
use crate::stream_manager::StreamManager;
use crate::{config::ApiConfig, db::models::camera_models::Camera};
use anyhow::Result;
use axum::routing::{delete, get, put};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use log::info;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use uuid::Uuid;

// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db_pool: Arc<PgPool>,
    pub cameras_repo: Arc<CamerasRepository>,
    pub stream_manager: Arc<StreamManager>,
    pub auth_service: Arc<AuthService>,
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
            return (*err).clone().into();
        }

        ApiError {
            message: err.to_string(),
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
}

impl RestApi {
    pub fn new(
        config: &ApiConfig,
        db_pool: Arc<PgPool>,
        stream_manager: Arc<StreamManager>,
        auth_service: Arc<AuthService>,
    ) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            db_pool,
            stream_manager,
            auth_service,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let state = AppState {
            db_pool: Arc::clone(&self.db_pool),
            cameras_repo: Arc::new(CamerasRepository::new(self.db_pool.clone())),
            stream_manager: self.stream_manager.clone(),
            auth_service: self.auth_service.clone(),
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
            // .route("/api/cameras/:id", get(get_camera_by_id))
            // .route("/api/cameras/:id", put(update_camera))
            // .route("/api/cameras/:id", delete(delete_camera))
            // .route("/api/cameras/:id/status", put(update_camera_status))
            // .route("/api/cameras/:id/refresh", post(refresh_camera_details))
            // .route("/api/cameras/:id/streams", get(get_camera_streams))
            // Schedule routes
            // .route("/api/schedules", get(get_schedules))
            // .route("/api/schedules", post(create_schedule))
            // .route("/api/schedules/:id", get(get_schedule_by_id))
            // .route("/api/schedules/:id", put(update_schedule))
            // .route("/api/schedules/:id", delete(delete_schedule))
            // .route("/api/schedules/:id/status", put(set_schedule_enabled))
            // .route("/api/cameras/:id/schedules", get(get_schedules_by_camera))
            // Recording routes
            // .route("/api/recordings", get(search_recordings))
            // .route("/api/recordings/:id", get(get_recording_by_id))
            // .route("/api/recordings/:id", delete(delete_recording))
            // .route("/api/recordings/:id/stream", get(stream_recording))
            // .route("/api/recordings/:id/download", get(download_recording))
            // .route("/api/cameras/:id/recordings", get(get_recordings_by_camera))
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
    let repo = CamerasRepository::new(Arc::clone(&state.db_pool));
    let camera = repo.get_by_id(&id).await?.ok_or_else(|| ApiError {
        message: format!("Camera not found: {}", id),
        status: StatusCode::NOT_FOUND.as_u16(),
    })?;

    Ok(Json(camera))
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
