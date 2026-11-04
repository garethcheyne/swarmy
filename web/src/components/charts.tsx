import type { ReactNode } from "react"
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Line,
  LineChart,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts"

import { cn } from "@/lib/utils"
import { formatBytes } from "@/lib/format"
import { ChartContainer, type ChartConfig } from "@/components/ui/chart"

export const speed = (v: number) => (v > 0 ? `${formatBytes(v)}/s` : "0")
/** Shorter rate labels for axes, so they never wrap. */
const axisSpeed = (v: number) => (v > 0 ? `${formatBytes(v, 0)}/s` : "0")

const throughputConfig = {
  dl: { label: "Download", color: "var(--down)" },
  ul: { label: "Upload", color: "var(--up)" },
} satisfies ChartConfig

interface TipRow {
  key: string
  label: string
  value: string
}

interface TipProps {
  active?: boolean
  payload?: { dataKey?: unknown; value?: unknown; payload?: Record<string, number> }[]
  title: (row: Record<string, number>) => ReactNode
  rows: (row: Record<string, number>) => TipRow[]
}

/** One tooltip for every chart: a title, then a swatch + label + value per series. */
function ChartTip({ active, payload, title, rows }: TipProps) {
  const row = payload?.[0]?.payload
  if (!active || !row) return null
  return (
    <div className="grid min-w-40 gap-1.5 rounded-lg border bg-popover px-3 py-2 text-xs shadow-lg">
      <div className="font-medium text-foreground">{title(row)}</div>
      {rows(row).map((r) => (
        <div key={r.key} className="flex items-center gap-2">
          <span
            className="size-2.5 shrink-0 rounded-[3px]"
            style={{ background: `var(--color-${r.key})` }}
          />
          <span className="text-muted-foreground">{r.label}</span>
          <span className="ml-auto font-medium text-foreground numeric">{r.value}</span>
        </div>
      ))}
    </div>
  )
}

export function Legend({
  items,
  className,
}: {
  items: { label: string; color: string; value?: string }[]
  className?: string
}) {
  return (
    <ul className={cn("flex flex-wrap items-center gap-x-4 gap-y-1 text-xs", className)}>
      {items.map((i) => (
        <li key={i.label} className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-[3px]" style={{ background: i.color }} />
          <span className="text-muted-foreground">{i.label}</span>
          {i.value && <span className="font-medium numeric">{i.value}</span>}
        </li>
      ))}
    </ul>
  )
}

interface ThroughputPoint {
  t: number
  dl: number
  ul: number
}

/** Download and upload rate over time, same unit, one axis. */
export function ThroughputChart({
  data,
  xFormat,
  tipTitle,
  className,
}: {
  data: ThroughputPoint[]
  xFormat: (t: number) => string
  tipTitle: (t: number) => string
  className?: string
}) {
  return (
    <ChartContainer config={throughputConfig} className={cn("aspect-auto", className)}>
      <AreaChart data={data} margin={{ top: 8, right: 4, bottom: 0, left: 4 }}>
        <defs>
          <linearGradient id="fill-dl" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--color-dl)" stopOpacity={0.28} />
            <stop offset="100%" stopColor="var(--color-dl)" stopOpacity={0.02} />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} strokeDasharray="2 4" />
        <XAxis
          dataKey="t"
          type="number"
          domain={["dataMin", "dataMax"]}
          tickFormatter={xFormat}
          tickLine={false}
          axisLine={false}
          minTickGap={48}
          tickMargin={8}
        />
        <YAxis
          tickFormatter={axisSpeed}
          tickLine={false}
          axisLine={false}
          width={80}
          tickCount={4}
        />
        <Tooltip
          cursor={{ strokeWidth: 1 }}
          isAnimationActive={false}
          content={
            <ChartTip
              title={(r) => tipTitle(r.t)}
              rows={(r) => [
                { key: "dl", label: "Download", value: speed(r.dl) },
                { key: "ul", label: "Upload", value: speed(r.ul) },
              ]}
            />
          }
        />
        <Area
          dataKey="dl"
          type="monotone"
          stroke="var(--color-dl)"
          strokeWidth={2}
          fill="url(#fill-dl)"
          isAnimationActive={false}
          dot={false}
          activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--card)" }}
        />
        <Area
          dataKey="ul"
          type="monotone"
          stroke="var(--color-ul)"
          strokeWidth={2}
          fill="transparent"
          isAnimationActive={false}
          dot={false}
          activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--card)" }}
        />
      </AreaChart>
    </ChartContainer>
  )
}

const volumeConfig = {
  dl: { label: "Downloaded", color: "var(--down)" },
  ul: { label: "Uploaded", color: "var(--up)" },
} satisfies ChartConfig

