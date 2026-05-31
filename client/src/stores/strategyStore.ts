import { create } from "zustand";
import { strategyAPI } from "../services/api";

export interface Strategy {
  id: string;
  name: string;
  description?: string;
  strategy_type?: string;
  code: string;
  params?: Record<string, any>;
  version?: number;
  user_id?: string;
  created_at?: string;
  updated_at?: string;
}

interface StrategyState {
  strategies: Strategy[];
  loading: boolean;
  loaded: boolean;
  error: string | null;
  fetchStrategies: () => Promise<void>;
  getStrategy: (id: string) => Strategy | undefined;
  saveStrategy: (data: {
    id?: string;
    name: string;
    description?: string;
    code: string;
    type?: string;
    params?: Record<string, any>;
  }) => Promise<Strategy>;
  removeStrategy: (id: string) => Promise<void>;
}

export const useStrategyStore = create<StrategyState>((set, get) => ({
  strategies: [],
  loading: false,
  loaded: false,
  error: null,

  fetchStrategies: async () => {
    set({ loading: true, error: null });
    try {
      const { data } = await strategyAPI.list();
      set({ strategies: data || [], loading: false, loaded: true });
    } catch (e: any) {
      const msg = e.response?.data?.error || e.message || "加载策略失败";
      set({ error: msg, loading: false, loaded: true });
    }
  },

  getStrategy: (id: string) => {
    return get().strategies.find((s) => s.id === id);
  },

  saveStrategy: async (strategy) => {
    if (strategy.id) {
      const { data } = await strategyAPI.update(strategy.id, {
        name: strategy.name,
        description: strategy.description,
        code: strategy.code,
        params: strategy.params,
      });
      set((state) => ({
        strategies: state.strategies.map((s) => (s.id === strategy.id ? data : s)),
      }));
      return data;
    } else {
      const { data } = await strategyAPI.create({
        name: strategy.name,
        description: strategy.description,
        code: strategy.code,
        strategy_type: strategy.type || "single_stock",
        params: strategy.params || {},
      });
      set((state) => ({
        strategies: [...state.strategies, data],
      }));
      return data;
    }
  },

  removeStrategy: async (id: string) => {
    try {
      await strategyAPI.delete(id);
    } catch (e: any) {
      if (e.response?.status !== 404) throw e;
    }
    set((state) => ({
      strategies: state.strategies.filter((s) => s.id !== id),
    }));
  },
}));
