import { useRef, useState, type FormEvent } from "react"
import { FileIcon, UploadIcon, XIcon } from "lucide-react"
import { toast } from "sonner"

import { api } from "@/lib/api"
import { formatBytes } from "@/lib/format"
import { navigate } from "@/lib/route"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Textarea } from "@/components/ui/textarea"

const NO_CATEGORY = "__none__"
const PRIORITIES = [
  { value: "0", label: "Normal" },
  { value: "1", label: "High" },
  { value: "2", label: "Force (skip the queue)" },
  { value: "-1", label: "Low" },
  { value: "-2", label: "Paused" },
]

export function AddNzbDialog({
  open,
  onOpenChange,
  categories,
  initialFiles,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  categories: string[]
  initialFiles?: File[]
}) {
  const [urls, setUrls] = useState("")
  const [files, setFiles] = useState<File[]>(initialFiles ?? [])
  const [category, setCategory] = useState(NO_CATEGORY)
  const [priority, setPriority] = useState("0")
  const [pending, setPending] = useState(false)
  const [dragging, setDragging] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const addFiles = (list: FileList | null) => {
    if (!list) return
    setFiles((prev) => [...prev, ...[...list].filter((f) => f.name.toLowerCase().endsWith(".nzb"))])
  }
  const linkCount = urls.split("\n").filter((l) => l.trim()).length
  const total = linkCount + files.length

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (total === 0) return
    const form = new FormData()
    if (urls.trim()) form.append("url", urls)
    for (const f of files) form.append("nzb", f, f.name)
    if (category !== NO_CATEGORY) form.append("category", category)
    form.append("priority", priority)
    setPending(true)
    try {
      const ids = await api.usenetAdd(form)
      toast.success(ids.length === 1 ? "Added to the Usenet queue" : `${ids.length} added to the Usenet queue`)
      onOpenChange(false)
      navigate("usenet")
    } catch (err) {
      toast.error("Couldn't add it", { description: err instanceof Error ? err.message : String(err) })
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={submit} className="flex flex-col gap-6">
          <DialogHeader>
            <DialogTitle>Add NZBs</DialogTitle>
            <DialogDescription>Drop .nzb files, or paste links to them from your indexer.</DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel>NZB files</FieldLabel>
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
                accept=".nzb,application/x-nzb"
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
                    <li key={`${f.name}-${i}`} className="flex items-center gap-2 rounded-md bg-muted px-2 py-1 text-sm">
                      <FileIcon className="size-4 shrink-0 text-muted-foreground" />
                      <span className="flex-1 truncate">{f.name}</span>
                      <span className="text-xs text-muted-foreground tabular-nums">{formatBytes(f.size)}</span>
                      <Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${f.name}`} onClick={() => setFiles((p) => p.filter((_, j) => j !== i))}>
                        <XIcon />
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </Field>
            <Field>
              <FieldLabel htmlFor="nzb-urls">Links</FieldLabel>
              <Textarea id="nzb-urls" rows={2} placeholder="https://indexer.example/getnzb/…  (one per line)" className="font-mono text-xs" value={urls} onChange={(e) => setUrls(e.target.value)} />
            </Field>
            <div className="grid grid-cols-2 gap-4">
              <Field>
                <FieldLabel>Category</FieldLabel>
                <Select value={category} onValueChange={(v) => setCategory(v ?? NO_CATEGORY)} items={[{ value: NO_CATEGORY, label: "None" }, ...categories.map((c) => ({ value: c, label: c }))]}>
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
                <FieldLabel>Priority</FieldLabel>
                <Select value={priority} onValueChange={(v) => setPriority(v ?? "0")} items={PRIORITIES}>
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {PRIORITIES.map((p) => (
                        <SelectItem key={p.value} value={p.value}>
                          {p.label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
            </div>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending || total === 0}>
              {pending && <Spinner data-icon="inline-start" />}
              {total > 1 ? `Add ${total} NZBs` : "Add NZB"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
