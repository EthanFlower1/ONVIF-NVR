-- 1. Create indices for the users table
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);


-- 2. Create indices for the cameras table
CREATE INDEX IF NOT EXISTS idx_cameras_name ON cameras(name);
CREATE INDEX IF NOT EXISTS idx_cameras_ip ON cameras(ip_address);
CREATE INDEX IF NOT EXISTS idx_cameras_status ON cameras(status);
CREATE INDEX IF NOT EXISTS idx_cameras_ptz ON cameras(ptz_supported);
CREATE INDEX IF NOT EXISTS idx_cameras_last_updated ON cameras(last_updated);
CREATE INDEX IF NOT EXISTS idx_cameras_analytics_supported ON cameras(analytics_supported);
CREATE INDEX IF NOT EXISTS idx_cameras_has_local_storage ON cameras(has_local_storage);
CREATE INDEX IF NOT EXISTS idx_cameras_object_detection ON cameras(object_detection_supported);
CREATE INDEX IF NOT EXISTS idx_cameras_recording_mode ON cameras(recording_mode);

-- 3. Index for searching schedules by camera
CREATE INDEX IF NOT EXISTS idx_schedules_camera ON recording_schedules(camera_id);
CREATE INDEX IF NOT EXISTS idx_schedules_enabled ON recording_schedules(enabled);
CREATE INDEX IF NOT EXISTS idx_schedules_name ON recording_schedules(name);

-- 4. Index for searching recordings by camera
CREATE INDEX IF NOT EXISTS idx_recordings_camera ON recordings(camera_id);
CREATE INDEX IF NOT EXISTS idx_recordings_schedule ON recordings(schedule_id);
CREATE INDEX IF NOT EXISTS idx_recordings_time_range ON recordings(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_recordings_start_time ON recordings(start_time DESC);
CREATE INDEX IF NOT EXISTS idx_recordings_timerange ON recordings USING btree (start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_recordings_camera_id ON recordings(camera_id);
CREATE INDEX IF NOT EXISTS idx_recordings_stream_id ON recordings(stream_id);
CREATE INDEX IF NOT EXISTS idx_recordings_camera_start_time ON recordings(camera_id, start_time);
CREATE INDEX IF NOT EXISTS idx_recordings_stream_start_time ON recordings(stream_id, start_time);
CREATE INDEX IF NOT EXISTS idx_recordings_start_time ON recordings(start_time);
CREATE INDEX IF NOT EXISTS idx_recordings_event_type ON recordings(event_type);
CREATE INDEX IF NOT EXISTS idx_recordings_parent_id ON recordings(parent_recording_id);
CREATE INDEX IF NOT EXISTS idx_recordings_segment_id ON recordings(parent_recording_id, segment_id);


-- 5. Create indices for the camera_events table
CREATE INDEX IF NOT EXISTS idx_events_camera_id ON events(camera_id);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_start_time ON events(start_time);
CREATE INDEX IF NOT EXISTS idx_events_acknowledged ON events(acknowledged);
CREATE INDEX IF NOT EXISTS idx_events_severity ON events(severity);

--6.  Create indices for the camera_stream_references table
CREATE INDEX IF NOT EXISTS idx_streams_camera_id ON streams(camera_id);
CREATE INDEX IF NOT EXISTS idx_streams_type ON streams(stream_type);
CREATE INDEX IF NOT EXISTS idx_streams_active ON streams(is_active);
CREATE INDEX IF NOT EXISTS idx_streams_primary ON streams(is_primary);

--7.  Create indices for the camera_event_settings table
CREATE INDEX IF NOT EXISTS idx_event_settings_camera ON event_settings(camera_id);
CREATE INDEX IF NOT EXISTS idx_event_settings_enabled ON event_settings(enabled);

--8.  Create indices for the camera_stream_references table
CREATE INDEX IF NOT EXISTS idx_stream_refs_camera_id ON stream_references(camera_id);
CREATE INDEX IF NOT EXISTS idx_stream_refs_stream_id ON stream_references(stream_id);
CREATE INDEX IF NOT EXISTS idx_stream_refs_type ON stream_references(reference_type);
CREATE INDEX IF NOT EXISTS idx_stream_refs_default ON stream_references(is_default);
