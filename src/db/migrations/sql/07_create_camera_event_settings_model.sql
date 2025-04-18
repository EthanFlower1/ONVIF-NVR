-- Camera event settings table for storing configuration of event monitoring and recording
CREATE TABLE IF NOT EXISTS event_settings (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    event_types JSONB NOT NULL, -- Array of event types to monitor
    event_topic_expressions JSONB NOT NULL, -- Array of ONVIF topic expressions
    trigger_recording BOOLEAN NOT NULL DEFAULT FALSE, -- Whether to trigger recording on events
    recording_duration BIGINT NOT NULL DEFAULT 60, -- Duration to record in seconds when event triggered
    recording_quality recording_quality NOT NULL DEFAULT 'medium',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    created_by UUID NOT NULL REFERENCES users(id)
);
