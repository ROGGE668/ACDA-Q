import { create } from "zustand";
import { Store } from "@tauri-apps/plugin-store";

interface AISettings {
  deepseekApiKey: string;
  deepseekBaseUrl: string;
  deepseekModel: string;
}

interface AISettingsState extends AISettings {
  loaded: boolean;
  setDeepseekApiKey: (v: string) => Promise<void>;
  setDeepseekBaseUrl: (v: string) => Promise<void>;
  setDeepseekModel: (v: string) => Promise<void>;
  loadSettings: () => Promise<void>;
}

let storeInstance: Store | null = null;

async function getStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = await Store.load("ai_settings.json");
  }
  return storeInstance;
}

export const useAISettingsStore = create<AISettingsState>((set, get) => ({
  deepseekApiKey: "",
  deepseekBaseUrl: "https://api.deepseek.com",
  deepseekModel: "deepseek-chat",
  loaded: false,

  loadSettings: async () => {
    try {
      const store = await getStore();
      const apiKey = await store.get<string>("deepseek_api_key");
      const baseUrl = await store.get<string>("deepseek_base_url");
      const model = await store.get<string>("deepseek_model");
      set({
        deepseekApiKey: apiKey ?? get().deepseekApiKey,
        deepseekBaseUrl: baseUrl ?? get().deepseekBaseUrl,
        deepseekModel: model ?? get().deepseekModel,
        loaded: true,
      });
    } catch (e) {
      console.error("Failed to load AI settings:", e);
      set({ loaded: true });
    }
  },

  setDeepseekApiKey: async (v) => {
    try {
      const store = await getStore();
      await store.set("deepseek_api_key", v);
      await store.save();
      set({ deepseekApiKey: v });
    } catch (e) {
      console.error("Failed to save API key:", e);
    }
  },

  setDeepseekBaseUrl: async (v) => {
    try {
      const store = await getStore();
      await store.set("deepseek_base_url", v);
      await store.save();
      set({ deepseekBaseUrl: v });
    } catch (e) {
      console.error("Failed to save base URL:", e);
    }
  },

  setDeepseekModel: async (v) => {
    try {
      const store = await getStore();
      await store.set("deepseek_model", v);
      await store.save();
      set({ deepseekModel: v });
    } catch (e) {
      console.error("Failed to save model:", e);
    }
  },
}));
