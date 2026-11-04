import { useMemo, useState } from "react"
import { CrosshairIcon, GlobeIcon, RefreshCwIcon } from "lucide-react"
import { toast } from "sonner"

import { api, type GeoPoint, type GeoStatus, type PeerPlace, type Torrent } from "@/lib/api"
import { formatSpeed } from "@/lib/format"
import { hashParam } from "@/lib/route"
import { usePoll } from "@/lib/use-poll"
import zones from "@/lib/geo/time-zones.json"
import { CountUp } from "@/components/charts"
import { PeerMapCanvas } from "@/components/peer-map"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"

const ALL = "__all__"
const TZ_ZONES = zones as unknown as Record<string, [number, number]>

/** Without a saved location, guess from the browser's time zone. */
function guessHome(): { point: GeoPoint; zone: string } | null {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
    const c = TZ_ZONES[zone]
    return c ? { point: { lat: c[0], lon: c[1] }, zone } : null
  } catch {
    return null
  }
}

interface CountryRow {
  code: string
  name: string
  peers: number
  dl: number
  ul: number
}

function GeoNotice({ geo, onRetry, onEnable }: { geo: GeoStatus; onRetry: () => void; onEnable: () => void }) {
  if (geo.state === "downloading" || (geo.state === "missing" && geo.progress != null)) {
    return (
      <Alert>
        <GlobeIcon />
        <AlertTitle>Getting the location database</AlertTitle>
        <AlertDescription>
          <p>Swarmy is downloading DB-IP's free city database (about 60 MB) so it can place peers. It's looked up on your server, and peer addresses are never sent anywhere.</p>
          <Progress value={Math.round((geo.progress ?? 0) * 100)} className="mt-2 max-w-sm" />
        </AlertDescription>
      </Alert>
    )
  }
  if (geo.state === "missing") {
    return (
      <Alert>
        <GlobeIcon />
        <AlertTitle>Peers will appear once the location database is ready</AlertTitle>
        <AlertDescription>Swarmy downloads it when it first starts. If that hasn't happened yet, start it now.</AlertDescription>
        <AlertAction>
          <Button size="sm" variant="outline" onClick={onRetry}>
            Download now
          </Button>
        </AlertAction>
      </Alert>
    )
  }
  if (geo.state === "error") {
    return (
      <Alert variant="destructive">
        <GlobeIcon />
        <AlertTitle>Couldn't get the location database</AlertTitle>
        <AlertDescription>{geo.error ?? "The download failed."} Swarmy tries again every few hours.</AlertDescription>
        <AlertAction>
          <Button size="sm" variant="outline" onClick={onRetry}>
            Try again
          </Button>
        </AlertAction>
      </Alert>
    )
  }
  if (geo.state === "off") {
    return (
      <Alert>
        <GlobeIcon />
        <AlertTitle>Peer locations are off</AlertTitle>
        <AlertDescription>
          Turn them on to see where your peers are. Swarmy downloads a free location database (about 60 MB) and keeps it up to date; lookups happen on your server.
        </AlertDescription>
        <AlertAction>
          <Button size="sm" onClick={onEnable}>
            Turn on
          </Button>
        </AlertAction>
      </Alert>
    )
  }
  return null
}

