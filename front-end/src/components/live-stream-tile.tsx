import React, { useEffect, useRef, useState } from 'react';
import { TransformWrapper, TransformComponent } from "react-zoom-pan-pinch";
interface WebRTCStreamPlayerProps {
  streamId: string;
  serverUrl?: string;
  cameraName: string;
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
  cameraName,
  streamId,
  serverUrl = window.location.origin
}) => {
  // Add a debug log on component initialization

  const videoRef = useRef<HTMLVideoElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const [status, setStatus] = useState('Disconnected');
  const [stats, setStats] = useState<StreamStats>({
    resolution: '--',
    dataRate: '0 KB/s',
    fps: '0 FPS',
    latency: '--'
  });
  const [showStats, setShowStats] = useState(false);

  // Use refs to store connection objects so they persist between renders
  const peerConnectionRef = useRef<RTCPeerConnection | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const statsIntervalRef = useRef<NodeJS.Timeout | null>(null);
  const lastBytesRef = useRef<number>(0);
  const lastStatsTimeRef = useRef<number>(Date.now());
  const iceCandidateQueueRef = useRef<RTCIceCandidate[]>([]);
  const peerConnectionReadyRef = useRef<boolean>(false);
  const mountedRef = useRef<boolean>(false);

  // Enhanced cleanup function to close connections properly
  const cleanup = () => {
    console.log('Cleaning up WebRTC connections');

    // First, stop stats interval 
    if (statsIntervalRef.current) {
      clearInterval(statsIntervalRef.current);
      statsIntervalRef.current = null;
    }

    // Gather information about what's being cleaned up
    const hasActivePC = !!peerConnectionRef.current;
    const sessionId = sessionIdRef.current;

    // Important: Stop tracks and close peer connection before notifying server
    if (peerConnectionRef.current) {
      console.log('Closing peer connection and cleaning up tracks');

      try {
        // Get all senders and stop their tracks first
        const senders = peerConnectionRef.current.getSenders();
        senders.forEach(sender => {
          if (sender.track) {
            console.log(`Stopping track: ${sender.track.kind}`);
            sender.track.stop();
          }
        });

        // Close the RTCPeerConnection
        peerConnectionRef.current.close();
      } catch (err) {
        console.warn('Error during peer connection cleanup:', err);
      }

      peerConnectionRef.current = null;
    }

    // Now notify server *after* closing local connection
    if (sessionId) {
      console.log(`Explicitly closing session ${sessionId} on server`);

      // Use a more robust approach for cleanup
      fetch(`${serverUrl}/webrtc/close/${sessionId}`, {
        method: 'GET',
        // Add timeout to prevent hanging requests
        signal: AbortSignal.timeout(3000)
      })
        .then(response => {
          if (response.ok) {
            console.log(`Successfully closed session ${sessionId} on server`);
          } else {
            console.warn(`Server returned ${response.status} when closing session ${sessionId}`);
          }
        })
        .catch(error => console.error('Failed to close session, but continuing cleanup:', error));

      sessionIdRef.current = null;
    }

    // Reset state
    peerConnectionReadyRef.current = false;
    iceCandidateQueueRef.current = [];
    lastBytesRef.current = 0;
    lastStatsTimeRef.current = Date.now();

    // Clear video and audio elements
    if (videoRef.current) {
      if (videoRef.current.srcObject) {
        // Stop all tracks from the srcObject
        const stream = videoRef.current.srcObject as MediaStream;
        if (stream) {
          stream.getTracks().forEach(track => {
            console.log(`Stopping video track: ${track.kind}`);
            track.stop();
          });
        }
      }
      videoRef.current.srcObject = null;
    }

    if (audioRef.current) {
      if (audioRef.current.srcObject) {
        // Stop all tracks from the srcObject
        const stream = audioRef.current.srcObject as MediaStream;
        if (stream) {
          stream.getTracks().forEach(track => {
            console.log(`Stopping audio track: ${track.kind}`);
            track.stop();
          });
        }
      }
      audioRef.current.srcObject = null;
    }

    // Report cleanup status
    console.log(`Cleanup complete. Had active connection: ${hasActivePC}, Session ID: ${sessionId || 'none'}`);
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
    if (!sessionIdRef.current) {
      console.error('Cannot send ICE candidate: No session ID');
      return false;
    }

    try {
      const response = await fetch(`${serverUrl}/webrtc/ice`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          session_id: sessionIdRef.current,
          candidate: candidate.candidate,
          sdp_mid: candidate.sdpMid,
          sdp_mline_index: candidate.sdpMLineIndex
        })
      });

      if (!response.ok) {
        console.error(`ICE candidate send failed with status: ${response.status}`);
        return false;
      }

      const data = await response.json();
      if (!data.success) {
        console.error('Server rejected ICE candidate:', data.message || data.error);

        // If the peer connection wasn't found, this means we need to reconnect
        if (data.error === 'peer_connection_not_found') {
          console.warn('Server reports peer connection not found, reconnecting...');

          // Schedule a reconnect if the component is still mounted
          if (mountedRef.current) {
            cleanup();
            setTimeout(() => {
              if (mountedRef.current) {
                connect();
              }
            }, 1000);
          }
        }
        return false;
      }

      console.log('ICE candidate successfully sent to server');
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

  // Enhanced connect function to ensure we handle the connection correctly with better error handling
  const connect = async () => {
    // Clear any existing connection
    cleanup();

    // Add a small delay after cleanup to ensure server has processed the cleanup
    await new Promise(resolve => setTimeout(resolve, 500));

    if (!mountedRef.current) return;

    try {
      setStatus('Connecting...');

      // Setup connection timeout to prevent hanging in "Connecting..." state
      const connectionTimeout = setTimeout(() => {
        if (mountedRef.current && status === 'Connecting...') {
          console.warn('Connection attempt timed out after 15 seconds');
          setStatus('Connection Failed (timeout)');
          cleanup();
        }
      }, 15000);

      // Step 1: Create a WebRTC session
      console.log(`Creating session for stream ${streamId} at ${new Date().toISOString()}`);
      const sessionResponse = await fetch(`${serverUrl}/webrtc/session`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ stream_id: streamId }),
        // Add timeout to prevent hanging requests
        signal: AbortSignal.timeout(5000)
      });

      if (!sessionResponse.ok) {
        const errorText = await sessionResponse.text();
        console.error('Session response not OK:', sessionResponse.status, errorText);
        clearTimeout(connectionTimeout);
        throw new Error(`Failed to create session: ${sessionResponse.status}`);
      }

      if (!mountedRef.current) {
        clearTimeout(connectionTimeout);
        return; // Check if still mounted
      }

      const sessionData = await sessionResponse.json();
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

        if (!mountedRef.current) return;

        if (event.track.kind === "video" && videoRef.current) {
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

      // Using the legacy format for compatibility with older servers
      const offerOptions = {
        offerToReceiveAudio: true,
        offerToReceiveVideo: true
      };

      const offer = await pc.createOffer(offerOptions);
      // console.log('Offer created:', {
      //   sdpType: offer.type,
      //   sdpLength: offer.sdp.length
      // });

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
      // Ensure we're sending the exact format the server expects
      const offerPayload = {
        session_id: sessionIdRef.current,
        stream_id: streamId,
        sdp: actualOffer.sdp,
        type_field: actualOffer.type
      };


      const offerResponse = await fetch(`${serverUrl}/webrtc/offer`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(offerPayload),
        // Add timeout to prevent hanging requests
        signal: AbortSignal.timeout(8000)
      });

      // Clear the connection timeout since we got a response
      clearTimeout(connectionTimeout);

      if (!offerResponse.ok) {
        const errorText = await offerResponse.text();
        console.error('Offer response not OK:', offerResponse.status, errorText);

        // Log detailed error information for debugging
        console.warn(`Offer failed with status: ${offerResponse.status}, text: ${errorText}`);

        if (offerResponse.status === 500) {
          // Server error - likely an issue with the pipeline setup
          setStatus('Server Error');
        } else {
          setStatus(`Error (${offerResponse.status})`);
        }

        throw new Error(`Failed to send offer: ${offerResponse.status}`);
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
        setStatus('Connected');
      } catch (error) {
        console.error('Failed to play video after click:', error);
      }
    }
  };

  // Make sure we're cleaning up the component properly on unmount
  useEffect(() => {
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
        setStatus('Connected');
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

  // Get status indicator color
  const getStatusColor = () => {
    if (status.includes('Connected')) return 'bg-green-500';
    if (status === 'Connecting...') return 'bg-yellow-500';
    return 'bg-red-500';
  };

  // Determine if we should show the spinner
  const isConnecting = status === 'Connecting...';

  return (
    <div
      className="w-full h-full flex flex-col overflow-hidden relative"
      onMouseEnter={() => setShowStats(true)}
      onMouseLeave={() => setShowStats(false)}
    >
      <div className="relative flex-grow bg-black">
        <TransformWrapper>
          <TransformComponent>
            <video
              ref={videoRef}
              autoPlay
              playsInline
              muted // Mute video to allow autoplay in most browsers
              className="w-full h-full object-contain cursor-pointer"
              onClick={handleVideoClick}
            />
          </TransformComponent>
        </TransformWrapper>
        <audio
          ref={audioRef}
          autoPlay
          playsInline
        />


        {/* Stats overlays - all aligned on top right - only show when hovering */}
        {showStats && (
          <>
            {/* Status indicator with colored circle (top left) */}
            <div className="absolute top-2 left-2 bg-black bg-opacity-10 text-white px-2 py-0.5 rounded-full flex items-center space-x-1">
              <div className={`w-2 h-2 rounded-full ${getStatusColor()}`}></div>
              <span className="text-xs">{status}</span>

              {/* Loading spinner for connecting state */}
              {isConnecting && (
                <svg className="animate-spin ml-1 h-3 w-3 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
              )}
            </div>
            {/* Network stats group in top right corner, stacked vertically */}
            <div className="absolute top-2 right-2 flex flex-col space-y-1 items-end">
              {/* Resolution */}
              <div className="bg-black/60 text-white text-xs px-2 py-0.5 rounded-lg">
                {stats.resolution}
              </div>

              {/* Data rate */}
              <div className="bg-black/60 text-white text-xs px-2 py-0.5 rounded-lg">
                {stats.dataRate}
              </div>

              {/* FPS */}
              <div className="bg-black/60 text-white text-xs px-2 py-0.5 rounded-lg">
                {stats.fps}
              </div>

              {/* Latency */}
              <div className="bg-black/60 text-white text-xs px-2 py-0.5 rounded-lg">
                Jitter: {stats.latency}
              </div>
            </div>

            {/* Stream name overlay at bottom center */}
            <div className="absolute bottom-4 left-1/2 transform -translate-x-1/2 bg-black/60  text-white px-3 py-1 rounded-lg text-sm">
              {cameraName}
            </div>
          </>
        )}
      </div>
    </div>
  );
};

export default WebRTCStreamPlayer;
