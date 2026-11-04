import { MenuIcon, PictureInPicture2Icon, PlusIcon, SearchIcon, TurtleIcon } from "lucide-react"

import type { GlobalStats } from "@/lib/api"
import { formatSpeed } from "@/lib/format"
import { Logo } from "@/components/logo"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"

interface Props {
  stats: GlobalStats | undefined
  demo: boolean
  reconnecting: boolean
  onOpenCommand: () => void
  onOpenMenu: () => void
  onAdd: () => void
  onToggleAlt: () => void
  onFloat: () => void
}

export function TopBar({ stats, demo, reconnecting, onOpenCommand, onOpenMenu, onAdd, onToggleAlt, onFloat }: Props) {
  const alt = !!stats?.alt_speed
  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b bg-sidebar px-3 pt-[env(safe-area-inset-top)] text-sidebar-foreground md:gap-4 md:px-4 box-content">
      <Button
        variant="ghost"
        size="icon"
        className="text-sidebar-foreground md:hidden"
        aria-label="Open menu"
        onClick={onOpenMenu}
      >
        <MenuIcon />
      </Button>
      <a href="#/" className="flex items-center gap-2 font-bold text-sidebar-accent-foreground">
        <Logo className="size-5" />
        <span className="text-[16px] tracking-[-0.01em]">Swarmy</span>
      </a>
      {demo && <Badge className="bg-sidebar-primary text-sidebar-primary-foreground">Demo</Badge>}

      <button
        type="button"
        onClick={onOpenCommand}
        className="hidden h-8 max-w-xl flex-1 items-center gap-2 rounded-md border border-sidebar-border bg-background/40 px-2.5 text-left text-[13px] text-sidebar-foreground/70 hover:border-sidebar-ring/50 md:flex"
      >
        <SearchIcon className="size-3.5" />
        <span className="truncate">Search torrents or type a command</span>
        <Kbd className="ml-auto">Ctrl K</Kbd>
      </button>

      <div className="ml-auto flex items-center gap-3 md:gap-4">
        {reconnecting && <Badge variant="destructive">Reconnecting</Badge>}
        <div className="data hidden gap-4 text-[13px] text-sidebar-accent-foreground sm:flex">
          <span>
            <span className="text-down">▼</span> {formatSpeed(stats?.download_speed ?? 0)}
          </span>
          <span>
            <span className="text-up">▲</span> {formatSpeed(stats?.upload_speed ?? 0)}
          </span>
        </div>
        <Button
          variant="ghost"
          size="icon"
          aria-pressed={alt}
          aria-label={alt ? "Alternative speed limits are on" : "Use alternative speed limits"}
          title={alt ? "Alternative speed limits are on" : "Use alternative speed limits"}
          onClick={onToggleAlt}
          className={alt ? "bg-sidebar-primary/20 text-sidebar-primary hover:bg-sidebar-primary/30" : "text-sidebar-foreground"}
        >
          <TurtleIcon />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="Float a mini view on top"
          title="Float a mini view on top of other windows"
          onClick={onFloat}
          className="hidden text-sidebar-foreground md:inline-flex"
        >
          <PictureInPicture2Icon />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="text-sidebar-foreground md:hidden"
          aria-label="Search and commands"
          onClick={onOpenCommand}
        >
          <SearchIcon />
        </Button>
        <Button
          size="sm"
          onClick={onAdd}
          className="hidden bg-sidebar-primary text-sidebar-primary-foreground hover:bg-sidebar-primary/90 md:inline-flex"
        >
          <PlusIcon data-icon="inline-start" />
          Add torrent
        </Button>
      </div>
    </header>
  )
}
