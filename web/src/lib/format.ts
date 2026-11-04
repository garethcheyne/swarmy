const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]

export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B"
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1)
  const v = bytes / 1024 ** i
  return `${v.toFixed(i === 0 ? 0 : v >= 100 ? 0 : digits)} ${UNITS[i]}`
}

export function formatSpeed(bps: number): string {
  return bps > 0 ? `${formatBytes(bps)}/s` : "—"
}

export function formatEta(secs: number | null): string {
  if (secs == null || !Number.isFinite(secs)) return "—"
  if (secs < 60) return `${secs}s`
  const m = Math.floor(secs / 60)
  if (m < 60) return `${m}m ${secs % 60}s`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ${m % 60}m`
  const d = Math.floor(h / 24)
  return d > 99 ? "∞" : `${d}d ${h % 24}h`
}

export function formatPercent(p: number): string {
  if (p >= 1) return "100%"
  return `${(Math.floor(p * 1000) / 10).toFixed(1)}%`
}

const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" })

export function formatRelative(unixSecs: number): string {
  const diff = unixSecs - Date.now() / 1000
  const abs = Math.abs(diff)
  if (abs < 60) return rtf.format(Math.round(diff), "second")
  if (abs < 3600) return rtf.format(Math.round(diff / 60), "minute")
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), "hour")
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), "day")
  return new Date(unixSecs * 1000).toLocaleDateString()
}

export function formatDate(unixSecs: number | null): string {
  if (!unixSecs) return "—"
  return new Date(unixSecs * 1000).toLocaleString()
}

/** Split "12.3 MiB/s" style values so the number and unit can be set differently. */
export function splitUnit(formatted: string): [string, string] {
  const i = formatted.indexOf(" ")
  return i < 0 ? [formatted, ""] : [formatted.slice(0, i), formatted.slice(i + 1)]
}

export function formatClock(unixSecs: number, withSeconds = false): string {
  return new Date(unixSecs * 1000).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    ...(withSeconds ? { second: "2-digit" } : {}),
  })
}

export function formatDay(unixSecs: number, long = false): string {
  return new Date(unixSecs * 1000).toLocaleDateString(undefined, {
    weekday: long ? "long" : undefined,
    day: "numeric",
    month: "short",
  })
}

export function formatDuration(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (d > 0) return `${d}d ${h}h`
  if (h > 0) return `${h}h ${m}m`
  return `${m}m`
}

export function formatRatio(up: number, down: number): string {
  if (down <= 0) return "—"
  return (up / down).toFixed(2)
}
