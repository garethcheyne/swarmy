import { useEffect, useRef, useState } from "react"

export type TorrentState =
  | "initializing"
  | "downloading"
  | "seeding"
  | "paused"
  | "error"
  | "moving"

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
  /** While it downloads in the incomplete folder: the folder it's in there. */
  download_path: string
  /** Follows its category's folder, so a category change moves its files. */
  auto_managed: boolean
  /** Set while its files are moving, or waiting their turn to. */
  moving: { progress: number; queued: boolean } | null
  /** Why the last move failed. */
  move_error: string | null
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
  /** Why Swarmy can't write to the download folder, if it can't. */
  download_dir_problem: string | null
  network: { listen_port: number | null; dht_nodes: number | null; bind_interface: string | null }
  /** A newer Swarmy release, if there is one. */
  update_available: string | null
}

/** Settings → About: newer releases, and how to install them. */
export interface UpdateStatus {
  current: string
  latest: { version: string; url: string; published: string | null } | null
  /** Notes for every release newer than this one, newest first. */
  notes: Release[]
  checked_at: number | null
  error: string | null
  check: boolean
  /** "installer": one click from here. "docker": pull the image. "manual": run the installer. */
  method: "installer" | "docker" | "manual"
  /** An install was asked for and hasn't started yet. */
  pending: boolean
  /** ...and nothing has picked it up for a while. */
  stuck: boolean
  last_run: { at: number; running: boolean; ok: boolean; log: string } | null
}

export interface FolderListing {
  path: string
  parent: string | null
  folders: { name: string; path: string }[]
  /** Whether Swarmy can write here; `problem` says why not. */
  writable: boolean
  problem: string | null
  /** Shortcuts: the download folder, mounted shares, the top of the disk. */
  places: { label: string; path: string }[]
}

export interface ChangelogSection {
  title: string
  items: string[]
}

export interface Release {
  version: string
  /** ISO date, or null while unreleased. */
  date: string | null
  summary: string
  sections: ChangelogSection[]
}

export interface Changelog {
  version: string
  releases: Release[]
}

export type SeedAction = "pause" | "remove" | "remove_with_files"

export interface Settings {
  downloads: {
    default_save_path: string
    add_paused: boolean
    use_incomplete_dir: boolean
    /** Empty = "<download folder>/incomplete". */
    incomplete_dir: string
    category_moves: boolean
  }
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
  }
  peer_map: { locations: boolean; home: GeoPoint | null }
  updates: { check: boolean }
  usenet: UsenetSettings
}

export interface UsenetServer {
  id: string
  name: string
  enabled: boolean
  /** 0 = main; higher numbers are backups, tried for articles the main ones lack. */
  priority: number
  host: string
  port: number
  tls: boolean
  username: string
  /** Always empty when read back; send empty to keep the saved one. */
  password: string
  connections: number
}

export interface UsenetSettings {
  servers: UsenetServer[]
  complete_dir: string
  incomplete_dir: string
  /** KiB/s, 0 = no limit. */
  speed_limit: number
  paused: boolean
  sab_api: boolean
}

export type UsenetStatus =
  | "queued"
  | "downloading"
  | "paused"
  | "fetching"
  | "verifying"
  | "repairing"
  | "extracting"
  | "moving"
  | "completed"
  | "failed"

export type UsenetPriority = "low" | "normal" | "high" | "force"

export interface UsenetJob {
  id: string
  name: string
  category: string
  priority: UsenetPriority
  status: UsenetStatus
  size: number
  downloaded: number
  missing: number
  files: number
  added: number
  completed: number | null
  storage: string | null
  fail_message: string | null
  message: string | null
}

export interface UsenetServerStatus {
  id: string
  connections: number
  downloaded: number
  error: string | null
}

export interface UsenetOverview {
  paused: boolean
  speed: number
  remaining: number
  queue: UsenetJob[]
  history: UsenetJob[]
  servers: UsenetServerStatus[]
  complete_dir: string
}

export interface GeoPoint {
  lat: number
  lon: number
}

export type GeoState = "missing" | "downloading" | "ready" | "error" | "off" | "demo"

export interface GeoStatus {
  state: GeoState
  /** Database edition in use, e.g. "2026-11". */
  edition: string | null
  progress: number | null
  error: string | null
  attribution: string
}

/** Peers grouped by where they are. Rates are bytes per second. */
export interface PeerPlace {
  lat: number
  lon: number
  /** ISO 3166-1 alpha-2. */
  country: string
  city: string | null
  peers: number
  dl: number
  ul: number
}

