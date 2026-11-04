import { useEffect, useState, type ReactNode } from "react"
import { cn } from "@/lib/utils"
import { CopyIcon, MousePointerClickIcon, PauseIcon, PlayIcon, Trash2Icon } from "lucide-react"
import { toast } from "sonner"

import { api, type PieceMap, type Torrent, type TorrentFile } from "@/lib/api"
import { formatBytes, formatDate, formatEta, formatPercent, formatRelative, formatSpeed } from "@/lib/format"
import { StatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

const NO_CATEGORY = "__none__"
const BUCKETS = 216

/** Poll `fetcher` every `ms` while `key` is set; reset when it changes. */
function useLive<T>(key: string | null, fetcher: (k: string) => Promise<T>, ms: number) {
  const [state, setState] = useState<{ key: string | null; data: T | null }>({ key: null, data: null })
  useEffect(() => {
    if (!key) return
    let cancelled = false
    const load = () =>
      fetcher(key)
        .then((d) => !cancelled && setState({ key, data: d }))
        .catch(() => {})
    load()
    const id = setInterval(load, ms)
    return () => {
      cancelled = true
      clearInterval(id)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- fetcher is stable per call site
  }, [key, ms])
  return state.key === key ? state.data : null
}

const COLS = 36

/**
 * Pieces ripple in diagonally when the map first loads. After that, a cell
 * that gains pieces flashes: its key includes its value, so it remounts with
 * the flash class, while unchanged cells are left alone.
 */
function PieceGrid({ map }: { map: PieceMap | null }) {
  const cells = map?.buckets ?? Array.from({ length: BUCKETS }, () => -1)
  const loaded = map != null
  // Which cells gained pieces since the last map (derived during render, the
  // React way of tracking a previous prop).
  const [prev, setPrev] = useState(map)
  const [fresh, setFresh] = useState<ReadonlySet<number>>(() => new Set())
  if (map !== prev) {
    const changed = new Set<number>()
    if (prev && map) map.buckets.forEach((v, i) => v > (prev.buckets[i] ?? 0) && changed.add(i))
    setPrev(map)
    setFresh(changed)
  }
  const [entering, setEntering] = useState(true)
  useEffect(() => {
    if (!loaded) return
    const id = setTimeout(() => setEntering(false), 1400)
    return () => clearTimeout(id)
  }, [loaded])
  return (
    <div
      role="img"
      aria-label={map ? `Piece map: ${map.have} of ${map.total} pieces` : "Loading piece map"}
      className="grid gap-[2px]"
      style={{ gridTemplateColumns: `repeat(${COLS}, minmax(0, 1fr))` }}
    >
      {cells.map((v, i) => (
        <span
          key={`${i}:${v}`}
          className={cn(
            "h-2 rounded-[1px]",
            loaded && entering && "piece-in",
            !entering && fresh.has(i) && "piece-flash"
          )}
          style={{
            animationDelay: entering ? `${((i % COLS) + Math.floor(i / COLS)) * 14}ms` : undefined,
            background:
              v <= 0 ? "var(--muted)" : `color-mix(in oklab, var(--down) ${Math.max(35, v)}%, var(--muted))`,
          }}
        />
      ))}
    </div>
  )
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="m-0 min-w-0">{children}</dd>
    </>
  )
}

interface Props {
  torrent: Torrent | undefined
  categories: string[]
  onPause: (hashes: string[]) => void
  onResume: (hashes: string[]) => void
  onDelete: (hashes: string[]) => void
}

export function Inspector({ torrent: t, categories, onPause, onResume, onDelete }: Props) {
  const map = useLive(t?.has_metadata ? t.hash : null, (h) => api.pieces(h, BUCKETS), 2000)
  const files = useLive<TorrentFile[]>(t?.has_metadata ? t.hash : null, api.files, 3000)

  if (!t) {
    return (
      <Empty className="h-full">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <MousePointerClickIcon />
          </EmptyMedia>
          <EmptyTitle>Nothing selected</EmptyTitle>
          <EmptyDescription>Pick a torrent to see its pieces, peers and files.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  const paused = t.state === "paused" || t.state === "error"
  const setCategory = (value: string) =>
    api
      .setMeta(t.hash, { category: value === NO_CATEGORY ? "" : value })
      .catch((e) => toast.error("Couldn't change category", { description: String(e) }))

  return (
    <div className="flex flex-col gap-5 text-[13px]">
      <div className="flex flex-col gap-1">
        <span className="text-[11px] text-muted-foreground">
          {t.category || "No category"}, added {formatRelative(t.added_on)}
        </span>
        <h2 className="text-lg leading-snug font-semibold break-all">{t.name}</h2>
        <div className="mt-1">
          <StatusBadge torrent={t} />
        </div>
      </div>

      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="data text-4xl font-medium">{formatPercent(t.progress)}</span>
        <span className="text-muted-foreground">
          {formatBytes(t.downloaded)} of {t.has_metadata ? formatBytes(t.size) : "unknown size"}
          {t.eta != null && `, ${formatEta(t.eta)} left`}
        </span>
      </div>

      <div className="flex flex-col gap-1.5">
        <span className="text-[11px] text-muted-foreground">
          {map ? `Pieces: ${map.have.toLocaleString()} of ${map.total.toLocaleString()}` : t.has_metadata ? "Pieces" : "Waiting for metadata from peers"}
        </span>
        <PieceGrid key={t.hash} map={map} />
      </div>

      <dl className="grid grid-cols-[100px_minmax(0,1fr)] gap-x-3 gap-y-1.5">
        <Row label="Download"><span className="data">{formatSpeed(t.download_speed)}</span></Row>
        <Row label="Upload"><span className="data">{formatSpeed(t.upload_speed)}</span></Row>
        <Row label="Peers"><span className="data">{t.peers} connected, {t.seeds} seeds</span></Row>
        <Row label="Uploaded"><span className="data">{formatBytes(t.uploaded)}, ratio {t.ratio.toFixed(2)}</span></Row>
        <Row label="Category">
          <Select
            value={t.category || NO_CATEGORY}
            onValueChange={(v) => v && setCategory(v)}
            items={[{ value: NO_CATEGORY, label: "None" }, ...categories.map((c) => ({ value: c, label: c }))]}
          >
            <SelectTrigger size="sm" className="h-7 w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value={NO_CATEGORY}>None</SelectItem>
                {categories.map((c) => (
                  <SelectItem key={c} value={c}>{c}</SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Save path"><span className="data break-all">{t.save_path}</span></Row>
        <Row label="Completed">{formatDate(t.completed_on)}</Row>
        <Row label="Info-hash">
          <button
            type="button"
            className="data inline-flex max-w-full items-center gap-1 text-left break-all hover:underline"
            onClick={() =>
              navigator.clipboard
                .writeText(t.hash)
                .then(() => toast.success("Info-hash copied"))
                .catch(() => toast.error("Couldn't copy"))
            }
          >
            {t.hash}
            <CopyIcon className="size-3 shrink-0" />
          </button>
        </Row>
        {t.error && <Row label="Error"><span className="text-destructive">{t.error}</span></Row>}
      </dl>

      {t.has_metadata && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[11px] text-muted-foreground">Files{files ? `, ${files.length}` : ""}</span>
          <ul className="flex flex-col gap-1">
            {(files ?? []).slice(0, 50).map((f) => (
              <li key={f.index} className="flex flex-col gap-1">
                <div className="flex justify-between gap-3">
                  <span className="truncate" title={f.path}>{f.path.split("/").pop()}</span>
                  <span className="data shrink-0 text-muted-foreground">{formatBytes(f.size)}</span>
                </div>
                <span className="h-0.5 overflow-hidden rounded bg-muted">
                  <span
                    className="block h-full bg-down"
                    style={{ width: `${f.size ? (f.downloaded_bytes / f.size) * 100 : 0}%` }}
                  />
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="flex gap-2">
        {paused ? (
          <Button className="flex-1" onClick={() => onResume([t.hash])}>
            <PlayIcon data-icon="inline-start" />
            Resume
          </Button>
        ) : (
          <Button variant="outline" className="flex-1" onClick={() => onPause([t.hash])}>
            <PauseIcon data-icon="inline-start" />
            Pause
          </Button>
        )}
        <Button variant="outline" className="flex-1" onClick={() => onDelete([t.hash])}>
          <Trash2Icon data-icon="inline-start" />
          Delete
        </Button>
      </div>
    </div>
  )
}
