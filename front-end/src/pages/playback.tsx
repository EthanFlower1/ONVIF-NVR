import React, { useState, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { HlsRecordingPlayer } from '../components/playback';

interface Recording {
  id: string;
  camera_id: string;
  camera_name: string;
  start_time: string;
  end_time?: string;
  duration: number;
  event_type: string;
}

interface Camera {
  id: string;
  name: string;
  model: string;
}

export default function Playback() {
  const [searchParams] = useSearchParams();
  const recordingId = searchParams.get('id');
  const cameraId = searchParams.get('camera_id');
  
  const [recording, setRecording] = useState<Recording | null>(null);
  const [camera, setCamera] = useState<Camera | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [playerMode, setPlayerMode] = useState<'recording' | 'camera'>('recording');

  useEffect(() => {
    // Determine the playback mode based on the URL params
    if (cameraId) {
      setPlayerMode('camera');
    } else if (recordingId) {
      setPlayerMode('recording');
    } else {
      setError('No recording ID or camera ID provided');
      return;
    }

    const fetchData = async () => {
      setLoading(true);
      setError(null);
      
      try {
        if (playerMode === 'recording' && recordingId) {
          // Get data for recording playback
          const response = await fetch(`/playback/${recordingId}`);
          
          if (!response.ok) {
            throw new Error(`Failed to fetch recording: ${response.statusText}`);
          }
          
          const data = await response.json();
          setRecording(data);
        } else if (playerMode === 'camera' && cameraId) {
          // Get camera info for camera playback
          const response = await fetch(`http://localhost:4750/api/cameras/${cameraId}`);
          
          if (!response.ok) {
            throw new Error(`Failed to fetch camera: ${response.statusText}`);
          }
          
          const data = await response.json();
          setCamera({
            id: data.camera.id,
            name: data.camera.name || data.camera.model || `Camera ${data.camera.id.substring(0, 8)}`,
            model: data.camera.model
          });
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'An error occurred');
      } finally {
        setLoading(false);
      }
    };

    fetchData();
  }, [recordingId, cameraId, playerMode]);

  return (
    <div className="container mx-auto px-4 py-8">
      <h1 className="text-2xl font-bold mb-6">
        {playerMode === 'camera' ? 'Camera Live View' : 'Recording Playback'}
      </h1>
      
      {loading && (
        <div className="flex justify-center items-center h-64">
          <div className="animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-indigo-500"></div>
        </div>
      )}
      
      {error && (
        <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4">
          <p>{error}</p>
        </div>
      )}
      
      {!loading && !error && !recording && !camera && !recordingId && !cameraId && (
        <div className="bg-yellow-100 border border-yellow-400 text-yellow-700 px-4 py-3 rounded">
          <p>Please select a recording or camera to play.</p>
        </div>
      )}
      
      {/* Recording Mode Player */}
      {!loading && !error && playerMode === 'recording' && recording && (
        <div className="w-full max-w-4xl mx-auto">
          <HlsRecordingPlayer
            recordingId={recording.id}
            cameraName={recording.camera_name || 'Camera'}
            startTime={recording.start_time}
            endTime={recording.end_time}
            eventType={recording.event_type}
            duration={recording.duration}
            serverUrl={window.location.origin}
            playerMode="recording"
          />
        </div>
      )}

      {/* Camera Mode Player */}
      {!loading && !error && playerMode === 'camera' && camera && (
        <div className="w-full max-w-4xl mx-auto">
          <HlsRecordingPlayer
            cameraId={camera.id}
            cameraName={camera.name}
            serverUrl={window.location.origin}
            playerMode="camera"
            eventType="continuous"
          />
        </div>
      )}

      {!loading && !error && recordingId && !recording && playerMode === 'recording' && (
        <div className="bg-yellow-100 border border-yellow-400 text-yellow-700 px-4 py-3 rounded">
          <p>Recording not found or unable to play.</p>
        </div>
      )}

      {!loading && !error && cameraId && !camera && playerMode === 'camera' && (
        <div className="bg-yellow-100 border border-yellow-400 text-yellow-700 px-4 py-3 rounded">
          <p>Camera not found or unable to play.</p>
        </div>
      )}
    </div>
  );
}
