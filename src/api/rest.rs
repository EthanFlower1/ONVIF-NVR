use crate::db::repositories::cameras::CamerasRepository;
use crate::device_manager;
use crate::error::Error;
use crate::{config::ApiConfig, db::models::camera_models::Camera};
use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use log::info;
use serde::Serialize;
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
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub message: String,
    pub status: u16,
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

pub struct RestApi {
    config: ApiConfig,
    db_pool: Arc<PgPool>,
}

impl RestApi {
    pub fn new(config: &ApiConfig, db_pool: Arc<PgPool>) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            db_pool,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let state = AppState {
            db_pool: Arc::clone(&self.db_pool),
        };

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
            // .route("/api/auth/login", post(login))
            // .route("/api/auth/register", post(register))
            // .route("/api/auth/me", get(get_current_user))
            // .route("/api/auth/users/:id/change-password", post(change_password))
            // .route("/api/auth/users/:id/reset-password", post(reset_password))
            // .route("/api/auth/users/:id/role", put(update_role))
            // .route("/api/auth/users/:id/status", put(set_user_active))
            // User routes
            // .route("/api/users", get(get_all_users))
            // .route("/api/users/:id", get(get_user_by_id))
            // .route("/api/users/:id", delete(delete_user))
            // Camera routes
            // .route("/api/cameras", get(get_cameras))
            // .route("/api/cameras", post(create_camera))
            .route("/api/cameras/discover", post(discover_cameras))
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

async fn discover_cameras(State(state): State<AppState>) -> ApiResult<Json<Vec<Camera>>> {
    info!("Starting camera discovery");

    // Discover cameras on the network without requiring database
    let discovered_cameras = device_manager::discovery::discover().await?;
    info!("Discovered {} cameras", discovered_cameras.len());

    // Return the discovered cameras directly without saving to the database
    Ok(Json(discovered_cameras))
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
