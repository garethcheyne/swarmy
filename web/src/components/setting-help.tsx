import { CircleHelpIcon } from "lucide-react"

import { SETTINGS_HELP, type HelpId } from "@/lib/settings-help"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"

/**
 * The ⓘ next to a setting. Opens on click or tap (not hover), so it works on
 * phones, and stays open while you read it. Keep it outside the <label>, or
 * clicking it would also flip the setting.
 */
export function SettingHelp({ id }: { id: HelpId }) {
  const topic = SETTINGS_HELP[id]
  return (
    <Popover>
      <PopoverTrigger
        aria-label={`What "${topic.title}" does`}
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none data-[popup-open]:text-primary"
      >
        <CircleHelpIcon className="size-3.5" />
      </PopoverTrigger>
      <PopoverContent align="start" className="max-h-(--available-height) w-80 max-w-[calc(100vw-1.5rem)] gap-3 overflow-y-auto p-4 text-[13px] leading-relaxed">
        <p className="text-sm font-semibold text-foreground">{topic.title}</p>
        <div className="flex flex-col gap-0.5">
          <p className="font-medium text-foreground">What it does</p>
          <p className="text-muted-foreground">{topic.what}</p>
        </div>
        <div className="flex flex-col gap-0.5">
          <p className="font-medium text-foreground">What changes</p>
          <p className="text-muted-foreground">{topic.effect}</p>
        </div>
        {"tip" in topic && topic.tip && (
          <div className="flex flex-col gap-0.5 rounded-md bg-muted/60 p-2.5">
            <p className="font-medium text-foreground">Good to know</p>
            <p className="text-muted-foreground">{topic.tip}</p>
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}
