import React, { useEffect, useState } from "react";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../components/table";
import { PencilIcon, TrashIcon, PlusIcon, XMarkIcon, CheckIcon } from "@heroicons/react/24/outline";
import { Button } from "../components/button";
import { Switch } from "../components/switch";
import { Dialog } from "../components/dialog";
import { Heading } from "../components/heading";
import { Text } from "../components/text";
import { Badge } from "../components/badge";

// Schedule type interface
interface RecordingSchedule {
  id: string;
  camera_id: string;
  stream_id: string;
  name: string;
  description?: string;
  start_time: string; // HH:MM
  end_time: string; // HH:MM
  days_of_week: number[]; // 0-6, where 0 is Sunday
  retention_days: number,
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export default function RecordingSchedules() {
  const [schedules, setSchedules] = useState<RecordingSchedule[]>([]);
  const [cameras, setCameras] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [isEditModalOpen, setIsEditModalOpen] = useState(false);
  const [isDeleteModalOpen, setIsDeleteModalOpen] = useState(false);
  const [selectedSchedule, setSelectedSchedule] = useState<RecordingSchedule | null>(null);
  const [formData, setFormData] = useState<Partial<RecordingSchedule>>({
    name: "",
    description: "",
    start_time: "00:00",
    end_time: "23:59",
    days_of_week: [0, 1, 2, 3, 4, 5, 6],
    retention_days: 30,
    enabled: true,
  });

  // Retention days options
  const retentionOptions = [
    { value: 1, label: "1 day" },
    { value: 3, label: "3 days" },
    { value: 7, label: "7 days" },
    { value: 14, label: "14 days" },
    { value: 30, label: "30 days" },
    { value: 60, label: "60 days" },
    { value: 90, label: "90 days" },
    { value: 180, label: "180 days" },
    { value: 365, label: "1 year" },
  ];

  // Fetch cameras and schedules on component mount
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

    const fetchSchedules = async () => {
      setLoading(true);
      try {
        const response = await fetch('http://localhost:4750/api/schedules');
        if (!response.ok) {
          throw new Error(`HTTP error! status: ${response.status}`);
        }
        const data = await response.json();
        setSchedules(data);
      } catch (error) {
        console.error("Error fetching schedules:", error);
        setError("Failed to load recording schedules");
      } finally {
        setLoading(false);
      }
    };

    fetchCameras();
    fetchSchedules();
  }, []);

  // Format days of week
  const formatDaysOfWeek = (days: number[]) => {
    const dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
    if (days.length === 7) return 'Every day';
    if (days.length === 5 && days.includes(1) && days.includes(2) && days.includes(3) && days.includes(4) && days.includes(5))
      return 'Weekdays';
    if (days.length === 2 && days.includes(0) && days.includes(6))
      return 'Weekends';
    return days.map(day => dayNames[day]).join(', ');
  };

  // Format retention days for display
  const formatRetentionDays = (days: number) => {
    if (days === 1) return "1 day";
    if (days === 365) return "1 year";
    return `${days} days`;
  };

  // Get camera name by ID
  const getCameraName = (cameraId: string) => {
    const camera = cameras.find(c => c.camera.id === cameraId);
    return camera ? camera.camera.name : 'Unknown Camera';
  };

  // Handle form input changes
  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>) => {
    const { name, value } = e.target;
    setFormData({
      ...formData,
      [name]: name === "retention_days" ? parseInt(value, 10) : value
    });
  };

  // Handle day of week toggle
  const toggleDayOfWeek = (day: number) => {
    const currentDays = formData.days_of_week || [];
    if (currentDays.includes(day)) {
      setFormData({
        ...formData,
        days_of_week: currentDays.filter(d => d !== day)
      });
    } else {
      setFormData({
        ...formData,
        days_of_week: [...currentDays, day].sort()
      });
    }
  };

  // Handle enabled toggle
  const toggleScheduleEnabled = async (scheduleId: string, currentValue: boolean) => {
    try {
      const response = await fetch(`http://localhost:4750/api/schedules/${scheduleId}/status`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ enabled: !currentValue }),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      // Update local state
      setSchedules(schedules.map(schedule => {
        if (schedule.id === scheduleId) {
          return { ...schedule, enabled: !currentValue };
        }
        return schedule;
      }));
    } catch (error) {
      console.error("Error toggling schedule status:", error);
      setError("Failed to update schedule status");
    }
  };

  // Reset form
  const resetForm = () => {
    setFormData({
      name: "",
      description: "",
      start_time: "00:00",
      end_time: "23:59",
      days_of_week: [0, 1, 2, 3, 4, 5, 6],
      retention_days: 30,
      enabled: true,
    });
  };

  // Handle add schedule form submission
  const handleAddSchedule = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!formData.camera_id || !formData.stream_id || !formData.name) {
      setError("Please fill out all required fields");
      return;
    }

    try {
      const response = await fetch('http://localhost:4750/api/schedules', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(formData),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const newSchedule = await response.json();
      setSchedules([...schedules, newSchedule]);
      setIsAddModalOpen(false);
      resetForm();
    } catch (error) {
      console.error("Error adding schedule:", error);
      setError("Failed to add recording schedule");
    }
  };

  // Handle edit schedule form submission
  const handleEditSchedule = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!selectedSchedule) return;

    if (!formData.name) {
      setError("Please fill out all required fields");
      return;
    }

    try {
      const response = await fetch(`http://localhost:4750/api/schedules/${selectedSchedule.id}`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(formData),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const updatedSchedule = await response.json();
      setSchedules(schedules.map(schedule => {
        if (schedule.id === selectedSchedule.id) {
          return updatedSchedule;
        }
        return schedule;
      }));
      setIsEditModalOpen(false);
    } catch (error) {
      console.error("Error updating schedule:", error);
      setError("Failed to update recording schedule");
    }
  };

  // Handle delete schedule
  const handleDeleteSchedule = async () => {
    if (!selectedSchedule) return;

    try {
      const response = await fetch(`http://localhost:4750/api/schedules/${selectedSchedule.id}`, {
        method: 'DELETE',
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      setSchedules(schedules.filter(schedule => schedule.id !== selectedSchedule.id));
      setIsDeleteModalOpen(false);
      setSelectedSchedule(null);
    } catch (error) {
      console.error("Error deleting schedule:", error);
      setError("Failed to delete recording schedule");
    }
  };

  // Open edit modal and set form data
  const openEditModal = (schedule: RecordingSchedule) => {
    setSelectedSchedule(schedule);
    setFormData({
      camera_id: schedule.camera_id,
      stream_id: schedule.stream_id,
      name: schedule.name,
      description: schedule.description || "",
      start_time: schedule.start_time,
      end_time: schedule.end_time,
      days_of_week: schedule.days_of_week,
      retention_days: schedule.retention_days || 30,
      enabled: schedule.enabled,
    });
    setIsEditModalOpen(true);
  };

  // Component for day of week selector
  const DayOfWeekSelector = () => {
    const days = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];
    const currentDays = formData.days_of_week || [];

    return (
      <div className="flex space-x-2">
        {days.map((day, index) => (
          <button
            key={index}
            type="button"
            className={`size-8 rounded-full flex items-center justify-center focus:outline-none ${currentDays.includes(index)
              ? 'bg-indigo-600 text-white'
              : 'bg-gray-200 text-gray-600'
              }`}
            onClick={() => toggleDayOfWeek(index)}
          >
            {day}
          </button>
        ))}
      </div>
    );
  };

  // Loading state
  if (loading && schedules.length === 0) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-center">
          <div className="inline-block animate-spin rounded-full h-8 w-8 border-4 border-indigo-500 border-t-transparent mb-2"></div>
          <p className="text-gray-600">Loading recording schedules...</p>
        </div>
      </div>
    );
  }

  return (
    <div className="p-4">
      <div className="flex justify-between items-center mb-6">
        <div>
          <Heading level={1} className="mb-2">Recording Schedules</Heading>
          <Text>Create and manage automated recording schedules</Text>
        </div>
        <Button
          className="bg-indigo-600 hover:bg-indigo-700 text-white"
          onClick={() => {
            resetForm();
            setIsAddModalOpen(true);
          }}
        >
          <PlusIcon className="-ml-1 mr-1 h-5 w-5" />
          Add Schedule
        </Button>
      </div>

      {error && (
        <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4">
          {error}
        </div>
      )}

      {/* Schedules Table */}
      <div className="bg-white rounded-lg shadow overflow-hidden">
        <Table className="min-w-full divide-y divide-gray-200">
          <TableHead>
            <TableRow>
              <TableHeader>Name</TableHeader>
              <TableHeader>Camera</TableHeader>
              <TableHeader>Time</TableHeader>
              <TableHeader>Days</TableHeader>
              <TableHeader>Retention</TableHeader>
              <TableHeader>Status</TableHeader>
              <TableHeader>Actions</TableHeader>
            </TableRow>
          </TableHead>
          <TableBody>
            {schedules.length === 0 ? (
              <TableRow>
                <TableCell colSpan={7} className="text-center py-4">
                  No recording schedules found. Click "Add Schedule" to create one.
                </TableCell>
              </TableRow>
            ) : (
              schedules.map((schedule) => (
                <TableRow key={schedule.id}>
                  <TableCell className="font-medium">{schedule.name}</TableCell>
                  <TableCell>{getCameraName(schedule.camera_id)}</TableCell>
                  <TableCell>{`${schedule.start_time} - ${schedule.end_time}`}</TableCell>
                  <TableCell>{formatDaysOfWeek(schedule.days_of_week)}</TableCell>
                  <TableCell>{formatRetentionDays(schedule.retention_days)}</TableCell>
                  <TableCell>
                    <div className="flex items-center">
                      <Switch
                        checked={schedule.enabled}
                        onChange={() => toggleScheduleEnabled(schedule.id, schedule.enabled)}
                      />
                      <Badge className={`ml-2 ${schedule.enabled ? 'bg-green-100 text-green-800' : 'bg-gray-100 text-gray-800'}`}>
                        {schedule.enabled ? 'Active' : 'Inactive'}
                      </Badge>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex space-x-2">
                      <button
                        onClick={() => openEditModal(schedule)}
                        className="text-indigo-600 hover:text-indigo-900"
                        title="Edit Schedule"
                      >
                        <PencilIcon className="h-5 w-5" />
                      </button>
                      <button
                        onClick={() => {
                          setSelectedSchedule(schedule);
                          setIsDeleteModalOpen(true);
                        }}
                        className="text-red-600 hover:text-red-900"
                        title="Delete Schedule"
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

      {/* Add Schedule Modal */}
      <Dialog open={isAddModalOpen} onClose={() => setIsAddModalOpen(false)} size="2xl">
        <div className="flex justify-between items-center mb-4">
          <Heading level={2}>Add Recording Schedule</Heading>
          <button
            onClick={() => setIsAddModalOpen(false)}
            className="text-gray-500 hover:text-gray-700"
          >
            <XMarkIcon className="h-6 w-6" />
          </button>
        </div>

        <form onSubmit={handleAddSchedule}>
          <div className="grid grid-cols-1 gap-4 mb-4">
            <div>
              <label htmlFor="name" className="block text-sm font-medium text-gray-700 mb-1">Schedule Name *</label>
              <input
                type="text"
                id="name"
                name="name"
                required
                className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                value={formData.name || ''}
                onChange={handleInputChange}
              />
            </div>

            <div>
              <label htmlFor="description" className="block text-sm font-medium text-gray-700 mb-1">Description</label>
              <textarea
                id="description"
                name="description"
                rows={2}
                className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                value={formData.description || ''}
                onChange={handleInputChange}
              />
            </div>

            <div>
              <label htmlFor="camera_id" className="block text-sm font-medium text-gray-700 mb-1">Camera *</label>
              <select
                id="camera_id"
                name="camera_id"
                required
                className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                value={formData.camera_id || ''}
                onChange={(e) => {
                  handleInputChange(e);
                  // Reset stream_id when camera changes
                  setFormData(prev => ({ ...prev, stream_id: '' }));
                }}
              >
                <option value="">Select Camera</option>
                {cameras.map(camera => (
                  <option key={camera.camera.id} value={camera.camera.id}>
                    {camera.camera.name || `Camera ${camera.camera.id.substring(0, 8)}`}
                  </option>
                ))}
              </select>
            </div>

            {formData.camera_id && (
              <div>
                <label htmlFor="stream_id" className="block text-sm font-medium text-gray-700 mb-1">Stream *</label>
                <select
                  id="stream_id"
                  name="stream_id"
                  required
                  className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                  value={formData.stream_id || ''}
                  onChange={handleInputChange}
                >
                  <option value="">Select Stream</option>
                  {cameras
                    .find(c => c.camera.id === formData.camera_id)?.streams
                    .map((stream: any) => (
                      <option key={stream.id} value={stream.id}>
                        {stream.name || `Stream ${stream.id.substring(0, 8)}`}
                      </option>
                    ))}
                </select>
              </div>
            )}

            <div className="grid grid-cols-2 gap-4">
              <div>
                <label htmlFor="start_time" className="block text-sm font-medium text-gray-700 mb-1">Start Time *</label>
                <input
                  type="time"
                  id="start_time"
                  name="start_time"
                  required
                  className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                  value={formData.start_time || ''}
                  onChange={handleInputChange}
                />
              </div>

              <div>
                <label htmlFor="end_time" className="block text-sm font-medium text-gray-700 mb-1">End Time *</label>
                <input
                  type="time"
                  id="end_time"
                  name="end_time"
                  required
                  className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                  value={formData.end_time || ''}
                  onChange={handleInputChange}
                />
              </div>
            </div>

            <div>
              <label htmlFor="retention_days" className="block text-sm font-medium text-gray-700 mb-1">
                Retention Period *
              </label>
              <select
                id="retention_days"
                name="retention_days"
                required
                className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                value={formData.retention_days || 30}
                onChange={handleInputChange}
              >
                {retentionOptions.map(option => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
              <p className="mt-1 text-sm text-gray-500">
                Recordings will be automatically deleted after this period
              </p>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-2">Days of Week *</label>
              <DayOfWeekSelector />
            </div>

            <div className="flex items-center">
              <Switch
                id="enabled"
                checked={formData.enabled || false}
                onChange={() => setFormData({ ...formData, enabled: !formData.enabled })}
              />
              <label htmlFor="enabled" className="ml-2 block text-sm font-medium text-gray-700">
                Enable schedule immediately
              </label>
            </div>
          </div>

          <div className="flex justify-end space-x-2 mt-6">
            <Button
              type="button"
              className="bg-gray-200 hover:bg-gray-300 text-gray-800"
              onClick={() => setIsAddModalOpen(false)}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              className="bg-indigo-600 hover:bg-indigo-700 text-white"
            >
              <CheckIcon className="-ml-1 mr-1 h-5 w-5" />
              Create Schedule
            </Button>
          </div>
        </form>
      </Dialog>

      {/* Edit Schedule Modal */}
      {selectedSchedule && (
        <Dialog open={isEditModalOpen} onClose={() => setIsEditModalOpen(false)} size="2xl">
          <div className="flex justify-between items-center mb-4">
            <Heading level={2}>Edit Recording Schedule</Heading>
            <button
              onClick={() => setIsEditModalOpen(false)}
              className="text-gray-500 hover:text-gray-700"
            >
              <XMarkIcon className="h-6 w-6" />
            </button>
          </div>

          <form onSubmit={handleEditSchedule}>
            <div className="grid grid-cols-1 gap-4 mb-4">
              <div>
                <label htmlFor="edit-name" className="block text-sm font-medium text-gray-700 mb-1">Schedule Name *</label>
                <input
                  type="text"
                  id="edit-name"
                  name="name"
                  required
                  className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                  value={formData.name || ''}
                  onChange={handleInputChange}
                />
              </div>

              <div>
                <label htmlFor="edit-description" className="block text-sm font-medium text-gray-700 mb-1">Description</label>
                <textarea
                  id="edit-description"
                  name="description"
                  rows={2}
                  className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                  value={formData.description || ''}
                  onChange={handleInputChange}
                />
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label htmlFor="edit-start_time" className="block text-sm font-medium text-gray-700 mb-1">Start Time *</label>
                  <input
                    type="time"
                    id="edit-start_time"
                    name="start_time"
                    required
                    className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                    value={formData.start_time || ''}
                    onChange={handleInputChange}
                  />
                </div>

                <div>
                  <label htmlFor="edit-end_time" className="block text-sm font-medium text-gray-700 mb-1">End Time *</label>
                  <input
                    type="time"
                    id="edit-end_time"
                    name="end_time"
                    required
                    className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                    value={formData.end_time || ''}
                    onChange={handleInputChange}
                  />
                </div>
              </div>

              <div>
                <label htmlFor="edit-retention_days" className="block text-sm font-medium text-gray-700 mb-1">
                  Retention Period *
                </label>
                <select
                  id="edit-retention_days"
                  name="retention_days"
                  required
                  className="w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
                  value={formData.retention_days || 30}
                  onChange={handleInputChange}
                >
                  {retentionOptions.map(option => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
                <p className="mt-1 text-sm text-gray-500">
                  Recordings will be automatically deleted after this period
                </p>
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-700 mb-2">Days of Week *</label>
                <DayOfWeekSelector />
              </div>

              <div className="flex items-center">
                <Switch
                  id="edit-enabled"
                  checked={formData.enabled || false}
                  onChange={() => setFormData({ ...formData, enabled: !formData.enabled })}
                />
                <label htmlFor="edit-enabled" className="ml-2 block text-sm font-medium text-gray-700">
                  {formData.enabled ? 'Schedule is active' : 'Schedule is inactive'}
                </label>
              </div>
            </div>

            <div className="flex justify-end space-x-2 mt-6">
              <Button
                type="button"
                className="bg-gray-200 hover:bg-gray-300 text-gray-800"
                onClick={() => setIsEditModalOpen(false)}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                className="bg-indigo-600 hover:bg-indigo-700 text-white"
              >
                <CheckIcon className="-ml-1 mr-1 h-5 w-5" />
                Save Changes
              </Button>
            </div>
          </form>
        </Dialog>
      )}

      {/* Delete Confirmation Modal */}
      {selectedSchedule && (
        <Dialog open={isDeleteModalOpen} onClose={() => setIsDeleteModalOpen(false)}>
          <Heading level={2} className="mb-4">Delete Recording Schedule</Heading>
          <Text className="mb-4">
            Are you sure you want to delete the schedule "{selectedSchedule.name}"?
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
              onClick={handleDeleteSchedule}
            >
              Delete
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
