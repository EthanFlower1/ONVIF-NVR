CREATE TABLE IF NOT EXISTS events (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    event_type VARCHAR(100) NOT NULL, -- motion, tamper, line_crossing, etc.
    severity VARCHAR(50), -- info, warning, critical, etc.
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ,
    duration INTEGER, -- in seconds
    confidence FLOAT, -- 0.0 to 1.0 for detection confidence
    metadata JSONB, -- Additional event data (coordinates, objects detected, etc.)
    thumbnail_path VARCHAR(1024),
    video_clip_path VARCHAR(1024),
    acknowledged BOOLEAN DEFAULT false,
    acknowledged_by UUID REFERENCES users(id),
    acknowledged_at TIMESTAMPTZ,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

