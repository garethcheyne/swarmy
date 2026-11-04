import { useState } from "react"
import {
  ArrowDownIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  CircleAlertIcon,
  ClockIcon,
  CopyIcon,
  EllipsisIcon,
  LoaderCircleIcon,
  PauseIcon,
  PlayIcon,
  PlusIcon,
  ServerIcon,
  Trash2Icon,
} from "lucide-react"
import { toast } from "sonner"

import { api, type UsenetJob, type UsenetOverview, type UsenetStatus } from "@/lib/api"
import { formatBytes, formatEta, formatRelative, formatSpeed } from "@/lib/format"
import { settingsHref } from "@/lib/settings-sections"
import { usePoll } from "@/lib/use-poll"
import { cn } from "@/lib/utils"
import { CountUp } from "@/components/charts"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Skeleton } from "@/components/ui/skeleton"

const STATUS: Record<UsenetStatus, { label: string; busy?: boolean; tone?: "down" | "done" | "error" }> = {
  queued: { label: "Queued" },
  downloading: { label: "Downloading", tone: "down" },
  paused: { label: "Paused" },
  fetching: { label: "Fetching repair data", busy: true, tone: "down" },
  verifying: { label: "Checking", busy: true },
  repairing: { label: "Repairing", busy: true },
  extracting: { label: "Unpacking", busy: true },
  moving: { label: "Moving", busy: true },
  completed: { label: "Completed", tone: "done" },
  failed: { label: "Failed", tone: "error" },
}

function JobStatus({ status }: { status: UsenetStatus }) {
  const s = STATUS[status]
  const Icon =
    s.tone === "error" ? CircleAlertIcon : s.tone === "done" ? CheckIcon : s.busy ? LoaderCircleIcon : status === "paused" ? PauseIcon : status === "queued" ? ClockIcon : ArrowDownIcon
  return (
    <Badge
      variant={s.tone === "error" ? "destructive" : s.tone ? "default" : "outline"}
      className={cn(s.tone === "down" && "bg-honey text-honey-foreground", s.tone === "done" && "bg-up text-background")}
    >
      <Icon data-icon="inline-start" className={cn(s.busy && "motion-safe:animate-spin")} />
      {s.label}
    </Badge>
  )
}

async function run(what: string, f: () => Promise<unknown>) {
  try {
    await f()
  } catch (e) {
    toast.error(`Couldn't ${what}`, { description: e instanceof Error ? e.message : String(e) })
  }
}

function QueueRow({
  job,
  index,
  last,
  eta,
  onChanged,
  onRemove,
}: {
  job: UsenetJob
  index: number
  last: boolean
  eta: number | null
  onChanged: () => void
  onRemove: (job: UsenetJob) => void
}) {
  const progress = job.size > 0 ? Math.min(1, job.downloaded / job.size) : 0
  const postProcessing = STATUS[job.status].busy && job.status !== "fetching"
  const paused = job.status === "paused"
  return (
    <div
      role="row"
      className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-2 border-b px-1 py-3 md:grid-cols-[minmax(0,1fr)_10rem_7rem_6.5rem_auto]"
    >
      <div role="cell" className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate font-medium" title={job.name}>
          {job.name}
        </span>
        <span className="truncate text-[11px] text-muted-foreground">
          {job.category || "no category"}
          {job.message ? `, ${job.message}` : ""}
          {job.missing > 0 && !job.message ? `, ${formatBytes(job.missing)} missing` : ""}
        </span>
      </div>
      <div role="cell" className="order-last col-span-2 flex flex-col gap-1 md:order-none md:col-span-1">
        <span className="h-1 overflow-hidden rounded-sm bg-muted">
          <span
            className={cn("bar-fill block h-full", job.status === "downloading" && "bar-live")}
            style={{ width: `${Math.max(2, progress * 100)}%`, background: postProcessing ? "var(--up)" : paused ? "var(--muted-foreground)" : "var(--honey)" }}
          />
        </span>
        <span className="data text-[11px] text-muted-foreground">
          {formatBytes(job.downloaded)} of {formatBytes(job.size)}
        </span>
      </div>
      <div role="cell" className="hidden md:block">
        <JobStatus status={job.status} />
      </div>
      <div role="cell" className="data hidden text-right text-[13px] md:block">
        {job.status === "downloading" && eta != null ? formatEta(Math.max(1, Math.round(eta))) : "—"}
      </div>
      <div role="cell" className="flex items-center justify-end gap-0.5">
        <span className="md:hidden">
          <JobStatus status={job.status} />
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={paused ? `Resume ${job.name}` : `Pause ${job.name}`}
          disabled={postProcessing}
          onClick={() => run(paused ? "resume it" : "pause it", () => (paused ? api.usenetResume(job.id) : api.usenetPause(job.id)).then(onChanged))}
        >
          {paused ? <PlayIcon /> : <PauseIcon />}
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Move ${job.name} up`}
          className="hidden sm:inline-flex"
          disabled={index === 0}
          onClick={() => run("move it", () => api.usenetMove(job.id, index - 1).then(onChanged))}
        >
          <ChevronUpIcon />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Move ${job.name} down`}
          className="hidden sm:inline-flex"
          disabled={last}
          onClick={() => run("move it", () => api.usenetMove(job.id, index + 1).then(onChanged))}
        >
          <ChevronDownIcon />
        </Button>
        <Button variant="ghost" size="icon-sm" aria-label={`Remove ${job.name}`} onClick={() => onRemove(job)}>
          <Trash2Icon />
        </Button>
      </div>
    </div>
  )
}

