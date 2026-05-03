import { create } from "zustand";
import { Store } from "@tauri-apps/plugin-store";

interface Strategy {
  id: string;
  name: string;
  description?: string;
  type?: string;
  code?: string;
  params?: Record<string, any>;
  created_at: string;
  updated_at: string;
}

interface StrategyState {
  strategies: Strategy[];
  loading: boolean;
  loaded: boolean;
  fetchStrategies: () => Promise<void>;
  getStrategy: (id: string) => Strategy | undefined;
  saveStrategy: (strategy: Omit<Strategy, "id" | "created_at" | "updated_at"> & { id?: string }) => Promise<Strategy>;
  removeStrategy: (id: string) => Promise<void>;
}

let storeInstance: Store | null = null;

async function getStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = await Store.load("strategies.json");
  }
  return storeInstance;
}

function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

export const useStrategyStore = create<StrategyState>((set, get) => ({
  strategies: [],
  loading: false,
  loaded: false,

  fetchStrategies: async () => {
    set({ loading: true });
    try {
      const store = await getStore();
      const data = await store.get<Strategy[]>("strategies");
      set({ strategies: data || [], loading: false, loaded: true });
    } catch (e) {
      console.error("Failed to load strategies:", e);
      set({ strategies: [], loading: false, loaded: true });
    }
  },

  getStrategy: (id: string) => {
    return get().strategies.find((s) => s.id === id);
  },

  saveStrategy: async (strategy) => {
    const store = await getStore();
    const existing = strategy.id ? get().strategies.find((s) => s.id === strategy.id) : undefined;
    const now = new Date().toISOString();

    let saved: Strategy;
    if (existing) {
      saved = {
        ...existing,
        ...strategy,
        updated_at: now,
      } as Strategy;
    } else {
      saved = {
        ...strategy,
        id: generateId(),
        created_at: now,
        updated_at: now,
      } as Strategy;
    }

    const updated = existing
      ? get().strategies.map((s) => (s.id === saved.id ? saved : s))
      : [...get().strategies, saved];

    await store.set("strategies", updated);
    await store.save();
    set({ strategies: updated });
    return saved;
  },

  removeStrategy: async (id: string) => {
    const store = await getStore();
    const updated = get().strategies.filter((s) => s.id !== id);
    await store.set("strategies", updated);
    await store.save();
    set({ strategies: updated });
  },
}));
