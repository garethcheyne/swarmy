import { useEffect, useRef, useState } from "react"

export type TorrentState =
  | "initializing"
  | "downloading"
  | "seeding"
  | "paused"
  | "error"

export interface Torrent {
  hash: string
  name: string
  state: TorrentState
  progress: number
  size: number
  downloaded: number
  uploaded: number
  ratio: number
  download_speed: number
  upload_speed: number
  eta: number | null
  peers: number
  seeds: number
  save_path: string
  category: string
  tags: string[]
  added_on: number
  completed_on: number | null
  has_metadata: boolean
  /** Paused by the queue, waiting for a free slot. */
  queued: boolean
  error: string | null
}

export interface SystemSample {
  cpu: number
  mem: number
  disk_free: number
  disk_total: number
}

export interface GlobalStats {
  alt_speed: boolean
  download_speed: number
  upload_speed: number
  torrents: number
  downloading: number
  seeding: number
  peers: number
  session_downloaded: number
  session_uploaded: number
  system: SystemSample
}

export interface LiveSample {
  t: number
  dl: number
  ul: number
  peers: number
  active: number
  cpu: number
  mem: number
}

export interface Bucket {
  t: number
  dl_bytes: number
  ul_bytes: number
  dl_avg: number
  ul_avg: number
  dl_peak: number
  ul_peak: number
  peers: number
  active: number
  cpu: number
  mem: number
}

export type HistoryRange = "1h" | "24h" | "7d" | "30d" | "1y"

export interface History {
  range: HistoryRange
  bucket_secs: number
  from: number
  to: number
  buckets: Bucket[]
}

export interface Day {
  day: number
  dl_bytes: number
  ul_bytes: number
}

export interface Totals {
  dl_bytes: number
  ul_bytes: number
}

export interface Summary {
  session: Totals
  today: Totals
  all_time: Totals
  recording_since: number | null
  uptime_secs: number
  peak_dl: number
  peak_ul: number
  completed_total: number
  completed_7d: number
  added_7d: number
  busiest_day: Day | null
  daily: Day[]
  hourly_profile: number[]
  system: SystemSample
}

export interface TorrentEvent {
  t: number
  kind: "added" | "completed" | "removed" | "errored"
  hash: string
  name: string
  size: number
  category: string
}

export interface ServerInfo {
  name: string
  version: string
  demo: boolean
  download_dir: string
  network: { listen_port: number | null; dht_nodes: number | null; bind_interface: string | null }
}

export type SeedAction = "pause" | "remove" | "remove_with_files"

export interface Settings {
  downloads: { default_save_path: string; add_paused: boolean }
  connection: {
    listen_port: number
    upnp: boolean
    bind_interface: string
    ipv4_only: boolean
    dht: boolean
    lsd: boolean
    peer_limit: number
    socks_proxy: string
    blocklist_url: string
  }
  speed: {
    download_limit: number
    upload_limit: number
    alt_download_limit: number
    alt_upload_limit: number
    alt_enabled: boolean
    schedule: { enabled: boolean; from: number; to: number; days: number[] }
  }
  queue: { enabled: boolean; max_downloads: number; max_uploads: number; max_active: number }
  seeding: {
    ratio_limit: number | null
    time_limit: number | null
    inactive_time_limit: number | null
    action: SeedAction
  }
  webui: {
    session_days: number
    max_attempts: number
    lockout_minutes: number
    trusted_proxies: string[]
    qbit_api: boolean
    api_key_hash: string
  }
}

export interface SettingsResponse {
  settings: Settings
  locked: string[]
  restart_needed: boolean
  sessions: number
  effective_download_dir: string
  data_dir: string
  api_key_set: boolean
  qbit_version: string
  qbit_webapi: string
}

export interface PieceMap {
  total: number
  have: number
  buckets: number[]
}

export interface NotifyPrefs {
  completed: boolean
  errored: boolean
  added: boolean
}

export interface PushInfo {
  public_key: string
  prefs: NotifyPrefs
  devices: number
}

export interface Snapshot {
  torrents: Torrent[]
  stats: GlobalStats
}

export interface TorrentFile {
  index: number
  path: string
  size: number
  downloaded_bytes: number
  selected: boolean
}

export interface Category {
  save_path: string
}

export class UnauthorizedError extends Error {}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, { credentials: "same-origin", ...init })
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    const message = body?.error ?? `${res.status} ${res.statusText}`
    if (res.status === 401) throw new UnauthorizedError(body?.error ?? "Not signed in")
    throw new Error(message)
  }
  if (res.status === 204) return undefined as T
  return res.json() as Promise<T>
}

const json = (body: unknown): RequestInit => ({
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
})