export interface PeerMap {
  geo: GeoStatus
  home: GeoPoint | null
  total: number
  unlocated: number
  places: PeerPlace[]
}

/** A named API key. The key itself is only ever shown when it's made. */
export interface ApiKey {
  id: string
  name: string
  /** The key's last four characters. Empty for a key made before keys had names. */
  hint: string
  created: number
  expires: number | null
  last_used: number | null
  expired: boolean
}

export interface SettingsResponse {
  settings: Settings
  locked: string[]
  restart_needed: boolean
  sessions: number
  effective_download_dir: string
  data_dir: string
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
  /** Move to `location`, or back under each torrent's category folder with null. */
  move: (hashes: string[], location: string | null) =>
    request<void>("/torrents/move", json({ hashes, location })),
  files: (hash: string) => request<TorrentFile[]>(`/torrents/${hash}/files`),
  folders: (path: string | null) =>
    request<FolderListing>(path ? `/folders?path=${encodeURIComponent(path)}` : "/folders"),
  newFolder: (parent: string, name: string) =>
    request<{ name: string; path: string }>("/folders/new", json({ parent, name })),
  setMeta: (hash: string, meta: { category?: string; tags?: string[] }) =>
    request<void>(`/torrents/${hash}/meta`, { ...json(meta), method: "PUT" }),

  info: () => request<ServerInfo>("/info"),
  changelog: () => request<Changelog>("/changelog"),
  usenet: () => request<UsenetOverview>("/usenet"),
  usenetAdd: (form: FormData) => request<string[]>("/usenet/add", { method: "POST", body: form }),
  usenetPauseAll: (paused: boolean) => request<boolean>("/usenet/pause", json({ paused })),
  usenetPause: (id: string) => request<void>(`/usenet/jobs/${encodeURIComponent(id)}/pause`, { method: "POST" }),
  usenetResume: (id: string) => request<void>(`/usenet/jobs/${encodeURIComponent(id)}/resume`, { method: "POST" }),
  usenetMove: (id: string, index: number) => request<number>(`/usenet/jobs/${encodeURIComponent(id)}/move`, json({ index })),
  usenetRemove: (id: string) => request<void>(`/usenet/jobs/${encodeURIComponent(id)}`, { method: "DELETE" }),
  usenetRemoveHistory: (id: string, deleteFiles: boolean) =>
    request<void>(`/usenet/history/${encodeURIComponent(id)}?delete_files=${deleteFiles}`, { method: "DELETE" }),
  usenetClearHistory: () => request<void>("/usenet/history", { method: "DELETE" }),
  usenetTestServer: (s: UsenetServer) => request<{ ok: boolean; message: string }>("/usenet/servers/test", json(s)),
  peers: (hash?: string | null) => request<PeerMap>(`/peers${hash ? `?hash=${encodeURIComponent(hash)}` : ""}`),
  geo: () => request<GeoStatus>("/geo"),
  geoUpdate: () => request<void>("/geo/update", { method: "POST" }),
  settings: () => request<SettingsResponse>("/settings"),
  saveSettings: (s: Settings) =>
    request<{ restart_needed: boolean }>("/settings", { ...json(s), method: "PUT" }),
  setAltSpeed: (enabled: boolean) => request<void>("/speed/alt", json({ enabled })),
  tags: () => request<string[]>("/tags"),
  putTag: (name: string) => request<void>(`/tags/${encodeURIComponent(name)}`, { method: "PUT" }),
  deleteTag: (name: string) => request<void>(`/tags/${encodeURIComponent(name)}`, { method: "DELETE" }),
  changePassword: (body: { current_password: string; username?: string; new_password?: string }) =>
    request<{ signed_out: number }>("/auth/password", json(body)),
  update: () => request<UpdateStatus>("/update"),
  checkUpdate: () => request<UpdateStatus>("/update/check", { method: "POST" }),
  installUpdate: () => request<UpdateStatus>("/update/install", { method: "POST" }),
  apiKeys: () => request<ApiKey[]>("/keys"),
  createApiKey: (name: string, expires: number | null) =>
    request<ApiKey & { key: string }>("/keys", json({ name, expires })),
  renameApiKey: (id: string, name: string) =>
    request<ApiKey>(`/keys/${encodeURIComponent(id)}`, { ...json({ name }), method: "PATCH" }),
  revokeApiKey: (id: string) => request<void>(`/keys/${encodeURIComponent(id)}`, { method: "DELETE" }),
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
