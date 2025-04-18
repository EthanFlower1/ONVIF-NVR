-- Recording schedules table
CREATE TABLE IF NOT EXISTS recording_schedules (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    days_of_week INTEGER[] NOT NULL, -- Array of integers 0-6 for Sunday-Saturday
    start_time VARCHAR(5) NOT NULL, -- "HH:MM" format
    end_time VARCHAR(5) NOT NULL, -- "HH:MM" format
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    created_by UUID NOT NULL REFERENCES users(id),
    retention_days INTEGER NOT NULL DEFAULT 30,
    recording_quality recording_quality NOT NULL
);

