import { useEffect, useMemo, useState } from "react"
import { PauseIcon, PlayIcon, SearchIcon, Trash2Icon, XIcon } from "lucide-react"

import type { Torrent } from "@/lib/api"
import { matchesFilter, type Filter } from "@/lib/filters"
import { useMediaQuery } from "@/hooks/use-media"
import { useIsMobile } from "@/hooks/use-mobile"
import { Inspector } from "@/components/inspector"
import { TorrentList } from "@/components/torrent-list"
import { sortTorrents, TorrentTable, type Sort } from "@/components/torrent-table"
import { Button } from "@/components/ui/button"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group"
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"

const VIEW_TITLE: Record<string, string> = {
  all: "All torrents",
  downloading: "Downloading",
  seeding: "Seeding",
  completed: "Completed",
  active: "Active",
  stalled: "Stalled",
  error: "Errored",
  paused: "Paused",
}

function title(f: Filter) {
  if (f.kind === "status") return VIEW_TITLE[f.value]
  if (f.kind === "category") return f.value
  return `#${f.value}`
}

interface Props {
  loading: boolean
  torrents: Torrent[]
  categories: string[]
  filter: Filter
  focused: string | null
  onFocus: (hash: string | null) => void
  onAdd: () => void
  onPause: (hashes: string[]) => void
  onResume: (hashes: string[]) => void
  onDelete: (hashes: string[]) => void
}

