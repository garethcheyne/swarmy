import type { Torrent } from "@/lib/api"
import { formatBytes, formatEta, formatPercent, formatSpeed } from "@/lib/format"
import { cn } from "@/lib/utils"
import { barColor, stateLabel } from "@/components/torrent-table"

/**
 * Phone layout: one tappable row per torrent. The name gets two lines (the end
 * of a release name is often the part that matters), unfinished torrents get a
 * progress bar, and the line under it says what matters for that torrent now:
 * how far and how fast while it downloads, size, ratio and upload once it's done.
 */
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
    <ul className="flex flex-col" aria-label="Torrents">
      {rows.map((t) => {
        const done = t.progress >= 1
        const state = stateLabel(t)
        const failed = t.state === "error" || !!t.move_error
        // The bar and the speed already say it's downloading; say so to screen
        // readers only, and leave the room for the numbers.
        const obvious = state === "downloading" && t.download_speed > 0
        return (
          <li key={t.hash}>
            <button
              type="button"
              data-hash={t.hash}
              onClick={() => onFocus(t.hash)}
              className={cn(
                "flex w-full flex-col gap-2 border-b border-border/60 px-4 py-3 text-left active:bg-muted",
                focused === t.hash && "bg-muted"
              )}
            >
              <span className="line-clamp-2 text-[14px] leading-snug font-medium [overflow-wrap:anywhere]">{t.name}</span>
              {!done && (
                <span className="h-1 overflow-hidden rounded-full bg-muted" aria-hidden>
                  <span className="block h-full rounded-full" style={{ width: `${Math.max(2, t.progress * 100)}%`, background: barColor(t) }} />
                </span>
              )}
              <span className="flex min-w-0 items-center gap-x-3 text-[12px] text-muted-foreground">
                <span className={cn("flex min-w-0 shrink items-center gap-1.5", failed && "text-destructive")}>
                  <span className="size-2 shrink-0 rounded-full" style={{ background: barColor(t) }} aria-hidden />
                  <span className={obvious ? "sr-only" : "truncate"}>{state}</span>
                </span>
                {!failed && (
                  <span className="data flex shrink-0 items-center gap-x-3 text-foreground/80">
                    {done ? (
                      <>
                        <span>{formatBytes(t.size)}</span>
                        <span>
                          <span className="text-muted-foreground">ratio </span>
                          {t.ratio.toFixed(2)}
                        </span>
                        {t.upload_speed > 0 && <span className="text-up">▲ {formatSpeed(t.upload_speed)}</span>}
                      </>
                    ) : (
                      <>
                        <span>
                          {formatPercent(t.progress)}
                          {t.size > 0 && <span className="text-muted-foreground"> of {formatBytes(t.size)}</span>}
                        </span>
                        {t.download_speed > 0 && <span className="text-down">▼ {formatSpeed(t.download_speed)}</span>}
                        {t.eta != null && t.download_speed > 0 && <span>{formatEta(t.eta)}</span>}
                      </>
                    )}
                  </span>
                )}
                {t.category && <span className="ml-auto truncate pl-2 text-right">{t.category}</span>}
              </span>
            </button>
          </li>
        )
      })}
    </ul>
  )
}
