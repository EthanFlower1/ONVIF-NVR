/// Generate a complete HLS playlist with segments for all recordings of a camera
async fn generate_camera_hls(
    camera_id: &Uuid,
    recordings: &[Recording],
    output_dir: &FilePath,
) -> Result<(), anyhow::Error> {
    info!("Generating complete HLS playlist for camera: {}", camera_id);
    
    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }
    
    // Determine the path for the main playlist
    let playlist_path = output_dir.join("playlist.m3u8");
    
    // If we don't have any recordings, return an error
    if recordings.is_empty() {
        return Err(anyhow::anyhow!("No recordings found for camera {}", camera_id));
    }
    
    // First check if we have TS-segmented recordings that we can use directly
    let segmented_recordings: Vec<&Recording> = recordings.iter()
        .filter(|r| {
            // Check if this recording has TS segments based on metadata
            if let Some(metadata) = &r.metadata {
                if let Ok(metadata_obj) = serde_json::from_value::<serde_json::Value>(metadata.clone()) {
                    if let Some(obj) = metadata_obj.as_object() {
                        if let Some(hls_info) = obj.get("hls") {
                            if let Some(hls_obj) = hls_info.as_object() {
                                if let Some(format) = hls_obj.get("format") {
                                    return format.as_str() == Some("ts");
                                }
                            }
                        }
                    }
                }
            }
            false
        })
        .collect();
        
    if !segmented_recordings.is_empty() {
        // We have TS-segmented recordings - we can use them directly for HLS
        info!("Found {} TS-segmented recordings for camera {}", segmented_recordings.len(), camera_id);
        
        // Create the playlist content
        let mut playlist_content = String::new();
        
        // Add HLS header
        playlist_content.push_str("#EXTM3U\n");
        playlist_content.push_str("#EXT-X-VERSION:3\n");
        playlist_content.push_str("#EXT-X-TARGETDURATION:5\n"); // Assuming ~5 second segments
        playlist_content.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
        playlist_content.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
        
        // Sort recordings chronologically
        let mut sorted_recordings = segmented_recordings.clone();
        sorted_recordings.sort_by(|a, b| a.start_time.cmp(&b.start_time));
        
        // Process each parent recording
        for recording in sorted_recordings {
            // Add a discontinuity marker between recordings
            playlist_content.push_str("#EXT-X-DISCONTINUITY\n");
            
            // Find all segments for this recording
            let segments_query = crate::db::models::recording_models::RecordingSearchQuery {
                camera_ids: None,
                stream_ids: None,
                start_time: None,
                end_time: None,
                event_types: None,
                schedule_id: None,
                min_duration: None,
                segment_id: None,
                parent_recording_id: Some(recording.id),
                is_segment: Some(true),
                limit: None,
                offset: None,
            };
            
            // Query for segments using a temporary connection
            // This is a simplified approach - in a real implementation you'd use a repository
            let db_pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/g_streamer".to_string())).await?;
            let recordings_repo = crate::db::repositories::recordings::RecordingsRepository::new(Arc::new(db_pool));
            
            let segments = match recordings_repo.search(&segments_query).await {
                Ok(segs) => segs,
                Err(e) => {
                    error!("Failed to query segments for recording {}: {}", recording.id, e);
                    continue; // Skip this recording but continue with others
                }
            };
            
            if segments.is_empty() {
                warn!("No segments found for recording {}", recording.id);
                continue;
            }
            
            // Sort segments by segment_id
            let mut sorted_segments = segments.clone();
            sorted_segments.sort_by(|a, b| a.segment_id.unwrap_or(0).cmp(&b.segment_id.unwrap_or(0)));
            
            // Get segment duration from metadata if available
            let segment_duration = if let Some(metadata) = &recording.metadata {
                if let Ok(metadata_obj) = serde_json::from_value::<serde_json::Value>(metadata.clone()) {
                    if let Some(obj) = metadata_obj.as_object() {
                        if let Some(hls_info) = obj.get("hls") {
                            if let Some(hls_obj) = hls_info.as_object() {
                                if let Some(duration) = hls_obj.get("segment_duration_seconds") {
                                    if let Some(duration_num) = duration.as_i64() {
                                        duration_num as f64
                                    } else {
                                        4.0 // Default to 4 seconds
                                    }
                                } else {
                                    4.0
                                }
                            } else {
                                4.0
                            }
                        } else {
                            4.0
                        }
                    } else {
                        4.0
                    }
                } else {
                    4.0
                }
            } else {
                4.0
            };
            
            // Add each segment to the playlist
            for (index, segment) in sorted_segments.iter().enumerate() {
                if !segment.file_path.exists() {
                    warn!("Segment file does not exist: {:?}", segment.file_path);
                    continue;
                }
                
                let segment_file_name = match segment.file_path.file_name() {
                    Some(name) => name.to_string_lossy().to_string(),
                    None => continue, // Skip segments with invalid filenames
                };
                
                // Get actual segment duration from metadata if available
                let mut actual_duration = segment_duration;
                if let Some(metadata) = &segment.metadata {
                    if let Ok(metadata_obj) = serde_json::from_value::<serde_json::Value>(metadata.clone()) {
                        if let Some(obj) = metadata_obj.as_object() {
                            if let Some(hls_info) = obj.get("hls") {
                                if let Some(hls_obj) = hls_info.as_object() {
                                    if let Some(duration) = hls_obj.get("actual_duration_seconds") {
                                        if let Some(duration_num) = duration.as_u64() {
                                            actual_duration = duration_num as f64;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Add segment info to the playlist
                playlist_content.push_str(&format!("#EXTINF:{:.6},\n", actual_duration));
                
                // Create segments directory in output_dir
                let segments_dir = output_dir.join("segments");
                if !segments_dir.exists() {
                    std::fs::create_dir_all(&segments_dir)?;
                }
                
                // Copy the segment file to the output directory
                let dest_path = segments_dir.join(&segment_file_name);
                if !dest_path.exists() {
                    if let Err(e) = std::fs::copy(&segment.file_path, &dest_path) {
                        warn!("Failed to copy segment file: {}", e);
                        // Continue with original path
                        playlist_content.push_str(&segment.file_path.to_string_lossy());
                    } else {
                        // Use the copied path
                        playlist_content.push_str(&format!("segments/{}", segment_file_name));
                    }
                } else {
                    // File already exists in destination, use that path
                    playlist_content.push_str(&format!("segments/{}", segment_file_name));
                }
                
                playlist_content.push_str("\n");
            }
        }
        
        // Add end marker
        playlist_content.push_str("#EXT-X-ENDLIST\n");
        
        // Write the playlist file
        std::fs::write(&playlist_path, playlist_content)?;
    } else {
        // No TS-segmented recordings, use FFmpeg to generate HLS playlists
        info!("No TS-segmented recordings found, using FFmpeg to generate HLS for camera {}", camera_id);
        
        let segments_pattern = output_dir.join("segment%03d.ts");
        
        // Create a temporary file to list all recording files for FFmpeg
        let input_list_path = output_dir.join("input_list.txt");
        let mut input_list_content = String::new();
        
        // Sort recordings chronologically
        let mut sorted_recordings = recordings.to_vec();
        sorted_recordings.sort_by(|a, b| a.start_time.cmp(&b.start_time));
        
        // Filter for recordings that actually exist
        let valid_recordings: Vec<_> = sorted_recordings.iter()
            .filter(|r| r.file_path.exists())
            .collect();
            
        if valid_recordings.is_empty() {
            return Err(anyhow::anyhow!("No valid recording files found for camera {}", camera_id));
        }
        
        // Build the input list file with all valid recordings
        for recording in valid_recordings {
            input_list_content.push_str(&format!("file '{}'\n", recording.file_path.to_string_lossy().replace("'", "\\'")));
        }
        
        // Write the input list file
        std::fs::write(&input_list_path, input_list_content)?;
        
        // Use FFmpeg to concatenate all recordings and create HLS playlist
        let status = Command::new("ffmpeg")
            .arg("-f")
            .arg("concat")
            .arg("-safe")
            .arg("0")  // Allow absolute paths
            .arg("-i")
            .arg(&input_list_path) // Input file list
            // Try to copy codecs if possible for better performance
            .arg("-c")
            .arg("copy")
            // HLS output settings
            .arg("-f")
            .arg("hls") // Output format is HLS
            .arg("-hls_time")
            .arg("4") // 4-second segments
            .arg("-hls_list_size")
            .arg("0") // Keep all segments in the playlist
            .arg("-hls_segment_type")
            .arg("mpegts") // Use MPEG-TS for segments
            .arg("-hls_segment_filename")
            .arg(&segments_pattern) // Pattern for segment files
            // Output path for the playlist
            .arg(&playlist_path)
            .stderr(Stdio::inherit())
            .status()?;
            
        if !status.success() {
            error!("Failed to generate HLS with concat+copy, trying with re-encoding");
            
            // If direct concatenation fails, try with re-encoding
            let fallback_status = Command::new("ffmpeg")
                .arg("-f")
                .arg("concat")
                .arg("-safe")
                .arg("0")  // Allow absolute paths
                .arg("-i")
                .arg(&input_list_path) // Input file list
                // Explicit transcoding settings
                .arg("-c:v")
                .arg("libx264") // H.264 video codec
                .arg("-profile:v")
                .arg("baseline") // Use baseline profile for maximum compatibility
                .arg("-level")
                .arg("3.0")
                .arg("-preset")
                .arg("superfast") // Fast encoding at slight quality cost
                .arg("-c:a")
                .arg("aac") // AAC audio codec
                .arg("-b:a")
                .arg("128k") // 128kbps audio bitrate
                .arg("-pix_fmt")
                .arg("yuv420p") // Standard pixel format
                // HLS output settings
                .arg("-f")
                .arg("hls") // Output format is HLS
                .arg("-hls_time")
                .arg("4") // 4-second segments
                .arg("-hls_list_size")
                .arg("0") // Keep all segments in the playlist
                .arg("-hls_segment_type")
                .arg("mpegts") // Use MPEG-TS for segments
                .arg("-hls_segment_filename")
                .arg(&segments_pattern) // Pattern for segment files
                // Output path for the playlist
                .arg(&playlist_path)
                .stderr(Stdio::inherit())
                .status()?;
                
            if !fallback_status.success() {
                return Err(anyhow::anyhow!("Failed to generate HLS playlist with multiple methods"));
            }
        }
        
        // Clean up the input list file
        let _ = std::fs::remove_file(&input_list_path);
    }
    
    // Verify the playlist was created
    if !playlist_path.exists() || std::fs::metadata(&playlist_path)?.len() == 0 {
        return Err(anyhow::anyhow!("Failed to create a valid HLS playlist"));
    }

    // Create a master playlist that references the main playlist
    let master_playlist_path = output_dir.join("master.m3u8");
    let master_content = format!(
        "#EXTM3U\n\
        #EXT-X-VERSION:3\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n\
        playlist.m3u8\n"
    );
    std::fs::write(&master_playlist_path, master_content)?;

    info!("Successfully generated HLS playlists for camera {} at: {}", camera_id, output_dir.display());
    Ok(())
}

/// Generate a complete HLS playlist with segments for a single recording
async fn generate_recording_hls(
    recording: &Recording,
    output_dir: &FilePath,
) -> Result<(), anyhow::Error> {
    info!("Generating HLS playlist for recording: {}", recording.id);
    
    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        std::fs::create_dir_all(output_dir)?;
    }
    
    // Check if this is a TS-segmented recording based on metadata
    let is_ts_segmented = if let Some(metadata) = &recording.metadata {
        if let Ok(metadata_obj) = serde_json::from_value::<serde_json::Value>(metadata.clone()) {
            if let Some(obj) = metadata_obj.as_object() {
                if let Some(hls_info) = obj.get("hls") {
                    if let Some(hls_obj) = hls_info.as_object() {
                        if let Some(format) = hls_obj.get("format") {
                            format.as_str() == Some("ts")
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    
    // If this is a segmented recording, use our new direct HLS generation
    if is_ts_segmented {
        info!("Recording {} has TS segments, generating HLS directly from segments", recording.id);
        
        // Check if this recording has segments or is itself a segment
        if recording.segment_id.is_some() {
            // This is a segment, not a parent recording
            return Err(anyhow::anyhow!("This is a segment recording, not a parent recording. Cannot generate HLS directly."));
        }
        
        // Find all segments for this recording
        let segments_query = crate::db::models::recording_models::RecordingSearchQuery {
            camera_ids: None,
            stream_ids: None,
            start_time: None,
            end_time: None,
            event_types: None,
            schedule_id: None,
            min_duration: None,
            segment_id: None,
            parent_recording_id: Some(recording.id),
            is_segment: Some(true),
            limit: None,
            offset: None,
        };
        
        // Query for segments using a temporary connection
        // This is a simplified approach - in a real implementation you'd use a repository
        let db_pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/g_streamer".to_string())).await?;
        let recordings_repo = crate::db::repositories::recordings::RecordingsRepository::new(Arc::new(db_pool));
        
        let segments = match recordings_repo.search(&segments_query).await {
            Ok(segs) => segs,
            Err(e) => {
                error!("Failed to query segments for recording {}: {}", recording.id, e);
                return Err(anyhow::anyhow!("Failed to query segments for recording: {}", e));
            }
        };
        
        if segments.is_empty() {
            return Err(anyhow::anyhow!("No segments found for recording {}", recording.id));
        }
        
        // Sort segments by segment_id
        let mut sorted_segments = segments.clone();
        sorted_segments.sort_by(|a, b| a.segment_id.unwrap_or(0).cmp(&b.segment_id.unwrap_or(0)));
        
        // Create the playlist content
        let mut playlist_content = String::new();
        
        // Get segment duration from recording metadata
        let segment_duration = if let Some(metadata) = &recording.metadata {
            if let Ok(metadata_obj) = serde_json::from_value::<serde_json::Value>(metadata.clone()) {
                if let Some(obj) = metadata_obj.as_object() {
                    if let Some(hls_info) = obj.get("hls") {
                        if let Some(hls_obj) = hls_info.as_object() {
                            if let Some(duration) = hls_obj.get("segment_duration_seconds") {
                                if let Some(duration_num) = duration.as_i64() {
                                    duration_num as f64
                                } else {
                                    4.0 // Default to 4 seconds
                                }
                            } else {
                                4.0
                            }
                        } else {
                            4.0
                        }
                    } else {
                        4.0
                    }
                } else {
                    4.0
                }
            } else {
                4.0
            }
        } else {
            4.0
        };
        
        // Add HLS header
        playlist_content.push_str("#EXTM3U\n");
        playlist_content.push_str("#EXT-X-VERSION:3\n");
        playlist_content.push_str(&format!("#EXT-X-TARGETDURATION:{}\n", segment_duration.ceil() as i32));
        playlist_content.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
        playlist_content.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n");
        
        // Add segments
        for (index, segment) in sorted_segments.iter().enumerate() {
            if !segment.file_path.exists() {
                warn!("Segment file does not exist: {:?}", segment.file_path);
                continue;
            }
            
            let segment_file_name = match segment.file_path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue, // Skip segments with invalid filenames
            };
            
            // Get actual segment duration from metadata if available
            let actual_duration = segment.duration as f64 / 1000.0; // Convert to seconds
            
            // Add segment info to playlist
            playlist_content.push_str(&format!("#EXTINF:{:.6},\n", actual_duration));
            
            // Create segments directory in output_dir
            let segments_dir = output_dir.join("segments");
            if !segments_dir.exists() {
                std::fs::create_dir_all(&segments_dir)?;
            }
            
            // Copy the segment file to the output directory
            let dest_path = segments_dir.join(&segment_file_name);
            if !dest_path.exists() {
                if let Err(e) = std::fs::copy(&segment.file_path, &dest_path) {
                    warn!("Failed to copy segment file: {}", e);
                    // Use original path as fallback
                    playlist_content.push_str(&segment.file_path.to_string_lossy());
                } else {
                    // Use the copied path
                    playlist_content.push_str(&format!("segments/{}", segment_file_name));
                }
            } else {
                // File already exists in destination, use that path
                playlist_content.push_str(&format!("segments/{}", segment_file_name));
            }
            
            playlist_content.push_str("\n");
        }
        
        // Add end marker
        playlist_content.push_str("#EXT-X-ENDLIST\n");
        
        // Write the playlist file
        let playlist_path = output_dir.join("playlist.m3u8");
        std::fs::write(&playlist_path, playlist_content)?;
        
        // Create a master playlist
        let master_playlist_path = output_dir.join("master.m3u8");
        let resolution = recording.resolution.clone();
        let master_content = format!(
            "#EXTM3U\n\
            #EXT-X-VERSION:3\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION={}\n\
            playlist.m3u8\n",
            resolution
        );
        std::fs::write(&master_playlist_path, master_content)?;
        
        info!("Successfully generated HLS playlist for recording {} with {} segments", 
            recording.id, sorted_segments.len());
        
        return Ok(());
    }
    
    // Not a TS-segmented recording, use FFmpeg's HLS generation
    // Check if the file exists first
    if !recording.file_path.exists() {
        return Err(anyhow::anyhow!("Recording file does not exist: {:?}", recording.file_path));
    }
    
    // Determine the path for the main playlist and segments
    let playlist_path = output_dir.join("playlist.m3u8");
    let segments_pattern = output_dir.join("segment%03d.ts");
    
    // Use FFmpeg's direct HLS generation capabilities
    let status = Command::new("ffmpeg")
        .arg("-i")
        .arg(&recording.file_path) // Input file
        // Try to copy codecs if possible for better performance
        .arg("-c")
        .arg("copy")
        // HLS output settings
        .arg("-f")
        .arg("hls") // Output format is HLS
        .arg("-hls_time")
        .arg("4") // 4-second segments
        .arg("-hls_list_size")
        .arg("0") // Keep all segments in the playlist
        .arg("-hls_segment_type")
        .arg("mpegts") // Use MPEG-TS for segments
        .arg("-hls_segment_filename")
        .arg(&segments_pattern) // Pattern for segment files
        // Output path for the playlist
        .arg(&playlist_path)
        .stderr(Stdio::inherit())
        .status()?;
        
    if !status.success() {
        error!("Failed to generate HLS with codec copy, trying with transcoding");
        
        // If direct copy fails, try with explicit transcoding
        let fallback_status = Command::new("ffmpeg")
            .arg("-i")
            .arg(&recording.file_path) // Input file
            // Explicit transcoding settings
            .arg("-c:v")
            .arg("libx264") // H.264 video codec
            .arg("-profile:v")
            .arg("baseline") // Use baseline profile for maximum compatibility
            .arg("-level")
            .arg("3.0")
            .arg("-preset")
            .arg("superfast") // Fast encoding at slight quality cost
            .arg("-c:a")
            .arg("aac") // AAC audio codec
            .arg("-b:a")
            .arg("128k") // 128kbps audio bitrate
            .arg("-pix_fmt")
            .arg("yuv420p") // Standard pixel format
            // HLS output settings
            .arg("-f")
            .arg("hls") // Output format is HLS
            .arg("-hls_time")
            .arg("4") // 4-second segments
            .arg("-hls_list_size")
            .arg("0") // Keep all segments in the playlist
            .arg("-hls_segment_type")
            .arg("mpegts") // Use MPEG-TS for segments
            .arg("-hls_segment_filename")
            .arg(&segments_pattern) // Pattern for segment files
            // Output path for the playlist
            .arg(&playlist_path)
            .stderr(Stdio::inherit())
            .status()?;
            
        if !fallback_status.success() {
            return Err(anyhow::anyhow!("Failed to generate HLS playlist with multiple methods"));
        }
    }
    
    // Verify the playlist was created
    if !playlist_path.exists() || std::fs::metadata(&playlist_path)?.len() == 0 {
        return Err(anyhow::anyhow!("Failed to create a valid HLS playlist"));
    }

    // Create a master playlist that references the main playlist
    let master_playlist_path = output_dir.join("master.m3u8");
    let resolution = recording.resolution.clone();
    let master_content = format!(
        "#EXTM3U\n\
        #EXT-X-VERSION:3\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION={}\n\
        playlist.m3u8\n",
        resolution
    );
    std::fs::write(&master_playlist_path, master_content)?;

    info!("Successfully generated HLS playlist for recording {} at: {}", recording.id, playlist_path.display());
    Ok(())
}