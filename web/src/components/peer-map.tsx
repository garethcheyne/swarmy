import { useEffect, useMemo, useRef, useState } from "react"
import { geoEqualEarth, geoPath, type GeoProjection } from "d3-geo"
import { feature } from "topojson-client"
import type { GeometryCollection, Topology } from "topojson-specification"
import land110 from "world-atlas/land-110m.json"

import type { GeoPoint, PeerPlace } from "@/lib/api"
import { formatSpeed } from "@/lib/format"
import { cn } from "@/lib/utils"
import { useReducedMotion } from "@/components/charts"

type RGB = [number, number, number]

const topo = land110 as unknown as Topology<{ land: GeometryCollection }>
const LAND = feature(topo, topo.objects.land)

function cssVar(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback
}

/** Resolve any CSS colour (oklch, hex…) to sRGB by painting one pixel. */
function rgbOf(color: string): RGB {
  const c = document.createElement("canvas")
  c.width = c.height = 1
  const x = c.getContext("2d", { willReadFrequently: true })
  if (!x) return [128, 128, 128]
  x.fillStyle = "#000"
  x.fillStyle = color
  x.fillRect(0, 0, 1, 1)
  const d = x.getImageData(0, 0, 1, 1).data
  return [d[0], d[1], d[2]]
}

const rgba = ([r, g, b]: RGB, a: number) => `rgba(${r},${g},${b},${a})`

export const placeKey = (p: PeerPlace) => `${p.country}|${p.city ?? ""}|${p.lat.toFixed(1)}|${p.lon.toFixed(1)}`

interface Particle {
  ax: number
  ay: number
  cx: number
  cy: number
  bx: number
  by: number
  t: number
  /** Progress per millisecond. */
  speed: number
  color: RGB
}

/** A point on a quadratic Bézier. */
function bez(p: Particle, t: number): [number, number] {
  const u = 1 - t
  return [u * u * p.ax + 2 * u * t * p.cx + t * t * p.bx, u * u * p.ay + 2 * u * t * p.cy + t * t * p.by]
}

/** The control point that bows an arc from a to b gently northwards. */
function control(ax: number, ay: number, bx: number, by: number): [number, number] {
  const mx = (ax + bx) / 2
  const my = (ay + by) / 2
  const dx = bx - ax
  const dy = by - ay
  const len = Math.hypot(dx, dy) || 1
  let nx = -dy / len
  let ny = dx / len
  if (ny > 0) {
    nx = -nx
    ny = -ny
  }
  const lift = Math.min(len * 0.28, 140)
  return [mx + nx * lift, my + ny * lift]
}

/**
 * The swarm on a dot-matrix world: where each group of peers is, an arc to
 * you, and pieces flowing along it (download colour towards you, upload
 * colour away). Busier places send more pieces.
 */
