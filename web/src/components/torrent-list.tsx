import type { Torrent } from "@/lib/api"
import { formatEta, formatPercent, formatSpeed } from "@/lib/format"
import { cn } from "@/lib/utils"
import { barColor, stateLabel } from "@/components/torrent-table"

/** Phone layout: one tappable row per torrent, 64px+ tall. */
export function TorrentList({
  rows,
  focused,
  onFocus,
}: {
  rows: Torrent[]
  focused: string | null
  onFocus: (hash: string) => void
}) {
  return (
    <ul className="flex flex-col text-[13px]" aria-label="Torrents">
      {rows.map((t) => (
        <li key={t.hash}>
          <button
            type="button"
            onClick={() => onFocus(t.hash)}
            className={cn(
              "flex min-h-16 w-full flex-col gap-1.5 border-b border-border/60 px-4 py-2.5 text-left active:bg-muted",
              focused === t.hash && "bg-muted"
            )}
          >
            <span className="flex items-baseline justify-between gap-3">
              <span className="min-w-0 truncate font-medium">{t.name}</span>
              <span className="data shrink-0 text-xs">
                {t.download_speed > 0 ? formatSpeed(t.download_speed) : t.upload_speed > 0 ? `▲ ${formatSpeed(t.upload_speed)}` : ""}
              </span>
            </span>
            <span className="h-1 overflow-hidden rounded-sm bg-muted">
              <span className="block h-full" style={{ width: `${Math.max(2, t.progress * 100)}%`, background: barColor(t) }} />
            </span>
            <span className={cn("flex justify-between gap-3 text-[11px] text-muted-foreground", t.state === "error" && "text-destructive")}>
              <span className="truncate">{t.category || "no category"}, {stateLabel(t)}</span>
              <span className="data shrink-0">
                {formatPercent(t.progress)}
                {t.eta != null && `, ${formatEta(t.eta)}`}
              </span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  )
}
