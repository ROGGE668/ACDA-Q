import { useEffect, useRef } from "react";
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

function toTs(dt: string): number {
  const m = (dt || "").match(/(\d{4})-(\d{2})-(\d{2})/);
  if (m) return Date.UTC(+m[1], +m[2] - 1, +m[3]) / 1000;
  return 0;
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
    if (this._requestUpdate) this._requestUpdate();
  }

  paneViews() {
    return [
      {
        renderer: () => ({
          draw: (target: any) => {
            target.useMediaCoordinateSpace(({ context, mediaSize }: any) => {
              const ranges = this._getRanges();
              for (const range of ranges) {
                const x1 = this._chart!.timeScale().timeToCoordinate(toTs(range.startTime) as any);
                const x2 = this._chart!.timeScale().timeToCoordinate(toTs(range.endTime) as any);
                if (x1 === null && x2 === null) continue;
                const left = x1 ?? 0;
                const right = x2 ?? mediaSize.width;
                if (right <= left) continue;
                context.fillStyle = range.color;
                context.fillRect(left, 0, right - left, mediaSize.height);
              }
            });
          },
        }),
      },
    ];
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

  const dataFp = validData.length + "|" + (validData.length > 0 ? validData[0].datetime + validData[validData.length - 1].datetime : "");
  const prevDataFp = useRef("");

  useEffect(() => {
    if (!chartContainerRef.current || validData.length === 0) return;
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
        layout: { background: { color: "#0f172a" }, textColor: "#94a3b8" },
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
      validData.map((d) => ({
        time: toTs(d.datetime) as any,
        open: Number(d.open) || 0,
        high: Number(d.high) || 0,
        low: Number(d.low) || 0,
        close: Number(d.close) || 0,
      })) as any
    );

    // MA
    const ma5 = calcMA(validData, 5);
    if (ma5.length)
      chart
        .addSeries(LineSeries, {
          color: "#f59e0b",
          lineWidth: 1,
          priceLineVisible: false,
          lastValueVisible: false,
        } as any)
        .setData(ma5 as any);
    const ma10 = calcMA(validData, 10);
    if (ma10.length)
      chart
        .addSeries(LineSeries, {
          color: "#8b5cf6",
          lineWidth: 1,
          priceLineVisible: false,
          lastValueVisible: false,
        } as any)
        .setData(ma10 as any);
    const ma20 = calcMA(validData, 20);
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
    if (validData[0]?.volume !== undefined) {
      const volSeries = chart.addSeries(HistogramSeries, {
        color: "#38bdf8",
        priceFormat: { type: "volume" },
        priceScaleId: "",
        priceLineVisible: false,
        lastValueVisible: false,
      });
      volSeries.priceScale().applyOptions({ scaleMargins: { top: 0.8, bottom: 0 } });
      volSeries.setData(
        validData.map((d) => ({
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
  }, [dataFp, validData, height]);

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

    const markers = validTrades.map((t) => {
      const isBuy = t.type === "BUY";
      const p = t.pnl ?? 0;
      const days = holdingDaysMap.get(t.time);
      const daysText = !isBuy && days !== undefined ? `(${days}天)` : "";
      return {
        time: toTs(t.time) as any,
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
  }, [validTrades]);

  if (validData.length === 0) {
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
              background: period === p ? "var(--primary)" : "var(--bg)",
              color: period === p ? "#fff" : "var(--muted)",
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
