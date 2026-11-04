import { useMemo } from "react"
import { ExternalLinkIcon, TurtleIcon } from "lucide-react"

import type { LiveSample, Snapshot } from "@/lib/api"
import { formatEta, formatPercent, formatSpeed } from "@/lib/format"
import { cn } from "@/lib/utils"
import { barColor, stateLabel } from "@/components/torrent-table"
import { Logo } from "@/components/logo"
import { LiveSpark } from "@/components/charts"

interface Props {
  snapshot: Snapshot | null
  history: LiveSample[]
  onToggleAlt: () => void
  /** Open a torrent (or the app) in the main window. */
  onOpen: (hash?: string) => void
  /** Rendered inside a floating window: no "open in a window" affordances. */
  floating?: boolean
}

function Spark({ history }: { history: LiveSample[] }) {
  return <LiveSpark points={history.slice(-180)} width={300} height={32} stretch className="h-8 w-full" />
}

/** A compact, glanceable Swarmy: for a small window or a floating picture-in-picture. */
export function MiniView({ snapshot, history, onToggleAlt, onOpen, floating }: Props) {
  const torrents = useMemo(() => snapshot?.torrents ?? [], [snapshot])
  const s = snapshot?.stats
  const attention = torrents.filter((t) => t.state === "error" || stateLabel(t) === "stalled")
  const active = useMemo(
    () =>
      torrents
        .filter((t) => t.state === "downloading" || t.state === "initializing" || t.state === "error")
        .sort((a, b) => b.download_speed - a.download_speed),
    [torrents]
  )
  const seeding = torrents.filter((t) => t.state === "seeding").length

  return (
    <div className="flex h-full min-h-0 flex-col bg-background text-[13px] text-foreground">
      <header className="flex items-center gap-2 border-b bg-sidebar px-3 py-2 text-sidebar-foreground">
        <Logo className="size-4" />
        <span className="font-semibold text-sidebar-accent-foreground">Swarmy</span>
        <div className="data ml-auto flex gap-3 text-xs text-sidebar-accent-foreground">
          <span>
            <span className="text-down">▼</span> {formatSpeed(s?.download_speed ?? 0)}
          </span>
          <span>
            <span className="text-up">▲</span> {formatSpeed(s?.upload_speed ?? 0)}
          </span>
        </div>
        <button
          type="button"
          onClick={onToggleAlt}
          aria-pressed={!!s?.alt_speed}
          aria-label={s?.alt_speed ? "Alternative speed limits are on" : "Use alternative speed limits"}
          title={s?.alt_speed ? "Alternative speed limits are on" : "Use alternative speed limits"}
          className={cn(
            "grid size-7 place-items-center rounded-md hover:bg-sidebar-accent",
            s?.alt_speed && "bg-sidebar-primary/20 text-sidebar-primary"
          )}
        >
          <TurtleIcon className="size-4" />
        </button>
      </header>

      <div className="border-b px-3 pt-1.5">
        <Spark history={history} />
      </div>

      <div className="flex gap-4 border-b px-3 py-1.5 text-xs text-muted-foreground">
        <span><b className="data font-medium text-foreground">{active.filter((t) => t.state !== "error").length}</b> downloading</span>
        <span><b className="data font-medium text-foreground">{seeding}</b> seeding</span>
        {attention.length > 0 && (
          <span className="text-destructive"><b className="data font-medium">{attention.length}</b> need attention</span>
        )}
      </div>

      <ul className="min-h-0 flex-1 overflow-y-auto">
        {!snapshot ? (
          <li className="p-3 text-muted-foreground">Connecting…</li>
        ) : active.length === 0 ? (
          <li className="p-3 text-muted-foreground">Nothing downloading right now.</li>
        ) : (
          active.map((t) => (
            <li key={t.hash}>
              <button
                type="button"
                onClick={() => onOpen(t.hash)}
                className="flex w-full flex-col gap-1 border-b border-border/60 px-3 py-2 text-left hover:bg-muted/50"
              >
                <span className="flex items-baseline justify-between gap-2">
                  <span className="min-w-0 truncate font-medium">{t.name}</span>
                  <span className="data shrink-0 text-xs">{formatSpeed(t.download_speed)}</span>
                </span>
                <span className="h-1 overflow-hidden rounded-sm bg-muted">
                  <span className="block h-full" style={{ width: `${Math.max(2, t.progress * 100)}%`, background: barColor(t) }} />
                </span>
                <span className={cn("data flex justify-between text-[11px] text-muted-foreground", t.state === "error" && "text-destructive")}>
                  <span>{formatPercent(t.progress)}</span>
                  <span>{t.state === "error" ? "error" : t.eta != null ? `${formatEta(t.eta)} left` : stateLabel(t)}</span>
                </span>
              </button>
            </li>
          ))
        )}
      </ul>

      <footer className="border-t px-3 py-2">
        <button
          type="button"
          onClick={() => onOpen()}
          className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground"
        >
          <ExternalLinkIcon className="size-3.5" />
          {floating ? "Show Swarmy" : "Open the full app"}
        </button>
      </footer>
    </div>
  )
}
