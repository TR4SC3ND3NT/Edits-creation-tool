// The right dock of the app shell: the page on the left (under the agent's
// status strip), the agent panel on the right, a draggable divider between
// them. Ctrl+L or the header button shows and hides the panel.

import * as React from "react"
import { BotIcon } from "lucide-react"

import { PresenceBridge } from "@/agent/bridge"
import { AgentPanel } from "@/agent/panel"
import { AgentStatusStrip } from "@/agent/presence-ui"
import { AgentProvider, useAgent } from "@/agent/store"
import { Button } from "@/components/ui/button"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

export { AgentProvider }

/** The page with the agent panel docked on its right. */
export function AgentDock({ children }: { children: React.ReactNode }) {
  const { open, setOpen } = useAgent()
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (
        (e.ctrlKey || e.metaKey) &&
        !e.shiftKey &&
        !e.altKey &&
        e.key.toLowerCase() === "l"
      ) {
        e.preventDefault()
        e.stopPropagation()
        setOpen(!open)
      }
    }
    window.addEventListener("keydown", onKey, true)
    return () => window.removeEventListener("keydown", onKey, true)
  }, [open, setOpen])
  const page = (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <AgentStatusStrip />
      <div className="@container/main flex min-h-0 flex-1 flex-col">
        {children}
      </div>
    </div>
  )
  return (
    <>
      <PresenceBridge />
      {open ? (
        <ResizablePanelGroup
          orientation="horizontal"
          className="min-h-0 flex-1"
        >
          <ResizablePanel id="page" defaultSize="70" minSize="40">
            {page}
          </ResizablePanel>
          <ResizableHandle withHandle />
          <ResizablePanel id="agent" defaultSize="30" minSize="20" maxSize="55">
            <AgentPanel />
          </ResizablePanel>
        </ResizablePanelGroup>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">{page}</div>
      )}
    </>
  )
}

/** The header's button for the panel. */
export function AgentToggle() {
  const { open, setOpen, agentName, thread } = useAgent()
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant={open ? "secondary" : "ghost"}
            size="sm"
            className="gap-1.5"
            onClick={() => setOpen(!open)}
            aria-pressed={open}
          />
        }
      >
        <BotIcon data-icon="inline-start" />
        {thread?.busy ? `${agentName} is working` : "Agent"}
      </TooltipTrigger>
      <TooltipContent>
        {open ? "Hide" : "Show"} the agent panel{" "}
        <KbdGroup>
          <Kbd>Ctrl</Kbd>
          <Kbd>L</Kbd>
        </KbdGroup>
      </TooltipContent>
    </Tooltip>
  )
}
