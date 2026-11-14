import * as React from "react"
import type { PanelImperativeHandle } from "react-resizable-panels"

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

/** Width of the browser's tab rail, in pixels: the browser folded. */
const RAIL = 65
/** Below this page width the browser folds by itself. */
const NARROW = 900

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
  // The browser folds to its tab rail (drag it narrow, or click the open tab);
  // it folds by itself when the page gets narrow (the agent panel opened) and
  // opens again when there is room, unless the person folded or opened it.
  const browser = React.useRef<PanelImperativeHandle | null>(null)
  const page = React.useRef<HTMLDivElement>(null)
  const [folded, setFolded] = React.useState(false)
  const auto = React.useRef<"folded" | "manual" | null>(null)
  React.useEffect(() => {
    const el = page.current
    if (!el) return
    let wide: boolean | null = null
    const ro = new ResizeObserver(() => {
      const now = el.clientWidth >= NARROW
      if (now === wide) return
      wide = now
      const b = browser.current
      if (!b) return
      if (!now && !b.isCollapsed() && auto.current !== "manual") {
        b.collapse()
        auto.current = "folded"
      } else if (now && b.isCollapsed() && auto.current === "folded") {
        b.expand()
        auto.current = null
      }
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const fold = (on: boolean) => {
    auto.current = "manual"
    if (on) browser.current?.collapse()
    else browser.current?.expand()
  }
  return (
    <EditorProvider>
      <div
        ref={page}
        className="flex min-h-0 flex-1 flex-col"
        data-slot="edit-page"
      >
        <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
          <ResizablePanel defaultSize="58" minSize="30">
            <ResizablePanelGroup orientation="horizontal">
              <ResizablePanel
                id="browser"
                panelRef={browser}
                defaultSize="26"
                minSize={200}
                collapsible
                collapsedSize={RAIL}
                onResize={(size) => setFolded(size.inPixels <= RAIL + 1)}
              >
                <Browser folded={folded} onFold={fold} />
              </ResizablePanel>
              <ResizableHandle />
              <ResizablePanel id="viewer" defaultSize="46" minSize={240}>
                <div className="flex h-full min-h-0 flex-col">
                  <Viewer />
                  <Transport />
                </div>
              </ResizablePanel>
              <ResizableHandle />
              <ResizablePanel id="inspector" defaultSize="28" minSize={264}>
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
