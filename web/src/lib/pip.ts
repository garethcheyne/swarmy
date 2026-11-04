/**
 * Float a small always-on-top window (Document Picture-in-Picture, Chrome/Edge
 * 116+), or fall back to a plain popup showing the #/mini route.
 */

interface DocumentPiP {
  requestWindow(opts?: { width?: number; height?: number }): Promise<Window>
  window: Window | null
}

declare global {
  interface Window {
    documentPictureInPicture?: DocumentPiP
  }
}

export function pipSupported() {
  return "documentPictureInPicture" in window
}

/** Copy every stylesheet into the floating document, so Tailwind and fonts apply. */
function copyStyles(target: Document) {
  for (const sheet of [...document.styleSheets]) {
    try {
      if (sheet.href) {
        const link = target.createElement("link")
        link.rel = "stylesheet"
        link.href = sheet.href
        target.head.appendChild(link)
      } else {
        const style = target.createElement("style")
        style.textContent = [...sheet.cssRules].map((r) => r.cssText).join("\n")
        target.head.appendChild(style)
      }
    } catch {
      // Cross-origin sheets can't be read; skip them.
    }
  }
}

/** Mirror theme classes (dark) and the palette attribute onto the floating document. */
export function syncTheme(target: Document) {
  const src = document.documentElement
  target.documentElement.className = src.className
  if (src.dataset.palette) target.documentElement.dataset.palette = src.dataset.palette
}

export async function openFloating(): Promise<Window | null> {
  const size = { width: 380, height: 540 }
  if (!pipSupported()) {
    return window.open("/#/mini", "swarmy-mini", `popup,width=${size.width},height=${size.height}`)
  }
  const existing = window.documentPictureInPicture!.window
  if (existing) {
    existing.focus()
    return existing
  }
  const pip = await window.documentPictureInPicture!.requestWindow(size)
  pip.document.title = "Swarmy"
  copyStyles(pip.document)
  syncTheme(pip.document)
  pip.document.body.style.margin = "0"
  pip.document.body.style.height = "100vh"
  return pip
}
