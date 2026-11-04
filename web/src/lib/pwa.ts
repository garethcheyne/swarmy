import { api } from "@/lib/api"

/** Service workers (and so offline shell + push) need HTTPS, or localhost. */
export function pwaSupported() {
  return "serviceWorker" in navigator && window.isSecureContext
}

export function registerServiceWorker() {
  if (!pwaSupported()) return
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch((e) => console.warn("service worker:", e))
  })
}

export function isStandalone() {
  return (
    window.matchMedia("(display-mode: standalone)").matches ||
    (navigator as Navigator & { standalone?: boolean }).standalone === true
  )
}

export function isIOS() {
  return /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1)
}

export type PushState =
  | { kind: "unsupported"; reason: string }
  | { kind: "denied" }
  | { kind: "off" }
  | { kind: "on"; endpoint: string }

export async function pushState(): Promise<PushState> {
  if (!window.isSecureContext) {
    return { kind: "unsupported", reason: "Notifications need Swarmy to be opened over HTTPS." }
  }
  if (!("serviceWorker" in navigator) || !("PushManager" in window) || !("Notification" in window)) {
    if (isIOS() && !isStandalone()) {
      return {
        kind: "unsupported",
        reason: "On iPhone and iPad, add Swarmy to your Home Screen first (Share, then Add to Home Screen), then open it from there.",
      }
    }
    return { kind: "unsupported", reason: "This browser doesn't support push notifications." }
  }
  if (Notification.permission === "denied") return { kind: "denied" }
  const reg = await navigator.serviceWorker.ready
  const sub = await reg.pushManager.getSubscription()
  return sub ? { kind: "on", endpoint: sub.endpoint } : { kind: "off" }
}

function keyToBytes(base64url: string): Uint8Array<ArrayBuffer> {
  const pad = "=".repeat((4 - (base64url.length % 4)) % 4)
  const raw = atob((base64url + pad).replace(/-/g, "+").replace(/_/g, "/"))
  const out = new Uint8Array(new ArrayBuffer(raw.length))
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i)
  return out
}

function deviceLabel() {
  const ua = navigator.userAgent
  const os = /Android/.test(ua) ? "Android" : isIOS() ? "iOS" : /Windows/.test(ua) ? "Windows" : /Mac/.test(ua) ? "macOS" : /Linux/.test(ua) ? "Linux" : "Device"
  const browser = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "Browser"
  return `${browser} on ${os}`
}

export async function enablePush(): Promise<string> {
  const permission = await Notification.requestPermission()
  if (permission !== "granted") throw new Error("Notifications were not allowed for this site.")
  const { public_key } = await api.pushInfo()
  const reg = await navigator.serviceWorker.ready
  const sub =
    (await reg.pushManager.getSubscription()) ??
    (await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: keyToBytes(public_key) }))
  const json = sub.toJSON()
  await api.pushSubscribe({
    endpoint: sub.endpoint,
    keys: { p256dh: json.keys?.p256dh ?? "", auth: json.keys?.auth ?? "" },
    label: deviceLabel(),
  })
  return sub.endpoint
}

export async function disablePush() {
  const reg = await navigator.serviceWorker.ready
  const sub = await reg.pushManager.getSubscription()
  if (!sub) return
  await api.pushUnsubscribe(sub.endpoint).catch(() => {})
  await sub.unsubscribe()
}
