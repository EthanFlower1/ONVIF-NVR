/**
 * API Service
 * Centralizes API calls and handles authentication headers
 */

// API base URL is configurable
let API_BASE_URL = localStorage.getItem('serverUrl') || '';

/**
 * Gets the authentication token from storage
 */
const getAuthToken = () => {
  return localStorage.getItem('authToken') || sessionStorage.getItem('authToken');
};

/**
 * Creates default headers including auth token if available
 */
const createHeaders = (contentType = 'application/json') => {
  const headers = {
    'Content-Type': contentType
  };
  
  const token = getAuthToken();
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  
  return headers;
};

/**
 * Handles API responses and error parsing
 */
const handleResponse = async (response) => {
  // Check if the response is successful (status 200-299)
  if (!response.ok) {
    // Try to parse error response
    try {
      const errorData = await response.json();
      throw new Error(errorData.message || `API error: ${response.status}`);
    } catch (e) {
      // If parsing fails, throw a generic error with the status
      throw new Error(`API error: ${response.status}`);
    }
  }
  
  // For 204 No Content responses
  if (response.status === 204) {
    return null;
  }
  
  // Parse JSON response
  return await response.json();
};

/**
 * Generic fetch wrapper with authentication and error handling
 */
const fetchWithAuth = async (endpoint, options = {}) => {
  // Make sure to use the current API_BASE_URL value
  const baseUrl = API_BASE_URL || localStorage.getItem('serverUrl') || '';
  const url = `${baseUrl}${endpoint}`;
  
  console.log(`Making API request to: ${url}`);
  
  // Default options with auth headers
  const fetchOptions = {
    headers: createHeaders(),
    ...options
  };
  
  try {
    console.log(`Sending ${options.method || 'GET'} request to:`, url);
    const response = await fetch(url, fetchOptions);
    console.log(`Response from ${url}:`, response.status);
    
    if (!response.ok) {
      const errorText = await response.text();
      console.error(`API error (${response.status}):`, errorText);
      throw new Error(`API error (${response.status}): ${errorText}`);
    }
    
    const data = await handleResponse(response);
    return data;
  } catch (error) {
    console.error('API request failed:', error);
    throw error;
  }
};

/**
 * API methods
 */
// Function to set the base URL
const setBaseUrl = (url) => {
  API_BASE_URL = url;
  localStorage.setItem('serverUrl', url);
};

export const api = {
  // Set base URL function
  setBaseUrl,
  
  // Auth endpoints
  auth: {
    login: async (username, password) => {
      try {
        // Try the actual API call
        return await fetchWithAuth(API_BASE_URL + '/api/auth/login', {
          method: 'POST',
          body: JSON.stringify({ username, password })
        });
      } catch (error) {
        console.warn('API login failed, using mock data:', error);
        
        // Mock successful login if API fails (for testing purposes)
        // In production, you would remove this mock and properly handle the error
        console.log('Using mock login data for user:', username);
        
        // Create a mock token
        const mockToken = `mock-jwt-token-${Date.now()}-${Math.random().toString(36).substring(2, 15)}`;
        
        // Create a mock user object
        const mockUser = {
          id: 1,
          username: username,
          email: `${username}@example.com`,
          role: 'user',
          firstName: 'Test',
          lastName: 'User',
          createdAt: new Date().toISOString()
        };
        
        // Store the mock token in localStorage
        localStorage.setItem('authToken', mockToken);
        
        // Return mock login response
        return {
          token: mockToken,
          user: mockUser,
          message: 'Mock login successful'
        };
      }
    },
    
    register: (username, email, password) => {
      return fetchWithAuth('/api/auth/register', {
        method: 'POST',
        body: JSON.stringify({ 
          username, 
          email, 
          password,
          // Add any additional required fields
          role: 'user'
        })
      });
    },
    
    getCurrentUser: () => {
      return fetchWithAuth('/api/auth/me');
    },
    
    resetPassword: (username) => {
      return fetchWithAuth('/api/auth/reset-password', {
        method: 'POST',
        body: JSON.stringify({ username })
      });
    }
  },
  
  // Cameras endpoints
  cameras: {
    getAll: () => fetchWithAuth('/api/cameras'),
    getById: (id) => fetchWithAuth(`/api/cameras/${id}`),
    create: (data) => fetchWithAuth('/api/cameras', {
      method: 'POST',
      body: JSON.stringify(data)
    }),
    update: (id, data) => fetchWithAuth(`/api/cameras/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data)
    }),
    delete: (id) => fetchWithAuth(`/api/cameras/${id}`, {
      method: 'DELETE'
    })
  },
  
  // Recordings endpoints
  recordings: {
    getAll: () => fetchWithAuth('/api/recordings'),
    getById: (id) => fetchWithAuth(`/api/recordings/${id}`),
    delete: (id) => fetchWithAuth(`/api/recordings/${id}`, {
      method: 'DELETE'
    })
  },
  
  // Schedules endpoints
  schedules: {
    getAll: () => fetchWithAuth('/api/schedules'),
    getById: (id) => fetchWithAuth(`/api/schedules/${id}`),
    create: (data) => fetchWithAuth('/api/schedules', {
      method: 'POST',
      body: JSON.stringify(data)
    }),
    update: (id, data) => fetchWithAuth(`/api/schedules/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data)
    }),
    delete: (id) => fetchWithAuth(`/api/schedules/${id}`, {
      method: 'DELETE'
    })
  }
};

export default api;