export function PeersPage({ torrents }: { torrents: Torrent[] }) {
  const [hash, setHash] = useState<string | null>(() => hashParam("hash"))
  const { data } = usePoll(() => api.peers(hash), 2000, hash ?? "")
  const [picking, setPicking] = useState(false)
  const [savedHome, setSavedHome] = useState<GeoPoint | null>(null)
  const [guess] = useState(guessHome)
  const home = savedHome ?? data?.home ?? guess?.point ?? null
  const estimated = !savedHome && !data?.home

  const places = useMemo(() => data?.places ?? [], [data])
  const regions = useMemo(() => {
    try {
      return new Intl.DisplayNames(undefined, { type: "region" })
    } catch {
      return null
    }
  }, [])
  const countries = useMemo(() => {
    const by = new Map<string, CountryRow>()
    for (const p of places) {
      const row = by.get(p.country) ?? { code: p.country, name: regions?.of(p.country) ?? p.country, peers: 0, dl: 0, ul: 0 }
      row.peers += p.peers
      row.dl += p.dl
      row.ul += p.ul
      by.set(p.country, row)
    }
    return [...by.values()].sort((a, b) => b.dl + b.ul - (a.dl + a.ul) || b.peers - a.peers)
  }, [places, regions])
  const [showAll, setShowAll] = useState(false)
  const totals = places.reduce((a, p: PeerPlace) => ({ dl: a.dl + p.dl, ul: a.ul + p.ul }), { dl: 0, ul: 0 })
  const busiest = Math.max(1, ...countries.map((c) => c.dl + c.ul))
  const running = torrents.filter((t) => t.state === "downloading" || t.state === "seeding")

  async function saveSettings(change: (s: Awaited<ReturnType<typeof api.settings>>["settings"]) => void, done: string) {
    try {
      const { settings } = await api.settings()
      change(settings)
      await api.saveSettings(settings)
      toast.success(done)
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }

  async function pick(p: GeoPoint) {
    setPicking(false)
    setSavedHome(p)
    await saveSettings((s) => (s.peer_map.home = p), "Location saved")
  }

  async function retry() {
    try {
      await api.geoUpdate()
      toast.success("Downloading the location database")
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="flex flex-col gap-6 p-4 pb-8 lg:p-6">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-xl font-semibold">Swarm map</h1>
          <p className="text-sm text-muted-foreground">
            {data
              ? `${data.total} ${data.total === 1 ? "peer" : "peers"} connected${countries.length ? ` in ${countries.length} ${countries.length === 1 ? "country" : "countries"}` : ""}. Pieces flow towards you as you download and away as you upload.`
              : "Finding your peers…"}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Select
            value={hash ?? ALL}
            onValueChange={(v) => setHash(!v || v === ALL ? null : v)}
            items={[{ value: ALL, label: "All running torrents" }, ...running.map((t) => ({ value: t.hash, label: t.name }))]}
          >
            <SelectTrigger className="w-64 max-w-full" aria-label="Torrent">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value={ALL}>All running torrents</SelectItem>
                {running.map((t) => (
                  <SelectItem key={t.hash} value={t.hash}>
                    <span className="truncate">{t.name}</span>
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Button variant={picking ? "default" : "outline"} onClick={() => setPicking((p) => !p)}>
            <CrosshairIcon data-icon="inline-start" />
            {picking ? "Click where you are" : "Set my location"}
          </Button>
        </div>
      </header>

      {data && data.geo.state !== "ready" && data.geo.state !== "demo" && (
        <GeoNotice
          geo={data.geo}
          onRetry={retry}
          onEnable={() => saveSettings((s) => (s.peer_map.locations = true), "Peer locations turned on")}
        />
      )}

      <dl className="grid grid-cols-2 gap-x-8 gap-y-4 md:grid-cols-4">
        <Figure label="Downloading" value={<CountUp value={totals.dl} format={formatSpeed} />} swatch="var(--down)" />
        <Figure label="Uploading" value={<CountUp value={totals.ul} format={formatSpeed} />} swatch="var(--up)" />
        <Figure label="Peers" value={<CountUp value={data?.total ?? 0} format={(v) => Math.round(v).toString()} />} />
        <Figure label="Countries" value={<CountUp value={countries.length} format={(v) => Math.round(v).toString()} />} />
      </dl>

      <section className="flex flex-col gap-3">
        <div className="overflow-hidden rounded-xl border bg-card p-2 sm:p-4">
          {data ? (
            <PeerMapCanvas places={places} home={home} picking={picking} onPick={pick} />
          ) : (
            <Skeleton className="aspect-[2.05] w-full" />
          )}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 text-xs text-muted-foreground">
          <ul className="flex flex-wrap items-center gap-x-4 gap-y-1">
            <li className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-(--down)" />
              Download, flowing to you
            </li>
            <li className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-(--up)" />
              Upload, flowing from you
            </li>
            <li className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-primary ring-2 ring-primary/30" />
              You
              {estimated && guess && <span>(estimated from your time zone, {guess.zone.replace(/_/g, " ")})</span>}
            </li>
          </ul>
          <span>
            {data?.geo.state === "demo" ? (
              "Demo mode: made-up peers in real cities."
            ) : (
              <>
                <a href="https://db-ip.com" target="_blank" rel="noreferrer" className="underline underline-offset-2 hover:text-foreground">
                  IP geolocation by DB-IP
                </a>
                {data?.geo.edition ? `, ${data.geo.edition} edition` : ""}
              </>
            )}
          </span>
        </div>
        {data && data.unlocated > 0 && (
          <p className="text-xs text-muted-foreground">
            {data.unlocated} {data.unlocated === 1 ? "peer isn't" : "peers aren't"} on the map: on a private network, or at an address the database can't place.
          </p>
        )}
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex items-baseline justify-between gap-2">
          <h2 className="text-[15px] font-semibold">By country</h2>
          {data?.geo.state === "ready" && (
            <Button size="sm" variant="ghost" onClick={retry}>
              <RefreshCwIcon data-icon="inline-start" />
              Check for a newer database
            </Button>
          )}
        </div>
        {countries.length === 0 ? (
          <p className="text-sm text-muted-foreground">No peers to show yet. They appear here as torrents connect.</p>
        ) : (
          <div role="table" aria-label="Peers by country" className="text-[13px]">
            <div role="row" className="grid grid-cols-[minmax(0,1fr)_4rem_6.5rem_6.5rem] gap-3 border-b pb-2 text-xs text-muted-foreground md:grid-cols-[minmax(0,14rem)_4rem_6.5rem_6.5rem_minmax(0,1fr)]">
              <span role="columnheader">Country</span>
              <span role="columnheader" className="text-right">Peers</span>
              <span role="columnheader" className="text-right">Download</span>
              <span role="columnheader" className="text-right">Upload</span>
              <span role="columnheader" className="hidden md:block">Share of traffic</span>
            </div>
            {(showAll ? countries : countries.slice(0, 12)).map((c) => (
              <div
                key={c.code}
                role="row"
                className="grid grid-cols-[minmax(0,1fr)_4rem_6.5rem_6.5rem] items-center gap-3 border-b py-2 md:grid-cols-[minmax(0,14rem)_4rem_6.5rem_6.5rem_minmax(0,1fr)]"
              >
                <span role="cell" className="truncate">
                  {c.name}
                </span>
                <span role="cell" className="data text-right">{c.peers}</span>
                <span role="cell" className="data text-right">{formatSpeed(c.dl)}</span>
                <span role="cell" className="data text-right">{formatSpeed(c.ul)}</span>
                <span role="cell" className="hidden h-2 items-stretch gap-[2px] md:flex" aria-hidden>
                  <span className="bar-fill rounded-l-sm bg-(--down)" style={{ width: `${(c.dl / busiest) * 100}%` }} />
                  <span className="bar-fill rounded-r-sm bg-(--up)" style={{ width: `${(c.ul / busiest) * 100}%` }} />
                </span>
              </div>
            ))}
            {countries.length > 12 && (
              <Button variant="ghost" size="sm" className="mt-2" onClick={() => setShowAll((s) => !s)}>
                {showAll ? "Show fewer" : `Show all ${countries.length} countries`}
              </Button>
            )}
          </div>
        )}
      </section>
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
