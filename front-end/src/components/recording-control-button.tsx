import React, { useState, useEffect } from 'react';
import { Button } from './button';
import { VideoCameraIcon, StopCircleIcon } from '@heroicons/react/24/outline';

interface RecordingControlButtonProps {
  cameraId: string;
  streamId?: string;
  buttonSize?: 'sm' | 'md' | 'lg';
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
  className?: string;
}

interface RecordingStatus {
  recording_id: string;
  camera_id: string;
  stream_id: string;
  start_time: string;
  duration_seconds: number;
  file_size_bytes: number;
  state: string;
  fps: number;
  event_type: string;
}

const RecordingControlButton: React.FC<RecordingControlButtonProps> = ({
  cameraId,
  streamId,
  buttonSize = 'md',
  variant = 'primary',
  className = ''
}) => {
  const [isRecording, setIsRecording] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recordingId, setRecordingId] = useState<string | null>(null);

  // Check if there's an active recording on component mount
  useEffect(() => {
    checkRecordingStatus();
    // Poll every 5 seconds to update status
    const interval = setInterval(checkRecordingStatus, 5000);
    return () => clearInterval(interval);
  }, [cameraId, streamId]);

  // Function to check recording status
  const checkRecordingStatus = async () => {
    try {
      const endpoint = streamId
        ? `http://localhost:4750/recording/status/${cameraId}/${streamId}`
        : `http://localhost:4750/recording/status/${cameraId}`;

      const response = await fetch(endpoint);
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      const hasActiveRecording = data.recordings && data.recordings.length > 0;

      setIsRecording(hasActiveRecording);
      if (hasActiveRecording) {
        setRecordingId(data.recordings[0].recording_id);
      } else {
        setRecordingId(null);
      }
    } catch (error) {
      console.error("Error checking recording status:", error);
      // Don't set error state here to avoid UI clutter
    }
  };

  // Function to start recording
  const startRecording = async () => {
    setIsLoading(true);
    setError(null);

    try {
      const endpoint = streamId
        ? `http://localhost:4750/recording/start/${cameraId}/${streamId}`
        : `http://localhost:4750/recording/start/${cameraId}`;

      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          event_type: 'manual'
        }),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      if (data.status === 'success' && data.recording_id) {
        setIsRecording(true);
        setRecordingId(data.recording_id);
      } else {
        throw new Error(data.message || 'Failed to start recording');
      }
    } catch (error) {
      console.error("Error starting recording:", error);
      setError((error as Error).message || 'Failed to start recording');
      setIsRecording(false);
    } finally {
      setIsLoading(false);
    }
  };

  // Function to stop recording
  const stopRecording = async () => {
    setIsLoading(true);
    setError(null);

    try {
      const endpoint = streamId
        ? `http://localhost:4750/recording/stop/${cameraId}/${streamId}`
        : `http://localhost:4750/recording/stop/${cameraId}`;

      const response = await fetch(endpoint, {
        method: 'POST',
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      if (data.status === 'success' || data.status === 'warning') {
        setIsRecording(false);
        setRecordingId(null);
      } else {
        throw new Error(data.message || 'Failed to stop recording');
      }
    } catch (error) {
      console.error("Error stopping recording:", error);
      setError((error as Error).message || 'Failed to stop recording');
      // Don't change recording state on error, as the recording might still be active
    } finally {
      setIsLoading(false);
    }
  };

  // Handle button click based on recording state
  const handleClick = () => {
    if (isLoading) return;
    if (isRecording) {
      stopRecording();
    } else {
      startRecording();
    }
  };

  // Get button text based on state
  const getButtonText = () => {
    if (isLoading) return isRecording ? 'Stopping...' : 'Starting...';
    return isRecording ? 'Stop Recording' : 'Start Recording';
  };

  // Get button icon based on state
  const getButtonIcon = () => {
    return isRecording ? <StopCircleIcon className="h-5 w-5" /> : <VideoCameraIcon className="h-5 w-5" />;
  };

  // Get button variant based on recording state
  const getButtonVariant = () => {
    if (isRecording) return 'danger';
    return variant;
  };

  return (
    <div>
      <Button
        size={buttonSize}
        className={`${isRecording ? 'animate-pulse' : ''} ${className}`}
        onClick={handleClick}
        disabled={isLoading}
      >
        {getButtonIcon()}
        <span className="ml-1">{getButtonText()}</span>
      </Button>
      {error && (
        <div className="text-red-500 text-sm mt-1">{error}</div>
      )}
    </div>
  );
};

export default RecordingControlButton;
