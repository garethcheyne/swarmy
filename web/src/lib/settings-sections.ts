import { useSyncExternalStore } from "react"
import {
  BellIcon,
  DownloadIcon,
  EarthIcon,
  GaugeIcon,
  InfoIcon,
  ListOrderedIcon,
  NetworkIcon,
  NewspaperIcon,
  PaletteIcon,
  PlugIcon,
  ShieldIcon,
  TagIcon,
} from "lucide-react"

import { hashParam } from "@/lib/route"

/** Settings → sections, in the order they appear in the main menu. */
export const SETTINGS_SECTIONS = [
  { id: "downloads", label: "Downloads", icon: DownloadIcon },
  { id: "queue", label: "Queue and seeding", icon: ListOrderedIcon },
  { id: "speed", label: "Speed", icon: GaugeIcon },
  { id: "connection", label: "Connection", icon: NetworkIcon },
  { id: "usenet", label: "Usenet", icon: NewspaperIcon },
  { id: "categories", label: "Categories and tags", icon: TagIcon },
  { id: "integrations", label: "Sonarr and Radarr", icon: PlugIcon },
  { id: "notifications", label: "Notifications", icon: BellIcon },
  { id: "map", label: "Swarm map", icon: EarthIcon },
  { id: "appearance", label: "Appearance", icon: PaletteIcon },
  { id: "security", label: "Web UI and security", icon: ShieldIcon },
  { id: "about", label: "About", icon: InfoIcon },
] as const

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"]

function current(): SettingsSectionId {
  const s = hashParam("section")
  return SETTINGS_SECTIONS.some((x) => x.id === s) ? (s as SettingsSectionId) : "downloads"
}

function subscribe(cb: () => void) {
  window.addEventListener("hashchange", cb)
  return () => window.removeEventListener("hashchange", cb)
}

/** The section in the URL (`#/settings?section=speed`). */
export function useSettingsSection(): SettingsSectionId {
  return useSyncExternalStore(subscribe, current, () => "downloads")
}

export const settingsHref = (id: SettingsSectionId) => `#/settings?section=${id}`
