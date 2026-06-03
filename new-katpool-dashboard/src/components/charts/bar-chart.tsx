"use client";

import { useMemo } from "react";
import type { EChartsCoreOption } from "echarts/core";
import { EChart } from "./echart";
import { useChartTokens } from "./use-tokens";

export interface BarDatum {
  label: string;
  value: number;
}

interface HBarChartProps {
  data: BarDatum[];
  height?: number;
  valueFormatter?: (v: number) => string;
  colorIndex?: number;
}

/** A themed horizontal bar chart (e.g. reject reasons, top workers). */
export function HBarChart({
  data,
  height = 280,
  valueFormatter = (v) => v.toLocaleString("en-US"),
  colorIndex = 0,
}: HBarChartProps) {
  const tokens = useChartTokens();

  const option = useMemo<EChartsCoreOption>(() => {
    const color = tokens.series[colorIndex % tokens.series.length] ?? "#49eacb";
    const sorted = [...data].sort((a, b) => a.value - b.value);
    return {
      animationDuration: 500,
      grid: { left: 8, right: 24, top: 8, bottom: 8, containLabel: true },
      tooltip: {
        trigger: "axis",
        axisPointer: { type: "shadow" },
        backgroundColor: tokens.tooltipBg,
        borderColor: tokens.border,
        textStyle: { color: tokens.text, fontSize: 12 },
        valueFormatter: (v: unknown) => valueFormatter(Number(v)),
      },
      xAxis: {
        type: "value",
        axisLabel: { color: tokens.muted, formatter: (v: number) => valueFormatter(v) },
        splitLine: { lineStyle: { color: tokens.grid, type: "dashed" } },
      },
      yAxis: {
        type: "category",
        data: sorted.map((d) => d.label),
        axisLine: { lineStyle: { color: tokens.grid } },
        axisLabel: { color: tokens.muted },
      },
      series: [
        {
          type: "bar",
          data: sorted.map((d) => d.value),
          barWidth: "60%",
          itemStyle: { color, borderRadius: [0, 6, 6, 0] },
        },
      ],
    };
  }, [data, tokens, valueFormatter, colorIndex]);

  return <EChart option={option} height={height} notMerge />;
}
