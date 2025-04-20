import React, { useEffect, useState } from "react";

export default function Cameras() {
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
      setData(json);
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData()
  }, [])

  return (
    <div>
      {data}
    </div>
  )
}
