import { useEffect, useRef, useMemo } from "react";
import {
  createChart,
  IChartApi,
  CandlestickSeries,
  HistogramSeries,
  LineSeries,
  ISeriesApi,
  createSeriesMarkers,
  ISeriesMarkersPluginApi,
} from "lightweight-charts";

interface KLineItem {
  datetime: string;
  open: number;
  high: number;
  low: number;
  close: number;
  volume?: number;
}

interface TradeMarker {
  time: string;
  price: number;
  type: "BUY" | "SELL";
  pnl?: number;
}

interface TradeHoldingRange {
  startTime: string;
  endTime: string;
  color: string;
}

interface KLineChartProps {
  data: KLineItem[];
  trades?: TradeMarker[];
  height?: number;
  period?: string;
  onPeriodChange?: (period: string) => void;
}

const PERIODS = ["1m", "5m", "15m", "30m", "1h", "1d", "1w"];

// 将后端存储的周期格式（1min/5min/60min 等）标准化为前端按钮格式（1m/5m/1h 等）
const BACKEND_TO_FRONTEND: Record<string, string> = {
  "1min": "1m", "5min": "5m", "15min": "15m", "30min": "30m", "60min": "1h",
};

function normalizePeriod(p: string): string {
  return BACKEND_TO_FRONTEND[p] || p;
}

function toTs(dt: string): number {
  const m = (dt || "").match(/(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2}))?/);
  if (m) return Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +(m[6] || 0)) / 1000;
  const d = (dt || "").match(/(\d{4})-(\d{2})-(\d{2})/);
  if (d) return Date.UTC(+d[1], +d[2] - 1, +d[3]) / 1000;
  return 0;
}

// 按目标周期聚合 K 线数据（客户端降采样）
const PERIOD_MINUTES: Record<string, number> = {
  "1m": 1, "5m": 5, "15m": 15, "30m": 30, "1h": 60, "1d": 1440, "1w": 10080,
};

function resampleKline(data: KLineItem[], targetPeriod: string): KLineItem[] {
  const targetMin = PERIOD_MINUTES[targetPeriod] || 1;
  // 获取源数据的最小周期
  let srcMin = Infinity;
  for (let i = 1; i < Math.min(data.length, 5); i++) {
    const diff = (toTs(data[i].datetime) - toTs(data[i - 1].datetime)) / 60;
    if (diff > 0 && diff < srcMin) srcMin = diff;
  }
  if (srcMin === Infinity || srcMin <= 0) srcMin = 1;
  // 如果目标周期 <= 源周期，不需要聚合
  if (targetMin <= srcMin) return data;

  const buckets = new Map<number, KLineItem[]>();
  for (const bar of data) {
    const ts = toTs(bar.datetime);
    // 按目标周期截断时间戳
    const bucketTs = Math.floor(ts / (targetMin * 60)) * (targetMin * 60);
    if (!buckets.has(bucketTs)) buckets.set(bucketTs, []);
    buckets.get(bucketTs)!.push(bar);
  }

  const result: KLineItem[] = [];
  for (const [bucketTs, bars] of buckets) {
    const first = bars[0];
    const last = bars[bars.length - 1];
    const dt = new Date(bucketTs * 1000);
    const y = dt.getUTCFullYear();
    const mo = String(dt.getUTCMonth() + 1).padStart(2, "0");
    const d = String(dt.getUTCDate()).padStart(2, "0");
    const h = String(dt.getUTCHours()).padStart(2, "0");
    const mi = String(dt.getUTCMinutes()).padStart(2, "0");
    const datetime = `${y}-${mo}-${d}T${h}:${mi}:00`;
    result.push({
      datetime,
      open: Number(first.open),
      high: Math.max(...bars.map(b => Number(b.high))),
      low: Math.min(...bars.map(b => Number(b.low))),
      close: Number(last.close),
      volume: bars.reduce((s, b) => s + (Number(b.volume) || 0), 0),
    });
  }
  result.sort((a, b) => toTs(a.datetime) - toTs(b.datetime));
  return result;
}

function calcMA(data: KLineItem[], period: number): { time: number; value: number }[] {
  const r: { time: number; value: number }[] = [];
  for (let i = 0; i < data.length; i++) {
    if (i < period - 1) continue;
    let sum = 0;
    for (let j = i - period + 1; j <= i; j++) sum += Number(data[j].close) || 0;
    r.push({ time: toTs(data[i].datetime), value: sum / period });
  }
  return r;
}

