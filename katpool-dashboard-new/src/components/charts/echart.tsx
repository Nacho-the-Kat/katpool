"use client";

import { useEffect, useRef } from "react";
import * as echarts from "echarts/core";
import { LineChart, BarChart, PieChart, EffectScatterChart } from "echarts/charts";
import {
  GridComponent,
  TooltipComponent,
  LegendComponent,
  DataZoomComponent,
  MarkLineComponent,
  GraphicComponent,
  TitleComponent,
} from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { EChartsCoreOption } from "echarts/core";
import { cn } from "@/lib/utils";

echarts.use([
  LineChart,
  BarChart,
  PieChart,
  EffectScatterChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  DataZoomComponent,
  MarkLineComponent,
  GraphicComponent,
  TitleComponent,
  CanvasRenderer,
]);

interface EChartProps {
  option: EChartsCoreOption;
  className?: string;
  /** Fixed pixel height; the chart fills its container width responsively. */
  height?: number;
  notMerge?: boolean;
  /**
   * Components to fully replace on update (e.g. `["series"]`). Lets live data
   * refreshes swap series cleanly — no stale slices — while merging the rest,
   * so an open tooltip/crosshair survives the refresh instead of vanishing.
   */
  replaceMerge?: string[];
}

interface PendingUpdate {
  option: EChartsCoreOption;
  notMerge: boolean;
  replaceMerge?: string[];
}

/** A disposable, resize-aware ECharts canvas. */
export function EChart({
  option,
  className,
  height = 300,
  notMerge = false,
  replaceMerge,
}: EChartProps) {
  const ref = useRef<HTMLDivElement>(null);
  const chartRef = useRef<echarts.ECharts | null>(null);
  // True while the pointer is over the canvas. Applying new data mid-hover
  // strands the axis pointer (it freezes and stops tracking) and resets pie
  // emphasis (a visible flicker) — a known ECharts setOption-during-hover
  // interaction. We hold the latest update here and flush it on pointer-leave.
  const hoveringRef = useRef(false);
  const pendingRef = useRef<PendingUpdate | null>(null);

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current, undefined, { renderer: "canvas" });
    chartRef.current = chart;
    const ro = new ResizeObserver(() => chart.resize());
    ro.observe(ref.current);

    const zr = chart.getZr();
    const onMove = () => {
      hoveringRef.current = true;
    };
    const onOut = () => {
      hoveringRef.current = false;
      const pending = pendingRef.current;
      if (pending) {
        pendingRef.current = null;
        chart.setOption(pending.option, {
          notMerge: pending.notMerge,
          replaceMerge: pending.replaceMerge,
        });
      }
    };
    zr.on("mousemove", onMove);
    zr.on("globalout", onOut);

    return () => {
      ro.disconnect();
      zr.off("mousemove", onMove);
      zr.off("globalout", onOut);
      chart.dispose();
      chartRef.current = null;
    };
  }, []);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    // Defer live refreshes while the user is exploring the chart; the newest
    // update is applied the instant the pointer leaves (see onOut above).
    if (hoveringRef.current) {
      pendingRef.current = { option, notMerge, replaceMerge };
      return;
    }
    chart.setOption(option, { notMerge, replaceMerge });
  }, [option, notMerge, replaceMerge]);

  return <div ref={ref} className={cn("w-full", className)} style={{ height }} />;
}
