import { clearTokens, getAccessToken, getRefreshToken, setTokens } from "../stores/tokenStore";
import { useSettingsStore } from "../stores/settingsStore";
import { isTauri } from "../stores/web-compat";

let API_BASE = import.meta.env.VITE_API_BASE || "";
let settingsLoaded = false;
let settingsPromise: Promise<void> | null = null;

export function setApiBase(url: string) {
  API_BASE = url;
}

export function getApiBase(): string {
  return API_BASE;
}

async function ensureSettingsLoaded(): Promise<void> {
  if (settingsLoaded) return;
  if (API_BASE) { settingsLoaded = true; return; }
  if (!settingsPromise) {
    settingsPromise = useSettingsStore.getState().loadSettings().then(() => {
      const state = useSettingsStore.getState();
      if (state.apiBase) {
        API_BASE = state.apiBase;
      }
      settingsLoaded = true;
    });
  }
  await settingsPromise;
}

const debug = (...args: any[]) => {
  if (import.meta.env.DEV) console.log("[API]", ...args);
};

function generateRequestId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

import { getDeviceFingerprint } from "../utils/fingerprint";

// 浏览器中使用原生 fetch（带 cookie），Tauri 中使用 plugin-http
async function compatFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  if (isTauri()) {
    const { fetch: tauriFetch } = await import("@tauri-apps/plugin-http");
    return tauriFetch(input as string | URL, init);
  }
  // 浏览器模式: credentials=include 确保 httpOnly cookie 自动携带
  return fetch(input, { ...init, credentials: "include" });
}

async function request(method: string, url: string, data?: any, config?: any) {
  if (!API_BASE && !url.startsWith("http")) {
    await ensureSettingsLoaded();
  }

  const fullUrl = url.startsWith("http") ? url : `${API_BASE}${url}`;

  if (!fullUrl) {
    throw new Error("API base URL not configured. Please set it in Settings.");
  }

  debug(method, fullUrl);

  const fingerprint: string | null = await getDeviceFingerprint().catch(() => null);
  const token = await getAccessToken();

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Request-ID": generateRequestId(),
    ...(config?.headers || {}),
  };

  // Tauri 模式使用 Authorization header，浏览器模式使用 httpOnly cookie
  if (isTauri() && token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  if (fingerprint) {
    headers["X-Device-Fingerprint"] = fingerprint;
  }

  const options: RequestInit = {
    method,
    headers,
  };
  if (data) {
    options.body = JSON.stringify(data);
  }

  try {
    const response = await compatFetch(fullUrl, options);

    if (response.status === 401) {
      if (!config?._retry) {
        try {
          const refreshTok = await getRefreshToken();
          if (refreshTok) {
            const refreshRes = await compatFetch(`${API_BASE}/auth/refresh`, {
              method: "POST",
              headers: {
                "Content-Type": "application/json",
                ...(isTauri() ? { "Authorization": `Bearer ${refreshTok}` } : {}),
              },
              body: JSON.stringify({ refresh_token: refreshTok }),
            });
            if (refreshRes.ok) {
              const refreshData = await refreshRes.json();
              if (refreshData.access_token && refreshData.refresh_token) {
                await setTokens(refreshData.access_token, refreshData.refresh_token);
              }
              return request(method, url, data, { ...config, _retry: true });
            }
          }
        } catch (e) {
          debug("Refresh failed", e);
        }
        await clearTokens();
        const confirmed = window.confirm("登录已过期，请重新登录。\n\n点击\"确定\"跳转到登录页面。");
        if (confirmed) {
          window.location.href = "/login";
        }
        throw new Error("Session expired");
      }
    }

    if (!response.ok) {
      const err: any = new Error(`HTTP ${response.status}`);
      err.response = { status: response.status, data: await response.json().catch(() => ({})) };
      console.error("[API] Request failed:", err.response);
      throw err;
    }

    const resData = await response.json().catch(() => ({}));
    debug("Response OK", resData);
    return { data: resData, status: response.status, statusText: response.statusText };
  } catch (err) {
    console.error("[API] Fetch error:", err);
    throw err;
  }
}

const api = {
  get: <T = any>(url: string, config?: any) => request("GET", url, undefined, config) as Promise<{ data: T }>,
  post: (url: string, data?: any, config?: any) => request("POST", url, data, config),
  put: (url: string, data?: any, config?: any) => request("PUT", url, data, config),
  delete: (url: string, config?: any) => request("DELETE", url, undefined, config),
};

export default api;

