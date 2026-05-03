import { create } from "zustand";
import { backtestAPI } from "../services/api";

interface BacktestJob {
  id: string;
  status: string;
  symbols?: string[];
  start_date?: string;
  end_date?: string;
  result_summary?: Record<string, any>;
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
    const { data } = await backtestAPI.list();
    set({ jobs: data, loading: false });
  },
}));