function HistoryRow({ job, onChanged }: { job: UsenetJob; onChanged: () => void }) {
  const failed = job.status === "failed"
  return (
    <div role="row" className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 border-b px-1 py-3 md:grid-cols-[minmax(0,1fr)_7rem_6rem_7rem_auto]">
      <div role="cell" className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate font-medium" title={job.name}>
          {job.name}
        </span>
        <span className={cn("truncate text-[11px] text-muted-foreground", failed && "text-destructive")} title={job.fail_message ?? job.storage ?? undefined}>
          {failed ? job.fail_message : `${job.category || "no category"}${job.message ? `, ${job.message}` : ""}`}
        </span>
      </div>
      <div role="cell" className="hidden md:block">
        <JobStatus status={job.status} />
      </div>
      <div role="cell" className="data hidden text-right text-[13px] md:block">
        {formatBytes(job.size)}
      </div>
      <div role="cell" className="hidden text-right text-[12px] text-muted-foreground md:block">
        {job.completed ? formatRelative(job.completed) : ""}
      </div>
      <div role="cell" className="flex items-center justify-end gap-1">
        <span className="md:hidden">
          <JobStatus status={job.status} />
        </span>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" aria-label={`Actions for ${job.name}`} />}>
            <EllipsisIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuGroup>
              {job.storage && (
                <DropdownMenuItem
                  onClick={() =>
                    navigator.clipboard
                      .writeText(job.storage ?? "")
                      .then(() => toast.success("Folder copied"))
                      .catch(() => toast.error("Couldn't copy"))
                  }
                >
                  <CopyIcon />
                  Copy folder path
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onClick={() => run("remove it", () => api.usenetRemoveHistory(job.id, false).then(onChanged))}>
                <Trash2Icon />
                Remove from history
              </DropdownMenuItem>
            </DropdownMenuGroup>
            {job.storage && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem variant="destructive" onClick={() => run("delete it", () => api.usenetRemoveHistory(job.id, true).then(onChanged))}>
                    <Trash2Icon />
                    Remove and delete files
                  </DropdownMenuItem>
                </DropdownMenuGroup>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  )
}

export function UsenetPage({ onAdd }: { onAdd: () => void }) {
  const [tick, setTick] = useState(0)
  const { data } = usePoll<UsenetOverview>(api.usenet, 1500, String(tick))
  const refresh = () => setTick((t) => t + 1)
  const [removing, setRemoving] = useState<UsenetJob | null>(null)

  if (!data) {
    return (
      <div className="flex flex-col gap-3 p-4 lg:p-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-14" />
        <Skeleton className="h-14" />
      </div>
    )
  }

  const noServers = data.servers.length === 0
  const failing = data.servers.filter((s) => s.error && s.connections === 0)
  // Rough finish time for each job in order, at the current speed.
  const etas = data.queue.reduce<{ ahead: number; out: (number | null)[] }>(
    (acc, j) => {
      const ahead = acc.ahead + Math.max(0, j.size - j.downloaded)
      return { ahead, out: [...acc.out, data.speed > 0 ? ahead / data.speed : null] }
    },
    { ahead: 0, out: [] }
  ).out

  return (
    <div className="flex flex-col gap-6 p-4 pb-8 lg:p-6">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-xl font-semibold">Usenet</h1>
          <p className="text-sm text-muted-foreground">
            {data.queue.length === 0
              ? "Nothing in the queue."
              : `${data.queue.length} in the queue, ${formatBytes(data.remaining)} to go${data.paused ? ", paused" : data.speed > 0 ? ` at ${formatSpeed(data.speed)}` : ""}.`}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" onClick={() => run(data.paused ? "resume" : "pause", () => api.usenetPauseAll(!data.paused).then(refresh))}>
            {data.paused ? <PlayIcon data-icon="inline-start" /> : <PauseIcon data-icon="inline-start" />}
            {data.paused ? "Resume all" : "Pause all"}
          </Button>
          <Button onClick={onAdd}>
            <PlusIcon data-icon="inline-start" />
            Add NZB
          </Button>
        </div>
      </header>

      {noServers ? (
        <Empty className="border py-12">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ServerIcon />
            </EmptyMedia>
            <EmptyTitle>Add a Usenet server to start</EmptyTitle>
            <EmptyDescription>
              Usenet downloads come from a news server, usually a paid provider. Add its address and your login, and Swarmy can download NZBs
              and take them from Sonarr and Radarr.
            </EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button render={<a href={settingsHref("usenet")} />}>Set up a server</Button>
          </EmptyContent>
        </Empty>
      ) : (
        <>
          {failing.length > 0 && (
            <Alert variant="destructive">
              <CircleAlertIcon />
              <AlertTitle>Can't reach {failing.length === 1 ? "a Usenet server" : `${failing.length} Usenet servers`}</AlertTitle>
              <AlertDescription>
                {failing.map((s) => s.error).join(" ")} Check the server's details in{" "}
                <a href={settingsHref("usenet")} className="underline underline-offset-2">
                  Settings
                </a>
                .
              </AlertDescription>
            </Alert>
          )}

          <dl className="grid grid-cols-2 gap-x-8 gap-y-4 md:grid-cols-4">
            <Figure label="Speed" value={<CountUp value={data.speed} format={formatSpeed} />} swatch="var(--down)" />
            <Figure label="Left to download" value={<CountUp value={data.remaining} format={(v) => formatBytes(v)} />} />
            <Figure label="Connections" value={String(data.servers.reduce((n, s) => n + s.connections, 0))} />
            <Figure label="Downloaded this session" value={<CountUp value={data.servers.reduce((n, s) => n + s.downloaded, 0)} format={(v) => formatBytes(v)} />} />
          </dl>
        </>
      )}

      <section className="flex flex-col gap-2">
        <h2 className="text-[15px] font-semibold">Queue</h2>
        {data.queue.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            Empty. Add an NZB, or let Sonarr and Radarr send them: connect them as a SABnzbd client in{" "}
            <a href={settingsHref("integrations")} className="underline underline-offset-2">
              Settings
            </a>
            .
          </p>
        ) : (
          <div role="table" aria-label="Usenet queue" className="text-[13px]">
            {data.queue.map((j, i) => (
              <QueueRow key={j.id} job={j} index={i} last={i === data.queue.length - 1} eta={etas[i]} onChanged={refresh} onRemove={setRemoving} />
            ))}
          </div>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <div className="flex items-baseline justify-between gap-2">
          <h2 className="text-[15px] font-semibold">History</h2>
          {data.history.length > 0 && (
            <Button variant="ghost" size="sm" onClick={() => run("clear the history", () => api.usenetClearHistory().then(refresh))}>
              Clear history
            </Button>
          )}
        </div>
        {data.history.length === 0 ? (
          <p className="text-sm text-muted-foreground">Finished downloads appear here, with where their files went.</p>
        ) : (
          <div role="table" aria-label="Usenet history" className="text-[13px]">
            {data.history.map((j) => (
              <HistoryRow key={j.id} job={j} onChanged={refresh} />
            ))}
          </div>
        )}
      </section>

      <AlertDialog open={removing !== null} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle className="break-all">Remove {removing?.name}?</AlertDialogTitle>
            <AlertDialogDescription>It leaves the queue, and what's been downloaded of it so far is deleted.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep it</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                const j = removing
                setRemoving(null)
                if (j) run("remove it", () => api.usenetRemove(j.id).then(() => (toast.success("Removed from the queue"), refresh())))
              }}
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function Figure({ label, value, swatch }: { label: string; value: React.ReactNode; swatch?: string }) {
  return (
    <div className="flex flex-col gap-1">
      <dt className="flex items-center gap-1.5 text-sm text-muted-foreground">
        {swatch && <span className="size-2.5 rounded-[3px]" style={{ background: swatch }} />}
        {label}
      </dt>
      <dd className="data text-xl font-medium">{value}</dd>
    </div>
  )
}