export const api = {
  me: () => request<{ username: string | null }>("/auth/me"),
  login: (username: string, password: string) =>
    request<void>("/auth/login", json({ username, password })),
  logout: () => request<void>("/auth/logout", { method: "POST" }),

  add: (form: FormData) =>
    request<string[]>("/torrents", { method: "POST", body: form }),
  pause: (hashes: string[]) => request<void>("/torrents/pause", json({ hashes })),
  resume: (hashes: string[]) =>
    request<void>("/torrents/resume", json({ hashes })),
  remove: (hashes: string[], deleteFiles: boolean) =>
    request<void>(
      "/torrents/delete",
      json({ hashes, delete_files: deleteFiles })
    ),
  files: (hash: string) => request<TorrentFile[]>(`/torrents/${hash}/files`),
  setMeta: (hash: string, meta: { category?: string; tags?: string[] }) =>
    request<void>(`/torrents/${hash}/meta`, { ...json(meta), method: "PUT" }),

  info: () => request<ServerInfo>("/info"),
  settings: () => request<SettingsResponse>("/settings"),
  saveSettings: (s: Settings) =>
    request<{ restart_needed: boolean }>("/settings", { ...json(s), method: "PUT" }),
  setAltSpeed: (enabled: boolean) => request<void>("/speed/alt", json({ enabled })),
  tags: () => request<string[]>("/tags"),
  putTag: (name: string) => request<void>(`/tags/${encodeURIComponent(name)}`, { method: "PUT" }),
  deleteTag: (name: string) => request<void>(`/tags/${encodeURIComponent(name)}`, { method: "DELETE" }),
  changePassword: (body: { current_password: string; username?: string; new_password?: string }) =>
    request<{ signed_out: number }>("/auth/password", json(body)),
  createApiKey: () => request<{ api_key: string }>("/apikey", { method: "POST" }),
  deleteApiKey: () => request<void>("/apikey", { method: "DELETE" }),
  logoutOthers: () => request<{ signed_out: number }>("/auth/logout-others", { method: "POST" }),
  pieces: (hash: string, buckets = 216) =>
    request<PieceMap>(`/torrents/${hash}/pieces?buckets=${buckets}`),
  pushInfo: () => request<PushInfo>("/push"),
  pushSubscribe: (sub: { endpoint: string; keys: { p256dh: string; auth: string }; label: string }) =>
    request<void>("/push/subscribe", json(sub)),
  pushUnsubscribe: (endpoint: string) => request<void>("/push/unsubscribe", json({ endpoint })),
  pushPrefs: (prefs: NotifyPrefs) => request<void>("/push/prefs", { ...json(prefs), method: "PUT" }),
  pushTest: (endpoint?: string) => request<{ delivered: number }>("/push/test", json({ endpoint })),
  live: () => request<LiveSample[]>("/stats/live"),
  history: (range: HistoryRange) =>
    request<History>(`/stats/history?range=${range}`),
  summary: () =>
    request<Summary>(`/stats/summary?tz=${-new Date().getTimezoneOffset()}`),
  events: (limit = 30) => request<TorrentEvent[]>(`/stats/events?limit=${limit}`),

  categories: () => request<Record<string, Category>>("/categories"),
  putCategory: (name: string, cat: Category) =>
    request<void>(`/categories/${encodeURIComponent(name)}`, {
      ...json(cat),
      method: "PUT",
    }),
  deleteCategory: (name: string) =>
    request<void>(`/categories/${encodeURIComponent(name)}`, {
      method: "DELETE",
    }),
}

export type ConnectionState = "connecting" | "open" | "closed"

const LIVE_WINDOW = 600
const SPARK_WINDOW = 60

export interface LiveData {
  snapshot: Snapshot | null
  connection: ConnectionState
  /** Global samples, oldest first, up to 10 minutes. */
  history: LiveSample[]
  /** Last 60 download+upload speeds per torrent, oldest first. */
  sparks: Map<string, number[]>
}

/**
 * Live snapshot pushed by the daemon over WebSocket (auto-reconnecting),
 * plus rolling histories built from it for live charts and sparklines.
 */
export function useLive(onUnauthorized: () => void): LiveData {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null)
  const [connection, setConnection] = useState<ConnectionState>("connecting")
  const [history, setHistory] = useState<LiveSample[]>([])
  const sparks = useRef(new Map<string, number[]>())

  useEffect(() => {
    let ws: WebSocket | null = null
    let retry: ReturnType<typeof setTimeout> | undefined
    let attempt = 0
    let disposed = false

    // Prefill the live chart with what the server already has.
    api
      .live()
      .then((samples) => !disposed && setHistory(samples))
      .catch(() => {})

    const onSnapshot = (snap: Snapshot) => {
      const now = Math.floor(Date.now() / 1000)
      const s = snap.stats
      setHistory((prev) => {
        const next = prev.length >= LIVE_WINDOW ? prev.slice(1) : prev.slice()
        next.push({
          t: now,
          dl: s.download_speed,
          ul: s.upload_speed,
          peers: s.peers,
          active: snap.torrents.filter((t) => t.download_speed > 0 || t.upload_speed > 0).length,
          cpu: s.system.cpu,
          mem: s.system.mem,
        })
        return next
      })
      const map = sparks.current
      const live = new Set<string>()
      for (const t of snap.torrents) {
        live.add(t.hash)
        const arr = map.get(t.hash) ?? []
        arr.push(t.download_speed + t.upload_speed)
        if (arr.length > SPARK_WINDOW) arr.shift()
        map.set(t.hash, arr)
      }
      for (const h of map.keys()) if (!live.has(h)) map.delete(h)
      setSnapshot(snap)
    }

    const connect = () => {
      const proto = location.protocol === "https:" ? "wss" : "ws"
      ws = new WebSocket(`${proto}://${location.host}/api/ws`)
      setConnection("connecting")
      ws.onopen = () => {
        attempt = 0
        setConnection("open")
      }
      ws.onmessage = (e) => onSnapshot(JSON.parse(e.data) as Snapshot)
      ws.onclose = async () => {
        if (disposed) return
        setConnection("closed")
        // A rejected upgrade looks like a plain close; check whether the session expired.
        try {
          await api.me()
        } catch (err) {
          if (err instanceof UnauthorizedError) return onUnauthorized()
        }
        attempt += 1
        retry = setTimeout(connect, Math.min(1000 * 2 ** attempt, 15000))
      }
    }

    connect()
    return () => {
      disposed = true
      clearTimeout(retry)
      ws?.close()
    }
  }, [onUnauthorized])

  // eslint-disable-next-line react-hooks/refs -- read on every snapshot render, intentionally
  return { snapshot, connection, history, sparks: sparks.current }
}
