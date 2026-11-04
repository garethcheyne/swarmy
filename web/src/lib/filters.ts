import type { Torrent } from "@/lib/api"

export type StatusFilter =
  | "all"
  | "downloading"
  | "seeding"
  | "completed"
  | "paused"
  | "active"
  | "stalled"
  | "error"

export type Filter =
  | { kind: "status"; value: StatusFilter }
  | { kind: "category"; value: string }
  | { kind: "tag"; value: string }

export const STATUS_PREDICATES: Record<StatusFilter, (t: Torrent) => boolean> = {
  all: () => true,
  downloading: (t) => t.state === "downloading" || t.state === "initializing",
  seeding: (t) => t.state === "seeding",
  completed: (t) => t.progress >= 1,
  paused: (t) => t.state === "paused",
  active: (t) => t.download_speed > 0 || t.upload_speed > 0,
  stalled: (t) => t.state === "downloading" && t.download_speed === 0 && t.peers === 0,
  error: (t) => t.state === "error",
}

export function matchesFilter(t: Torrent, f: Filter): boolean {
  switch (f.kind) {
    case "status":
      return STATUS_PREDICATES[f.value](t)
    case "category":
      return t.category === f.value
    case "tag":
      return t.tags.includes(f.value)
  }
}

export function sameFilter(a: Filter, b: Filter) {
  return a.kind === b.kind && a.value === b.value
}
