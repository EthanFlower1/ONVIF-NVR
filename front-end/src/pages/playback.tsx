// SimplifiedHlsPlayer.tsx
import React, { useEffect, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import Hls from 'hls.js';
import type { ErrorData } from 'hls.js';

const SimplifiedHlsPlayer: React.FC = () => {
  const [searchParams] = useSearchParams();
  const selectedCameraId = searchParams.get('camera_id') || '';
  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<Hls | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [debugInfo, setDebugInfo] = useState<string | null>(null);
  const [showDebug, setShowDebug] = useState(false);
  const recoveryAttemptsRef = useRef(0);
  const maxRecoveryAttempts = 10; // Increased max attempts to allow for HLS preparation

  // Clean up function
  const destroyPlayer = () => {
    if (hlsRef.current) {
      hlsRef.current.destroy();
      hlsRef.current = null;
    }
  };

  useEffect(() => {
    let mounted = true;
    recoveryAttemptsRef.current = 0;

    const initPlayer = async () => {
      if (!selectedCameraId) {
        setError("No camera selected");
        setIsLoading(false);
        return;
      }

      setIsLoading(true);
      setError(null);
      destroyPlayer();

      try {
        // Get recording ID from URL params (if any)
        const recordingId = searchParams.get('recording_id');
        
        // The HLS stream URLs - use the API endpoint format first for live, playback endpoints for recordings
        const apiUrl = `http://localhost:4750/api/cameras/${selectedCameraId}/hls?playlist_type=master`;
        
        // Playback URLs based on whether we have a recording ID
        const playbackUrl = recordingId ? 
          `http://localhost:4750/playback/${recordingId}/hls?playlist_type=master` : 
          `http://localhost:4750/playback/cameras/${selectedCameraId}/hls?playlist_type=master`;
          
        const variantUrl = recordingId ? 
          `http://localhost:4750/playback/${recordingId}/hls?playlist_type=variant` : 
          `http://localhost:4750/playback/cameras/${selectedCameraId}/hls?playlist_type=variant`;
        
        // First try the most appropriate URL based on whether it's a recording or live
        const hlsUrl = recordingId ? playbackUrl : apiUrl;

        // First verify the playlist is accessible - try both URLs if needed
        let effectiveUrl = hlsUrl;
        let playlistContent = "";
        
        try {
          // Try the API URL first
          console.log("Trying API URL:", apiUrl);
          const apiResponse = await fetch(apiUrl);
          
          if (apiResponse.ok) {
            effectiveUrl = apiUrl;
            playlistContent = await apiResponse.text();
            console.log("Successfully fetched from API URL");
          } else {
            // If API URL fails, try the playback URL
            console.log("API URL failed, trying playback URL:", playbackUrl);
            const playbackResponse = await fetch(playbackUrl);
            
            if (playbackResponse.ok) {
              effectiveUrl = playbackUrl;
              playlistContent = await playbackResponse.text();
              console.log("Successfully fetched from playback URL");
            } else {
              throw new Error(`Both API and playback URLs failed to fetch HLS playlist`);
            }
          }
          
          if (!mounted) return;
          
          if (showDebug) {
            setDebugInfo(`Using URL: ${effectiveUrl}\nMaster playlist content: ${playlistContent}`);
          }
        } catch (err) {
          console.error('Error fetching playlist:', err);
          if (!mounted) return;
          
          // Check if it might be HLS preparation in progress
          const errorMsg = err instanceof Error ? err.message : String(err);
          if (errorMsg.includes("failed to fetch") || errorMsg.includes("404")) {
            setDebugInfo(`HLS content may still be preparing. Please wait a moment and try again.
              Error details: ${errorMsg}`);
            setError("HLS preparation in progress. The system is preparing your video stream. Please wait a moment and try again.");
          } else {
            setDebugInfo(`Error fetching playlist: ${errorMsg}`);
          }
        }

        // Check if HLS.js is supported
        if (Hls.isSupported() && videoRef.current) {
          const hls = new Hls({
            debug: false,
            enableWorker: true,
            lowLatencyMode: false,
            fragLoadingMaxRetry: 15,
            manifestLoadingMaxRetry: 15,
            levelLoadingMaxRetry: 15,
            fragLoadingRetryDelay: 500,
            manifestLoadingRetryDelay: 500,
            levelLoadingRetryDelay: 500,
            // This is key for handling codec issues
            preferManagedMediaSource: false,
            // Increase buffer size to handle inconsistencies
            maxBufferLength: 60000,
            maxMaxBufferLength: 1200
          });

          // Setup event handlers
          hls.on(Hls.Events.MEDIA_ATTACHED, () => {
            console.log('HLS media attached, loading source:', effectiveUrl);
            hls.loadSource(effectiveUrl);
          });

          hls.on(Hls.Events.MANIFEST_PARSED, (_, data) => {
            console.log('HLS manifest parsed, found ' + data.levels.length + ' quality levels');
            if (mounted) {
              setIsLoading(false);
            }
          });

          const handleError = (_: any, data: ErrorData) => {
            if (!mounted) return;

            console.error('HLS error:', data.type, data.details);

            if (showDebug) {
              setDebugInfo(prev => `${prev || ''}\nHLS error: ${data.type} - ${data.details}`);
            }

            if (data.fatal) {
              switch (data.type) {
                case Hls.ErrorTypes.NETWORK_ERROR:
                  console.log('Fatal network error, trying to recover...');
                  hls.startLoad();
                  break;

                case Hls.ErrorTypes.MEDIA_ERROR:
                  console.log('Fatal media error, trying to recover...');

                  // If we get a codec error, try different approaches
                  if (data.details === Hls.ErrorDetails.BUFFER_ADD_CODEC_ERROR ||
                    data.details === Hls.ErrorDetails.BUFFER_INCOMPATIBLE_CODECS_ERROR ||
                    data.details === Hls.ErrorDetails.FRAG_PARSING_ERROR) {

                    recoveryAttemptsRef.current++;

                    if (recoveryAttemptsRef.current <= maxRecoveryAttempts) {
                      console.log(`Recovery attempt ${recoveryAttemptsRef.current}/${maxRecoveryAttempts}`);

                      // On codec errors, try recreating the player completely
                      destroyPlayer();

                      // Short delay before trying again
                      // Wait longer between recovery attempts to allow HLS preparation to occur
                      const retryDelay = Math.min(1000 * (recoveryAttemptsRef.current + 1), 5000);
                      console.log(`Waiting ${retryDelay}ms before retry attempt ${recoveryAttemptsRef.current}`);
                      
                      setTimeout(() => {
                        if (mounted && videoRef.current) {
                          const newHls = new Hls({
                            debug: false,
                            enableWorker: true,
                            lowLatencyMode: false,
                            // Increase retry limits to handle HLS preparation
                            fragLoadingMaxRetry: 20,
                            manifestLoadingMaxRetry: 20,
                            levelLoadingMaxRetry: 20,
                            fragLoadingRetryDelay: 500,
                            manifestLoadingRetryDelay: 500,
                            preferManagedMediaSource: false,
                            // Try both URLs
                            ...((recoveryAttemptsRef.current % 2 === 0) ? {} : { startLevel: -1 })
                          });

                          newHls.attachMedia(videoRef.current);
                          newHls.on(Hls.Events.MEDIA_ATTACHED, () => {
                            // Try different URLs in sequence during recovery
                            let url;
                            const attemptNum = recoveryAttemptsRef.current;
                            if (attemptNum % 3 === 0) {
                              url = apiUrl;
                              console.log("Recovery: trying API URL");
                            } else if (attemptNum % 3 === 1) {
                              url = playbackUrl;
                              console.log("Recovery: trying playback URL");
                            } else {
                              url = variantUrl;
                              console.log("Recovery: trying variant URL");
                            }
                            newHls.loadSource(url);
                          });

                          // Same event handlers as before
                          newHls.on(Hls.Events.MANIFEST_PARSED, () => {
                            if (mounted) setIsLoading(false);
                          });

                          newHls.on(Hls.Events.ERROR, handleError);

                          hlsRef.current = newHls;
                        }
                      }, retryDelay);
                      return;
                    }
                  } else {
                    // For non-codec media errors, try standard recovery
                    hls.recoverMediaError();
                  }
                  break;

                default:
                  if (recoveryAttemptsRef.current < maxRecoveryAttempts) {
                    recoveryAttemptsRef.current++;
                    console.log(`General recovery attempt ${recoveryAttemptsRef.current}/${maxRecoveryAttempts}`);

                    // Try to restart with different URL
                    destroyPlayer();
                    // Use exponential backoff to give HLS preparation time
                    const retryDelay = Math.min(1000 * (recoveryAttemptsRef.current + 1), 5000);
                    console.log(`General recovery: waiting ${retryDelay}ms before retry attempt ${recoveryAttemptsRef.current}`);
                    setTimeout(() => initPlayer(), retryDelay);
                  } else {
                    console.log('Unrecoverable error');
                    if (mounted) {
                      setError(`Playback error: ${data.details}. This may indicate the HLS stream is still being prepared. Please wait a moment and try again.`);
                      destroyPlayer();
                    }
                  }
                  break;
              }
            }
          };

          hls.on(Hls.Events.ERROR, handleError);
          hls.attachMedia(videoRef.current);
          hlsRef.current = hls;
        }
        // For Safari which has native HLS support
        else if (videoRef.current && videoRef.current.canPlayType('application/vnd.apple.mpegurl')) {
          videoRef.current.src = hlsUrl;
          videoRef.current.addEventListener('loadedmetadata', () => {
            if (mounted) {
              setIsLoading(false);
            }
          });

          videoRef.current.addEventListener('error', () => {
            if (!mounted) return;

            const errorMessage = videoRef.current?.error?.message || 'Unknown error';
            console.error('Video error:', errorMessage);
            setError(`Video playback error: ${errorMessage}`);
            setIsLoading(false);
          });
        } else {
          throw new Error('HLS playback is not supported in this browser');
        }
      } catch (err) {
        console.error('Error initializing player:', err);
        if (mounted) {
          setError(`Failed to initialize player: ${err instanceof Error ? err.message : 'Unknown error'}`);
          setIsLoading(false);
        }
      }
    };

    initPlayer();

    return () => {
      mounted = false;
      destroyPlayer();
    };
  }, [selectedCameraId, showDebug]);

  return (
    <div className="relative w-full h-full">
      {isLoading && (
        <div className="absolute inset-0 flex items-center justify-center bg-black bg-opacity-50 z-10">
          <span className="text-white">Loading video...</span>
        </div>
      )}

      {error && (
        <div className="absolute inset-0 flex flex-col items-center justify-center bg-black bg-opacity-70 z-10">
          <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded max-w-md">
            <strong className="font-bold">Error:</strong>
            <span className="block sm:inline"> {error}</span>
          </div>

          {debugInfo && showDebug && (
            <div className="mt-4 bg-gray-800 text-green-400 p-4 rounded max-w-lg max-h-60 overflow-auto">
              <pre className="text-xs whitespace-pre-wrap">{debugInfo}</pre>
            </div>
          )}

          <div className="mt-4 flex gap-2">
            <button
              className="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
              onClick={() => {
                setError("Retrying... HLS preparation may take a moment.");
                // First destroy any existing player
                destroyPlayer();
                
                // Wait a bit to allow HLS preparation to complete
                setTimeout(() => {
                  // Reset recovery attempts
                  recoveryAttemptsRef.current = 0;
                  // Reinitialize player
                  initPlayer();
                }, 3000);
              }}
            >
              Retry
            </button>

            <button
              className="bg-gray-500 hover:bg-gray-700 text-white font-bold py-2 px-4 rounded"
              onClick={() => setShowDebug(!showDebug)}
            >
              {showDebug ? 'Hide Debug Info' : 'Show Debug Info'}
            </button>
          </div>
        </div>
      )}

      <video
        ref={videoRef}
        className="w-full h-full bg-black"
        controls
        playsInline
      />

      {!error && (
        <button
          className="absolute top-4 right-4 bg-gray-800 text-white px-3 py-1 text-sm rounded opacity-50 hover:opacity-100 z-20"
          onClick={() => setShowDebug(!showDebug)}
        >
          {showDebug ? 'Hide Debug' : 'Show Debug'}
        </button>
      )}

      {!error && debugInfo && showDebug && (
        <div className="absolute bottom-16 right-0 bg-black bg-opacity-75 text-green-400 p-2 rounded-tl-md max-w-2xl max-h-96 overflow-auto z-10">
          <pre className="text-xs whitespace-pre-wrap">{debugInfo}</pre>
        </div>
      )}
      
      {/* Add HLS Debug Button */}
      {videoRef.current && !error && (
        <button
          className="absolute top-16 right-4 bg-gray-800 text-white px-3 py-1 text-sm rounded opacity-50 hover:opacity-100 z-20"
          onClick={async () => {
            try {
              let apiPlaylist = "Failed to fetch";
              let playbackPlaylist = "Failed to fetch";
              let variantPlaylist = "Failed to fetch";
              
              // Try API URL
              try {
                const apiUrl = `http://localhost:4750/api/cameras/${selectedCameraId}/hls?playlist_type=master`;
                const apiResponse = await fetch(apiUrl);
                if (apiResponse.ok) {
                  apiPlaylist = await apiResponse.text();
                } else {
                  apiPlaylist = `Failed: ${apiResponse.status} ${apiResponse.statusText}`;
                }
              } catch (error) {
                apiPlaylist = `Error: ${error}`;
              }
              
              // Try playback URL
              try {
                const playbackUrl = `http://localhost:4750/playback/cameras/${selectedCameraId}/hls?playlist_type=master`;
                const playbackResponse = await fetch(playbackUrl);
                if (playbackResponse.ok) {
                  playbackPlaylist = await playbackResponse.text();
                } else {
                  playbackPlaylist = `Failed: ${playbackResponse.status} ${playbackResponse.statusText}`;
                }
              } catch (error) {
                playbackPlaylist = `Error: ${error}`;
              }
              
              // Try variant URL
              try {
                const variantUrl = `http://localhost:4750/playback/cameras/${selectedCameraId}/hls?playlist_type=variant`;
                const variantResponse = await fetch(variantUrl);
                if (variantResponse.ok) {
                  variantPlaylist = await variantResponse.text();
                } else {
                  variantPlaylist = `Failed: ${variantResponse.status} ${variantResponse.statusText}`;
                }
              } catch (error) {
                variantPlaylist = `Error: ${error}`;
              }
              
              // Display all playlists and URLs
              setDebugInfo(`
API URL Master Playlist:
${apiPlaylist}

Playback URL Master Playlist:
${playbackPlaylist}

Variant Playlist:
${variantPlaylist}

Current video status:
- Current video.src: ${videoRef.current.src}
- Player state: ${videoRef.current.paused ? 'Paused' : 'Playing'}
              `);
              setShowDebug(true);
            } catch (err) {
              setDebugInfo(`Error fetching HLS playlists: ${err}`);
              setShowDebug(true);
            }
          }}
        >
          Debug HLS
        </button>
      )}
    </div>
  );
};

export default SimplifiedHlsPlayer;
