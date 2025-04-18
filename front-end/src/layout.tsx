import { ApplicationLayout } from './application-layout'

// Sample event data (in a real app, this would come from an API)
const sampleEvents = [
  { id: '1', name: 'Conference 2025', url: '/events/1' },
  { id: '2', name: 'Product Launch', url: '/events/2' },
  { id: '3', name: 'Annual Meeting', url: '/events/3' },
]

export default function Layout({ children }: { children: React.ReactNode }) {
  return <ApplicationLayout events={sampleEvents}>{children}</ApplicationLayout>
}