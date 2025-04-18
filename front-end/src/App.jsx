import { Routes, Route, Navigate } from 'react-router-dom'
import Layout from './layout.tsx'
import { AuthLayout } from './components/auth-layout.tsx'
import Login from './pages/login.tsx'
import './App.css'

// Placeholder Home component
const Home = () => (
  <div className="text-center">
    <h1 className="text-2xl font-bold mb-4">Welcome to G-Streamer</h1>
    <p>This is the home page of our application.</p>
  </div>
)

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

// Placeholder Orders component
const Orders = () => (
  <div>
    <h1 className="text-2xl font-bold mb-4">Orders</h1>
    <p>Manage your orders here.</p>
  </div>
)

function App() {
  return (
    <Routes>
      {/* Auth routes */}
      <Route path="/login" element={
          <Login />
      } />
      
      {/* Main application routes with Layout */}
      <Route path="/" element={<Layout><Home /></Layout>} />
      <Route path="/settings" element={<Layout><Settings /></Layout>} />
      <Route path="/events" element={<Layout><Events /></Layout>} />
      <Route path="/orders" element={<Layout><Orders /></Layout>} />
      
      {/* Catch-all route - redirect to home */}
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}

export default App
