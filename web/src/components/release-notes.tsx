import { Fragment, useEffect, useState, type ReactNode } from "react"
import {
  BugIcon,
  SparklesIcon,
  TrendingUpIcon,
  type LucideIcon,
} from "lucide-react"

import { api, type Changelog, type Release } from "@/lib/api"
import { cn } from "@/lib/utils"
import { compareVersions } from "@/lib/version"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Skeleton } from "@/components/ui/skeleton"

/** Fetch the built-in changelog once (it can't change without a restart). */
export function useChangelog(enabled = true) {
  const [data, setData] = useState<Changelog | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!enabled || data) return
    let cancelled = false
    api
      .changelog()
      .then((d) => !cancelled && setData(d))
      .catch(
        (e) =>
          !cancelled && setError(e instanceof Error ? e.message : String(e))
      )
    return () => {
      cancelled = true
    }
  }, [enabled, data])
  return { data, error }
}

const SECTION_ICONS: Record<string, LucideIcon> = {
  new: SparklesIcon,
  added: SparklesIcon,
  improved: TrendingUpIcon,
  changed: TrendingUpIcon,
  fixed: BugIcon,
}

/** `code` and **bold**, the only inline markup the changelog uses. */
function inline(text: string): ReactNode {
  return text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).map((part, i) => {
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return (
        <code
          key={i}
          className="data rounded-sm bg-muted px-1 py-px text-[0.92em]"
        >
          {part.slice(1, -1)}
        </code>
      )
    }
    if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
      return (
        <strong key={i} className="font-semibold text-foreground">
          {part.slice(2, -2)}
        </strong>
      )
    }
    return <Fragment key={i}>{part}</Fragment>
  })
}

function formatDate(iso: string) {
  const d = new Date(`${iso}T00:00:00`)
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleDateString(undefined, {
        day: "numeric",
        month: "long",
        year: "numeric",
      })
}

/**
 * Releases on a rail, newest first. The running version's node is filled in.
 */
export function ReleaseNotes({
  releases,
  current,
}: {
  releases: Release[]
  current: string
}) {
  return (
    <ol className="relative flex flex-col gap-8 pl-6">
      <span
        aria-hidden
        className="absolute top-2 bottom-2 left-[5px] w-px bg-border"
      />
      {releases.map((r) => {
        const isCurrent = r.version === current
        return (
          <li key={r.version} className="relative">
            <span
              aria-hidden
              className={cn(
                "absolute top-[7px] -left-6 size-[11px] rounded-full border-2",
                isCurrent
                  ? "border-primary bg-primary shadow-[0_0_0_4px_color-mix(in_oklab,var(--primary)_22%,transparent)]"
                  : "border-border bg-background"
              )}
            />
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <h3 className="data text-lg font-semibold">{r.version}</h3>
              <span className="text-[13px] text-muted-foreground">
                {r.date ? formatDate(r.date) : "Not released yet"}
              </span>
              {isCurrent && (
                <Badge variant="secondary">You're running this</Badge>
              )}
            </div>
            {r.summary && (
              <p className="mt-1.5 max-w-prose text-[14px] text-muted-foreground">
                {inline(r.summary)}
              </p>
            )}
            <div className="mt-4 flex flex-col gap-4">
              {r.sections.map((s) => {
                const Icon =
                  SECTION_ICONS[s.title.toLowerCase()] ?? SparklesIcon
                return (
                  <section key={s.title}>
                    <h4 className="flex items-center gap-1.5 text-[13px] font-semibold">
                      <Icon className="size-3.5 text-primary" aria-hidden />
                      {s.title}
                    </h4>
                    <ul className="mt-1.5 flex flex-col gap-1.5">
                      {s.items.map((item, i) => (
                        <li
                          key={i}
                          className="relative max-w-prose pl-4 text-[13px] leading-relaxed text-foreground/85"
                        >
                          <span
                            aria-hidden
                            className="absolute top-[0.6em] left-0.5 size-1.5 rounded-full bg-primary/60"
                          />
                          {inline(item)}
                        </li>
                      ))}
                    </ul>
                  </section>
                )
              })}
            </div>
          </li>
        )
      })}
    </ol>
  )
}

function NotesSkeleton() {
  return (
    <div className="flex flex-col gap-3 pl-6">
      <Skeleton className="h-6 w-32" />
      <Skeleton className="h-4 w-4/5" />
      <Skeleton className="h-4 w-3/5" />
      <Skeleton className="h-4 w-2/3" />
    </div>
  )
}

/**
 * "What's new". After an upgrade it shows what changed since `since`;
 * opened by hand it shows every release.
 */
export function WhatsNewDialog({
  open,
  onOpenChange,
  since,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  since: string | null
}) {
  const { data, error } = useChangelog(open)
  const releases = data
    ? since
      ? data.releases.filter((r) => compareVersions(r.version, since) > 0)
      : data.releases
    : []
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(44rem,88svh)] flex-col gap-0 p-0 sm:max-w-xl">
        <DialogHeader className="border-b p-5">
          <DialogTitle className="text-lg">
            {data ? `What's new in Swarmy ${data.version}` : "What's new"}
          </DialogTitle>
          <DialogDescription>
            {since
              ? `You've been upgraded from ${since}. Here's what changed.`
              : "Every Swarmy release and what it brought."}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          {error ? (
            <p className="text-[13px] text-destructive">
              Couldn't load the release notes: {error}
            </p>
          ) : data ? (
            <ReleaseNotes
              releases={releases.length ? releases : data.releases.slice(0, 1)}
              current={data.version}
            />
          ) : (
            <NotesSkeleton />
          )}
        </div>
        <DialogFooter className="m-0 border-t p-4">
          <Button onClick={() => onOpenChange(false)}>Got it</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** The full changelog, for Settings → About. */
export function ChangelogPanel() {
  const { data, error } = useChangelog()
  if (error)
    return (
      <p className="text-[13px] text-destructive">
        Couldn't load the release notes: {error}
      </p>
    )
  if (!data) return <NotesSkeleton />
  return <ReleaseNotes releases={data.releases} current={data.version} />
}
