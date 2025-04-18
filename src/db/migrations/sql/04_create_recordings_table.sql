-- Recordings table for storing metadata about recorded videos
CREATE TABLE IF NOT EXISTS recordings (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    schedule_id UUID REFERENCES recording_schedules(id),
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ,
    file_path TEXT NOT NULL,
    file_size BIGINT NOT NULL DEFAULT 0,
    duration BIGINT NOT NULL DEFAULT 0,
    format VARCHAR(50) NOT NULL,
    resolution VARCHAR(50) NOT NULL,
    fps INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    metadata JSONB
);

