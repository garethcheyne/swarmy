import { useState } from "react"
import {
  ArrowDownIcon,
  ArrowUpDownIcon,
  ArrowUpIcon,
  CopyIcon,
  PauseIcon,
  PlayIcon,
  Trash2Icon,
} from "lucide-react"

import type { Torrent } from "@/lib/api"
import { formatEta, formatPercent, formatRelative, formatSpeed } from "@/lib/format"
import { cn } from "@/lib/utils"
import { Checkbox } from "@/components/ui/checkbox"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"

export type SortKey =
  | "name"
  | "progress"
  | "download_speed"
  | "upload_speed"
  | "eta"
  | "ratio"
  | "peers"
  | "added_on"

export interface Sort {
  key: SortKey
  desc: boolean
}

export function sortTorrents(list: Torrent[], sort: Sort): Torrent[] {
  const cmp = (a: Torrent, b: Torrent) => {
    if (sort.key === "name") return a.name.localeCompare(b.name, undefined, { numeric: true })
    if (sort.key === "eta") return (a.eta ?? Infinity) - (b.eta ?? Infinity)
    return (a[sort.key] as number) - (b[sort.key] as number)
  }
  const sorted = [...list].sort(cmp)
  return sort.desc ? sorted.reverse() : sorted
}

/** Short state label, matching what the status badge would say. */
export function stateLabel(t: Torrent): string {
  if (t.state === "error") return t.error ? `error: ${t.error}` : "error"
  if (t.state === "initializing") return t.has_metadata ? "checking" : "fetching metadata"
  if (t.queued) return "queued"
  if (t.state === "paused") return t.progress >= 1 ? "completed" : "paused"
  if (t.state === "downloading" && t.download_speed === 0 && t.peers === 0) return "stalled"
  return t.state
}

export function barColor(t: Torrent): string {
  const s = stateLabel(t)
  if (t.state === "error") return "var(--destructive)"
  if (t.state === "seeding") return "var(--up)"
  if (s === "paused" || s === "completed" || s === "stalled") return "color-mix(in oklab, var(--muted-foreground) 70%, transparent)"
  return "var(--down)"
}

const COLS: { key: SortKey; label: string; className: string }[] = [
  { key: "name", label: "Name", className: "" },
  { key: "progress", label: "Progress", className: "w-32" },
  { key: "download_speed", label: "Down", className: "w-24 text-right" },
  { key: "upload_speed", label: "Up", className: "w-24 text-right" },
  { key: "eta", label: "ETA", className: "w-20 text-right" },
  { key: "ratio", label: "Ratio", className: "w-16 text-right" },
  { key: "peers", label: "Peers", className: "w-14 text-right" },
  { key: "added_on", label: "Added", className: "hidden w-28 text-right 2xl:block" },
]

const GRID = "grid grid-cols-[28px_minmax(0,1fr)_8rem_6rem_6rem_5rem_4rem_3.5rem] 2xl:grid-cols-[28px_minmax(0,1fr)_8rem_6rem_6rem_5rem_4rem_3.5rem_7rem] items-center gap-3"

interface Props {
  rows: Torrent[]
  sort: Sort
  onSortChange: (s: Sort) => void
  focused: string | null
  onFocus: (hash: string) => void
  selected: Set<string>
  onSelectedChange: (s: Set<string>) => void
  onPause: (hashes: string[]) => void
  onResume: (hashes: string[]) => void
  onDelete: (hashes: string[]) => void
}

export function TorrentTable({
  rows,
  sort,
  onSortChange,
  focused,
  onFocus,
  selected,
  onSelectedChange,
  onPause,
  onResume,
  onDelete,
}: Props) {
  const allSelected = rows.length > 0 && rows.every((t) => selected.has(t.hash))
  const someSelected = !allSelected && rows.some((t) => selected.has(t.hash))
  const targets = (hash: string) => (selected.has(hash) ? [...selected] : [hash])

  const toggle = (hash: string, on: boolean) => {
    const next = new Set(selected)
    if (on) next.add(hash)
    else next.delete(hash)
    onSelectedChange(next)
  }

  return (
    <div role="table" aria-label="Torrents" className="min-w-[760px] text-[13px]">
      <div role="row" className={cn(GRID, "label-caps sticky top-0 z-10 border-b bg-background px-4 py-2")}>
        <span role="columnheader">
          <Checkbox
            aria-label="Select all"
            checked={allSelected}
            indeterminate={someSelected}
            onCheckedChange={(on) => onSelectedChange(on ? new Set(rows.map((t) => t.hash)) : new Set())}
          />
        </span>
        {COLS.map((c) => {
          const active = sort.key === c.key
          const Icon = !active ? ArrowUpDownIcon : sort.desc ? ArrowDownIcon : ArrowUpIcon
          return (
            <span role="columnheader" key={c.key} className={c.className} aria-sort={active ? (sort.desc ? "descending" : "ascending") : undefined}>
              <button
                type="button"
                className={cn("inline-flex items-center gap-1 uppercase hover:text-foreground", active && "text-foreground")}
                onClick={() => onSortChange(active ? { key: c.key, desc: !sort.desc } : { key: c.key, desc: c.key !== "name" })}
              >
                {c.label}
                <Icon className={cn("size-3", !active && "opacity-40")} />
              </button>
            </span>
          )
        })}
      </div>
      <TableBody
        rows={rows}
        focused={focused}
        onFocus={onFocus}
        selected={selected}
        toggle={toggle}
        targets={targets}
        onPause={onPause}
        onResume={onResume}
        onDelete={onDelete}
      />
    </div>
  )
}

