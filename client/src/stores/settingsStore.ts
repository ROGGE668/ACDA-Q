import { create } from "zustand";
import { isTauri, BrowserStore, IStore } from "./web-compat";

const isDev = import.meta.env.DEV;

interface Settings {
  apiBase: string;
  theme: "dark" | "light";
}

interface SettingsState extends Settings {
  loaded: boolean;
  setApiBase: (v: string) => Promise<void>;
  setTheme: (v: "dark" | "light") => Promise<void>;
  loadSettings: () => Promise<void>;
}

let storeInstance: IStore | null = null;

async function getStore(): Promise<IStore> {
  if (!storeInstance) {
    if (isTauri()) {
      const { Store } = await import("@tauri-apps/plugin-store");
      storeInstance = await Store.load("settings.json") as unknown as IStore;
    } else {
      storeInstance = new BrowserStore("settings");
    }
  }
  return storeInstance;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  apiBase: import.meta.env.VITE_API_BASE || "",
  theme: "dark",
  loaded: false,

  loadSettings: async () => {
    try {
      const store = await getStore();
      const theme = await store.get("theme") as "dark" | "light" | undefined;

      // 生产环境：API 地址使用构建时注入的值，不从本地存储加载
      let apiBase = import.meta.env.VITE_API_BASE || "";
      if (isDev) {
        const storedApiBase = await store.get("api_base");
        if (storedApiBase) {
          apiBase = storedApiBase;
        }
      }

      set({
        apiBase,
        theme: theme ?? get().theme,
        loaded: true,
      });
    } catch (e) {
      console.error("Failed to load settings:", e);
      set({ loaded: true });
    }
  },

  setApiBase: async (v) => {
    // 生产环境不允许修改 API 地址
    if (!isDev) return;
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


}));