export function PeerMapCanvas({
  places,
  home,
  picking = false,
  onPick,
  className,
}: {
  places: PeerPlace[]
  home: GeoPoint | null
  /** Clicking the map sets "you" instead of selecting a place. */
  picking?: boolean
  onPick?: (p: GeoPoint) => void
  className?: string
}) {
  const wrapRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const reduced = useReducedMotion()
  const [hover, setHover] = useState<{ place: PeerPlace; x: number; y: number; width: number } | null>(null)
  // The latest props, read by the animation loop without restarting it.
  const live = useRef({ places, home, picking })
  useEffect(() => {
    live.current = { places, home, picking }
  }, [places, home, picking])
  const projRef = useRef<GeoProjection | null>(null)

  useEffect(() => {
    const wrap = wrapRef.current
    const canvas = canvasRef.current
    const ctx = canvas?.getContext("2d")
    if (!wrap || !canvas || !ctx) return

    let w = 0
    let h = 0
    let dpr = 1
    let base: HTMLCanvasElement | null = null
    let raf = 0
    let particles: Particle[] = []
    let last = performance.now()
    let colors = { down: [80, 160, 60] as RGB, up: [60, 140, 200] as RGB, home: [150, 200, 100] as RGB, land: [120, 130, 120] as RGB, idle: [120, 130, 120] as RGB, dark: true }

    const paint = () => {
      colors = {
        down: rgbOf(cssVar("--down", "#5f9e3a")),
        up: rgbOf(cssVar("--up", "#3d93c7")),
        home: rgbOf(cssVar("--primary", "#9ccc65")),
        land: rgbOf(cssVar("--muted-foreground", "#888")),
        idle: rgbOf(cssVar("--muted-foreground", "#888")),
        dark: document.documentElement.classList.contains("dark"),
      }
    }

    /** The static layer: the globe's outline and a dot for every bit of land. */
    const buildBase = () => {
      const proj = projRef.current
      if (!proj) return
      base = document.createElement("canvas")
      base.width = Math.round(w * dpr)
      base.height = Math.round(h * dpr)
      const b = base.getContext("2d")
      if (!b) return
      b.scale(dpr, dpr)
      // Sphere outline.
      b.beginPath()
      geoPath(proj, b)({ type: "Sphere" })
      b.fillStyle = rgba(colors.land, colors.dark ? 0.035 : 0.05)
      b.fill()
      b.strokeStyle = rgba(colors.land, 0.18)
      b.lineWidth = 1
      b.stroke()
      // Land mask at 1x, sampled on a grid to place the dots.
      const mask = document.createElement("canvas")
      mask.width = Math.ceil(w)
      mask.height = Math.ceil(h)
      const m = mask.getContext("2d", { willReadFrequently: true })
      if (!m) return
      m.fillStyle = "#000"
      m.beginPath()
      geoPath(proj, m)(LAND)
      m.fill()
      const data = m.getImageData(0, 0, mask.width, mask.height).data
      const step = Math.max(4.5, w / 190)
      const r = Math.max(0.9, step * 0.24)
      b.fillStyle = rgba(colors.land, colors.dark ? 0.42 : 0.5)
      for (let y = step / 2; y < h; y += step) {
        for (let x = step / 2; x < w; x += step) {
          const i = (Math.floor(y) * mask.width + Math.floor(x)) * 4 + 3
          if (data[i] > 0) {
            b.beginPath()
            b.arc(x, y, r, 0, Math.PI * 2)
            b.fill()
          }
        }
      }
    }

    const resize = () => {
      dpr = Math.min(window.devicePixelRatio || 1, 2)
      w = wrap.clientWidth
      h = Math.round(w / 2.05)
      canvas.width = Math.round(w * dpr)
      canvas.height = Math.round(h * dpr)
      canvas.style.height = `${h}px`
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      projRef.current = geoEqualEarth().fitExtent(
        [
          [6, 6],
          [w - 6, h - 6],
        ],
        { type: "Sphere" }
      )
      particles = []
      buildBase()
    }

    // Marks scale with the map, so a phone isn't buried in dots.
    const k = () => Math.min(1, Math.max(0.45, w / 1100))
    const radius = (p: PeerPlace) => Math.min(12, 2.2 + Math.sqrt(p.peers) * 1.7) * k()

    const draw = (now: number) => {
      const dt = Math.min(64, now - last)
      last = now
      const proj = projRef.current
      if (!proj || !base) return
      const { places, home } = live.current
      ctx.clearRect(0, 0, w, h)
      ctx.drawImage(base, 0, 0, w, h)
      const hp = home ? proj([home.lon, home.lat]) : null
      const busiest = Math.max(1, ...places.map((p) => p.dl + p.ul))

      // Arcs to everyone we're trading with.
      if (hp) {
        ctx.lineWidth = 1
        for (const p of places) {
          const rate = p.dl + p.ul
          if (rate <= 0) continue
          const pp = proj([p.lon, p.lat])
          if (!pp) continue
          const [cx, cy] = control(pp[0], pp[1], hp[0], hp[1])
          const col = p.dl >= p.ul ? colors.down : colors.up
          ctx.strokeStyle = rgba(col, (reduced ? 0.35 : 0.1) + 0.25 * Math.sqrt(rate / busiest))
          ctx.beginPath()
          ctx.moveTo(pp[0], pp[1])
          ctx.quadraticCurveTo(cx, cy, hp[0], hp[1])
          ctx.stroke()
          if (reduced) continue
          // Spawn pieces: more for busier places, log-scaled so a slow peer
          // still visibly trickles.
          const dist = Math.hypot(hp[0] - pp[0], hp[1] - pp[1])
          const speed = 1 / (1100 + (dist / w) * 2600)
          for (const [bytes, col2, inbound] of [
            [p.dl, colors.down, true],
            [p.ul, colors.up, false],
          ] as const) {
            if (bytes <= 0) continue
            const perSec = Math.min(9, Math.log2(1 + bytes / 8192) * 0.7) * k()
            if (Math.random() < (perSec * dt) / 1000 && particles.length < 900) {
              const [ax, ay, bx, by] = inbound ? [pp[0], pp[1], hp[0], hp[1]] : [hp[0], hp[1], pp[0], pp[1]]
              particles.push({ ax, ay, cx, cy, bx, by, t: 0, speed: speed * (0.8 + Math.random() * 0.4), color: col2 })
            }
          }
        }
      }

      // Pieces in flight, each with a short fading tail.
      // Overlapping pieces brighten each other on dark themes ("screen", not
      // "lighter", which saturates to white where arcs bunch up).
      ctx.globalCompositeOperation = colors.dark ? "screen" : "source-over"
      const size = 1.9 * k()
      particles = particles.filter((q) => (q.t += q.speed * dt) < 1)
      for (const q of particles) {
        for (let j = 0; j < 4; j++) {
          const t = q.t - j * 0.018
          if (t < 0) break
          const [x, y] = bez(q, t)
          ctx.fillStyle = rgba(q.color, (1 - j / 4) * 0.9)
          ctx.beginPath()
          ctx.arc(x, y, size * (1 - j * 0.16), 0, Math.PI * 2)
          ctx.fill()
        }
      }
      ctx.globalCompositeOperation = "source-over"

      // Places.
      for (const p of places) {
        const pp = proj([p.lon, p.lat])
        if (!pp) continue
        const r = radius(p)
        const active = p.dl + p.ul > 0
        const col = !active ? colors.idle : p.dl >= p.ul ? colors.down : colors.up
        if (active) {
          ctx.fillStyle = rgba(col, 0.16)
          ctx.beginPath()
          ctx.arc(pp[0], pp[1], r * 2.1, 0, Math.PI * 2)
          ctx.fill()
        }
        ctx.fillStyle = rgba(col, active ? 0.95 : 0.55)
        ctx.beginPath()
        ctx.arc(pp[0], pp[1], r, 0, Math.PI * 2)
        ctx.fill()
      }

      // You.
      if (hp) {
        const pulse = reduced ? 0 : (now % 2400) / 2400
        ctx.strokeStyle = rgba(colors.home, 0.7 * (1 - pulse))
        ctx.lineWidth = 1.5
        ctx.beginPath()
        ctx.arc(hp[0], hp[1], 6 + pulse * 18, 0, Math.PI * 2)
        ctx.stroke()
        ctx.fillStyle = rgba(colors.home, 1)
        ctx.beginPath()
        ctx.arc(hp[0], hp[1], 5, 0, Math.PI * 2)
        ctx.fill()
        ctx.lineWidth = 2
        ctx.strokeStyle = colors.dark ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.9)"
        ctx.stroke()
      }
    }

    const loop = (now: number) => {
      draw(now)
      raf = requestAnimationFrame(loop)
    }

    paint()
    resize()
    const ro = new ResizeObserver(() => {
      if (wrap.clientWidth !== w) resize()
    })
    ro.observe(wrap)
    const mo = new MutationObserver(() => {
      paint()
      buildBase()
    })
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-palette"] })
    if (reduced) {
      // Static: redraw a few times a second as data changes.
      const id = setInterval(() => draw(performance.now()), 500)
      draw(performance.now())
      return () => {
        clearInterval(id)
        ro.disconnect()
        mo.disconnect()
      }
    }
    raf = requestAnimationFrame(loop)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      mo.disconnect()
    }
  }, [reduced])

  const onMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const proj = projRef.current
    if (!proj) return
    const rect = e.currentTarget.getBoundingClientRect()
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top
    let best: PeerPlace | null = null
    let bestD = 18
    for (const p of places) {
      const pp = proj([p.lon, p.lat])
      if (!pp) continue
      const d = Math.hypot(pp[0] - x, pp[1] - y)
      if (d < bestD) {
        best = p
        bestD = d
      }
    }
    setHover(best && !picking ? { place: best, x, y, width: rect.width } : null)
  }

  const onClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!picking || !onPick || !projRef.current?.invert) return
    const rect = e.currentTarget.getBoundingClientRect()
    const ll = projRef.current.invert([e.clientX - rect.left, e.clientY - rect.top])
    if (ll && Number.isFinite(ll[0]) && Number.isFinite(ll[1])) {
      onPick({ lon: Math.round(ll[0] * 100) / 100, lat: Math.round(ll[1] * 100) / 100 })
    }
  }

  const regions = useMemo(() => {
    try {
      return new Intl.DisplayNames(undefined, { type: "region" })
    } catch {
      return null
    }
  }, [])

  return (
    <div ref={wrapRef} className={cn("relative w-full", className)}>
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={`Map of ${places.reduce((n, p) => n + p.peers, 0)} peers in ${places.length} places`}
        className={cn("block w-full", picking ? "cursor-crosshair" : hover ? "cursor-default" : undefined)}
        onPointerMove={onMove}
        onPointerLeave={() => setHover(null)}
        onClick={onClick}
      />
      {hover && (
        <div
          className="pointer-events-none absolute z-10 grid min-w-44 gap-1 rounded-lg border bg-popover px-3 py-2 text-xs shadow-lg"
          style={{
            left: Math.max(0, Math.min(hover.x + 14, hover.width - 190)),
            top: Math.max(0, hover.y - 70),
          }}
        >
          <div className="font-medium text-foreground">
            {hover.place.city ? `${hover.place.city}, ` : ""}
            {regions?.of(hover.place.country) ?? hover.place.country}
          </div>
          <div className="flex justify-between gap-4 text-muted-foreground">
            <span>Peers</span>
            <span className="data text-foreground">{hover.place.peers}</span>
          </div>
          <div className="flex items-center gap-2 text-muted-foreground">
            <span className="size-2.5 rounded-[3px] bg-(--down)" />
            Download
            <span className="data ml-auto text-foreground">{formatSpeed(hover.place.dl)}</span>
          </div>
          <div className="flex items-center gap-2 text-muted-foreground">
            <span className="size-2.5 rounded-[3px] bg-(--up)" />
            Upload
            <span className="data ml-auto text-foreground">{formatSpeed(hover.place.ul)}</span>
          </div>
        </div>
      )}
    </div>
  )
}
