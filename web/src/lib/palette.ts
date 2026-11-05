import { useSyncExternalStore } from "react"

export const PALETTES = [
  { id: "swarmy", label: "Swarmy", note: "Pine and moss, the house theme", swatch: ["#7cb342", "#2a7fb8", "#1c2a1f"] },
  { id: "emerald", label: "Emerald", note: "Jewel green and gold", swatch: ["#10b981", "#b8860b", "#0f2e22"] },
  { id: "midnight", label: "Midnight", note: "Navy and cyan", swatch: ["#5cc8ff", "#c26d0a", "#121a30"] },
  { id: "arctic", label: "Arctic", note: "Icy blue, signal orange", swatch: ["#2563eb", "#d9480f", "#e8eef5"] },
  { id: "ember", label: "Ember", note: "Charcoal and embers", swatch: ["#ff7a45", "#2a7fb8", "#1c1916"] },
  { id: "honey", label: "Honey", note: "Honey and blue ink", swatch: ["#e8a200", "#6250d6", "#1b2030"] },
  { id: "sakura", label: "Sakura", note: "Blossom pink, slate blue", swatch: ["#d6457f", "#3a6fc4", "#f3e7ed"] },
  { id: "aurora", label: "Aurora", note: "Plum night, teal, coral", swatch: ["#2dd4bf", "#e5604d", "#231b3a"] },
  { id: "riso", label: "Riso", note: "Fluorescent pink print", swatch: ["#ff48b0", "#0078bf", "#1c2b6e"] },
  { id: "fjord", label: "Fjord", note: "Calm Nordic slate", swatch: ["#2f7fb0", "#4a8f5c", "#e6ebea"] },
  { id: "citrus", label: "Citrus", note: "Loud tangerine", swatch: ["#ff6b1a", "#00968c", "#1e1b18"] },
  { id: "graphite", label: "Graphite", note: "Quiet and neutral", swatch: ["#2a78d6", "#eb6834", "#18181b"] },
] as const

export type PaletteId = (typeof PALETTES)[number]["id"]

const KEY = "swarmy-palette"
const listeners = new Set<() => void>()

function read(): PaletteId {
  const fromUrl = new URLSearchParams(location.search).get("palette")
  let stored: string | null = null
  try {
    stored = localStorage.getItem(KEY)
  } catch {
    // storage unavailable (private mode etc.)
  }
  // "forest" was the old name of the Swarmy theme.
  const raw = fromUrl ?? stored ?? "swarmy"
  const id = raw === "forest" ? "swarmy" : raw
  return PALETTES.some((p) => p.id === id) ? (id as PaletteId) : "swarmy"
}

/** Keep the browser/PWA chrome (and the iPhone status bar) the colour of the top bar. */
function syncChrome() {
  requestAnimationFrame(() => {
    const bg = getComputedStyle(document.documentElement).getPropertyValue("--sidebar").trim()
    document.querySelector('meta[name="theme-color"]')?.setAttribute("content", bg || "#0e1510")
  })
}

function apply(id: PaletteId) {
  document.documentElement.dataset.palette = id
  syncChrome()
}

// Light and dark change the bar colour too (the theme provider swaps a class on <html>).
new MutationObserver(syncChrome).observe(document.documentElement, { attributes: true, attributeFilter: ["class"] })

let current = read()
apply(current)

export function setPalette(id: PaletteId) {
  current = id
  apply(id)
  try {
    localStorage.setItem(KEY, id)
  } catch {
    // ignore
  }
  listeners.forEach((l) => l())
}

export function usePalette(): PaletteId {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb)
      return () => listeners.delete(cb)
    },
    () => current,
    () => "swarmy"
  )
}
