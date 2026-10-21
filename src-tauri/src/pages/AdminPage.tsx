// 管理员页：审计日志与缓存管理。
//
// 后端早就有了这些接口，但此前浏览器里没有入口——
// 前端知道谁是管理员（employee.is_admin），却从来没有用到。
// 这个页面把审计日志、缓存统计和清空缓存放到界面上，
// 管理员不用再去翻 data/audit.log 这个文本文件。

import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  AuthTokens,
  getAuditLog,
  getCacheStats,
  invalidateCache,
  logout,
  type AuditEvent,
  type CacheStats,
} from '../services/api';

/** 审计动作的中文名。 */
const ACTION_LABELS: Record<string, string> = {
  LOGIN: '登录',
  LOGIN_FAIL: '登录失败',
  LOGIN_BLOCKED: '触发锁定',
  LOGOUT: '退出',
  VIEW: '查看工资',
  EXPORT_IMG: '导出截图',
  EXPORT_PDF: '导出 PDF',
  PRINT: '打印',
  AUDIT_VIEW: '查看审计',
  CACHE_STATS: '查看缓存',
  CACHE_INVALIDATE: '清空缓存',
};

/** 审计动作的配色分类。 */
function actionTone(action: string): string {
  if (action === 'LOGIN_FAIL' || action === 'LOGIN_BLOCKED') return 'action-fail';
  if (action === 'VIEW' || action === 'EXPORT_IMG' || action === 'EXPORT_PDF' || action === 'PRINT') {
    return 'action-view';
  }
  if (action === 'LOGIN' || action === 'LOGOUT') return 'action-ok';
  return 'action-neutral';
}

/** 把 RFC3339 时间转成本地时间的可读格式。 */
function formatTime(raw: string): string {
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) {
    return raw;
  }

  const pad = (value: number) => String(value).padStart(2, '0');
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  );
}

/** 可选的日志条数。 */
const LIMIT_OPTIONS = [50, 100, 200, 500];

export function AdminPage() {
  const navigate = useNavigate();
  const employee = AuthTokens.getUser();

  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [stats, setStats] = useState<CacheStats | null>(null);
  const [limit, setLimit] = useState(100);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);

    const [log, cacheStats] = await Promise.all([getAuditLog(limit), getCacheStats()]);

    if (!log.ok) {
      setError(log.error || '获取审计日志失败');
    } else {
      setEvents(log.events);
    }

    if (cacheStats.ok) {
      setStats(cacheStats.stats ?? null);
    }

    setLoading(false);
  }, [limit]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleRefresh = () => {
    void load();
  };

  const handleInvalidate = async () => {
    const result = await invalidateCache();
    if (result.ok) {
      setNotice(result.message || '缓存已清空');
      await load();
    } else {
      setError(result.error || '清空缓存失败');
    }
  };

  const handleLogout = async () => {
    await logout();
    navigate('/login', { replace: true });
  };

  const total = stats ? stats.hits + stats.misses : 0;
  const hitRate = total > 0 && stats ? Math.round((stats.hits / total) * 100) : 0;

  return (
    <>
      <div className="header no-print">
        <h1>员工工资查询系统 · 管理</h1>
        <div className="user-info">
          <span>
            {employee?.name ?? ''}
            {employee?.dept ? `（${employee.dept}）` : ''}
          </span>
          <button type="button" className="logout-btn" onClick={() => navigate('/')}>
            返回工资条
          </button>
          <button type="button" className="logout-btn" onClick={handleLogout}>
            退出
          </button>
        </div>
      </div>

      <div className="container">
        {error && <div className="error-card">{error}</div>}
        {notice && <div className="info-msg">{notice}</div>}

        <div className="card">
          <div className="toolbar">
            <div className="title-text">缓存状态</div>
            <div className="btn-group">
              <button type="button" className="print-btn" onClick={handleRefresh}>
                刷新
              </button>
              <button type="button" className="export-img-btn" onClick={handleInvalidate}>
                清空缓存
              </button>
            </div>
          </div>

          {stats ? (
            <div className="stat-grid">
              <div className="stat-item">
                <div className="stat-label">命中</div>
                <div className="stat-value">{stats.hits}</div>
              </div>
              <div className="stat-item">
                <div className="stat-label">未命中</div>
                <div className="stat-value">{stats.misses}</div>
              </div>
              <div className="stat-item">
                <div className="stat-label">命中率</div>
                <div className="stat-value">{hitRate}%</div>
              </div>
              <div className="stat-item">
                <div className="stat-label">已缓存工资表</div>
                <div className="stat-value">{stats.salary_files_cached}</div>
              </div>
            </div>
          ) : (
            <div className="empty-hint">暂无缓存统计</div>
          )}
        </div>

        <div className="card">
          <div className="toolbar">
            <div className="title-text">审计日志</div>
            <div className="btn-group">
              <select
                className="month-select"
                value={limit}
                onChange={(e) => setLimit(Number(e.target.value))}
                aria-label="显示条数"
              >
                {LIMIT_OPTIONS.map((option) => (
                  <option key={option} value={option}>
                    最近 {option} 条
                  </option>
                ))}
              </select>
              <button type="button" className="print-btn" onClick={handleRefresh}>
                刷新
              </button>
            </div>
          </div>

          {loading ? (
            <div className="empty-hint">正在加载…</div>
          ) : events.length === 0 ? (
            <div className="empty-hint">暂无审计记录</div>
          ) : (
            <div className="payslip-wrapper">
              <table className="payslip-table admin-table">
                <thead>
                  <tr>
                    <th>时间</th>
                    <th>动作</th>
                    <th>工号</th>
                    <th>姓名</th>
                    <th>来源 IP</th>
                    <th>详情</th>
                  </tr>
                </thead>
                <tbody>
                  {events.map((event, index) => (
                    <tr key={`${event.timestamp}-${index}`}>
                      <td className="audit-time">{formatTime(event.timestamp)}</td>
                      <td>
                        <span className={`action-tag ${actionTone(event.action)}`}>
                          {ACTION_LABELS[event.action] ?? event.action}
                        </span>
                      </td>
                      <td>{event.employee_id}</td>
                      <td>{event.employee_name}</td>
                      <td>{event.ip}</td>
                      <td>{event.detail}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          <div className="warn-msg">⚠ 审计日志包含谁在什么时候查看了工资，请勿外传</div>
        </div>
      </div>
    </>
  );
}
