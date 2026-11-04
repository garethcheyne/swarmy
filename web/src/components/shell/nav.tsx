import { useState, type ReactNode } from "react"
import {
  ChartColumnIcon,
  ChevronRightIcon,
  EarthIcon,
  ListIcon,
  LogOutIcon,
  MoonIcon,
  SettingsIcon,
  SunIcon,
  type LucideIcon,
} from "lucide-react"

import type { LiveSample, Torrent } from "@/lib/api"
import { STATUS_PREDICATES, matchesFilter, sameFilter, type Filter, type StatusFilter } from "@/lib/filters"
import { href, navigate, type Route } from "@/lib/route"
import { cn } from "@/lib/utils"
import { SETTINGS_SECTIONS, settingsHref, useSettingsSection } from "@/lib/settings-sections"
import { useTheme } from "@/components/theme-provider"
import { LiveSpark } from "@/components/charts"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { Separator } from "@/components/ui/separator"

/** Busy first, then finished, then the ones that need attention. */
const VIEWS: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "active", label: "Active" },
  { value: "downloading", label: "Downloading" },
  { value: "seeding", label: "Seeding" },
  { value: "completed", label: "Completed" },
  { value: "paused", label: "Paused" },
  { value: "stalled", label: "Stalled" },
  { value: "error", label: "Errored" },
]

/** Always listed, even when empty, so the list doesn't jump around. */
const ALWAYS: StatusFilter[] = ["all", "downloading", "seeding"]

type SectionId = "status" | "categories" | "tags" | "settings"
const COLLAPSED_KEY = "swarmy.nav.collapsed"

/** Settings starts folded; it opens by itself on a settings page. */
function readCollapsed(): SectionId[] {
  try {
    const v = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '["settings"]')
    return Array.isArray(v) ? v : ["settings"]
  } catch {
    return ["settings"]
  }
}

interface Props {
  route: Route
  torrents: Torrent[]
  categories: string[]
  history: LiveSample[]
  filter: Filter
  onFilterChange: (f: Filter) => void
  onLogout: () => void
  /** Called after any navigation, so the mobile sheet can close. */
  onNavigate?: () => void
}

const itemClass = (active: boolean) =>
  cn(
    "flex min-h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-sidebar-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
    active && "bg-sidebar-accent text-sidebar-accent-foreground"
  )

function FilterItem({
  active,
  label,
  count,
  onClick,
}: {
  active: boolean
  label: string
  count: number
  onClick: () => void
}) {
  return (
    <button type="button" onClick={onClick} aria-current={active ? "true" : undefined} className={cn(itemClass(active), "justify-between")}>
      <span className="truncate">{label}</span>
      <span className="data text-xs text-sidebar-foreground/70">{count}</span>
    </button>
  )
}

function PageLink({
  to,
  route,
  icon: Icon,
  label,
  count,
  onNavigate,
}: {
  to: Route
  route: Route
  icon: LucideIcon
  label: string
  count?: number
  onNavigate?: () => void
}) {
  return (
    <a href={href(to)} onClick={() => onNavigate?.()} aria-current={route === to ? "page" : undefined} className={itemClass(route === to)}>
      <Icon className="size-4" />
      {label}
      {count !== undefined && <span className="data ml-auto text-xs text-sidebar-foreground/70">{count}</span>}
    </a>
  )
}

/**
 * A filter group that folds away. Folded, it still says how many items it
 * holds, and shows a dot if the filter in use is one of them.
 */
function Section({
  title,
  count,
  open,
  onOpenChange,
  holdsActive,
  children,
}: {
  title: string
  count: number
  open: boolean
  onOpenChange: (open: boolean) => void
  holdsActive: boolean
  children: ReactNode
}) {
  return (
    <Collapsible open={open} onOpenChange={onOpenChange} className="flex flex-col">
      <CollapsibleTrigger className="group flex min-h-7 w-full items-center gap-1.5 rounded-md px-2.5 text-left hover:bg-sidebar-accent/60 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
        <ChevronRightIcon
          aria-hidden
          className="size-3.5 text-sidebar-foreground/60 transition-transform duration-200 group-data-[panel-open]:rotate-90"
        />
        <span className="label-caps">{title}</span>
        {!open && (
          <span className="ml-auto flex items-center gap-1.5">
            {holdsActive && <span className="size-1.5 rounded-full bg-sidebar-primary" aria-label="Current filter is in here" />}
            <span className="data text-xs text-sidebar-foreground/60">{count}</span>
          </span>
        )}
      </CollapsibleTrigger>
      <CollapsibleContent className="h-(--collapsible-panel-height) overflow-hidden transition-[height] duration-200 ease-out data-[ending-style]:h-0 data-[starting-style]:h-0 motion-reduce:transition-none">
        <div className="flex flex-col gap-0.5 pt-0.5">{children}</div>
      </CollapsibleContent>
    </Collapsible>
  )
}

