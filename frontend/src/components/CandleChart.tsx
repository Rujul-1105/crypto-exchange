"use client";

import { useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  createChart,
  CandlestickSeries,
  HistogramSeries,
  type IChartApi,
  type ISeriesApi,
  type Time,
  ColorType,
  CrosshairMode,
} from "lightweight-charts";

import { api } from "@/lib/api";
import { useWebSocket } from "@/hooks/useWebSocket";
import { useCandleStore, type Candle } from "@/stores/candleStore";

type Interval = "1m" | "5m" | "1h";

const INTERVAL_OPTIONS: { value: Interval; label: string }[] = [
  { value: "1m", label: "1m" },
  { value: "5m", label: "5m" },
  { value: "1h", label: "1h" },
];

export function CandleChart({ symbol, interval }: { symbol: string; interval: Interval }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const volumeSeriesRef = useRef<ISeriesApi<"Histogram"> | null>(null);
  const candles = useCandleStore((s) => s.byInterval[interval] ?? []);
  const updateCandle = useCandleStore((s) => s.update);
  const setSeries = useCandleStore((s) => s.setSeries);

  // Historical candle fetch (REST) when symbol/interval changes.
  const history = useQuery({
    queryKey: ["candles", symbol, interval],
    queryFn: () => api.candles(symbol, interval, 500),
    enabled: !!symbol,
    staleTime: 30_000,
  });

  // Build chart once.
  useEffect(() => {
    if (!containerRef.current) return;
    const chart = createChart(containerRef.current, {
      layout: {
        background: { type: ColorType.Solid, color: "#11141b" },
        textColor: "#9aa3b2",
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      },
      grid: {
        vertLines: { color: "#222633" },
        horzLines: { color: "#222633" },
      },
      crosshair: { mode: CrosshairMode.Normal },
      rightPriceScale: { borderColor: "#2e3445" },
      timeScale: { borderColor: "#2e3445", timeVisible: true, secondsVisible: false },
      autoSize: true,
    });

    const candleSeries = chart.addSeries(CandlestickSeries, {
      upColor: "#16a34a",
      downColor: "#dc2626",
      borderVisible: false,
      wickUpColor: "#16a34a",
      wickDownColor: "#dc2626",
    });

    const volumeSeries = chart.addSeries(HistogramSeries, {
      color: "#3b82f6",
      priceFormat: { type: "volume" },
      priceScaleId: "",
    });
    volumeSeries.priceScale().applyOptions({
      scaleMargins: { top: 0.8, bottom: 0 },
    });

    chartRef.current = chart;
    candleSeriesRef.current = candleSeries;
    volumeSeriesRef.current = volumeSeries;

    return () => {
      chart.remove();
      chartRef.current = null;
      candleSeriesRef.current = null;
      volumeSeriesRef.current = null;
    };
  }, []);

  // Hydrate from REST history.
  useEffect(() => {
    if (!history.data) return;
    const series: Candle[] = history.data.map((c) => ({
      time: Math.floor(c.open_ts / 1000),
      open: Number(c.open),
      high: Number(c.high),
      low: Number(c.low),
      close: Number(c.close),
      volume: Number(c.volume),
    }));
    setSeries(interval, series);
  }, [history.data, interval, setSeries]);

  // Push series data into the chart when the store updates.
  useEffect(() => {
    if (!candleSeriesRef.current || !volumeSeriesRef.current) return;
    const seriesData = candles.map((c) => ({
      time: c.time as Time,
      open: c.open,
      high: c.high,
      low: c.low,
      close: c.close,
    }));
    const volumeData = candles.map((c) => ({
      time: c.time as Time,
      value: c.volume ?? 0,
      color: c.close >= c.open ? "#16a34a55" : "#dc262655",
    }));
    candleSeriesRef.current.setData(seriesData);
    volumeSeriesRef.current.setData(volumeData);
  }, [candles]);

  // WS subscription for live candle updates.
  const wsBase = process.env.NEXT_PUBLIC_WS_URL ?? "ws://localhost:8080/ws";
  const { subscribe, unsubscribe } = useWebSocket(wsBase, (msg) => {
    if (!msg || typeof msg !== "object") return;
    const event = (msg as any).event;
    const channel = (msg as any).channel;
    if (!event || !channel) return;
    if (channel === `candles:${symbol}:${interval}` && event.type === "candle") {
      const raw = event.candle;
      const candle: Candle = {
        time: Math.floor(raw.open_ts / 1000),
        open: Number(raw.open),
        high: Number(raw.high),
        low: Number(raw.low),
        close: Number(raw.close),
        volume: Number(raw.volume),
      };
      updateCandle(interval, candle);
    }
  });

  useEffect(() => {
    subscribe(`candles:${symbol}:${interval}`);
    return () => unsubscribe(`candles:${symbol}:${interval}`);
  }, [symbol, interval, subscribe, unsubscribe]);

  return (
    <section className="rounded-xl border border-line bg-bg-surface">
      <header className="flex items-center justify-between border-b border-line px-4 py-3">
        <div className="inline-flex rounded-full bg-bg-raised p-1 text-xs">
          {INTERVAL_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              onClick={() => {
                /* interval is controlled by the parent; we render all three
                   options to indicate availability, but for Phase 9 we keep a
                   single interval. A future enhancement can swap live. */
              }}
              className={`rounded-full px-3 py-1 font-mono transition ${
                opt.value === interval
                  ? "bg-bg-surface text-text-primary"
                  : "text-text-muted hover:text-text-primary"
              }`}
              aria-pressed={opt.value === interval}
            >
              {opt.label}
            </button>
          ))}
        </div>
        {history.isLoading && (
          <span className="text-[11px] uppercase tracking-[0.14em] text-text-dim">Loading</span>
        )}
      </header>
      <div ref={containerRef} className="h-[420px] w-full" />
    </section>
  );
}
