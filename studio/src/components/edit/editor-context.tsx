// The Edit page's own state, next to the studio's (`lib/store.tsx`): the
// layers selected on the timeline (several with Shift or a marquee), the
// in and out points (I / O), snapping and ripple, the live preview shown
// while a control is dragged, the song analysis and waveform, and how an
// item of the browser (an effect, a preset, a transition, a text...) is
// applied. Its keyboard handles the editing keys of this page.

import * as React from "react"
import { listen } from "@tauri-apps/api/event"
import { isTauri } from "@tauri-apps/api/core"
import { toast } from "sonner"

import {
  api,
  errorText,
  type EditDone,
  type Info,
  type PropKey,
  type Row,
  type Song,
  type Waveform,
} from "@/lib/api"
import { useStudio } from "@/lib/store"
import { frameAt, jumpUnit, lastFrame, tidy } from "@/lib/time"

/** Something from the browser that can be applied. */
export type BrowserItem =
  | { type: "effect"; name: string }
  | { type: "preset"; name: string; project: boolean; text: boolean }
  | { type: "transition"; name: string }
  | { type: "layer"; name: string } // text, shape:rect, particles, solid, adjustment
  | { type: "slot"; name: string } // a picture slot: an image layer of it

/** A browser card being dragged (with the pointer, not HTML drag and drop, so it works in every webview). */
export type ItemDrag = {
  item: BrowserItem
  label: string
  x: number
  y: number
} | null

/** A place that takes dropped items: true when the drop at (x, y) was its. */
export type DropHandler = (x: number, y: number, item: BrowserItem) => boolean

type Live = { bitmap: ImageBitmap; frame: number } | null

/** A key picked on a keyframe lane of the timeline. */
export type PickedKey = { layer: string | null; key: PropKey; t: number } | null

type Editor = {
  selected: string[]
  setSelected: (ids: string[]) => void
  /** Select one layer; with `add`, add it to (or take it from) the selection. */
  pick: (id: string, add?: boolean) => void
  range: [number, number] | null
  setRange: (r: [number, number] | null) => void
  keyPicked: PickedKey
  setKeyPicked: (k: PickedKey) => void
  snap: boolean
  setSnap: (on: boolean) => void
  ripple: boolean
  setRipple: (on: boolean) => void
  guides: boolean
  setGuides: (on: boolean) => void
  live: Live
  /** Show a property at `value` without writing it (while dragging). */
  previewProp: (layer: string | null, key: PropKey, value: number[]) => void
  previewOps: (ops: unknown[]) => void
  endPreview: () => void
  song: Song | null
  analyze: () => void
  waveform: Waveform | null
  itemDrag: ItemDrag
  /** Start dragging a browser item from a pointer press (it becomes a drag once the pointer moves). */
  beginItemDrag: (
    e: React.PointerEvent,
    item: BrowserItem,
    label: string
  ) => void
  /** Take drops; returns the function that stops taking them. */
  registerDrop: (h: DropHandler) => () => void
  shortcutsOpen: boolean
  setShortcutsOpen: (open: boolean) => void
  cutOpen: boolean
  setCutOpen: (open: boolean) => void
  accentOpen: boolean
  setAccentOpen: (open: boolean) => void
  /** Apply a browser item to `layer` (the selection, or the picture on screen). */
  apply: (item: BrowserItem, layer?: string | null) => Promise<void>
  /** The layer an item lands on when none is named: the selection, else the top picture under the playhead. */
  targetLayer: () => string | null
  split: () => void
  remove: (ids?: string[]) => void
}

const EditorContext = React.createContext<Editor | null>(null)

export function useEditor() {
  const e = React.useContext(EditorContext)
  if (!e) throw new Error("useEditor outside EditorProvider")
  return e
}

const PICTURE = new Set(["image", "video", "cube", "comp"])

/** `t` moved onto the quarter-beat grid when it is less than a frame from it (beats projects). */
export function onGrid(info: Info, t: number) {
  if (info.time_base !== "beats" || !info.bpm) return t
  const q = Math.round(t * 4) / 4
  const frameBeats = info.bpm / 60 / info.fps
  return Math.abs(q - t) < frameBeats ? q : t
}

/** The top picture layer visible at `t`. */
export function pictureAt(rows: Row[], t: number) {
  return (
    rows.find(
      (r) =>
        r.index != null &&
        r.enabled &&
        PICTURE.has(r.kind) &&
        r.start <= t + 1e-6 &&
        t < r.end - 1e-6
    )?.id ?? null
  )
}

