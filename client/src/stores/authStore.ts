import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
function getOsType(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("win")) return "windows";
  if (ua.includes("mac")) return "macos";
  if (ua.includes("linux")) return "linux";
  return "unknown";
}
import { authAPI, deviceAPI } from "../services/api";
import { clearTokens, onTokenSync } from "./tokenStore";

interface User {
  id: string;
  email: string;
  nickname?: string;
  tier: string;
  quota_ai_daily?: number;
  ai_used_today?: number;
  quota_backtest_daily?: number;
}

interface AuthState {
  user: User | null;
  setUser: (user: User | null) => void;
  fetchMe: () => Promise<void>;
  logout: () => void;
}

async function registerDevice() {
  try {
    const fingerprint = await invoke<string>("get_device_fingerprint");
    const osType = getOsType();
    await deviceAPI.register({
      device_fingerprint: fingerprint,
      device_name: `${osType} Device`,
      os_type: osType,
    });
  } catch (e) {
    console.error("Device registration failed:", e);
  }
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  setUser: (user) => set({ user }),
  fetchMe: async () => {
    const { data } = await authAPI.me();
    set({ user: data });
    // 登录成功后注册设备
    await registerDevice();
  },
  logout: async () => {
    await clearTokens();
    set({ user: null });
  },
}));

// Multi-tab sync: if another tab refreshes token, re-fetch user info
onTokenSync(() => {
  const { user, fetchMe } = useAuthStore.getState();
  if (user) {
    fetchMe().catch(() => {});
  }
});
