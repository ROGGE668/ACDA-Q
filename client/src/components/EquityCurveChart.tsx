import { useEffect, useRef, useState } from "react";
import { createChart, IChartApi, LineSeries, LineStyle } from "lightweight-charts";

interface EquityPoint {
  datetime: string;
  total_value: number;
}

interface EquityCurveChartProps {
  data?: EquityPoint[];
  jobId?: string;
  initialCash?: number;
  height?: number;
}

function toTs(dt: string): number {
  const m = (dt || "").match(/(\d{4})-(\d{2})-(\d{2})/);
  if (m) return Date.UTC(+m[1], +m[2] - 1, +m[3]) / 1000;
  return 0;
}

export default function EquityCurveChart({ data: propData, jobId, initialCash, height = 300 }: EquityCurveChartProps) {
  const chartContainerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [chartData, setChartData] = useState<EquityPoint[]>(propData || []);

  // If jobId provided, fetch downsampled chart data
  useEffect(() => {
    if (!jobId) return;
    import("../services/api").then(({ backtestAPI }) => {
      backtestAPI.chart(jobId, "auto").then((res: any) => {
        setChartData(res.data?.points || []);
      }).catch(() => {});
    });
  }, [jobId]);

  useEffect(() => {
    if (propData && propData.length > 0) {
      setChartData(propData);
    }
  }, [propData]);

  useEffect(() => {
    if (!chartContainerRef.current || chartData.length === 0) return;

    const chart = createChart(chartContainerRef.current, {
      height,
      layout: {
        background: { color: "#0f172a" },
        textColor: "#94a3b8",
      },
      grid: {
        vertLines: { color: "#1e293b" },
        horzLines: { color: "#1e293b" },
      },
      crosshair: {
        mode: 1,
        vertLine: { color: "#38bdf8", labelBackgroundColor: "#38bdf8" },
        horzLine: { color: "#38bdf8", labelBackgroundColor: "#38bdf8" },
      },
      rightPriceScale: { borderColor: "#334155" },
      timeScale: {
        borderColor: "#334155",
        timeVisible: true,
        secondsVisible: false,
        fixLeftEdge: true,
        fixRightEdge: true,
      },
    });
    chartRef.current = chart;

    const baselineValue = initialCash ?? chartData[0]?.total_value ?? 0;

    // 基准线（初始资金）
    if (baselineValue > 0) {
      const baseSeries = chart.addSeries(LineSeries, {
        color: "#64748b",
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
        tooltipEl.innerHTML = `
          <div style="font-size:0.75rem;color:#94a3b8">${dateStr}</div>
          <div style="font-size:0.875rem;font-weight:600">净值: ${value.toLocaleString()}</div>
          <div style="font-size:0.875rem;color:${Number(ret) >= 0 ? "#ef4444" : "#22c55e"}">收益率: ${ret}%</div>
        `;
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
  }, [chartData, height, initialCash]);

  if (chartData.length === 0) {
    return <div style={{ color: "#94a3b8", padding: "2rem", textAlign: "center" }}>暂无净值数据</div>;
  }

  return (
    <div ref={chartContainerRef} style={{ width: "100%", position: "relative" }}>
      <div
        ref={tooltipRef}
        style={{
          position: "absolute",
          display: "none",
          pointerEvents: "none",
          background: "#1e293b",
          border: "1px solid #334155",
          borderRadius: "0.375rem",
          padding: "0.5rem",
          zIndex: 10,
          whiteSpace: "nowrap",
        }}
      />
    </div>
  );
}
