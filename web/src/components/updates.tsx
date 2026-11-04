import { useEffect, useRef, useState } from "react"
import { CircleAlertIcon, CircleCheckIcon, DownloadIcon, ExternalLinkIcon, RefreshCwIcon } from "lucide-react"
import { toast } from "sonner"

import { api, type UpdateStatus } from "@/lib/api"
import { formatRelative } from "@/lib/format"
import { ReleaseNotes } from "@/components/release-notes"
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
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Progress } from "@/components/ui/progress"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"

const INSTALLER = "curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh | sudo bash"
const DOCKER = "docker compose pull && docker compose up -d"
/** Downloading, checking and restarting takes well under this. */
const GIVE_UP_AFTER = 5 * 60 * 1000

function Command({ children }: { children: string }) {
  return <pre className="data overflow-x-auto rounded-md border bg-muted/40 px-3 py-2 text-[12px] whitespace-pre-wrap [overflow-wrap:anywhere]">{children}</pre>
}

function errorText(e: unknown) {
  return e instanceof Error ? e.message : String(e)
}

/** Settings → About: is there a newer Swarmy, what's in it, and installing it. */
export function UpdatesPanel() {
  const [status, setStatus] = useState<UpdateStatus | null>(null)
  const [checking, setChecking] = useState(false)
  const [confirm, setConfirm] = useState(false)
  const [installing, setInstalling] = useState<{ from: string; to: string; lastRunAt: number | null } | null>(null)

  useEffect(() => {
    api.update().then(setStatus).catch(() => {})
  }, [])

  async function checkNow() {
    setChecking(true)
    try {
      setStatus(await api.checkUpdate())
    } catch (e) {
      toast.error("Couldn't check for updates", { description: errorText(e) })
    } finally {
      setChecking(false)
    }
  }

  async function install() {
    if (!status?.latest) return
    try {
      const s = await api.installUpdate()
      setStatus(s)
      setInstalling({ from: s.current, to: status.latest.version, lastRunAt: status.last_run?.at ?? null })
    } catch (e) {
      toast.error("Couldn't start the update", { description: errorText(e) })
    }
  }

  if (!status) return <Skeleton className="h-24" />

  const latest = status.latest
  const run = status.last_run
  const failed = run && !run.running && !run.ok && !status.pending ? run : null

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start gap-x-4 gap-y-3">
        {latest ? (
          <DownloadIcon className="mt-0.5 size-5 shrink-0 text-primary" />
        ) : (
          <CircleCheckIcon className="mt-0.5 size-5 shrink-0 text-up" />
        )}
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <p className="text-[14px] font-medium">
            {latest ? `Swarmy ${latest.version} is available` : `You have the newest Swarmy, ${status.current}`}
          </p>
          <p className="text-[13px] text-muted-foreground">
            {latest && `You have ${status.current}. `}
            {status.checked_at ? `Checked ${formatRelative(status.checked_at)}.` : "Not checked yet."}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={checkNow} disabled={checking}>
            {checking ? <Spinner data-icon="inline-start" /> : <RefreshCwIcon data-icon="inline-start" />}
            Check now
          </Button>
          {latest && status.method === "installer" && (
            <Button size="sm" onClick={() => setConfirm(true)} disabled={status.pending}>
              {status.pending && <Spinner data-icon="inline-start" />}
              Install {latest.version}
            </Button>
          )}
        </div>
      </div>

      {status.error && (
        <Alert>
          <CircleAlertIcon />
          <AlertDescription className="[overflow-wrap:anywhere]">{status.error}</AlertDescription>
        </Alert>
      )}

      {status.stuck && (
        <Alert variant="destructive">
          <CircleAlertIcon />
          <AlertTitle>The update hasn't started</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            Swarmy asked for it, but the updater on the server didn't pick the request up. On the server, check it's
            running:
            <Command>systemctl status swarmy-update.path</Command>
          </AlertDescription>
        </Alert>
      )}

      {failed && (
        <Alert variant="destructive">
          <CircleAlertIcon />
          <AlertTitle>The last update didn't finish ({formatRelative(failed.at)})</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            Swarmy is still on {status.current}. What the updater said:
            <Command>{failed.log}</Command>
          </AlertDescription>
        </Alert>
      )}

      {latest && status.method === "docker" && (
        <div className="flex flex-col gap-2 text-[13px]">
          <p>Swarmy runs in Docker, so update it by pulling the new image, next to your compose file:</p>
          <Command>{DOCKER}</Command>
        </div>
      )}
      {latest && status.method === "manual" && (
        <div className="flex flex-col gap-2 text-[13px]">
          <p>
            Run the Linux installer again to update. It keeps your settings and downloads, and turns on one-click
            updates here, so next time you can install from this page.
          </p>
          <Command>{INSTALLER}</Command>
          <p className="text-muted-foreground">
            On Proxmox, run it inside the container: <span className="data">pct exec &lt;id&gt; -- bash -c "…"</span>, or
            from the container's console.
          </p>
        </div>
      )}

      {latest && status.notes.length > 0 && (
        <div className="flex flex-col gap-4 rounded-lg border p-4">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <h3 className="text-[13px] font-semibold">What's new since {status.current}</h3>
            <a
              href={latest.url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-[13px] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
            >
              On GitHub
              <ExternalLinkIcon className="size-3.5" />
            </a>
          </div>
          <ReleaseNotes releases={status.notes} current={status.current} />
        </div>
      )}

      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Install Swarmy {latest?.version}?</AlertDialogTitle>
            <AlertDialogDescription>
              Swarmy downloads it, checks it, installs it and restarts. Torrents and Usenet downloads stop for a few
              seconds and then carry on where they were. This page reloads by itself when it's done.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Not now</AlertDialogCancel>
            <AlertDialogAction onClick={install}>Install and restart</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {installing && (
        <InstallProgress
          {...installing}
          onClose={(s) => {
            setInstalling(null)
            if (s) setStatus(s)
          }}
        />
      )}
    </div>
  )
}

