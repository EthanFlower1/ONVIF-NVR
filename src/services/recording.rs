use crate::stream_manager::{BranchConfig, BranchId, BranchType, StreamId, StreamManager};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub struct RecordingRequest {
    pub stream_id: StreamId,
    pub output_path: Option<String>,
    pub duration: Option<u64>, // in seconds, None means record until explicitly stopped
    pub quality: Option<String>, // "low", "medium", "high"
}

pub struct Recording {
    pub id: String,
    pub stream_id: StreamId,
    pub branch_id: BranchId,
    pub output_path: String,
    pub start_time: std::time::SystemTime,
    pub duration: Option<u64>,
    pub status: RecordingStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingStatus {
    Active,
    Completed,
    Failed,
}

pub struct RecordingService {
    stream_manager: Arc<StreamManager>,
    recordings: HashMap<String, Recording>,
}

impl RecordingService {
    pub fn new(stream_manager: Arc<StreamManager>) -> Self {
        Self {
            stream_manager,
            recordings: HashMap::new(),
        }
    }

    pub fn start_recording(&mut self, request: RecordingRequest) -> Result<String> {
        // Generate recording ID
        let recording_id = Uuid::new_v4().to_string();

        // Determine output path, using defaults if not provided
        let output_dir = request.output_path.unwrap_or_else(|| "/tmp".to_string());
        let output_path = format!("{}/recording_{}.mp4", output_dir, recording_id);

        // Set encoding options based on quality
        let mut options = HashMap::new();

        match request.quality.as_deref() {
            Some("low") => {
                options.insert("bitrate".to_string(), "500".to_string());
                options.insert("tune".to_string(), "zerolatency".to_string());
            }
            Some("medium") => {
                options.insert("bitrate".to_string(), "1000".to_string());
                options.insert("tune".to_string(), "film".to_string());
            }
            Some("high") => {
                options.insert("bitrate".to_string(), "2000".to_string());
                options.insert("tune".to_string(), "film".to_string());
                options.insert("preset".to_string(), "medium".to_string());
            }
            _ => {
                options.insert("bitrate".to_string(), "1000".to_string());
                options.insert("tune".to_string(), "zerolatency".to_string());
            }
        }

        // Create branch config
        let config = BranchConfig {
            branch_type: BranchType::Recording,
            output_path: Some(output_path.clone()),
            options,
        };

        // Add branch to stream
        let branch_id = self.stream_manager.add_branch(&request.stream_id, config)?;

        // Save stream_id for later use
        let stream_id = request.stream_id.clone();

        // Create recording object
        let recording = Recording {
            id: recording_id.clone(),
            stream_id,
            branch_id,
            output_path,
            start_time: std::time::SystemTime::now(),
            duration: request.duration,
            status: RecordingStatus::Active,
        };

        // Store recording
        self.recordings.insert(recording_id.clone(), recording);

        // If there's a duration, schedule stopping
        if let Some(_duration) = request.duration {
            // In a real implementation, we would schedule stopping after duration
            // let recording_id_clone = recording_id.clone();
            // let stream_manager_clone = self.stream_manager.clone();
            // let stream_id_clone = stream_id.clone();

            // In a real app, we'd use tokio to schedule this
            // tokio::spawn(async move {
            //     tokio::time::sleep(tokio::time::Duration::from_secs(duration)).await;
            //     // Find branch ID
            //     if let Some(recording) = self.recordings.get(&recording_id_clone) {
            //         if recording.status == RecordingStatus::Active {
            //             let _ = stream_manager_clone.remove_branch(&stream_id_clone, &recording.branch_id);
            //             // Update status
            //             recording.status = RecordingStatus::Completed;
            //         }
            //     }
            // });
        }

        Ok(recording_id)
    }

    pub fn stop_recording(&mut self, recording_id: &str) -> Result<()> {
        let recording = self
            .recordings
            .get_mut(recording_id)
            .ok_or_else(|| anyhow!("Recording not found: {}", recording_id))?;

        if recording.status != RecordingStatus::Active {
            return Err(anyhow!("Recording is not active"));
        }

        // Remove branch from stream
        self.stream_manager
            .remove_branch(&recording.stream_id, &recording.branch_id)?;

        // Update status
        recording.status = RecordingStatus::Completed;

        Ok(())
    }

    pub fn get_recording(&self, recording_id: &str) -> Option<&Recording> {
        self.recordings.get(recording_id)
    }

    pub fn list_recordings(&self) -> Vec<&Recording> {
        self.recordings.values().collect()
    }

    pub fn list_active_recordings(&self) -> Vec<&Recording> {
        self.recordings
            .values()
            .filter(|r| r.status == RecordingStatus::Active)
            .collect()
    }
}

