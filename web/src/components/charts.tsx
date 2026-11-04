import { useEffect, useId, useRef, useState, type ReactNode } from "react"
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Rectangle,
  Tooltip,
  XAxis,
  YAxis,
  type RectangleProps,
} from "recharts"

import { cn } from "@/lib/utils"
import { formatBytes } from "@/lib/format"
import { useMediaQuery } from "@/hooks/use-media"
import { ChartContainer, type ChartConfig } from "@/components/ui/chart"

// ---------------------------------------------------------------- motion

export function useReducedMotion() {
  return useMediaQuery("(prefers-reduced-motion: reduce)")
}

/** True for the first `ms` after mount: entrance animations only play then. */
function useEntrance(ms: number) {
  const reduced = useReducedMotion()
  const [entering, setEntering] = useState(true)
  useEffect(() => {
    const id = setTimeout(() => setEntering(false), ms)
    return () => clearTimeout(id)
  }, [ms])
  return entering && !reduced
}

/**
 * Tween a number towards `target` (ease-out cubic): counts up from zero on
 * mount and glides from the current value when the target changes.
 */
export function useTween(target: number, ms = 1100, delay = 0) {
  const reduced = useReducedMotion()
  const [value, setValue] = useState(0)
  const current = useRef(0)
  useEffect(() => {
    if (reduced) return
    const from = current.current
    let raf = 0
    let start = 0
    const tick = (t: number) => {
      if (!start) start = t + delay
      const k = Math.min(1, Math.max(0, (t - start) / ms))
      const v = from + (target - from) * (1 - (1 - k) ** 3)
      current.current = v
      setValue(v)
      if (k < 1) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [target, ms, delay, reduced])
  return reduced ? target : value
}

/** A number that counts up and glides, formatted on every frame. */
export function CountUp({
  value,
  format,
  delay = 0,
}: {
  value: number
  format: (v: number) => string
  delay?: number
}) {
  const v = useTween(value, 1100, delay)
  return <>{format(v)}</>
}

/** "Now": the newest point, with a ring that pings outwards. */
function LiveDot({
  cx,
  cy,
  color,
}: {
  cx?: number
  cy?: number
  color: string
}) {
  if (cx == null || cy == null) return null
  return (
    <g>
      <circle
        cx={cx}
        cy={cy}
        r={4}
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        className="live-ping"
      />
      <circle
        cx={cx}
        cy={cy}
        r={3.5}
        fill={color}
        stroke="var(--card)"
        strokeWidth={1.5}
      />
    </g>
  )
}

type BarShape = RectangleProps & { index?: number }

/**
 * A bar that grows out of the baseline with a little overshoot. Bars enter
 * in a wave (`stagger` ms apart), only while the chart is first appearing.
 */
function growingBar(
  entering: boolean,
  stagger: number,
  offset = 0,
  extra?: (i: number) => RectangleProps
) {
  function GrowingBar(props: unknown) {
    const p = props as BarShape
    const i = p.index ?? 0
    return (
      <g
        className={entering ? "bar-grow" : undefined}
        style={
          entering ? { animationDelay: `${offset + i * stagger}ms` } : undefined
        }
      >
        <Rectangle {...p} {...extra?.(i)} />
      </g>
    )
  }
  return GrowingBar
}

/** Recharts calls `dot` for every point; draw only the newest. */
function lastDot(last: number, render: (cx: number, cy: number) => ReactNode) {
  function Dot(props: unknown) {
    const p = props as { cx?: number; cy?: number; index?: number }
    if (p.index !== last || p.cx == null || p.cy == null)
      return <g key={p.index} />
    return <g key="last">{render(p.cx, p.cy)}</g>
  }
  return Dot
}

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
  payload?: {
    dataKey?: unknown
    value?: unknown
    payload?: Record<string, number>
  }[]
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
          <span className="numeric ml-auto font-medium text-foreground">
            {r.value}
          </span>
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
    <ul
      className={cn(
        "flex flex-wrap items-center gap-x-4 gap-y-1 text-xs",
        className
      )}
    >
      {items.map((i) => (
        <li key={i.label} className="flex items-center gap-1.5">
          <span
            className="size-2.5 rounded-[3px]"
            style={{ background: i.color }}
          />
          <span className="text-muted-foreground">{i.label}</span>
          {i.value && <span className="numeric font-medium">{i.value}</span>}
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
  const reduced = useReducedMotion()
  const id = useId().replace(/:/g, "")
  const last = data.length - 1
  // The "now" dots appear once the lines have drawn in.
  const showDots = !useEntrance(1500)
  return (
    <ChartContainer
      config={throughputConfig}
      className={cn("aspect-auto", className)}
    >
      <AreaChart data={data} margin={{ top: 8, right: 10, bottom: 0, left: 4 }}>
        <defs>
          <linearGradient id={`${id}-dl`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--color-dl)" stopOpacity={0.34} />
            <stop offset="60%" stopColor="var(--color-dl)" stopOpacity={0.1} />
            <stop
              offset="100%"
              stopColor="var(--color-dl)"
              stopOpacity={0.01}
            />
          </linearGradient>
          <linearGradient id={`${id}-ul`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--color-ul)" stopOpacity={0.12} />
            <stop offset="100%" stopColor="var(--color-ul)" stopOpacity={0} />
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
          dataKey="ul"
          type="monotone"
          className="glow-ul"
          stroke="var(--color-ul)"
          strokeWidth={2}
          fill={`url(#${id}-ul)`}
          isAnimationActive={!reduced}
          animationBegin={180}
          animationDuration={1300}
          animationEasing="ease-out"
          dot={
            showDots &&
            lastDot(last, (cx, cy) => (
              <LiveDot cx={cx} cy={cy} color="var(--color-ul)" />
            ))
          }
          activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--card)" }}
        />
        <Area
          dataKey="dl"
          type="monotone"
          className="glow-dl"
          stroke="var(--color-dl)"
          strokeWidth={2}
          fill={`url(#${id}-dl)`}
          isAnimationActive={!reduced}
          animationDuration={1300}
          animationEasing="ease-out"
          dot={
            showDots &&
            lastDot(last, (cx, cy) => (
              <LiveDot cx={cx} cy={cy} color="var(--color-dl)" />
            ))
          }
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
  const entering = useEntrance(2400)
  return (
    <ChartContainer
      config={volumeConfig}
      className={cn("aspect-auto", className)}
    >
      <BarChart
        data={data}
        barGap={2}
        barCategoryGap="22%"
        margin={{ top: 8, right: 4, left: 4 }}
      >
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
        <Bar
          dataKey="dl"
          fill="var(--color-dl)"
          radius={[4, 4, 0, 0]}
          isAnimationActive={false}
          shape={growingBar(entering, 34)}
        />
        <Bar
          dataKey="ul"
          fill="var(--color-ul)"
          radius={[4, 4, 0, 0]}
          isAnimationActive={false}
          shape={growingBar(entering, 34, 90)}
        />
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
  const reduced = useReducedMotion()
  const id = useId().replace(/:/g, "")
  const drawing = useEntrance(1300)
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="text-sm font-semibold">{title}</h3>
        <span className="numeric text-sm text-muted-foreground">{current}</span>
      </div>
      <ChartContainer config={config} className="aspect-auto h-28">
        <AreaChart
          data={data}
          margin={{ top: 6, right: 6, bottom: 0, left: 4 }}
        >
          <defs>
            <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
              <stop
                offset="0%"
                stopColor={`var(--color-${dataKey})`}
                stopOpacity={0.24}
              />
              <stop
                offset="100%"
                stopColor={`var(--color-${dataKey})`}
                stopOpacity={0}
              />
            </linearGradient>
          </defs>
          <CartesianGrid vertical={false} strokeDasharray="2 4" />
          <XAxis
            dataKey="t"
            type="number"
            domain={["dataMin", "dataMax"]}
            hide
          />
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
                rows={(r) => [
                  { key: dataKey, label: title, value: format(r[dataKey]) },
                ]}
              />
            }
          />
          <Area
            dataKey={dataKey}
            type="monotone"
            stroke={`var(--color-${dataKey})`}
            strokeWidth={1.5}
            fill={`url(#${id})`}
            isAnimationActive={!reduced}
            animationDuration={1100}
            animationEasing="ease-out"
            dot={
              !drawing &&
              lastDot(data.length - 1, (cx, cy) => (
                <circle
                  cx={cx}
                  cy={cy}
                  r={2.5}
                  fill={`var(--color-${dataKey})`}
                />
              ))
            }
            activeDot={{ r: 4, strokeWidth: 2, stroke: "var(--card)" }}
          />
        </AreaChart>
      </ChartContainer>
    </div>
  )
}

/** Average download rate by hour of day — single series, honey. */
export function HourProfileChart({
  values,
  className,
}: {
  values: number[]
  className?: string
}) {
  const entering = useEntrance(2200)
  const [now] = useState(() => new Date().getHours())
  const data = values.map((v, h) => ({ h, v }))
  const peak = data.reduce(
    (a, b) => (b.v > a.v ? b : a),
    data[0] ?? { h: 0, v: 0 }
  )
  const config = {
    v: { label: "Average download", color: "var(--down)" },
  } satisfies ChartConfig
  const hour = (h: number) =>
    new Date(2000, 0, 1, h).toLocaleTimeString(undefined, { hour: "numeric" })
  return (
    <ChartContainer config={config} className={cn("aspect-auto", className)}>
      <BarChart
        data={data}
        barCategoryGap="18%"
        margin={{ top: 18, right: 4, left: 4 }}
      >
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
              title={(r) =>
                `${hour(r.h)}–${hour((r.h + 1) % 24)}${r.h === now ? ", now" : ""}`
              }
              rows={(r) => [
                { key: "v", label: "Average download", value: speed(r.v) },
              ]}
            />
          }
        />
        <Bar
          dataKey="v"
          radius={[4, 4, 0, 0]}
          isAnimationActive={false}
          fill="var(--color-v)"
          shape={growingBar(entering, 30, 0, (i) => ({
            fillOpacity: i === now ? 1 : 0.6,
          }))}
          label={
            !entering && {
              position: "top",
              fontSize: 11,
              fill: "var(--muted-foreground)",
              formatter: (v: unknown) =>
                v === peak.v && peak.v > 0 ? speed(peak.v) : "",
            }
          }
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
  if (values.length < 2)
    return <svg className={className} width={w} height={h} aria-hidden />
  const max = Math.max(...values, 1)
  const step = w / (values.length - 1)
  const pts = values.map(
    (v, i) =>
      `${(i * step).toFixed(1)},${(h - 2 - (v / max) * (h - 4)).toFixed(1)}`
  )
  return (
    <svg
      className={className}
      width={w}
      height={h}
      viewBox={`0 0 ${w} ${h}`}
      aria-hidden
    >
      <polyline
        points={pts.join(" ")}
        pathLength={1}
        className="draw-in"
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}

/**
 * The live throughput strip (sidebar, mini view): download as a shaded area,
 * upload as a line, and a pinging dot on "now". Draws itself in on mount.
 */
export function LiveSpark({
  points,
  width,
  height,
  stretch = false,
  className,
}: {
  points: { dl: number; ul: number }[]
  width: number
  height: number
  /** Fill the box at any aspect ratio (then the end dots are left out). */
  stretch?: boolean
  className?: string
}) {
  const id = useId().replace(/:/g, "")
  const max = Math.max(1, ...points.map((p) => Math.max(p.dl, p.ul)))
  const right = stretch ? width : width - 5
  const x = (i: number) => (i / Math.max(1, points.length - 1)) * right
  const y = (v: number) => height - 1.5 - (v / max) * (height - 6)
  const line = (k: "dl" | "ul") =>
    points.map((p, i) => `${x(i).toFixed(1)},${y(p[k]).toFixed(1)}`).join(" ")
  const dl = line("dl")
  const end = points.at(-1)
  const n = points.length - 1
  const stroke = stretch ? { vectorEffect: "non-scaling-stroke" as const } : {}
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio={stretch ? "none" : undefined}
      className={className}
      aria-hidden
    >
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="var(--down)" stopOpacity={0.32} />
          <stop offset="100%" stopColor="var(--down)" stopOpacity={0} />
        </linearGradient>
      </defs>
      {points.length > 1 && end && (
        <>
          <polygon
            points={`0,${height} ${dl} ${x(n).toFixed(1)},${height}`}
            fill={`url(#${id})`}
            className="fade-in"
          />
          <polyline
            points={line("ul")}
            pathLength={1}
            className="draw-in"
            fill="none"
            stroke="var(--up)"
            strokeWidth="1.75"
            strokeLinejoin="round"
            {...stroke}
          />
          <polyline
            points={dl}
            pathLength={1}
            className="draw-in"
            fill="none"
            stroke="var(--down)"
            strokeWidth="1.75"
            strokeLinejoin="round"
            {...stroke}
          />
          {!stretch && (
            <>
              <circle
                cx={x(n)}
                cy={y(end.ul)}
                r={2.25}
                fill="var(--up)"
                className="fade-in"
              />
              <LiveDot cx={x(n)} cy={y(end.dl)} color="var(--down)" />
            </>
          )}
        </>
      )}
    </svg>
  )
}
