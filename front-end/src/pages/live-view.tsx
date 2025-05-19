import React, { useEffect, useState } from "react";
import WebRTCStreamPlayer from "../components/live-stream-tile";

export default function Liveview() {
  const [data, setData] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<any>(null);

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

  // Calculate grid class based on number of cameras
  const getGridClass = (count: number) => {
    if (count <= 1) return 'grid-cols-1';
    if (count <= 4) return 'grid-cols-2';
    if (count <= 9) return 'grid-cols-3';
    if (count <= 16) return 'grid-cols-4';
    return 'grid-cols-5';
  };

  const streamCount = data.reduce((count, camera) =>
    count + (camera.streams.length > 0 ? 1 : 0), 0);

  return (
    <div className="h-full w-full p-4">
      {loading && (
        <div className="flex items-center justify-center h-full">
          <div className="text-gray-600">Loading cameras...</div>
        </div>
      )}

      {error && (
        <div className="flex items-center justify-center h-full">
          <div className="text-red-500">Error: {error.message}</div>
        </div>
      )}

      {!loading && !error && (
        <div
          className={`
            grid ${getGridClass(streamCount)} gap-4 h-full
            auto-rows-fr
          `}
        >
          {data.map((camera_with_stream) =>
            camera_with_stream.streams.map((stream, i) => {
              if (i !== 0) return null; // Only show first stream for each camera
              return (
                <div
                  key={stream.id}
                  className="relative aspect-video black rounded-lg overflow-hidden shadow-md"
                >
                  <WebRTCStreamPlayer
                    cameraName={camera_with_stream.camera.name}
                    streamId={stream.id}
                    serverUrl="http://localhost:4750"
                    stream={stream}
                  />
                </div>
              );
            })
          )}
        </div>
      )}

      {!loading && !error && streamCount === 0 && (
        <div className="flex items-center justify-center h-full">
          <div className="text-gray-600">No camera streams available</div>
        </div>
      )}
    </div>
  );
}
