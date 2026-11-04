import { useMemo } from "react"
import {
  BellIcon,
  ChartColumnIcon,
  LogOutIcon,
  MoonIcon,
  PaletteIcon,
  SettingsIcon,
  SunIcon,
} from "lucide-react"

import type { LiveSample, Torrent } from "@/lib/api"
import { STATUS_PREDICATES, matchesFilter, sameFilter, type Filter, type StatusFilter } from "@/lib/filters"
import { PALETTES, usePalette } from "@/lib/palette"
import { href, navigate, type Route } from "@/lib/route"
import { cn } from "@/lib/utils"
import { useTheme } from "@/components/theme-provider"

const VIEWS: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "downloading", label: "Downloading" },
  { value: "seeding", label: "Seeding" },
  { value: "completed", label: "Completed" },
  { value: "active", label: "Active" },
  { value: "stalled", label: "Stalled" },
  { value: "error", label: "Errored" },
  { value: "paused", label: "Paused" },
]

interface Props {
  route: Route
  torrents: Torrent[]
  categories: string[]
  history: LiveSample[]
  filter: Filter
  onFilterChange: (f: Filter) => void
  onNotifications: () => void
  onAppearance: () => void
  onLogout: () => void
  /** Called after any navigation, so the mobile sheet can close. */
  onNavigate?: () => void
}

function NavItem({
  active,
  label,
  count,
  onClick,
}: {
  active: boolean
  label: string
  count?: number
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "true" : undefined}
      className={cn(
        "flex min-h-8 w-full items-center justify-between gap-2 rounded-md px-2.5 text-left text-sidebar-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        active && "bg-sidebar-accent text-sidebar-accent-foreground"
      )}
    >
      <span className="truncate">{label}</span>
      {count !== undefined && <span className="data text-xs text-sidebar-foreground/70">{count}</span>}
    </button>
  )
}

/** Download/upload over the last 10 minutes, as two thin lines. */
function MiniThroughput({ history }: { history: LiveSample[] }) {
  const { dl, ul } = useMemo(() => {
    const pts = history.slice(-120)
    const max = Math.max(1, ...pts.map((p) => Math.max(p.dl, p.ul)))
    const line = (key: "dl" | "ul") =>
      pts
        .map((p, i) => `${((i / Math.max(1, pts.length - 1)) * 180).toFixed(1)},${(38 - (p[key] / max) * 34).toFixed(1)}`)
        .join(" ")
    return { dl: line("dl"), ul: line("ul") }
  }, [history])
  return (
    <svg viewBox="0 0 180 40" className="h-10 w-full" aria-hidden>
      <polyline points={dl} fill="none" stroke="var(--down)" strokeWidth="1.75" strokeLinejoin="round" />
      <polyline points={ul} fill="none" stroke="var(--up)" strokeWidth="1.75" strokeLinejoin="round" />
    </svg>
  )
}

export function Nav({
  route,
  torrents,
  categories,
  history,
  filter,
  onFilterChange,
  onNotifications,
  onAppearance,
  onLogout,
  onNavigate,
}: Props) {
  const palette = usePalette()
  const { theme, setTheme } = useTheme()
  const isDark =
    theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)

  const pick = (f: Filter) => {
    onFilterChange(f)
    if (route !== "torrents") navigate("torrents")
    onNavigate?.()
  }
  const onTorrents = route === "torrents"

  return (
    <nav aria-label="Views" className="flex h-full flex-col gap-5 overflow-y-auto bg-sidebar p-2 pt-3 text-[13px]">
      <div className="flex flex-col gap-0.5">
        <div className="label-caps px-2.5 pb-1.5">Views</div>
        {VIEWS.filter((v) => v.value === "all" || v.value === "downloading" || v.value === "seeding" || torrents.some(STATUS_PREDICATES[v.value])).map((v) => {
          const f: Filter = { kind: "status", value: v.value }
          return (
            <NavItem
              key={v.value}
              label={v.label}
              count={torrents.filter(STATUS_PREDICATES[v.value]).length}
              active={onTorrents && sameFilter(filter, f)}
              onClick={() => pick(f)}
            />
          )
        })}
      </div>

      {categories.length > 0 && (
        <div className="flex flex-col gap-0.5">
          <div className="label-caps px-2.5 pb-1.5">Categories</div>
          {categories.map((c) => {
            const f: Filter = { kind: "category", value: c }
            return (
              <NavItem
                key={c}
                label={c}
                count={torrents.filter((t) => matchesFilter(t, f)).length}
                active={onTorrents && sameFilter(filter, f)}
                onClick={() => pick(f)}
              />
            )
          })}
        </div>
      )}

      <div className="flex flex-col gap-0.5">
        <div className="label-caps px-2.5 pb-1.5">Swarmy</div>
        <a
          href={href("statistics")}
          onClick={() => onNavigate?.()}
          aria-current={route === "statistics" ? "page" : undefined}
          className={cn(
            "flex min-h-8 items-center gap-2 rounded-md px-2.5 text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
            route === "statistics" && "bg-sidebar-accent text-sidebar-accent-foreground"
          )}
        >
          <ChartColumnIcon className="size-4" />
          Statistics
        </a>
        <a
          href={href("settings")}
          onClick={() => onNavigate?.()}
          aria-current={route === "settings" ? "page" : undefined}
          className={cn(
            "flex min-h-8 items-center gap-2 rounded-md px-2.5 text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
            route === "settings" && "bg-sidebar-accent text-sidebar-accent-foreground"
          )}
        >
          <SettingsIcon className="size-4" />
          Settings
        </a>
      </div>

      <div className="mt-auto flex flex-col gap-3">
        <div className="flex flex-col gap-1 rounded-md border border-sidebar-border p-2.5">
          <span className="text-[11px] text-sidebar-foreground/70">Last 10 minutes</span>
          <MiniThroughput history={history} />
        </div>
        <div className="flex flex-col gap-0.5">
          <button
            type="button"
            onClick={() => {
              onAppearance()
              onNavigate?.()
            }}
            className="flex min-h-8 w-full items-center gap-2 rounded-md px-2.5 text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <PaletteIcon className="size-4" />
            Theme
            <span className="ml-auto text-xs text-sidebar-foreground/70">
              {PALETTES.find((p) => p.id === palette)?.label}
            </span>
          </button>
          <button
            type="button"
            onClick={() => setTheme(isDark ? "light" : "dark")}
            className="flex min-h-8 w-full items-center gap-2 rounded-md px-2.5 text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            {isDark ? <SunIcon className="size-4" /> : <MoonIcon className="size-4" />}
            {isDark ? "Light mode" : "Dark mode"}
          </button>
          <button
            type="button"
            onClick={() => {
              onNotifications()
              onNavigate?.()
            }}
            className="flex min-h-8 w-full items-center gap-2 rounded-md px-2.5 text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <BellIcon className="size-4" />
            Notifications
          </button>
          <button
            type="button"
            onClick={onLogout}
            className="flex min-h-8 w-full items-center gap-2 rounded-md px-2.5 text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          >
            <LogOutIcon className="size-4" />
            Sign out
          </button>
        </div>
      </div>
    </nav>
  )
}