function getTradeColor(index: number): string {
  const hue = (index * 137.508) % 360;
  return `hsla(${Math.round(hue)}, 65%, 55%, 0.3)`;
}

// 按 FIFO 配对买卖，生成每笔交易的持有区间
function buildHoldingRanges(trades: TradeMarker[]): TradeHoldingRange[] {
  const sorted = [...trades].sort((a, b) => new Date(a.time).getTime() - new Date(b.time).getTime());
  const buyQueue: { time: string }[] = [];
  const ranges: TradeHoldingRange[] = [];
  let colorIndex = 0;

  for (const t of sorted) {
    if (t.type === "BUY") {
      buyQueue.push({ time: t.time });
    } else if (t.type === "SELL" && buyQueue.length > 0) {
      const buy = buyQueue.shift()!;
      ranges.push({
        startTime: buy.time,
        endTime: t.time,
        color: getTradeColor(colorIndex++),
      });
    }
  }

  // 未平仓的买入（持有到最后一根 K 线）
  for (const buy of buyQueue) {
    ranges.push({
      startTime: buy.time,
      endTime: sorted[sorted.length - 1]?.time || buy.time,
      color: getTradeColor(colorIndex++),
    });
  }

  return ranges;
}

// lightweight-charts v5 自定义 Primitive：在持有区间绘制半透明背景色柱
class TradeBackgroundPrimitive {
  private _chart: IChartApi | null = null;
  private _requestUpdate: (() => void) | null = null;

  constructor(private _getRanges: () => TradeHoldingRange[]) {}

  attached(params: any) {
    this._chart = params.chart;
    this._requestUpdate = params.requestUpdate;
  }

  detached() {
    this._chart = null;
    this._requestUpdate = null;
  }

  update() {
    this._requestUpdate?.();
  }

  renderer() {
    return {
      draw: (target: any) => {
        try {
          const chart = this._chart;
          if (!chart) return;
          const ranges = this._getRanges();
          if (!ranges.length) return;

          const timeScale = chart.timeScale();
          const priceScale = chart.priceScale("right");

          const seriesArr = typeof (chart as any).serieses === "function" ? (chart as any).serieses() : [];
          const series = seriesArr[0];
          if (!series) return;

          for (const range of ranges) {
            const x1 = timeScale.timeToCoordinate(toTs(range.startTime) as any);
            const x2 = timeScale.timeToCoordinate(toTs(range.endTime) as any);
            if (x1 === null || x2 === null) continue;

            const data0 = series.data()?.[0];
            if (!data0) continue;
            const yTop = (priceScale as any).priceToCoordinate((series as any).priceToCoordinate(data0.high ?? 100) ?? 0);
            const yBottom = (priceScale as any).priceToCoordinate((series as any).priceToCoordinate(data0.low ?? 0) ?? 100);
            if (yTop === null || yBottom === null) continue;

            target.rect(
              { x: Math.min(x1, x2), y: Math.min(yTop, yBottom) },
              { width: Math.abs(x2 - x1), height: Math.abs(yBottom - yTop) },
              range.color
            );
          }
        } catch (_) {}
      },
    };
  }
}

