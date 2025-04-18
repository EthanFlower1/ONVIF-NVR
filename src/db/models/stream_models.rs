use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stream Type Enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum StreamType {
    Rtsp,
    Hls,
    Mjpeg,
    Webrtc,
    Srt,
    Rtmp,
    RtmpHds,
    Dash,
    Other(String),
}

impl ToString for StreamType {
    fn to_string(&self) -> String {
        match self {
            StreamType::Rtsp => "RTSP".to_string(),
            StreamType::Hls => "HLS".to_string(),
            StreamType::Mjpeg => "MJPEG".to_string(),
            StreamType::Webrtc => "WEBRTC".to_string(),
            StreamType::Srt => "SRT".to_string(),
            StreamType::Rtmp => "RTMP".to_string(),
            StreamType::RtmpHds => "RTMPHDS".to_string(),
            StreamType::Dash => "DASH".to_string(),
            StreamType::Other(s) => s.clone(),
        }
    }
}

impl From<String> for StreamType {
    fn from(s: String) -> Self {
        match s.to_uppercase().as_str() {
            "RTSP" => StreamType::Rtsp,
            "HLS" => StreamType::Hls,
            "MJPEG" => StreamType::Mjpeg,
            "WEBRTC" => StreamType::Webrtc,
            "SRT" => StreamType::Srt,
            "RTMP" => StreamType::Rtmp,
            "RTMPHDS" => StreamType::RtmpHds,
            "DASH" => StreamType::Dash,
            _ => StreamType::Other(s),
        }
    }
}

/// Stream model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub name: String,
    pub stream_type: StreamType,
    pub url: String,
    pub resolution: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub level: Option<String>,
    pub framerate: Option<i32>,
    pub bitrate: Option<i32>,
    pub variable_bitrate: Option<bool>,
    pub keyframe_interval: Option<i32>,
    pub quality_level: Option<String>,
    pub transport_protocol: Option<String>,
    pub authentication_required: Option<bool>,
    pub is_primary: Option<bool>,
    pub is_audio_enabled: Option<bool>,
    pub audio_codec: Option<String>,
    pub audio_bitrate: Option<i32>,
    pub audio_channels: Option<i32>,
    pub audio_sample_rate: Option<i32>,
    pub is_active: Option<bool>,
    pub last_connected_at: Option<DateTime<Utc>>,
    pub multicast_address: Option<String>,
    pub multicast_port: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Stream Reference model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamReference {
    pub id: Uuid,
    pub camera_id: Uuid,
    pub stream_id: Uuid,
    pub reference_type: String,
    pub display_order: Option<i32>,
    pub is_default: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
