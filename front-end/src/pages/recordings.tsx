import React, { useEffect, useState } from "react";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../components/table";
import { FolderIcon, TrashIcon, PlayIcon } from "@heroicons/react/24/outline";
import { Button } from "../components/button";
import { Badge } from "../components/badge";
import { Dialog } from "../components/dialog";
import { Heading } from "../components/heading";
import { Text } from "../components/text";
import { Select } from "../components/select";
import { Input } from "../components/input";

// Recording type interface
interface Recording {
  id: string;
  camera_id: string;
  stream_id: string;
  start_time: string;
  end_time?: string | null;
  file_path: string;
  file_size: number;
  duration: number;
  format: string;
  resolution: string;
  fps: number;
  event_type: string;
  schedule_id?: string | null;
  metadata?: any | null;
}

interface RecordingSearchParams {
  camera_id?: string;
  stream_id?: string;
  start_time?: string;
  end_time?: string;
  event_type?: string;
  limit?: number;
  offset?: number;
}

export default function Recordings() {
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [filteredRecordings, setFilteredRecordings] = useState<Recording[]>([]);
  const [cameras, setCameras] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchParams, setSearchParams] = useState<RecordingSearchParams>({
    limit: 100,
    offset: 0,
  });
  const [selectedRecording, setSelectedRecording] = useState<Recording | null>(null);
  const [isDetailModalOpen, setIsDetailModalOpen] = useState(false);
  const [isDeleteModalOpen, setIsDeleteModalOpen] = useState(false);

  // Fetch cameras data
  useEffect(() => {
    const fetchCameras = async () => {
      try {
        const response = await fetch('http://localhost:4750/api/cameras');
        if (!response.ok) {
          throw new Error(`HTTP error! status: ${response.status}`);
        }
        const data = await response.json();
        setCameras(data);
      } catch (error) {
        console.error("Error fetching cameras:", error);
        setError("Failed to load cameras");
      }
    };

    fetchCameras();
  }, []);

  // Function to search recordings
  const searchRecordings = async () => {
    setLoading(true);
    setError(null);

    try {
      // Build query string from search params
      const queryParams = new URLSearchParams();
      if (searchParams.camera_id) queryParams.append('camera_id', searchParams.camera_id);
      if (searchParams.stream_id) queryParams.append('stream_id', searchParams.stream_id);
      if (searchParams.start_time) queryParams.append('start_time', searchParams.start_time);
      if (searchParams.end_time) queryParams.append('end_time', searchParams.end_time);
      if (searchParams.event_type) queryParams.append('event_type', searchParams.event_type);
      if (searchParams.limit) queryParams.append('limit', searchParams.limit.toString());
      if (searchParams.offset) queryParams.append('offset', searchParams.offset.toString());

      const response = await fetch(`http://localhost:4750/recording/search?${queryParams}`);

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const data = await response.json();
      console.log("Recordings Data: ", data)
      if (data && data.recordings) {
        setRecordings(data.recordings);
        setFilteredRecordings(data.recordings);
      } else {
        setRecordings([]);
        setFilteredRecordings([]);
      }
    } catch (error) {
      console.error("Error searching recordings:", error);
      setError("Failed to search recordings");
    } finally {
      setLoading(false);
    }
  };

  // Initial load
  useEffect(() => {
    searchRecordings();
  }, []);

  // Handle search form submission
  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    searchRecordings();
  };

  // Handle input changes
  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    const { name, value } = e.target;
    setSearchParams(prev => ({
      ...prev,
      [name]: value
    }));
  };

  // Format byte size to human-readable format
  const formatFileSize = (bytes: number) => {
    if (bytes === 0) return "0 B";
    if (bytes < 1024) return bytes + " B";
    else if (bytes < 1048576) return (bytes / 1024).toFixed(2) + " KB";
    else if (bytes < 1073741824) return (bytes / 1048576).toFixed(2) + " MB";
    else return (bytes / 1073741824).toFixed(2) + " GB";
  };

  // Format duration in seconds to HH:MM:SS
  const formatDuration = (milliseconds: number) => {
    if (milliseconds === 0) return "In progress";

    const totalSeconds = Math.floor(milliseconds / 1000);
    const hrs = Math.floor(totalSeconds / 3600);
    const mins = Math.floor((totalSeconds % 3600) / 60);
    const secs = Math.floor(totalSeconds % 60);

    return `${hrs.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  };

  // Get camera name by ID
  const getCameraName = (cameraId: string) => {
    const camera = cameras.find(c => c.camera.id === cameraId);
    console.log("camaeras: ", cameras)
    return camera ? camera.camera.name === '' ? camera.camera.model : camera.camera.name : `Camera ${cameraId.substring(0, 8)}`;
  };

  // Get event type badge color
  const getEventTypeBadgeColor = (eventType: string) => {
    switch (eventType.toLowerCase()) {
      case 'motion': return 'bg-yellow-100 text-yellow-800';
      case 'audio': return 'bg-blue-100 text-blue-800';
      case 'continuous': return 'bg-green-100 text-green-800';
      case 'manual': return 'bg-purple-100 text-purple-800';
      case 'analytics': return 'bg-red-100 text-red-800';
      case 'external': return 'bg-gray-100 text-gray-800';
      default: return 'bg-gray-100 text-gray-800';
    }
  };

  // Delete recording
  const deleteRecording = async (id: string) => {
    try {
      const response = await fetch(`http://localhost:4750/recording/${id}`, {
        method: 'DELETE',
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      // Remove deleted recording from state
      setRecordings(recordings.filter(recording => recording.id !== id));
      setFilteredRecordings(filteredRecordings.filter(recording => recording.id !== id));
      setIsDeleteModalOpen(false);
    } catch (error) {
      console.error("Error deleting recording:", error);
      setError("Failed to delete recording");
    }
  };

  // Format date string
  const formatDate = (dateString: string) => {
    console.log("dateString", dateString)
    return new Date(dateString).toLocaleString();
  };

  // Removed unused getFileName function

  if (loading && recordings.length === 0) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="inline-block animate-spin rounded-full h-8 w-8 border-4 border-indigo-500 border-t-transparent mb-2"></div>
          <p >Loading recordings...</p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4">
      <div className="mb-6">
        <Heading level={1} className="mb-2">Recordings</Heading>
        <Text>Search and manage your camera recordings</Text>
      </div>

      {/* Search Form */}
      <div className="p-4 rounded-lg shadow mb-6">
        <form onSubmit={handleSearchSubmit} className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div>
            <Text className="block text-sm font-medium  mb-1">Camera</Text>
            <Select
              id="camera_id"
              name="camera_id"
              className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
              value={searchParams.camera_id || ''}
              onChange={handleInputChange}
            >
              <option value="">All Cameras</option>
              {cameras.map(camera => (
                <option key={camera.camera.id} value={camera.camera.id}>
                  {camera.camera.name || `Camera ${camera.camera.id.substring(0, 8)}`}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <Text className="block text-sm font-medium  mb-1">Event Type</Text>
            <Select
              id="event_type"
              name="event_type"
              className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
              value={searchParams.event_type || ''}
              onChange={handleInputChange}
            >
              <option value="">All Types</option>
              <option value="continuous">Continuous</option>
              <option value="motion">Motion</option>
              <option value="audio">Audio</option>
              <option value="manual">Manual</option>
              <option value="analytics">Analytics</option>
              <option value="external">External</option>
            </Select>
          </div>

          <div>
            <Text className="block text-sm font-medium  mb-1">Start Date</Text>
            <Input
              type="datetime-local"
              id="start_time"
              name="start_time"
              className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
              value={searchParams.start_time || ''}
              onChange={handleInputChange}
            />
          </div>

          <div>
            <Text className="block text-sm font-medium mb-1">End Date</Text>
            <Input
              type="datetime-local"
              id="end_time"
              name="end_time"
              className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
              value={searchParams.end_time || ''}
              onChange={handleInputChange}
            />
          </div>

          <div>
            <Text className="block text-sm font-medium mb-1">Limit</Text>
            <Input
              type="number"
              id="limit"
              name="limit"
              min="1"
              max="1000"
              className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
              value={searchParams.limit || 100}
              onChange={handleInputChange}
            />
          </div>

          <div className="flex items-end">
            <Button type="submit" className="w-full">Search</Button>
          </div>
        </form>
      </div>

      {error && (
        <Text className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4">
          {error}
        </Text>
      )}

      {/* Recordings Table */}
      <div className="rounded-lg shadow overflow-hidden">
        <Table className="min-w-full divide-y divide-gray-200">
          <TableHead>
            <TableRow>
              <TableHeader>Camera</TableHeader>
              <TableHeader>Type</TableHeader>
              <TableHeader>Start Time</TableHeader>
              <TableHeader>End Time</TableHeader>
              <TableHeader>Duration</TableHeader>
              <TableHeader>Resolution</TableHeader>
              <TableHeader>Size</TableHeader>
              <TableHeader>Actions</TableHeader>
            </TableRow>
          </TableHead>
          <TableBody>
            {filteredRecordings.length === 0 ? (
              <TableRow>
                <TableCell colSpan={7} className="text-center py-4">
                  No recordings found
                </TableCell>
              </TableRow>
            ) : (
              filteredRecordings.map((recording) => (
                <TableRow key={recording.id}>
                  <TableCell>{getCameraName(recording.camera_id)}</TableCell>
                  <TableCell>
                    <Badge className={getEventTypeBadgeColor(recording.event_type)}>
                      {recording.event_type}
                    </Badge>
                  </TableCell>
                  <TableCell>{formatDate(recording.start_time)}</TableCell>
                  <TableCell>{formatDate(recording.end_time ?? '')}</TableCell>
                  <TableCell>{formatDuration(recording.duration)}</TableCell>
                  <TableCell>{recording.resolution === "unknown" ? "Processing..." : recording.resolution}</TableCell>
                  <TableCell>{formatFileSize(recording.file_size)}</TableCell>
                  <TableCell>
                    <div className="flex space-x-2">
                      <button
                        onClick={() => {
                          setSelectedRecording(recording);
                          setIsDetailModalOpen(true);
                        }}
                        className="text-indigo-600 hover:text-indigo-900"
                        title="View Details"
                      >
                        <FolderIcon className="h-5 w-5" />
                      </button>
                      <button
                        onClick={() => {
                          setSelectedRecording(recording);
                          setIsDeleteModalOpen(true);
                        }}
                        className="text-red-600 hover:text-red-900"
                        title="Delete Recording"
                      >
                        <TrashIcon className="h-5 w-5" />
                      </button>
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      {/* Recording Details Modal */}
      {selectedRecording && (
        <Dialog open={isDetailModalOpen} onClose={() => setIsDetailModalOpen(false)} size="3xl">
          <Heading level={2} className="mb-4">Recording Details</Heading>
          <div className="grid grid-cols-2 gap-4 mb-4">
            <div>
              <Text className="font-semibold">Camera:</Text>
              <Text>{getCameraName(selectedRecording.camera_id)}</Text>
            </div>
            <div>
              <Text className="font-semibold">Event Type:</Text>
              <Badge className={getEventTypeBadgeColor(selectedRecording.event_type)}>
                {selectedRecording.event_type}
              </Badge>
            </div>
            <div>
              <Text className="font-semibold">Start Time:</Text>
              <Text>{formatDate(selectedRecording.start_time)}</Text>
            </div>
            <div>
              <Text className="font-semibold">End Time:</Text>
              <Text>{selectedRecording.end_time ? formatDate(selectedRecording.end_time) : 'In progress'}</Text>
            </div>
            <div>
              <Text className="font-semibold">Duration:</Text>
              <Text>{formatDuration(selectedRecording.duration)}</Text>
            </div>
            <div>
              <Text className="font-semibold">File Size:</Text>
              <Text>{formatFileSize(selectedRecording.file_size)}</Text>
            </div>
            <div>
              <Text className="font-semibold">Resolution:</Text>
              <Text>{selectedRecording.resolution === "unknown" ? "Processing..." : selectedRecording.resolution}</Text>
            </div>
            <div>
              <Text className="font-semibold">FPS:</Text>
              <Text>{selectedRecording.fps}</Text>
            </div>
            <div>
              <Text className="font-semibold">Format:</Text>
              <Text>{selectedRecording.format}</Text>
            </div>
            <div>
              <Text className="font-semibold">File Path:</Text>
              <Text className="truncate">{selectedRecording.file_path}</Text>
            </div>
            {selectedRecording.schedule_id && (
              <div>
                <Text className="font-semibold">Schedule ID:</Text>
                <Text>{selectedRecording.schedule_id}</Text>
              </div>
            )}
            <div>
              <Text className="font-semibold">Stream ID:</Text>
              <Text>{selectedRecording.stream_id}</Text>
            </div>
          </div>
          <div className="flex justify-between mt-6">
            <Button
              className="bg-gray-200 hover:bg-gray-300 text-gray-800"
              onClick={() => setIsDetailModalOpen(false)}
            >
              Close
            </Button>
            <div className="flex space-x-2">
              <Button
                className="bg-indigo-600 hover:bg-indigo-700 text-white"
                disabled={selectedRecording.file_size === 0}
                onClick={() => {
                  const url = `/playback?camera_id=${selectedRecording.camera_id}&recording_id=${selectedRecording.id}`;
                  window.open(url, '_blank');
                }}
              >
                <PlayIcon className="-ml-1 mr-1 h-5 w-5" />
                Play
              </Button>
              <Button
                className="bg-blue-600 hover:bg-blue-700 text-white"
                disabled={selectedRecording.file_size === 0}
              >
                <FolderIcon className="-ml-1 mr-1 h-5 w-5" />
                Download
              </Button>
              <Button
                className="bg-red-600 hover:bg-red-700 text-white"
                onClick={() => {
                  setIsDetailModalOpen(false);
                  setIsDeleteModalOpen(true);
                }}
              >
                <TrashIcon className="-ml-1 mr-1 h-5 w-5" />
                Delete
              </Button>
            </div>
          </div>
        </Dialog>
      )}

      {/* Delete Confirmation Modal */}
      {selectedRecording && (
        <Dialog open={isDeleteModalOpen} onClose={() => setIsDeleteModalOpen(false)}>
          <Heading level={2} className="mb-4">Delete Recording</Heading>
          <Text className="mb-4">
            Are you sure you want to delete this recording from {getCameraName(selectedRecording.camera_id)}?
            This action cannot be undone.
          </Text>
          <div className="flex justify-end space-x-2 mt-6">
            <Button
              className="bg-gray-200 hover:bg-gray-300 text-gray-800"
              onClick={() => setIsDeleteModalOpen(false)}
            >
              Cancel
            </Button>
            <Button
              className="bg-red-600 hover:bg-red-700 text-white"
              onClick={() => deleteRecording(selectedRecording.id)}
            >
              Delete
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
