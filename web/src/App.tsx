import { useCallback, useEffect, useMemo, useRef, useState, lazy, Suspense } from "react"
import { createRoot, type Root } from "react-dom/client"
import { toast } from "sonner"

import { api, UnauthorizedError, useLive, type ServerInfo } from "@/lib/api"
import type { Filter } from "@/lib/filters"
import { usePalette } from "@/lib/palette"
import { openFloating, pipSupported, syncTheme } from "@/lib/pip"
import { hashParam, navigate, useRoute } from "@/lib/route"
import { usePoll } from "@/lib/use-poll"
import { compareVersions, markSeen, seenVersion } from "@/lib/version"
import { AddTorrentDialog, type AddPrefill } from "@/components/add-torrent-dialog"
import { AddNzbDialog } from "@/components/add-nzb-dialog"
import { AppearanceDialog } from "@/components/appearance-dialog"
import { DeleteDialog } from "@/components/delete-dialog"
import { Login } from "@/components/login"
import { MiniView } from "@/components/mini-view"
import { NotificationsDialog } from "@/components/notifications-dialog"
import { WhatsNewDialog } from "@/components/release-notes"
import { CommandPalette } from "@/components/shell/command-palette"
import { Nav } from "@/components/shell/nav"
import { StatusBar } from "@/components/shell/status-bar"
import { TabBar } from "@/components/shell/tab-bar"
import { TopBar } from "@/components/shell/top-bar"
import { useTheme } from "@/components/theme-provider"
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Spinner } from "@/components/ui/spinner"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { SettingsPage } from "@/pages/settings"
import { StatisticsPage } from "@/pages/statistics"
import { UsenetPage } from "@/pages/usenet"
const PeersPage = lazy(() => import("@/pages/peers").then((m) => ({ default: m.PeersPage })))
import { TorrentsPage } from "@/pages/torrents"

type AuthState = "checking" | "signed-out" | "signed-in"

export function App() {
  const [auth, setAuth] = useState<AuthState>("checking")

  useEffect(() => {
    api
      .me()
      .then(() => setAuth("signed-in"))
      .catch((e) => setAuth(e instanceof UnauthorizedError ? "signed-out" : "signed-in"))
  }, [])

  const signedOut = useCallback(() => setAuth("signed-out"), [])

  return (
    <TooltipProvider>
      {auth === "checking" ? (
        <div className="flex min-h-svh items-center justify-center">
          <Spinner />
        </div>
      ) : auth === "signed-out" ? (
        <Login onSuccess={() => setAuth("signed-in")} />
      ) : (
        <Shell onSignedOut={signedOut} />
      )}
      <Toaster position="bottom-right" />
    </TooltipProvider>
  )
}

function errorText(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}

