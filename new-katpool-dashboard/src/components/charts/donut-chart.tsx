"use client";

import { useMemo } from "react";
import type { EChartsCoreOption } from "echarts/core";
import { EChart } from "./echart";
import { useChartTokens } from "./use-tokens";

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
    return {
      animationDuration: 600,
      color: tokens.series,
      tooltip: {
        trigger: "item",
        backgroundColor: tokens.tooltipBg,
        borderColor: tokens.border,
        textStyle: { color: tokens.text, fontSize: 12 },
        valueFormatter: (v: unknown) => valueFormatter(Number(v)),
      },
      legend: {
        type: "scroll",
        orient: "vertical",
        right: 8,
        top: "middle",
        textStyle: { color: tokens.muted, fontSize: 12 },
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
          radius: ["58%", "82%"],
          center: ["32%", "50%"],
          avoidLabelOverlap: true,
          itemStyle: { borderColor: tokens.tooltipBg, borderWidth: 2, borderRadius: 6 },
          label: { show: false },
          emphasis: { scaleSize: 6 },
          data: data.map((d) => ({ name: d.name, value: d.value })),
        },
      ],
    };
  }, [data, tokens, valueFormatter, centerLabel, centerValue]);

  return <EChart option={option} height={height} notMerge />;
}
