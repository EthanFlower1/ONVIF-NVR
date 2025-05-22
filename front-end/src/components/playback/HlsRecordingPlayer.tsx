import React, { useEffect, useRef, useState } from 'react';
import Hls from 'hls.js';
import {
  PlayIcon,
  PauseIcon,
  ForwardIcon,
  BackwardIcon,
  SpeakerWaveIcon,
  SpeakerXMarkIcon,
  ArrowPathIcon
} from '@heroicons/react/24/solid';
import { Badge } from '../badge';
import { Heading } from '../heading';
import { Text } from '../text';

interface HlsRecordingPlayerProps {
  recordingId?: string;
  cameraId?: string;
  cameraName?: string;
  startTime?: string;
  endTime?: string;
  eventType?: string;
  duration?: number;
  onClose?: () => void;
  apiBaseUrl?: string;
  serverUrl?: string;
  playerMode?: 'recording' | 'camera';
  currentSegment?: number;
  isSegmentedRecording?: boolean;
}

/**
 * A dedicated HLS player component built specifically for HLS streaming
 * Optimized for the on-the-fly HLS streaming backend
 */
const HlsRecordingPlayer: React.FC<HlsRecordingPlayerProps> = ({
  recordingId,
  cameraId,
  cameraName = 'Camera',
  startTime,
  endTime,
  eventType = 'continuous',
  duration = 0,
  onClose,
  apiBaseUrl = window.location.origin,
  serverUrl = 'http://localhost:4750',
  playerMode = 'recording',
  currentSegment,
  isSegmentedRecording = false
}) => {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [videoDuration, setVideoDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const [isMuted, setIsMuted] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [playbackRate, setPlaybackRate] = useState(1);
  const [hls, setHls] = useState<Hls | null>(null);
  const [hlsRetryCount, setHlsRetryCount] = useState(0);

  // Construct the correct ID to use for HLS playback based on mode
  const playbackId = playerMode === 'camera' && cameraId 
    ? `camera-${cameraId}` 
    : recordingId;

  // Make sure we have a valid ID for playback
  const isValidPlayback = playerMode === 'camera' ? !!cameraId : !!recordingId;
  
  // Construct HLS playlist URLs using the on-the-fly HLS endpoints with explicit server URL
  // Add segment parameter if we're playing a specific segment of a segmented recording
  const segmentParam = isSegmentedRecording && currentSegment !== undefined ? `&segment=${currentSegment}` : '';
  
  const masterPlaylistUrl = isValidPlayback 
    ? `${serverUrl}/hls/${playbackId}/playlist?playlist_type=master${segmentParam}` 
    : '';
    
  const mediaPlaylistUrl = isValidPlayback 
    ? `${serverUrl}/hls/${playbackId}/playlist?playlist_type=media${segmentParam}` 
    : '';

  // Use master playlist by default, but fall back to media playlist if needed
  const [hlsPlaylistUrl, setHlsPlaylistUrl] = useState(masterPlaylistUrl);

  // Debug recorded URLs and mode
  useEffect(() => {
    console.log(`Player mode: ${playerMode}`);
    console.log(`Playback ID: ${playbackId}`);
    console.log(`Is segmented recording: ${isSegmentedRecording}`);
    console.log(`Current segment: ${currentSegment !== undefined ? currentSegment : 'N/A'}`);
    console.log(`Master playlist URL: ${masterPlaylistUrl}`);
    console.log(`Media playlist URL: ${mediaPlaylistUrl}`);
  }, [playerMode, playbackId, masterPlaylistUrl, mediaPlaylistUrl, isSegmentedRecording, currentSegment]);
  
  // Validate that we have proper IDs
  useEffect(() => {
    if (!isValidPlayback) {
      console.error(`Invalid playback configuration: ${playerMode} mode requires ${playerMode === 'camera' ? 'cameraId' : 'recordingId'}`);
      setError(`Invalid playback configuration. ${playerMode === 'camera' ? 'Camera' : 'Recording'} ID is required.`);
    } else {
      // Clear error if previously set for this reason
      if (error === `Invalid playback configuration. ${playerMode === 'camera' ? 'Camera' : 'Recording'} ID is required.`) {
        setError(null);
      }
    }
  }, [playerMode, cameraId, recordingId, isValidPlayback, error]);
  
  // Effect to reload the player when the segment changes
  useEffect(() => {
    if (isSegmentedRecording && currentSegment !== undefined) {
      console.log(`Segment changed to: ${currentSegment}, reloading player`);
      
      // Reset loading state
      setIsLoading(true);
      setError(null);
      setHlsRetryCount(0);
      
      // Update playlist URL with new segment parameter
      setHlsPlaylistUrl(masterPlaylistUrl);
      
      // Clean up existing HLS instance
      if (hls) {
        hls.destroy();
        setHls(null);
      }
      
      // Reset video element
      if (videoRef.current) {
        videoRef.current.pause();
        videoRef.current.removeAttribute('src');
        videoRef.current.load();
      }
    }
  }, [currentSegment, isSegmentedRecording, masterPlaylistUrl, hls]);

  // Function to retry playback with different settings
  const retryWithDifferentSettings = () => {
    // If we've already tried too many times, show a persistent error message
    if (hlsRetryCount >= 3) {
      setError('Could not play this recording after multiple attempts. Please try again later or refresh the page.');
      return;
    }

    setIsLoading(true);
    setError(null);

    // Increment the retry counter
    setHlsRetryCount(prev => prev + 1);

    // Clean up existing HLS instance
    if (hls) {
      hls.destroy();
      setHls(null);
    }

    // Different strategies for different retry attempts
    if (hlsRetryCount === 0) {
      console.log('First retry: Switching to media playlist');
      setHlsPlaylistUrl(mediaPlaylistUrl);
    } else if (hlsRetryCount === 1) {
      console.log('Second retry: Using master playlist with different config');
      setHlsPlaylistUrl(masterPlaylistUrl);
    } else {
      console.log('Final retry: Using direct media segments with minimal config');
      // On third retry, go back to media playlist with most aggressive settings
      setHlsPlaylistUrl(mediaPlaylistUrl);

      // Also attempt to reset the video element state
      if (videoRef.current) {
        videoRef.current.pause();
        videoRef.current.removeAttribute('src');
        videoRef.current.load();
      }
    }
  };

  // Effect for initializing the HLS player
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // HLS.js initialization
    const setupHls = () => {
      setIsLoading(true);
      setError(null);

      // Clean up any existing HLS instance
      if (hls) {
        hls.destroy();
      }

      // Check if HLS.js is supported
      if (Hls.isSupported()) {
        console.log(`HLS.js is supported, initializing with URL: ${hlsPlaylistUrl} (attempt: ${hlsRetryCount + 1})`);
        console.log(`Recording ID: ${recordingId}, Server URL: ${serverUrl}`);

        const hlsConfig = {
          debug: true,  // Enable for troubleshooting
          enableWorker: true,
          lowLatencyMode: false,
          // Simpler configuration for MPEG-TS segments
          startLevel: 0,
          defaultAudioCodec: 'mp4a.40.2', // AAC-LC
          // MPEG-TS specific optimizations
          progressive: false, // Better for ts segments
          // Buffer settings
          maxBufferLength: 30,
          maxMaxBufferLength: 60,
          maxBufferSize: 30 * 1000 * 1000, // 30MB
          // More aggressive retry settings
          fragLoadingMaxRetry: 15,
          manifestLoadingMaxRetry: 15,
          levelLoadingMaxRetry: 15,
          fragLoadingRetryDelay: 500,
          manifestLoadingRetryDelay: 500,
          levelLoadingRetryDelay: 500,
          // Recovery options
          appendErrorMaxRetry: 10,  // Critical for bufferAppendError
          // Force certain modes
          capLevelToPlayerSize: true,
          // Use proper MIME type for content
          xhrSetup: function(xhr: XMLHttpRequest, url: string) {
            console.log(`Setting up XHR for URL: ${url}`);
            // Set appropriate MIME types for better compatibility
            if (url.includes('/init')) {
              xhr.overrideMimeType('video/mp4');
            } else if (url.includes('.ts') || url.includes('/segment')) {
              xhr.overrideMimeType('video/mp2t'); // Use MPEG-TS MIME type for segments
            } else if (url.includes('/playlist')) {
              xhr.overrideMimeType('application/vnd.apple.mpegurl');
            }
            // Add range request capability
            xhr.withCredentials = false;
          },
          // Special settings for MPEG-TS segments
          cmcd: false, // Don't add extra CMCD data
          startFragPrefetch: false, // Don't prefetch
          
          // Retry attempts have different strategies
          ...(hlsRetryCount === 1 && {
            autoStartLoad: true,
            startPosition: 0,
            // Switch to more aggressive TS handling
            abrEwmaDefaultEstimate: 1000000,
            testBandwidth: false,
            emeEnabled: false,
          }),
          
          // For the final retry attempt, use simplest possible settings
          ...(hlsRetryCount === 2 && {
            autoStartLoad: true,
            startPosition: 0,
            // Simplest possible config
            maxBufferLength: 5,
            maxMaxBufferLength: 10,
            liveSyncDurationCount: 1,
            // Force new MediaSource
            forceKeyFrameOnDiscontinuity: true,
            disableWebVTT: true,
            forceKeyFrameOnDiscontinuity: true, 
            abrEwmaDefaultEstimate: 1000000,
          })
        };

        const newHls = new Hls(hlsConfig);

        // Add event listeners
        newHls.on(Hls.Events.MEDIA_ATTACHED, () => {
          console.log('HLS media attached');
        });

        newHls.on(Hls.Events.MANIFEST_PARSED, (event, data) => {
          console.log('HLS manifest parsed, found ' + data.levels.length + ' quality level(s)');
          setIsLoading(false);

          // Auto-play when available
          if (isPlaying) {
            video.play().catch((error) => {
              console.warn('Error attempting to play:', error);
            });
          }
        });

        newHls.on(Hls.Events.FRAG_LOADING, () => {
          if (!isLoading) setIsLoading(true);
        });

        newHls.on(Hls.Events.FRAG_LOADED, () => {
          if (isLoading) setIsLoading(false);
        });

        newHls.on(Hls.Events.ERROR, (event, data) => {
          console.error('HLS error:', data);

          // Check if error is related to CORS issues (common when using different origin)
          const isCorsError = data.response && (
            data.response.code === 0 ||
            data.details === Hls.ErrorDetails.FRAG_LOAD_ERROR ||
            (data.response.text && data.response.text.includes('cors'))
          );

          if (isCorsError) {
            console.error('Detected possible CORS issue:', data);
            setError('Cross-origin (CORS) error detected. The server may not allow requests from this origin.');
          }
          
          // Special handling for segment-specific errors
          if (isSegmentedRecording && currentSegment !== undefined) {
            const isSegmentError = 
              data.details === Hls.ErrorDetails.FRAG_LOAD_ERROR ||
              (data.response && data.response.code === 404) ||
              data.details === Hls.ErrorDetails.MANIFEST_LOAD_ERROR;
              
            if (isSegmentError) {
              console.error(`Error loading segment ${currentSegment}:`, data);
              setError(`Unable to load segment ${currentSegment}. The segment may be corrupted or unavailable.`);
            }
          }

          // Handle buffer and codec errors with more resilience
          const isBufferError =
            data.details === Hls.ErrorDetails.BUFFER_APPEND_ERROR ||
            data.details === Hls.ErrorDetails.BUFFER_APPENDING_ERROR ||
            data.details === 'bufferAppendError' || // Direct string comparison for reliability
            data.details === 'bufferAddCodecError' ||
            data.details === Hls.ErrorDetails.BUFFER_INCOMPATIBLE_CODECS_ERROR;

          if (isBufferError) {
            console.error('HLS buffer error:', data.details);

            // For buffer errors, try more aggressive recovery steps
            if (hlsRetryCount < 2) {
              console.log('Buffer error detected, will retry with different settings');

              // First try a media recovery
              if (newHls.media) {
                try {
                  // Try to flush buffer and recover before full retry
                  newHls.recoverMediaError();

                  // If that didn't immediately trigger another error, we'll schedule a full retry
                  setTimeout(() => {
                    retryWithDifferentSettings();
                  }, 1000);
                } catch (e) {
                  console.error('Error during media recovery:', e);
                  retryWithDifferentSettings();
                }
              } else {
                // No media attached, just retry
                retryWithDifferentSettings();
              }
              return; // Return early to avoid showing the error message
            }
          }

          if (data.fatal) {
            switch (data.type) {
              case Hls.ErrorTypes.NETWORK_ERROR:
                console.error('HLS fatal network error:', data.details);

                // More specific error messages based on the error detail
                if (data.details === Hls.ErrorDetails.MANIFEST_LOAD_ERROR ||
                  data.details === Hls.ErrorDetails.MANIFEST_PARSING_ERROR) {
                  setError('Unable to load the video stream. The recording may be unavailable.');
                } else if (data.details === Hls.ErrorDetails.FRAG_LOAD_ERROR) {
                  setError('Error loading video segment. This may be a network issue or a CORS problem.');
                } else {
                  setError(`Network error: ${data.details}`);
                }

                // Try to recover with a reload
                try {
                  newHls.startLoad();
                } catch (e) {
                  console.error('Error during startLoad:', e);

                  // If that fails, try a full retry
                  if (hlsRetryCount < 2) {
                    setTimeout(() => retryWithDifferentSettings(), 1000);
                  }
                }
                break;

              case Hls.ErrorTypes.MEDIA_ERROR:
                console.error('HLS fatal media error:', data.details);

                // Handle all buffer-related errors with the same approach
                if (data.details === 'bufferAddCodecError' ||
                  data.details === 'bufferAppendingError' ||
                  data.details === 'bufferAppendError' ||
                  data.details === Hls.ErrorDetails.BUFFER_APPEND_ERROR) {

                  if (hlsRetryCount < 2) {
                    console.log('Buffer codec error detected, will retry with different settings');

                    // Try media recovery first
                    try {
                      newHls.recoverMediaError();

                      // If successful, still schedule a retry to be safe
                      setTimeout(() => {
                        retryWithDifferentSettings();
                      }, 1000);
                    } catch (e) {
                      console.error('Error during media recovery:', e);
                      retryWithDifferentSettings();
                    }
                    return;
                  }
                }

                setError(`Media playback error: ${data.details}`);

                // Try to recover with a more aggressive approach
                try {
                  // First try simple media error recovery
                  newHls.recoverMediaError();

                  // If that succeeds, we also try reloading source
                  setTimeout(() => {
                    if (videoRef.current) {
                      videoRef.current.currentTime = 0;
                    }
                    newHls.startLoad();
                  }, 1000);
                } catch (e) {
                  console.error('Error during media recovery:', e);
                  // If recovery fails and we haven't retried much, try settings change
                  if (hlsRetryCount < 2) {
                    setTimeout(() => retryWithDifferentSettings(), 1000);
                  }
                }
                break;

              default:
                console.error('HLS fatal error:', data.details);
                setError(`Video playback error: ${data.details}`);

                // Try one more approach before giving up
                if (hlsRetryCount < 2) {
                  setTimeout(() => retryWithDifferentSettings(), 1000);
                } else {
                  // Cannot recover from other fatal errors after retries
                  newHls.destroy();
                }
                break;
            }
          }
        });

        // Attach to video element and load source
        newHls.attachMedia(video);
        newHls.loadSource(hlsPlaylistUrl);
        setHls(newHls);
      } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
        // For Safari which has built-in HLS support
        console.log('Using native HLS support with URL:', hlsPlaylistUrl);
        video.src = hlsPlaylistUrl;
        video.addEventListener('loadedmetadata', () => {
          setIsLoading(false);
        });
        video.addEventListener('canplay', () => {
          setIsLoading(false);
        });
        video.addEventListener('error', (e) => {
          console.error('Native HLS playback error:', e);
          
          // Try direct MP4 playback as a fallback
          console.log('Trying direct MP4 playback as fallback');
          // Include segment parameter if applicable
          const segmentQueryParam = isSegmentedRecording && currentSegment !== undefined ? `?segment=${currentSegment}` : '';
          const directVideoUrl = `${serverUrl}/playback/video/${recordingId}${segmentQueryParam}`;
          video.src = directVideoUrl;
          
          video.addEventListener('loadedmetadata', () => {
            setIsLoading(false);
            setError(null); // Clear previous error if direct playback works
          });
          
          video.addEventListener('error', (err) => {
            console.error('Direct video playback also failed:', err);
            setError('Error loading video stream. The recording may be unavailable.');
            setIsLoading(false);
          });
        });
      } else {
        // Try direct MP4 playback for browsers without HLS support
        console.log('HLS not supported, trying direct MP4 playback');
        // Include segment parameter if applicable
        const segmentQueryParam = isSegmentedRecording && currentSegment !== undefined ? `?segment=${currentSegment}` : '';
        const directVideoUrl = `${serverUrl}/playback/video/${recordingId}${segmentQueryParam}`;
        video.src = directVideoUrl;
        
        video.addEventListener('loadedmetadata', () => {
          setIsLoading(false);
        });
        
        video.addEventListener('error', (err) => {
          console.error('Direct video playback failed:', err);
          setError('Video playback is not supported in this browser.');
          setIsLoading(false);
        });
      }
    };

    // Setup HLS player
    setupHls();

    // Event listeners for the video element
    const onTimeUpdate = () => setCurrentTime(video.currentTime);
    const onLoadedMetadata = () => {
      setVideoDuration(video.duration);
      setIsLoading(false);
    };
    const onEnded = () => setIsPlaying(false);
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onWaiting = () => setIsLoading(true);
    const onPlaying = () => setIsLoading(false);
    const onError = (e: Event) => {
      console.error('Video element error:', e);
      setError('Error playing the video. The recording may be corrupted or unavailable.');
      setIsLoading(false);
    };

    // Set initial volume
    video.volume = volume;
    video.muted = isMuted;
    video.playbackRate = playbackRate;

    // Add event listeners
    video.addEventListener('timeupdate', onTimeUpdate);
    video.addEventListener('loadedmetadata', onLoadedMetadata);
    video.addEventListener('ended', onEnded);
    video.addEventListener('play', onPlay);
    video.addEventListener('pause', onPause);
    video.addEventListener('waiting', onWaiting);
    video.addEventListener('playing', onPlaying);
    video.addEventListener('error', onError);

    // Clean up
    return () => {
      video.removeEventListener('timeupdate', onTimeUpdate);
      video.removeEventListener('loadedmetadata', onLoadedMetadata);
      video.removeEventListener('ended', onEnded);
      video.removeEventListener('play', onPlay);
      video.removeEventListener('pause', onPause);
      video.removeEventListener('waiting', onWaiting);
      video.removeEventListener('playing', onPlaying);
      video.removeEventListener('error', onError);

      if (hls) {
        hls.destroy();
      }
    };
  }, [hlsPlaylistUrl, hlsRetryCount]);

  // Play/Pause toggle
  const togglePlay = () => {
    if (!videoRef.current) return;

    if (isPlaying) {
      videoRef.current.pause();
    } else {
      videoRef.current.play().catch(e => {
        console.warn('Playback prevented:', e);
        setError('Playback could not be started automatically.');
      });
    }
  };

  // Seek to specific time
  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!videoRef.current) return;

    const time = parseFloat(e.target.value);
    videoRef.current.currentTime = time;
    setCurrentTime(time);
  };

  // Volume control
  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!videoRef.current) return;

    const vol = parseFloat(e.target.value);
    videoRef.current.volume = vol;
    setVolume(vol);

    if (vol === 0) {
      setIsMuted(true);
    } else if (isMuted) {
      setIsMuted(false);
    }
  };

  // Toggle mute
  const toggleMute = () => {
    if (!videoRef.current) return;

    const newMuteState = !isMuted;
    videoRef.current.muted = newMuteState;
    setIsMuted(newMuteState);
  };

  // Playback speed control
  const handlePlaybackRateChange = (rate: number) => {
    if (!videoRef.current) return;
    videoRef.current.playbackRate = rate;
    setPlaybackRate(rate);
  };

  // Skip forward/backward
  const skip = (seconds: number) => {
    if (!videoRef.current) return;
    videoRef.current.currentTime = Math.max(
      0,
      Math.min(videoRef.current.duration, videoRef.current.currentTime + seconds)
    );
  };

  // Format time display (MM:SS)
  const formatTime = (seconds: number): string => {
    if (isNaN(seconds)) return "00:00";
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  };

  // Get event type badge color
  const getEventTypeBadgeColor = (eventType: string) => {
    switch (eventType.toLowerCase()) {
      case 'motion': return 'bg-yellow-100 text-yellow-800';
      case 'audio': return 'bg-blue-100 text-blue-800';
      case 'continuous': return 'bg-green-100 text-green-800';
      case 'manual': return 'bg-purple-100 text-purple-800';
      case 'analytics': return 'bg-red-100 text-red-800';
      case 'external': return 'bg-gray-100 text-gray-800';
      default: return 'bg-gray-100 text-gray-800';
    }
  };

  // Format date string
  const formatDate = (dateString?: string) => {
    if (!dateString) return 'N/A';
    return new Date(dateString).toLocaleString();
  };

  // Format duration in seconds to HH:MM:SS
  const formatDuration = (milliseconds: number) => {
    if (milliseconds === 0) return "In progress";

    const totalSeconds = Math.floor(milliseconds / 1000);
    const hrs = Math.floor(totalSeconds / 3600);
    const mins = Math.floor((totalSeconds % 3600) / 60);
    const secs = Math.floor(totalSeconds % 60);

    return `${hrs.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  };

  return (
    <div className="rounded-lg overflow-hidden shadow-lg bg-white dark:bg-gray-800">
      {/* Video display */}
      <div className="relative bg-black">
        {isLoading && (
          <div className="absolute inset-0 flex items-center justify-center bg-black bg-opacity-50 z-10">
            <div className="flex flex-col items-center">
              <div className="animate-spin rounded-full h-12 w-12 border-t-2 border-b-2 border-white mb-2"></div>
              <p className="text-white">Loading video...</p>
            </div>
          </div>
        )}
        {error && (
          <div className="absolute inset-0 flex items-center justify-center bg-black bg-opacity-70 z-10">
            <div className="text-white bg-red-600 p-4 rounded-md max-w-md text-center">
              <p className="mb-3">{error}</p>
              {hlsRetryCount < 2 && (
                <button
                  onClick={retryWithDifferentSettings}
                  className="flex items-center justify-center mx-auto bg-white text-red-600 hover:bg-gray-200 px-3 py-1 rounded-md"
                >
                  <ArrowPathIcon className="w-4 h-4 mr-1" />
                  <span>Retry with different settings</span>
                </button>
              )}
            </div>
          </div>
        )}
        <video
          ref={videoRef}
          className="w-full h-full"
          playsInline
          controls={false}
        />
      </div>

      {/* Video metadata */}
      <div className="p-3 dark:border-gray-700">
        <div className="flex justify-between items-center mb-2">
          <div>
            <Heading level={3} className="text-lg font-semibold">
              {cameraName}
            </Heading>
            <div className="flex mt-1 space-x-2">
              <Badge className={getEventTypeBadgeColor(eventType)}>
                {eventType}
              </Badge>
              {duration > 0 && (
                <Badge className="bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-300">
                  {formatDuration(duration)}
                </Badge>
              )}
            </div>
          </div>
          {onClose && (
            <button
              onClick={onClose}
              className="p-1 rounded-full hover:bg-gray-200 dark:hover:bg-gray-700"
            >
              <svg className="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          )}
        </div>

        {/* Playback timeline */}
        <div className="my-2">
          <input
            type="range"
            min={0}
            max={videoDuration || 100}
            value={currentTime}
            onChange={handleSeek}
            className="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-gray-700"
          />
          <div className="flex justify-between text-xs text-gray-500 dark:text-gray-400 mt-1">
            <span>{formatTime(currentTime)}</span>
            <span>{formatTime(videoDuration)}</span>
          </div>
        </div>

        {/* Playback controls */}
        <div className="flex items-center justify-between mt-3">
          <div className="flex items-center space-x-2">
            <button
              onClick={() => skip(-10)}
              className="p-2 rounded-full hover:bg-gray-200 dark:hover:bg-gray-700"
            >
              <BackwardIcon className="w-5 h-5" />
            </button>

            <button
              onClick={togglePlay}
              className="p-2 bg-indigo-600 rounded-full hover:bg-indigo-700 text-white"
            >
              {isPlaying ? (
                <PauseIcon className="w-5 h-5" />
              ) : (
                <PlayIcon className="w-5 h-5" />
              )}
            </button>

            <button
              onClick={() => skip(10)}
              className="p-2 rounded-full hover:bg-gray-200 dark:hover:bg-gray-700"
            >
              <ForwardIcon className="w-5 h-5" />
            </button>
          </div>

          <div className="flex items-center space-x-2">
            <button
              onClick={toggleMute}
              className="p-2 rounded-full hover:bg-gray-200 dark:hover:bg-gray-700"
            >
              {isMuted ? (
                <SpeakerXMarkIcon className="w-5 h-5" />
              ) : (
                <SpeakerWaveIcon className="w-5 h-5" />
              )}
            </button>

            <input
              type="range"
              min={0}
              max={1}
              step={0.1}
              value={volume}
              onChange={handleVolumeChange}
              className="w-20 h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-gray-700"
            />

            <select
              value={playbackRate}
              onChange={(e) => handlePlaybackRateChange(parseFloat(e.target.value))}
              className="bg-gray-200 dark:bg-gray-700 rounded p-1 text-sm"
            >
              <option value={0.5}>0.5×</option>
              <option value={1}>1×</option>
              <option value={1.5}>1.5×</option>
              <option value={2}>2×</option>
            </select>
          </div>
        </div>

        {/* Time metadata */}
        {(startTime || endTime) && (
          <div className="mt-3 pt-2 border-t border-gray-200 dark:border-gray-700 text-xs text-gray-500 dark:text-gray-400 grid grid-cols-2 gap-2">
            <div>
              <Text className="font-semibold">Start:</Text>
              <Text>{formatDate(startTime)}</Text>
            </div>
            <div>
              <Text className="font-semibold">End:</Text>
              <Text>{formatDate(endTime)}</Text>
            </div>
            
            {/* Segment indicator (if applicable) */}
            {isSegmentedRecording && currentSegment !== undefined && (
              <div className="col-span-2 mt-2">
                <Badge className="bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200">
                  Segment {currentSegment}
                </Badge>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
};

export default HlsRecordingPlayer;
