import { useEffect, useMemo, useState, type ReactNode } from "react"
import {
  BellIcon,
  DownloadIcon,
  GaugeIcon,
  InfoIcon,
  ListOrderedIcon,
  LockIcon,
  NetworkIcon,
  PaletteIcon,
  PlugIcon,
  PlusIcon,
  RotateCwIcon,
  ShieldIcon,
  TagIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react"
import { toast } from "sonner"

import { api, UnauthorizedError, type Category, type ServerInfo, type Settings, type SettingsResponse } from "@/lib/api"
import { usePoll } from "@/lib/use-poll"
import { cn } from "@/lib/utils"
import { ChangelogPanel } from "@/components/release-notes"
import { AppearancePanel } from "@/components/appearance-dialog"
import { IntegrationsSection } from "@/components/integrations-section"
import { NotificationsPanel } from "@/components/notifications-dialog"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldSeparator,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { InputGroup, InputGroupAddon, InputGroupInput, InputGroupText } from "@/components/ui/input-group"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const SECTIONS = [
  { id: "downloads", label: "Downloads", icon: DownloadIcon },
  { id: "integrations", label: "Sonarr and Radarr", icon: PlugIcon },
  { id: "connection", label: "Connection", icon: NetworkIcon },
  { id: "speed", label: "Speed", icon: GaugeIcon },
  { id: "queue", label: "Queue and seeding", icon: ListOrderedIcon },
  { id: "categories", label: "Categories and tags", icon: TagIcon },
  { id: "security", label: "Web UI and security", icon: ShieldIcon },
  { id: "notifications", label: "Notifications", icon: BellIcon },
  { id: "appearance", label: "Appearance", icon: PaletteIcon },
  { id: "about", label: "About", icon: InfoIcon },
] as const
type SectionId = (typeof SECTIONS)[number]["id"]

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

const toTime = (mins: number) => `${String(Math.floor(mins / 60)).padStart(2, "0")}:${String(mins % 60).padStart(2, "0")}`
const fromTime = (v: string) => {
  const [h, m] = v.split(":").map(Number)
  return (h || 0) * 60 + (m || 0)
}

function errorText(e: unknown) {
  return e instanceof Error ? e.message : String(e)
}

// ---------------------------------------------------------------- layout bits

function Section({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex max-w-2xl flex-col gap-5">
      <div className="flex flex-col gap-1">
        <h2 className="text-[15px] font-semibold">{title}</h2>
        {description && <p className="text-[13px] text-muted-foreground">{description}</p>}
      </div>
      {children}
    </section>
  )
}

function Locked({ on }: { on: boolean }) {
  if (!on) return null
  return (
    <Badge variant="outline" className="ml-2 align-middle">
      <LockIcon data-icon="inline-start" />
      Set by a command-line flag
    </Badge>
  )
}

function SwitchRow({
  id,
  label,
  description,
  checked,
  onChange,
  locked = false,
}: {
  id: string
  label: string
  description?: string
  checked: boolean
  onChange: (v: boolean) => void
  locked?: boolean
}) {
  return (
    <Field orientation="horizontal" data-disabled={locked || undefined}>
      <FieldContent>
        <FieldLabel htmlFor={id}>
          {label}
          <Locked on={locked} />
        </FieldLabel>
        {description && <FieldDescription>{description}</FieldDescription>}
      </FieldContent>
      <Switch id={id} checked={checked} onCheckedChange={onChange} disabled={locked} />
    </Field>
  )
}

function NumberInput({
  id,
  value,
  onChange,
  unit,
  min = 0,
  max,
  step = 1,
  disabled,
  className,
}: {
  id: string
  value: number
  onChange: (v: number) => void
  unit?: string
  min?: number
  max?: number
  step?: number
  disabled?: boolean
  className?: string
}) {
  return (
    <InputGroup className={cn("w-40", className)} data-disabled={disabled || undefined}>
      <InputGroupInput
        id={id}
        type="number"
        inputMode="decimal"
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        className="data"
        value={Number.isFinite(value) ? value : 0}
        onChange={(e) => onChange(e.target.value === "" ? 0 : Number(e.target.value))}
      />
      {unit && (
        <InputGroupAddon align="inline-end">
          <InputGroupText>{unit}</InputGroupText>
        </InputGroupAddon>
      )}
    </InputGroup>
  )
}

// ---------------------------------------------------------------- page

export function SettingsPage({ onSignedOut }: { onSignedOut: () => void }) {
  const [section, setSection] = useState<SectionId>(() => {
    const s = new URLSearchParams(location.hash.split("?")[1] ?? "").get("section")
    return SECTIONS.some((x) => x.id === s) ? (s as SectionId) : "downloads"
  })
  const [server, setServer] = useState<SettingsResponse | null>(null)
  const [draft, setDraft] = useState<Settings | null>(null)
  const [saving, setSaving] = useState(false)
  const [username, setUsername] = useState("")

  useEffect(() => {
    api.me().then((m) => setUsername(m.username ?? "")).catch(() => {})
  }, [])

  useEffect(() => {
    api
      .settings()
      .then((r) => {
        setServer(r)
        setDraft(r.settings)
      })
      .catch((e) => (e instanceof UnauthorizedError ? onSignedOut() : toast.error("Couldn't load settings", { description: errorText(e) })))
  }, [onSignedOut])

  const dirty = useMemo(
    () => !!server && !!draft && JSON.stringify(server.settings) !== JSON.stringify(draft),
    [server, draft]
  )

  const save = async () => {
    if (!draft) return
    setSaving(true)
    try {
      const r = await api.saveSettings(draft)
      setServer((s) => (s ? { ...s, settings: draft, restart_needed: r.restart_needed } : s))
      toast.success("Settings saved")
    } catch (e) {
      if (e instanceof UnauthorizedError) return onSignedOut()
      toast.error("Couldn't save settings", { description: errorText(e) })
    } finally {
      setSaving(false)
    }
  }

  /** Update one nested group of the draft. */
  const set = <K extends keyof Settings>(key: K, patch: Partial<Settings[K]>) =>
    setDraft((d) => (d ? { ...d, [key]: { ...d[key], ...patch } } : d))

  const locked = (f: string) => !!server?.locked.includes(f)

  return (
    <div className="flex h-full min-h-0 flex-col md:flex-row">
      <nav aria-label="Settings sections" className="shrink-0 overflow-x-auto border-b md:w-56 md:overflow-y-auto md:border-r md:border-b-0">
        <div className="flex gap-1 p-2 md:flex-col md:p-3">
          <h1 className="hidden px-2.5 pt-1 pb-2 text-[15px] font-semibold md:block">Settings</h1>
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              aria-current={section === s.id ? "page" : undefined}
              onClick={() => setSection(s.id)}
              className={cn(
                "flex min-h-9 shrink-0 items-center gap-2 rounded-md px-2.5 text-left text-[13px] whitespace-nowrap text-muted-foreground hover:bg-muted hover:text-foreground",
                section === s.id && "bg-muted text-foreground"
              )}
            >
              <s.icon className="size-4" />
              {s.label}
            </button>
          ))}
        </div>
      </nav>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="min-h-0 flex-1 overflow-y-auto p-4 pb-24 lg:p-6">
          {!draft || !server ? (
            <div className="flex max-w-2xl flex-col gap-3">
              <Skeleton className="h-6 w-48" />
              <Skeleton className="h-10" />
              <Skeleton className="h-10" />
              <Skeleton className="h-10" />
            </div>
          ) : (
            <>
              {section === "downloads" && (
                <Section title="Downloads" description="Where new torrents go, and how they start.">
                  <FieldGroup>
                    <Field data-disabled={locked("downloads.default_save_path") || undefined}>
                      <FieldLabel htmlFor="save-path">
                        Default save folder
                        <Locked on={locked("downloads.default_save_path")} />
                      </FieldLabel>
                      <Input
                        id="save-path"
                        className="data"
                        placeholder={server.effective_download_dir}
                        disabled={locked("downloads.default_save_path")}
                        value={draft.downloads.default_save_path}
                        onChange={(e) => set("downloads", { default_save_path: e.target.value })}
                      />
                      <FieldDescription>
                        Torrents with a category go in a subfolder named after it, unless the category has its own folder. Leave empty to use {server.effective_download_dir}.
                      </FieldDescription>
                    </Field>
                    <SwitchRow
                      id="add-paused"
                      label="Add torrents paused"
                      description="New torrents wait until you start them. Sonarr and Radarr follow this too."
                      checked={draft.downloads.add_paused}
                      onChange={(v) => set("downloads", { add_paused: v })}
                    />
                  </FieldGroup>
                </Section>
              )}

              {section === "integrations" && (
                <IntegrationsSection
                  draft={draft}
                  set={set}
                  server={server}
                  username={username}
                  onKeyChanged={(v) => setServer((s) => (s ? { ...s, api_key_set: v } : s))}
                />
              )}

              {section === "connection" && (
                <Section title="Connection" description="How Swarmy reaches peers. Changes here take effect the next time Swarmy starts.">
                  {server.restart_needed && (
                    <Alert>
                      <RotateCwIcon />
                      <AlertTitle>Restart Swarmy to apply</AlertTitle>
                      <AlertDescription>Your connection changes are saved and will be used after the next restart.</AlertDescription>
                    </Alert>
                  )}
                  <FieldGroup>
                    <Field data-disabled={locked("connection.listen_port") || undefined}>
                      <FieldLabel htmlFor="port">
                        Listening port
                        <Locked on={locked("connection.listen_port")} />
                      </FieldLabel>
                      <NumberInput
                        id="port"
                        min={1024}
                        max={65535}
                        disabled={locked("connection.listen_port")}
                        value={draft.connection.listen_port}
                        onChange={(v) => set("connection", { listen_port: Math.round(v) })}
                      />
                      <FieldDescription>For incoming peers, over TCP and uTP. Forward this port on your router for the best speeds.</FieldDescription>
                    </Field>
                    <SwitchRow id="upnp" label="Forward the port with UPnP" description="Asks your router to open the port automatically." checked={draft.connection.upnp} onChange={(v) => set("connection", { upnp: v })} locked={locked("connection.upnp")} />
                    <Field data-disabled={locked("connection.bind_interface") || undefined}>
                      <FieldLabel htmlFor="iface">
                        Network interface
                        <Locked on={locked("connection.bind_interface")} />
                      </FieldLabel>
                      <Input
                        id="iface"
                        className="data w-48"
                        placeholder="Any, e.g. wg0"
                        disabled={locked("connection.bind_interface")}
                        value={draft.connection.bind_interface}
                        onChange={(e) => set("connection", { bind_interface: e.target.value })}
                      />
                      <FieldDescription>Bind all torrent traffic to a VPN interface. If the VPN drops, torrent traffic stops instead of leaking.</FieldDescription>
                    </Field>
                    <SwitchRow id="ipv4" label="IPv4 only" checked={draft.connection.ipv4_only} onChange={(v) => set("connection", { ipv4_only: v })} locked={locked("connection.ipv4_only")} />
                    <FieldSeparator />
                    <SwitchRow id="dht" label="DHT" description="Find peers without a tracker. Needed for magnet links." checked={draft.connection.dht} onChange={(v) => set("connection", { dht: v })} />
                    <SwitchRow id="lsd" label="Local peer discovery" description="Find peers on your own network." checked={draft.connection.lsd} onChange={(v) => set("connection", { lsd: v })} />
                    <Field>
                      <FieldLabel htmlFor="peers">Maximum peers per torrent</FieldLabel>
                      <NumberInput id="peers" value={draft.connection.peer_limit} onChange={(v) => set("connection", { peer_limit: Math.max(0, Math.round(v)) })} />
                      <FieldDescription>0 uses the engine's default.</FieldDescription>
                    </Field>
                    <FieldSeparator />
                    <Field>
                      <FieldLabel htmlFor="proxy">SOCKS5 proxy for peers</FieldLabel>
                      <Input id="proxy" className="data" placeholder="socks5://user:password@host:1080" value={draft.connection.socks_proxy} onChange={(e) => set("connection", { socks_proxy: e.target.value })} />
                      <FieldDescription>Outgoing peer connections go through this proxy. Leave empty for none.</FieldDescription>
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="blocklist">IP blocklist URL</FieldLabel>
                      <Input id="blocklist" className="data" placeholder="https://example.org/blocklist.p2p.gz" value={draft.connection.blocklist_url} onChange={(e) => set("connection", { blocklist_url: e.target.value })} />
                      <FieldDescription>A P2P or DAT format list, gzipped or plain. Peers on it are refused.</FieldDescription>
                    </Field>
                  </FieldGroup>
                </Section>
              )}

              {section === "speed" && (
                <Section title="Speed" description="Limits apply to all torrents. Use 0 for no limit.">
                  <FieldGroup>
                    <div className="grid gap-4 sm:grid-cols-2">
                      <Field>
                        <FieldLabel htmlFor="dl">Download limit</FieldLabel>
                        <NumberInput id="dl" unit="KiB/s" value={draft.speed.download_limit} onChange={(v) => set("speed", { download_limit: Math.max(0, Math.round(v)) })} />
                      </Field>
                      <Field>
                        <FieldLabel htmlFor="ul">Upload limit</FieldLabel>
                        <NumberInput id="ul" unit="KiB/s" value={draft.speed.upload_limit} onChange={(v) => set("speed", { upload_limit: Math.max(0, Math.round(v)) })} />
                      </Field>
                    </div>
                    <FieldSeparator />
                    <div className="flex flex-col gap-1">
                      <h3 className="text-[13px] font-semibold">Alternative limits</h3>
                      <p className="text-[13px] text-muted-foreground">A second set of limits for when you need the bandwidth, switched by hand (the turtle in the top bar) or on a schedule.</p>
                    </div>
                    <div className="grid gap-4 sm:grid-cols-2">
                      <Field>
                        <FieldLabel htmlFor="adl">Download limit</FieldLabel>
                        <NumberInput id="adl" unit="KiB/s" value={draft.speed.alt_download_limit} onChange={(v) => set("speed", { alt_download_limit: Math.max(0, Math.round(v)) })} />
                      </Field>
                      <Field>
                        <FieldLabel htmlFor="aul">Upload limit</FieldLabel>
                        <NumberInput id="aul" unit="KiB/s" value={draft.speed.alt_upload_limit} onChange={(v) => set("speed", { alt_upload_limit: Math.max(0, Math.round(v)) })} />
                      </Field>
                    </div>
                    <SwitchRow id="alt-now" label="Use alternative limits now" checked={draft.speed.alt_enabled} onChange={(v) => set("speed", { alt_enabled: v })} />
                    <SwitchRow
                      id="sched"
                      label="Switch on automatically"
                      description="Uses the server's local time."
                      checked={draft.speed.schedule.enabled}
                      onChange={(v) => set("speed", { schedule: { ...draft.speed.schedule, enabled: v } })}
                    />
                    {draft.speed.schedule.enabled && (
                      <div className="flex flex-col gap-4 rounded-lg border p-4">
                        <div className="flex flex-wrap items-end gap-4">
                          <Field className="w-auto">
                            <FieldLabel htmlFor="from">From</FieldLabel>
                            <Input id="from" type="time" className="data w-32" value={toTime(draft.speed.schedule.from)} onChange={(e) => set("speed", { schedule: { ...draft.speed.schedule, from: fromTime(e.target.value) } })} />
                          </Field>
                          <Field className="w-auto">
                            <FieldLabel htmlFor="to">Until</FieldLabel>
                            <Input id="to" type="time" className="data w-32" value={toTime(draft.speed.schedule.to)} onChange={(e) => set("speed", { schedule: { ...draft.speed.schedule, to: fromTime(e.target.value) } })} />
                          </Field>
                        </div>
                        <Field>
                          <FieldLabel id="days-label">On these days</FieldLabel>
                          <ToggleGroup
                            multiple
                            variant="outline"
                            size="sm"
                            spacing={1}
                            className="flex-wrap"
                            aria-labelledby="days-label"
                            value={draft.speed.schedule.days.map(String)}
                            onValueChange={(v: string[]) => set("speed", { schedule: { ...draft.speed.schedule, days: v.map(Number).sort() } })}
                          >
                            {DAYS.map((d, i) => (
                              <ToggleGroupItem
                                key={d}
                                value={String(i)}
                                className="data-pressed:border-primary data-pressed:bg-primary data-pressed:text-primary-foreground"
                              >
                                {d}
                              </ToggleGroupItem>
                            ))}
                          </ToggleGroup>
                          <FieldDescription>
                            {draft.speed.schedule.from > draft.speed.schedule.to ? "Runs overnight, ending the next morning." : "Runs within the same day."}
                          </FieldDescription>
                        </Field>
                      </div>
                    )}
                  </FieldGroup>
                </Section>
              )}

              {section === "queue" && (
                <Section title="Queue and seeding" description="Limit how many torrents run at once, and when to stop seeding.">
                  <FieldGroup>
                    <SwitchRow id="queue" label="Queue torrents" description="Torrents over the limits wait, then start when a slot frees up. Oldest first." checked={draft.queue.enabled} onChange={(v) => set("queue", { enabled: v })} />
                    {draft.queue.enabled && (
                      <div className="grid gap-4 sm:grid-cols-3">
                        <Field>
                          <FieldLabel htmlFor="qd">Active downloads</FieldLabel>
                          <NumberInput id="qd" className="w-full" value={draft.queue.max_downloads} onChange={(v) => set("queue", { max_downloads: Math.max(0, Math.round(v)) })} />
                        </Field>
                        <Field>
                          <FieldLabel htmlFor="qu">Active uploads</FieldLabel>
                          <NumberInput id="qu" className="w-full" value={draft.queue.max_uploads} onChange={(v) => set("queue", { max_uploads: Math.max(0, Math.round(v)) })} />
                        </Field>
                        <Field>
                          <FieldLabel htmlFor="qa">Active in total</FieldLabel>
                          <NumberInput id="qa" className="w-full" min={1} value={draft.queue.max_active} onChange={(v) => set("queue", { max_active: Math.max(1, Math.round(v)) })} />
                        </Field>
                      </div>
                    )}
                    <FieldSeparator />
                    <div className="flex flex-col gap-1">
                      <h3 className="text-[13px] font-semibold">Seeding limits</h3>
                      <p className="text-[13px] text-muted-foreground">Sonarr and Radarr check these to know when a download can be cleaned up.</p>
                    </div>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="ratio-on"
                        checked={draft.seeding.ratio_limit !== null}
                        onCheckedChange={(on) => set("seeding", { ratio_limit: on ? 2 : null })}
                      />
                      <FieldLabel htmlFor="ratio-on" className="w-44">Stop at a share ratio of</FieldLabel>
                      <NumberInput id="ratio" step={0.1} className="w-28" disabled={draft.seeding.ratio_limit === null} value={draft.seeding.ratio_limit ?? 2} onChange={(v) => set("seeding", { ratio_limit: Math.max(0, v) })} />
                    </Field>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="time-on"
                        checked={draft.seeding.time_limit !== null}
                        onCheckedChange={(on) => set("seeding", { time_limit: on ? 1440 : null })}
                      />
                      <FieldLabel htmlFor="time-on" className="w-44">Stop after seeding for</FieldLabel>
                      <NumberInput id="seedtime" unit="min" className="w-36" disabled={draft.seeding.time_limit === null} value={draft.seeding.time_limit ?? 1440} onChange={(v) => set("seeding", { time_limit: Math.max(1, Math.round(v)) })} />
                    </Field>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="idle-on"
                        checked={draft.seeding.inactive_time_limit !== null}
                        onCheckedChange={(on) => set("seeding", { inactive_time_limit: on ? 720 : null })}
                      />
                      <FieldLabel htmlFor="idle-on" className="w-44">Stop after no activity for</FieldLabel>
                      <NumberInput id="idletime" unit="min" className="w-36" disabled={draft.seeding.inactive_time_limit === null} value={draft.seeding.inactive_time_limit ?? 720} onChange={(v) => set("seeding", { inactive_time_limit: Math.max(1, Math.round(v)) })} />
                    </Field>
                    <Field>
                      <FieldLabel>Then</FieldLabel>
                      <Select
                        value={draft.seeding.action}
                        onValueChange={(v) => v && set("seeding", { action: v as Settings["seeding"]["action"] })}
                        items={[
                          { value: "pause", label: "Pause the torrent" },
                          { value: "remove", label: "Remove the torrent, keep its files" },
                          { value: "remove_with_files", label: "Remove the torrent and its files" },
                        ]}
                      >
                        <SelectTrigger className="w-72">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="pause">Pause the torrent</SelectItem>
                            <SelectItem value="remove">Remove the torrent, keep its files</SelectItem>
                            <SelectItem value="remove_with_files">Remove the torrent and its files</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </Field>
                  </FieldGroup>
                </Section>
              )}

              {section === "categories" && <CategoriesAndTags />}

              {section === "security" && (
                <SecuritySection
                  draft={draft}
                  set={set}
                  sessions={server.sessions}
                  onSessionsChanged={(n) => setServer((s) => (s ? { ...s, sessions: n } : s))}
                />
              )}

              {section === "notifications" && (
                <Section title="Notifications" description="Get a notification on this device when something happens, even when Swarmy isn't open.">
                  <NotificationsPanel />
                </Section>
              )}

              {section === "appearance" && (
                <Section title="Appearance" description="Saved in this browser, so each device can have its own look.">
                  <AppearancePanel />
                </Section>
              )}

              {section === "about" && <About server={server} />}
            </>
          )}
        </div>

        {dirty && (
          <div className="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t bg-card px-4 py-2.5 pb-[max(0.625rem,env(safe-area-inset-bottom))]">
            <span className="mr-auto text-[13px] text-muted-foreground">You have unsaved changes.</span>
            <Button variant="outline" size="sm" onClick={() => setDraft(server!.settings)} disabled={saving}>
              Discard
            </Button>
            <Button size="sm" onClick={save} disabled={saving}>
              {saving && <Spinner data-icon="inline-start" />}
              Save changes
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}

// ---------------------------------------------------------------- categories and tags

function CategoriesAndTags() {
  const [cats, setCats] = useState<Record<string, Category> | null>(null)
  const [tags, setTags] = useState<string[] | null>(null)
  const [newCat, setNewCat] = useState({ name: "", path: "" })
  const [newTag, setNewTag] = useState("")

  const load = () => {
    api.categories().then(setCats).catch(() => {})
    api.tags().then(setTags).catch(() => {})
  }
  useEffect(load, [])

  const run = async (label: string, fn: () => Promise<void>) => {
    try {
      await fn()
      load()
    } catch (e) {
      toast.error(`Couldn't ${label}`, { description: errorText(e) })
    }
  }

  return (
    <div className="flex flex-col gap-10">
      <Section title="Categories" description="Sonarr and Radarr each use a category (for example tv-sonarr and radarr). Give a category its own folder, or leave it empty to use a subfolder of the default save folder.">
        <ul className="flex flex-col divide-y rounded-lg border">
          {cats === null ? (
            <li className="p-3"><Skeleton className="h-8" /></li>
          ) : Object.keys(cats).length === 0 ? (
            <li className="p-3 text-[13px] text-muted-foreground">No categories yet.</li>
          ) : (
            Object.entries(cats).map(([name, c]) => (
              <CategoryRow
                key={name}
                name={name}
                path={c.save_path}
                onSave={(p) => run("save the category", () => api.putCategory(name, { save_path: p }))}
                onDelete={() => run("delete the category", () => api.deleteCategory(name))}
              />
            ))
          )}
        </ul>
        <form
          className="flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            if (!newCat.name.trim()) return
            run("add the category", async () => {
              await api.putCategory(newCat.name.trim(), { save_path: newCat.path.trim() })
              setNewCat({ name: "", path: "" })
            })
          }}
        >
          <Field className="w-44">
            <FieldLabel htmlFor="cat-name">New category</FieldLabel>
            <Input id="cat-name" placeholder="movies" value={newCat.name} onChange={(e) => setNewCat({ ...newCat, name: e.target.value })} />
          </Field>
          <Field className="min-w-48 flex-1">
            <FieldLabel htmlFor="cat-path">Folder (optional)</FieldLabel>
            <Input id="cat-path" className="data" placeholder="/data/movies" value={newCat.path} onChange={(e) => setNewCat({ ...newCat, path: e.target.value })} />
          </Field>
          <Button type="submit" disabled={!newCat.name.trim()}>
            <PlusIcon data-icon="inline-start" />
            Add category
          </Button>
        </form>
      </Section>

      <Section title="Tags" description="Free-form labels. A torrent can have several.">
        <div className="flex flex-wrap gap-2">
          {tags === null ? (
            <Skeleton className="h-7 w-40" />
          ) : tags.length === 0 ? (
            <span className="text-[13px] text-muted-foreground">No tags yet.</span>
          ) : (
            tags.map((t) => (
              <Badge key={t} variant="secondary" className="gap-1 pr-1">
                {t}
                <button
                  type="button"
                  aria-label={`Delete tag ${t}`}
                  className="grid size-4 place-items-center rounded-sm hover:bg-foreground/10"
                  onClick={() => run("delete the tag", () => api.deleteTag(t))}
                >
                  <XIcon className="size-3" />
                </button>
              </Badge>
            ))
          )}
        </div>
        <form
          className="flex items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            if (!newTag.trim()) return
            run("add the tag", async () => {
              await api.putTag(newTag.trim())
              setNewTag("")
            })
          }}
        >
          <Field className="w-56">
            <FieldLabel htmlFor="tag-name">New tag</FieldLabel>
            <Input id="tag-name" placeholder="4k" value={newTag} onChange={(e) => setNewTag(e.target.value)} />
          </Field>
          <Button type="submit" variant="outline" disabled={!newTag.trim()}>
            <PlusIcon data-icon="inline-start" />
            Add tag
          </Button>
        </form>
      </Section>
    </div>
  )
}