type Phase = "waiting" | "restarting" | "done" | "failed"

/** Follows an update: waits for Swarmy to go away and come back newer, then reloads. */
function InstallProgress({
  from,
  to,
  lastRunAt,
  onClose,
}: {
  from: string
  to: string
  lastRunAt: number | null
  onClose: (status: UpdateStatus | null) => void
}) {
  const [phase, setPhase] = useState<Phase>("waiting")
  const [failure, setFailure] = useState<{ text: string; log?: string } | null>(null)
  const latestStatus = useRef<UpdateStatus | null>(null)

  useEffect(() => {
    const started = Date.now()
    let stop = false
    let wentAway = false
    const tick = async () => {
      if (stop) return
      try {
        const info = await api.info()
        if (info.version !== from) {
          stop = true
          setPhase("done")
          setTimeout(() => location.reload(), 1200)
          return
        }
        if (wentAway) setPhase("restarting")
        const s = await api.update()
        latestStatus.current = s
        if (s.stuck) {
          stop = true
          setPhase("failed")
          setFailure({ text: "The updater on the server didn't start. Check it with: systemctl status swarmy-update.path" })
          return
        }
        const run = s.last_run
        if (!s.pending && run && !run.running && !run.ok && run.at !== lastRunAt) {
          stop = true
          setPhase("failed")
          setFailure({ text: `Swarmy is still on ${from}. What the updater said:`, log: run.log })
          return
        }
      } catch {
        // Swarmy is restarting.
        wentAway = true
        setPhase("restarting")
      }
      if (Date.now() - started > GIVE_UP_AFTER) {
        stop = true
        setPhase("failed")
        setFailure({ text: "This is taking much longer than it should. Check the updater's log on the server: /var/log/swarmy-update.log" })
        return
      }
      setTimeout(tick, 2000)
    }
    const first = setTimeout(tick, 1500)
    return () => {
      stop = true
      clearTimeout(first)
    }
  }, [from, lastRunAt])

  const busy = phase === "waiting" || phase === "restarting"
  return (
    <Dialog open onOpenChange={(o) => !o && onClose(latestStatus.current)}>
      <DialogContent className="sm:max-w-lg" showCloseButton={!busy}>
        <DialogHeader>
          <DialogTitle>
            {phase === "done" ? `Swarmy ${to} is installed` : phase === "failed" ? "The update didn't finish" : `Updating to Swarmy ${to}`}
          </DialogTitle>
          <DialogDescription>
            {phase === "waiting" && "Downloading and checking the new version."}
            {phase === "restarting" && "Installing and restarting. Swarmy is away for a moment."}
            {phase === "done" && "Reloading this page."}
            {phase === "failed" && failure?.text}
          </DialogDescription>
        </DialogHeader>
        {busy || phase === "done" ? (
          <Progress value={phase === "done" ? 100 : phase === "restarting" ? 66 : 33} aria-label="Update progress" />
        ) : (
          failure?.log && <Command>{failure.log}</Command>
        )}
        {phase === "failed" && (
          <DialogFooter>
            <Button onClick={() => onClose(latestStatus.current)}>Close</Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  )
}
