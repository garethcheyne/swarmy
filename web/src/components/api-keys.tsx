import { useEffect, useState, type FormEvent } from "react"
import { CheckIcon, CopyIcon, KeyRoundIcon, PencilIcon, TriangleAlertIcon } from "lucide-react"
import { toast } from "sonner"

import { api, type ApiKey } from "@/lib/api"
import { formatRelative } from "@/lib/format"
import { cn } from "@/lib/utils"
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
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"

const DAY = 86400
const SOON = 7 * DAY

const EXPIRY = [
  { value: "never", label: "Never" },
  { value: "30", label: "In 30 days" },
  { value: "90", label: "In 90 days" },
  { value: "365", label: "In a year" },
  { value: "date", label: "On a date…" },
]

function CopyKey({ value }: { value: string }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error("Couldn't copy. Select the key and copy it instead.")
    }
  }
  return (
    <InputGroup className="h-8">
      <InputGroupInput readOnly aria-label="New API key" className="data" value={value} onFocus={(e) => e.target.select()} />
      <InputGroupAddon align="inline-end">
        <InputGroupButton size="icon-xs" aria-label="Copy the API key" onClick={copy}>
          {copied ? <CheckIcon /> : <CopyIcon />}
        </InputGroupButton>
      </InputGroupAddon>
    </InputGroup>
  )
}

