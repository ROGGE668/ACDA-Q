import { create } from "zustand";
import { Store } from "@tauri-apps/plugin-store";

interface Settings {
  apiBase: string;
  theme: "dark" | "light";
  globalShortcutEnabled: boolean;
}

interface SettingsState extends Settings {
  loaded: boolean;
  setApiBase: (v: string) => Promise<void>;
  setTheme: (v: "dark" | "light") => Promise<void>;
  setGlobalShortcutEnabled: (v: boolean) => Promise<void>;
  loadSettings: () => Promise<void>;
}

let storeInstance: Store | null = null;

async function getStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = await Store.load("settings.json");
  }
  return storeInstance;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  apiBase: import.meta.env.VITE_API_BASE || "http://124.220.70.210:8000/api/v1",
  theme: "dark",
  globalShortcutEnabled: true,
  loaded: false,

  loadSettings: async () => {
    try {
      const store = await getStore();
      const apiBase = await store.get<string>("api_base");
      const theme = await store.get<"dark" | "light">("theme");
      const shortcut = await store.get<boolean>("global_shortcut_enabled");

      // 如果 Store 中的 api_base 无效（空或 localhost），使用默认值
      const validApiBase =
        apiBase && !apiBase.includes("localhost") && !apiBase.includes("127.0.0.1")
          ? apiBase
          : get().apiBase;

      set({
        apiBase: validApiBase,
        theme: theme ?? get().theme,
        globalShortcutEnabled: shortcut ?? get().globalShortcutEnabled,
        loaded: true,
      });
    } catch (e) {
      console.error("Failed to load settings:", e);
      set({ loaded: true });
    }
  },

  setApiBase: async (v) => {
    try {
      const store = await getStore();
      await store.set("api_base", v);
      await store.save();
      set({ apiBase: v });
    } catch (e) {
      console.error("Failed to save api_base:", e);
    }
  },

  setTheme: async (v) => {
    try {
      const store = await getStore();
      await store.set("theme", v);
      await store.save();
      set({ theme: v });
      document.documentElement.setAttribute("data-theme", v);
    } catch (e) {
      console.error("Failed to save theme:", e);
    }
  },

  setGlobalShortcutEnabled: async (v) => {
    try {
      const store = await getStore();
      await store.set("global_shortcut_enabled", v);
      await store.save();
      set({ globalShortcutEnabled: v });
    } catch (e) {
      console.error("Failed to save shortcut setting:", e);
    }
  },
}));