export function Nav({
  route,
  torrents,
  categories,
  history,
  filter,
  onFilterChange,
  onLogout,
  onNavigate,
}: Props) {
  const { theme, setTheme } = useTheme()
  const settingsSection = useSettingsSection()
  const isDark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)
  const [collapsed, setCollapsed] = useState<SectionId[]>(readCollapsed)
  const toggle = (id: SectionId) => (open: boolean) => {
    const next = open ? collapsed.filter((c) => c !== id) : [...new Set([...collapsed, id])]
    setCollapsed(next)
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(next))
    } catch {
      // Storage blocked: the sections just reset next time.
    }
  }

  const pick = (f: Filter) => {
    onFilterChange(f)
    if (route !== "torrents") navigate("torrents")
    onNavigate?.()
  }
  const onTorrents = route === "torrents"
  // In Settings the filters fold away so its sections fit; their headers
  // (with counts) stay, and picking a filter goes back to Torrents.
  const foldFilters = route === "settings"
  const views = VIEWS.filter((v) => ALWAYS.includes(v.value) || torrents.some(STATUS_PREDICATES[v.value]))
  const tags = [...new Set(torrents.flatMap((t) => t.tags))].sort((a, b) => a.localeCompare(b))
  const holds = (kind: Filter["kind"]) => onTorrents && filter.kind === kind && !(kind === "status" && filter.value === "all")

  const prefButton = (icon: ReactNode, label: ReactNode, onClick: () => void) => (
    <button type="button" onClick={onClick} className={itemClass(false)}>
      {icon}
      {label}
    </button>
  )

  return (
    <nav aria-label="Swarmy" className="flex h-full flex-col gap-4 overflow-y-auto bg-sidebar p-2 pt-3 text-[13px]">
      <div className="flex flex-col gap-0.5">
        <PageLink to="torrents" route={route} icon={ListIcon} label="Torrents" count={torrents.length} onNavigate={onNavigate} />
        <PageLink to="statistics" route={route} icon={ChartColumnIcon} label="Statistics" onNavigate={onNavigate} />
        <PageLink to="peers" route={route} icon={EarthIcon} label="Swarm map" onNavigate={onNavigate} />
      </div>

      <div className="flex flex-col gap-3">
        <Section
          title="Status"
          count={views.length}
          open={!foldFilters && !collapsed.includes("status")}
          onOpenChange={toggle("status")}
          holdsActive={holds("status")}
        >
          {views.map((v) => {
            const f: Filter = { kind: "status", value: v.value }
            return (
              <FilterItem
                key={v.value}
                label={v.label}
                count={torrents.filter(STATUS_PREDICATES[v.value]).length}
                active={onTorrents && sameFilter(filter, f)}
                onClick={() => pick(f)}
              />
            )
          })}
        </Section>

        {categories.length > 0 && (
          <Section
            title="Categories"
            count={categories.length}
            open={!foldFilters && !collapsed.includes("categories")}
            onOpenChange={toggle("categories")}
            holdsActive={holds("category")}
          >
            {categories.map((c) => {
              const f: Filter = { kind: "category", value: c }
              return (
                <FilterItem
                  key={c}
                  label={c}
                  count={torrents.filter((t) => matchesFilter(t, f)).length}
                  active={onTorrents && sameFilter(filter, f)}
                  onClick={() => pick(f)}
                />
              )
            })}
          </Section>
        )}

        {tags.length > 0 && (
          <Section
            title="Tags"
            count={tags.length}
            open={!foldFilters && !collapsed.includes("tags")}
            onOpenChange={toggle("tags")}
            holdsActive={holds("tag")}
          >
            {tags.map((tag) => {
              const f: Filter = { kind: "tag", value: tag }
              return (
                <FilterItem
                  key={tag}
                  label={tag}
                  count={torrents.filter((t) => matchesFilter(t, f)).length}
                  active={onTorrents && sameFilter(filter, f)}
                  onClick={() => pick(f)}
                />
              )
            })}
          </Section>
        )}
      </div>

      <Collapsible
        open={route === "settings" || !collapsed.includes("settings")}
        onOpenChange={toggle("settings")}
        className="flex flex-col"
      >
        <CollapsibleTrigger className={cn(itemClass(false), "group")}>
          <SettingsIcon className="size-4" />
          Settings
          <ChevronRightIcon
            aria-hidden
            className="ml-auto size-3.5 text-sidebar-foreground/60 transition-transform duration-200 group-data-[panel-open]:rotate-90"
          />
        </CollapsibleTrigger>
        <CollapsibleContent className="h-(--collapsible-panel-height) overflow-hidden transition-[height] duration-200 ease-out data-[ending-style]:h-0 data-[starting-style]:h-0 motion-reduce:transition-none">
          <div className="ml-[17px] flex flex-col gap-0.5 border-l border-sidebar-border pt-0.5 pl-2">
            {SETTINGS_SECTIONS.map((sec) => {
              const active = route === "settings" && settingsSection === sec.id
              return (
                <a
                  key={sec.id}
                  href={settingsHref(sec.id)}
                  onClick={() => onNavigate?.()}
                  aria-current={active ? "page" : undefined}
                  className={cn(itemClass(active), "min-h-7")}
                >
                  {sec.label}
                </a>
              )
            })}
          </div>
        </CollapsibleContent>
      </Collapsible>

      <div className="mt-auto flex flex-col gap-3 pt-2">
        <a
          href={href("statistics")}
          onClick={() => onNavigate?.()}
          className="flex flex-col gap-1 rounded-md border border-sidebar-border p-2.5 transition-colors hover:bg-sidebar-accent/60"
          aria-label="Last 10 minutes of throughput; open Statistics"
        >
          <span className="text-[11px] text-sidebar-foreground/70">Last 10 minutes</span>
          <LiveSpark points={history.slice(-120)} width={180} height={40} className="h-10 w-full" />
        </a>
        <div className="flex flex-col gap-0.5">
          {prefButton(
            isDark ? <SunIcon className="size-4" /> : <MoonIcon className="size-4" />,
            isDark ? "Light mode" : "Dark mode",
            () => setTheme(isDark ? "light" : "dark")
          )}
          <Separator className="my-1 bg-sidebar-border" />
          {prefButton(<LogOutIcon className="size-4" />, "Sign out", onLogout)}
        </div>
      </div>
    </nav>
  )
}