/** Tomorrow, as yyyy-mm-dd in local time: the earliest date a key can expire on. */
function tomorrow(): string {
  const d = new Date(Date.now() + DAY * 1000)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

/** Named API keys: one per app, each can expire, and each can be revoked on its own. */
export function ApiKeys() {
  const [keys, setKeys] = useState<ApiKey[] | null>(null)
  const [created, setCreated] = useState<{ name: string; key: string } | null>(null)
  const [revoking, setRevoking] = useState<ApiKey | null>(null)

  const load = () =>
    api
      .apiKeys()
      .then(setKeys)
      .catch((e) => toast.error("Couldn't load API keys", { description: e instanceof Error ? e.message : String(e) }))

  useEffect(() => {
    void load()
  }, [])

  async function revoke() {
    if (!revoking) return
    try {
      await api.revokeApiKey(revoking.id)
      toast.success(`Revoked ${revoking.name}`, { description: "Anything using that key can no longer connect." })
      if (created?.name === revoking.name) setCreated(null)
      await load()
    } catch (e) {
      toast.error("Couldn't revoke the key", { description: e instanceof Error ? e.message : String(e) })
    } finally {
      setRevoking(null)
    }
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h3 className="text-[13px] font-semibold">API keys</h3>
        <p className="text-[13px] text-muted-foreground">
          Give each app its own key, named after it. You can then see which ones are in use, and revoke one without
          breaking the others. Keys work for the qBittorrent and SABnzbd connections.
        </p>
      </div>

      {created && (
        <Alert>
          <TriangleAlertIcon />
          <AlertTitle>Copy the key for {created.name} now</AlertTitle>
          <AlertDescription className="flex flex-col gap-2">
            Swarmy keeps only a fingerprint of it, so it can't show this key again.
            <CopyKey value={created.key} />
          </AlertDescription>
        </Alert>
      )}

      {!keys ? (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-14" />
          <Skeleton className="h-14" />
        </div>
      ) : keys.length > 0 ? (
        <ul className="flex flex-col divide-y rounded-lg border">
          {keys.map((k) => (
            <KeyRow key={k.id} k={k} onChanged={load} onRevoke={() => setRevoking(k)} />
          ))}
        </ul>
      ) : null}

      <NewKey
        first={keys?.length === 0}
        onCreated={(name, key) => {
          setCreated({ name, key })
          void load()
        }}
      />

      <AlertDialog open={revoking !== null} onOpenChange={(o) => !o && setRevoking(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle className="break-all">Revoke {revoking?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              Anything using this key stops connecting straight away. Other keys keep working.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={revoke}>
              Revoke key
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  )
}

function KeyRow({ k, onChanged, onRevoke }: { k: ApiKey; onChanged: () => void; onRevoke: () => void }) {
  const [editing, setEditing] = useState(false)
  const [name, setName] = useState(k.name)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const [now] = useState(() => Date.now() / 1000)
  const soon = !k.expired && k.expires !== null && k.expires - now < SOON

  async function save(e: FormEvent) {
    e.preventDefault()
    setSaving(true)
    try {
      await api.renameApiKey(k.id, name)
      setEditing(false)
      setError(null)
      onChanged()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2.5">
      <KeyRoundIcon className={cn("size-4 shrink-0", k.expired ? "text-muted-foreground" : "text-down")} />
      {editing ? (
        <form onSubmit={save} className="flex min-w-0 flex-1 flex-col gap-1.5">
          <div className="flex gap-2">
            <Input
              autoFocus
              aria-label={`New name for ${k.name}`}
              aria-invalid={error ? true : undefined}
              className="h-8"
              maxLength={60}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
            <Button type="submit" size="sm" disabled={saving || !name.trim()}>
              {saving && <Spinner data-icon="inline-start" />}
              Save
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => {
                setEditing(false)
                setName(k.name)
                setError(null)
              }}
            >
              Cancel
            </Button>
          </div>
          {error && <p className="text-[13px] text-destructive">{error}</p>}
        </form>
      ) : (
        <>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className={cn("truncate text-[13px] font-medium", k.expired && "text-muted-foreground")}>{k.name}</span>
              {k.hint && <span className="data text-[12px] text-muted-foreground">…{k.hint}</span>}
              {k.expired && <Badge variant="destructive">Expired</Badge>}
              {soon && <Badge variant="secondary">Expires soon</Badge>}
            </div>
            <div className="flex flex-wrap gap-x-3 text-[12px] text-muted-foreground">
              <span>{k.last_used ? `Last used ${formatRelative(k.last_used)}` : "Never used"}</span>
              <span>
                {k.expires === null
                  ? "Never expires"
                  : k.expired
                    ? `Expired ${formatRelative(k.expires)}`
                    : `Expires ${formatRelative(k.expires)}`}
              </span>
              <span>Created {formatRelative(k.created)}</span>
            </div>
          </div>
          <div className="flex gap-1">
            <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>
              <PencilIcon data-icon="inline-start" />
              Rename
            </Button>
            <Button size="sm" variant="ghost" className="text-destructive" onClick={onRevoke}>
              Revoke
            </Button>
          </div>
        </>
      )}
    </li>
  )
}

function NewKey({ first, onCreated }: { first: boolean; onCreated: (name: string, key: string) => void }) {
  const [name, setName] = useState("")
  const [expiry, setExpiry] = useState("never")
  const [date, setDate] = useState("")
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function expiresAt(): number | null {
    if (expiry === "never") return null
    if (expiry === "date") return date ? Math.floor(new Date(`${date}T23:59:59`).getTime() / 1000) : null
    return Math.floor(Date.now() / 1000) + Number(expiry) * DAY
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!name.trim() || (expiry === "date" && !date)) return
    setPending(true)
    try {
      const r = await api.createApiKey(name, expiresAt())
      onCreated(r.name, r.key)
      setName("")
      setExpiry("never")
      setDate("")
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setPending(false)
    }
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-3 rounded-lg border border-dashed p-3">
      <span className="text-[13px] font-medium">{first ? "Make your first key" : "Make another key"}</span>
      <FieldGroup className="gap-3 sm:flex-row sm:items-end">
        <Field data-invalid={error ? true : undefined} className="sm:flex-1">
          <FieldLabel htmlFor="key-name">What it's for</FieldLabel>
          <Input
            id="key-name"
            className="h-8"
            placeholder="Sonarr"
            maxLength={60}
            aria-invalid={error ? true : undefined}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field className="sm:w-40">
          <FieldLabel htmlFor="key-expiry">Expires</FieldLabel>
          <Select value={expiry} onValueChange={(v) => v && setExpiry(v)} items={EXPIRY}>
            <SelectTrigger id="key-expiry" className="h-8 w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {EXPIRY.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </Field>
        {expiry === "date" && (
          <Field className="sm:w-40">
            <FieldLabel htmlFor="key-date">Last day</FieldLabel>
            <Input id="key-date" type="date" className="h-8" min={tomorrow()} value={date} onChange={(e) => setDate(e.target.value)} />
          </Field>
        )}
        <Button type="submit" size="sm" className="h-8" disabled={pending || !name.trim() || (expiry === "date" && !date)}>
          {pending && <Spinner data-icon="inline-start" />}
          Create key
        </Button>
      </FieldGroup>
      {error && <FieldError>{error}</FieldError>}
    </form>
  )
}
