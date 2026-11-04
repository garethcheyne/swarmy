import { useEffect, useState } from "react"
import { BellOffIcon, BellRingIcon, InfoIcon } from "lucide-react"
import { toast } from "sonner"

import { api, type NotifyPrefs } from "@/lib/api"
import { disablePush, enablePush, pushState, type PushState } from "@/lib/pwa"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldSet,
  FieldLegend,
} from "@/components/ui/field"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"

const EVENTS: { key: keyof NotifyPrefs; label: string; description: string }[] =
  [
    {
      key: "completed",
      label: "A download finishes",
      description: "Including ones Sonarr and Radarr asked for.",
    },
    {
      key: "errored",
      label: "A torrent hits a problem",
      description: "Disk full, missing files, tracker errors.",
    },
    {
      key: "added",
      label: "A torrent is added",
      description: "Useful to see what the *arr apps are grabbing.",
    },
  ]

/** Remount with a new `key` on each open so it re-reads the device state. */
export function NotificationsDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Notifications</DialogTitle>
          <DialogDescription>
            Get a notification on this device when something happens, even when
            Swarmy isn't open.
          </DialogDescription>
        </DialogHeader>
        <NotificationsPanel />
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function NotificationsPanel() {
  const [device, setDevice] = useState<PushState | null>(null)
  const [prefs, setPrefs] = useState<NotifyPrefs | null>(null)
  const [devices, setDevices] = useState(0)
  const [busy, setBusy] = useState<"enable" | "disable" | "test" | null>(null)

  useEffect(() => {
    let cancelled = false
    pushState()
      .then((s) => !cancelled && setDevice(s))
      .catch(() => {})
    api
      .pushInfo()
      .then((i) => {
        if (cancelled) return
        setPrefs(i.prefs)
        setDevices(i.devices)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  const savePrefs = (next: NotifyPrefs) => {
    setPrefs(next)
    api
      .pushPrefs(next)
      .catch((e) => toast.error("Couldn't save", { description: String(e) }))
  }

  const enable = async () => {
    setBusy("enable")
    try {
      const endpoint = await enablePush()
      setDevice({ kind: "on", endpoint })
      setDevices((d) => d + 1)
      toast.success("Notifications are on for this device")
    } catch (e) {
      toast.error("Couldn't turn on notifications", {
        description: e instanceof Error ? e.message : String(e),
      })
      setDevice(await pushState().catch(() => null))
    } finally {
      setBusy(null)
    }
  }

  const disable = async () => {
    setBusy("disable")
    try {
      await disablePush()
      setDevice({ kind: "off" })
      setDevices((d) => Math.max(0, d - 1))
      toast.success("Notifications are off for this device")
    } finally {
      setBusy(null)
    }
  }

  const test = async () => {
    if (device?.kind !== "on") return
    setBusy("test")
    try {
      await api.pushTest(device.endpoint)
      toast.success("Test sent. It should appear in a few seconds.")
    } catch (e) {
      toast.error("The push service refused the test", {
        description: e instanceof Error ? e.message : String(e),
      })
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="flex flex-col gap-5">
      {device === null ? (
        <div className="flex justify-center py-6">
          <Spinner />
        </div>
      ) : device.kind === "unsupported" ? (
        <Alert>
          <InfoIcon />
          <AlertTitle>Not available here</AlertTitle>
          <AlertDescription>{device.reason}</AlertDescription>
        </Alert>
      ) : device.kind === "denied" ? (
        <Alert variant="destructive">
          <BellOffIcon />
          <AlertTitle>Blocked by the browser</AlertTitle>
          <AlertDescription>
            Notifications for this site are blocked. Allow them in the browser's
            site settings, then reopen this panel.
          </AlertDescription>
        </Alert>
      ) : (
        <div className="flex items-center justify-between gap-4 rounded-lg border p-3">
          <div className="flex items-center gap-3">
            {device.kind === "on" ? (
              <BellRingIcon className="size-5 text-down" />
            ) : (
              <BellOffIcon className="size-5 text-muted-foreground" />
            )}
            <div className="flex flex-col">
              <span className="font-medium">
                {device.kind === "on"
                  ? "On for this device"
                  : "Off for this device"}
              </span>
              <span className="text-xs text-muted-foreground">
                {devices === 0
                  ? "No devices signed up yet"
                  : `${devices} device${devices === 1 ? "" : "s"} signed up`}
              </span>
            </div>
          </div>
          {device.kind === "on" ? (
            <Button
              variant="outline"
              size="sm"
              onClick={disable}
              disabled={busy !== null}
            >
              {busy === "disable" && <Spinner data-icon="inline-start" />}
              Turn off
            </Button>
          ) : (
            <Button size="sm" onClick={enable} disabled={busy !== null}>
              {busy === "enable" && <Spinner data-icon="inline-start" />}
              Turn on
            </Button>
          )}
        </div>
      )}

      {prefs && (
        <FieldSet>
          <FieldLegend variant="label">Notify me when</FieldLegend>
          <FieldGroup className="gap-4">
            {EVENTS.map((e) => (
              <Field key={e.key} orientation="horizontal">
                <FieldContent>
                  <FieldLabel htmlFor={`notify-${e.key}`}>{e.label}</FieldLabel>
                  <FieldDescription>{e.description}</FieldDescription>
                </FieldContent>
                <Switch
                  id={`notify-${e.key}`}
                  checked={prefs[e.key]}
                  onCheckedChange={(on) => savePrefs({ ...prefs, [e.key]: on })}
                />
              </Field>
            ))}
          </FieldGroup>
        </FieldSet>
      )}

      <div>
        <Button
          variant="outline"
          onClick={test}
          disabled={device?.kind !== "on" || busy !== null}
        >
          {busy === "test" && <Spinner data-icon="inline-start" />}
          Send a test notification
        </Button>
      </div>
    </div>
  )
}
