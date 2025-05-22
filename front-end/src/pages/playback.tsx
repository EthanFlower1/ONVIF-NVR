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
  metadata?: any;
  segments?: RecordingSegment[];
}

interface RecordingSegment {
  id: string;
  start_time: string;
  duration: number;
  segment_id: number;
}

interface TimelineSegment {
  id: string;
  startTime: Date;
  endTime: Date;
  duration: number; 
  segment_id: number;
}

export default function Playback() {
  const [searchParams] = useSearchParams();
  const recordingId = searchParams.get('id');
  const cameraId = searchParams.get('camera_id');

  const [recording, setRecording] = useState<Recording | null>(null);
  const [segments, setSegments] = useState<TimelineSegment[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentSegmentIndex, setCurrentSegmentIndex] = useState<number>(0);
  const [isSegmentedRecording, setIsSegmentedRecording] = useState<boolean>(false);

  // Effect to fetch recording details
  useEffect(() => {
    if (!recordingId && !cameraId) {
      setError('No recording ID or camera ID provided');
      return;
    }

    const fetchRecording = async () => {
      setLoading(true);
      setError(null);

      try {
        // Get data from the playback info API endpoint
        const endpoint = recordingId 
          ? `/api/recordings/${recordingId}`
          : `/api/cameras/${cameraId}/latest-recording`;
          
        const response = await fetch(endpoint);

        if (!response.ok) {
          throw new Error(`Failed to fetch recording: ${response.statusText}`);
        }

        const data = await response.json();
        setRecording(data);
        
        // Check if this is a segmented recording by checking metadata
        const isSegmented = data.metadata && 
                           data.metadata.hls && 
                           data.metadata.hls.format === 'ts';
        
        setIsSegmentedRecording(isSegmented);
        
        // If segmented, fetch the segments
        if (isSegmented) {
          await fetchSegments(data.id);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'An error occurred');
      } finally {
        setLoading(false);
      }
    };

    const fetchSegments = async (parentId: string) => {
      try {
        const response = await fetch(`/api/recordings/${parentId}/segments`);
        
        if (!response.ok) {
          throw new Error(`Failed to fetch segments: ${response.statusText}`);
        }
        
        const segmentData = await response.json();
        
        // Convert segments to timeline format
        const timelineSegments = segmentData.map((segment: any) => {
          const startTime = new Date(segment.start_time);
          const durationMs = segment.duration * 1000; // Convert to milliseconds
          const endTime = new Date(startTime.getTime() + durationMs);
          
          return {
            id: segment.id,
            startTime,
            endTime,
            duration: segment.duration,
            segment_id: segment.segment_id
          };
        });
        
        // Sort segments by time
        timelineSegments.sort((a: TimelineSegment, b: TimelineSegment) => 
          a.startTime.getTime() - b.startTime.getTime());
          
        setSegments(timelineSegments);
      } catch (err) {
        console.error('Error fetching segments:', err);
        // Don't set error here, we can still play the recording even without segments
      }
    };

    fetchRecording();
  }, [recordingId, cameraId]);

  // Function to handle segment change from timeline
  const handleSegmentChange = (segmentIndex: number) => {
    if (segmentIndex >= 0 && segmentIndex < segments.length) {
      setCurrentSegmentIndex(segmentIndex);
    }
  };

  return (
    <div className="container mx-auto px-4 py-8">
      <h1 className="text-2xl font-bold mb-6">Recording Playback</h1>

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

      {!loading && !error && !recording && !recordingId && !cameraId && (
        <div className="bg-yellow-100 border border-yellow-400 text-yellow-700 px-4 py-3 rounded">
          <p>Please select a recording or camera to play.</p>
        </div>
      )}

      {!loading && !error && recording && (
        <div className="w-full max-w-4xl mx-auto">
          {/* Timeline component for segmented recordings */}
          {isSegmentedRecording && segments.length > 0 && (
            <div className="mb-4 bg-gray-100 p-4 rounded-lg">
              <h3 className="text-lg font-semibold mb-2">Timeline</h3>
              <div className="flex items-center">
                <button 
                  onClick={() => handleSegmentChange(currentSegmentIndex - 1)}
                  disabled={currentSegmentIndex <= 0}
                  className="p-1 rounded disabled:opacity-50 hover:bg-gray-200"
                >
                  &larr;
                </button>
                
                <div className="flex-1 mx-2 overflow-x-auto">
                  <div className="flex space-x-1">
                    {segments.map((segment, idx) => (
                      <div 
                        key={segment.id}
                        onClick={() => handleSegmentChange(idx)}
                        className={`h-8 ${idx === currentSegmentIndex ? 'bg-blue-500' : 'bg-blue-300'} 
                                   rounded cursor-pointer flex-1 min-w-[30px] hover:bg-blue-400
                                   flex items-center justify-center text-xs text-white`}
                        title={`Segment ${segment.segment_id}: ${segment.startTime.toLocaleTimeString()}`}
                      >
                        {segment.segment_id}
                      </div>
                    ))}
                  </div>
                </div>
                
                <button 
                  onClick={() => handleSegmentChange(currentSegmentIndex + 1)}
                  disabled={currentSegmentIndex >= segments.length - 1}
                  className="p-1 rounded disabled:opacity-50 hover:bg-gray-200"
                >
                  &rarr;
                </button>
              </div>
              <div className="mt-2 text-sm text-gray-600">
                <span>Start: {segments[currentSegmentIndex]?.startTime.toLocaleString()}</span>
                <span className="mx-2">|</span>
                <span>Duration: {segments[currentSegmentIndex]?.duration}s</span>
              </div>
            </div>
          )}
          
          <HlsRecordingPlayer
            recordingId={recording.id}
            cameraName={recording.camera_name || 'Camera'}
            startTime={recording.start_time}
            endTime={recording.end_time}
            eventType={recording.event_type}
            duration={recording.duration}
            serverUrl={window.location.origin}
            currentSegment={isSegmentedRecording ? segments[currentSegmentIndex]?.segment_id : undefined}
            isSegmentedRecording={isSegmentedRecording}
          />
        </div>
      )}

      {!loading && !error && (recordingId || cameraId) && !recording && (
        <div className="bg-yellow-100 border border-yellow-400 text-yellow-700 px-4 py-3 rounded">
          <p>Recording not found or unable to play.</p>
        </div>
      )}
    </div>
  );
}
