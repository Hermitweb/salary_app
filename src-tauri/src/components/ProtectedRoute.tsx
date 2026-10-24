// Protected Route Component with Authentication Check

import { Navigate, useLocation } from 'react-router-dom';
import { AuthTokens } from '../services/api';

interface ProtectedRouteProps {
  children: React.ReactNode;
}

export function ProtectedRoute({ children }: ProtectedRouteProps) {
  const isAuthenticated = AuthTokens.isAuthenticated();
  const location = useLocation();

  if (!isAuthenticated) {
    // Redirect to login page with the attempted URL
    return <Navigate to="/login" state={{ from: location }} replace />;
  }

  return <>{children}</>;
}
