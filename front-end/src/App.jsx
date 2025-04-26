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
    <Routes>
      {/* Main application routes with Layout */}
      <Route path="/" element={<Layout><Liveview /></Layout>} />
      <Route path="/login" element={<AuthLayout><Login /></AuthLayout>} />
      <Route path="/register" element={<AuthLayout><Register /></AuthLayout>} />
      <Route path="/forgot-password" element={<AuthLayout><ForgotPassword /></AuthLayout>} />
      <Route path="/settings" element={<Layout><Settings /></Layout>} />
      <Route path="/events" element={<Layout><Events /></Layout>} />
      <Route path="/cameras" element={<Layout><Cameras /></Layout>} />
      <Route path="/discovery" element={<Layout><DeviceDiscovery /></Layout>} />
      <Route path="/recordings" element={<Layout><Recordings /></Layout>} />
      <Route path="/recording-schedules" element={<Layout><RecordingSchedules /></Layout>} />

      {/* Catch-all route - redirect to home */}
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}

export default App
