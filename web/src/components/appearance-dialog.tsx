import { CheckIcon } from "lucide-react"

import { PALETTES, setPalette, usePalette } from "@/lib/palette"
import { cn } from "@/lib/utils"
import { useTheme } from "@/components/theme-provider"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

type Mode = "light" | "dark" | "system"

/**
 * A tiny Control Room rendered in one palette. The palette CSS blocks match
 * any element with `data-palette`, so the preview scopes them to itself.
 */
function Preview({ id, dark }: { id: string; dark: boolean }) {
  const rows = [
    { w: "78%", p: 0.66, c: "var(--down)" },
    { w: "62%", p: 1, c: "var(--up)" },
    { w: "70%", p: 0.3, c: "var(--down)" },
    { w: "54%", p: 1, c: "var(--up)" },
  ]
  return (
    <div
      data-palette={id}
      className={cn(
        "pointer-events-none flex h-24 overflow-hidden rounded-md border bg-background",
        dark && "dark"
      )}
      aria-hidden
    >
      <div className="flex w-10 shrink-0 flex-col gap-1.5 bg-sidebar p-1.5">
        <span className="h-2 w-5 rounded-sm bg-sidebar-primary" />
        <span className="h-1.5 rounded-sm bg-sidebar-accent" />
        <span className="h-1.5 w-6 rounded-sm bg-sidebar-foreground/40" />
        <span className="h-1.5 w-5 rounded-sm bg-sidebar-foreground/40" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1.5 p-2">
        {rows.map((r, i) => (
          <div
            key={i}
            className={cn(
              "flex flex-col gap-1 rounded-sm px-1 py-0.5",
              i === 0 && "bg-muted"
            )}
          >
            <span
              className="h-1 rounded-full bg-foreground/70"
              style={{ width: r.w }}
            />
            <span className="h-[3px] overflow-hidden rounded-full bg-muted">
              <span
                className="block h-full"
                style={{ width: `${r.p * 100}%`, background: r.c }}
              />
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

export function AppearancePanel() {
  const palette = usePalette()
  const { theme, setTheme } = useTheme()
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches)

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <span className="text-sm font-medium" id="mode-label">
          Mode
        </span>
        <ToggleGroup
          variant="outline"
          size="sm"
          aria-labelledby="mode-label"
          value={[theme]}
          onValueChange={(v: string[]) => v[0] && setTheme(v[0] as Mode)}
        >
          <ToggleGroupItem value="light">Light</ToggleGroupItem>
          <ToggleGroupItem value="dark">Dark</ToggleGroupItem>
          <ToggleGroupItem value="system">Match system</ToggleGroupItem>
        </ToggleGroup>
      </div>

      <div
        role="radiogroup"
        aria-label="Colour theme"
        className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4"
      >
        {PALETTES.map((p) => {
          const on = p.id === palette
          return (
            <button
              key={p.id}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => setPalette(p.id)}
              className={cn(
                "flex flex-col gap-2 rounded-lg border p-2 text-left transition-colors hover:border-foreground/40 focus-visible:outline-2 focus-visible:outline-ring",
                on && "border-foreground ring-1 ring-foreground"
              )}
            >
              <Preview id={p.id} dark={dark} />
              <span className="flex items-start justify-between gap-2 px-0.5">
                <span className="flex min-w-0 flex-col">
                  <span className="text-sm font-medium">{p.label}</span>
                  <span className="truncate text-xs text-muted-foreground">
                    {p.note}
                  </span>
                </span>
                {on && <CheckIcon className="mt-0.5 size-4 shrink-0" />}
              </span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

export function AppearanceDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Appearance</DialogTitle>
          <DialogDescription>
            Pick a colour theme. Each one has a light and a dark version.
          </DialogDescription>
        </DialogHeader>
        <AppearancePanel />
      </DialogContent>
    </Dialog>
  )
}
