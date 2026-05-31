import { useEffect, useRef, useState } from "react";
import { createChart, IChartApi, LineSeries, LineStyle } from "lightweight-charts";

interface EquityPoint {
  datetime: string;
  total_value: number | string;
}

interface EquityCurveChartProps {
  data?: EquityPoint[];
  initialCash?: number;
  height?: number;
}

function toTs(dt: string): number {
  const m = (dt || "").match(/(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2}))?/);
  if (m) return Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +(m[6] || 0)) / 1000;
  const d = (dt || "").match(/(\d{4})-(\d{2})-(\d{2})/);
  if (d) return Date.UTC(+d[1], +d[2] - 1, +d[3]) / 1000;
  return 0;
}

/** 去重 + 排序 + 过滤无效时间戳 */
function sanitize(data: EquityPoint[]): EquityPoint[] {
  const seen = new Map<number, EquityPoint>();
  for (const d of data) {
    const ts = toTs(d.datetime);
    if (ts <= 0) continue;
    // 同一天保留最后一条
    seen.set(ts, d);
  }
  return [...seen.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([, v]) => v);
}



interface ChartThemeColors {
  bg: string;
  surface: string;
  text: string;
  muted: string;
  border: string;
  crosshair: string;
}

function getChartThemeColors(): ChartThemeColors {
  const isLight = document.documentElement.getAttribute("data-theme") === "light";
  if (isLight) {
    return { bg: "#f8fafc", surface: "#ffffff", text: "#0f172a", muted: "#64748b", border: "#e2e8f0", crosshair: "#6366f1" };
  }
  return { bg: "#0f172a", surface: "#1e293b", text: "#94a3b8", muted: "#94a3b8", border: "#334155", crosshair: "#38bdf8" };
}
export default function EquityCurveChart({ data: propData, initialCash, height = 300 }: EquityCurveChartProps) {
  const [theme, setTheme] = useState<"dark" | "light">(() => (document.documentElement.getAttribute("data-theme") as any) || "dark");
  useEffect(() => {
    const obs = new MutationObserver(() => {
      setTheme((document.documentElement.getAttribute("data-theme") as any) || "dark");
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => obs.disconnect();
  }, []);
  const chartContainerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [chartData, setChartData] = useState<EquityPoint[]>(propData || []);

  useEffect(() => {
    if (propData && propData.length > 0) {
      setChartData(sanitize(propData));
    }
  }, [propData]);

  useEffect(() => {
    if (!chartContainerRef.current || chartData.length === 0) return;

    const tc = getChartThemeColors();
    const chart = createChart(chartContainerRef.current, {
      height,
      layout: {
        background: { color: tc.bg },
        fontFamily: "-apple-system, BlinkMacSystemFont, PingFang SC, Microsoft YaHei, sans-serif",
        textColor: tc.muted,
      },
      grid: {
        vertLines: { color: tc.surface },
        horzLines: { color: tc.surface },
      },
      crosshair: {
        mode: 1,
        vertLine: { color: tc.crosshair, labelBackgroundColor: tc.crosshair },
        horzLine: { color: tc.crosshair, labelBackgroundColor: tc.crosshair },
      },
      rightPriceScale: { borderColor: tc.border },
      timeScale: {
        borderColor: tc.border,
        timeVisible: true,
        secondsVisible: false,
        fixLeftEdge: true,
        fixRightEdge: true,
      },
    });
    chartRef.current = chart;

    const baselineValue = initialCash !== undefined ? Number(initialCash) : (Number(chartData[0]?.total_value) ?? 0);

    // 基准线（初始资金）
    if (baselineValue > 0) {
      const baseSeries = chart.addSeries(LineSeries, {
        color: theme === "light" ? "#94a3b8" : "#64748b",
        lineWidth: 1,
        lineStyle: LineStyle.Dashed,
        lastValueVisible: false,
        priceLineVisible: false,
      });
      baseSeries.setData(chartData.map((d) => ({ time: toTs(d.datetime) as any, value: baselineValue })));
    }

    const lineSeries = chart.addSeries(LineSeries, {
      color: "#38bdf8",
      lineWidth: 2,
      crosshairMarkerVisible: true,
      crosshairMarkerRadius: 4,
    });

    const mapped = chartData.map((d) => ({
      time: toTs(d.datetime) as any,
      value: Number(d.total_value),
    }));
    lineSeries.setData(mapped);

    chart.timeScale().fitContent();

    // 自定义 tooltip
    const tooltipEl = tooltipRef.current;
    if (tooltipEl) {
      chart.subscribeCrosshairMove((param: any) => {
        if (!param.time || param.point === undefined) {
          tooltipEl.style.display = "none";
          return;
        }
        const seriesData = param.seriesData?.get(lineSeries);
        if (!seriesData || typeof seriesData !== "object" || !("value" in seriesData)) {
          tooltipEl.style.display = "none";
          return;
        }
        const value = (seriesData as any).value as number;
        const ret = baselineValue > 0 ? ((value - baselineValue) / baselineValue * 100).toFixed(2) : "0.00";
        const dateStr = new Date((param.time as number) * 1000).toISOString().slice(0, 10);
        tooltipEl.style.display = "block";
        tooltipEl.textContent = "";
        const d1 = document.createElement("div");
        d1.style.cssText = `font-size:0.75rem;color:${tc.muted}`;
        d1.textContent = dateStr;
        const d2 = document.createElement("div");
        d2.style.cssText = `font-size:0.875rem;font-weight:600;color:${tc.text}`;
        d2.textContent = `净值: ${value.toLocaleString()}`;
        const d3 = document.createElement("div");
        d3.style.cssText = `font-size:0.875rem;color:${Number(ret) >= 0 ? (theme === "light" ? "#dc2626" : "#ef4444") : (theme === "light" ? "#16a34a" : "#22c55e")}`;
        d3.textContent = `收益率: ${ret}%`;
        tooltipEl.appendChild(d1);
        tooltipEl.appendChild(d2);
        tooltipEl.appendChild(d3);
        const rect = chartContainerRef.current!.getBoundingClientRect();
        let left = param.point.x + 12;
        let top = param.point.y + 12;
        if (left + 140 > rect.width) left = param.point.x - 150;
        if (top + 60 > rect.height) top = param.point.y - 70;
        tooltipEl.style.left = `${left}px`;
        tooltipEl.style.top = `${top}px`;
      });
    }

    const handleResize = () => {
      if (chartContainerRef.current) {
        chart.applyOptions({ width: chartContainerRef.current.clientWidth });
      }
    };
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      chart.remove();
      chartRef.current = null;
    };
  }, [chartData, height, initialCash, theme]);

  if (chartData.length === 0) {
    return <div style={{ color: "var(--muted)", padding: "2rem", textAlign: "center" }}>暂无净值数据</div>;
  }

  return (
    <div ref={chartContainerRef} style={{ width: "100%", position: "relative" }}>
      <div
        ref={tooltipRef}
        style={{
          position: "absolute",
          display: "none",
          pointerEvents: "none",
          background: "var(--surface)",
          border: "1px solid var(--border)",
          borderRadius: "0.375rem",
          padding: "0.5rem",
          zIndex: 10,
          whiteSpace: "nowrap",
        }}
      />
    </div>
  );
}