export function TorrentsPage({
  loading,
  torrents,
  categories,
  filter,
  focused,
  onFocus,
  onAdd,
  onPause,
  onResume,
  onDelete,
}: Props) {
  const isMobile = useIsMobile()
  const wide = useMediaQuery("(min-width: 1024px)")
  const [query, setQuery] = useState("")
  const [sort, setSort] = useState<Sort>({ key: "added_on", desc: true })
  const [rawSelected, setSelected] = useState<Set<string>>(new Set())

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase()
    const list = torrents.filter((t) => matchesFilter(t, filter) && (!q || t.name.toLowerCase().includes(q)))
    return sortTorrents(list, sort)
  }, [torrents, filter, query, sort])

  const selected = useMemo(() => {
    const live = new Set(rows.map((t) => t.hash))
    return new Set([...rawSelected].filter((h) => live.has(h)))
  }, [rawSelected, rows])
  const targets = useMemo(
    () => (selected.size > 0 ? [...selected] : focused ? [focused] : []),
    [selected, focused]
  )
  const focusedTorrent = torrents.find((t) => t.hash === focused)

  // Keyboard: j/k or arrows move, space pauses/resumes, Delete deletes, Esc clears.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement
      if (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)) return
      if (document.querySelector("[role=dialog],[role=menu]")) return
      const i = rows.findIndex((t) => t.hash === focused)
      const move = (d: number) => {
        const next = rows[Math.min(rows.length - 1, Math.max(0, (i < 0 ? -1 : i) + d))]
        if (!next) return
        onFocus(next.hash)
        document.querySelector(`[data-hash="${next.hash}"]`)?.scrollIntoView({ block: "nearest" })
      }
      if (e.key === "j" || e.key === "ArrowDown") {
        e.preventDefault()
        move(1)
      } else if (e.key === "k" || e.key === "ArrowUp") {
        e.preventDefault()
        move(-1)
      } else if (e.key === " " && focusedTorrent) {
        e.preventDefault()
        if (focusedTorrent.state === "paused" || focusedTorrent.state === "error") onResume(targets)
        else onPause(targets)
      } else if ((e.key === "Delete" || e.key === "Backspace") && targets.length) {
        e.preventDefault()
        onDelete(targets)
      } else if (e.key === "Escape") {
        setSelected(new Set())
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [rows, focused, focusedTorrent, targets, onFocus, onPause, onResume, onDelete])

  const inspector = (
    <Inspector
      torrent={focusedTorrent}
      categories={categories}
      onPause={onPause}
      onResume={onResume}
      onDelete={onDelete}
    />
  )

  return (
    <div className="flex h-full min-h-0">
      <section className="flex min-w-0 flex-1 flex-col" aria-label={title(filter)}>
        <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-2.5">
          <h1 className="text-[15px] font-semibold">{title(filter)}</h1>
          <span className="text-[13px] text-muted-foreground">
            {rows.length} {rows.length === 1 ? "torrent" : "torrents"}
            {selected.size > 0 && `, ${selected.size} selected`}
          </span>
          <div className="ml-auto flex items-center gap-1.5">
            <InputGroup className="h-8 w-44 sm:w-56">
              <InputGroupAddon>
                <SearchIcon />
              </InputGroupAddon>
              <InputGroupInput
                aria-label="Filter by name"
                placeholder="Filter by name"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </InputGroup>
            {selected.size > 0 && (
              <>
                <Button size="sm" variant="outline" onClick={() => onResume([...selected])}>
                  <PlayIcon data-icon="inline-start" />
                  <span className="hidden sm:inline">Resume</span>
                </Button>
                <Button size="sm" variant="outline" onClick={() => onPause([...selected])}>
                  <PauseIcon data-icon="inline-start" />
                  <span className="hidden sm:inline">Pause</span>
                </Button>
                <Button size="sm" variant="outline" className="text-destructive" onClick={() => onDelete([...selected])}>
                  <Trash2Icon data-icon="inline-start" />
                  <span className="hidden sm:inline">Delete</span>
                </Button>
                <Button size="icon-sm" variant="ghost" aria-label="Clear selection" onClick={() => setSelected(new Set())}>
                  <XIcon />
                </Button>
              </>
            )}
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto">
          {loading ? (
            <div className="flex flex-col gap-2 p-4">
              {Array.from({ length: 10 }, (_, i) => (
                <Skeleton key={i} className="h-9" />
              ))}
            </div>
          ) : rows.length === 0 ? (
            <Empty className="h-full">
              <EmptyHeader>
                <EmptyTitle>{torrents.length === 0 ? "No torrents yet" : "Nothing in this view"}</EmptyTitle>
                <EmptyDescription>
                  {torrents.length === 0 ? (
                    <>
                      Paste a magnet link anywhere, drop a .torrent file on the window, or{" "}
                      <button type="button" className="underline" onClick={onAdd}>
                        add one
                      </button>
                      .
                    </>
                  ) : (
                    "Pick another view or clear the filter."
                  )}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : isMobile ? (
            <TorrentList rows={rows} focused={focused} onFocus={onFocus} />
          ) : (
            <TorrentTable
              rows={rows}
              sort={sort}
              onSortChange={setSort}
              focused={focused}
              onFocus={onFocus}
              selected={selected}
              onSelectedChange={setSelected}
              onPause={onPause}
              onResume={onResume}
              onDelete={onDelete}
            />
          )}
        </div>
      </section>

      {wide && (
        <aside aria-label="Selected torrent" className="w-90 shrink-0 overflow-y-auto border-l bg-card/40 p-4 xl:w-95">
          {inspector}
        </aside>
      )}

      {/* Phones and narrow windows: the inspector slides up from the bottom. */}
      <Sheet open={!!focusedTorrent && !wide} onOpenChange={(o) => !o && onFocus(null)}>
        <SheetContent side="bottom" className="max-h-[85svh] overflow-y-auto rounded-t-2xl px-4 pt-2 pb-[max(1rem,env(safe-area-inset-bottom))]">
          <SheetHeader className="sr-only">
            <SheetTitle>{focusedTorrent?.name ?? "Torrent"}</SheetTitle>
            <SheetDescription>Details and actions for this torrent</SheetDescription>
          </SheetHeader>
          <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-muted" aria-hidden />
          {inspector}
        </SheetContent>
      </Sheet>
    </div>
  )
}
