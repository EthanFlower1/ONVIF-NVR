import React, { useState } from 'react';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../components/table';
import { Button } from '../components/button';
import { Heading } from '../components/heading';
import { Input } from '../components/input';
import { Spinner } from '../components/spinner';
import { Text } from '../components/text';

export default function DeviceDiscovery() {
  const [data, setData] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [connectingIds, setConnectingIds] = useState<string[]>([]);
  const [error, setError] = useState<any>(null);
  const [credentials, setCredentials] = useState<{ [key: string]: { username: string, password: string } }>({});

  const fetchData = async () => {
    setLoading(true);
    try {
      const response = await fetch('http://localhost:4750/api/cameras/discover', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
      });
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const json = await response.json();
      console.log("Response: ", json);
      setData(json);

      // Initialize credentials state for each camera
      const initialCredentials = {} as { [key: string]: { username: string, password: string } };
      json.forEach((device: any) => {
        initialCredentials[device.id] = { username: '', password: '' };
      });
      setCredentials(initialCredentials);

    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  };

  const handleCredentialChange = (deviceId: string, field: 'username' | 'password', value: string) => {
    setCredentials(prev => ({
      ...prev,
      [deviceId]: {
        ...prev[deviceId],
        [field]: value
      }
    }));
  };

  const handleConnect = async (device: any) => {
    console.log("Device: ", device)
    setConnectingIds(prev => [...prev, device.id]);
    try {
      const response = await fetch(`http://localhost:4750/api/cameras/connect`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          username: credentials[device.id].username,
          password: credentials[device.id].password,
          ip_address: device.ip_address,
        }),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const result = await response.json();
      console.log("Authentication result:", result);
      // Update the device in the data array to show it's connected
      setData(prev =>
        prev.map(device =>
          device.id === device.id
            ? { ...device, status: 'connected' }
            : device
        )
      );

    } catch (error) {
      console.error("Failed to connect to camera:", error);
      // Handle connection error
    } finally {
      setConnectingIds(prev => prev.filter(id => id !== device.id));
    }
  };

  return (
    <div>
      <div className="flex items-end justify-between gap-4">
        <Heading>Discover Devices</Heading>
        <Button
          onClick={() => fetchData()}
          disabled={loading}
        >
          {loading ? (
            <>
              <Spinner className="mr-2" size="sm" />
              Discovering...
            </>
          ) : (
            'Discover'
          )}
        </Button>
      </div>

      {loading ? (
        <div className="flex justify-center items-center h-64">
          <Spinner size="lg" />
          <Text className="ml-3 text-lg">Discovering cameras...</Text>
        </div>
      ) : data.length > 0 ? (
        <Table className="mt-8 [--gutter:--spacing(6)] lg:[--gutter:--spacing(10)]">
          <TableHead>
            <TableRow>
              <TableHeader>Device Name</TableHeader>
              <TableHeader>Address</TableHeader>
              <TableHeader>Username</TableHeader>
              <TableHeader>Password</TableHeader>
              <TableHeader>Actions</TableHeader>
            </TableRow>
          </TableHead>
          <TableBody>
            {data.map((device) => (
              <TableRow key={device.id}>
                <TableCell>{device.name}</TableCell>
                <TableCell className="text-zinc-500">{device.ip_address}</TableCell>
                <TableCell>
                  <Input
                    type="text"
                    value={credentials[device.id]?.username || ''}
                    onChange={(e) => handleCredentialChange(device.id, 'username', e.target.value)}
                    placeholder="Username"
                  />
                </TableCell>
                <TableCell>
                  <Input
                    type="password"
                    value={credentials[device.id]?.password || ''}
                    onChange={(e) => handleCredentialChange(device.id, 'password', e.target.value)}
                    placeholder="Password"
                  />
                </TableCell>
                <TableCell>
                  <Button
                    onClick={() => handleConnect(device)}
                    disabled={connectingIds.includes(device.id) || device.status === 'connected'}
                    className="whitespace-nowrap"
                  >
                    {connectingIds.includes(device.id) ? (
                      <>
                        <Spinner className="mr-2" size="sm" />
                        Connecting...
                      </>
                    ) : device.status === 'connected' ? (
                      'Connected'
                    ) : (
                      'Connect'
                    )}
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      ) : error ? (
        <Text>
          Error discovering devices: {error.message}
        </Text>

      ) : (
        <Text className="mt-8 p-4 border rounded-lg text-zinc-500">
          No devices discovered yet. Click the Discover button to scan your network.
        </Text>
      )}
    </div>
  );
}
