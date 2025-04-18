CREATE TABLE IF NOT EXISTS cameras (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    model VARCHAR(255),
    manufacturer VARCHAR(255),
    ip_address VARCHAR(45) NOT NULL,
    username VARCHAR(255),
    password VARCHAR(255),
    onvif_endpoint VARCHAR(255),
    status VARCHAR(50) NOT NULL,
    primary_stream_id UUID, -- Reference to primary stream (populated after streams are created)
    sub_stream_id UUID, -- Reference to secondary/sub stream (typically lower resolution)

    -- Extended camera details
    firmware_version VARCHAR(100),
    serial_number VARCHAR(100),
    hardware_id VARCHAR(100),
    mac_address VARCHAR(20),
    ptz_supported BOOLEAN,
    audio_supported BOOLEAN,
    analytics_supported BOOLEAN,

    -- Events support
    events_supported JSONB, -- Array of supported event types like motion, tamper, etc.
    event_notification_endpoint VARCHAR(255), -- Where camera sends event notifications

    -- Storage information
    has_local_storage BOOLEAN DEFAULT false,
    storage_type VARCHAR(100), -- SD card, internal drive, etc.
    storage_capacity_gb INTEGER,
    storage_used_gb INTEGER,
    retention_days INTEGER,
    recording_mode VARCHAR(50), -- continuous, motion, scheduled, etc.

    -- Analytics information
    analytics_capabilities JSONB, -- Detailed analytics capabilities
    ai_processor_type VARCHAR(100), -- GPU, TPU, VPU, etc.
    ai_processor_model VARCHAR(100),
    object_detection_supported BOOLEAN DEFAULT false,
    face_detection_supported BOOLEAN DEFAULT false,
    license_plate_recognition_supported BOOLEAN DEFAULT false,
    person_tracking_supported BOOLEAN DEFAULT false,
    line_crossing_supported BOOLEAN DEFAULT false,
    zone_intrusion_supported BOOLEAN DEFAULT false,
    object_classification_supported BOOLEAN DEFAULT false,
    behavior_analysis_supported BOOLEAN DEFAULT false,

    -- Original fields
    capabilities JSONB,
    profiles JSONB,
    last_updated TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    created_by UUID NOT NULL REFERENCES users(id)
);

-- 2. Create indices for the cameras table
CREATE INDEX IF NOT EXISTS idx_cameras_name ON cameras(name);
CREATE INDEX IF NOT EXISTS idx_cameras_ip ON cameras(ip_address);
CREATE INDEX IF NOT EXISTS idx_cameras_status ON cameras(status);
CREATE INDEX IF NOT EXISTS idx_cameras_ptz ON cameras(ptz_supported);
CREATE INDEX IF NOT EXISTS idx_cameras_last_updated ON cameras(last_updated);

-- Additional indexes for new fields
CREATE INDEX IF NOT EXISTS idx_cameras_analytics_supported ON cameras(analytics_supported);
CREATE INDEX IF NOT EXISTS idx_cameras_has_local_storage ON cameras(has_local_storage);
CREATE INDEX IF NOT EXISTS idx_cameras_object_detection ON cameras(object_detection_supported);
CREATE INDEX IF NOT EXISTS idx_cameras_recording_mode ON cameras(recording_mode);
