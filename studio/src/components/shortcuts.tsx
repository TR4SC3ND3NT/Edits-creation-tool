import * as React from "react"

import { api } from "@/lib/api"
import { useStudio, PAGES } from "@/lib/store"
import { jumpUnit, lastFrame } from "@/lib/time"

function editable(target: EventTarget | null) {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      !!target.closest(
        "input, textarea, select, [contenteditable='true'], [role='combobox'], [role='slider']"
      ))
  )
}

/** The keyboard: playback, undo and redo, the command palette, pages. Ctrl+D is the debug panel's (dev builds). */
export function Shortcuts() {
  const s = useStudio()
  const ref = React.useRef(s)
  React.useLayoutEffect(() => {
    ref.current = s
  })
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = ref.current
      const ctrl = e.ctrlKey || e.metaKey
      const key = e.key.toLowerCase()
      if (ctrl && key === "k") {
        e.preventDefault()
        s.setPaletteOpen(!s.paletteOpen)
        return
      }
      if (editable(e.target) || s.paletteOpen) return
      if (ctrl && key === "z") {
        e.preventDefault()
        if (e.shiftKey) s.redo()
        else s.undo()
        return
      }
      if (ctrl && key === "y") {
        e.preventDefault()
        s.redo()
        return
      }
      if (ctrl && /^[1-6]$/.test(e.key)) {
        e.preventDefault()
        s.setPage(PAGES[Number(e.key) - 1].id)
        return
      }
      if (ctrl || e.altKey || !s.info) return
      const info = s.info
      switch (e.key) {
        case " ":
          e.preventDefault()
          s.togglePlay()
          break
        case "ArrowLeft":
          e.preventDefault()
          s.setPlaying(false)
          s.seek(e.shiftKey ? jumpUnit(info, s.frame, false) : s.frame - 1)
          break
        case "ArrowRight":
          e.preventDefault()
          s.setPlaying(false)
          s.seek(e.shiftKey ? jumpUnit(info, s.frame, true) : s.frame + 1)
          break
        case "Home":
          e.preventDefault()
          s.seek(0)
          break
        case "End":
          e.preventDefault()
          s.seek(lastFrame(info))
          break
        case "l":
        case "L":
          s.setLooping(!s.looping)
          break
        case "Escape":
          s.select(null)
          break
        case "Delete":
        case "Backspace":
          if (s.layer) {
            e.preventDefault()
            void s.run(api.layerRemove(s.layer))
          }
          break
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])
  return null
}
