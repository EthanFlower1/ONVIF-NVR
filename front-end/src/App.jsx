import { Routes, Route, Navigate } from 'react-router-dom'
import Layout from './layout.tsx'
import { AuthLayout } from './components/auth-layout.tsx'
import Login from './pages/login.tsx'
import ForgotPassword from './pages/forgot-password.tsx'
import Register from './pages/register.tsx'
import Cameras from './pages/cameras.tsx'
import DeviceDiscovery from './pages/discovery.tsx'
import Liveview from './pages/live-view.tsx'
import Recordings from './pages/recordings.tsx'
import RecordingSchedules from './pages/recording-schedules.tsx'
import { ProtectedRoute } from './components/ProtectedRoute'
import { AuthProvider } from './contexts/AuthContext'
import './App.css'

// Placeholder Settings component
const Settings = () => (
  <div>
    <h1 className="text-2xl font-bold mb-4">Settings</h1>
    <p>Configure your application settings here.</p>
  </div>
)

// Placeholder Events component
const Events = () => (
  <div>
    <h1 className="text-2xl font-bold mb-4">Events</h1>
    <p>View all your events here.</p>
  </div>
)

function App() {
  return (
    <AuthProvider>
      <Routes>
        {/* Auth routes */}
        <Route path="/login" element={<AuthLayout><Login /></AuthLayout>} />
        <Route path="/register" element={<AuthLayout><Register /></AuthLayout>} />
        <Route path="/forgot-password" element={<AuthLayout><ForgotPassword /></AuthLayout>} />
        
        {/* Protected application routes with Layout */}
        <Route 
          path="/" 
          element={
            <ProtectedRoute>
              <Layout><Liveview /></Layout>
            </ProtectedRoute>
          } 
        />
        <Route 
          path="/settings" 
          element={
            <ProtectedRoute>
              <Layout><Settings /></Layout>
            </ProtectedRoute>
          } 
        />
        <Route 
          path="/events" 
          element={
            <ProtectedRoute>
              <Layout><Events /></Layout>
            </ProtectedRoute>
          } 
        />
        <Route 
          path="/cameras" 
          element={
            <ProtectedRoute>
              <Layout><Cameras /></Layout>
            </ProtectedRoute>
          } 
        />
        <Route 
          path="/discovery" 
          element={
            <ProtectedRoute>
              <Layout><DeviceDiscovery /></Layout>
            </ProtectedRoute>
          } 
        />
        <Route 
          path="/recordings" 
          element={
            <ProtectedRoute>
              <Layout><Recordings /></Layout>
            </ProtectedRoute>
          } 
        />
        <Route 
          path="/recording-schedules" 
          element={
            <ProtectedRoute>
              <Layout><RecordingSchedules /></Layout>
            </ProtectedRoute>
          } 
        />

        {/* Catch-all route - redirect to home */}
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AuthProvider>
  )
}

export default App
