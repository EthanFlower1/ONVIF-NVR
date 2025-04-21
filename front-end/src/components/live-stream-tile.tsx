import React, { useEffect, useRef, useState } from 'react';

interface WebRTCStreamPlayerProps {
  streamId: string;
  serverUrl?: string;
}

interface StreamStats {
  resolution: string;
  dataRate: string;
  fps: string;
  latency: string;
}

interface WebRTCIceServer {
  urls: string[];
  username?: string;
  credential?: string;
}

const WebRTCStreamPlayer: React.FC<WebRTCStreamPlayerProps> = ({
  streamId,
  serverUrl = window.location.origin
}) => {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [status, setStatus] = useState('Disconnected');
  const [stats, setStats] = useState<StreamStats>({
    resolution: '--',
    dataRate: '0 KB/s',
    fps: '0 FPS',
    latency: '--'
  });

  useEffect(() => {
    let peerConnection: RTCPeerConnection | null = null;
    let sessionId: string | null = null;
    let statsInterval: NodeJS.Timeout | null = null;
    let lastBytes = 0;
    let lastStatsTime = Date.now();
    let iceCandidateQueue: RTCIceCandidate[] = [];
    let peerConnectionReady = false;

    const updateStats = async () => {
      if (!peerConnection) return;

      try {
        const stats = await peerConnection.getStats();
        let inboundRtpStats: any | null = null;  // Using 'any' since RTCInboundRtpStreamStats properties vary by browser
        let videoStats: any | null = null;

        stats.forEach((stat: any) => {
          if (stat.type === "inbound-rtp" && stat.kind === "video") {
            inboundRtpStats = stat;
          } else if (stat.type === "track" && stat.kind === "video") {
            videoStats = stat;
          }
        });

        if (inboundRtpStats) {
          const now = Date.now();
          const elapsedSec = (now - lastStatsTime) / 1000;

          if (inboundRtpStats.bytesReceived !== undefined) {
            const newBytes = inboundRtpStats.bytesReceived;
            const bytesPerSec = (newBytes - lastBytes) / elapsedSec;
            const kbPerSec = Math.round(bytesPerSec / 1024);

            if (inboundRtpStats.framesPerSecond !== undefined) {
              setStats(prevStats => ({
                ...prevStats,
                dataRate: `${kbPerSec} KB/s`,
                fps: `${Math.round(inboundRtpStats.framesPerSecond)} FPS`
              }));
            }

            lastBytes = newBytes;
          }

          if (inboundRtpStats.jitter !== undefined) {
            const jitterMs = Math.round(inboundRtpStats.jitter * 1000);
            setStats(prevStats => ({
              ...prevStats,
              latency: `${jitterMs} ms`
            }));
          }

          lastStatsTime = now;
        }

        if (videoStats && videoStats.frameWidth && videoStats.frameHeight) {
          setStats(prevStats => ({
            ...prevStats,
            resolution: `${videoStats.frameWidth}x${videoStats.frameHeight}`
          }));
        }
      } catch (e) {
        console.error("Error getting stats:", e);
      }
    };

    const connect = async () => {
      try {
        setStatus('Connecting...');

        // Step 1: Create a WebRTC session
        const sessionResponse = await fetch(`${serverUrl}/webrtc/session`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ stream_id: streamId })
        });
        const sessionData = await sessionResponse.json();
        sessionId = sessionData.session_id;

        // Step 2: Create peer connection
        const configuration: RTCConfiguration = {
          iceServers: sessionData.ice_servers.map((server: WebRTCIceServer) => ({
            urls: server.urls,
            username: server.username,
            credential: server.credential
          }))
        };

        peerConnection = new RTCPeerConnection(configuration);

        // Set up event handlers
        peerConnection.onicecandidate = async (event: RTCPeerConnectionIceEvent) => {
          if (event.candidate) {
            if (!peerConnectionReady || !sessionId) {
              iceCandidateQueue.push(event.candidate);
              return;
            }

            try {
              await fetch(`${serverUrl}/webrtc/ice`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                  session_id: sessionId,
                  candidate: event.candidate.candidate,
                  sdp_mid: event.candidate.sdpMid,
                  sdp_mline_index: event.candidate.sdpMLineIndex
                })
              });
            } catch (error) {
              iceCandidateQueue.push(event.candidate);
            }
          }
        };

        peerConnection.ontrack = (event: RTCTrackEvent) => {
          if (event.track.kind === "video" && videoRef.current) {
            videoRef.current.srcObject = event.streams[0];
          }
        };

        peerConnection.onconnectionstatechange = () => {
          if (!peerConnection) return;

          const state = peerConnection.connectionState;
          if (state === "connected") {
            setStatus('Connected');
          } else if (state === "disconnected" || state === "failed" || state === "closed") {
            setStatus(`Disconnected (${state})`);
          }
        };

        // Step 3: Create offer
        const offer = await peerConnection.createOffer({
          offerToReceiveVideo: true,
          offerToReceiveAudio: true
        });

        // Step 4: Set local description
        await peerConnection.setLocalDescription(offer);

        // Step 5: Send offer to server
        const offerResponse = await fetch(`${serverUrl}/webrtc/offer`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            session_id: sessionId,
            stream_id: streamId,
            sdp: offer.sdp,
            type_field: offer.type
          })
        });
        const answerData = await offerResponse.json();

        // Step 6: Set remote description
        const answer = new RTCSessionDescription({
          type: answerData.type_field,
          sdp: answerData.sdp
        });
        await peerConnection.setRemoteDescription(answer);

        peerConnectionReady = true;

        // Step 7: Send queued ICE candidates
        if (iceCandidateQueue.length > 0) {
          for (const candidate of iceCandidateQueue) {
            try {
              await fetch(`${serverUrl}/webrtc/ice`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                  session_id: sessionId,
                  candidate: candidate.candidate,
                  sdp_mid: candidate.sdpMid,
                  sdp_mline_index: candidate.sdpMLineIndex
                })
              });
            } catch (error) {
              console.error('Failed to send ICE candidate:', error);
            }
          }
          iceCandidateQueue = [];
        }

        // Start stats updater
        statsInterval = setInterval(updateStats, 1000);

      } catch (error) {
        console.error('Connection failed:', error);
        setStatus('Connection Failed');
        if (peerConnection) {
          peerConnection.close();
          peerConnection = null;
        }
      }
    };

    // Connect when component mounts
    connect();

    // Cleanup on unmount
    return () => {
      if (statsInterval) {
        clearInterval(statsInterval);
      }

      if (sessionId) {
        fetch(`${serverUrl}/webrtc/close/${sessionId}`, { method: 'GET' })
          .catch(error => console.error('Failed to close session:', error));
      }

      if (peerConnection) {
        peerConnection.close();
      }
    };
  }, [streamId, serverUrl]);

  return (
    <div className="flex flex-col bg-white rounded-lg shadow-lg overflow-hidden">
      <div className="relative aspect-video bg-black">
        <video
          ref={videoRef}
          autoPlay
          playsInline
          className="w-full h-full object-contain"
        />
        <div className="absolute top-2 left-2 bg-black bg-opacity-60 text-white px-2 py-1 rounded text-sm">
          {status}
        </div>
      </div>
      <div className="flex justify-between p-2 bg-gray-100 text-xs font-mono">
        <span>{stats.resolution}</span>
        <span>{stats.dataRate}</span>
        <span>{stats.fps}</span>
        <span>Jitter: {stats.latency}</span>
      </div>
    </div>
  );
};

export default WebRTCStreamPlayer;
