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
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <label style={{ fontSize: "0.875rem", color: "var(--muted)" }}>
          {fullMarketScan
            ? "全市场扫描（按市值Top500）"
            : `选择标的（已选 ${selectedStocks.length} 只）`}
        </label>
        <label
          style={{
            fontSize: "0.875rem",
            color: "var(--muted)",
            display: "flex",
            alignItems: "center",
            gap: "0.25rem",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={fullMarketScan}
            onChange={(e) => onFullMarketScanChange(e.target.checked)}
          />
          全市场扫描
        </label>
      </div>
      <div style={{ display: "flex", gap: "0.5rem", marginTop: "0.5rem" }}>
        <select
          value={exchange}
          onChange={(e) => onExchangeChange?.(e.target.value)}
          style={{ width: "auto", minWidth: 100 }}
        >
          {EXCHANGES.map((ex) => (
            <option key={ex.value} value={ex.value}>{ex.label}</option>
          ))}
        </select>
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