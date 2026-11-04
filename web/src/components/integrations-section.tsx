import { useState } from "react"
import {
  CheckIcon,
  CopyIcon,
  KeyRoundIcon,
  TriangleAlertIcon,
} from "lucide-react"
import { toast } from "sonner"

import { api, type Settings, type SettingsResponse } from "@/lib/api"
import { SettingHelp } from "@/components/setting-help"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from "@/components/ui/field"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"

function CopyValue({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error("Couldn't copy. Select the text and copy it instead.")
    }
  }
  return (
    <InputGroup className="h-8">
      <InputGroupInput
        readOnly
        aria-label={label}
        className="data"
        value={value}
        onFocus={(e) => e.target.select()}
      />
      <InputGroupAddon align="inline-end">
        <InputGroupButton
          size="icon-xs"
          aria-label={`Copy ${label}`}
          onClick={copy}
        >
          {copied ? <CheckIcon /> : <CopyIcon />}
        </InputGroupButton>
      </InputGroupAddon>
    </InputGroup>
  )
}

interface Props {
  draft: Settings
  set: <K extends keyof Settings>(key: K, patch: Partial<Settings[K]>) => void
  server: SettingsResponse
  onKeyChanged: (set: boolean) => void
  username: string
}

/** How to point Sonarr, Radarr and Prowlarr at Swarmy's qBittorrent-compatible API. */
export function IntegrationsSection({
  draft,
  set,
  server,
  onKeyChanged,
  username,
}: Props) {
  const [newKey, setNewKey] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const https = location.protocol === "https:"
  const port = location.port || (https ? "443" : "80")

  const createKey = async () => {
    setBusy(true)
    try {
      const r = await api.createApiKey()
      setNewKey(r.api_key)
      onKeyChanged(true)
    } catch (e) {
      toast.error("Couldn't create an API key", {
        description: e instanceof Error ? e.message : String(e),
      })
    } finally {
      setBusy(false)
    }
  }
  const revokeKey = async () => {
    setBusy(true)
    try {
      await api.deleteApiKey()
      setNewKey(null)
      onKeyChanged(false)
      toast.success("API key revoked. Apps using it can no longer connect.")
    } finally {
      setBusy(false)
    }
  }

  const rows: [string, string][] = [
    ["Host", location.hostname],
    ["Port", port],
    ["Use SSL", https ? "On" : "Off"],
    ["URL base", "Leave empty"],
    ["Username", username || "admin"],
  ]

  return (
    <div className="flex max-w-2xl flex-col gap-8">
      <section className="flex flex-col gap-5">
        <div className="flex flex-col gap-1">
          <h2 className="text-[15px] font-semibold">Sonarr and Radarr</h2>
          <p className="text-[13px] text-muted-foreground">
            Swarmy speaks the qBittorrent Web API, so Sonarr, Radarr, Prowlarr
            and anything else that supports qBittorrent can use it. It reports
            itself as qBittorrent {server.qbit_version} (Web API{" "}
            {server.qbit_webapi}).
          </p>
        </div>
        <Field orientation="horizontal">
          <FieldContent>
            <div className="flex items-center gap-1">
              <FieldLabel htmlFor="qbit-api">qBittorrent-compatible API</FieldLabel>
              <SettingHelp id="qbit_api" />
            </div>
            <FieldDescription>
              Served at /api/v2. Turn it off if nothing else needs to connect.
            </FieldDescription>
          </FieldContent>
          <Switch
            id="qbit-api"
            checked={draft.webui.qbit_api}
            onCheckedChange={(v) => set("webui", { qbit_api: v })}
          />
        </Field>
      </section>

      {draft.webui.qbit_api && (
        <>
          <section className="flex flex-col gap-4">
            <h3 className="text-[13px] font-semibold">
              Connect Sonarr or Radarr
            </h3>
            <ol className="flex list-decimal flex-col gap-4 pl-5 text-[13px] marker:text-muted-foreground">
              <li>
                In Sonarr or Radarr, open Settings, then Download Clients, and
                add qBittorrent.
              </li>
              <li>
                <div className="flex flex-col gap-3">
                  <span>Fill it in like this:</span>
                  <dl className="grid grid-cols-[7rem_minmax(0,1fr)] items-center gap-x-3 gap-y-2">
                    {rows.map(([k, v]) => (
                      <div key={k} className="contents">
                        <dt className="text-muted-foreground">{k}</dt>
                        <dd className="m-0">
                          {k === "Use SSL" || k === "URL base" ? (
                            v
                          ) : (
                            <CopyValue label={k} value={v} />
                          )}
                        </dd>
                      </div>
                    ))}
                    <dt className="text-muted-foreground">Password</dt>
                    <dd className="m-0">
                      Your Swarmy password, or leave it empty and use an API key
                    </dd>
                    <dt className="text-muted-foreground">Category</dt>
                    <dd className="m-0">
                      <span className="data">tv-sonarr</span> in Sonarr,{" "}
                      <span className="data">radarr</span> in Radarr. Sonarr
                      creates it when you test.
                    </dd>
                  </dl>
                </div>
              </li>
              <li>Choose Test, then Save.</li>
            </ol>
            <p className="text-[13px] text-muted-foreground">
              Behind a reverse proxy, use the proxy's address with SSL on, or
              point the apps straight at Swarmy on your network.
            </p>
          </section>

          <section className="flex flex-col gap-4">
            <div className="flex flex-col gap-1">
              <h3 className="text-[13px] font-semibold">API key</h3>
              <p className="text-[13px] text-muted-foreground">
                Newer Sonarr and Radarr versions can use an API key instead of
                your username and password. Paste it into their API key field.
              </p>
            </div>
            {newKey && (
              <Alert>
                <TriangleAlertIcon />
                <AlertTitle>Copy your key now</AlertTitle>
                <AlertDescription className="flex flex-col gap-2">
                  Swarmy keeps only a fingerprint of it, so it can't show this
                  key again.
                  <CopyValue label="API key" value={newKey} />
                </AlertDescription>
              </Alert>
            )}
            <div className="flex flex-wrap items-center gap-2">
              {server.api_key_set ? (
                <>
                  <span className="mr-2 flex items-center gap-1.5 text-[13px]">
                    <KeyRoundIcon className="size-4 text-down" />
                    An API key is set
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={createKey}
                    disabled={busy}
                  >
                    {busy && <Spinner data-icon="inline-start" />}
                    Replace key
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-destructive"
                    onClick={revokeKey}
                    disabled={busy}
                  >
                    Revoke key
                  </Button>
                </>
              ) : (
                <Button size="sm" onClick={createKey} disabled={busy}>
                  {busy && <Spinner data-icon="inline-start" />}
                  Create API key
                </Button>
              )}
            </div>
          </section>

          <section className="flex flex-col gap-2 text-[13px]">
            <h3 className="font-semibold">Good to know</h3>
            <ul className="flex list-disc flex-col gap-1.5 pl-5 text-muted-foreground">
              <li>
                Seeding limits from Sonarr's indexer settings are applied per
                torrent, on top of Queue and seeding.
              </li>
              <li>
                With "Remove completed" on in Sonarr, Swarmy pauses torrents at
                their seeding limit and Sonarr then removes them.
              </li>
              <li>
                If Sonarr runs in Docker with different paths, add a Remote Path
                Mapping in Sonarr for Swarmy's download folder.
              </li>
            </ul>
          </section>
        </>
      )}

      {draft.usenet.sab_api && (
        <section className="flex flex-col gap-4">
          <div className="flex flex-col gap-1">
            <h3 className="text-[13px] font-semibold">Usenet: connect them as SABnzbd</h3>
            <p className="text-[13px] text-muted-foreground">
              Swarmy also speaks SABnzbd's API, so Sonarr and Radarr can send it NZBs. Set up a Usenet server first, in Settings, Usenet.
            </p>
          </div>
          <ol className="flex list-decimal flex-col gap-4 pl-5 text-[13px] marker:text-muted-foreground">
            <li>In Sonarr or Radarr, open Settings, then Download Clients, and add SABnzbd.</li>
            <li>
              <div className="flex flex-col gap-3">
                <span>Fill it in like this:</span>
                <dl className="grid grid-cols-[7rem_minmax(0,1fr)] items-center gap-x-3 gap-y-2">
                  <dt className="text-muted-foreground">Host</dt>
                  <dd className="m-0">
                    <CopyValue label="Host" value={location.hostname} />
                  </dd>
                  <dt className="text-muted-foreground">Port</dt>
                  <dd className="m-0">
                    <CopyValue label="Port" value={port} />
                  </dd>
                  <dt className="text-muted-foreground">Use SSL</dt>
                  <dd className="m-0">{https ? "On" : "Off"}</dd>
                  <dt className="text-muted-foreground">URL base</dt>
                  <dd className="m-0">
                    <span className="data">sabnzbd</span>, or leave it empty
                  </dd>
                  <dt className="text-muted-foreground">API key</dt>
                  <dd className="m-0">The API key above (SABnzbd clients need one)</dd>
                  <dt className="text-muted-foreground">Category</dt>
                  <dd className="m-0">
                    <span className="data">tv</span> in Sonarr, <span className="data">movies</span> in Radarr. Finished downloads land in a folder named after it.
                  </dd>
                </dl>
              </div>
            </li>
            <li>Choose Test, then Save.</li>
          </ol>
        </section>
      )}
    </div>
  )
}
