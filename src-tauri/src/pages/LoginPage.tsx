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
            <svg
              width="28"
              height="28"
              viewBox="0 0 32 32"
              fill="none"
              xmlns="http://www.w3.org/2000/svg"
              aria-hidden="true"
            >
              {/* 工资条文档 */}
              <rect x="5" y="4" width="22" height="24" rx="2.5" fill="#ffffff" stroke="#3b82f6" strokeWidth="1.8" />
              {/* 文档顶部的横线（标题栏） */}
              <line x1="9" y1="10" x2="23" y2="10" stroke="#3b82f6" strokeWidth="1.6" strokeLinecap="round" />
              <line x1="9" y1="14" x2="19" y2="14" stroke="#3b82f6" strokeWidth="1.4" strokeLinecap="round" opacity="0.5" />
              <line x1="9" y1="17.5" x2="17" y2="17.5" stroke="#3b82f6" strokeWidth="1.4" strokeLinecap="round" opacity="0.5" />
              {/* ¥ 符号（用路径绘制，避免字体兼容问题） */}
              <path d="M13 21 L16 23.5 L19 21" stroke="#10b981" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" fill="none" />
              <line x1="13.2" y1="24.5" x2="18.8" y2="24.5" stroke="#10b981" strokeWidth="1.8" strokeLinecap="round" />
              <line x1="16" y1="23.5" x2="16" y2="26.5" stroke="#10b981" strokeWidth="1.8" strokeLinecap="round" />
            </svg>
          </div>
          <h1>工资查询系统</h1>
          <p>Salary Query System</p>
        </div>

        {expired && <div className="info-msg">登录状态已过期，请重新登录</div>}

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
