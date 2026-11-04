import { useEffect, useRef } from "react"

export type SwarmMode = "idle" | "busy" | "error" | "success"

/** The logo's 14 particles in its 64x64 box (see components/logo.tsx). */
const ANCHORS = Array.from({ length: 14 }, (_, i) => {
  const t = i / 13
  return { x: 32 + 17 * Math.sin(t * Math.PI * 2), y: 8 + t * 48, r: 1.8 + t * 3.9 }
})

/**
 * The idle loop: hold the S (with pieces trickling in and out), leave the
 * screen in one of three ways, then stream back in and re-form.
 */
const HOLD = 8500
const LEAVE = 2600
const RETURN = 3400
const CYCLE = HOLD + LEAVE + RETURN
type Exit = "gust" | "burst" | "vortex"
const EXITS: Exit[] = ["gust", "burst", "vortex"]

/** Colour steps along the ribbon, tail to head, plus the upload colour. */
const STEPS = 8
const UP = STEPS

interface Particle {
  x: number
  y: number
  vx: number
  vy: number
  /** Which logo particle this one belongs to, and where inside it. */
  anchor: number
  ox: number
  oy: number
  size: number
  /** 0 is far away (small, dim), 1 is close. */
  depth: number
  phase: number
  tone: number
  /** Off screen, waiting to come back in at `release`. */
  parked: boolean
  release: number
  /** Leaving as an uploaded piece; it comes back in from another edge. */
  away: boolean
  /** Has started this cycle's exit. */
  left: boolean
}

type RGB = [number, number, number]

function cssVar(name: string, fallback: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback
}

/** Resolve any CSS colour (oklch, hex…) to sRGB by painting one pixel. */
function rgbOf(color: string): RGB {
  const c = document.createElement("canvas")
  c.width = c.height = 1
  const x = c.getContext("2d", { willReadFrequently: true })
  if (!x) return [150, 200, 100]
  x.fillStyle = "#000"
  x.fillStyle = color
  x.fillRect(0, 0, 1, 1)
  const d = x.getImageData(0, 0, 1, 1).data
  return [d[0], d[1], d[2]]
}

const mix = (a: RGB, b: RGB, t: number): RGB => [0, 1, 2].map((i) => Math.round(a[i] + (b[i] - a[i]) * t)) as RGB

/** The sprite's solid core is this fraction of its width. */
const CORE = 0.11

/**
 * A shaded bead: a lit centre, the base colour, and (on dark themes) a soft
 * halo. Pre-rendered once per colour so a thousand of them stay cheap.
 */
function bead(base: RGB, glow: boolean) {
  const S = 96
  const c = document.createElement("canvas")
  c.width = c.height = S
  const x = c.getContext("2d")
  if (!x) return c
  const g = x.createRadialGradient(S * 0.47, S * 0.46, 0, S / 2, S / 2, S / 2)
  const col = ([r, gg, b]: RGB, a: number) => `rgba(${r},${gg},${b},${a})`
  const lit = mix(base, [255, 255, 255], 0.45)
  g.addColorStop(0, col(lit, 1))
  g.addColorStop(CORE * 1.2, col(base, 1))
  g.addColorStop(CORE * 2, col(base, 0.95))
  g.addColorStop(CORE * 2.3, col(base, glow ? 0.26 : 0))
  g.addColorStop(0.55, col(base, glow ? 0.06 : 0))
  g.addColorStop(1, col(base, 0))
  x.fillStyle = g
  x.fillRect(0, 0, S, S)
  return c
}

/**
 * A swarm of particles that flies in and assembles into the Swarmy S, trades
 * pieces with the edges of the screen, periodically leaves and comes back,
 * dodges the pointer, bursts on a failed sign-in and rushes into the S on
 * success.
 */
