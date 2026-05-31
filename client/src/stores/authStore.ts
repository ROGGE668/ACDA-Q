import { create } from "zustand";
import { persist } from "zustand/middleware";
import { authAPI, deviceAPI } from "../services/api";
import { setTokens, clearTokens, getAccessToken } from "./tokenStore";
import { getDeviceFingerprint } from "../utils/fingerprint";

interface User {
  id: string;
  email: string;
  nickname: string;
  tier: string;
  is_admin: boolean;
  quota_ai_daily: number;
  ai_used_today: number;
  created_at: string;
}

interface AuthState {
  user: User | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  lastAuthCheck: number;
  init: () => Promise<void>;
  login: (email: string, password: string) => Promise<void>;
  register: (email: string, password: string, nickname?: string) => Promise<void>;
  logout: () => Promise<void>;
  fetchUser: () => Promise<void>;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      user: null,
      isAuthenticated: false,
      isLoading: false,
      lastAuthCheck: 0,

      init: async () => {
        const token = await getAccessToken();
        if (token) {
          await get().fetchUser();
        }
      },

      login: async (email, password) => {
        set({ isLoading: true });
        try {
          const res = await authAPI.login(email, password);
          const { access_token, refresh_token } = res.data;
          await setTokens(access_token, refresh_token);
          await get().fetchUser();
        } finally {
          set({ isLoading: false });
        }
      },

      register: async (email, password, nickname) => {
        set({ isLoading: true });
        try {
          const res = await authAPI.register(email, password, nickname);
          const { access_token, refresh_token } = res.data;
          await setTokens(access_token, refresh_token);
          await get().fetchUser();
        } finally {
          set({ isLoading: false });
        }
      },

      logout: async () => {
        const currentPath = window.location.pathname;
        await clearTokens();
        set({ user: null, isAuthenticated: false });
        try {
          await authAPI.logout();
        } catch (_) {}
        if (currentPath !== "/login") {
          window.location.href = "/login";
        }
      },

      fetchUser: async () => {
        try {
          const res = await authAPI.me();
          set({ user: res.data, isAuthenticated: true, lastAuthCheck: Date.now() });
          await registerDeviceIfNeeded();
        } catch (e: any) {
          // 任何错误都清除用户状态，让 PrivateRoute 跳转登录
          set({ user: null, isAuthenticated: false });
          if (e?.response?.status === 401) {
            await clearTokens();
          }
        }
      },
    }),
    {
      name: "auth-store",
      partialize: (state) => ({ user: state.user, isAuthenticated: state.isAuthenticated, lastAuthCheck: state.lastAuthCheck }),
      onRehydrateStorage: () => (state) => {
        if (state) {
          const { isAuthenticated, lastAuthCheck } = state;
          const DAY_MS = 24 * 60 * 60 * 1000;
          if (isAuthenticated && (!lastAuthCheck || Date.now() - lastAuthCheck > DAY_MS)) {
            state.isAuthenticated = false;
            state.user = null;
          }
        }
      },
    }
  )
);

async function registerDeviceIfNeeded() {
  try {
    const fingerprint = await getDeviceFingerprint();
    await deviceAPI.register({
      device_fingerprint: fingerprint,
      device_name: navigator.userAgent.includes("Codex") ? "ACDA-Quant Desktop" : "ACDA-Quant Web",
      os_type: navigator.platform || "unknown",
    });
  } catch (e) {
    console.warn("[Device] register failed:", e);
  }
}
