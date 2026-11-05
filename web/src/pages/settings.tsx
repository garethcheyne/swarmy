import { useEffect, useMemo, useState, type ReactNode } from "react"
import {
  CircleAlertIcon,
  LockIcon,
  PlusIcon,
  RotateCwIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react"
import { toast } from "sonner"

import { api, UnauthorizedError, type Category, type ServerInfo, type Settings, type SettingsResponse, type GeoStatus, type UsenetServer } from "@/lib/api"
import { usePoll } from "@/lib/use-poll"
import { cn } from "@/lib/utils"
import type { HelpId } from "@/lib/settings-help"
import { SETTINGS_SECTIONS, settingsHref, useSettingsSection } from "@/lib/settings-sections"
import { SettingHelp } from "@/components/setting-help"
import { ChangelogPanel } from "@/components/release-notes"
import { UpdatesPanel } from "@/components/updates"
import { ScreenDiagnostics } from "@/components/screen-diagnostics"
import { FolderInput } from "@/components/folder-picker"
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

/** A field label with its ⓘ beside it (outside the label, so clicking it
 * doesn't toggle the control). */
function HelpLabel({
  htmlFor,
  help,
  className,
  children,
}: {
  htmlFor?: string
  help?: HelpId
  className?: string
  children: ReactNode
}) {
  return (
    <div className={cn("flex items-center gap-1", className)}>
      <FieldLabel htmlFor={htmlFor}>{children}</FieldLabel>
      {help && <SettingHelp id={help} />}
    </div>
  )
}

function SwitchRow({
  id,
  label,
  description,
  checked,
  onChange,
  locked = false,
  help,
}: {
  id: string
  label: string
  description?: string
  checked: boolean
  onChange: (v: boolean) => void
  locked?: boolean
  help?: HelpId
}) {
  return (
    <Field orientation="horizontal" data-disabled={locked || undefined}>
      <FieldContent>
        <HelpLabel htmlFor={id} help={help}>
          {label}
          <Locked on={locked} />
        </HelpLabel>
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
  const section = useSettingsSection()
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
    <div className="flex h-full min-h-0 flex-col">
      {/* On desktop the sections are in the main menu; phones get a strip. */}
      <nav aria-label="Settings sections" className="shrink-0 overflow-x-auto border-b md:hidden">
        <div className="flex gap-1 p-2">
          {SETTINGS_SECTIONS.map((s) => (
            <a
              key={s.id}
              href={settingsHref(s.id)}
              aria-current={section === s.id ? "page" : undefined}
              className={cn(
                "flex min-h-9 shrink-0 items-center gap-2 rounded-md px-2.5 text-[13px] whitespace-nowrap text-muted-foreground hover:bg-muted hover:text-foreground",
                section === s.id && "bg-muted text-foreground"
              )}
            >
              <s.icon className="size-4" />
              {s.label}
            </a>
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
                <Section title="Downloads" description="Where torrents go, how they start, and when their files move.">
                  <FieldGroup>
                    <DownloadFolderCheck />
                    <Field data-disabled={locked("downloads.default_save_path") || undefined}>
                      <HelpLabel htmlFor="save-path" help="save_path">
                        Default save folder
                        <Locked on={locked("downloads.default_save_path")} />
                      </HelpLabel>
                      <FolderInput
                        id="save-path"
                        title="Default save folder"
                        placeholder={server.effective_download_dir}
                        disabled={locked("downloads.default_save_path")}
                        value={draft.downloads.default_save_path}
                        onChange={(v) => set("downloads", { default_save_path: v })}
                      />
                      <FieldDescription className="[overflow-wrap:anywhere]">
                        Torrents with a category go in a subfolder named after it, unless the category has its own folder. Leave empty to use {server.effective_download_dir}.
                      </FieldDescription>
                    </Field>
                    <SwitchRow
                      id="add-paused" help="add_paused"
                      label="Add torrents paused"
                      description="New torrents wait until you start them. Sonarr and Radarr follow this too."
                      checked={draft.downloads.add_paused}
                      onChange={(v) => set("downloads", { add_paused: v })}
                    />
                    <SwitchRow
                      id="use-incomplete" help="incomplete_dir"
                      label="Keep unfinished downloads in a separate folder"
                      description="Torrents download there and move to their save folder when complete, so nothing half-finished shows up in your library."
                      checked={draft.downloads.use_incomplete_dir}
                      onChange={(v) => set("downloads", { use_incomplete_dir: v })}
                    />
                    {draft.downloads.use_incomplete_dir && (
                      <Field>
                        <HelpLabel htmlFor="incomplete-dir" help="incomplete_dir">
                          Unfinished downloads folder
                        </HelpLabel>
                        <FolderInput
                          id="incomplete-dir"
                          title="Unfinished downloads folder"
                          placeholder={`${server.effective_download_dir}/incomplete`}
                          value={draft.downloads.incomplete_dir}
                          onChange={(v) => set("downloads", { incomplete_dir: v })}
                        />
                        <FieldDescription>
                          Leave empty to use an "incomplete" folder inside the save folder. Only affects torrents added from now on.
                        </FieldDescription>
                      </Field>
                    )}
                    <SwitchRow
                      id="category-moves" help="category_moves"
                      label="Move files when the category changes"
                      description="Changing a torrent's category, a category's folder or the downloads folder moves its files to match. Torrents you've moved by hand stay where you put them."
                      checked={draft.downloads.category_moves}
                      onChange={(v) => set("downloads", { category_moves: v })}
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
                      <HelpLabel htmlFor="port" help="listen_port">
                        Listening port
                        <Locked on={locked("connection.listen_port")} />
                      </HelpLabel>
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
                    <SwitchRow id="upnp" help="upnp" label="Forward the port with UPnP" description="Asks your router to open the port automatically." checked={draft.connection.upnp} onChange={(v) => set("connection", { upnp: v })} locked={locked("connection.upnp")} />
                    <Field data-disabled={locked("connection.bind_interface") || undefined}>
                      <HelpLabel htmlFor="iface" help="bind_interface">
                        Network interface
                        <Locked on={locked("connection.bind_interface")} />
                      </HelpLabel>
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
                    <SwitchRow id="ipv4" help="ipv4_only" label="IPv4 only" description="Never connect to peers over IPv6. Stops IPv6 slipping around a VPN that only tunnels IPv4." checked={draft.connection.ipv4_only} onChange={(v) => set("connection", { ipv4_only: v })} locked={locked("connection.ipv4_only")} />
                    <FieldSeparator />
                    <SwitchRow id="dht" help="dht" label="DHT" description="Find peers without a tracker. Needed for magnet links." checked={draft.connection.dht} onChange={(v) => set("connection", { dht: v })} />
                    <SwitchRow id="lsd" help="lsd" label="Local peer discovery" description="Find peers on your own network." checked={draft.connection.lsd} onChange={(v) => set("connection", { lsd: v })} />
                    <Field>
                      <HelpLabel htmlFor="peers" help="peer_limit">Maximum peers per torrent</HelpLabel>
                      <NumberInput id="peers" value={draft.connection.peer_limit} onChange={(v) => set("connection", { peer_limit: Math.max(0, Math.round(v)) })} />
                      <FieldDescription>More can be faster on busy torrents but uses more memory, and strains some routers. 0 uses the engine's default.</FieldDescription>
                    </Field>
                    <FieldSeparator />
                    <Field>
                      <HelpLabel htmlFor="proxy" help="socks_proxy">SOCKS5 proxy for peers</HelpLabel>
                      <Input id="proxy" className="data" placeholder="socks5://user:password@host:1080" value={draft.connection.socks_proxy} onChange={(e) => set("connection", { socks_proxy: e.target.value })} />
                      <FieldDescription>Outgoing peer connections go through this proxy. Leave empty for none.</FieldDescription>
                    </Field>
                    <Field>
                      <HelpLabel htmlFor="blocklist" help="blocklist">IP blocklist URL</HelpLabel>
                      <Input id="blocklist" className="data" placeholder="https://example.org/blocklist.p2p.gz" value={draft.connection.blocklist_url} onChange={(e) => set("connection", { blocklist_url: e.target.value })} />
                      <FieldDescription>A P2P or DAT format list, gzipped or plain. Peers on it are refused.</FieldDescription>
                    </Field>
                  </FieldGroup>
                </Section>
              )}

              {section === "speed" && (
                <Section title="Speed" description="Limits are shared by all torrents, in KiB/s (1024 KiB/s is 1 MiB/s). Use 0 for no limit. A little headroom below your line speed keeps the rest of your internet responsive.">
                  <FieldGroup>
                    <div className="grid gap-4 sm:grid-cols-2">
                      <Field>
                        <HelpLabel htmlFor="dl" help="download_limit">Download limit</HelpLabel>
                        <NumberInput id="dl" unit="KiB/s" value={draft.speed.download_limit} onChange={(v) => set("speed", { download_limit: Math.max(0, Math.round(v)) })} />
                      </Field>
                      <Field>
                        <HelpLabel htmlFor="ul" help="upload_limit">Upload limit</HelpLabel>
                        <NumberInput id="ul" unit="KiB/s" value={draft.speed.upload_limit} onChange={(v) => set("speed", { upload_limit: Math.max(0, Math.round(v)) })} />
                      </Field>
                    </div>
                    <FieldSeparator />
                    <div className="flex flex-col gap-1">
                      <div className="flex items-center gap-1">
                        <h3 className="text-[13px] font-semibold">Alternative limits</h3>
                        <SettingHelp id="alt_limits" />
                      </div>
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
                    <SwitchRow id="alt-now" help="alt_limits" label="Use alternative limits now" checked={draft.speed.alt_enabled} onChange={(v) => set("speed", { alt_enabled: v })} />
                    <SwitchRow
                      id="sched" help="schedule"
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
                    <SwitchRow id="queue" help="queue" label="Queue torrents" description="Torrents over the limits wait, then start when a slot frees up. Oldest first." checked={draft.queue.enabled} onChange={(v) => set("queue", { enabled: v })} />
                    {draft.queue.enabled && (
                      <div className="grid gap-4 sm:grid-cols-3">
                        <Field>
                          <HelpLabel htmlFor="qd" help="queue_downloads">Active downloads</HelpLabel>
                          <NumberInput id="qd" className="w-full" value={draft.queue.max_downloads} onChange={(v) => set("queue", { max_downloads: Math.max(0, Math.round(v)) })} />
                        </Field>
                        <Field>
                          <HelpLabel htmlFor="qu" help="queue_uploads">Active uploads</HelpLabel>
                          <NumberInput id="qu" className="w-full" value={draft.queue.max_uploads} onChange={(v) => set("queue", { max_uploads: Math.max(0, Math.round(v)) })} />
                        </Field>
                        <Field>
                          <HelpLabel htmlFor="qa" help="queue_total">Active in total</HelpLabel>
                          <NumberInput id="qa" className="w-full" min={1} value={draft.queue.max_active} onChange={(v) => set("queue", { max_active: Math.max(1, Math.round(v)) })} />
                        </Field>
                      </div>
                    )}
                    <FieldSeparator />
                    <div className="flex flex-col gap-1">
                      <h3 className="text-[13px] font-semibold">Seeding limits</h3>
                      <p className="text-[13px] text-muted-foreground">When a finished torrent should stop seeding. The first limit it reaches applies. Sonarr and Radarr check these to know when a download can be cleaned up.</p>
                    </div>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="ratio-on"
                        checked={draft.seeding.ratio_limit !== null}
                        onCheckedChange={(on) => set("seeding", { ratio_limit: on ? 2 : null })}
                      />
                      <HelpLabel htmlFor="ratio-on" help="ratio_limit" className="w-44">Stop at a share ratio of</HelpLabel>
                      <NumberInput id="ratio" step={0.1} className="w-28" disabled={draft.seeding.ratio_limit === null} value={draft.seeding.ratio_limit ?? 2} onChange={(v) => set("seeding", { ratio_limit: Math.max(0, v) })} />
                    </Field>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="time-on"
                        checked={draft.seeding.time_limit !== null}
                        onCheckedChange={(on) => set("seeding", { time_limit: on ? 1440 : null })}
                      />
                      <HelpLabel htmlFor="time-on" help="seeding_time" className="w-44">Stop after seeding for</HelpLabel>
                      <NumberInput id="seedtime" unit="min" className="w-36" disabled={draft.seeding.time_limit === null} value={draft.seeding.time_limit ?? 1440} onChange={(v) => set("seeding", { time_limit: Math.max(1, Math.round(v)) })} />
                    </Field>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="idle-on"
                        checked={draft.seeding.inactive_time_limit !== null}
                        onCheckedChange={(on) => set("seeding", { inactive_time_limit: on ? 720 : null })}
                      />
                      <HelpLabel htmlFor="idle-on" help="inactive_time" className="w-44">Stop after no activity for</HelpLabel>
                      <NumberInput id="idletime" unit="min" className="w-36" disabled={draft.seeding.inactive_time_limit === null} value={draft.seeding.inactive_time_limit ?? 720} onChange={(v) => set("seeding", { inactive_time_limit: Math.max(1, Math.round(v)) })} />
                    </Field>
                    <Field>
                      <HelpLabel help="seed_action">Then</HelpLabel>
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

              {section === "usenet" && <UsenetSection draft={draft} set={set} saved={server.settings.usenet} />}

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

              {section === "map" && (
                <Section
                  title="Swarm map"
                  description="Shows where your peers are and how data moves between you. Locations come from a free database Swarmy keeps on your server, so peer addresses are never sent to anyone."
                >
                  <FieldGroup>
                    <SwitchRow
                      id="peer-locations" help="peer_locations"
                      label="Show where peers are"
                      description="Downloads DB-IP's free city database (about 60 MB) and checks for a new edition every few hours. Turn off to make no connections for this."
                      checked={draft.peer_map.locations}
                      onChange={(v) => set("peer_map", { locations: v })}
                    />
                    <GeoDatabaseStatus />
                    <Field>
                      <FieldLabel>Your location</FieldLabel>
                      <FieldDescription>
                        {draft.peer_map.home
                          ? `Saved at ${draft.peer_map.home.lat.toFixed(2)}, ${draft.peer_map.home.lon.toFixed(2)}. Set it from the map with "Set my location".`
                          : 'Estimated from your browser\'s time zone. Use "Set my location" on the map to pin it.'}
                      </FieldDescription>
                      {draft.peer_map.home && (
                        <div>
                          <Button variant="outline" size="sm" onClick={() => set("peer_map", { home: null })}>
                            Forget my location
                          </Button>
                        </div>
                      )}
                    </Field>
                  </FieldGroup>
                </Section>
              )}

              {section === "appearance" && (
                <Section title="Appearance" description="Saved in this browser, so each device can have its own look.">
                  <AppearancePanel />
                </Section>
              )}

              {section === "about" && (
                <About server={server} check={draft.updates.check} onCheck={(v) => set("updates", { check: v })} />
              )}
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
            <FolderInput id="cat-path" title="Folder for the new category" placeholder="/data/movies" value={newCat.path} onChange={(v) => setNewCat({ ...newCat, path: v })} />
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

/** Says so, right where it's fixed, when Swarmy can't write to the download folder. */
function DownloadFolderCheck() {
  const { data: info } = usePoll<ServerInfo>(api.info, 10000)
  if (!info?.download_dir_problem) return null
  return (
    <Alert variant="destructive">
      <CircleAlertIcon />
      <AlertTitle>Downloads will fail until this is fixed</AlertTitle>
      <AlertDescription className="[overflow-wrap:anywhere]">
        {info.download_dir_problem} Pick another folder below, or give Swarmy's user access to this one.
      </AlertDescription>
    </Alert>
  )
}

function CategoryRow({ name, path, onSave, onDelete }: { name: string; path: string; onSave: (p: string) => void; onDelete: () => void }) {
  const [value, setValue] = useState(path)
  return (
    <li className="flex flex-wrap items-center gap-2 p-2.5">
      <span className="w-36 truncate text-[13px] font-medium" title={name}>{name}</span>
      <FolderInput
        aria-label={`Folder for ${name}`}
        title={`Folder for ${name}`}
        className="h-8 min-w-48 flex-1"
        placeholder="Subfolder of the default save folder"
        value={value}
        onChange={setValue}
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
            <HelpLabel htmlFor="session-days" help="session_days">Stay signed in for</HelpLabel>
            <NumberInput id="session-days" unit="days" min={1} max={365} value={draft.webui.session_days} onChange={(v) => set("webui", { session_days: Math.min(365, Math.max(1, Math.round(v))) })} />
            <FieldDescription>Applies to new sign-ins.</FieldDescription>
          </Field>
        </FieldGroup>
      </Section>

      <Section title="Sign-in protection">
        <FieldGroup>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field>
              <HelpLabel htmlFor="attempts" help="lockout">Lock out after</HelpLabel>
              <NumberInput id="attempts" unit="failed tries" className="w-48" value={draft.webui.max_attempts} onChange={(v) => set("webui", { max_attempts: Math.max(0, Math.round(v)) })} />
              <FieldDescription>Per address. 0 turns lockout off.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="lockout">For</FieldLabel>
              <NumberInput id="lockout" unit="minutes" className="w-44" min={1} value={draft.webui.lockout_minutes} onChange={(v) => set("webui", { lockout_minutes: Math.max(1, Math.round(v)) })} />
            </Field>
          </div>
          <Field>
            <HelpLabel htmlFor="proxies" help="trusted_proxies">Trusted reverse proxies</HelpLabel>
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

// ---------------------------------------------------------------- usenet

const newServerId = () => Math.random().toString(36).slice(2, 10)

function UsenetSection({
  draft,
  set,
  saved,
}: {
  draft: Settings
  set: <K extends keyof Settings>(key: K, patch: Partial<Settings[K]>) => void
  saved: Settings["usenet"]
}) {
  const u = draft.usenet
  const [tests, setTests] = useState<Record<string, { busy: boolean; ok?: boolean; message?: string }>>({})
  const setServers = (servers: UsenetServer[]) => set("usenet", { servers })
  const update = (id: string, patch: Partial<UsenetServer>) => setServers(u.servers.map((s) => (s.id === id ? { ...s, ...patch } : s)))
  const add = () =>
    setServers([
      ...u.servers,
      { id: newServerId(), name: "", enabled: true, priority: u.servers.length ? Math.max(...u.servers.map((s) => s.priority)) + 1 : 0, host: "", port: 563, tls: true, username: "", password: "", connections: 20 },
    ])
  const test = async (srv: UsenetServer) => {
    setTests((t) => ({ ...t, [srv.id]: { busy: true } }))
    try {
      const r = await api.usenetTestServer(srv)
      setTests((t) => ({ ...t, [srv.id]: { busy: false, ok: r.ok, message: r.message } }))
    } catch (e) {
      setTests((t) => ({ ...t, [srv.id]: { busy: false, ok: false, message: e instanceof Error ? e.message : String(e) } }))
    }
  }

  return (
    <div className="flex max-w-2xl flex-col gap-10">
      <Section
        title="Usenet servers"
        description="Where NZB downloads come from: your Usenet provider's news server. Add backups (a second provider, or a block account) to fill in articles the main server is missing."
      >
        {u.servers.length === 0 && <p className="text-[13px] text-muted-foreground">No servers yet. Your provider's welcome email lists the address, port and login.</p>}
        <div className="flex flex-col gap-4">
          {u.servers.map((srv) => {
            const t = tests[srv.id]
            const isSaved = saved.servers.some((o) => o.id === srv.id)
            return (
              <div key={srv.id} className="flex flex-col gap-4 rounded-lg border p-4">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <Switch id={`srv-on-${srv.id}`} checked={srv.enabled} onCheckedChange={(v) => update(srv.id, { enabled: v })} aria-label="Use this server" />
                    <span className="text-[13px] font-semibold">{srv.name || srv.host || "New server"}</span>
                  </div>
                  <div className="flex items-center gap-1">
                    <Button size="sm" variant="outline" disabled={!srv.host || t?.busy} onClick={() => test(srv)}>
                      {t?.busy && <Spinner data-icon="inline-start" />}
                      Test
                    </Button>
                    <Button size="icon-sm" variant="ghost" aria-label={`Remove ${srv.name || srv.host || "server"}`} onClick={() => setServers(u.servers.filter((s) => s.id !== srv.id))}>
                      <Trash2Icon />
                    </Button>
                  </div>
                </div>
                {t && !t.busy && (
                  <p className={cn("text-[13px]", t.ok ? "text-up" : "text-destructive")}>{t.ok ? `${t.message}.` : t.message}</p>
                )}
                <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_7rem]">
                  <Field>
                    <HelpLabel htmlFor={`srv-host-${srv.id}`} help="usenet_host">
                      Address
                    </HelpLabel>
                    <Input id={`srv-host-${srv.id}`} className="data" placeholder="news.example.com" value={srv.host} onChange={(e) => update(srv.id, { host: e.target.value })} />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor={`srv-port-${srv.id}`}>Port</FieldLabel>
                    <NumberInput id={`srv-port-${srv.id}`} className="w-full" min={1} max={65535} value={srv.port} onChange={(v) => update(srv.id, { port: Math.round(v) })} />
                  </Field>
                </div>
                <SwitchRow
                  id={`srv-tls-${srv.id}`}
                  help="usenet_tls"
                  label="Use SSL"
                  description="Encrypts the connection, usually on port 563. Leave it on unless your provider says otherwise."
                  checked={srv.tls}
                  onChange={(v) => update(srv.id, { tls: v, port: v && srv.port === 119 ? 563 : !v && srv.port === 563 ? 119 : srv.port })}
                />
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field>
                    <FieldLabel htmlFor={`srv-user-${srv.id}`}>Username</FieldLabel>
                    <Input id={`srv-user-${srv.id}`} autoComplete="off" value={srv.username} onChange={(e) => update(srv.id, { username: e.target.value })} />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor={`srv-pass-${srv.id}`}>Password</FieldLabel>
                    <Input
                      id={`srv-pass-${srv.id}`}
                      type="password"
                      autoComplete="new-password"
                      placeholder={isSaved ? "Saved; leave empty to keep it" : ""}
                      value={srv.password}
                      onChange={(e) => update(srv.id, { password: e.target.value })}
                    />
                  </Field>
                </div>
                <div className="grid gap-4 sm:grid-cols-3">
                  <Field>
                    <HelpLabel htmlFor={`srv-conn-${srv.id}`} help="usenet_connections">
                      Connections
                    </HelpLabel>
                    <NumberInput id={`srv-conn-${srv.id}`} className="w-full" min={1} max={200} value={srv.connections} onChange={(v) => update(srv.id, { connections: Math.min(200, Math.max(1, Math.round(v))) })} />
                  </Field>
                  <Field>
                    <HelpLabel htmlFor={`srv-prio-${srv.id}`} help="usenet_priority">
                      Order
                    </HelpLabel>
                    <NumberInput id={`srv-prio-${srv.id}`} className="w-full" min={0} max={99} value={srv.priority} onChange={(v) => update(srv.id, { priority: Math.max(0, Math.round(v)) })} />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor={`srv-name-${srv.id}`}>Name</FieldLabel>
                    <Input id={`srv-name-${srv.id}`} placeholder="Optional" value={srv.name} onChange={(e) => update(srv.id, { name: e.target.value })} />
                  </Field>
                </div>
              </div>
            )
          })}
        </div>
        <div>
          <Button variant="outline" onClick={add}>
            <PlusIcon data-icon="inline-start" />
            Add a server
          </Button>
        </div>
      </Section>

      <Section title="Downloads" description="Where Usenet downloads are assembled, and where they go when they're done.">
        <FieldGroup>
          <Field>
            <HelpLabel htmlFor="nzb-complete" help="usenet_complete_dir">
              Finished downloads
            </HelpLabel>
            <FolderInput id="nzb-complete" title="Finished Usenet downloads" placeholder="<download folder>/usenet" value={u.complete_dir} onChange={(v) => set("usenet", { complete_dir: v })} />
            <FieldDescription>Each download gets its own folder, inside a folder for its category. Leave empty for the default.</FieldDescription>
          </Field>
          <Field>
            <HelpLabel htmlFor="nzb-incomplete" help="usenet_incomplete_dir">
              Unfinished downloads
            </HelpLabel>
            <FolderInput id="nzb-incomplete" title="Unfinished Usenet downloads" placeholder="<download folder>/usenet/incomplete" value={u.incomplete_dir} onChange={(v) => set("usenet", { incomplete_dir: v })} />
          </Field>
          <Field>
            <HelpLabel htmlFor="nzb-speed" help="usenet_speed">
              Speed limit
            </HelpLabel>
            <NumberInput id="nzb-speed" unit="KiB/s" value={u.speed_limit} onChange={(v) => set("usenet", { speed_limit: Math.max(0, Math.round(v)) })} />
            <FieldDescription>Separate from the torrent limits. 0 for no limit.</FieldDescription>
          </Field>
          <SwitchRow
            id="sab-api"
            help="sab_api"
            label="SABnzbd-compatible API"
            description="Lets Sonarr and Radarr send NZBs to Swarmy as if it were SABnzbd. See Settings, Sonarr and Radarr."
            checked={u.sab_api}
            onChange={(v) => set("usenet", { sab_api: v })}
          />
        </FieldGroup>
      </Section>
    </div>
  )
}

// ---------------------------------------------------------------- swarm map

function GeoDatabaseStatus() {
  const { data: geo } = usePoll<GeoStatus>(api.geo, 3000)
  if (!geo) return null
  const text: Record<GeoStatus["state"], string> = {
    demo: "Demo mode: peers are placed in made-up locations.",
    ready: `Location database ready${geo.edition ? `, ${geo.edition} edition` : ""}. ${geo.attribution}.`,
    downloading: `Downloading the location database${geo.progress != null ? `, ${Math.round(geo.progress * 100)}%` : ""}…`,
    missing: "The location database hasn't been downloaded yet.",
    error: `Couldn't get the location database: ${geo.error ?? "unknown error"}. Swarmy tries again every few hours.`,
    off: "Peer locations are off.",
  }
  return (
    <p className={cn("text-[13px]", geo.state === "error" ? "text-destructive" : "text-muted-foreground")}>{text[geo.state]}</p>
  )
}

// ---------------------------------------------------------------- about

function About({ server, check, onCheck }: { server: SettingsResponse; check: boolean; onCheck: (v: boolean) => void }) {
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
    ["Licence", "Open source, MIT or Apache-2.0. No ads, no tracking."],
  ]
  return (
    <div className="flex flex-col gap-10">
      <Section title="Updates">
        <UpdatesPanel />
        <SwitchRow
          id="update-check"
          label="Check for updates"
          description="Every hour, Swarmy asks GitHub whether there's a newer release. When there is, a button in the top bar says so until you install it. Nothing is installed unless you choose to."
          checked={check}
          onChange={onCheck}
        />
      </Section>
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
        title="Screen details"
        description="What this device reports about its screen. If something looks wrong on a phone, copy these and send them with a screenshot."
      >
        <ScreenDiagnostics />
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
