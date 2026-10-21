// Protected Route Component with Authentication Check

import { Navigate, useLocation } from 'react-router-dom';
import { AuthTokens } from '../services/api';

interface ProtectedRouteProps {
  children: React.ReactNode;
  /**
   * 是否要求管理员权限。
   *
   * 管理员身份来自登录时下发的员工信息（`employee.is_admin`），
   * 服务端同样会对管理员接口再校验一次，前端这层只是避免普通员工
   * 点进去看到一片空白。
   */
  adminOnly?: boolean;
}

export function ProtectedRoute({ children, adminOnly = false }: ProtectedRouteProps) {
  const isAuthenticated = AuthTokens.isAuthenticated();
  const location = useLocation();

  if (!isAuthenticated) {
    // Redirect to login page with the attempted URL
    return <Navigate to="/login" state={{ from: location }} replace />;
  }

  if (adminOnly && !AuthTokens.getUser()?.is_admin) {
    // 不是管理员就回工资条页
    return <Navigate to="/" replace />;
  }

  return <>{children}</>;
}