export default function KLineChart({ data, trades = [], height = 400, period = "1d", onPeriodChange }: KLineChartProps) {
  const chartContainerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const markersRef = useRef<ISeriesMarkersPluginApi<any> | null>(null);
  const primitiveRef = useRef<TradeBackgroundPrimitive | null>(null);
  const rangesRef = useRef<TradeHoldingRange[]>([]);

  const validData: KLineItem[] = Array.isArray(data) ? data : [];
  const validTrades: TradeMarker[] = Array.isArray(trades) ? trades : [];

  // 标准化后端周期格式（1min→1m, 5min→5m, 60min→1h 等）
  const normalizedPeriod = normalizePeriod(period);

  // 客户端降采样到目标周期
  const displayData = useMemo(
    () => resampleKline(validData, normalizedPeriod),
    [validData, normalizedPeriod]
  );

  // 构建 resampled 时间戳集合，用于将交易标记对齐到最近的 K 线
  const candleTimeSet = useMemo(() => {
    const set = new Set<number>();
    for (const d of displayData) set.add(toTs(d.datetime));
    return set;
  }, [displayData]);

  // 构建有序时间戳数组，用于 snap 交易时间到最近的 K 线
  const candleTimes = useMemo(() => {
    return Array.from(candleTimeSet).sort((a, b) => a - b);
  }, [candleTimeSet]);

  // snap 交易时间到最近的 candle 时间戳
  const snapTradeTime = (tradeTime: string): number => {
    const ts = toTs(tradeTime);
    if (candleTimeSet.has(ts)) return ts;
    // 二分查找最近的 candle 时间
    let lo = 0, hi = candleTimes.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (candleTimes[mid] < ts) lo = mid + 1; else hi = mid;
    }
    // 比较 lo 和 lo-1，选最近的
    if (lo > 0 && Math.abs(candleTimes[lo - 1] - ts) < Math.abs(candleTimes[lo] - ts)) {
      return candleTimes[lo - 1];
    }
    return candleTimes[lo] ?? ts;
  };

  // 用 snap 后的时间生成有效的交易标记
  const snappedTrades = useMemo(() => {
    if (candleTimes.length === 0) return validTrades;
    return validTrades.map((t) => ({
      ...t,
      _snapTs: snapTradeTime(t.time),
    }));
  }, [validTrades, candleTimes]);

  const dataFp = displayData.length + "|" + (displayData.length > 0 ? displayData[0].datetime + displayData[displayData.length - 1].datetime : "") + "|" + normalizedPeriod;
  const prevDataFp = useRef("");

  useEffect(() => {
    if (!chartContainerRef.current || displayData.length === 0) return;
    if (dataFp === prevDataFp.current) return;
    prevDataFp.current = dataFp;

    if (chartRef.current) {
      chartRef.current.remove();
      chartRef.current = null;
      candleRef.current = null;
      primitiveRef.current = null;
      markersRef.current = null;
    }

    let chart: IChartApi;
    try {
      chart = createChart(chartContainerRef.current, {
        height,
        layout: { background: { color: "#0f172a" }, textColor: "#94a3b8", fontFamily: "-apple-system, BlinkMacSystemFont, PingFang SC, Microsoft YaHei, sans-serif" },
        localization: { locale: "zh-CN" },
        grid: { vertLines: { color: "#1e293b" }, horzLines: { color: "#1e293b" } },
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
    } catch (e) {
      return;
    }
    chartRef.current = chart;

    const candleSeries = chart.addSeries(CandlestickSeries, {
      upColor: "#ef4444",
      downColor: "#22c55e",
      borderUpColor: "#ef4444",
      borderDownColor: "#22c55e",
      wickUpColor: "#ef4444",
      wickDownColor: "#22c55e",
      priceLineVisible: false,
      lastValueVisible: false,
    });
    candleRef.current = candleSeries as any;

    candleSeries.setData(
      displayData.map((d) => ({
        time: toTs(d.datetime) as any,
        open: Number(d.open) || 0,
        high: Number(d.high) || 0,
        low: Number(d.low) || 0,
        close: Number(d.close) || 0,
      })) as any
    );

    // MA
    const ma5 = calcMA(displayData, 5);
    if (ma5.length)
      chart
        .addSeries(LineSeries, {
          color: "#f59e0b",
          lineWidth: 1,
          priceLineVisible: false,
          lastValueVisible: false,
        } as any)
        .setData(ma5 as any);
    const ma10 = calcMA(displayData, 10);
    if (ma10.length)
      chart
        .addSeries(LineSeries, {
          color: "#8b5cf6",
          lineWidth: 1,
          priceLineVisible: false,
          lastValueVisible: false,
        } as any)
        .setData(ma10 as any);
    const ma20 = calcMA(displayData, 20);
    if (ma20.length)
      chart
        .addSeries(LineSeries, {
          color: "#06b6d4",
          lineWidth: 1,
          priceLineVisible: false,
          lastValueVisible: false,
        } as any)
        .setData(ma20 as any);

    // 成交量
    if (displayData[0]?.volume !== undefined) {
      const volSeries = chart.addSeries(HistogramSeries, {
        color: "#38bdf8",
        priceFormat: { type: "volume" },
        priceScaleId: "",
        priceLineVisible: false,
        lastValueVisible: false,
      });
      volSeries.priceScale().applyOptions({ scaleMargins: { top: 0.8, bottom: 0 } });
      volSeries.setData(
        displayData.map((d) => ({
          time: toTs(d.datetime) as any,
          value: Number(d.volume) || 0,
          color: (Number(d.close) || 0) >= (Number(d.open) || 0) ? "#ef4444" : "#22c55e",
        })) as any
      );
    }

    // markers plugin (initially empty)
    markersRef.current = createSeriesMarkers(candleSeries, []);

    // 附加交易背景 primitive（ranges 会在 trades effect 中填充后触发 update）
    const primitive = new TradeBackgroundPrimitive(() => rangesRef.current);
    (candleSeries as any).attachPrimitive(primitive);
    primitiveRef.current = primitive;

    chart.timeScale().fitContent();

    const handleResize = () => {
      if (chartContainerRef.current)
        chart.applyOptions({ width: chartContainerRef.current.clientWidth });
    };
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      if (candleRef.current && primitiveRef.current) {
        try {
          (candleRef.current as any).detachPrimitive(primitiveRef.current);
        } catch {}
      }
      primitiveRef.current = null;
      markersRef.current = null;
      chart.remove();
      chartRef.current = null;
      candleRef.current = null;
    };
  }, [dataFp, displayData, height, normalizedPeriod]);

  // 交易标记 + 持有区间背景（独立更新，不重建 chart）
  useEffect(() => {
    if (!markersRef.current) return;
    if (validTrades.length === 0) {
      markersRef.current.setMarkers([]);
      rangesRef.current = [];
      primitiveRef.current?.update();
      return;
    }

    // 计算持有区间
    rangesRef.current = buildHoldingRanges(validTrades);
    primitiveRef.current?.update();

    // 计算每笔卖出的持仓天数（FIFO 配对）
    const holdingDaysMap = new Map<string, number>();
    const buyQueue: { time: string; ts: number }[] = [];
    const sorted = [...validTrades].sort((a, b) => new Date(a.time).getTime() - new Date(b.time).getTime());
    for (const t of sorted) {
      const ts = new Date(t.time).getTime();
      if (t.type === "BUY") {
        buyQueue.push({ time: t.time, ts });
      } else if (t.type === "SELL" && buyQueue.length > 0) {
        const buy = buyQueue.shift()!;
        const days = Math.round((ts - buy.ts) / (1000 * 60 * 60 * 24));
        holdingDaysMap.set(t.time, days);
      }
    }

    const markers = snappedTrades.map((t: any) => {
      const isBuy = t.type === "BUY";
      const p = t.pnl ?? 0;
      const days = holdingDaysMap.get(t.time);
      const daysText = !isBuy && days !== undefined ? `(${days}天)` : "";
      return {
        time: t._snapTs as any,
        position: isBuy ? ("belowBar" as const) : ("aboveBar" as const),
        color: isBuy ? "#ffffff" : p >= 0 ? "#ef4444" : "#22c55e",
        shape: isBuy ? ("arrowUp" as const) : ("arrowDown" as const),
        text: isBuy
          ? "买"
          : (p >= 0 ? "赚" + p.toFixed(0) : "亏" + Math.abs(p).toFixed(0)) + daysText,
        size: 1.8,
      };
    });
    markersRef.current.setMarkers(markers);
  }, [snappedTrades, candleTimes]);

  if (displayData.length === 0) {
    return <div style={{ color: "#94a3b8", padding: "2rem", textAlign: "center" }}>暂无 K 线数据</div>;
  }
  return (
    <div>
      <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: "0.5rem", gap: "0.25rem" }}>
        {PERIODS.map((p) => (
          <button
            key={p}
            onClick={() => onPeriodChange?.(p)}
            style={{
              padding: "0.25rem 0.5rem",
              fontSize: "0.75rem",
              background: normalizedPeriod === p ? "var(--primary)" : "var(--bg)",
              color: normalizedPeriod === p ? "#fff" : "var(--muted)",
              border: "1px solid var(--border)",
              borderRadius: "0.25rem",
              cursor: "pointer",
            }}
          >
            {p}
          </button>
        ))}
      </div>
      <div ref={chartContainerRef} style={{ width: "100%" }} />
    </div>
  );
}
