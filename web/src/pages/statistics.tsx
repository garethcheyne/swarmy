import { useMemo, useState, type ReactNode } from "react"

import { api, type HistoryRange } from "@/lib/api"
import {
  formatBytes,
  formatClock,
  formatDate,
  formatDay,
  formatDuration,
  formatRatio,
} from "@/lib/format"
import { usePoll } from "@/lib/use-poll"
import {
  CountUp,
  HourProfileChart,
  Legend,
  MiniChart,
  ThroughputChart,
  VolumeChart,
  speed,
} from "@/components/charts"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const RANGES: { value: HistoryRange; label: string }[] = [
  { value: "1h", label: "Hour" },
  { value: "24h", label: "Day" },
  { value: "7d", label: "Week" },
  { value: "30d", label: "Month" },
  { value: "1y", label: "Year" },
]

const RANGE_NOUN: Record<HistoryRange, string> = {
  "1h": "the last hour",
  "24h": "the last 24 hours",
  "7d": "the last 7 days",
  "30d": "the last 30 days",
  "1y": "the last year",
}

export function StatisticsPage() {
  const [range, setRange] = useState<HistoryRange>("24h")
  const { data: history } = usePoll(() => api.history(range), 30000, range)
  const { data: summary } = usePoll(api.summary, 10000)

  const points = useMemo(
    () =>
      (history?.buckets ?? []).map((b) => ({
        t: b.t,
        dl: b.dl_avg,
        ul: b.ul_avg,
        peers: b.peers,
        active: b.active,
        cpu: b.cpu,
        mem: b.mem,
        dl_bytes: b.dl_bytes,
        ul_bytes: b.ul_bytes,
      })),
    [history]
  )

  const span = history?.bucket_secs ?? 60
  const short = range === "1h" || range === "24h"
  const xFormat = (t: number) => (short ? formatClock(t) : formatDay(t))
  const tipTitle = (t: number) =>
    span >= 86400
      ? formatDay(t, true)
      : span >= 3600
        ? `${formatDay(t)}, ${formatClock(t)}`
        : formatClock(t)

  const totals = points.reduce(
    (a, p) => ({ dl: a.dl + p.dl_bytes, ul: a.ul + p.ul_bytes }),
    { dl: 0, ul: 0 }
  )
  const peak = points.reduce((m, p) => Math.max(m, p.dl), 0)
  const last = points.at(-1)

  return (
    <div className="flex flex-col gap-8 p-4 pb-8 lg:p-6">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-xl font-semibold">Statistics</h1>
          <p className="text-sm text-muted-foreground">
            {summary?.recording_since
              ? `Recording since ${formatDate(summary.recording_since)}, up for ${formatDuration(summary.uptime_secs)}`
              : "Recording starts as soon as anything transfers."}
          </p>
        </div>
        <ToggleGroup
          variant="outline"
          value={[range]}
          onValueChange={(v: string[]) => v[0] && setRange(v[0] as HistoryRange)}
          aria-label="Time range"
        >
          {RANGES.map((r) => (
            <ToggleGroupItem key={r.value} value={r.value}>
              {r.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </header>

      {summary && (
        <dl className="grid grid-cols-2 gap-x-8 gap-y-6 border-y py-6 md:grid-cols-3 xl:grid-cols-6">
          <Figure
            label="Downloaded, all time"
            value={<CountUp value={summary.all_time.dl_bytes} format={formatBytes} />}
          />
          <Figure
            label="Uploaded, all time"
            value={<CountUp value={summary.all_time.ul_bytes} format={formatBytes} delay={80} />}
          />
          <Figure
            label="Share ratio"
            value={
              summary.all_time.dl_bytes > 0 ? (
                <CountUp
                  value={summary.all_time.ul_bytes / summary.all_time.dl_bytes}
                  format={(v) => formatRatio(v, 1)}
                  delay={160}
                />
              ) : (
                formatRatio(summary.all_time.ul_bytes, summary.all_time.dl_bytes)
              )
            }
          />
          <Figure label="Fastest download" value={<CountUp value={summary.peak_dl} format={speed} delay={240} />} />
          <Figure
            label="Finished"
            value={<CountUp value={summary.completed_total} format={(v) => Math.round(v).toString()} delay={320} />}
            detail={`${summary.completed_7d} this week`}
          />
          <Figure
            label="Busiest day"
            value={
              summary.busiest_day ? (
                <CountUp value={summary.busiest_day.dl_bytes} format={formatBytes} delay={400} />
              ) : (
                "—"
              )
            }
            detail={summary.busiest_day ? formatDay(summary.busiest_day.day, true) : undefined}
          />
        </dl>
      )}

      <section className="flex flex-col gap-4">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <div className="flex flex-col gap-0.5">
            <h2 className="text-[15px] font-semibold">Throughput</h2>
            <p className="text-sm text-muted-foreground numeric">
              {formatBytes(totals.dl)} down and {formatBytes(totals.ul)} up over{" "}
              {RANGE_NOUN[range]}, peaking at {speed(peak)} on average
            </p>
          </div>
          <Legend
            items={[
              { label: "Download", color: "var(--down)" },
              { label: "Upload", color: "var(--up)" },
            ]}
          />
        </div>
        {history ? (
          <ThroughputChart key={range} data={points} xFormat={xFormat} tipTitle={tipTitle} className="h-72" />
        ) : (
          <Skeleton className="h-72" />
        )}
      </section>

      <section className="flex flex-col gap-4">
        <div className="flex flex-col gap-0.5">
          <h2 className="text-[15px] font-semibold">How hard it's working</h2>
          <p className="text-sm text-muted-foreground">
            Averages per {span >= 86400 ? "day" : span >= 3600 ? `${span / 3600} hours` : `${span / 60} minutes`}{" "}
            over {RANGE_NOUN[range]}.
          </p>
        </div>
        <div className="grid gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-4">
          <MiniChart
            key={range}
            title="Peers connected"
            current={last ? Math.round(last.peers).toString() : "—"}
            data={points}
            dataKey="peers"
            format={(v) => Math.round(v).toString()}
            tipTitle={tipTitle}
          />
          <MiniChart
            key={range}
            title="Active torrents"
            current={last ? last.active.toFixed(1) : "—"}
            data={points}
            dataKey="active"
            format={(v) => v.toFixed(1)}
            tipTitle={tipTitle}
          />
          <MiniChart
            key={range}
            title="Swarmy CPU"
            current={last ? `${last.cpu.toFixed(1)}%` : "—"}
            data={points}
            dataKey="cpu"
            format={(v) => `${v.toFixed(1)}%`}
            tipTitle={tipTitle}
          />
          <MiniChart
            key={range}
            title="Swarmy memory"
            current={last ? formatBytes(last.mem) : "—"}
            data={points}
            dataKey="mem"
            format={(v) => formatBytes(v, 0)}
            tipTitle={tipTitle}
          />
        </div>
      </section>

      <section className="grid gap-10 xl:grid-cols-[3fr_2fr]">
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <h2 className="text-[15px] font-semibold">Last 30 days</h2>
            <Legend
              items={[
                { label: "Downloaded", color: "var(--down)" },
                { label: "Uploaded", color: "var(--up)" },
              ]}
            />
          </div>
          {summary ? (
            <VolumeChart
              data={summary.daily.map((d) => ({ t: d.day, dl: d.dl_bytes, ul: d.ul_bytes }))}
              xFormat={(t) => formatDay(t)}
              tipTitle={(t) => formatDay(t, true)}
              className="h-64"
            />
          ) : (
            <Skeleton className="h-64" />
          )}
        </div>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-0.5">
            <h2 className="text-[15px] font-semibold">Time of day</h2>
            <p className="text-sm text-muted-foreground">
              Average download rate by hour, last 30 days. The brighter bar is the hour you're in now.
            </p>
          </div>
          {summary ? (
            <HourProfileChart values={summary.hourly_profile} className="h-56" />
          ) : (
            <Skeleton className="h-56" />
          )}
        </div>
      </section>
    </div>
  )
}

function Figure({ label, value, detail }: { label: string; value: ReactNode; detail?: string }) {
  return (
    <div className="flex flex-col gap-1">
      <dt className="text-sm text-muted-foreground">{label}</dt>
      <dd className="data text-xl font-medium">{value}</dd>
      {detail && <dd className="text-xs text-muted-foreground">{detail}</dd>}
    </div>
  )
}
