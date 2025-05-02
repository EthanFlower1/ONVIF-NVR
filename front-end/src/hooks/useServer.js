import { useState, useEffect } from 'react';
import { useAuth } from '../contexts/AuthContext';

/**
 * Custom hook to access and manage the server URL
 * This provides a convenient way for components to get the current server URL
 */
export function useServer() {
  const { apiBaseUrl, setApiBaseUrl } = useAuth();
  const [isConnected, setIsConnected] = useState(false);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState(null);

  useEffect(() => {
    if (apiBaseUrl) {
      setIsConnecting(true);
      setConnectionError(null);
      
      // Try to ping the server to check connectivity
      fetch(`${apiBaseUrl}/api/health`)
        .then(response => {
          if (response.ok) {
            setIsConnected(true);
          } else {
            setConnectionError(`Server responded with status: ${response.status}`);
            setIsConnected(false);
          }
        })
        .catch(err => {
          setConnectionError(err.message || 'Connection failed');
          setIsConnected(false);
        })
        .finally(() => {
          setIsConnecting(false);
        });
    } else {
      setIsConnected(false);
    }
  }, [apiBaseUrl]);

  return {
    serverUrl: apiBaseUrl,
    setServerUrl: setApiBaseUrl,
    isConnected,
    isConnecting,
    connectionError
  };
}

export default useServer;