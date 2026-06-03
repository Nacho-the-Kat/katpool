"use client";

import { useMemo } from "react";
import type { EChartsCoreOption } from "echarts/core";
import { EChart } from "./echart";
import { useChartTokens } from "./use-tokens";
import { withAlpha } from "./color";

export interface SeriesDef {
  name: string;
  points: { t: string; v: number }[];
  /** Index into the token palette. */
  colorIndex?: number;
  area?: boolean;
}

interface TimeSeriesChartProps {
  series: SeriesDef[];
  height?: number;
  /** Formats the y value in tooltip + axis. */
  valueFormatter: (v: number) => string;
  showZoom?: boolean;
  smooth?: boolean;
}

/** A themed multi-series time chart with gradient area fill and zoom. */
export function TimeSeriesChart({
  series,
  height = 300,
  valueFormatter,
  showZoom = false,
  smooth = true,
}: TimeSeriesChartProps) {
  const tokens = useChartTokens();

  const option = useMemo<EChartsCoreOption>(() => {
    const palette = tokens.series;
    return {
      animationDuration: 600,
      grid: { left: 8, right: 16, top: 16, bottom: showZoom ? 56 : 24, containLabel: true },
      tooltip: {
        trigger: "axis",
        backgroundColor: tokens.tooltipBg,
        borderColor: tokens.border,
        textStyle: { color: tokens.text, fontSize: 12 },
        valueFormatter: (v: unknown) => valueFormatter(Number(v)),
      },
      legend:
        series.length > 1
          ? { top: 0, right: 8, textStyle: { color: tokens.muted }, icon: "roundRect" }
          : undefined,
      xAxis: {
        type: "time",
        axisLine: { lineStyle: { color: tokens.grid } },
        axisLabel: { color: tokens.muted, hideOverlap: true },
        splitLine: { show: false },
      },
      yAxis: {
        type: "value",
        axisLabel: { color: tokens.muted, formatter: (v: number) => valueFormatter(v) },
        splitLine: { lineStyle: { color: tokens.grid, type: "dashed" } },
      },
      dataZoom: showZoom
        ? [
            { type: "inside", throttle: 50 },
            {
              type: "slider",
              height: 18,
              bottom: 16,
              borderColor: tokens.border,
              fillerColor: withAlpha(palette[0] ?? "#49eacb", 0.13),
              handleStyle: { color: palette[0] },
              textStyle: { color: tokens.muted },
            },
          ]
        : undefined,
      series: series.map((s) => {
        const color = palette[(s.colorIndex ?? 0) % palette.length] ?? "#49eacb";
        return {
          name: s.name,
          type: "line",
          smooth,
          showSymbol: false,
          lineStyle: { width: 2, color },
          itemStyle: { color },
          areaStyle:
            s.area === false
              ? undefined
              : {
                  color: {
                    type: "linear",
                    x: 0,
                    y: 0,
                    x2: 0,
                    y2: 1,
                    colorStops: [
                      { offset: 0, color: withAlpha(color, 0.28) },
                      { offset: 1, color: withAlpha(color, 0) },
                    ],
                  },
                },
          data: s.points.map((p) => [p.t, p.v]),
        };
      }),
    };
  }, [series, tokens, valueFormatter, showZoom, smooth]);

  return <EChart option={option} height={height} notMerge />;
}
