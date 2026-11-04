import { useSyncExternalStore } from "react"

export type Route = "torrents" | "statistics" | "settings" | "mini"

const ROUTES: Route[] = ["torrents", "statistics", "settings", "mini"]

function current(): Route {
  const r = location.hash.replace(/^#\/?/, "").split("?")[0] as Route
  return ROUTES.includes(r) ? r : "torrents"
}

/** `#/torrents?hash=…` (used by notification links) selects a torrent. */
export function hashParam(name: string): string | null {
  const q = location.hash.split("?")[1]
  return q ? new URLSearchParams(q).get(name) : null
}

function subscribe(cb: () => void) {
  window.addEventListener("hashchange", cb)
  return () => window.removeEventListener("hashchange", cb)
}

export function useRoute(): Route {
  return useSyncExternalStore(subscribe, current, () => "torrents")
}

export function href(r: Route) {
  return `#/${r === "torrents" ? "" : r}`
}

export function navigate(r: Route) {
  location.hash = href(r)
}
