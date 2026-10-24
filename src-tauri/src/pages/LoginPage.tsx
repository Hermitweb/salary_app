// 登录页：与 Python 版 templates/login.html 保持一致。

import { useEffect, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { AuthTokens, login } from '../services/api';

export function LoginPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams] = useSearchParams();

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 会话过期跳回来时的提示（与 Python 原版的 expired 标记一致）
  const expired = searchParams.get('expired') === '1';

  // 托盘控制面板带来的令牌没生效时回到登录页
  const autologinFailed = searchParams.get('autologin') === '1';

  // 已登录时直接进入工资条页
  useEffect(() => {
    if (AuthTokens.isAuthenticated()) {
      navigate('/', { replace: true });
    }
  }, [navigate]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!username.trim() || !password.trim()) {
      setError('请输入账号和密码');
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const response = await login({
        username: username.trim(),
        password: password.trim(),
      });

      if (response.ok && response.token) {
        const from = (location.state as { from?: { pathname: string } } | null)?.from?.pathname;
        navigate(from || '/', { replace: true });
      } else {
        setError(response.error || '登录失败');
      }
    } catch {
      setError('网络错误，请稍后重试');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="login-page">
      <div className="login-container">
        <div className="logo">
          <div className="logo-icon">
            {/* 与任务栏、托盘同一份应用图标（src-tauri/public/app_icon.png） */}
            <img src="/app_icon.png" alt="" width={40} height={40} aria-hidden="true" />
          </div>
          <h1>工资查询系统</h1>
          <p>Salary Query System</p>
        </div>

        {expired && <div className="info-msg">登录状态已过期，请重新登录</div>}

        {autologinFailed && <div className="error-msg">控制面板的自动登录没有生效，请手动登录</div>}

        {error && <div className="error-msg">{error}</div>}

        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label htmlFor="username">账号</label>
            <input
              type="text"
              id="username"
              name="username"
              placeholder="请输入账号"
              required
              autoFocus
              autoComplete="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </div>
          <div className="form-group">
            <label htmlFor="password">密码</label>
            <input
              type="password"
              id="password"
              name="password"
              placeholder="请输入密码"
              required
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          <button type="submit" className="login-btn" disabled={loading}>
            {loading ? '登录中…' : '登录'}
          </button>
        </form>

        <div className="pwd-hint">
          初始密码为 HR 登记的紧急联系人号码
          <br />
          忘记密码请联系管理员重置
        </div>

        <div className="footer">© 2026 工资查询系统</div>
      </div>

      <div className="warning-bar">⚠ 薪资保密，请勿讨论，违者按公司制度处理</div>
    </div>
  );
}
