import { useEditor } from "@/components/edit/editor-context"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { ScrollArea } from "@/components/ui/scroll-area"

export const SHORTCUTS: [string, [string, string[]][]][] = [
  [
    "Playback",
    [
      ["Play / pause", ["Space"]],
      ["Play (L), stop (K), back one beat (J)", ["J", "K", "L"]],
      ["One frame back / forward", ["←", "→"]],
      ["One beat back / forward", ["Shift", "← →"]],
      ["Start / end", ["Home", "End"]],
      ["Loop on or off", ["Shift", "L"]],
    ],
  ],
  [
    "Editing",
    [
      ["Split the selected clip at the playhead", ["S"]],
      ["Delete the selected clips (or the picked key)", ["Delete"]],
      ["Undo / redo (yours or the agent's)", ["Ctrl", "Z / Shift Z"]],
      ["Select several clips", ["Shift", "click"]],
      ["Snap to beats and edges on or off", ["N"]],
      ["Deselect", ["Esc"]],
    ],
  ],
  [
    "Range",
    [
      ["In point at the playhead", ["I"]],
      ["Out point at the playhead", ["O"]],
      ["Go to the in / out point", ["[", "]"]],
      ["Clear the range", ["X"]],
    ],
  ],
  [
    "Timeline and window",
    [
      ["Zoom the timeline", ["Ctrl", "wheel"]],
      ["Command palette", ["Ctrl", "K"]],
      ["Pages", ["Ctrl", "1 … 6"]],
      ["This list", ["?"]],
    ],
  ],
]

/** Every key of the Edit page, in a dialog (`?`). */
export function ShortcutsDialog() {
  const ed = useEditor()
  return (
    <Dialog open={ed.shortcutsOpen} onOpenChange={ed.setShortcutsOpen}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>
            Keys work when no text field has the focus. Mouse: drag a card from
            the browser onto a clip or the viewer, or double-click it.
          </DialogDescription>
        </DialogHeader>
        <ScrollArea className="max-h-[60vh]">
          <div className="flex flex-col gap-4 pr-3">
            {SHORTCUTS.map(([group, list]) => (
              <div key={group} className="flex flex-col gap-1.5">
                <h4 className="text-xs font-medium text-muted-foreground">
                  {group}
                </h4>
                {list.map(([what, keys]) => (
                  <div key={what} className="flex items-center gap-3 text-sm">
                    <span className="flex-1">{what}</span>
                    <KbdGroup>
                      {keys.map((k) => (
                        <Kbd key={k}>{k}</Kbd>
                      ))}
                    </KbdGroup>
                  </div>
                ))}
              </div>
            ))}
          </div>
        </ScrollArea>
      </DialogContent>
    </Dialog>
  )
}
