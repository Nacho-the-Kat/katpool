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

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current, undefined, { renderer: "canvas" });
    chartRef.current = chart;
    const ro = new ResizeObserver(() => chart.resize());
    ro.observe(ref.current);
    return () => {
      ro.disconnect();
      chart.dispose();
      chartRef.current = null;
    };
  }, []);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    // Clear any live tooltip/axis pointer *before* swapping series models on a
    // refresh. With `replaceMerge: ["series"]`, a mousemove hit-test can land on
    // stale shapes during the swap and strand the pointer — it freezes and stops
    // tracking the cursor. Tearing the tip down first closes that race; the next
    // mousemove re-shows it cleanly. (Apache ECharts replaceMerge interaction.)
    chart.dispatchAction({ type: "hideTip" });
    chart.dispatchAction({ type: "updateAxisPointer", currTrigger: "leave" });
    chart.setOption(option, { notMerge, replaceMerge });
  }, [option, notMerge, replaceMerge]);

  return <div ref={ref} className={cn("w-full", className)} style={{ height }} />;
}
