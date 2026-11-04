import { useState } from "react"

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldLabel } from "@/components/ui/field"
import { Spinner } from "@/components/ui/spinner"

interface Props {
  names: string[]
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: (deleteFiles: boolean) => Promise<void>
}

/** Remount with a new `key` on each open to reset the checkbox. */
export function DeleteDialog({ names, open, onOpenChange, onConfirm }: Props) {
  const [deleteFiles, setDeleteFiles] = useState(false)
  const [pending, setPending] = useState(false)

  const title =
    names.length === 1 ? `Delete “${names[0]}”?` : `Delete ${names.length} torrents?`

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="break-all">{title}</AlertDialogTitle>
          <AlertDialogDescription>
            The torrent will be removed from Swarmy.{" "}
            {deleteFiles
              ? "Downloaded files will be permanently deleted from disk."
              : "Downloaded files stay on disk."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <Field orientation="horizontal">
          <Checkbox
            id="delete-files"
            checked={deleteFiles}
            onCheckedChange={setDeleteFiles}
          />
          <FieldLabel htmlFor="delete-files">Also delete downloaded files</FieldLabel>
        </Field>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <Button
            variant="destructive"
            disabled={pending}
            onClick={async () => {
              setPending(true)
              try {
                await onConfirm(deleteFiles)
                onOpenChange(false)
              } finally {
                setPending(false)
              }
            }}
          >
            {pending && <Spinner data-icon="inline-start" />}
            Delete
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
