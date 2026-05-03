import { fetch } from "@tauri-apps/plugin-http";
import { invoke } from "@tauri-apps/api/core";
import { clearTokens, getAccessToken, getRefreshToken, setTokens } from "../stores/tokenStore";
import { useSettingsStore } from "../stores/settingsStore";

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

// 添加日志函数（生产时可关闭）
const debug = (...args: any[]) => {
  if (import.meta.env.DEV) console.log("[API]", ...args);
};

function generateRequestId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

let cachedFingerprint: string | null = null;

async function getDeviceFingerprint(): Promise<string | null> {
  if (cachedFingerprint) return cachedFingerprint;
  try {
    // 优先使用 Tauri 原生接口获取硬件指纹
    const fp = await invoke<string>("get_device_fingerprint");
    cachedFingerprint = fp;
    return fp;
  } catch (e) {
    // 降级：非 Tauri 环境使用浏览器指纹
    debug("Tauri fingerprint unavailable, using fallback:", e);
    try {
      const fallback = `${navigator.userAgent}|${screen.width}x${screen.height}|${navigator.language}`;
      cachedFingerprint = fallback;
      return fallback;
    } catch (_) {
      return null;
    }
  }
}

async function request(method: string, url: string, data?: any, config?: any) {
  // Wait for settings to load if API_BASE is not set yet
  if (!API_BASE && !url.startsWith("http")) {
    await ensureSettingsLoaded();
  }

  const fullUrl = url.startsWith("http") ? url : `${API_BASE}${url}`;

  if (!API_BASE && !url.startsWith("http")) {
    throw new Error("API base URL not configured. Please set it in Settings.");
  }

  debug(method, fullUrl);

  const fingerprint = await getDeviceFingerprint();
  const token = await getAccessToken();

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Request-ID": generateRequestId(),
    ...(config?.headers || {}),
  };

  if (token) {
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
    const response = await fetch(fullUrl, options);

    if (response.status === 401) {
      if (!config?._retry) {
        try {
          // plugin-http 不共享 WebView Cookie，直接用 Header 刷新
          const refreshTok = await getRefreshToken();
          if (refreshTok) {
            const refreshRes = await fetch(`${API_BASE}/auth/refresh`, {
              method: "POST",
              headers: {
                "Content-Type": "application/json",
                "Authorization": `Bearer ${refreshTok}`,
              },
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
        window.location.href = "/login";
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
  get: (url: string, config?: any) => request("GET", url, undefined, config),
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

export const backtestAPI = {
  submit: (data: any) => api.post("/backtests", data),
  list: () => api.get("/backtests"),
  get: (id: string) => api.get(`/backtests/${id}`),
  result: (id: string) => api.get(`/backtests/${id}/result`),
  chart: (id: string, agg?: string) =>
    api.get(`/backtests/${id}/chart${agg ? `?agg=${agg}` : ""}`),
  trades: (id: string, page: number = 1, pageSize: number = 50) =>
    api.get(`/backtests/${id}/trades?page=${page}&page_size=${pageSize}`),
};

export const deviceAPI = {
  register: (data: any) => api.post("/devices/register", data),
  heartbeat: (data: any) => api.post("/devices/heartbeat", data),
  list: () => api.get("/devices"),
  revoke: (id: string) => api.post(`/devices/${id}/revoke`),
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
  search: (query: string) =>
    api.get(`/market/stocks?search=${encodeURIComponent(query)}`),
  history: (symbol: string, start: string, end: string) =>
    api.get(`/market/history/${symbol}?start_date=${start}&end_date=${end}`),
};

export const aiAPI = {
  generate: (prompt: string, model?: string) =>
    api.post("/ai/generate", { prompt, model }),
  extractParams: (code: string) =>
    api.post("/ai/extract-params", { code }),
};
