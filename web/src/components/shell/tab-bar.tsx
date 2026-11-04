import { ChartColumnIcon, ListIcon, MenuIcon, PlusIcon } from "lucide-react"

import { href, type Route } from "@/lib/route"
import { cn } from "@/lib/utils"

/** Phone navigation, within thumb reach. Hidden from md up. */
export function TabBar({ route, onAdd, onMenu }: { route: Route; onAdd: () => void; onMenu: () => void }) {
  const item = "flex min-h-12 flex-1 flex-col items-center justify-center gap-0.5 text-[11px] font-medium text-sidebar-foreground/70"
  return (
    <nav
      aria-label="Main"
      className="flex shrink-0 items-stretch border-t bg-sidebar px-2 pb-[env(safe-area-inset-bottom)] md:hidden"
    >
      <a href={href("torrents")} className={cn(item, route === "torrents" && "text-sidebar-accent-foreground")} aria-current={route === "torrents" ? "page" : undefined}>
        <ListIcon className="size-5" />
        Torrents
      </a>
      <button type="button" onClick={onAdd} className={item} aria-label="Add torrent">
        <span className="grid size-10 place-items-center rounded-full bg-sidebar-primary text-sidebar-primary-foreground">
          <PlusIcon className="size-5" />
        </span>
      </button>
      <a href={href("statistics")} className={cn(item, route === "statistics" && "text-sidebar-accent-foreground")} aria-current={route === "statistics" ? "page" : undefined}>
        <ChartColumnIcon className="size-5" />
        Statistics
      </a>
      <button type="button" onClick={onMenu} className={item}>
        <MenuIcon className="size-5" />
        More
      </button>
    </nav>
  )
}
