import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { persist } from "zustand/middleware";
import { authAPI, deviceAPI } from "../services/api";
import { setTokens, clearTokens, getAccessToken } from "./tokenStore";

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
        try {
          await authAPI.logout();
        } catch (_) {
          // ignore
        }
        await clearTokens();
        set({ user: null, isAuthenticated: false });
        window.location.href = "/login";
      },

      fetchUser: async () => {
        try {
          const res = await authAPI.me();
          set({ user: res.data, isAuthenticated: true });
          // 登录成功后自动注册设备
          await registerDeviceIfNeeded();
        } catch (e) {
          set({ user: null, isAuthenticated: false });
          await clearTokens();
        }
      },
    }),
    {
      name: "auth-store",
      partialize: (state) => ({ user: state.user, isAuthenticated: state.isAuthenticated }),
    }
  )
);

async function registerDeviceIfNeeded() {
  try {
    let fingerprint: string;
    try {
      fingerprint = await invoke<string>("get_device_fingerprint");
    } catch (e) {
      // 非 Tauri 环境使用降级指纹
      fingerprint = `${navigator.userAgent}|${screen.width}x${screen.height}|${navigator.language}`;
    }
    await deviceAPI.register({
      device_fingerprint: fingerprint,
      device_name: "ACDA-Quant Client",
      device_type: "desktop",
    });
  } catch (e) {
    console.warn("[Device] register failed:", e);
  }
}
