// 应用入口：登录页与工资条页。

import { useEffect } from 'react';
import {
  BrowserRouter as Router,
  Navigate,
  Route,
  Routes,
  useNavigate,
  useSearchParams,
} from 'react-router-dom';
import { ProtectedRoute } from './components/ProtectedRoute';
import { LoginPage } from './pages/LoginPage';
import { SalaryPage } from './pages/SalaryPage';
import { adoptToken } from './services/api';

/**
 * 接管托盘控制面板带来的会话令牌。
 *
 * 托盘换到的是令牌而不是密码，所以地址栏里不会留下明文口令；
 * 但令牌本身也不该留在地址栏里，用完立刻抹掉。
 */
function TokenBootstrap() {
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();

  useEffect(() => {
    const token = searchParams.get('token');
    if (!token) return;

    const next = new URLSearchParams(searchParams);
    next.delete('token');
    setSearchParams(next, { replace: true });

    void adoptToken(token).then((ok) => {
      navigate(ok ? '/' : '/login?autologin=1', { replace: true });
    });
  }, [searchParams, setSearchParams, navigate]);

  return null;
}

function App() {
  return (
    <Router>
      <TokenBootstrap />
      <Routes>
        {/* 公开路由 */}
        <Route path="/login" element={<LoginPage />} />

        {/* 需要登录 */}
        <Route
          path="/"
          element={
            <ProtectedRoute>
              <SalaryPage />
            </ProtectedRoute>
          }
        />
        <Route
          path="/salary"
          element={
            <ProtectedRoute>
              <SalaryPage />
            </ProtectedRoute>
          }
        />

        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Router>
  );
}

export default App;
