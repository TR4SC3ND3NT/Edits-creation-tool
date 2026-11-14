import { Inspector } from "@/components/edit/inspector"
import { Timeline } from "@/components/edit/timeline"
import { Transport } from "@/components/edit/transport"
import { Viewer } from "@/components/edit/viewer"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { useStudio } from "@/lib/store"

/** The main page: the frame viewer and transport, the inspector, the timeline. */
export function EditPage() {
  const { lastAction } = useStudio()
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
        <ResizablePanel defaultSize="60" minSize="30">
          <ResizablePanelGroup orientation="horizontal">
            <ResizablePanel defaultSize="66" minSize="35">
              <div className="flex h-full min-h-0 flex-col">
                <Viewer />
                <Transport />
              </div>
            </ResizablePanel>
            <ResizableHandle />
            <ResizablePanel defaultSize="34" minSize="24">
              <Inspector />
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel defaultSize="40" minSize="20">
          <Timeline />
        </ResizablePanel>
      </ResizablePanelGroup>
      <div className="flex h-7 shrink-0 items-center gap-2 border-t px-3 text-xs text-muted-foreground">
        <span className="truncate">
          {lastAction ||
            "Every edit is checked, written to the project file and kept in the history shared with the agent."}
        </span>
      </div>
    </div>
  )
}
