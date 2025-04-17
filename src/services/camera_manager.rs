use crate::stream_manager::{StreamId, StreamManager, StreamSource, StreamType};
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Camera {
    pub id: String,
    pub name: String,
    pub device_path: String,
    pub description: Option<String>,
    pub stream_id: Option<StreamId>,
    pub camera_type: CameraType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraType {
    Local,
    Network,
    File,
    TestSource,
}

pub struct CameraManager {
    stream_manager: Arc<StreamManager>,
    cameras: HashMap<String, Camera>,
}

impl CameraManager {
    pub fn new(stream_manager: Arc<StreamManager>) -> Self {
        Self {
            stream_manager,
            cameras: HashMap::new(),
        }
    }

    pub fn add_camera(
        &mut self,
        name: String,
        device_path: String,
        description: Option<String>,
    ) -> Result<String> {
        let camera_id = uuid::Uuid::new_v4().to_string();

        // Determine camera type based on device_path
        let camera_type = if device_path.starts_with("rtsp://")
            || device_path.starts_with("rtsps://")
            || device_path.starts_with("http://")
            || device_path.starts_with("https://")
        {
            CameraType::Network
        } else if device_path.contains(".mp4")
            || device_path.contains(".avi")
            || device_path.contains(".mov")
            || device_path.contains(".mkv")
        {
            CameraType::File
        } else if device_path == "test" || device_path.starts_with("test:") {
            CameraType::TestSource
        } else {
            // Assume it's a local device path or index
            CameraType::Local
        };

        let camera = Camera {
            id: camera_id.clone(),
            name,
            device_path,
            description,
            stream_id: None,
            camera_type,
        };

        self.cameras.insert(camera_id.clone(), camera);

        Ok(camera_id)
    }

    pub fn remove_camera(&mut self, camera_id: &str) -> Result<()> {
        if let Some(camera) = self.cameras.get(camera_id) {
            // If camera is streaming, stop it
            if let Some(stream_id) = &camera.stream_id {
                self.stream_manager.remove_stream(stream_id)?;
            }

            self.cameras.remove(camera_id);
            Ok(())
        } else {
            Err(anyhow!("Camera not found: {}", camera_id))
        }
    }

    pub fn start_camera_stream(&mut self, camera_id: &str) -> Result<StreamId> {
        let camera = self
            .cameras
            .get_mut(camera_id)
            .ok_or_else(|| anyhow!("Camera not found: {}", camera_id))?;

        // Check if camera is already streaming
        if let Some(stream_id) = &camera.stream_id {
            return Ok(stream_id.clone());
        }

        // Map camera type to stream type
        let stream_type = match camera.camera_type {
            CameraType::Local => StreamType::RTSP,
            CameraType::Network => StreamType::RTSP,
            CameraType::File => StreamType::RTSP,
            CameraType::TestSource => StreamType::TestSource,
        };

        // Create stream source from camera info
        let source = StreamSource {
            stream_type,
            uri: camera.device_path.clone(),
            name: camera.name.clone(),
            description: camera.description.clone(),
        };

        // Add stream to manager
        match self.stream_manager.add_stream(source) {
            Ok(stream_id) => {
                // Update camera with stream ID
                camera.stream_id = Some(stream_id.clone());
                Ok(stream_id)
            }
            Err(e) => {
                // If there's an error with the primary method (e.g., no camera),
                // log the error and try a fallback
                eprintln!("Failed to start camera stream: {}", e);

                // Return the original error
                Err(e)
            }
        }
    }

    pub fn stop_camera_stream(&mut self, camera_id: &str) -> Result<()> {
        let camera = self
            .cameras
            .get_mut(camera_id)
            .ok_or_else(|| anyhow!("Camera not found: {}", camera_id))?;

        if let Some(stream_id) = &camera.stream_id {
            self.stream_manager.remove_stream(stream_id)?;
            camera.stream_id = None;
            Ok(())
        } else {
            Err(anyhow!("Camera is not streaming"))
        }
    }

    pub fn list_cameras(&self) -> Vec<&Camera> {
        self.cameras.values().collect()
    }

    pub fn get_camera(&self, camera_id: &str) -> Option<&Camera> {
        self.cameras.get(camera_id)
    }
}

