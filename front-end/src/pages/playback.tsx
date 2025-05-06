import React, { useState } from 'react'


export default function Playback() {
  const [data, setData] = useState<any[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<any>(null);

  return (
    <>
      <div>
        <video />
      </div>
    </>

  )
}