/** Bytes moved per bucket (e.g. per day), downloaded and uploaded side by side. */
export function VolumeChart({
  data,
  xFormat,
  tipTitle,
  className,
}: {
  data: { t: number; dl: number; ul: number }[]
  xFormat: (t: number) => string
  tipTitle: (t: number) => string
  className?: string
}) {
  return (
    <ChartContainer config={volumeConfig} className={cn("aspect-auto", className)}>
      <BarChart data={data} barGap={2} barCategoryGap="22%" margin={{ top: 8, right: 4, left: 4 }}>
        <CartesianGrid vertical={false} strokeDasharray="2 4" />
        <XAxis
          dataKey="t"
          tickFormatter={xFormat}
          tickLine={false}
          axisLine={false}
          minTickGap={24}
          tickMargin={8}
        />
        <YAxis
          tickFormatter={(v: number) => (v > 0 ? formatBytes(v, 0) : "0")}
          tickLine={false}
          axisLine={false}
          width={64}
          tickCount={4}
        />
        <Tooltip
          cursor={{ fillOpacity: 0.5 }}
          isAnimationActive={false}
          content={
            <ChartTip
              title={(r) => tipTitle(r.t)}
              rows={(r) => [
                { key: "dl", label: "Downloaded", value: formatBytes(r.dl) },
                { key: "ul", label: "Uploaded", value: formatBytes(r.ul) },
              ]}
            />
          }
        />
        <Bar dataKey="dl" fill="var(--color-dl)" radius={[4, 4, 0, 0]} isAnimationActive={false} />
        <Bar dataKey="ul" fill="var(--color-ul)" radius={[4, 4, 0, 0]} isAnimationActive={false} />
      </BarChart>
    </ChartContainer>
  )
}

/** A small single-series chart; the title names the series, so no legend. */
export function MiniChart({
  title,
  current,
  data,
  dataKey,
  format,
  tipTitle,
  color = "var(--muted-foreground)",
}: {
  title: string
  current: string
  data: Record<string, number>[]
  dataKey: string
  format: (v: number) => string
  tipTitle: (t: number) => string
  color?: string
}) {
  const config = { [dataKey]: { label: title, color } } satisfies ChartConfig
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="text-sm font-semibold">{title}</h3>
        <span className="text-sm numeric text-muted-foreground">{current}</span>
      </div>
      <ChartContainer config={config} className="aspect-auto h-28">
        <LineChart data={data} margin={{ top: 4, right: 4, bottom: 0, left: 4 }}>
          <CartesianGrid vertical={false} strokeDasharray="2 4" />
          <XAxis dataKey="t" type="number" domain={["dataMin", "dataMax"]} hide />
          <YAxis
            tickFormatter={format}
            tickLine={false}
            axisLine={false}
            width={56}
            tickCount={3}
          />
          <Tooltip
            cursor={{ strokeWidth: 1 }}
            isAnimationActive={false}
            content={
              <ChartTip
                title={(r) => tipTitle(r.t)}
                rows={(r) => [{ key: dataKey, label: title, value: format(r[dataKey]) }]}
              />
            }
          />
          <Line
            dataKey={dataKey}
            type="monotone"
            stroke={`var(--color-${dataKey})`}
            strokeWidth={1.5}
            dot={false}
            isAnimationActive={false}
            activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--card)" }}
          />
        </LineChart>
      </ChartContainer>
    </div>
  )
}

/** Average download rate by hour of day — single series, honey. */
export function HourProfileChart({ values, className }: { values: number[]; className?: string }) {
  const data = values.map((v, h) => ({ h, v }))
  const peak = data.reduce((a, b) => (b.v > a.v ? b : a), data[0] ?? { h: 0, v: 0 })
  const config = { v: { label: "Average download", color: "var(--down)" } } satisfies ChartConfig
  const hour = (h: number) =>
    new Date(2000, 0, 1, h).toLocaleTimeString(undefined, { hour: "numeric" })
  return (
    <ChartContainer config={config} className={cn("aspect-auto", className)}>
      <BarChart data={data} barCategoryGap="18%" margin={{ top: 18, right: 4, left: 4 }}>
        <XAxis
          dataKey="h"
          tickFormatter={hour}
          tickLine={false}
          axisLine={false}
          interval={2}
          tickMargin={8}
        />
        <YAxis hide />
        <Tooltip
          cursor={{ fillOpacity: 0.5 }}
          isAnimationActive={false}
          content={
            <ChartTip
              title={(r) => `${hour(r.h)}–${hour((r.h + 1) % 24)}`}
              rows={(r) => [{ key: "v", label: "Average download", value: speed(r.v) }]}
            />
          }
        />
        <Bar
          dataKey="v"
          radius={[4, 4, 0, 0]}
          isAnimationActive={false}
          fill="var(--color-v)"
          label={{
            position: "top",
            fontSize: 11,
            fill: "var(--muted-foreground)",
            formatter: (v: unknown) =>
              v === peak.v && peak.v > 0 ? speed(peak.v) : "",
          }}
        />
      </BarChart>
    </ChartContainer>
  )
}

/** Tiny inline SVG sparkline for table rows. */
export function Sparkline({
  values,
  className,
  color = "var(--down)",
}: {
  values: number[]
  className?: string
  color?: string
}) {
  const w = 80
  const h = 22
  if (values.length < 2) return <svg className={className} width={w} height={h} aria-hidden />
  const max = Math.max(...values, 1)
  const step = w / (values.length - 1)
  const pts = values.map((v, i) => `${(i * step).toFixed(1)},${(h - 2 - (v / max) * (h - 4)).toFixed(1)}`)
  return (
    <svg className={className} width={w} height={h} viewBox={`0 0 ${w} ${h}`} aria-hidden>
      <polyline
        points={pts.join(" ")}
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}
