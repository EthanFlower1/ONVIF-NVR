import React, { useEffect, useState } from "react";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../components/table";
import { PencilIcon, TrashIcon } from "@heroicons/react/24/outline";
import { CheckIcon, XMarkIcon } from "@heroicons/react/16/solid";

export default function Cameras() {
  const [data, setData] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<any>(null);

  // New state for editing camera details
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editedData, setEditedData] = useState({
    name: "",
    username: "",
    password: ""
  });

  const fetchData = async () => {
    setLoading(true);
    try {
      const response = await fetch('http://localhost:4750/api/cameras', {
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
      });
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const json = await response.json();
      console.log("Response: ", json);

      // Process data to ensure camera names have default values
      const processedData = json.map((item: any, index: number) => {
        if (!item.camera.name || item.camera.name.trim() === "") {
          return {
            ...item,
            camera: {
              ...item.camera,
              name: `Camera ${index + 1}`
            }
          };
        }
        return item;
      });

      setData(processedData);
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  const startEditing = (device: any) => {
    setEditingId(device.camera.id);
    setEditedData({
      name: device.camera.name,
      username: device.camera.username || "",
      password: device.camera.password || ""
    });
  };

  const saveChanges = async (id: string) => {
    // Here you would implement the API call to save changes
    // For now, we'll just update the local state
    setData(data.map(item => {
      if (item.camera.id === id) {
        const camera = {
          ...item.camera,
          name: editedData.name || `Camera ${data.indexOf(item) + 1}`,
          username: editedData.username,
          password: editedData.password
        };

        fetch(`http://localhost:4750/api/cameras/${camera.id}`, {
          method: 'PUT',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify(camera)
        });

        return {
          ...item,
          camera,
        };
      }

      return item;
    }));
    setEditingId(null);
  };

  const cancelEditing = () => {
    setEditingId(null);
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    setEditedData(prev => ({
      ...prev,
      [name]: value
    }));
  };

  // Placeholder delete function
  const deleteCamera = async (id: string) => {
    // In a real implementation, you would make an API call to delete the camera
    console.log(`Deleting camera with ID: ${id}`);
    try {
      const response = await fetch(`http://localhost:4750/api/cameras/${id}`, {
        method: 'DELETE',
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const json = await response.json();
      console.log("Response: ", json);

      setData(data.filter(item => item.camera.id !== id));
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }


  };

  if (loading) return <div>Loading cameras...</div>;
  if (error) return <div>Error loading cameras: {error.message}</div>;

  return (
    <div>
      <Table className="mt-8 [--gutter:--spacing(6)] lg:[--gutter:--spacing(10)]">
        <TableHead>
          <TableRow>
            <TableHeader>Device Name</TableHeader>
            <TableHeader>Address</TableHeader>
            <TableHeader>Manufacturer</TableHeader>
            <TableHeader>Model</TableHeader>
            <TableHeader>Username</TableHeader>
            <TableHeader>Password</TableHeader>
            <TableHeader>Actions</TableHeader>
          </TableRow>
        </TableHead>
        <TableBody>
          {data.map((device, index) => (
            <TableRow key={device.camera.id}>
              <TableCell>
                {editingId === device.camera.id ? (
                  <input
                    type="text"
                    name="name"
                    value={editedData.name}
                    onChange={handleChange}
                    className="w-full p-1 border border-gray-300 rounded"
                    placeholder={`Camera ${index + 1}`}
                  />
                ) : (
                  device.camera.name || `Camera ${index + 1}`
                )}
              </TableCell>
              <TableCell className="text-zinc-500">{device.camera.ip_address}</TableCell>
              <TableCell className="text-zinc-500">{device.camera.manufacturer}</TableCell>
              <TableCell className="text-zinc-500">{device.camera.model}</TableCell>
              <TableCell className="text-zinc-500">
                {editingId === device.camera.id ? (
                  <input
                    type="text"
                    name="username"
                    value={editedData.username}
                    onChange={handleChange}
                    className="w-full p-1 border border-gray-300 rounded"
                    placeholder="Username"
                  />
                ) : (
                  device.camera.username
                )}
              </TableCell>
              <TableCell className="text-zinc-500">
                {editingId === device.camera.id ? (
                  <input
                    type="password"
                    name="password"
                    value={editedData.password}
                    onChange={handleChange}
                    className="w-full p-1 border border-gray-300 rounded"
                    placeholder="Password"
                  />
                ) : (
                  device.camera.password ? "••••••••" : ""
                )}
              </TableCell>
              <TableCell>
                {editingId === device.camera.id ? (
                  <div className="flex gap-2">
                    <button
                      onClick={() => saveChanges(device.camera.id)}
                      className="px-2 py-1 bg-green-500 text-white rounded text-sm"
                    >
                      <CheckIcon className="h-4 w-4" />
                    </button>
                    <button
                      onClick={cancelEditing}
                      className="px-2 py-1 bg-gray-300 rounded text-sm"
                    >
                      <XMarkIcon className="h-4 w-4" />
                    </button>
                  </div>
                ) : (
                  <div className="flex gap-2">
                    <button
                      onClick={() => startEditing(device)}
                      className="p-1 text-blue-500 hover:text-blue-700"
                      title="Edit"
                    >
                      <PencilIcon className="h-4 w-4" />
                    </button>
                    <button
                      onClick={() => deleteCamera(device.camera.id)}
                      className="p-1 text-red-500 hover:text-red-700"
                      title="Delete"
                    >
                      <TrashIcon className="h-4 w-4" />
                    </button>
                  </div>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
