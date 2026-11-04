import {
  ArrowDownIcon,
  ClockIcon,
  ArrowUpIcon,
  CircleAlertIcon,
  LoaderCircleIcon,
  PauseIcon,
} from "lucide-react"

import type { Torrent } from "@/lib/api"
import { Badge } from "@/components/ui/badge"

const CONFIG = {
  downloading: {
    label: "Downloading",
    variant: "default",
    icon: ArrowDownIcon,
    className: "bg-honey text-honey-foreground",
  },
  seeding: {
    label: "Seeding",
    variant: "default",
    icon: ArrowUpIcon,
    className: "bg-up text-background",
  },
  paused: { label: "Paused", variant: "outline", icon: PauseIcon },
  error: { label: "Error", variant: "destructive", icon: CircleAlertIcon },
  initializing: {
    label: "Checking",
    variant: "outline",
    icon: LoaderCircleIcon,
  },
} as const

export function StatusBadge({ torrent }: { torrent: Torrent }) {
  const stalled =
    torrent.state === "downloading" &&
    torrent.download_speed === 0 &&
    torrent.peers === 0
  const state = torrent.queued
    ? { ...CONFIG.paused, label: "Queued", icon: ClockIcon }
    : torrent.state === "initializing" && !torrent.has_metadata
      ? { ...CONFIG.initializing, label: "Metadata" }
      : torrent.state === "paused" && torrent.progress >= 1
        ? { ...CONFIG.paused, label: "Completed" }
        : stalled
          ? {
              ...CONFIG.downloading,
              label: "Stalled",
              variant: "outline" as const,
              className: undefined,
            }
          : CONFIG[torrent.state]
  const Icon = state.icon
  return (
    <Badge
      variant={state.variant}
      className={"className" in state ? state.className : undefined}
    >
      <Icon
        data-icon="inline-start"
        className={
          torrent.state === "initializing" ? "animate-spin" : undefined
        }
      />
      {state.label}
    </Badge>
  )
}
