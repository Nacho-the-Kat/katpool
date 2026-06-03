"use client";

import { useMemo } from "react";
import type { EChartsCoreOption } from "echarts/core";
import { EChart } from "./echart";
import { useChartTokens } from "./use-tokens";
import { withAlpha } from "./color";
import { areaGradient, chartTooltip, crosshair, splitLine } from "./theme";

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

/**
 * A themed multi-series time chart: glowing lines, a layered gradient fill,
 * a soft crosshair, and — for single-series views — a live pulse and a
 * current-value label pinned to the leading edge.
 */
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
    const single = series.length === 1;
    const primary = series[0];
    const last = primary?.points[primary.points.length - 1];
    const primaryColor = palette[(primary?.colorIndex ?? 0) % palette.length] ?? "#49eacb";

    const lineSeries = series.map((s) => {
      const color = palette[(s.colorIndex ?? 0) % palette.length] ?? "#49eacb";
      return {
        name: s.name,
        type: "line" as const,
        smooth,
        smoothMonotone: "x" as const,
        showSymbol: false,
        sampling: "lttb" as const,
        lineStyle: { width: 2, color, shadowColor: withAlpha(color, 0.45), shadowBlur: 10 },
        itemStyle: { color },
        emphasis: { focus: "series" as const },
        areaStyle: s.area === false ? undefined : { color: areaGradient(color, 0.3) },
        endLabel:
          single && last
            ? {
                show: true,
                formatter: () => valueFormatter(last.v),
                color: tokens.text,
                backgroundColor: withAlpha(tokens.tooltipBg, 0.9),
                borderColor: withAlpha(color, 0.5),
                borderWidth: 1,
                borderRadius: 6,
                padding: [3, 7] as [number, number],
                fontSize: 12,
                fontWeight: 600 as const,
              }
            : undefined,
        data: s.points.map((p) => [p.t, p.v]),
      };
    });

    const pulse =
      single && last
        ? [
            {
              type: "effectScatter" as const,
              coordinateSystem: "cartesian2d" as const,
              symbolSize: 8,
              showEffectOn: "render" as const,
              rippleEffect: { scale: 3, brushType: "stroke" as const },
              itemStyle: { color: primaryColor, shadowColor: withAlpha(primaryColor, 0.8), shadowBlur: 10 },
              data: [[last.t, last.v]],
              z: 5,
              silent: true,
            },
          ]
        : [];

    return {
      animationDuration: 700,
      animationEasing: "cubicOut" as const,
      grid: {
        left: 8,
        right: single && last ? 64 : 16,
        top: 16,
        bottom: showZoom ? 56 : 24,
        containLabel: true,
      },
      tooltip: {
        trigger: "axis",
        axisPointer: crosshair(tokens),
        valueFormatter: (v: unknown) => valueFormatter(Number(v)),
        ...chartTooltip(tokens),
      },
      legend:
        series.length > 1
          ? { top: 0, right: 8, textStyle: { color: tokens.muted }, icon: "roundRect", itemWidth: 10, itemHeight: 10 }
          : undefined,
      xAxis: {
        type: "time",
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { color: tokens.muted, hideOverlap: true, fontSize: 11, margin: 12 },
        splitLine: { show: false },
      },
      yAxis: {
        type: "value",
        axisLabel: { color: tokens.muted, fontSize: 11, formatter: (v: number) => valueFormatter(v) },
        splitLine: splitLine(tokens),
      },
      dataZoom: showZoom
        ? [
            { type: "inside", throttle: 50 },
            {
              type: "slider",
              height: 18,
              bottom: 16,
              borderColor: "transparent",
              backgroundColor: withAlpha(tokens.muted, 0.06),
              fillerColor: withAlpha(primaryColor, 0.12),
              handleStyle: { color: primaryColor, borderColor: primaryColor },
              moveHandleStyle: { color: primaryColor },
              dataBackground: { lineStyle: { color: withAlpha(primaryColor, 0.4) }, areaStyle: { color: withAlpha(primaryColor, 0.1) } },
              selectedDataBackground: { lineStyle: { color: primaryColor }, areaStyle: { color: withAlpha(primaryColor, 0.2) } },
              textStyle: { color: tokens.muted, fontSize: 10 },
            },
          ]
        : undefined,
      series: [...lineSeries, ...pulse],
    };
  }, [series, tokens, valueFormatter, showZoom, smooth]);

  return <EChart option={option} height={height} notMerge />;
}
