import { AccentDialog, BeatCutDialog } from "@/components/edit/beat-dialogs"
import { Browser } from "@/components/edit/browser"
import { EditorProvider, useEditor } from "@/components/edit/editor-context"
import { Inspector } from "@/components/edit/inspector"
import { ShortcutsDialog } from "@/components/edit/shortcuts-dialog"
import { Timeline } from "@/components/edit/timeline"
import { Transport } from "@/components/edit/transport"
import { Viewer } from "@/components/edit/viewer"
import { Kbd } from "@/components/ui/kbd"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import { useStudio } from "@/lib/store"

function StatusLine() {
  const { lastAction } = useStudio()
  const ed = useEditor()
  return (
    <div className="flex h-7 shrink-0 items-center gap-2 border-t px-3 text-xs text-muted-foreground">
      <span className="min-w-0 flex-1 truncate">
        {lastAction ||
          "Drag an effect onto a clip, double-click it to apply it at the playhead. Every edit is written to the project file and kept in the history shared with the agent."}
      </span>
      <button
        type="button"
        className="flex shrink-0 items-center gap-1 hover:text-foreground"
        onClick={() => ed.setShortcutsOpen(true)}
      >
        <Kbd>?</Kbd> shortcuts
      </button>
    </div>
  )
}

/**
 * The Edit page, laid out like the desktop edit apps: the browser on the
 * left, the viewer with its transport in the middle, the inspector on the
 * right, the timeline across the bottom. The dividers can be dragged.
 */
export function EditPage() {
  return (
    <EditorProvider>
      <div className="flex min-h-0 flex-1 flex-col" data-slot="edit-page">
        <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
          <ResizablePanel defaultSize="58" minSize="30">
            <ResizablePanelGroup orientation="horizontal">
              <ResizablePanel defaultSize="23" minSize="14">
                <Browser />
              </ResizablePanel>
              <ResizableHandle />
              <ResizablePanel defaultSize="49" minSize="28">
                <div className="flex h-full min-h-0 flex-col">
                  <Viewer />
                  <Transport />
                </div>
              </ResizablePanel>
              <ResizableHandle />
              <ResizablePanel defaultSize="28" minSize="18">
                <Inspector />
              </ResizablePanel>
            </ResizablePanelGroup>
          </ResizablePanel>
          <ResizableHandle />
          <ResizablePanel defaultSize="42" minSize="18">
            <Timeline />
          </ResizablePanel>
        </ResizablePanelGroup>
        <StatusLine />
      </div>
      <ShortcutsDialog />
      <BeatCutDialog />
      <AccentDialog />
    </EditorProvider>
  )
}
