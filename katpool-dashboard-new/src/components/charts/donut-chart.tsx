"use client";

import { useMemo } from "react";
import type { EChartsCoreOption } from "echarts/core";
import { EChart } from "./echart";
import { useChartTokens } from "./use-tokens";
import { chartTooltip } from "./theme";

export interface DonutDatum {
  name: string;
  value: number;
}

interface DonutChartProps {
  data: DonutDatum[];
  height?: number;
  valueFormatter?: (v: number) => string;
  centerLabel?: string;
  centerValue?: string;
}

/** A themed donut with a centered headline and legend. */
export function DonutChart({
  data,
  height = 280,
  valueFormatter = (v) => v.toLocaleString("en-US"),
  centerLabel,
  centerValue,
}: DonutChartProps) {
  const tokens = useChartTokens();

  const option = useMemo<EChartsCoreOption>(() => {
    const single = data.length === 1;
    return {
      animationDuration: 700,
      animationEasing: "cubicOut" as const,
      color: tokens.series,
      tooltip: {
        trigger: "item",
        valueFormatter: (v: unknown) => valueFormatter(Number(v)),
        ...chartTooltip(tokens),
      },
      legend: {
        type: "scroll",
        orient: "vertical",
        right: 8,
        top: "middle",
        textStyle: { color: tokens.muted, fontSize: 12 },
        itemWidth: 10,
        itemHeight: 10,
        itemGap: 12,
        icon: "roundRect",
      },
      graphic:
        centerValue != null
          ? {
              type: "group",
              left: "32%",
              top: "center",
              children: [
                {
                  type: "text",
                  style: {
                    text: centerValue,
                    fill: tokens.text,
                    font: "600 22px var(--font-sans, sans-serif)",
                    textAlign: "center",
                  },
                  top: -10,
                },
                {
                  type: "text",
                  style: {
                    text: centerLabel ?? "",
                    fill: tokens.muted,
                    font: "12px var(--font-sans, sans-serif)",
                    textAlign: "center",
                  },
                  top: 16,
                },
              ],
            }
          : undefined,
      series: [
        {
          type: "pie",
          radius: ["62%", "84%"],
          center: ["32%", "50%"],
          avoidLabelOverlap: true,
          // A single category reads as one continuous ring — no seam.
          itemStyle: {
            borderColor: tokens.card,
            borderWidth: single ? 0 : 2,
            borderRadius: single ? 0 : 6,
          },
          label: { show: false },
          emphasis: { scaleSize: 6, itemStyle: { shadowBlur: 16, shadowColor: "rgba(0,0,0,0.25)" } },
          data: data.map((d) => ({ name: d.name, value: d.value })),
        },
      ],
    };
  }, [data, tokens, valueFormatter, centerLabel, centerValue]);

  return <EChart option={option} height={height} notMerge />;
}