export const authAPI = {
  register: (email: string, password: string, nickname?: string) =>
    api.post("/auth/register", { email, password, nickname }),
  login: (email: string, password: string) =>
    api.post("/auth/login", { email, password }),
  logout: () => api.post("/auth/logout"),
  me: () => api.get("/auth/me"),
};

export const strategyAPI = {
  list: () => api.get("/strategies"),
  get: (id: string) => api.get(`/strategies/${id}`),
  create: (data: any) => api.post("/strategies", data),
  update: (id: string, data: any) => api.put(`/strategies/${id}`, data),
  delete: (id: string) => api.delete(`/strategies/${id}`),
  validate: (code: string) => api.post("/strategies/validate", { code }),
};

export interface BacktestJob {
  id: string;
  status: string;
  scope?: string;
  symbols?: string[];
  start_date?: string;
  end_date?: string;
  initial_cash?: number;
  result_summary?: BacktestResultSummary;
  error_message?: string;
  period?: string;
  created_at: string;
}

export interface BacktestResultSummary {
  total_return?: number;
  annual_return?: number;
  max_drawdown?: number;
  sharpe_ratio?: number;
  sortino_ratio?: number;
  calmar_ratio?: number;
  win_rate?: number;
  total_commission?: number;
  total_trades?: number;
  final_value?: number;
  monthly_returns?: MonthlyReturn[];
  avg_return?: number;
  median_return?: number;
  avg_sharpe?: number;
  avg_drawdown?: number;
  total_signals?: number;
  scanned_count?: number;
  suitable_count?: number;
}

export interface MonthlyReturn {
  month: string;
  return: number;
}

export interface BacktestResult {
  summary: BacktestResultSummary;
  trades?: Trade[];
  signals?: Signal[];
  suitable_stocks?: SuitableStock[];
}

export interface Trade {
  timestamp: string;
  symbol: string;
  type: "buy" | "sell";
  amount?: number;
  price: number;
  pnl?: number;
}

export interface Signal {
  symbol: string;
  direction: "buy" | "sell";
  timestamp: string;
  price: number;
  score: number;
}

export interface SuitableStock {
  symbol: string;
  score: number;
  total_return: number;
  max_drawdown: number;
  sharpe_ratio?: number;
  total_trades: number;
}

export interface KLineData {
  datetime: string;
  open: number;
  high: number;
  low: number;
  close: number;
  volume?: number;
}

export interface PaginatedTrades {
  items: Trade[];
  total: number;
  page: number;
  page_size: number;
}

export const backtestAPI = {
  submit: (data: any) => api.post("/backtests", data),
  list: () => api.get("/backtests"),
  get: (id: string) => api.get<BacktestJob>(`/backtests/${id}`),
  result: (id: string) => api.get<BacktestResult>(`/backtests/${id}/result`),
  chart: (id: string, agg?: string) =>
    api.get<KLineData[]>(`/backtests/${id}/chart${agg ? `?agg=${agg}` : ""}`),
  trades: (id: string, page: number = 1, pageSize: number = 50) =>
    api.get<PaginatedTrades>(`/backtests/${id}/trades?page=${page}&page_size=${pageSize}`),
  remove: (id: string) => api.delete(`/backtests/${id}`),
};

export const deviceAPI = {
  register: (data: any) => api.post("/devices/register", data),
  heartbeat: (data: any) => api.post("/devices/heartbeat", data),
  list: () => api.get("/devices"),
  revoke: (id: string) => api.post(`/devices/${id}/revoke`),
  delete: (id: string) => api.delete(`/devices/${id}`),
};

export const subscriptionAPI = {
  status: () => api.get("/subscription"),
};

export const paymentAPI = {
  create: (data: any) => api.post("/payments", data),
  get: (orderNo: string) => api.get(`/payments/${orderNo}`),
  cancel: (orderNo: string) => api.post(`/payments/${orderNo}/cancel`),
};

export const marketAPI = {
  search: (query: string, exchange?: string) =>
    api.get(`/market/stocks?search=${encodeURIComponent(query)}${exchange ? `&exchange=${exchange}` : ""}`),
  history: (symbol: string, start: string, end: string, exchange?: string, period?: string) =>
    api.get(`/market/history/${symbol}?start_date=${start}&end_date=${end}${exchange ? `&exchange=${exchange}` : ""}${period ? `&period=${period}` : ""}`),
};

export const aiAPI = {
  generate: (prompt: string, model?: string) =>
    api.post("/ai/generate", { prompt, model }),
  extractParams: (code: string) =>
    api.post("/ai/extract-params", { code }),
};
