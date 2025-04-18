use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stream Type Enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    Unknown,
}
// Implement PostgreSQL type conversion for your enum
impl sqlx::Type<sqlx::Postgres> for StreamType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("text")
    }
}

// Implement Decode for converting from DB to Rust
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for StreamType {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let text = <String as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        match text.to_uppercase().as_str() {
            "RTSP" => Ok(StreamType::Rtsp),
            "HLS" => Ok(StreamType::Hls),
            "MJPEG" => Ok(StreamType::Mjpeg),
            "WEBRTC" => Ok(StreamType::Webrtc),
            "SRT" => Ok(StreamType::Srt),
            "RTMP" => Ok(StreamType::Rtmp),
            "RTMPHDS" => Ok(StreamType::RtmpHds),
            "DASH" => Ok(StreamType::Dash),
            _ => Ok(StreamType::Unknown),
        }
    }
}

// Implement Encode for converting from Rust to DB
// Implement Encode for converting from Rust to DB
impl sqlx::Encode<'_, sqlx::Postgres> for StreamType {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let s = match self {
            StreamType::Rtsp => "RTSP",
            StreamType::Hls => "HLS",
            StreamType::Mjpeg => "MJPEG",
            StreamType::Webrtc => "WEBRTC",
            StreamType::Srt => "SRT",
            StreamType::Rtmp => "RTMP",
            StreamType::RtmpHds => "RTMPHDS",
            StreamType::Dash => "DASH",
            StreamType::Unknown => "UNKNOWN",
        };
        <&str as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&s, buf)
    }
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
            StreamType::Unknown => "UNKNOWN".to_string(),
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
            _ => StreamType::Unknown,
        }
    }
}

/// Stream model
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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
