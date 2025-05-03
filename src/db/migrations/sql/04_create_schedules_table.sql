-- Recording schedules table
CREATE TABLE IF NOT EXISTS recording_schedules (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    days_of_week INTEGER[] NOT NULL, -- Array of integers 0-6 for Sunday-Saturday
    start_time VARCHAR(5) NOT NULL, -- "HH:MM" format
    end_time VARCHAR(5) NOT NULL, -- "HH:MM" format
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    retention_days INTEGER NOT NULL DEFAULT 30
);

-- Add event-based fields to recording_schedules table
ALTER TABLE recording_schedules 
ADD COLUMN IF NOT EXISTS record_on_motion BOOLEAN NOT NULL DEFAULT FALSE,
ADD COLUMN IF NOT EXISTS record_on_audio BOOLEAN NOT NULL DEFAULT FALSE,
ADD COLUMN IF NOT EXISTS record_on_analytics BOOLEAN NOT NULL DEFAULT FALSE,
ADD COLUMN IF NOT EXISTS record_on_external BOOLEAN NOT NULL DEFAULT FALSE,
ADD COLUMN IF NOT EXISTS continuous_recording BOOLEAN NOT NULL DEFAULT TRUE;
