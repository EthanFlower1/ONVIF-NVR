-- Recordings table for storing metadata about recorded videos
CREATE TABLE IF NOT EXISTS recordings (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    schedule_id UUID REFERENCES recording_schedules(id),
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ,
    file_path TEXT NOT NULL,
    file_size BIGINT NOT NULL DEFAULT 0,
    duration BIGINT NOT NULL DEFAULT 0,
    format VARCHAR(50) NOT NULL,
    resolution VARCHAR(50) NOT NULL,
    fps INTEGER NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'continuous',
    created_at TIMESTAMPTZ NOT NULL,
    metadata JSONB
);

-- Create indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_recordings_camera_id ON recordings(camera_id);
CREATE INDEX IF NOT EXISTS idx_recordings_stream_id ON recordings(stream_id);
CREATE INDEX IF NOT EXISTS idx_recordings_camera_start_time ON recordings(camera_id, start_time);
CREATE INDEX IF NOT EXISTS idx_recordings_stream_start_time ON recordings(stream_id, start_time);
CREATE INDEX IF NOT EXISTS idx_recordings_start_time ON recordings(start_time);
CREATE INDEX IF NOT EXISTS idx_recordings_event_type ON recordings(event_type);

