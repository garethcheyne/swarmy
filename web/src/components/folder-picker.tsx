import { useEffect, useState, type FormEvent } from "react"
import { CircleAlertIcon, CircleCheckIcon, CornerLeftUpIcon, FolderIcon, FolderOpenIcon, FolderPlusIcon } from "lucide-react"

import { api, type FolderListing } from "@/lib/api"
import { cn } from "@/lib/utils"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"

/** A folder path field with a Browse button that opens the server's folders. */
export function FolderInput({
  id,
  value,
  onChange,
  placeholder,
  disabled,
  className,
  title = "Choose a folder",
  "aria-label": ariaLabel,
}: {
  id?: string
  value: string
  onChange: (path: string) => void
  placeholder?: string
  disabled?: boolean
  className?: string
  title?: string
  "aria-label"?: string
}) {
  const [open, setOpen] = useState(false)
  const [key, setKey] = useState(0)
  return (
    <>
      <InputGroup className={className} data-disabled={disabled || undefined}>
        <InputGroupInput
          id={id}
          className="data"
          aria-label={ariaLabel}
          placeholder={placeholder}
          disabled={disabled}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
        <InputGroupAddon align="inline-end">
          <InputGroupButton
            disabled={disabled}
            onClick={() => {
              setKey((k) => k + 1)
              setOpen(true)
            }}
          >
            <FolderOpenIcon data-icon="inline-start" />
            Browse
          </InputGroupButton>
        </InputGroupAddon>
      </InputGroup>
      <FolderPicker
        key={key}
        open={open}
        onOpenChange={setOpen}
        title={title}
        start={value.trim() || placeholder || ""}
        onPick={onChange}
      />
    </>
  )
}

/** Browse the server's folders and pick one Swarmy can write to. */
export function FolderPicker({
  open,
  onOpenChange,
  title,
  start,
  onPick,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  start: string
  onPick: (path: string) => void
}) {
  const [listing, setListing] = useState<FolderListing | null>(null)
  const [typed, setTyped] = useState(start)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [newName, setNewName] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)

  async function go(path: string | null) {
    setLoading(true)
    try {
      const l = await api.folders(path)
      setListing(l)
      setTyped(l.path)
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  // Open where the field points; if that isn't there yet, start from the download folder.
  useEffect(() => {
    if (!open) return
    let live = true
    api
      .folders(start || null)
      .catch(() => api.folders(null))
      .then((l) => {
        if (!live) return
        setListing(l)
        setTyped(l.path)
      })
      .catch((e) => live && setError(e instanceof Error ? e.message : String(e)))
    return () => {
      live = false
    }
  }, [open, start])

  async function makeFolder(e: FormEvent) {
    e.preventDefault()
    if (!listing || !newName?.trim()) return
    setCreating(true)
    try {
      const made = await api.newFolder(listing.path, newName.trim())
      setNewName(null)
      await go(made.path)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setCreating(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(40rem,90svh)] flex-col gap-4 sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>Folders on the server Swarmy runs on. Swarmy checks it can write to the one you pick.</DialogDescription>
        </DialogHeader>

        <form
          onSubmit={(e) => {
            e.preventDefault()
            void go(typed.trim())
          }}
        >
          <InputGroup>
            <InputGroupInput className="data" aria-label="Folder path" value={typed} onChange={(e) => setTyped(e.target.value)} />
            <InputGroupAddon align="inline-end">
              <InputGroupButton type="submit">Go</InputGroupButton>
            </InputGroupAddon>
          </InputGroup>
        </form>

        {listing && listing.places.length > 0 && (
          <div className="flex flex-wrap gap-1.5" aria-label="Places">
            {listing.places.map((p) => (
              <Button key={p.path} size="xs" variant={p.path === listing.path ? "secondary" : "outline"} onClick={() => go(p.path)}>
                {p.label}
              </Button>
            ))}
          </div>
        )}

        <div className="min-h-48 flex-1 overflow-y-auto rounded-lg border">
          {!listing ? (
            <div className="flex flex-col gap-2 p-2">
              {Array.from({ length: 6 }, (_, i) => (
                <Skeleton key={i} className="h-7" />
              ))}
            </div>
          ) : (
            <ul className={cn("flex flex-col p-1 text-[13px]", loading && "opacity-60")}>
              {listing.parent && (
                <li>
                  <button type="button" className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-muted-foreground hover:bg-muted" onClick={() => go(listing.parent)}>
                    <CornerLeftUpIcon className="size-4" />
                    Up to {listing.parent}
                  </button>
                </li>
              )}
              {listing.folders.map((f) => (
                <li key={f.path}>
                  <button type="button" className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-muted" onClick={() => go(f.path)}>
                    <FolderIcon className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">{f.name}</span>
                  </button>
                </li>
              ))}
              {listing.folders.length === 0 && <li className="px-2 py-3 text-muted-foreground">No folders in here.</li>}
            </ul>
          )}
        </div>

        {newName !== null ? (
          <form onSubmit={makeFolder} className="flex gap-2">
            <InputGroup>
              <InputGroupInput autoFocus aria-label="New folder name" placeholder="New folder name" value={newName} onChange={(e) => setNewName(e.target.value)} />
            </InputGroup>
            <Button type="submit" variant="outline" disabled={creating || !newName.trim()}>
              {creating && <Spinner data-icon="inline-start" />}
              Create
            </Button>
            <Button type="button" variant="ghost" onClick={() => setNewName(null)}>
              Cancel
            </Button>
          </form>
        ) : null}

        {error ? (
          <Alert variant="destructive">
            <CircleAlertIcon />
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : listing ? (
          <p className={cn("flex items-start gap-2 text-[13px]", listing.writable ? "text-muted-foreground" : "text-destructive")}>
            {listing.writable ? <CircleCheckIcon className="mt-0.5 size-4 shrink-0 text-up" /> : <CircleAlertIcon className="mt-0.5 size-4 shrink-0" />}
            <span className="[overflow-wrap:anywhere]">{listing.writable ? "Swarmy can write here." : listing.problem}</span>
          </p>
        ) : null}

        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" disabled={!listing || newName !== null} onClick={() => setNewName("")}>
            <FolderPlusIcon data-icon="inline-start" />
            New folder
          </Button>
          <div className="flex flex-col-reverse gap-2 sm:flex-row">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              disabled={!listing?.writable || loading}
              onClick={() => {
                if (!listing) return
                onPick(listing.path)
                onOpenChange(false)
              }}
            >
              Use this folder
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
