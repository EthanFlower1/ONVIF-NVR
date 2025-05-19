
CREATE TABLE IF NOT EXISTS streams (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    stream_type VARCHAR(50) NOT NULL, -- RTSP, HLS, MJPEG, etc.
    url VARCHAR(1024) NOT NULL,
    -- Stream specific details
    resolution VARCHAR(50), -- e.g. "1920x1080"
    width INTEGER,
    height INTEGER,
    codec VARCHAR(50), -- e.g. "H.264", "H.265", "MJPEG"
    profile VARCHAR(50), -- e.g. "Baseline", "Main", "High" for H.264
    level VARCHAR(20), -- e.g. "4.1", "5.0" for H.264/H.265
    framerate INTEGER,
    bitrate INTEGER, -- in kbps
    variable_bitrate BOOLEAN DEFAULT true,
    keyframe_interval INTEGER, -- in frames or seconds
    quality_level VARCHAR(50), -- e.g. "high", "medium", "low"
    transport_protocol VARCHAR(50), -- e.g. "UDP", "TCP", "HTTP"
    authentication_required BOOLEAN DEFAULT true,
    is_primary BOOLEAN DEFAULT false,
    is_audio_enabled BOOLEAN DEFAULT false,
    audio_codec VARCHAR(50), -- e.g. "AAC", "G.711"
    audio_bitrate INTEGER, -- in kbps
    audio_channels INTEGER DEFAULT 1,
    audio_sample_rate INTEGER, -- in Hz
    is_active BOOLEAN DEFAULT true,
    last_connected_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

