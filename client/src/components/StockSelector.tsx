import { useState } from "react";
import { marketAPI } from "../services/api";

interface StockItem {
  symbol: string;
  name: string;
  industry?: string;
}

interface StockSelectorProps {
  selectedStocks: string[];
  onToggleStock: (symbol: string) => void;
  fullMarketScan: boolean;
  onFullMarketScanChange: (value: boolean) => void;
  exchange?: string;
  onExchangeChange?: (exchange: string) => void;
  excludeSt?: boolean;
  onExcludeStChange?: (v: boolean) => void;
}

const EXCHANGES = [
  { value: "cn", label: "A股" },
  { value: "hk", label: "港股" },
  { value: "us", label: "美股" },
];

export default function StockSelector({
  selectedStocks,
  onToggleStock,
  fullMarketScan,
  onFullMarketScanChange,
  exchange = "cn",
  excludeSt = false,
  onExcludeStChange,
  onExchangeChange,
}: StockSelectorProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<StockItem[]>([]);

  const search = async () => {
    if (!query.trim()) return;
    try {
      const { data } = await marketAPI.search(query, exchange);
      setResults(data.items || []);
    } catch {
      setResults([]);
    }
  };

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: "0.75rem", marginBottom: "0.5rem" }}>
        <label
          onClick={() => onFullMarketScanChange(!fullMarketScan)}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: "0.5rem",
            padding: "0.375rem 0.75rem",
            borderRadius: "0.375rem",
            border: fullMarketScan ? "1px solid var(--primary, #6366f1)" : "1px solid var(--border)",
            background: fullMarketScan ? "var(--primary, #6366f1)" : "transparent",
            color: fullMarketScan ? "#fff" : "var(--muted)",
            fontSize: "0.8125rem",
            cursor: "pointer",
            transition: "all 0.15s",
            whiteSpace: "nowrap" as const,
          }}
        >
          <span style={{ width: 14, height: 14, border: fullMarketScan ? "none" : "1.5px solid var(--muted)", borderRadius: 3, display: "flex", alignItems: "center", justifyContent: "center", fontSize: "0.7rem", background: fullMarketScan ? "rgba(255,255,255,0.25)" : "transparent" }}>
            {fullMarketScan ? "✓" : ""}
          </span>
          全市场扫描
        </label>
        <span style={{ fontSize: "0.8125rem", color: "var(--muted)" }}>
          {fullMarketScan ? "按市值Top500扫描" : `已选 ${selectedStocks.length} 只标的`}
        </span>
      </div>
      <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.5rem", alignItems: "center" }}>
        <select
          value={exchange}
          onChange={(e) => onExchangeChange?.(e.target.value)}
          style={{ width: "auto", minWidth: 100 }}
        >
          {EXCHANGES.map((ex) => (
            <option key={ex.value} value={ex.value}>{ex.label}</option>
          ))}
        </select>
        {fullMarketScan && (
          <label style={{ display: "flex", alignItems: "center", gap: 4, fontSize: "0.8125rem", cursor: "pointer", color: "var(--muted)", userSelect: "none" }}>
            <input type="checkbox" checked={excludeSt} onChange={(e) => onExcludeStChange?.(e.target.checked)} style={{ accentColor: "#3b82f6" }} />
            排除ST
          </label>
        )}
      </div>
      {!fullMarketScan && (
        <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.25rem" }}>
          <input
            placeholder="搜索股票代码或名称..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && search()}
            style={{ flex: 1 }}
          />
          <button className="secondary" onClick={search}>
            搜索
          </button>
        </div>
      )}
      {!fullMarketScan && results.length > 0 && (
        <div
          className="card"
          style={{ marginTop: "0.5rem", maxHeight: 200, overflow: "auto" }}
        >
          {results.map((s) => (
            <div
              key={s.symbol}
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                padding: "0.25rem 0",
                cursor: "pointer",
                color: selectedStocks.includes(s.symbol) ? "var(--success)" : "inherit",
              }}
              onClick={() => onToggleStock(s.symbol)}
            >
              <span>
                {s.symbol} - {s.name}
                {s.industry ? ` (${s.industry})` : ""}
              </span>
              <span>{selectedStocks.includes(s.symbol) ? "已选" : "+"}</span>
            </div>
          ))}
        </div>
      )}
      {!fullMarketScan && selectedStocks.length > 0 && (
        <div
          style={{
            marginTop: "0.5rem",
            fontSize: "0.875rem",
            display: "flex",
            flexWrap: "wrap",
            gap: "0.25rem",
            alignItems: "center",
          }}
        >
          <span style={{ color: "var(--muted)" }}>已选:</span>
          {selectedStocks.map((s) => (
            <span
              key={s}
              style={{
                background: "var(--bg)",
                border: "1px solid var(--border)",
                borderRadius: "0.25rem",
                padding: "0.125rem 0.375rem",
                display: "inline-flex",
                alignItems: "center",
                gap: "0.25rem",
                cursor: "pointer",
              }}
              onClick={() => onToggleStock(s)}
              title="点击移除"
            >
              {s}
              <span style={{ color: "#ef4444", fontWeight: 700 }}>×</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}