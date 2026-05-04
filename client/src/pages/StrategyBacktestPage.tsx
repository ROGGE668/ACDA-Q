import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { strategyAPI, backtestAPI } from "../services/api";
import StockSelector from "../components/StockSelector";

export default function StrategyBacktestPage() {
  const navigate = useNavigate();

  const [strategies, setStrategies] = useState<any[]>([]);
  const [selectedStrategy, setSelectedStrategy] = useState("");
  const [startDate, setStartDate] = useState("2024-01-01");
  const [endDate, setEndDate] = useState("2024-06-01");
  const [initialCash, setInitialCash] = useState(1_000_000);
  const [fullMarketScan, setFullMarketScan] = useState(false);

  const [selectedStocks, setSelectedStocks] = useState<string[]>([]);

  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    strategyAPI.list().then((res) => setStrategies(res.data));
  }, []);

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
      navigate(`/backtests/${data.id}`);
    } catch (e: any) {
      alert(e.response?.data?.detail || "回测提交失败");
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
      </div>
    </div>
  );
}