function CategoryRow({ name, path, onSave, onDelete }: { name: string; path: string; onSave: (p: string) => void; onDelete: () => void }) {
  const [value, setValue] = useState(path)
  return (
    <li className="flex flex-wrap items-center gap-2 p-2.5">
      <span className="w-36 truncate text-[13px] font-medium" title={name}>{name}</span>
      <Input
        aria-label={`Folder for ${name}`}
        className="data h-8 min-w-48 flex-1"
        placeholder="Subfolder of the default save folder"
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <Button size="sm" variant="outline" disabled={value === path} onClick={() => onSave(value.trim())}>
        Save
      </Button>
      <Button size="icon-sm" variant="ghost" aria-label={`Delete category ${name}`} onClick={onDelete}>
        <Trash2Icon />
      </Button>
    </li>
  )
}

// ---------------------------------------------------------------- security

function SecuritySection({
  draft,
  set,
  sessions,
  onSessionsChanged,
}: {
  draft: Settings
  set: <K extends keyof Settings>(key: K, patch: Partial<Settings[K]>) => void
  sessions: number
  onSessionsChanged: (n: number) => void
}) {
  const [account, setAccount] = useState({ username: "", current: "", next: "", confirm: "" })
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.me().then((m) => setAccount((a) => ({ ...a, username: m.username ?? "" }))).catch(() => {})
  }, [])

  const mismatch = account.next !== "" && account.confirm !== "" && account.next !== account.confirm

  const changeAccount = async (e: React.FormEvent) => {
    e.preventDefault()
    if (mismatch) return
    setBusy(true)
    try {
      const r = await api.changePassword({
        current_password: account.current,
        username: account.username,
        new_password: account.next || undefined,
      })
      toast.success(account.next ? "Password changed" : "Username saved", {
        description: r.signed_out > 0 ? `Signed out ${r.signed_out} other session${r.signed_out === 1 ? "" : "s"}.` : undefined,
      })
      if (r.signed_out > 0) onSessionsChanged(1)
      setAccount((a) => ({ ...a, current: "", next: "", confirm: "" }))
    } catch (err) {
      toast.error("Couldn't update your account", { description: errorText(err) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex flex-col gap-10">
      <Section title="Account" description="One account controls the web UI and the qBittorrent-compatible API Sonarr and Radarr use.">
        <form onSubmit={changeAccount}>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="username">Username</FieldLabel>
              <Input id="username" autoComplete="username" className="w-64" value={account.username} onChange={(e) => setAccount({ ...account, username: e.target.value })} />
            </Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="new-pw">New password</FieldLabel>
                <Input id="new-pw" type="password" autoComplete="new-password" placeholder="Leave empty to keep it" value={account.next} onChange={(e) => setAccount({ ...account, next: e.target.value })} />
              </Field>
              <Field data-invalid={mismatch || undefined}>
                <FieldLabel htmlFor="confirm-pw">Confirm new password</FieldLabel>
                <Input id="confirm-pw" type="password" autoComplete="new-password" aria-invalid={mismatch || undefined} value={account.confirm} onChange={(e) => setAccount({ ...account, confirm: e.target.value })} />
                {mismatch && <FieldDescription>The passwords don't match.</FieldDescription>}
              </Field>
            </div>
            <Field>
              <FieldLabel htmlFor="current-pw">Current password</FieldLabel>
              <Input id="current-pw" type="password" autoComplete="current-password" className="w-64" required value={account.current} onChange={(e) => setAccount({ ...account, current: e.target.value })} />
              <FieldDescription>Changing the password signs out every other device.</FieldDescription>
            </Field>
            <div>
              <Button type="submit" disabled={busy || !account.current || mismatch}>
                {busy && <Spinner data-icon="inline-start" />}
                Update account
              </Button>
            </div>
          </FieldGroup>
        </form>
      </Section>

      <Section title="Sessions" description={`${sessions} signed-in session${sessions === 1 ? "" : "s"}, including this one.`}>
        <div>
          <Button
            variant="outline"
            onClick={async () => {
              try {
                const r = await api.logoutOthers()
                onSessionsChanged(1)
                toast.success(r.signed_out ? `Signed out ${r.signed_out} other session${r.signed_out === 1 ? "" : "s"}` : "No other sessions were signed in")
              } catch (err) {
                toast.error("Couldn't sign out other sessions", { description: errorText(err) })
              }
            }}
          >
            Sign out everywhere else
          </Button>
        </div>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="session-days">Stay signed in for</FieldLabel>
            <NumberInput id="session-days" unit="days" min={1} max={365} value={draft.webui.session_days} onChange={(v) => set("webui", { session_days: Math.min(365, Math.max(1, Math.round(v))) })} />
            <FieldDescription>Applies to new sign-ins.</FieldDescription>
          </Field>
        </FieldGroup>
      </Section>

      <Section title="Sign-in protection">
        <FieldGroup>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="attempts">Lock out after</FieldLabel>
              <NumberInput id="attempts" unit="failed tries" className="w-48" value={draft.webui.max_attempts} onChange={(v) => set("webui", { max_attempts: Math.max(0, Math.round(v)) })} />
              <FieldDescription>Per address. 0 turns lockout off.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="lockout">For</FieldLabel>
              <NumberInput id="lockout" unit="minutes" className="w-44" min={1} value={draft.webui.lockout_minutes} onChange={(v) => set("webui", { lockout_minutes: Math.max(1, Math.round(v)) })} />
            </Field>
          </div>
          <Field>
            <FieldLabel htmlFor="proxies">Trusted reverse proxies</FieldLabel>
            <Textarea
              id="proxies"
              rows={3}
              className="data"
              value={draft.webui.trusted_proxies.join("\n")}
              onChange={(e) => set("webui", { trusted_proxies: e.target.value.split(/[\n,]/).map((s) => s.trim()).filter(Boolean) })}
            />
            <FieldDescription>
              One address or IPv4 range per line, like 127.0.0.1 or 10.0.0.0/8. Requests from these may set X-Forwarded-For and X-Forwarded-Proto, so lockouts apply to the real visitor and sign-in cookies are marked secure behind HTTPS.
            </FieldDescription>
          </Field>
        </FieldGroup>
      </Section>
    </div>
  )
}

// ---------------------------------------------------------------- about

function About({ server }: { server: SettingsResponse }) {
  const { data: info } = usePoll<ServerInfo>(api.info, 10000)
  const rows: [string, ReactNode][] = [
    ["Version", info ? `Swarmy ${info.version}` : "…"],
    ["Mode", info?.demo ? "Demo: simulated torrents" : "Live"],
    ["Engine", "librqbit"],
    ["Downloads folder", <span className="data break-all">{server.effective_download_dir}</span>],
    ["Data folder", <span className="data break-all">{server.data_dir}</span>],
    ["Listening port", info?.network.listen_port ?? "—"],
    ["DHT", info?.network.dht_nodes != null ? `${info.network.dht_nodes} nodes` : "Off"],
    ["Network interface", info?.network.bind_interface ?? "Any"],
  ]
  return (
    <div className="flex flex-col gap-10">
      <Section title="About">
        <dl className="grid grid-cols-[10rem_minmax(0,1fr)] gap-x-4 gap-y-2 text-[13px]">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className="m-0">{v}</dd>
            </div>
          ))}
        </dl>
      </Section>
      <Section
        title="Release notes"
        description="What each version of Swarmy brought. The notes are built into Swarmy, so they always match the version you're running."
      >
        <ChangelogPanel />
      </Section>
    </div>
  )
}
