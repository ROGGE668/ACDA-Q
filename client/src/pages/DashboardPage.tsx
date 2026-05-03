import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useAuthStore } from "../stores/authStore";
import { useStrategyStore } from "../stores/strategyStore";
import { useBacktestStore } from "../stores/backtestStore";
import { backtestAPI } from "../services/api";
import StockSelector from "../components/StockSelector";

export default function DashboardPage() {
  const navigate = useNavigate();
  const { user } = useAuthStore();
  const { strategies, fetchStrategies } = useStrategyStore();
  const { jobs, fetchJobs } = useBacktestStore();

  const [showQuickBacktest, setShowQuickBacktest] = useState(false);
  const [qbStrategy, setQbStrategy] = useState("");
  const [qbStart, setQbStart] = useState("2024-01-01");
  const [qbEnd, setQbEnd] = useState("2024-06-01");
  const [qbCash, setQbCash] = useState(1_000_000);
  const [qbFullMarket, setQbFullMarket] = useState(false);
  const [qbSelectedStocks, setQbSelectedStocks] = useState<string[]>([]);
  const [qbSubmitting, setQbSubmitting] = useState(false);

  useEffect(() => {
    fetchStrategies();
    fetchJobs();
  }, [fetchStrategies, fetchJobs]);

  const successJobs = jobs.filter((j) => j.status === "success").length;
  const failedJobs = jobs.filter((j) => j.status === "failed").length;

  const toggleQuickStock = (symbol: string) => {
    setQbSelectedStocks((prev) =>
      prev.includes(symbol) ? prev.filter((s) => s !== symbol) : [...prev, symbol]
    );
  };

  const submitQuickBacktest = async () => {
    if (!qbStrategy) {
      alert("请选择策略");
      return;
    }
    if (!qbFullMarket && qbSelectedStocks.length === 0) {
      alert("请至少选择一只股票，或开启全市场扫描");
      return;
    }
    setQbSubmitting(true);
    try {
      const { data } = await backtestAPI.submit({
        strategy_id: qbStrategy,
        symbols: qbFullMarket ? [] : qbSelectedStocks,
        start_date: qbStart,
        end_date: qbEnd,
        initial_cash: qbCash,
      });
      navigate(`/backtests/${data.id}`);
    } catch (e: any) {
      alert(e.response?.data?.detail || "回测提交失败");
    } finally {
      setQbSubmitting(false);
    }
  };

  return (
    <div>
      <h1>仪表盘</h1>
      <p style={{ color: "#94a3b8", marginTop: "0.5rem" }}>
        欢迎回来，{user?.nickname || user?.email}
      </p>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: "1rem", marginTop: "1.5rem" }}>
        <div className="card">
          <h3>策略数量</h3>
          <p style={{ fontSize: "1.5rem", marginTop: "0.5rem" }}>{strategies.length}</p>
        </div>
        <div className="card">
          <h3>回测总数</h3>
          <p style={{ fontSize: "1.5rem", marginTop: "0.5rem" }}>{jobs.length}</p>
        </div>
        <div className="card">
          <h3>成功 / 失败</h3>
          <p style={{ fontSize: "1.5rem", marginTop: "0.5rem", color: "#22c55e" }}>
            {successJobs} <span style={{ color: "#ef4444" }}>/ {failedJobs}</span>
          </p>
        </div>
        <div className="card">
          <h3>今日额度（已用/总额）</h3>
          <p style={{ fontSize: "1.5rem", marginTop: "0.5rem" }}>
            AI: {user?.ai_used_today ?? 0}/{user?.quota_ai_daily ?? "--"} / 回测: {user?.quota_ai_daily ?? "--"}
          </p>
        </div>
      </div>

      {/* 快速回测 */}
      <div className="card" style={{ marginTop: "1.5rem" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <h3 style={{ margin: 0 }}>快速回测</h3>
          <button className="secondary" onClick={() => setShowQuickBacktest((v) => !v)}>
            {showQuickBacktest ? "收起" : "展开"}
          </button>
        </div>

        {showQuickBacktest && (
          <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "0.75rem" }}>
            <div>
              <label style={{ fontSize: "0.875rem", color: "#94a3b8" }}>选择策略</label>
              <select value={qbStrategy} onChange={(e) => setQbStrategy(e.target.value)}>
                <option value="">-- 请选择策略 --</option>
                {strategies.map((s: any) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </select>
            </div>

            <StockSelector
              selectedStocks={qbSelectedStocks}
              onToggleStock={toggleQuickStock}
              fullMarketScan={qbFullMarket}
              onFullMarketScanChange={setQbFullMarket}
            />

            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "0.75rem" }}>
              <div>
                <label style={{ fontSize: "0.875rem", color: "#94a3b8" }}>开始日期</label>
                <input type="date" value={qbStart} onChange={(e) => setQbStart(e.target.value)} />
              </div>
              <div>
                <label style={{ fontSize: "0.875rem", color: "#94a3b8" }}>结束日期</label>
                <input type="date" value={qbEnd} onChange={(e) => setQbEnd(e.target.value)} />
              </div>
              <div>
                <label style={{ fontSize: "0.875rem", color: "#94a3b8" }}>初始资金</label>
                <input type="number" value={qbCash} onChange={(e) => setQbCash(Number(e.target.value))} />
              </div>
            </div>

            <div>
              <button onClick={submitQuickBacktest} disabled={qbSubmitting}>
                {qbSubmitting ? "提交中..." : "运行回测"}
              </button>
            </div>
          </div>
        )}
      </div>

      <div style={{ marginTop: "2rem" }}>
        <h2>最近回测</h2>
        <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "0.75rem" }}>
          {jobs.slice(0, 5).map((job) => (
            <Link key={job.id} to={`/backtests/${job.id}`} style={{ textDecoration: "none" }}>
              <div className="card" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <div>
                  <span style={{ fontWeight: 600 }}>{job.symbols?.join(", ") || "--"}</span>
                  <span style={{ color: "#94a3b8", marginLeft: "1rem", fontSize: "0.875rem" }}>
                    {job.start_date} ~ {job.end_date}
                  </span>
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
            <div className="card" style={{ textAlign: "center", color: "#94a3b8" }}>
              暂无回测记录，点击上方「快速回测」或前往<Link to="/strategy-backtest">策略回测</Link>开始。
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
