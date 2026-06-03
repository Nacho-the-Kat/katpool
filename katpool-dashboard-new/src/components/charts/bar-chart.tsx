"use client";

import { useMemo } from "react";
import type { EChartsCoreOption } from "echarts/core";
import { EChart } from "./echart";
import { useChartTokens } from "./use-tokens";
import { withAlpha } from "./color";
import { chartTooltip, splitLine } from "./theme";

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
      animationDuration: 600,
      animationEasing: "cubicOut" as const,
      grid: { left: 8, right: 48, top: 8, bottom: 8, containLabel: true },
      tooltip: {
        trigger: "axis",
        axisPointer: { type: "shadow", shadowStyle: { color: withAlpha(color, 0.08) } },
        valueFormatter: (v: unknown) => valueFormatter(Number(v)),
        ...chartTooltip(tokens),
      },
      xAxis: {
        type: "value",
        axisLabel: { color: tokens.muted, fontSize: 11, formatter: (v: number) => valueFormatter(v) },
        splitLine: splitLine(tokens),
      },
      yAxis: {
        type: "category",
        data: sorted.map((d) => d.label),
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { color: tokens.muted, fontSize: 12 },
      },
      series: [
        {
          type: "bar",
          data: sorted.map((d) => d.value),
          barWidth: "58%",
          showBackground: true,
          backgroundStyle: { color: withAlpha(tokens.muted, 0.07), borderRadius: 6 },
          itemStyle: {
            borderRadius: [0, 6, 6, 0],
            color: {
              type: "linear",
              x: 0,
              y: 0,
              x2: 1,
              y2: 0,
              colorStops: [
                { offset: 0, color: withAlpha(color, 0.55) },
                { offset: 1, color },
              ],
            },
          },
          label: {
            show: true,
            position: "right",
            color: tokens.muted,
            fontSize: 11,
            formatter: (p: { value: number }) => valueFormatter(p.value),
          },
        },
      ],
    };
  }, [data, tokens, valueFormatter, colorIndex]);

  return <EChart option={option} height={height} notMerge />;
}
