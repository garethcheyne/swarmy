import { useCallback, useEffect, useState } from "react"
import { CopyIcon, RefreshCwIcon } from "lucide-react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"

/** What the browser reports about the screen, for sorting out phone layout problems. */
function measure(): [string, string][] {
  const px = (n: number) => `${Math.round(n * 10) / 10}`
  const probe = document.createElement("div")
  probe.style.cssText =
    "position:fixed;left:0;top:0;width:0;visibility:hidden;pointer-events:none;" +
    "padding:env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left)"
  document.body.appendChild(probe)
  const cs = getComputedStyle(probe)
  const insets = `${cs.paddingTop} / ${cs.paddingRight} / ${cs.paddingBottom} / ${cs.paddingLeft}`
  const unit = (u: string) => {
    probe.style.cssText = `position:fixed;left:0;top:0;width:0;visibility:hidden;height:100${u}`
    return px(probe.getBoundingClientRect().height)
  }
  const heights = `svh ${unit("svh")}, dvh ${unit("dvh")}, lvh ${unit("lvh")}, vh ${unit("vh")}`
  probe.remove()

  const rect = (sel: string) => {
    const el = document.querySelector(sel)
    if (!el) return "not on this page"
    const r = el.getBoundingClientRect()
    return `top ${px(r.top)}, bottom ${px(r.bottom)}, position ${getComputedStyle(el).position}`
  }
  const vv = window.visualViewport
  const nav = navigator as Navigator & { standalone?: boolean }
  return [
    ["Opened as", matchMedia("(display-mode: standalone)").matches || nav.standalone ? "home screen app" : "browser tab"],
    ["Status bar style", document.querySelector('meta[name="apple-mobile-web-app-status-bar-style"]')?.getAttribute("content") ?? "none"],
    ["Screen", `${screen.width} × ${screen.height} at ${devicePixelRatio}x`],
    ["Window", `${innerWidth} × ${innerHeight} (outer ${outerWidth} × ${outerHeight})`],
    ["Visible area", vv ? `${px(vv.width)} × ${px(vv.height)}, offset ${px(vv.offsetTop)}, page top ${px(vv.pageTop)}` : "not reported"],
    ["Page", `client height ${document.documentElement.clientHeight}, scroll height ${document.documentElement.scrollHeight}, scrolled ${px(document.scrollingElement?.scrollTop ?? 0)}`],
    ["Safe area (top/right/bottom/left)", insets],
    ["100 viewport units", heights],
    ["Top bar", rect("header")],
    ["Tab bar", rect('nav[aria-label="Main"]')],
    ["Browser", navigator.userAgent],
  ]
}

/** Settings → About: screen measurements to copy. */
export function ScreenDiagnostics() {
  const [rows, setRows] = useState<[string, string][]>([])
  const refresh = useCallback(() => setRows(measure()), [])

  useEffect(() => {
    const id = requestAnimationFrame(refresh)
    window.addEventListener("resize", refresh)
    return () => {
      cancelAnimationFrame(id)
      window.removeEventListener("resize", refresh)
    }
  }, [refresh])

  async function copy() {
    const text = rows.map(([k, v]) => `${k}: ${v}`).join("\n")
    try {
      await navigator.clipboard.writeText(text)
      toast.success("Screen details copied")
    } catch {
      toast.error("Couldn't copy. Take a screenshot instead.")
    }
  }


  return (
    <div className="flex flex-col gap-4">
      <dl className="grid grid-cols-[minmax(0,10rem)_minmax(0,1fr)] gap-x-4 gap-y-1.5 rounded-lg border p-3 text-[12px]">
        {rows.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="text-muted-foreground">{k}</dt>
            <dd className="data [overflow-wrap:anywhere]">{v}</dd>
          </div>
        ))}
      </dl>
      <div className="flex gap-2">
        <Button variant="outline" size="sm" onClick={copy}>
          <CopyIcon data-icon="inline-start" />
          Copy details
        </Button>
        <Button variant="ghost" size="sm" onClick={refresh}>
          <RefreshCwIcon data-icon="inline-start" />
          Measure again
        </Button>
      </div>
    </div>
  )
}