function Shell({ onSignedOut }: { onSignedOut: () => void }) {
  const route = useRoute()
  const { theme, setTheme } = useTheme()
  const { snapshot, connection, history } = useLive(onSignedOut)
  const torrents = useMemo(() => snapshot?.torrents ?? [], [snapshot])
  const { data: info } = usePoll<ServerInfo>(api.info, 10000)

  const [categories, setCategories] = useState<string[]>([])
  const [filter, setFilter] = useState<Filter>({ kind: "status", value: "all" })
  const [focused, setFocused] = useState<string | null>(() => hashParam("hash"))
  const [menuOpen, setMenuOpen] = useState(false)
  const [commandOpen, setCommandOpen] = useState(false)
  const [notifyOpen, setNotifyOpen] = useState(false)
  const [notifyKey, setNotifyKey] = useState(0)
  const [appearanceOpen, setAppearanceOpen] = useState(false)
  // Dialogs remount (via key) each time they open, which resets their form state.
  const [addOpen, setAddOpen] = useState(false)
  const [addKey, setAddKey] = useState(0)
  const [addPrefill, setAddPrefill] = useState<AddPrefill | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleteKey, setDeleteKey] = useState(0)
  const [deleteTargets, setDeleteTargets] = useState<string[]>([])
  const [nzbOpen, setNzbOpen] = useState(false)
  const [nzbKey, setNzbKey] = useState(0)
  const [nzbFiles, setNzbFiles] = useState<File[]>([])
  const openNzb = useCallback((files: File[] = []) => {
    setNzbFiles(files)
    setNzbKey((k) => k + 1)
    setNzbOpen(true)
  }, [])

  // "What's new": shown once after an upgrade (this browser last saw an
  // older version), or opened by hand from the status bar, palette or About.
  const [seen, setSeen] = useState(seenVersion)
  const [notesOpen, setNotesOpen] = useState(false)
  const upgradedFrom = info && seen && compareVersions(info.version, seen) > 0 ? seen : null
  useEffect(() => {
    // First visit: nothing to announce, just remember where we started.
    if (info && seenVersion() === null) markSeen(info.version)
  }, [info])
  const closeNotes = () => {
    setNotesOpen(false)
    if (info) {
      markSeen(info.version)
      setSeen(info.version)
    }
  }

  // Categories can be created implicitly (by adds, Sonarr, …), so refresh
  // whenever a torrent shows up with one we don't know.
  const torrentCategories = useMemo(
    () => [...new Set(torrents.map((t) => t.category).filter(Boolean))].sort().join("\n"),
    [torrents]
  )
  useEffect(() => {
    api
      .categories()
      .then((c) => setCategories(Object.keys(c).sort()))
      .catch(() => {})
  }, [torrentCategories])

  // Tapping a notification opens #/torrents?hash=… — select that torrent.
  useEffect(() => {
    const onHash = () => {
      const h = hashParam("hash")
      if (h) setFocused(h)
    }
    window.addEventListener("hashchange", onHash)
    return () => window.removeEventListener("hashchange", onHash)
  }, [])

  // A toast when a download finishes while you're looking.
  const finished = useRef<Set<string> | null>(null)
  useEffect(() => {
    if (!snapshot) return
    const now = new Set(snapshot.torrents.filter((t) => t.completed_on).map((t) => t.hash))
    if (finished.current) {
      for (const t of snapshot.torrents) {
        if (now.has(t.hash) && !finished.current.has(t.hash)) {
          toast.success("Download finished", {
            description: t.name,
            action: { label: "Show", onClick: () => setFocused(t.hash) },
          })
        }
      }
    }
    finished.current = now
  }, [snapshot])

  const openAdd = useCallback((prefill: AddPrefill | null = null) => {
    setAddPrefill(prefill)
    setAddKey((k) => k + 1)
    setAddOpen(true)
  }, [])
  const openNotifications = useCallback(() => {
    setNotifyKey((k) => k + 1)
    setNotifyOpen(true)
  }, [])
  const requestDelete = useCallback((hashes: string[]) => {
    if (!hashes.length) return
    setDeleteTargets(hashes)
    setDeleteKey((k) => k + 1)
    setDeleteOpen(true)
  }, [])

  // Global shortcuts: Ctrl/Cmd+K, "n" to add, paste a magnet or drop a .torrent anywhere.
  useEffect(() => {
    const editable = (el: EventTarget | null) =>
      el instanceof HTMLElement && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName))
    const onPaste = (e: ClipboardEvent) => {
      if (editable(e.target) || addOpen) return
      const text = e.clipboardData?.getData("text") ?? ""
      if (/^(magnet:|https?:\/\/)/m.test(text.trim())) {
        e.preventDefault()
        openAdd({ urls: text.trim() })
      }
    }
    const onDragOver = (e: DragEvent) => {
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault()
    }
    const onDrop = (e: DragEvent) => {
      if (addOpen || nzbOpen) return
      const dropped = [...(e.dataTransfer?.files ?? [])]
      const files = dropped.filter((f) => f.name.endsWith(".torrent"))
      const nzbs = dropped.filter((f) => f.name.toLowerCase().endsWith(".nzb"))
      if (files.length) {
        e.preventDefault()
        openAdd({ files })
      } else if (nzbs.length) {
        e.preventDefault()
        openNzb(nzbs)
      }
    }
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault()
        setCommandOpen((o) => !o)
        return
      }
      if (e.key === "n" && !e.metaKey && !e.ctrlKey && !e.altKey && !editable(e.target)) {
        if (document.querySelector("[role=dialog]")) return
        e.preventDefault()
        openAdd()
      }
    }
    window.addEventListener("paste", onPaste)
    window.addEventListener("dragover", onDragOver)
    window.addEventListener("drop", onDrop)
    window.addEventListener("keydown", onKey)
    return () => {
      window.removeEventListener("paste", onPaste)
      window.removeEventListener("dragover", onDragOver)
      window.removeEventListener("drop", onDrop)
      window.removeEventListener("keydown", onKey)
    }
  }, [addOpen, openAdd, nzbOpen, openNzb])

  const run = useCallback(
    async (label: string, fn: () => Promise<void>) => {
      try {
        await fn()
      } catch (err) {
        if (err instanceof UnauthorizedError) return onSignedOut()
        toast.error(`Couldn't ${label}`, { description: errorText(err) })
      }
    },
    [onSignedOut]
  )
  const pause = useCallback((h: string[]) => run("pause", () => api.pause(h)), [run])
  const resume = useCallback((h: string[]) => run("resume", () => api.resume(h)), [run])
  const logout = async () => {
    await api.logout().catch(() => {})
    onSignedOut()
  }
  const toggleTheme = () => {
    const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)
    setTheme(dark ? "light" : "dark")
  }

  // ---- Mini view: floating always-on-top window (Document PiP) or popup.
  const palette = usePalette()
  const pip = useRef<{ win: Window; root: Root } | null>(null)
  const [floating, setFloating] = useState(false)
  const toggleAlt = useCallback(
    () => run("change speed limits", () => api.setAltSpeed(!snapshot?.stats.alt_speed)),
    [run, snapshot]
  )
  const openFromMini = useCallback((hash?: string) => {
    window.focus()
    navigate("torrents")
    if (hash) setFocused(hash)
  }, [])
  const floatMini = useCallback(async () => {
    try {
      const win = await openFloating()
      if (!win || !pipSupported() || pip.current?.win === win) return
      const host = win.document.createElement("div")
      host.style.height = "100%"
      win.document.body.appendChild(host)
      pip.current = { win, root: createRoot(host) }
      setFloating(true)
      win.addEventListener("pagehide", () => {
        pip.current?.root.unmount()
        pip.current = null
        setFloating(false)
      })
    } catch (e) {
      toast.error("Couldn't open the mini view", { description: errorText(e) })
    }
  }, [])
  // Keep the floating window live: re-render on every snapshot and theme change.
  useEffect(() => {
    const p = pip.current
    if (!p || !floating) return
    syncTheme(p.win.document)
    p.root.render(
      <MiniView snapshot={snapshot} history={history} onToggleAlt={toggleAlt} onOpen={openFromMini} floating />
    )
  }, [floating, snapshot, history, toggleAlt, openFromMini, palette, theme])

  if (route === "mini") {
    // Standalone mini window (popup or PWA shortcut).
    return (
      <div className="h-svh">
        <MiniView
          snapshot={snapshot}
          history={history}
          onToggleAlt={toggleAlt}
          onOpen={(hash) => {
            const target = `/#/torrents${hash ? `?hash=${hash}` : ""}`
            if (window.opener && !window.opener.closed) {
              window.opener.location.hash = target.slice(1)
              window.opener.focus()
            } else {
              window.open(target, "_blank")
            }
          }}
        />
      </div>
    )
  }

  const deleteNames = deleteTargets.map((h) => torrents.find((t) => t.hash === h)?.name ?? h)
  const nav = (onNavigate?: () => void) => (
    <Nav
      route={route}
      torrents={torrents}
      categories={categories}
      history={history}
      filter={filter}
      onFilterChange={(f) => {
        setFilter(f)
        setFocused(null)
      }}
      onLogout={logout}
      onNavigate={onNavigate}
    />
  )

  return (
    <div className="flex h-svh flex-col bg-background">
      <TopBar
        stats={snapshot?.stats}
        demo={!!info?.demo}
        reconnecting={connection !== "open" && !!snapshot}
        onOpenCommand={() => setCommandOpen(true)}
        onOpenMenu={() => setMenuOpen(true)}
        onAdd={() => openAdd()}
        onToggleAlt={toggleAlt}
        onFloat={floatMini}
      />
      <div className="flex min-h-0 flex-1">
        <aside className="hidden w-52 shrink-0 border-r md:block">{nav()}</aside>
        <div id="page" className="min-w-0 flex-1 overflow-hidden">
          {route === "settings" ? (
            <SettingsPage onSignedOut={onSignedOut} />
          ) : route === "statistics" ? (
            <div className="h-full overflow-auto">
              <StatisticsPage />
            </div>
          ) : route === "usenet" ? (
            <div className="h-full overflow-auto">
              <UsenetPage onAdd={() => openNzb()} />
            </div>
          ) : route === "peers" ? (
            <div className="h-full overflow-auto">
              <Suspense fallback={null}>
                <PeersPage torrents={torrents} />
              </Suspense>
            </div>
          ) : (
            <TorrentsPage
              loading={!snapshot}
              torrents={torrents}
              categories={categories}
              filter={filter}
              focused={focused}
              onFocus={setFocused}
              onAdd={() => openAdd()}
              onPause={pause}
              onResume={resume}
              onDelete={requestDelete}
            />
          )}
        </div>
      </div>
      <StatusBar info={info} stats={snapshot?.stats} onVersion={() => setNotesOpen(true)} />
      <TabBar route={route} onAdd={() => openAdd()} onMenu={() => setMenuOpen(true)} />

      <Sheet open={menuOpen} onOpenChange={setMenuOpen}>
        <SheetContent side="left" className="w-72 bg-sidebar p-0 pt-[env(safe-area-inset-top)]">
          <SheetHeader className="sr-only">
            <SheetTitle>Menu</SheetTitle>
            <SheetDescription>Views, categories and settings</SheetDescription>
          </SheetHeader>
          {nav(() => setMenuOpen(false))}
        </SheetContent>
      </Sheet>

      <CommandPalette
        open={commandOpen}
        onOpenChange={setCommandOpen}
        torrents={torrents}
        categories={categories}
        onAdd={() => openAdd()}
        onPauseAll={() => pause(torrents.filter((t) => t.state !== "paused").map((t) => t.hash))}
        onResumeAll={() => resume(torrents.filter((t) => t.state === "paused").map((t) => t.hash))}
        onToggleTheme={toggleTheme}
        onNotifications={openNotifications}
        onAppearance={() => setAppearanceOpen(true)}
        onWhatsNew={() => setNotesOpen(true)}
        onAddNzb={() => openNzb()}
        onFloat={floatMini}
        onFilter={setFilter}
        onSelect={setFocused}
      />
      <AppearanceDialog open={appearanceOpen} onOpenChange={setAppearanceOpen} />
      <WhatsNewDialog
        open={notesOpen || upgradedFrom !== null}
        since={notesOpen ? null : upgradedFrom}
        onOpenChange={(o) => (o ? setNotesOpen(true) : closeNotes())}
      />
      <NotificationsDialog key={notifyKey} open={notifyOpen} onOpenChange={setNotifyOpen} />
      <AddNzbDialog key={nzbKey} open={nzbOpen} onOpenChange={setNzbOpen} categories={categories} initialFiles={nzbFiles} />
      <AddTorrentDialog key={addKey} open={addOpen} onOpenChange={setAddOpen} categories={categories} prefill={addPrefill} />
      <DeleteDialog
        key={deleteKey}
        names={deleteNames}
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onConfirm={async (deleteFiles) => {
          const hashes = deleteTargets
          await run("delete", async () => {
            await api.remove(hashes, deleteFiles)
            if (focused && hashes.includes(focused)) setFocused(null)
            toast.success(hashes.length === 1 ? "Torrent deleted" : `${hashes.length} torrents deleted`)
          })
        }}
      />
    </div>
  )
}

export default App
