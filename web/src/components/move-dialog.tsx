import { useId, useState, type FormEvent } from "react"
import { toast } from "sonner"

import { api, type Torrent } from "@/lib/api"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

type Mode = "folder" | "category"

/** Full paths only: /data/tv, C:\Downloads, \\nas\share. */
const isAbsolute = (p: string) => /^(\/|[A-Za-z]:[\\/]|\\\\)/.test(p)

export function MoveDialog({
  open,
  onOpenChange,
  targets,
  folders,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  targets: Torrent[]
  /** Folders to suggest: where other torrents already live. */
  folders: string[]
}) {
  const first = targets[0]
  const [mode, setMode] = useState<Mode>("folder")
  const [path, setPath] = useState(first?.save_path ?? "")
  const [pending, setPending] = useState(false)
  const listId = useId()

  const one = targets.length === 1
  const trimmed = path.trim()
  const invalid = mode === "folder" && trimmed !== "" && !isAbsolute(trimmed)
  const unchanged = mode === "folder" && targets.every((t) => t.save_path === trimmed)
  const uncategorised = targets.filter((t) => !t.category).length

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (invalid || (mode === "folder" && !trimmed)) return
    setPending(true)
    try {
      await api.move(
        targets.map((t) => t.hash),
        mode === "category" ? null : trimmed
      )
      toast.success(one ? `Moving ${first.name}` : `Moving ${targets.length} torrents`, {
        description: "Progress shows in the list. Moves run one at a time.",
      })
      onOpenChange(false)
    } catch (err) {
      toast.error("Couldn't move", { description: err instanceof Error ? err.message : String(err) })
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={submit} className="flex flex-col gap-6">
          <DialogHeader>
            <DialogTitle className="break-all">{one ? `Move ${first?.name}` : `Move ${targets.length} torrents`}</DialogTitle>
            <DialogDescription>
              Swarmy moves the files and carries on seeding from the new folder. Nothing is downloaded again.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <ToggleGroup
              variant="outline"
              size="sm"
              aria-label="Where to move to"
              value={[mode]}
              onValueChange={(v: string[]) => v[0] && setMode(v[0] as Mode)}
            >
              <ToggleGroupItem value="folder">A folder</ToggleGroupItem>
              <ToggleGroupItem value="category">Its category's folder</ToggleGroupItem>
            </ToggleGroup>
            {mode === "folder" ? (
              <Field data-invalid={invalid || undefined}>
                <FieldLabel htmlFor="move-path">Folder</FieldLabel>
                <Input
                  id="move-path"
                  className="data"
                  list={listId}
                  autoFocus
                  aria-invalid={invalid || undefined}
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                  placeholder="/data/torrents"
                />
                <datalist id={listId}>
                  {folders.map((f) => (
                    <option key={f} value={f} />
                  ))}
                </datalist>
                {invalid ? (
                  <FieldError>Give the full path, starting from the top, like /data/torrents.</FieldError>
                ) : (
                  <FieldDescription>
                    Swarmy creates the folder if it isn't there. A torrent moved by hand stops following its category's
                    folder.
                  </FieldDescription>
                )}
              </Field>
            ) : (
              <FieldDescription>
                {one ? "It goes" : "Each goes"} to {one ? "its" : "their"} category's folder, and moves again if{" "}
                {one ? "its" : "their"} category changes.
                {uncategorised > 0 &&
                  (one ? " It has no category, so it goes to the default downloads folder." : ` ${uncategorised} without a category go to the default downloads folder.`)}
              </FieldDescription>
            )}
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending || invalid || (mode === "folder" && (!trimmed || unchanged))}>
              {pending && <Spinner data-icon="inline-start" />}
              Move
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
