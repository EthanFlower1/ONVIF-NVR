import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { ApplicationLayout } from './application-layout'
import { useAuth } from './contexts/AuthContext'

// Sample event data (in a real app, this would come from an API)
const sampleEvents = [
  { id: '1', name: 'Conference 2025', url: '/events/1' },
  { id: '2', name: 'Product Launch', url: '/events/2' },
  { id: '3', name: 'Annual Meeting', url: '/events/3' },
]

export default function Layout({ children }: { children: React.ReactNode }) {
  const { logout, user } = useAuth()
  const navigate = useNavigate()

  useEffect(() => {
    // Setup logout event listener
    const handleLogout = () => {
      logout()
      navigate('/login')
    }

    window.addEventListener('logout', handleLogout)
    
    return () => {
      window.removeEventListener('logout', handleLogout)
    }
  }, [logout, navigate])

  return (
    <ApplicationLayout events={sampleEvents}>
      {children}
    </ApplicationLayout>
  )
}