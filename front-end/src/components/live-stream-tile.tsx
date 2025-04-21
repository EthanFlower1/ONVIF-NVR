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
  // Add a debug log on component initialization
  console.log(`WebRTCStreamPlayer initializing with streamId: ${streamId} at ${new Date().toISOString()}`);

  const videoRef = useRef<HTMLVideoElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const [status, setStatus] = useState('Disconnected');
  const [stats, setStats] = useState<StreamStats>({
    resolution: '--',
    dataRate: '0 KB/s',
    fps: '0 FPS',
    latency: '--'
  });

  // Use refs to store connection objects so they persist between renders
  const peerConnectionRef = useRef<RTCPeerConnection | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const statsIntervalRef = useRef<NodeJS.Timeout | null>(null);
  const lastBytesRef = useRef<number>(0);
  const lastStatsTimeRef = useRef<number>(Date.now());
  const iceCandidateQueueRef = useRef<RTCIceCandidate[]>([]);
  const peerConnectionReadyRef = useRef<boolean>(false);
  const mountedRef = useRef<boolean>(false);

  // Cleanup function to close connections properly
  const cleanup = () => {
    console.log('Cleaning up WebRTC connections');

    if (statsIntervalRef.current) {
      clearInterval(statsIntervalRef.current);
      statsIntervalRef.current = null;
    }

    // Important: Close peer connection before notifying server
    if (peerConnectionRef.current) {
      peerConnectionRef.current.close();
      peerConnectionRef.current = null;
    }

    // Now notify server *after* closing local connection
    if (sessionIdRef.current) {
      console.log(`Explicitly closing session ${sessionIdRef.current} on server`);
      fetch(`${serverUrl}/webrtc/close/${sessionIdRef.current}`, {
        method: 'GET',
        // Add timeout to prevent hanging requests
        signal: AbortSignal.timeout(2000)
      }).catch(error => console.error('Failed to close session, but continuing cleanup:', error));
      sessionIdRef.current = null;
    }

    // Reset state
    peerConnectionReadyRef.current = false;
    iceCandidateQueueRef.current = [];
    lastBytesRef.current = 0;
    lastStatsTimeRef.current = Date.now();

    // Clear video and audio elements
    if (videoRef.current) {
      videoRef.current.srcObject = null;
    }

    if (audioRef.current) {
      audioRef.current.srcObject = null;
    }
  };

  const updateStats = async () => {
    if (!peerConnectionRef.current || !mountedRef.current) return;

    try {
      const stats = await peerConnectionRef.current.getStats();
      let inboundRtpStats: any | null = null;
      let videoStats: any | null = null;

      stats.forEach((stat: any) => {
        if (stat.type === "inbound-rtp" && stat.kind === "video") {
          inboundRtpStats = stat;
        } else if (stat.type === "track" && stat.kind === "video") {
          videoStats = stat;
        }
      });

      if (inboundRtpStats && mountedRef.current) {
        const now = Date.now();
        const elapsedSec = (now - lastStatsTimeRef.current) / 1000;

        if (inboundRtpStats.bytesReceived !== undefined) {
          const newBytes = inboundRtpStats.bytesReceived;
          const bytesPerSec = (newBytes - lastBytesRef.current) / elapsedSec;
          const kbPerSec = Math.round(bytesPerSec / 1024);

          if (inboundRtpStats.framesPerSecond !== undefined) {
            setStats(prevStats => ({
              ...prevStats,
              dataRate: `${kbPerSec} KB/s`,
              fps: `${Math.round(inboundRtpStats.framesPerSecond)} FPS`
            }));
          }

          lastBytesRef.current = newBytes;
        }

        if (inboundRtpStats.jitter !== undefined) {
          const jitterMs = Math.round(inboundRtpStats.jitter * 1000);
          setStats(prevStats => ({
            ...prevStats,
            latency: `${jitterMs} ms`
          }));
        }

        lastStatsTimeRef.current = now;
      }

      if (videoStats && videoStats.frameWidth && videoStats.frameHeight && mountedRef.current) {
        setStats(prevStats => ({
          ...prevStats,
          resolution: `${videoStats.frameWidth}x${videoStats.frameHeight}`
        }));
      }
    } catch (e) {
      console.error("Error getting stats:", e);
    }
  };

  // Function to send ICE candidates
  const sendIceCandidate = async (candidate: RTCIceCandidate) => {
    if (!sessionIdRef.current) return;

    try {
      await fetch(`${serverUrl}/webrtc/ice`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          session_id: sessionIdRef.current,
          candidate: candidate.candidate,
          sdp_mid: candidate.sdpMid,
          sdp_mline_index: candidate.sdpMLineIndex
        })
      });
      console.log('ICE candidate sent to server');
    } catch (error) {
      console.error('Failed to send ICE candidate:', error);
      return false;
    }
    return true;
  };

  // Function to process queued ICE candidates
  const processIceCandidateQueue = async () => {
    console.log(`Processing queued ICE candidates: ${iceCandidateQueueRef.current.length}`);
    if (iceCandidateQueueRef.current.length > 0 && peerConnectionReadyRef.current) {
      const queue = [...iceCandidateQueueRef.current];
      iceCandidateQueueRef.current = [];

      for (const candidate of queue) {
        const success = await sendIceCandidate(candidate);
        if (!success && mountedRef.current) {
          // If sending fails, put it back in the queue
          iceCandidateQueueRef.current.push(candidate);
        }
      }
    }
  };

  // Also update the connect function to ensure we handle the connection correctly
  const connect = async () => {

    // Add a small delay after cleanup to ensure server has processed the cleanup
    await new Promise(resolve => setTimeout(resolve, 300));

    if (!mountedRef.current) return;

    try {
      setStatus('Connecting...');

      // Step 1: Create a WebRTC session
      console.log(`Creating session for stream ${streamId}`);
      const sessionResponse = await fetch(`${serverUrl}/webrtc/session`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ stream_id: streamId })
      });

      if (!sessionResponse.ok) {
        const errorText = await sessionResponse.text();
        console.error('Session response not OK:', sessionResponse.status, errorText);
        throw new Error(`Failed to create session: ${sessionResponse.status}`);
      }

      if (!mountedRef.current) return; // Check if still mounted

      const sessionData = await sessionResponse.json();
      console.log('Session created:', sessionData.session_id);
      sessionIdRef.current = sessionData.session_id;

      // Step 2: Create peer connection with only IPv4 STUN servers if possible
      // This helps avoid the IPv6 binding issues in the logs
      const configuration: RTCConfiguration = {
        iceServers: sessionData.ice_servers.map((server: WebRTCIceServer) => ({
          urls: server.urls,
          username: server.username,
          credential: server.credential
        })),
        // Prefer relay to help with NAT traversal issues
        iceTransportPolicy: 'all'
      };

      console.log('Creating RTCPeerConnection');
      const pc = new RTCPeerConnection(configuration);
      peerConnectionRef.current = pc;

      // Set up event handlers
      pc.onicecandidate = async (event: RTCPeerConnectionIceEvent) => {
        if (event.candidate) {
          console.log('ICE candidate:', event.candidate.candidate.substring(0, 50) + '...');

          if (!peerConnectionReadyRef.current || !sessionIdRef.current) {
            console.log('Queuing ICE candidate (connection not ready)');
            iceCandidateQueueRef.current.push(event.candidate);
            return;
          }

          await sendIceCandidate(event.candidate);
        }
      };

      pc.ontrack = (event: RTCTrackEvent) => {
        console.log('Track received:', event.track.kind);

        if (!mountedRef.current) return;

        if (event.track.kind === "video" && videoRef.current) {
          console.log('Setting video track to element');
          videoRef.current.srcObject = event.streams[0];

          // We already have video event handlers set up in the video-specific useEffect
          // No need to set additional handlers here
        }

        if (event.track.kind === "audio" && audioRef.current) {
          console.log('Setting audio track to element');
          audioRef.current.srcObject = event.streams[0];
        }
      };

      pc.onconnectionstatechange = () => {
        if (!pc || !mountedRef.current) return;

        const state = pc.connectionState;
        console.log('Connection state changed:', state);

        if (state === "connected") {
          setStatus('Connected');

          // Ensure the video plays when the connection is established
          if (videoRef.current && videoRef.current.paused && videoRef.current.readyState >= 2) {
            videoRef.current.play().catch(err => console.warn('Auto-play on connection failed:', err.name));
          }
        } else if (state === "disconnected" || state === "failed" || state === "closed") {
          setStatus(`Disconnected (${state})`);
        }
      };

      pc.oniceconnectionstatechange = () => {
        if (!pc) return;
        console.log('ICE connection state:', pc.iceConnectionState);
      };

      pc.onicegatheringstatechange = () => {
        if (!pc) return;
        console.log('ICE gathering state:', pc.iceGatheringState);
      };

      if (!mountedRef.current) return; // Check if still mounted

      // Step 3: Create offer with specific constraints
      console.log('Creating offer');
      // Using the legacy format for compatibility with older servers
      const offerOptions = {
        offerToReceiveAudio: true,
        offerToReceiveVideo: true
      };

      const offer = await pc.createOffer(offerOptions);
      console.log('Offer created:', {
        sdpType: offer.type,
        sdpLength: offer.sdp.length
      });

      if (!mountedRef.current) return; // Check if still mounted

      // Step 4: Set local description
      console.log('Setting local description');
      await pc.setLocalDescription(offer);
      console.log('Local description set');

      if (!mountedRef.current) return; // Check if still mounted

      // Make sure the local description is actually set before proceeding
      // This helps avoid race conditions
      if (!pc.localDescription) {
        console.log('Waiting for local description to be set...');
        await new Promise<void>(resolve => {
          const checkLocalDesc = () => {
            if (pc.localDescription) {
              resolve();
            } else {
              setTimeout(checkLocalDesc, 50);
            }
          };
          checkLocalDesc();
        });
      }

      // Use the actual local description, which may have been modified by the browser
      const actualOffer = pc.localDescription;
      if (!actualOffer) {
        throw new Error('No local description available');
      }

      // Step 5: Send offer to server
      console.log('Sending offer to server:', {
        sessionId: sessionIdRef.current,
        streamId,
        sdpType: actualOffer.type,
        sdpLength: actualOffer.sdp.length
      });

      // Ensure we're sending the exact format the server expects
      const offerPayload = {
        session_id: sessionIdRef.current,
        stream_id: streamId,
        sdp: actualOffer.sdp,
        type_field: actualOffer.type
      };

      console.log('Offer payload structure:', Object.keys(offerPayload));

      const offerResponse = await fetch(`${serverUrl}/webrtc/offer`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(offerPayload)
      });

      if (!offerResponse.ok) {
        const errorText = await offerResponse.text();
        console.error('Offer response not OK:', offerResponse.status, errorText);
        throw new Error('Failed to send offer');
      }

      if (!mountedRef.current) return; // Check if still mounted

      const answerData = await offerResponse.json();
      console.log('Received answer from server:', {
        type: answerData.type_field,
        sdpLength: answerData.sdp.length
      });

      if (!mountedRef.current) return; // Check if still mounted

      // Step 6: Set remote description
      console.log('Setting remote description');
      const answer = new RTCSessionDescription({
        type: answerData.type_field,
        sdp: answerData.sdp
      });

      await pc.setRemoteDescription(answer);
      console.log('Remote description set');

      peerConnectionReadyRef.current = true;

      if (!mountedRef.current) return; // Check if still mounted

      // Step 7: Send queued ICE candidates
      await processIceCandidateQueue();

      if (!mountedRef.current) return; // Check if still mounted

      // Start stats updater
      if (statsIntervalRef.current) {
        clearInterval(statsIntervalRef.current);
      }
      statsIntervalRef.current = setInterval(updateStats, 1000);
      console.log('WebRTC connection setup complete');

    } catch (error) {
      console.error('Connection failed:', error);
      if (mountedRef.current) {
        setStatus('Connection Failed');
      }
      cleanup();
    }
  };

  // Simpler video playback handling
  const handleVideoClick = async () => {
    if (videoRef.current && videoRef.current.paused) {
      try {
        await videoRef.current.play();
        console.log('Video playback started by user interaction');
        setStatus('Connected (Playing)');
      } catch (error) {
        console.error('Failed to play video after click:', error);
      }
    }
  };

  // Make sure we're cleaning up the component properly on unmount
  useEffect(() => {
    console.log('Component mounting with streamId:', streamId);
    mountedRef.current = true;

    // Important: Add a small delay before connecting to ensure any previous
    // connections have been fully cleaned up on the server side
    const timer = setTimeout(() => {
      if (mountedRef.current) {
        connect();
      }
    }, 500);

    // Return cleanup function
    return () => {
      console.log('Component unmounting, cleaning up WebRTC connections');
      clearTimeout(timer);
      mountedRef.current = false;
      cleanup();
    };
  }, [streamId, serverUrl]); // Only re-run if streamId or serverUrl changes

  // Add separate effect for video-specific setup
  useEffect(() => {
    if (videoRef.current) {
      // Pre-configure video element for better autoplay
      const videoElement = videoRef.current;

      // Set up listeners before we even have a stream
      videoElement.oncanplay = () => {
        console.log('Video can play event triggered');
        if (videoElement.paused && mountedRef.current) {
          videoElement.play()
            .then(() => console.log('Video started playing from canplay event'))
            .catch(err => console.warn('Autoplay from canplay event failed:', err.name));
        }
      };

      videoElement.onplay = () => console.log('Video play event triggered');
      videoElement.onplaying = () => {
        console.log('Video playing event triggered');
        setStatus('Connected (Playing)');
      };

      // If the browser suspends the video for any reason
      videoElement.onsuspend = () => console.log('Video suspended');
      videoElement.onwaiting = () => console.log('Video waiting for data');

      return () => {
        // Clean up listeners
        videoElement.oncanplay = null;
        videoElement.onplay = null;
        videoElement.onplaying = null;
        videoElement.onsuspend = null;
        videoElement.onwaiting = null;
      };
    }
  }, []);  // Empty dependency array ensures this only runs once

  return (
    <div className="w-full h-full flex flex-col overflow-hidden">
      <div className="relative flex-grow bg-black">
        <video
          ref={videoRef}
          autoPlay
          playsInline
          muted // Mute video to allow autoplay in most browsers
          className="w-full h-full object-contain cursor-pointer"
          onClick={handleVideoClick}
        />
        <audio
          ref={audioRef}
          autoPlay
          playsInline
        />
        <div className="absolute top-2 left-2 bg-black bg-opacity-60 text-white px-2 py-1 rounded text-sm">
          {status}
        </div>
        {/* Simplified play button overlay */}
        {status === 'Click to play' && (
          <div
            className="absolute inset-0 flex items-center justify-center bg-black bg-opacity-40 cursor-pointer"
            onClick={handleVideoClick}
          >
            <div className="bg-white bg-opacity-80 text-black p-3 rounded-full">
              ▶️
            </div>
          </div>
        )}
      </div>
      <div className="flex justify-between p-1 bg-gray-800 text-white text-xs font-mono">
        <span>{stats.resolution}</span>
        <span>{stats.dataRate}</span>
        <span>{stats.fps}</span>
        <span>Jitter: {stats.latency}</span>
      </div>
    </div>
  );
};

export default WebRTCStreamPlayer;
