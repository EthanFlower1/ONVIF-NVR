import { createContext, useContext, useState, useEffect } from 'react';
import api from '../services/api';

const AuthContext = createContext();

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [apiBaseUrl, setApiBaseUrl] = useState(() => {
    // Try to get the server URL from localStorage first
    return localStorage.getItem('serverUrl') || '';
  });

  // Check if user is already logged in on mount
  useEffect(() => {
    const checkAuth = async () => {
      const token = localStorage.getItem('authToken') || sessionStorage.getItem('authToken');
      // Ensure we have a server URL before trying to authenticate
      const serverUrl = localStorage.getItem('serverUrl');

      if (token && serverUrl) {
        // Make sure API service has the server URL
        api.setBaseUrl(serverUrl);
        setApiBaseUrl(serverUrl);

        try {
          const userData = await api.auth.getCurrentUser();
          setUser(userData);
        } catch (err) {
          console.error('Auth check failed:', err);
          // Token is invalid, clear it
          localStorage.removeItem('authToken');
          sessionStorage.removeItem('authToken');
        }
      } else {
        // No token or server URL, we're definitely not logged in
        setUser(null);
      }
      setLoading(false);
    };

    checkAuth();
  }, []);

  const login = async (username, password, remember = false) => {
    setLoading(true);
    setError(null);

    try {
      // Make sure we're using the current API base URL
      const data = await api.auth.login(username, password);

      // Store token in localStorage or sessionStorage based on "remember me"
      if (remember) {
        localStorage.setItem('authToken', data.token);
      } else {
        sessionStorage.setItem('authToken', data.token);
      }

      console.log('Setting user data:', data.user);

      // Ensure token is stored properly
      if (data.token && !localStorage.getItem('authToken') && !sessionStorage.getItem('authToken')) {
        localStorage.setItem('authToken', data.token);
      }

      // Set user state
      setUser(data.user);

      // Wait a moment for state to update
      setTimeout(() => {
        console.log('Authentication state updated: isAuthenticated =', !!data.user);
      }, 100);
      return data.user;
    } catch (err) {
      setError(err.message);
      throw err;
    } finally {
      setLoading(false);
    }
  };

  const register = async (username, email, password) => {
    setLoading(true);
    setError(null);

    try {
      const data = await api.auth.register(username, email, password);
      return data;
    } catch (err) {
      setError(err.message);
      throw err;
    } finally {
      setLoading(false);
    }
  };

  const logout = () => {
    localStorage.removeItem('authToken');
    sessionStorage.removeItem('authToken');
    setUser(null);
  };

  const resetPassword = async (username) => {
    setLoading(true);
    setError(null);

    try {
      const response = await api.auth.resetPassword(username);
      return response;
    } catch (err) {
      setError(err.message);
      throw err;
    } finally {
      setLoading(false);
    }
  };

  // Helper to get auth token for API requests
  const getAuthToken = () => {
    return localStorage.getItem('authToken') || sessionStorage.getItem('authToken');
  };

  // Update the API base URL and store it in localStorage
  const handleSetApiBaseUrl = (url) => {
    localStorage.setItem('serverUrl', url);
    setApiBaseUrl(url);
    // Update the API service with the new base URL
    api.setBaseUrl(url);
  };

  // Set the API base URL on initial load
  useEffect(() => {
    if (apiBaseUrl) {
      api.setBaseUrl(apiBaseUrl);
    }
  }, [apiBaseUrl]);

  const value = {
    user,
    loading,
    error,
    login,
    logout,
    register,
    resetPassword,
    getAuthToken,
    setApiBaseUrl: handleSetApiBaseUrl,
    apiBaseUrl,
    isAuthenticated: !!user
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export const useAuth = () => {
  const context = useContext(AuthContext);
  if (context === null) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
};
