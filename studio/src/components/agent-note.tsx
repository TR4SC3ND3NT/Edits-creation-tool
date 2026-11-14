import * as React from "react"
import { MessageSquareIcon, MessageSquareTextIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import { Textarea } from "@/components/ui/textarea"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { useStudio } from "@/lib/store"

/** "Tell the agent…": a note that goes into `ui.json`, which the agent reads with `ui_context`. */
export function AgentNote() {
  const { note, setNote } = useStudio()
  const [open, setOpen] = React.useState(false)
  const [text, setText] = React.useState(note)
  const save = () => {
    setNote(text)
    setOpen(false)
  }
  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o)
        if (o) setText(note)
      }}
    >
      <Tooltip>
        <TooltipTrigger
          render={
            <PopoverTrigger
              render={<Button variant="ghost" size="icon" className="size-8" />}
            />
          }
        >
          {note.trim() ? <MessageSquareTextIcon /> : <MessageSquareIcon />}
          <span className="sr-only">Note for the agent</span>
        </TooltipTrigger>
        <TooltipContent>
          {note.trim() ? `Note for the agent: ${note}` : "Tell the agent…"}
        </TooltipContent>
      </Tooltip>
      <PopoverContent align="end" className="w-80">
        <PopoverHeader>
          <PopoverTitle>Tell the agent</PopoverTitle>
          <PopoverDescription>
            Saved in .edits/ui.json with the selection and the playhead; the
            agent reads it with ui_context.
          </PopoverDescription>
        </PopoverHeader>
        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="make this hit stronger"
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) save()
          }}
        />
        <div className="flex justify-end gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setText("")
              setNote("")
              setOpen(false)
            }}
          >
            Clear
          </Button>
          <Button size="sm" onClick={save}>
            Save note
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
