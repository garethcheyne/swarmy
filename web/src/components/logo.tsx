/**
 * The Swarmy mark: a ribbon of particles that grow as they arrive, like
 * pieces landing. Same geometry as the app icons (64x64 box, 14 particles).
 * Colours come from the theme, so it works in every palette.
 */
const PARTICLES = Array.from({ length: 14 }, (_, i) => {
  const t = i / 13
  return {
    x: 32 + 17 * Math.sin(t * Math.PI * 2),
    y: 8 + t * 48,
    r: 1.8 + t * 3.9,
    tail: t < 0.45,
  }
})

export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 64 64" className={className} aria-hidden>
      {PARTICLES.map((p, i) => (
        <circle
          key={i}
          cx={p.x.toFixed(2)}
          cy={p.y.toFixed(2)}
          r={p.r.toFixed(2)}
          fill={
            p.tail
              ? "color-mix(in oklab, var(--sidebar-primary) 60%, var(--sidebar))"
              : "var(--sidebar-primary)"
          }
        />
      ))}
    </svg>
  )
}

/** The mark on its app-icon tile, for the sign-in screen and empty states. */
export function LogoTile({ className }: { className?: string }) {
  return (
    <span
      className={`grid place-items-center rounded-[23%] border border-sidebar-border bg-linear-160 from-sidebar-accent to-sidebar ${className ?? ""}`}
    >
      <Logo className="size-[68%]" />
    </span>
  )
}
