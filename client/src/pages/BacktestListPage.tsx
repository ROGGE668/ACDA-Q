import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { backtestAPI, strategyAPI } from "../services/api";
import StockSelector from "../components/StockSelector";

interface BacktestJob {
  id: string;
  status: string;
  scope?: string;
  symbols?: string[];
  start_date?: string;
  end_date?: string;
  result_summary?: Record<string, any>;
  created_at: string;
}

interface StrategyItem {
  id: string;
  name: string;
}

export default function BacktestListPage() {
  const navigate = useNavigate();
  const [jobs, setJobs] = useState<BacktestJob[]>([]);
  const [loading, setLoading] = useState(false);

  const [showNew, setShowNew] = useState(false);
  const [strategies, setStrategies] = useState<StrategyItem[]>([]);
  const [selectedStrategy, setSelectedStrategy] = useState("");
  const [startDate, setStartDate] = useState("2024-01-01");
  const [endDate, setEndDate] = useState("2024-06-01");
  const [initialCash, setInitialCash] = useState(1_000_000);
  const [fullMarketScan, setFullMarketScan] = useState(false);

  const [selectedStocks, setSelectedStocks] = useState<string[]>([]);

  const [submitting, setSubmitting] = useState(false);

  const loadJobs = async () => {
    setLoading(true);
    const { data } = await backtestAPI.list();
    setJobs(data);
    setLoading(false);
  };

  useEffect(() => {
    loadJobs();
  }, []);

  useEffect(() => {
    if (showNew) {
      strategyAPI.list().then((res) => setStrategies(res.data));
    }
  }, [showNew]);

  const toggleStock = (symbol: string) => {
    setSelectedStocks((prev) =>
      prev.includes(symbol) ? prev.filter((s) => s !== symbol) : [...prev, symbol]
    );
  };

  const submitBacktest = async () => {
    if (!selectedStrategy) {
      alert("请选择策略");
      return;
    }
    if (!fullMarketScan && selectedStocks.length === 0) {
      alert("请至少选择一只股票，或开启全市场扫描");
      return;
    }
    setSubmitting(true);
    try {
      const { data } = await backtestAPI.submit({
        strategy_id: selectedStrategy,
        symbols: fullMarketScan ? [] : selectedStocks,
        start_date: startDate,
        end_date: endDate,
        initial_cash: initialCash,
      });
      setShowNew(false);
      navigate(`/backtests/${data.id}`);
    } catch (e: any) {
      alert(e.response?.data?.detail || "回测提交失败");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h1>回测记录</h1>
        <div style={{ display: "flex", gap: "0.5rem" }}>
          <button className="secondary" onClick={loadJobs} disabled={loading}>
            {loading ? "刷新中..." : "刷新"}
          </button>
          <button onClick={() => setShowNew((v) => !v)}>
            {showNew ? "取消" : "发起回测"}
          </button>
        </div>
      </div>

      {showNew && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>发起新回测</h3>
          <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "0.75rem" }}>
            <div>
              <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>选择策略</label>
              <select
                value={selectedStrategy}
                onChange={(e) => setSelectedStrategy(e.target.value)}
              >
                <option value="">-- 请选择策略 --</option>
                {strategies.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </div>

            <StockSelector
              selectedStocks={selectedStocks}
              onToggleStock={toggleStock}
              fullMarketScan={fullMarketScan}
              onFullMarketScanChange={setFullMarketScan}
            />

            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "0.75rem" }}>
              <div>
                <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>开始日期</label>
                <input type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} />
              </div>
              <div>
                <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>结束日期</label>
                <input type="date" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
              </div>
              <div>
                <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>初始资金</label>
                <input type="number" value={initialCash} onChange={(e) => setInitialCash(Number(e.target.value))} />
              </div>
            </div>

            <div>
              <button onClick={submitBacktest} disabled={submitting}>
                {submitting ? "提交中..." : "运行回测"}
              </button>
            </div>
          </div>
        </div>
      )}

      <div style={{ marginTop: "1rem", display: "flex", flexDirection: "column", gap: "0.75rem" }}>
        {jobs.map((job) => (
          <Link key={job.id} to={`/backtests/${job.id}`} style={{ textDecoration: "none" }}>
            <div className="card" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <div>
                <span style={{ fontWeight: 600 }}>
                  {job.symbols?.slice(0, 3).join(", ") || "全市场扫描"}
                  {job.symbols && job.symbols.length > 3 ? ` +${job.symbols.length - 3}` : ""}
                </span>
                <span style={{ color: "var(--muted)", marginLeft: "1rem", fontSize: "0.875rem" }}>
                  {job.start_date?.slice(0, 10)} ~ {job.end_date?.slice(0, 10)}
                </span>
                {job.result_summary?.total_return !== undefined && (
                  <span
                    style={{
                      marginLeft: "1rem",
                      fontSize: "0.875rem",
                      color: job.result_summary.total_return >= 0 ? "#22c55e" : "#ef4444",
                    }}
                  >
                    收益: {(job.result_summary.total_return * 100).toFixed(2)}%
                  </span>
                )}
              </div>
              <span
                style={{
                  padding: "0.25rem 0.5rem",
                  borderRadius: "0.25rem",
                  fontSize: "0.75rem",
                  fontWeight: 600,
                  background:
                    job.status === "success"
                      ? "#064e3b"
                      : job.status === "failed"
                      ? "#450a0a"
                      : "#1e3a8a",
                  color:
                    job.status === "success"
                      ? "#22c55e"
                      : job.status === "failed"
                      ? "#ef4444"
                      : "#38bdf8",
                }}
              >
                {job.status}
              </span>
            </div>
          </Link>
        ))}
        {jobs.length === 0 && (
          <div className="card" style={{ textAlign: "center", color: "var(--muted)" }}>
            暂无回测记录，点击右上角「发起回测」开始。
          </div>
        )}
      </div>
    </div>
  );
}
