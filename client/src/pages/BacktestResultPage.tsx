import { useEffect, useState, useRef, useCallback } from "react";
import { useParams } from "react-router-dom";
import { backtestAPI, marketAPI, getApiBase } from "../services/api";
import type { BacktestJob, BacktestResult, KLineData, Trade, Signal, SuitableStock, PaginatedTrades, MonthlyReturn } from "../services/api";
import KLineChart from "../components/KLineChart";
import EquityCurveChart from "../components/EquityCurveChart";

export default function BacktestResultPage() {
  const { id } = useParams();
  const [job, setJob] = useState<BacktestJob | null>(null);
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [klineData, setKlineData] = useState<KLineData[]>([]);
  const [activeSymbol, setActiveSymbol] = useState<string>("");
  const [period, setPeriod] = useState("1d");
  const [exchange, setExchange] = useState<string>("cn");

  // WebSocket ref
  const wsRef = useRef<WebSocket | null>(null);
  const fallbackPollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Paging for trades
  const [tradesPage, setTradesPage] = useState(1);
  const [tradesData, setTradesData] = useState<PaginatedTrades | null>(null);
  const [tradesLoading, setTradesLoading] = useState(false);
  const [klineError, setKlineError] = useState<string | null>(null);

  // Load initial job + fallback poll
  useEffect(() => {
    if (!id) return;

    const loadJob = async () => {
      try {
        const res = await backtestAPI.get(id);
        setJob(res.data);
        if (res.data.status === "success" && !result) {
          const r = await backtestAPI.result(id);
          setResult(r.data);
        }
      } catch (e) {
        console.error("Load job failed:", e);
      }
    };

    loadJob();

    // Fallback polling if WebSocket not available
    fallbackPollRef.current = setInterval(() => {
      backtestAPI.get(id).then((res) => {
        setJob(res.data);
        if (res.data.status === "success") {
          if (!result) {
            backtestAPI.result(id).then((r) => setResult(r.data));
          }
          if (fallbackPollRef.current) {
            clearInterval(fallbackPollRef.current);
            fallbackPollRef.current = null;
          }
        }
        if (res.data.status === "failed" && fallbackPollRef.current) {
          clearInterval(fallbackPollRef.current);
          fallbackPollRef.current = null;
        }
      }).catch((e) => {
        console.error("[Polling] fallback poll error:", e);
      });
    }, 3000);

    return () => {
      if (fallbackPollRef.current) clearInterval(fallbackPollRef.current);
    };
  }, [id]);

  // WebSocket connection with limited retries
  useEffect(() => {
    if (!id) return;
    if (wsRef.current) return;

    const apiBase = getApiBase();
    const wsUrl = `${apiBase.replace(/^http/, "ws")}/ws/backtest/${id}`;
    let retryCount = 0;
    const maxRetries = 3;

    const connect = () => {
      try {
        const ws = new WebSocket(wsUrl);
        wsRef.current = ws;

        ws.onopen = () => {
          console.log("[WS] Connected", wsUrl);
          retryCount = 0;
          if (fallbackPollRef.current) {
            clearInterval(fallbackPollRef.current);
            fallbackPollRef.current = null;
          }
        };

        ws.onmessage = (event) => {
          try {
            const msg = JSON.parse(event.data);
            if (msg.status) {
              setJob((prev: BacktestJob | null) => (prev ? { ...prev, status: msg.status } : prev));
              if (msg.status === "success" && !result) {
                backtestAPI.result(id).then((r) => setResult(r.data));
              }
            }
          } catch (e) {
            console.error("[WS] Message parse error:", e);
          }
        };

        ws.onerror = () => {
          console.warn("[WS] Error");
        };

        ws.onclose = () => {
          wsRef.current = null;
          if (retryCount < maxRetries) {
            retryCount++;
            console.log(`[WS] Reconnecting (${retryCount}/${maxRetries})...`);
            setTimeout(connect, 2000 * retryCount);
          } else {
            console.warn("[WS] Max retries reached, falling back to polling");
          }
        };
      } catch (e) {
        console.warn("[WS] Connection failed:", e);
        if (retryCount < maxRetries) {
          retryCount++;
          setTimeout(connect, 2000 * retryCount);
        }
      }
    };

    connect();

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [id]);

  // Load K-line after success
  useEffect(() => {
    if (!job || job.status !== "success" || !result) return;

    const symbols = job.symbols || [];
    if (symbols.length === 0) return;

    const start = job.start_date?.slice(0, 10);
    const end = job.end_date?.slice(0, 10);
    if (!start || !end) return;

    const target = activeSymbol || symbols[0];
    marketAPI.history(target, start, end, exchange, period).then((res) => {
      setKlineData(res.data?.data || []);
      if (!res.data?.data?.length) {
        console.warn(`[K-line] No data returned for ${target}`);
      }
    }).catch((e) => {
      console.error("K-line load failed:", e);
      setKlineData([]);
      setKlineError(`K线加载失败: ${e?.message || "未知错误"}`);
    });
  }, [job, result, activeSymbol, exchange, period]);

  // Load trades with pagination
  const loadTrades = useCallback(async (page: number) => {
    if (!id || !job || job.status !== "success") return;
    setTradesLoading(true);
    try {
      const res = await backtestAPI.trades(id, page, 50);
      setTradesData(res.data);
      setTradesPage(page);
    } catch (e) {
      console.error("Load trades failed:", e);
    } finally {
      setTradesLoading(false);
    }
  }, [id, job]);

  useEffect(() => {
    if (job?.status === "success") {
      loadTrades(1);
    }
  }, [job, loadTrades]);

  const escapeHtml = (str: string): string =>
    str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

  if (!job) return <div style={{textAlign:"center",padding:"2rem",color:"var(--muted)"}}>加载中...</div>;

  const summary = job.result_summary || result?.summary || {};
  const isScan = job.scope === "scan" || !!result?.signals;
  const symbols = job.symbols || [];
  const monthlyReturns = summary.monthly_returns || [];
  const signals = result?.signals || [];
  const suitableStocks = result?.suitable_stocks || [];

  // Trade markers for K-line
  const allTrades = result?.trades || [];
  const tradeMarkers = allTrades
    .filter((t: Trade) => t.symbol === (activeSymbol || symbols[0]))
    .map((t: Trade) => ({
      time: t.timestamp,
      price: t.price,
      type: (t.type === "buy" ? "BUY" : "SELL") as "BUY" | "SELL",
      pnl: t.pnl,
    }));

  const exportCSV = () => {
    if (!allTrades.length && !signals.length) return;

    const escapeCSV = (val: string | number | undefined | null): string => {
      const str = String(val ?? "");
      if (str.includes(",") || str.includes('"') || str.includes("\n")) {
        return `"${str.replace(/"/g, '""')}"`;
      }
      return str;
    };

    const rows = isScan
      ? [
          ["标的", "方向", "时间", "价格", "评分"],
          ...signals.map((s: Signal) => [s.symbol, s.direction, s.timestamp, s.price, s.score].map(escapeCSV)),
        ]
      : [
          ["时间", "标的", "类型", "数量", "价格", "盈亏"],
          ...allTrades.map((t: Trade) => [t.timestamp, t.symbol, t.type, t.amount, t.price, t.pnl].map(escapeCSV)),
        ];
    const csv = rows.map((r) => r.join(",")).join("\n");
    const blob = new Blob(["\uFEFF" + csv], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `backtest_${id}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div>
      <h1>回测详情</h1>
      <div className="card" style={{ marginTop: "1rem" }}>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: "1rem" }}>
          <div><strong>状态:</strong> {job.status}</div>
          <div><strong>标的:</strong> {symbols.slice(0, 5).join(", ") || "--"}{symbols.length > 5 ? ` +${symbols.length - 5}` : ""}</div>
          <div><strong>时间范围:</strong> {job.start_date?.slice(0, 10)} ~ {job.end_date?.slice(0, 10)}</div>
          <div><strong>初始资金:</strong> {job.initial_cash?.toLocaleString()}</div>
          <div><strong>交易次数:</strong> {summary.total_trades ?? "--"}</div>
          <div><strong>最终市值:</strong> {summary.final_value?.toLocaleString() ?? "--"}</div>
        </div>
        {job.error_message && <p style={{ color: "#ef4444", marginTop: "0.5rem" }}>错误: {job.error_message}</p>}
      </div>

      {isScan && summary.avg_return !== undefined && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>扫描整体表现</h3>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: "1rem", marginTop: "0.75rem" }}>
            <Metric label="平均收益" value={`${((summary.avg_return ?? 0) * 100).toFixed(2)}%`} color={(summary.avg_return ?? 0) >= 0 ? "#22c55e" : "#ef4444"} />
            <Metric label="中位数收益" value={`${((summary.median_return ?? 0) * 100).toFixed(2)}%`} />
            <Metric label="平均夏普" value={summary.avg_sharpe?.toFixed(2)} />
            <Metric label="平均回撤" value={`${((summary.avg_drawdown ?? 0) * 100).toFixed(2)}%`} color="#ef4444" />
            <Metric label="信号总数" value={summary.total_signals} />
            <Metric label="扫描标的数" value={summary.scanned_count} />
            <Metric label="胜率" value={`${((summary.win_rate ?? 0) * 100).toFixed(1)}%`} />
            <Metric label="适合策略数" value={summary.suitable_count ?? suitableStocks.length} />
          </div>
        </div>
      )}

      {isScan && suitableStocks.length > 0 && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>适合策略的股票 (评分≥60)</h3>
          <div style={{ maxHeight: 300, overflow: "auto", marginTop: "0.5rem" }}>
            <table style={{ width: "100%", fontSize: "0.875rem", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid var(--border)", textAlign: "left" }}>
                  <th style={{ padding: "0.25rem" }}>标的</th>
                  <th style={{ padding: "0.25rem" }}>评分</th>
                  <th style={{ padding: "0.25rem" }}>总收益</th>
                  <th style={{ padding: "0.25rem" }}>最大回撤</th>
                  <th style={{ padding: "0.25rem" }}>夏普</th>
                  <th style={{ padding: "0.25rem" }}>交易次数</th>
                </tr>
              </thead>
              <tbody>
                {suitableStocks.map((s: SuitableStock, i: number) => (
                  <tr key={i} style={{ borderBottom: "1px solid var(--border)" }}>
                    <td style={{ padding: "0.25rem" }}>{s.symbol}</td>
                    <td style={{ padding: "0.25rem", color: s.score >= 80 ? "#22c55e" : s.score >= 60 ? "#f59e0b" : "var(--muted)" }}>{s.score}</td>
                    <td style={{ padding: "0.25rem" }}>{(s.total_return * 100).toFixed(2)}%</td>
                    <td style={{ padding: "0.25rem" }}>{(s.max_drawdown * 100).toFixed(2)}%</td>
                    <td style={{ padding: "0.25rem" }}>{s.sharpe_ratio?.toFixed(2)}</td>
                    <td style={{ padding: "0.25rem" }}>{s.total_trades}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {isScan && signals.length > 0 && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>最新信号 ({signals.length}笔)</h3>
          <div style={{ maxHeight: 300, overflow: "auto", marginTop: "0.5rem" }}>
            <table style={{ width: "100%", fontSize: "0.875rem", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid var(--border)", textAlign: "left" }}>
                  <th style={{ padding: "0.25rem" }}>标的</th>
                  <th style={{ padding: "0.25rem" }}>方向</th>
                  <th style={{ padding: "0.25rem" }}>时间</th>
                  <th style={{ padding: "0.25rem" }}>价格</th>
                  <th style={{ padding: "0.25rem" }}>评分</th>
                </tr>
              </thead>
              <tbody>
                {signals.map((s: Signal, i: number) => (
                  <tr key={i} style={{ borderBottom: "1px solid var(--border)" }}>
                    <td style={{ padding: "0.25rem" }}>{s.symbol}</td>
                    <td style={{ padding: "0.25rem", color: s.direction === "buy" ? "#22c55e" : "#ef4444" }}>{s.direction === "buy" ? "买入" : "卖出"}</td>
                    <td style={{ padding: "0.25rem" }}>{s.timestamp?.slice(0, 10)}</td>
                    <td style={{ padding: "0.25rem" }}>{s.price}</td>
                    <td style={{ padding: "0.25rem" }}>{s.score}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {!isScan && summary.total_return !== undefined && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>绩效摘要</h3>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: "1rem", marginTop: "0.75rem" }}>
            <Metric label="总收益" value={`${((summary.total_return ?? 0) * 100).toFixed(2)}%`} color={(summary.total_return ?? 0) >= 0 ? "#22c55e" : "#ef4444"} />
            <Metric label="年化收益" value={`${((summary.annual_return ?? 0) * 100).toFixed(2)}%`} />
            <Metric label="最大回撤" value={`${((summary.max_drawdown ?? 0) * 100).toFixed(2)}%`} color="#ef4444" />
            <Metric label="夏普比率" value={summary.sharpe_ratio?.toFixed(2)} />
            <Metric label="索提诺比率" value={summary.sortino_ratio?.toFixed(2)} />
            <Metric label="Calmar比率" value={summary.calmar_ratio?.toFixed(2)} />
            <Metric label="胜率" value={`${((summary.win_rate ?? 0) * 100).toFixed(1)}%`} />
            <Metric label="总佣金" value={summary.total_commission?.toFixed(2)} />
          </div>
        </div>
      )}

      {/* 净值曲线（降采样） */}
      {!isScan && job.status === "success" && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>净值曲线</h3>
          <EquityCurveChart jobId={id!} initialCash={job.initial_cash} />
        </div>
      )}

      {!isScan && monthlyReturns.length > 0 && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <h3>月度收益分布</h3>
          <MonthlyHeatmap data={monthlyReturns} />
        </div>
      )}

      {!isScan && symbols.length > 0 && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "0.5rem" }}>
            <h3 style={{ margin: 0 }}>K 线与交易标记 ({tradeMarkers.length}笔)</h3>
            <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
              {symbols.length > 1 && (
                <>
                  <span style={{ fontSize: "0.875rem", color: "var(--muted)" }}>标的:</span>
                  <select
                    value={activeSymbol || symbols[0]}
                    onChange={(e) => setActiveSymbol(e.target.value)}
                    style={{ width: "auto", minWidth: 120 }}
                  >
                    {symbols.map((s: string) => (
                      <option key={s} value={s}>{s}</option>
                    ))}
                  </select>
                </>
              )}
              <span style={{ fontSize: "0.875rem", color: "var(--muted)" }}>市场:</span>
              <select value={exchange} onChange={(e) => setExchange(e.target.value)} style={{ width: "auto", minWidth: 80 }}>
                <option value="cn">A股</option>
                <option value="hk">港股</option>
                <option value="us">美股</option>
              </select>
            </div>
          </div>
          <KLineChart data={klineData} trades={tradeMarkers} period={period} onPeriodChange={setPeriod} />
          {klineError && <p style={{ color: "#ef4444", fontSize: "0.875rem", marginTop: "0.5rem" }} dangerouslySetInnerHTML={{ __html: escapeHtml(klineError) }} />}
        </div>
      )}

      {/* 交易记录分页 */}
      {!isScan && tradesData && tradesData.total > 0 && (
        <div className="card" style={{ marginTop: "1rem" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <h3 style={{ margin: 0 }}>交易记录 ({tradesData.total}笔)</h3>
            <button className="secondary" onClick={exportCSV} style={{ fontSize: "0.75rem", padding: "0.25rem 0.5rem" }}>
              导出 CSV
            </button>
          </div>
          <div style={{ maxHeight: 300, overflow: "auto", marginTop: "0.5rem" }}>
            <table style={{ width: "100%", fontSize: "0.875rem", borderCollapse: "collapse" }}>
              <thead>
                <tr style={{ borderBottom: "1px solid var(--border)", textAlign: "left" }}>
                  <th style={{ padding: "0.25rem" }}>时间</th>
                  <th style={{ padding: "0.25rem" }}>标的</th>
                  <th style={{ padding: "0.25rem" }}>类型</th>
                  <th style={{ padding: "0.25rem" }}>数量</th>
                  <th style={{ padding: "0.25rem" }}>价格</th>
                  <th style={{ padding: "0.25rem" }}>盈亏</th>
                </tr>
              </thead>
              <tbody>
                {tradesData.items.map((t: Trade, i: number) => (
                  <tr key={i} style={{ borderBottom: "1px solid var(--border)" }}>
                    <td style={{ padding: "0.25rem" }}>{t.timestamp?.slice(0, 10)}</td>
                    <td style={{ padding: "0.25rem" }}>{t.symbol}</td>
                    <td style={{ padding: "0.25rem", color: t.type === "buy" ? "#22c55e" : "#ef4444" }}>{t.type === "buy" ? "买入" : "卖出"}</td>
                    <td style={{ padding: "0.25rem" }}>{t.amount}</td>
                    <td style={{ padding: "0.25rem" }}>{t.price}</td>
                    <td style={{ padding: "0.25rem", color: (t.pnl ?? 0) >= 0 ? "#ef4444" : "#22c55e" }}>{(t.pnl ?? 0).toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {/* Pagination */}
          {tradesData.total > tradesData.page_size && (
            <div style={{ display: "flex", justifyContent: "center", gap: "0.5rem", marginTop: "0.75rem" }}>
              <button
                className="secondary"
                disabled={tradesPage <= 1 || tradesLoading}
                onClick={() => loadTrades(tradesPage - 1)}
                style={{ fontSize: "0.75rem", padding: "0.25rem 0.5rem" }}
              >
                上一页
              </button>
              <span style={{ fontSize: "0.875rem", color: "var(--muted)", alignSelf: "center" }}>
                {tradesPage} / {Math.ceil(tradesData.total / tradesData.page_size)}
              </span>
              <button
                className="secondary"
                disabled={tradesPage >= Math.ceil(tradesData.total / tradesData.page_size) || tradesLoading}
                onClick={() => loadTrades(tradesPage + 1)}
                style={{ fontSize: "0.75rem", padding: "0.25rem 0.5rem" }}
              >
                下一页
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function Metric({ label, value, color }: { label: string; value: string | number | undefined; color?: string }) {
  return (
    <div>
      <div style={{ fontSize: "0.75rem", color: "var(--muted)" }}>{label}</div>
      <div style={{ fontSize: "1.125rem", fontWeight: 600, color: color || "inherit" }}>{value ?? "--"}</div>
    </div>
  );
}

function MonthlyHeatmap({ data }: { data: MonthlyReturn[] }) {
  const byYear: Record<string, any[]> = {};
  data.forEach((d) => {
    const year = d.month.slice(0, 4);
    if (!byYear[year]) byYear[year] = [];
    byYear[year].push(d);
  });

  const years = Object.keys(byYear).sort();
  const months = ["01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12"];

  const getColor = (ret: number) => {
    if (ret > 0.05) return "#991b1b";
    if (ret > 0.02) return "#ef4444";
    if (ret > 0) return "#fca5a5";
    if (ret > -0.02) return "#86efac";
    if (ret > -0.05) return "#22c55e";
    return "#166534";
  };

  return (
    <div style={{ overflowX: "auto" }}>
      {years.map((year) => (
        <div key={year} style={{ marginBottom: "0.5rem" }}>
          <div style={{ fontSize: "0.75rem", color: "var(--muted)", marginBottom: "0.25rem" }}>{year}</div>
          <div style={{ display: "flex", gap: "0.25rem" }}>
            {months.map((m) => {
              const item = byYear[year].find((d) => d.month.endsWith(`-${m}`));
              const ret = item ? item.return : 0;
              return (
                <div
                  key={m}
                  style={{
                    width: "2rem",
                    height: "2rem",
                    background: item ? getColor(ret) : "var(--surface)",
                    borderRadius: "0.25rem",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: "0.625rem",
                    color: Math.abs(ret) > 0.03 ? "#fff" : "#000",
                  }}
                  title={item ? `${item.month}: ${(ret * 100).toFixed(2)}%` : `${year}-${m}: 无数据`}
                >
                  {m}
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}


