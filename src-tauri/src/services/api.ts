// API 服务层：统一处理 Bearer 令牌与后端交互。

import axios from 'axios';

/**
 * 始终与后端同源访问：
 * - 生产环境由 Rust 服务托管前端，天然同源；
 * - 开发环境由 Vite 将 /api 代理到后端，同样同源。
 */
const api = axios.create({
  baseURL: window.location.origin,
  headers: {
    'Content-Type': 'application/json',
  },
  timeout: 30000,
});

/** 登录接口路径：这里的 401 表示「密码错误」，不能当作会话过期处理。 */
const LOGIN_PATH = '/api/auth/login';

/** 登录后的员工信息。 */
export interface Employee {
  employee_id: string;
  name?: string;
  dept?: string;
  is_admin: boolean;
}

const TOKEN_KEY = 'salary_app_token';
const USER_KEY = 'salary_app_user';

/** 令牌与当前用户的管理。 */
export const AuthTokens = {
  getToken: (): string | null => localStorage.getItem(TOKEN_KEY),

  setToken: (token: string): void => {
    localStorage.setItem(TOKEN_KEY, token);
    api.defaults.headers.common['Authorization'] = `Bearer ${token}`;
  },

  getUser: (): Employee | null => {
    const raw = localStorage.getItem(USER_KEY);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as Employee;
    } catch {
      return null;
    }
  },

  setUser: (employee: Employee): void => {
    localStorage.setItem(USER_KEY, JSON.stringify(employee));
  },

  clearToken: (): void => {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(USER_KEY);
    delete api.defaults.headers.common['Authorization'];
  },

  isAuthenticated: (): boolean => !!localStorage.getItem(TOKEN_KEY),
};

// 刷新页面后恢复 Authorization 头
const restoredToken = AuthTokens.getToken();
if (restoredToken) {
  api.defaults.headers.common['Authorization'] = `Bearer ${restoredToken}`;
}

/**
 * 会话过期处理。
 *
 * 令牌有效期 30 分钟，过期后接口会返回 401。此时清掉本地令牌并跳回登录页，
 * 用 `?expired=1` 标记「是过期跳回来的」，登录页据此给出提示。
 * 登录接口自身的 401（密码错误）不参与这个流程。
 */
api.interceptors.response.use(
  (response) => response,
  (error) => {
    const status = error?.response?.status;
    const url = String(error?.config?.url ?? '');

    if (status === 401 && !url.includes(LOGIN_PATH)) {
      AuthTokens.clearToken();

      if (!window.location.pathname.startsWith('/login')) {
        window.location.assign('/login?expired=1');
      }
    }

    return Promise.reject(error);
  },
);

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginResponse {
  ok: boolean;
  token?: string;
  employee?: Employee;
  error?: string;
}

export interface MonthOption {
  year: number;
  month: number;
}

export interface SalaryQueryResponse {
  ok: boolean;
  year?: number;
  month?: number;
  headers?: string[];
  values?: string[];
  data?: Record<string, string>;
  error?: string;
}

export interface AvailableMonthsResponse {
  ok: boolean;
  months: MonthOption[];
  error?: string;
}

export interface CacheStats {
  hits: number;
  misses: number;
  salary_files_cached: number;
}

/** 审计事件。 */
export interface AuditEvent {
  timestamp: string;
  ip: string;
  action: string;
  employee_id: string;
  employee_name: string;
  detail: string;
}

/** 审计日志响应。 */
export interface AuditLogResponse {
  ok: boolean;
  events: AuditEvent[];
  error?: string;
}

/** 缓存统计响应。 */
export interface CacheStatsResponse {
  ok: boolean;
  stats?: CacheStats;
  error?: string;
}