function TableBody({
  rows,
  focused,
  onFocus,
  selected,
  toggle,
  targets,
  onPause,
  onResume,
  onDelete,
}: {
  rows: Torrent[]
  focused: string | null
  onFocus: (hash: string) => void
  selected: Set<string>
  toggle: (hash: string, on: boolean) => void
  targets: (hash: string) => string[]
  onPause: (h: string[]) => void
  onResume: (h: string[]) => void
  onDelete: (h: string[]) => void
}) {
  const items = rows
  const [menuFor, setMenuFor] = useState<string | null>(null)
  return (
    <div role="rowgroup">
      {items.map((t) => {
        const isFocused = focused === t.hash
        const label = stateLabel(t)
        return (
          <ContextMenu key={t.hash} onOpenChange={(o) => setMenuFor(o ? t.hash : null)}>
            <ContextMenuTrigger
              render={
                <div
                  role="row"
                  tabIndex={-1}
                  data-hash={t.hash}
                  aria-selected={isFocused}
                  onClick={() => onFocus(t.hash)}
                  className={cn(
                    GRID,
                    "cursor-default border-b border-border/60 px-4 py-[7px] hover:bg-muted/50",
                    isFocused && "bg-muted shadow-[inset_2px_0_0_var(--sidebar-primary)]",
                    menuFor === t.hash && "bg-muted/70"
                  )}
                />
              }
            >
              <span role="cell" onClick={(e) => e.stopPropagation()}>
                <Checkbox
                  aria-label={`Select ${t.name}`}
                  checked={selected.has(t.hash)}
                  onCheckedChange={(on) => toggle(t.hash, on)}
                />
              </span>
              <span role="cell" className="flex min-w-0 flex-col">
                <span className="truncate font-medium" title={t.name}>{t.name}</span>
                <span className={cn("truncate text-[11px] text-muted-foreground", t.state === "error" && "text-destructive")}>
                  {t.category || "no category"}, {label}
                </span>
              </span>
              <span role="cell" className="flex flex-col gap-[3px]">
                <span className="h-1 overflow-hidden rounded-sm bg-muted">
                  <span
                    className={cn("bar-fill block h-full", t.state === "downloading" && t.download_speed > 0 && "bar-live")}
                    style={{ width: `${Math.max(2, t.progress * 100)}%`, background: barColor(t) }}
                  />
                </span>
                <span className="data text-[11px] text-muted-foreground">{formatPercent(t.progress)}</span>
              </span>
              <span role="cell" className="data text-right">{formatSpeed(t.download_speed)}</span>
              <span role="cell" className="data text-right">{formatSpeed(t.upload_speed)}</span>
              <span role="cell" className="data text-right">{formatEta(t.eta)}</span>
              <span role="cell" className="data text-right">{t.ratio.toFixed(2)}</span>
              <span role="cell" className="data text-right">{t.peers}</span>
              <span role="cell" className="hidden text-right text-muted-foreground 2xl:block">{formatRelative(t.added_on)}</span>
            </ContextMenuTrigger>
            <ContextMenuContent>
              <ContextMenuGroup>
                <ContextMenuItem onClick={() => onResume(targets(t.hash))}>
                  <PlayIcon />
                  Resume
                </ContextMenuItem>
                <ContextMenuItem onClick={() => onPause(targets(t.hash))}>
                  <PauseIcon />
                  Pause
                </ContextMenuItem>
              </ContextMenuGroup>
              <ContextMenuSeparator />
              <ContextMenuGroup>
                <ContextMenuItem onClick={() => navigator.clipboard.writeText(t.hash).catch(() => {})}>
                  <CopyIcon />
                  Copy info-hash
                </ContextMenuItem>
              </ContextMenuGroup>
              <ContextMenuSeparator />
              <ContextMenuGroup>
                <ContextMenuItem variant="destructive" onClick={() => onDelete(targets(t.hash))}>
                  <Trash2Icon />
                  Delete…
                </ContextMenuItem>
              </ContextMenuGroup>
            </ContextMenuContent>
          </ContextMenu>
        )
      })}
    </div>
  )
}
