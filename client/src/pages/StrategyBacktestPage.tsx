import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { backtestAPI } from "../services/api";
import { useStrategyStore } from "../stores/strategyStore";
import StockSelector from "../components/StockSelector";
import { useToast } from "../components/Toast";

export default function StrategyBacktestPage() {
  const navigate = useNavigate();
  const { strategies, loaded, fetchStrategies, getStrategy } = useStrategyStore();

  const [selectedStrategy, setSelectedStrategy] = useState("");
  const { toast } = useToast();
  const [startDate, setStartDate] = useState("2024-01-01");
  const [endDate, setEndDate] = useState("2024-06-01");
  const [initialCash, setInitialCash] = useState(1_000_000);
  const [fullMarketScan, setFullMarketScan] = useState(false);

  const [selectedStocks, setSelectedStocks] = useState<string[]>([]);
  const [exchange, setExchange] = useState("cn");
  const [excludeSt, setExcludeSt] = useState(false);

  const [period, setPeriod] = useState("1d");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!loaded) {
      fetchStrategies();
    }
  }, [loaded, fetchStrategies]);

  const toggleStock = (symbol: string) => {
    setSelectedStocks((prev) =>
      prev.includes(symbol) ? prev.filter((s) => s !== symbol) : [...prev, symbol]
    );
  };

  const submitBacktest = async () => {
    if (!selectedStrategy) {
      toast("请选择策略", "error");
      return;
    }
    if (!fullMarketScan && selectedStocks.length === 0) {
      toast("请至少选择一只股票，或开启全市场扫描", "error");
      return;
    }
    const strategy = getStrategy(selectedStrategy);
    if (!strategy || !strategy.code) {
      toast("策略代码为空，无法回测", "error");
      return;
    }
    setSubmitting(true);
    try {
      const submitParams: any = {
        strategy_id: selectedStrategy,
        strategy_code: strategy.code,
        symbols: fullMarketScan ? [] : selectedStocks,
        start_date: startDate,
        end_date: endDate,
        initial_cash: initialCash,
        period,
      };
      if (fullMarketScan) {
        submitParams.scope = "scan";
        submitParams.params = { top_n: 50, score_threshold: 0, exchange, exclude_st: excludeSt };
      }
      const { data } = await backtestAPI.submit(submitParams);
      navigate(`/backtests/${data.id}`);
    } catch (e: any) {
      toast(e.response?.data?.error || e.response?.data?.detail || "回测提交失败", "error");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div>
      <h1>策略回测</h1>
      <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "1rem" }}>

        <div className="card">
          <h3>选择策略</h3>
          <div style={{ marginTop: "0.5rem" }}>
            <select value={selectedStrategy} onChange={(e) => setSelectedStrategy(e.target.value)}>
              <option value="">-- 请选择策略 --</option>
              {strategies.map((s) => (
                <option key={s.id} value={s.id}>{s.name}</option>
              ))}
            </select>
          </div>
        </div>

        <div className="card">
          <h3>回测参数</h3>
          <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem", marginTop: "0.75rem" }}>

            <StockSelector
              selectedStocks={selectedStocks}
              onToggleStock={toggleStock}
              fullMarketScan={fullMarketScan}
              onFullMarketScanChange={setFullMarketScan}
              exchange={exchange}
              onExchangeChange={setExchange}
              excludeSt={excludeSt}
              onExcludeStChange={setExcludeSt}
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
              <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>K线周期</label>
              <div style={{ display: "flex", gap: "0.25rem", marginTop: "0.25rem" }}>
                {["1d", "1min", "5min", "15min", "30min", "60min"].map((p) => (
                  <button
                    key={p}
                    type="button"
                    onClick={() => setPeriod(p)}
                    style={{
                      padding: "0.25rem 0.5rem",
                      fontSize: "0.75rem",
                      background: period === p ? "var(--primary)" : "var(--bg)",
                      color: period === p ? "#fff" : "var(--muted)",
                      border: "1px solid var(--border)",
                      borderRadius: "0.25rem",
                      cursor: "pointer",
                    }}
                  >
                    {p === "1d" ? "日线" : p === "1min" ? "1分钟" : p === "5min" ? "5分钟" : p === "15min" ? "15分钟" : p === "30min" ? "30分钟" : "60分钟"}
                  </button>
                ))}
              </div>
            </div>
            <div>
              <button onClick={submitBacktest} disabled={submitting}>
                {submitting ? "提交中..." : "运行回测"}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