/** 登录并保存令牌。 */
export const login = async (credentials: LoginRequest): Promise<LoginResponse> => {
  try {
    const response = await api.post<LoginResponse>('/api/auth/login', credentials);
    const { ok, token, employee } = response.data;

    if (ok && token) {
      AuthTokens.setToken(token);
      if (employee) {
        AuthTokens.setUser(employee);
      }
    }

    return response.data;
  } catch (error) {
    if (axios.isAxiosError(error)) {
      return {
        ok: false,
        error: error.response?.data?.error || '网络错误，请稍后重试',
      };
    }
    throw error;
  }
};

/** 退出登录：通知后端记录审计事件，然后清除本地令牌。 */
export const logout = async (): Promise<void> => {
  try {
    await api.post('/api/logout');
  } catch {
    // 后端不可达时也要保证本地状态被清理
  } finally {
    AuthTokens.clearToken();
  }
};

/**
 * 查询当前登录员工的工资数据。
 *
 * 员工身份来自会话令牌，接口不接受工号参数，避免越权查询他人工资。
 */
export const getSalary = async (
  year: number,
  month: number,
): Promise<SalaryQueryResponse> => {
  if (!AuthTokens.isAuthenticated()) {
    return { ok: false, error: '请先登录 / Please login first' };
  }

  try {
    const response = await api.get<SalaryQueryResponse>(`/api/salary/${year}/${month}`);
    return response.data;
  } catch (error) {
    if (axios.isAxiosError(error)) {
      return {
        ok: false,
        error: error.response?.data?.error || '获取工资数据失败',
      };
    }
    throw error;
  }
};

/** 获取可查询的工资月份列表。 */
export const getAvailableMonths = async (): Promise<AvailableMonthsResponse> => {
  if (!AuthTokens.isAuthenticated()) {
    return { ok: false, months: [], error: '请先登录 / Please login first' };
  }

  try {
    const response = await api.get<AvailableMonthsResponse>('/api/available_months');
    return response.data;
  } catch (error) {
    if (axios.isAxiosError(error)) {
      return {
        ok: false,
        months: [],
        error: error.response?.data?.error || '获取月份列表失败',
      };
    }
    throw error;
  }
};

/** 健康检查。 */
export const healthCheck = async (): Promise<boolean> => {
  try {
    await api.get('/healthz');
    return true;
  } catch {
    return false;
  }
};

/** 缓存统计（仅管理员）。 */
export const getCacheStats = async (): Promise<CacheStatsResponse> => {
  try {
    const response = await api.get<CacheStatsResponse>('/api/cache/stats');
    return response.data;
  } catch (error) {
    if (axios.isAxiosError(error)) {
      return {
        ok: false,
        error: error.response?.data?.error || '获取缓存统计失败',
      };
    }
    throw error;
  }
};

/** 清空缓存（仅管理员）。 */
export const invalidateCache = async (): Promise<{ ok: boolean; message?: string; error?: string }> => {
  try {
    const response = await api.post<{ ok: boolean; message?: string; error?: string }>(
      '/api/cache/invalidate',
    );
    return response.data;
  } catch (error) {
    if (axios.isAxiosError(error)) {
      return {
        ok: false,
        error: error.response?.data?.error || '清空缓存失败',
      };
    }
    throw error;
  }
};

/** 获取审计日志（仅管理员）。 */
export const getAuditLog = async (limit = 100): Promise<AuditLogResponse> => {
  try {
    const response = await api.get<AuditLogResponse>(`/api/audit/log?limit=${limit}`);
    return { ok: response.data.ok, events: response.data.events ?? [] };
  } catch (error) {
    if (axios.isAxiosError(error)) {
      return {
        ok: false,
        events: [],
        error: error.response?.data?.error || '获取审计日志失败',
      };
    }
    throw error;
  }
};

/** 上报操作事件（导出、打印等）。 */
export const auditExport = async (
  action: 'EXPORT_IMG' | 'EXPORT_PDF' | 'PRINT',
  detail?: string,
): Promise<void> => {
  if (!AuthTokens.isAuthenticated()) return;

  try {
    await api.post('/api/audit', { action, detail });
  } catch (error) {
    console.warn('Audit logging failed:', error);
  }
};
