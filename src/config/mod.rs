use std::path::PathBuf;
use std::fs;
use anyhow::Result;
use log::info;

// Simple configuration management
// In a real application, this would be more comprehensive

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub recording_dir: PathBuf,
    pub log_level: String,
    pub api_port: u16,
    pub websocket_port: u16,
    pub camera_settings: CameraSettings,
}

#[derive(Debug, Clone)]
pub struct CameraSettings {
    pub default_framerate: u32,
    pub default_resolution: (u32, u32),
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("/tmp/g-streamer/data"),
            recording_dir: PathBuf::from("/tmp/g-streamer/recordings"),
            log_level: "info".to_string(),
            api_port: 8080,
            websocket_port: 8081,
            camera_settings: CameraSettings {
                default_framerate: 30,
                default_resolution: (640, 480),
            },
        }
    }
}

impl AppConfig {
    pub fn load_from_file(_path: &str) -> Result<Self> {
        // In a real application, this would parse a config file
        let config = Self::default();
        
        // Create directories if they don't exist
        fs::create_dir_all(&config.data_dir)?;
        fs::create_dir_all(&config.recording_dir)?;
        
        Ok(config)
    }
    
    pub fn get_recording_path(&self, recording_id: &str) -> PathBuf {
        self.recording_dir.join(format!("{}.mp4", recording_id))
    }
}

pub fn setup_config() -> Result<AppConfig> {
    // Look for config file in standard locations
    let config_paths = vec![
        "config.toml",
        "/etc/g-streamer/config.toml",
    ];
    
    for path in config_paths {
        if let Ok(config) = AppConfig::load_from_file(path) {
            info!("Loaded configuration from {}", path);
            return Ok(config);
        }
    }
    
    // Fall back to default config
    info!("Using default configuration");
    Ok(AppConfig::default())
}