export function SwarmCanvas({ mode, className }: { mode: SwarmMode; className?: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const modeRef = useRef<{ mode: SwarmMode; since: number }>({ mode, since: 0 })

  useEffect(() => {
    modeRef.current = { mode, since: performance.now() }
  }, [mode])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext("2d")
    if (!ctx) return
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches

    let w = 0
    let h = 0
    let particles: Particle[] = []
    let layout = { cx: 0, cy: 0, scale: 1 }
    const pointer = { x: -9999, y: -9999 }
    let raf = 0
    let burstDone = 0
    let seenMode = modeRef.current.since
    let trickle = 0
    let sprites: HTMLCanvasElement[] = []
    let dark = true
    // Start as if the previous cycle just left, so the first thing you see
    // is the swarm streaming in.
    const cycle = { start: performance.now() - HOLD - LEAVE, exit: "burst" as Exit, angle: 0, n: 0, left: true, back: false }

    const paint = () => {
      dark = document.documentElement.classList.contains("dark")
      const tail = rgbOf(cssVar("--down", "#5f9e3a"))
      const head = rgbOf(cssVar("--sidebar-primary", "#9ccc65"))
      const up = rgbOf(cssVar("--up", "#3d93c7"))
      sprites = [...Array.from({ length: STEPS }, (_, i) => bead(mix(tail, head, i / (STEPS - 1)), dark)), bead(up, dark)]
    }

    const place = () => {
      const desktop = w >= 1024
      // On desktop the S sits above the wordmark, in the left column. On
      // phones it sits in the top half, with the card below it.
      const size = desktop ? Math.min(h * 0.6, w * 0.36) : Math.min(h * 0.34, w * 0.72)
      layout = { cx: desktop ? w * 0.3 : w * 0.5, cy: desktop ? h * 0.36 : h * 0.21, scale: size / 64 }
    }

    const offscreen = (p: Particle) => p.x < -40 || p.x > w + 40 || p.y < -40 || p.y > h + 40

    /** A point just outside the screen, in direction `ang` from the S. */
    const ring = (ang: number) => {
      const r = Math.hypot(w, h) * 0.5 + 60
      return { x: w / 2 + Math.cos(ang) * r, y: h / 2 + Math.sin(ang) * r }
    }

    const target = (p: Particle, t: number, m: SwarmMode) => {
      const a = ANCHORS[p.anchor]
      const s = layout.scale
      let x = layout.cx + (a.x - 32) * s + p.ox * a.r * s
      let y = layout.cy + (a.y - 32) * s + p.oy * a.r * s
      // Each particle orbits its spot a little; faster while signing in.
      const speed = m === "busy" ? 3.2 : 1
      const wobble = a.r * s * (m === "busy" ? 0.12 : 0.22)
      x += Math.cos(t * 0.0011 * speed + p.phase) * wobble
      y += Math.sin(t * 0.0013 * speed + p.phase * 1.3) * wobble
      if (m === "success") {
        // Everything rushes into the head of the S.
        const head = ANCHORS[13]
        x = layout.cx + (head.x - 32) * s + p.ox * 2
        y = layout.cy + (head.y - 32) * s + p.oy * 2
      }
      return { x, y }
    }

    /** Bring a parked particle back in from just off screen. */
    const respawn = (p: Particle, ang: number, release: number, swirl = 0) => {
      const at = ring(ang)
      p.x = at.x
      p.y = at.y
      // A little sideways velocity makes arrivals curve in rather than
      // arrive in straight lines.
      p.vx = -Math.sin(ang) * swirl
      p.vy = Math.cos(ang) * swirl
      p.parked = true
      p.release = release
      p.away = false
      p.left = false
    }

    const build = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2)
      w = canvas.clientWidth
      h = canvas.clientHeight
      canvas.width = Math.round(w * dpr)
      canvas.height = Math.round(h * dpr)
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      place()
      const count = Math.round(Math.min(1100, Math.max(380, (w * h) / 1500)))
      // Bigger logo particles get more members, so the S keeps its shape.
      const weights = ANCHORS.map((a) => a.r * a.r)
      const total = weights.reduce((x, y) => x + y, 0)
      particles = Array.from({ length: count }, () => {
        let pick = Math.random() * total
        let anchor = 0
        while (pick > weights[anchor]) pick -= weights[anchor++]
        const ang = Math.random() * Math.PI * 2
        const rad = Math.sqrt(Math.random()) * 0.95
        const step = Math.round((anchor / 13) * (STEPS - 1) + (Math.random() - 0.5) * 1.6)
        return {
          x: 0,
          y: 0,
          vx: 0,
          vy: 0,
          anchor,
          ox: Math.cos(ang) * rad,
          oy: Math.sin(ang) * rad,
          size: 0.8 + Math.random() * 1.6 + ANCHORS[anchor].r * 0.09,
          depth: Math.random() ** 0.7,
          phase: Math.random() * Math.PI * 2,
          tone: Math.random() < 0.05 ? UP : Math.min(STEPS - 1, Math.max(0, step)),
          parked: true,
          release: 0,
          away: false,
          left: false,
        }
      })
      if (reduced) {
        for (const p of particles) {
          const tg = target(p, 0, "idle")
          p.x = tg.x
          p.y = tg.y
          p.parked = false
        }
        draw(1)
        return
      }
      // Restart the loop at "coming back", so a resize re-forms the S.
      cycle.start = performance.now() - HOLD - LEAVE
      cycle.left = true
      cycle.back = false
    }

    const draw = (trail: number) => {
      // Fade what's there towards transparent (the page background and its
      // aura show through) for short motion trails.
      ctx.globalCompositeOperation = "destination-out"
      ctx.globalAlpha = trail
      ctx.fillRect(0, 0, w, h)
      // On dark themes overlapping beads brighten each other, so dense parts
      // glow ("screen" rather than "lighter", which blows out to white).
      ctx.globalCompositeOperation = dark ? "screen" : "source-over"
      for (const p of particles) {
        if (p.parked) continue
        const d = (p.size * (0.6 + 0.6 * p.depth)) / CORE
        ctx.globalAlpha = (0.3 + 0.7 * p.depth) * (dark ? 0.8 : 1)
        ctx.drawImage(sprites[p.away ? UP : p.tone], p.x - d / 2, p.y - d / 2, d, d)
      }
      ctx.globalCompositeOperation = "source-over"
      ctx.globalAlpha = 1
    }

    /** Move the idle loop along: start exits, bring everyone back, repeat. */
    const choreograph = (t: number) => {
      let age = t - cycle.start
      if (age >= CYCLE) {
        cycle.start = t
        cycle.n++
        cycle.exit = EXITS[cycle.n % EXITS.length]
        cycle.angle = Math.random() * Math.PI * 2
        cycle.left = false
        cycle.back = false
        age = 0
      }
      if (!cycle.left && age >= HOLD) {
        cycle.left = true
        for (const p of particles) p.left = false
      }
      if (!cycle.back && age >= HOLD + LEAVE) {
        cycle.back = true
        for (const p of particles) {
          if (!p.parked && !offscreen(p)) continue
          const delay = Math.random() * 1100
          if (cycle.exit === "gust") {
            // Carry on with the wind: back in from the upwind side.
            respawn(p, cycle.angle + Math.PI + (Math.random() - 0.5) * 1.4, t + delay, 3)
          } else if (cycle.exit === "vortex") {
            respawn(p, Math.random() * Math.PI * 2, t + delay, 11)
          } else {
            respawn(p, Math.random() * Math.PI * 2, t + delay)
          }
        }
      }
      // While the S holds, pieces trickle out (in the upload colour) and new
      // ones arrive, like a swarm trading pieces.
      if (age > 1200 && age < HOLD - 400) {
        trickle += (particles.length * 0.011) / 60
        while (trickle >= 1) {
          trickle--
          const p = particles[Math.floor(Math.random() * particles.length)]
          if (p.parked || p.away) continue
          const ang = Math.atan2(p.y - layout.cy, p.x - layout.cx) + (Math.random() - 0.5) * 1.2
          const v = 2.5 + Math.random() * 3
          p.away = true
          p.vx = Math.cos(ang) * v
          p.vy = Math.sin(ang) * v
        }
      }
      return age >= HOLD && age < HOLD + LEAVE ? age - HOLD : -1
    }

    const step = (t: number) => {
      const { mode: m, since } = modeRef.current
      if (since !== seenMode) {
        seenMode = since
        if (m === "idle") {
          // Back from signing in or an error: hold the S for a full spell.
          cycle.start = t
          cycle.left = false
          cycle.back = false
        } else {
          // Signing in needs the whole swarm: call everyone back now.
          cycle.left = true
          cycle.back = true
          for (const p of particles) {
            if (p.parked || p.away || offscreen(p)) respawn(p, Math.random() * Math.PI * 2, t)
          }
        }
      }
      const age = t - since
      if (m === "error" && burstDone !== since) {
        burstDone = since
        for (const p of particles) {
          const ang = Math.random() * Math.PI * 2
          const kick = 6 + Math.random() * 10
          p.vx += Math.cos(ang) * kick
          p.vy += Math.sin(ang) * kick
        }
      }
      const leaving = m === "idle" ? choreograph(t) : -1
      const pull = m === "success" ? 0.06 : m === "busy" ? 0.03 : m === "error" && age < 700 ? 0.004 : 0.014
      const damping = m === "success" ? 0.82 : 0.9
      for (const p of particles) {
        if (p.parked) {
          if (t < p.release) continue
          p.parked = false
        }
        if (p.away) {
          // An uploaded piece speeds off; it comes back in from elsewhere.
          p.vx *= 1.02
          p.vy *= 1.02
          p.x += p.vx
          p.y += p.vy
          if (offscreen(p)) respawn(p, Math.random() * Math.PI * 2, t + Math.random() * 400, (Math.random() - 0.5) * 8)
          continue
        }
        // The ribbon peels away tail first.
        if (leaving >= 0 && leaving > (p.anchor / 13) * 700 + p.phase * 40) {
          const dx = p.x - layout.cx
          const dy = p.y - layout.cy
          const r = Math.hypot(dx, dy) || 1
          if (cycle.exit === "gust") {
            const wob = Math.sin(t * 0.004 + p.phase * 3) * 0.3
            p.vx += Math.cos(cycle.angle) * 0.42 - Math.sin(cycle.angle) * wob
            p.vy += Math.sin(cycle.angle) * 0.42 + Math.cos(cycle.angle) * wob
          } else if (cycle.exit === "burst") {
            if (!p.left) {
              const kick = 7 + Math.random() * 9
              p.vx += (dx / r) * kick
              p.vy += (dy / r) * kick
            }
            p.vx += (dx / r) * 0.08
            p.vy += (dy / r) * 0.08
          } else {
            p.vx += (-dy / r) * 0.6 + (dx / r) * 0.16
            p.vy += (dx / r) * 0.6 + (dy / r) * 0.16
          }
          p.left = true
          p.vx *= 0.985
          p.vy *= 0.985
          p.x += p.vx
          p.y += p.vy
          if (offscreen(p)) p.parked = true
          continue
        }
        const tg = target(p, t, m)
        p.vx += (tg.x - p.x) * pull
        p.vy += (tg.y - p.y) * pull
        // The pointer pushes particles aside.
        const dx = p.x - pointer.x
        const dy = p.y - pointer.y
        const d2 = dx * dx + dy * dy
        if (d2 < 120 * 120 && d2 > 0.01) {
          const d = Math.sqrt(d2)
          const f = (1 - d / 120) * 2.4
          p.vx += (dx / d) * f
          p.vy += (dy / d) * f
        }
        p.vx *= damping
        p.vy *= damping
        p.x += p.vx
        p.y += p.vy
      }
      draw(0.3)
      raf = requestAnimationFrame(step)
    }

    const onMove = (e: PointerEvent) => {
      const r = canvas.getBoundingClientRect()
      pointer.x = e.clientX - r.left
      pointer.y = e.clientY - r.top
    }
    const onLeave = () => {
      pointer.x = -9999
      pointer.y = -9999
    }
    paint()
    const ro = new ResizeObserver(() => build())
    ro.observe(canvas)
    build()
    if (!reduced) {
      window.addEventListener("pointermove", onMove)
      window.addEventListener("pointerleave", onLeave)
      raf = requestAnimationFrame(step)
    }
    // Recolour if the theme changes.
    const mo = new MutationObserver(() => {
      paint()
      if (reduced) draw(1)
    })
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-palette"] })

    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      mo.disconnect()
      window.removeEventListener("pointermove", onMove)
      window.removeEventListener("pointerleave", onLeave)
    }
  }, [])

  return <canvas ref={canvasRef} className={className} aria-hidden />
}
