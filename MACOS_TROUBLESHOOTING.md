Implement GStreamer-Based Recording Using stream_manager in Rust
Goal:
Implement scalable recording functionality in Rust using GStreamer, integrated with the stream_manager module. Video segments from RTSP IP camera streams must be saved as seekable MP4 files, organized for fast playback and long-term reliability. The system must also manage storage limits automatically via configurable cleanup policies.

✅ Core Requirements
1. Input Stream Handling (via stream_manager)
Use the existing stream_manager module to manage stream lifecycle.

All recording actions (start, stop, reconnect) must be initiated and controlled via this module.

Each stream corresponds to a camera_id & stream_id.

2. GStreamer Pipeline Design
Create pipelines like:

nginx
Copy
Edit
rtspsrc ! rtph264depay ! h264parse ! splitmuxsink
Use splitmuxsink to segment into MP4 files:

Use fragmented or faststart MP4 format

Segment duration should be configurable (e.g., default 2–5 minutes)

File naming template:
/recordings/{camera_id}/%Y/%m/%d/%H/%M/%S.mp4

Ensure proper keyframe alignment for efficient segmenting and seeking.

3. File Organization
Store recordings in the following structure:
/recordings/<camera_id>/<year>/<month>/<day>/<hour>/cam<id>_<timestamp>.mp4

Automatically create folders as needed.

Use zero-padded timestamps for filename ordering.

4. Metadata Persistence (PostgreSQL via sqlx)
Insert a record per video segment:

camera_id, start_time, end_time, file_path, event_type

Index on (camera_id, start_time) for fast filtering and retrieval.
Index on (stream_id, start_time) for fast filtering and retrieval.

Ensure DB transactions align with file creation — if the file fails, no DB entry should be created.

5. Event-Driven Recording Support
Enable recording triggers:

Always-on (continuous)

Event-based (e.g., from motion detection, external alarm)

Must be callable through stream_manager or control API.

6. Playback Compatibility
Ensure all MP4 segments:

Are seekable via moov atom at the start

Play reliably in browsers and standard players

Make segments available via HTTP/streaming APIs.

7. Storage Cleanup (NEW)
Introduce a configurable cleanup policy:

Option 1: Max retention age per camera (e.g., delete after 30 days)

Option 2: Max storage usage (e.g., 80% of disk triggers oldest segment deletion)

Cleanup runs on a schedule (e.g., every hour):

Scans the database for eligible files

Deletes both DB records and corresponding files

Logs all deletions (for audit)

Sample config:

toml
Copy
Edit
[storage.cleanup]
enabled = true
max_retention_days = 30
max_disk_usage_percent = 80
check_interval_secs = 3600
8. Health Monitoring
Implement pipeline health tracking:

Drop rate, state changes, errors

Log and optionally expose via HTTP API (/status/recording/<camera_id>)

9. Control API
Add endpoints (REST):
POST /recording/start/{camera_id}/{stream_id}
POST /recording/stop/{camera_id}/{stream_id}
GET /recording/status/{camera_id}/{stream_id}
DELETE /recording/prune/{camera_id}/{stream_id}?older_than_days=30 (manual cleanup)

// use primary stream as default for these routes...
POST /recording/start/{camera_id} 
POST /recording/stop/{camera_id}
GET /recording/status/{camera_id}
DELETE /recording/prune/{camera_id}?older_than_days=30 (manual cleanup)


10. Update the front-end to have the necessary pages to create recording schedules, view recordings status and trigger recordings
