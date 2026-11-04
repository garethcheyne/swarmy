import { useRef, useState, type FormEvent } from "react"
import { FileIcon, UploadIcon, XIcon } from "lucide-react"
import { toast } from "sonner"

import { api } from "@/lib/api"
import { formatBytes } from "@/lib/format"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"

const NO_CATEGORY = "__none__"

export interface AddPrefill {
  urls?: string
  files?: File[]
}

/** Remount with a new `key` on each open to reset the form. */
interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  categories: string[]
  prefill: AddPrefill | null
}

export function AddTorrentDialog({ open, onOpenChange, categories, prefill }: Props) {
  const [urls, setUrls] = useState(prefill?.urls ?? "")
  const [files, setFiles] = useState<File[]>(prefill?.files ?? [])
  const [category, setCategory] = useState(NO_CATEGORY)
  const [savePath, setSavePath] = useState("")
  const [paused, setPaused] = useState(false)
  const [pending, setPending] = useState(false)
  const [dragging, setDragging] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const addFiles = (list: FileList | null) => {
    if (!list) return
    const torrents = [...list].filter((f) => f.name.endsWith(".torrent"))
    setFiles((prev) => [...prev, ...torrents])
  }

  const linkCount = urls.split("\n").filter((l) => l.trim()).length
  const total = linkCount + files.length

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (total === 0) return
    const form = new FormData()
    if (urls.trim()) form.append("urls", urls)
    for (const f of files) form.append("torrents", f, f.name)
    if (category !== NO_CATEGORY) form.append("category", category)
    if (savePath.trim()) form.append("savepath", savePath.trim())
    form.append("paused", String(paused))

    setPending(true)
    try {
      const added = await api.add(form)
      toast.success(
        added.length === 1 ? "Torrent added" : `${added.length} torrents added`
      )
      onOpenChange(false)
    } catch (err) {
      toast.error("Couldn't add torrent", {
        description: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={submit} className="flex flex-col gap-6">
          <DialogHeader>
            <DialogTitle>Add torrents</DialogTitle>
            <DialogDescription>
              Paste magnet links or URLs, or drop .torrent files.
            </DialogDescription>
          </DialogHeader>

          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="urls">Magnet links or URLs</FieldLabel>
              <Textarea
                id="urls"
                rows={3}
                placeholder="magnet:?xt=urn:btih:…  (one per line)"
                className="font-mono text-xs"
                value={urls}
                onChange={(e) => setUrls(e.target.value)}
              />
            </Field>

            <Field>
              <FieldLabel>Torrent files</FieldLabel>
              <button
                type="button"
                onClick={() => fileInput.current?.click()}
                onDragOver={(e) => {
                  e.preventDefault()
                  setDragging(true)
                }}
                onDragLeave={() => setDragging(false)}
                onDrop={(e) => {
                  e.preventDefault()
                  setDragging(false)
                  addFiles(e.dataTransfer.files)
                }}
                data-dragging={dragging || undefined}
                className="flex flex-col items-center justify-center gap-1 rounded-lg border border-dashed p-4 text-sm text-muted-foreground transition-colors hover:bg-muted/50 data-dragging:border-primary data-dragging:bg-muted"
              >
                <UploadIcon className="size-5" />
                Drop files here or click to browse
              </button>
              <input
                ref={fileInput}
                type="file"
                accept=".torrent,application/x-bittorrent"
                multiple
                hidden
                onChange={(e) => {
                  addFiles(e.target.files)
                  e.target.value = ""
                }}
              />
              {files.length > 0 && (
                <ul className="flex flex-col gap-1">
                  {files.map((f, i) => (
                    <li
                      key={`${f.name}-${i}`}
                      className="flex items-center gap-2 rounded-md bg-muted px-2 py-1 text-sm"
                    >
                      <FileIcon className="size-4 shrink-0 text-muted-foreground" />
                      <span className="flex-1 truncate">{f.name}</span>
                      <span className="text-xs text-muted-foreground tabular-nums">
                        {formatBytes(f.size)}
                      </span>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Remove ${f.name}`}
                        onClick={() => setFiles((p) => p.filter((_, j) => j !== i))}
                      >
                        <XIcon />
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </Field>

            <div className="grid grid-cols-2 gap-4">
              <Field>
                <FieldLabel>Category</FieldLabel>
                <Select
                  value={category}
                  onValueChange={(v) => setCategory(v ?? NO_CATEGORY)}
                  items={[
                    { value: NO_CATEGORY, label: "None" },
                    ...categories.map((c) => ({ value: c, label: c })),
                  ]}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value={NO_CATEGORY}>None</SelectItem>
                      {categories.map((c) => (
                        <SelectItem key={c} value={c}>
                          {c}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              <Field>
                <FieldLabel htmlFor="savepath">Save path</FieldLabel>
                <Input
                  id="savepath"
                  placeholder="Default"
                  value={savePath}
                  onChange={(e) => setSavePath(e.target.value)}
                />
              </Field>
            </div>

            <Field orientation="horizontal">
              <FieldContent>
                <FieldLabel htmlFor="paused">Add paused</FieldLabel>
                <FieldDescription>Don't start downloading right away.</FieldDescription>
              </FieldContent>
              <Switch id="paused" checked={paused} onCheckedChange={setPaused} />
            </Field>
          </FieldGroup>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending || total === 0}>
              {pending && <Spinner data-icon="inline-start" />}
              {total > 1 ? `Add ${total} torrents` : "Add torrent"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
