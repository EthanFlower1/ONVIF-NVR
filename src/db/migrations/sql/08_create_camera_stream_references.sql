
CREATE TABLE IF NOT EXISTS stream_references (
    id UUID PRIMARY KEY,
    camera_id UUID NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    reference_type VARCHAR(50) NOT NULL, -- 'primary', 'sub', 'tertiary', 'lowres', 'mobile', 'analytics', etc.
    display_order INTEGER DEFAULT 0, -- For ordering streams in UI
    is_default BOOLEAN DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Enforce uniqueness for reference_type per camera
    UNIQUE(camera_id, reference_type),
    -- Ensure a stream isn't referenced multiple times by the same camera
    UNIQUE(camera_id, stream_id)
);

