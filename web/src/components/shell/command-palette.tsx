import {
  BellIcon,
  ChartColumnIcon,
  FilterIcon,
  ListIcon,
  MoonIcon,
  PaletteIcon,
  PauseIcon,
  PictureInPicture2Icon,
  PlayIcon,
  PlusIcon,
  SettingsIcon,
} from "lucide-react"

import type { Torrent } from "@/lib/api"
import type { Filter, StatusFilter } from "@/lib/filters"
import { formatPercent } from "@/lib/format"
import { navigate } from "@/lib/route"
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command"

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  torrents: Torrent[]
  categories: string[]
  onAdd: () => void
  onPauseAll: () => void
  onResumeAll: () => void
  onToggleTheme: () => void
  onNotifications: () => void
  onAppearance: () => void
  onFloat: () => void
  onFilter: (f: Filter) => void
  onSelect: (hash: string) => void
}

const VIEWS: [StatusFilter, string][] = [
  ["all", "All torrents"],
  ["downloading", "Downloading"],
  ["seeding", "Seeding"],
  ["completed", "Completed"],
  ["stalled", "Stalled"],
  ["error", "Errored"],
  ["paused", "Paused"],
]

export function CommandPalette({
  open,
  onOpenChange,
  torrents,
  categories,
  onAdd,
  onPauseAll,
  onResumeAll,
  onToggleTheme,
  onNotifications,
  onAppearance,
  onFloat,
  onFilter,
  onSelect,
}: Props) {
  const run = (fn: () => void) => () => {
    onOpenChange(false)
    fn()
  }
  const goTorrents = () => navigate("torrents")

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Search and commands"
    >
      <Command>
        <CommandInput placeholder="Search torrents or type a command" />
        <CommandList>
          <CommandEmpty>Nothing matches.</CommandEmpty>
          <CommandGroup heading="Actions">
            <CommandItem onSelect={run(onAdd)}>
              <PlusIcon />
              Add torrent
              <CommandShortcut>N</CommandShortcut>
            </CommandItem>
            <CommandItem onSelect={run(onPauseAll)}>
              <PauseIcon />
              Pause all
            </CommandItem>
            <CommandItem onSelect={run(onResumeAll)}>
              <PlayIcon />
              Resume all
            </CommandItem>
            <CommandItem onSelect={run(onToggleTheme)}>
              <MoonIcon />
              Switch light or dark
            </CommandItem>
            <CommandItem onSelect={run(onFloat)}>
              <PictureInPicture2Icon />
              Float mini view on top
            </CommandItem>
            <CommandItem onSelect={run(onAppearance)}>
              <PaletteIcon />
              Change theme
            </CommandItem>
            <CommandItem onSelect={run(onNotifications)}>
              <BellIcon />
              Notification settings
            </CommandItem>
          </CommandGroup>
          <CommandSeparator />
          <CommandGroup heading="Go to">
            <CommandItem onSelect={run(goTorrents)}>
              <ListIcon />
              Torrents
            </CommandItem>
            <CommandItem onSelect={run(() => navigate("statistics"))}>
              <ChartColumnIcon />
              Statistics
            </CommandItem>
            <CommandItem onSelect={run(() => navigate("settings"))}>
              <SettingsIcon />
              Settings
            </CommandItem>
            {VIEWS.map(([v, label]) => (
              <CommandItem
                key={v}
                value={`view ${label}`}
                onSelect={run(() => {
                  onFilter({ kind: "status", value: v })
                  goTorrents()
                })}
              >
                <FilterIcon />
                {label}
              </CommandItem>
            ))}
            {categories.map((c) => (
              <CommandItem
                key={c}
                value={`category ${c}`}
                onSelect={run(() => {
                  onFilter({ kind: "category", value: c })
                  goTorrents()
                })}
              >
                <FilterIcon />
                Category: {c}
              </CommandItem>
            ))}
          </CommandGroup>
          <CommandSeparator />
          <CommandGroup heading="Torrents">
            {torrents.map((t) => (
              <CommandItem
                key={t.hash}
                value={`${t.name} ${t.category} ${t.hash}`}
                onSelect={run(() => {
                  onFilter({ kind: "status", value: "all" })
                  goTorrents()
                  onSelect(t.hash)
                })}
              >
                <span className="truncate">{t.name}</span>
                <CommandShortcut className="data">
                  {formatPercent(t.progress)}
                </CommandShortcut>
              </CommandItem>
            ))}
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
