import { Navigate, useLocation } from 'react-router-dom';
import { useAuth } from '../contexts/AuthContext';
import { FullPageLoader } from './loading-spinner';

export function ProtectedRoute({ children }) {
  const { user, isAuthenticated, loading, apiBaseUrl } = useAuth();
  const location = useLocation();

  console.log('ProtectedRoute state:', { user, isAuthenticated, loading, apiBaseUrl, location });

  // Show loading state while checking authentication
  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center fixed inset-0 bg-white/80 dark:bg-zinc-900/80 z-50">
        <FullPageLoader />
        {apiBaseUrl && (
          <p className="mt-4 text-sm text-zinc-500">
            Connecting to: {apiBaseUrl}
          </p>
        )}
      </div>
    );
  }

  // Redirect to login if not authenticated
  // Use direct check on user instead of isAuthenticated prop for more explicit logic
  if (!user) {
    console.log('User not authenticated, redirecting to login');
    return <Navigate to="/login" state={{ from: location }} replace />;
  }
  
  console.log('User authenticated, rendering protected content');
  
  // Render children if authenticated
  return children;
}