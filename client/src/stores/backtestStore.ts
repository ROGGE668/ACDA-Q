import { create } from "zustand";
import { backtestAPI } from "../services/api";
import type { BacktestResultSummary } from "../services/api";

interface BacktestJob {
  id: string;
  status: string;
  symbols?: string[];
  start_date?: string;
  end_date?: string;
  result_summary?: BacktestResultSummary;
  created_at: string;
}

interface BacktestState {
  jobs: BacktestJob[];
  loading: boolean;
  fetchJobs: () => Promise<void>;
}

export const useBacktestStore = create<BacktestState>((set) => ({
  jobs: [],
  loading: false,
  fetchJobs: async () => {
    set({ loading: true });
    try {
      const { data } = await backtestAPI.list();
      set({ jobs: data, loading: false });
    } catch (e) {
      console.error("[backtestStore] fetchJobs failed:", e);
      set({ loading: false });
    }
  },
}));