function editable(target: EventTarget | null) {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      !!target.closest(
        "input, textarea, select, [contenteditable='true'], [role='combobox'], [role='slider'], [role='dialog']"
      ))
  )
}

export function EditorProvider({ children }: { children: React.ReactNode }) {
  const s = useStudio()
  const { snapshot, info } = s
  const [selected, setSelectedState] = React.useState<string[]>(
    s.layer ? [s.layer] : []
  )
  const [range, setRange] = React.useState<[number, number] | null>(null)
  const [keyPicked, setKeyPicked] = React.useState<PickedKey>(null)
  const [snap, setSnap] = React.useState(true)
  const [ripple, setRipple] = React.useState(false)
  const [guides, setGuides] = React.useState(false)
  const [live, setLive] = React.useState<Live>(null)
  const [song, setSong] = React.useState<Song | null>(null)
  const [waveform, setWaveform] = React.useState<Waveform | null>(null)
  const [shortcutsOpen, setShortcutsOpen] = React.useState(false)
  const [itemDrag, setItemDrag] = React.useState<ItemDrag>(null)
  const drops = React.useRef(new Set<DropHandler>())
  const registerDrop = React.useCallback((h: DropHandler) => {
    drops.current.add(h)
    return () => {
      drops.current.delete(h)
    }
  }, [])
  const beginItemDrag = React.useCallback(
    (e: React.PointerEvent, item: BrowserItem, label: string) => {
      if (e.button !== 0) return
      const x0 = e.clientX
      const y0 = e.clientY
      let started = false
      const move = (ev: PointerEvent) => {
        if (!started && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return
        started = true
        setItemDrag({ item, label, x: ev.clientX, y: ev.clientY })
      }
      const up = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", move)
        window.removeEventListener("pointerup", up)
        if (!started) return
        setItemDrag(null)
        for (const h of drops.current)
          if (h(ev.clientX, ev.clientY, item)) break
      }
      window.addEventListener("pointermove", move)
      window.addEventListener("pointerup", up)
    },
    []
  )
  const [cutOpen, setCutOpen] = React.useState(false)
  const [accentOpen, setAccentOpen] = React.useState(false)

  // The studio's selection is the primary one; keep the list in step with it.
  React.useEffect(() => {
    setSelectedState((cur) =>
      s.layer == null ? [] : cur.includes(s.layer) ? cur : [s.layer]
    )
  }, [s.layer])
  const setSelected = React.useCallback(
    (ids: string[]) => {
      setSelectedState(ids)
      const primary = ids[ids.length - 1] ?? null
      if (primary !== s.layer) s.select(primary)
    },
    [s]
  )
  const pick = React.useCallback(
    (id: string, add = false) => {
      if (!add) return setSelected([id])
      setSelected(
        selected.includes(id)
          ? selected.filter((x) => x !== id)
          : [...selected, id]
      )
    },
    [selected, setSelected]
  )
  // Layers that are gone leave the selection.
  React.useEffect(() => {
    if (!snapshot) return
    setSelectedState((cur) =>
      cur.filter((id) => snapshot.layers.some((l) => l.id === id))
    )
  }, [snapshot])

  // ---- live preview ------------------------------------------------------------------------
  const busy = React.useRef(false)
  const next = React.useRef<(() => Promise<ArrayBuffer>) | null>(null)
  const ended = React.useRef(false)
  const frameRef = React.useRef(s.frame)
  React.useLayoutEffect(() => {
    frameRef.current = s.frame
  })
  const pumpRef = React.useRef<() => void>(() => {})
  const pump = React.useCallback(() => {
    if (busy.current || !next.current) return
    const job = next.current
    next.current = null
    busy.current = true
    const frame = frameRef.current
    job()
      .then((buf) => createImageBitmap(new Blob([buf], { type: "image/jpeg" })))
      .then((bitmap) => {
        if (!ended.current) setLive({ bitmap, frame })
      })
      .catch(() => {})
      .finally(() => {
        busy.current = false
        pumpRef.current()
      })
  }, [])
  React.useLayoutEffect(() => {
    pumpRef.current = pump
  })
  const scaleRef = React.useRef(s.scale)
  React.useLayoutEffect(() => {
    scaleRef.current = s.scale
  })
  /** Interaction renders at half the preview size (at least a quarter of the project's). */
  const liveScale = () => Math.max(0.25, scaleRef.current / 2)
  const previewProp = React.useCallback(
    (layer: string | null, key: PropKey, value: number[]) => {
      if (!info) return
      ended.current = false
      const t = s.t
      next.current = () =>
        api.previewProp(layer, key, t, value, frameRef.current, liveScale())
      pump()
    },
    [info, s.t, pump]
  )
  const previewOps = React.useCallback(
    (ops: unknown[]) => {
      ended.current = false
      next.current = () => api.previewOps(ops, frameRef.current, liveScale())
      pump()
    },
    [pump]
  )
  const endPreview = React.useCallback(() => {
    ended.current = true
    next.current = null
    setLive(null)
  }, [])
  // A new version (the committed edit) or another frame ends the preview.
  const revision = snapshot?.revision
  React.useEffect(() => {
    setLive(null)
  }, [revision, s.frame])

  // ---- the song ------------------------------------------------------------------------------
  const path = snapshot?.path
  const audioKey = `${info?.audio?.path ?? ""}:${info?.audio?.offset ?? 0}:${info?.bpm ?? 0}`
  React.useEffect(() => {
    if (!isTauri() || !path) return
    api
      .songStatus()
      .then(setSong)
      .catch(() => {})
  }, [path, audioKey])
  React.useEffect(() => {
    if (!isTauri() || !path || !info?.audio?.exists) {
      setWaveform(null)
      return
    }
    let live = true
    api
      .waveform()
      .then((w) => live && setWaveform(w))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [path, info?.audio?.path, info?.audio?.exists])
  React.useEffect(() => {
    if (!isTauri()) return
    let un: (() => void) | undefined
    listen<Song>("song", (e) => {
      setSong(e.payload)
      if (e.payload.error)
        toast.error("The song could not be analysed", {
          description: e.payload.error,
        })
      else if (e.payload.marks)
        toast.success("Song analysed", {
          description: e.payload.marks.summary,
        })
    }).then((u) => (un = u))
    return () => un?.()
  }, [])
  const analyze = React.useCallback(() => {
    api
      .songAnalyze()
      .then(setSong)
      .catch((e) =>
        toast.error("Cannot analyse the song", { description: errorText(e) })
      )
  }, [])

  // ---- applying browser items ------------------------------------------------------------------
  const rows = snapshot?.rows
  const targetLayer = React.useCallback(
    () => s.layer ?? (rows ? pictureAt(rows, s.t) : null),
    [s.layer, s.t, rows]
  )

  // After adding a layer, select it.
  const known = React.useRef<Set<string> | null>(null)
  React.useEffect(() => {
    if (!known.current || !snapshot) return
    const fresh = snapshot.layers.filter((l) => !known.current!.has(l.id))
    if (fresh.length) {
      known.current = null
      setSelected([fresh[fresh.length - 1].id])
    }
  }, [snapshot, setSelected])
  const selectNew = React.useCallback(
    async (what: Promise<EditDone>) => {
      known.current = new Set(snapshot?.layers.map((l) => l.id) ?? [])
      const done = await s.run(what)
      if (!done) known.current = null
    },
    [s, snapshot]
  )

  const apply = React.useCallback(
    async (item: BrowserItem, layer?: string | null) => {
      const target = layer ?? targetLayer()
      // The playhead sits on a frame; a beat (or quarter beat) less than a frame away is meant.
      const at = info ? onGrid(info, s.t) : s.t
      const kindOf = (id: string | null) =>
        snapshot?.layers.find((l) => l.id === id)?.kind
      switch (item.type) {
        case "effect":
          if (!target) {
            toast("Nothing to put it on", {
              description:
                "Select a layer, or move the playhead over a picture.",
            })
            return
          }
          await s.run(api.effectAdd(target, item.name))
          if (target !== s.layer) setSelected([target])
          return
        case "transition":
          if (!target) {
            toast("No clip to come in with it", {
              description:
                "Drop the transition on the clip that should come in, or select it.",
            })
            return
          }
          await s.run(
            api.presetApply({
              name: `transition:${item.name}`,
              target: "selected",
              selected: target,
              at: "cut",
              playhead: at,
              values: { align: "after" },
            })
          )
          return
        case "preset": {
          if (item.text && kindOf(target) !== "text") {
            toast("This animates a text layer", {
              description:
                "Select a text layer (Text tab: Title adds one), then apply it again.",
            })
            return
          }
          const run = api.presetApply({
            name: item.name,
            target: target ? "selected" : "auto",
            selected: target,
            at: item.text && target ? "cut" : "playhead",
            playhead: at,
            values: {},
          })
          if (item.project) await selectNew(run)
          else await s.run(run)
          return
        }
        case "layer":
          await selectNew(api.layerNew(item.name, s.t, s.layer))
          return
        case "slot":
          await selectNew(api.layerAdd("image", item.name, s.t, s.layer))
          return
      }
    },
    [s, info, snapshot, targetLayer, selectNew, setSelected]
  )

  const split = React.useCallback(() => {
    const id = s.layer ?? (rows ? pictureAt(rows, s.t) : null)
    if (!id) {
      toast("Nothing to split", {
        description: "Select a clip and put the playhead inside it.",
      })
      return
    }
    void s.run(api.layerSplit(id, s.t))
  }, [s, rows])

  const remove = React.useCallback(
    (ids?: string[]) => {
      const list = ids ?? selected
      if (!list.length) return
      void s.run(api.layersRemove(list, ripple))
    },
    [s, selected, ripple]
  )

  // ---- keys of the Edit page -----------------------------------------------------------------------
  // A picked key goes when the version changes (it may have moved).
  React.useEffect(() => setKeyPicked(null), [revision])
  const removeKey = React.useCallback(() => {
    if (!keyPicked) return false
    void s.run(api.propRemoveKey(keyPicked.layer, keyPicked.key, keyPicked.t))
    setKeyPicked(null)
    return true
  }, [keyPicked, s])

  const ref = React.useRef({ s, range, split, remove, setRange, removeKey })
  React.useLayoutEffect(() => {
    ref.current = { s, range, split, remove, setRange, removeKey }
  })
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { s, range, split, remove, setRange, removeKey } = ref.current
      if (editable(e.target) || s.paletteOpen || !s.info) return
      if (e.ctrlKey || e.metaKey || e.altKey) return
      const info = s.info
      const key = e.key
      if (key === "?" || (key === "/" && e.shiftKey)) {
        e.preventDefault()
        setShortcutsOpen((o) => !o)
        return
      }
      switch (key.toLowerCase()) {
        case "s":
          e.preventDefault()
          split()
          break
        case "delete":
        case "backspace":
          e.preventDefault()
          e.stopImmediatePropagation()
          if (!removeKey()) remove()
          break
        case "j":
          e.preventDefault()
          s.setPlaying(false)
          s.seek(jumpUnit(info, s.frame, false))
          break
        case "k":
          e.preventDefault()
          s.setPlaying(false)
          break
        case "l":
          e.preventDefault()
          e.stopImmediatePropagation()
          if (e.shiftKey) s.setLooping(!s.looping)
          else if (!s.playing) s.setPlaying(true)
          break
        case "i":
          e.preventDefault()
          setRange([s.t, Math.max(s.t + 0.25, range?.[1] ?? info.duration)])
          break
        case "o":
          e.preventDefault()
          setRange([Math.min(range?.[0] ?? 0, s.t - 0.25), s.t])
          break
        case "x":
          if (range) {
            e.preventDefault()
            setRange(null)
          }
          break
        case "n":
          e.preventDefault()
          setSnap((v) => !v)
          break
        case "[":
          e.preventDefault()
          s.seek(Math.max(0, frameAt(info, range?.[0] ?? 0)))
          break
        case "]":
          e.preventDefault()
          s.seek(
            Math.min(
              lastFrame(info),
              frameAt(info, range?.[1] ?? info.duration)
            )
          )
          break
      }
    }
    // Capture, so this page's Delete and L win over the app-wide keys.
    window.addEventListener("keydown", onKey, true)
    return () => window.removeEventListener("keydown", onKey, true)
  }, [])

  const value: Editor = {
    selected,
    setSelected,
    pick,
    range: range ? [tidy(range[0], 4), tidy(range[1], 4)] : null,
    setRange,
    keyPicked,
    setKeyPicked,
    snap,
    setSnap,
    ripple,
    setRipple,
    guides,
    setGuides,
    live,
    previewProp,
    previewOps,
    endPreview,
    song,
    analyze,
    waveform,
    itemDrag,
    beginItemDrag,
    registerDrop,
    shortcutsOpen,
    setShortcutsOpen,
    cutOpen,
    setCutOpen,
    accentOpen,
    setAccentOpen,
    apply,
    targetLayer,
    split,
    remove,
  }
  return (
    <EditorContext.Provider value={value}>
      {children}
      {itemDrag && (
        <div
          className="pointer-events-none fixed z-50 -translate-x-1/2 -translate-y-[130%] rounded-md border bg-popover px-2 py-1 text-xs text-popover-foreground shadow-md"
          style={{ left: itemDrag.x, top: itemDrag.y }}
        >
          {itemDrag.label}
        </div>
      )}
    </EditorContext.Provider>
  )
}
