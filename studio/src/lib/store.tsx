// The studio's state, shared by every page: the project snapshot (pushed by
// the Rust side on every change of the file), what is selected, the
// playhead and playback (with the project's audio), and the bookkeeping
// that tells the agent what the person is looking at (`ui.json`).

import * as React from "react"
import { listen } from "@tauri-apps/api/event"
import { convertFileSrc, isTauri } from "@tauri-apps/api/core"
import { toast } from "sonner"

import {
  api,
  errorText,
  type EditDone,
  type Info,
  type ProjectEvent,
  type PropKey,
  type RenderStatus,
  type Snapshot,
} from "@/lib/api"
import { clearPrefetch, keepRevision } from "@/lib/frames"
import { frameTime, lastFrame, toSeconds } from "@/lib/time"

export type Page =
  "edit" | "overview" | "reference" | "library" | "activity" | "renders"

export const PAGES: { id: Page; title: string }[] = [
  { id: "edit", title: "Edit" },
  { id: "overview", title: "Overview" },
  { id: "reference", title: "Reference" },
  { id: "library", title: "Presets & Effects" },
  { id: "activity", title: "Activity" },
  { id: "renders", title: "Renders" },
]

export type Picked = {
  key: PropKey
  pointer: string
  effect: number | null
  keyIndex: number | null
} | null

type Studio = {
  snapshot: Snapshot | null
  info: Info | null
  page: Page
  setPage: (p: Page) => void
  layer: string | null
  camera: boolean
  select: (layer: string | null, camera?: boolean) => void
  picked: Picked
  setPicked: (p: Picked) => void
  frame: number
  t: number
  seek: (frame: number) => void
  playing: boolean
  togglePlay: () => void
  setPlaying: (on: boolean) => void
  looping: boolean
  setLooping: (on: boolean) => void
  scale: number
  setScale: (s: number) => void
  blurPaused: boolean
  setBlurPaused: (on: boolean) => void
  visible: [number, number] | null
  setVisible: (v: [number, number] | null) => void
  note: string
  noteAt: string | null
  setNote: (text: string) => void
  renderStatus: RenderStatus | null
  audioState: string
  lastAction: string
  paletteOpen: boolean
  setPaletteOpen: (open: boolean) => void
  /** Run an edit command: its result goes to the status line, its error to a toast. */
  run: (what: Promise<EditDone>) => Promise<EditDone | null>
  undo: () => void
  redo: () => void
  openProject: (path: string) => Promise<void>
}

const StudioContext = React.createContext<Studio | null>(null)

export function useStudio() {
  const s = React.useContext(StudioContext)
  if (!s) throw new Error("useStudio outside StudioProvider")
  return s
}

function stamp(d = new Date()) {
  const p = (n: number, w = 2) => String(n).padStart(w, "0")
  return `${d.getUTCFullYear()}${p(d.getUTCMonth() + 1)}${p(d.getUTCDate())}T${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}.${p(d.getUTCMilliseconds(), 3)}Z`
}

export function StudioProvider({ children }: { children: React.ReactNode }) {
  const [snapshot, setSnapshot] = React.useState<Snapshot | null>(null)
  const [page, setPage] = React.useState<Page>("edit")
  const [layer, setLayer] = React.useState<string | null>(null)
  const [camera, setCamera] = React.useState(false)
  const [picked, setPicked] = React.useState<Picked>(null)
  const [frame, setFrame] = React.useState(0)
  const [playing, setPlayingState] = React.useState(false)
  const [looping, setLooping] = React.useState(true)
  const [scale, setScale] = React.useState(0.5)
  const [blurPaused, setBlurPaused] = React.useState(true)
  const [visible, setVisible] = React.useState<[number, number] | null>(null)
  const [note, setNoteText] = React.useState("")
  const [noteAt, setNoteAt] = React.useState<string | null>(null)
  const [renderStatus, setRenderStatus] = React.useState<RenderStatus | null>(
    null
  )
  const [audioState, setAudioState] = React.useState("no audio")
  const [lastAction, setLastAction] = React.useState("")
  const [paletteOpen, setPaletteOpen] = React.useState(false)

  const info = snapshot?.info ?? null
  const infoRef = React.useRef(info)
  React.useLayoutEffect(() => {
    infoRef.current = info
  })
  const t = info ? frameTime(info, frame) : 0

  // ---- the project, pushed by the Rust side ----------------------------------------
  const undoRef = React.useRef<() => void>(() => {})
  React.useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    listen<ProjectEvent>("project", (e) => {
      const { snapshot: s, reason } = e.payload
      setSnapshot(s)
      keepRevision(s.revision)
      switch (reason.kind) {
        case "edit":
          toast(
            reason.actor === "agent" ? "Agent edit" : `Edit by ${reason.actor}`,
            {
              description: s.history.undo[0]?.readable ?? reason.summary,
              action: { label: "Undo", onClick: () => undoRef.current() },
            duration: 8000,
            }
          )
          break
        case "undo":
          toast(
            reason.actor === "agent"
              ? "The agent undid an edit"
              : "An edit was undone",
            {
              description: reason.summary,
            }
          )
          break
        case "reloaded":
          toast("Reloaded", {
            description: `The file changed on disk · revision ${s.revision}`,
          })
          break
        case "invalid":
          toast.error(
            `The file has ${reason.errors} error${reason.errors === 1 ? "" : "s"}`,
            {
              description: `Showing revision ${s.revision} until it is fixed`,
            }
          )
          break
        case "window":
          setLastAction(reason.summary)
          break
      }
    }).then((u) => (unlisten = u))
    api
      .startup()
      .then(async (st) => {
        if (st.page) setPage(st.page as Page)
        if (st.select) setLayer(st.select)
        if (st.at != null) setFrame(st.at)
        const s = await api.snapshot()
        if (s) setSnapshot(s)
      })
      .catch(() => {})
    return () => unlisten?.()
  }, [])

  // The renderer's adapter and timings, for the status line.
  React.useEffect(() => {
    if (!isTauri()) return
    const tick = () =>
      api
        .renderStatus()
        .then(setRenderStatus)
        .catch(() => {})
    tick()
    const id = window.setInterval(tick, 2000)
    return () => window.clearInterval(id)
  }, [])

  // Keep the playhead and the selection inside the project as it changes.
  React.useEffect(() => {
    if (!info) return
    setFrame((f) => Math.min(f, lastFrame(info)))
    if (layer && snapshot && !snapshot.layers.some((l) => l.id === layer)) {
      setLayer(null)
      setPicked(null)
    }
  }, [info, layer, snapshot])

  // ---- playback --------------------------------------------------------------------------
  const audio = React.useRef<HTMLAudioElement | null>(null)
  const audioPath = info?.audio?.exists ? info.audio.path : null
  React.useEffect(() => {
    audio.current?.pause()
    audio.current = null
    if (!audioPath || !isTauri()) {
      setAudioState(info?.audio ? "audio file missing" : "no audio")
      return
    }
    const el = new Audio(convertFileSrc(audioPath))
    el.preload = "auto"
    el.addEventListener("canplaythrough", () => setAudioState("audio ready"))
    el.addEventListener("error", () =>
      setAudioState("audio silent: the webview cannot decode the file")
    )
    audio.current = el
    setAudioState("loading audio")
    return () => el.pause()
  }, [audioPath, info?.audio])

  const syncAudio = React.useCallback((f: number, play: boolean) => {
    const el = audio.current
    const i = infoRef.current
    if (!el || !i?.audio) return
    el.volume = Math.max(0, Math.min(1, i.audio.gain ?? 1))
    const want = i.audio.offset + f / i.fps
    if (Math.abs(el.currentTime - want) > 0.08)
      el.currentTime = Math.max(0, want)
    if (play) {
      el.play().catch((e) => setAudioState(`audio silent: ${errorText(e)}`))
    } else {
      el.pause()
    }
  }, [])

  const clockRef = React.useRef<{ start: number; frame: number } | null>(null)
  const frameRef = React.useRef(frame)
  const loopingRef = React.useRef(looping)
  React.useLayoutEffect(() => {
    frameRef.current = frame
    loopingRef.current = looping
  })

  const setPlaying = React.useCallback(
    (on: boolean) => {
      setPlayingState(on)
      if (on) {
        const i = infoRef.current
        let f = frameRef.current
        if (i && f >= lastFrame(i)) f = 0
        setFrame(f)
        clockRef.current = { start: performance.now(), frame: f }
        syncAudio(f, true)
      } else {
        clockRef.current = null
        syncAudio(frameRef.current, false)
        clearPrefetch()
      }
    },
    [syncAudio]
  )

  React.useEffect(() => {
    if (!playing) return
    let raf = 0
    const step = () => {
      const i = infoRef.current
      const c = clockRef.current
      if (i && c) {
        let f =
          c.frame + Math.floor(((performance.now() - c.start) / 1000) * i.fps)
        if (f > lastFrame(i)) {
          if (loopingRef.current) {
            f = 0
            clockRef.current = { start: performance.now(), frame: 0 }
            syncAudio(0, true)
          } else {
            f = lastFrame(i)
            setFrame(f)
            setPlaying(false)
            return
          }
        }
        if (f !== frameRef.current) setFrame(f)
      }
      raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
    return () => cancelAnimationFrame(raf)
  }, [playing, setPlaying, syncAudio])

  const seek = React.useCallback(
    (f: number) => {
      const i = infoRef.current
      const clamped = i ? Math.max(0, Math.min(Math.round(f), lastFrame(i))) : 0
      setFrame(clamped)
      if (clockRef.current) {
        clockRef.current = { start: performance.now(), frame: clamped }
        clearPrefetch()
        syncAudio(clamped, true)
      }
    },
    [syncAudio]
  )

  const togglePlay = React.useCallback(
    () => setPlaying(!clockRef.current),
    [setPlaying]
  )

  const select = React.useCallback((id: string | null, cam = false) => {
    setLayer(id)
    setCamera(cam && id == null)
    setPicked(null)
  }, [])

  const setNote = React.useCallback((text: string) => {
    setNoteText(text)
    setNoteAt(text.trim() ? stamp() : null)
  }, [])

  // ---- edits -------------------------------------------------------------------------------
  const run = React.useCallback(async (what: Promise<EditDone>) => {
    try {
      const done = await what
      setLastAction(done.summary)
      if (done.warnings.length)
        toast.warning("Applied with warnings", {
          description: done.warnings.slice(0, 3).join("\n"),
        })
      return done
    } catch (e) {
      const text = errorText(e)
      toast.error("Not applied", { description: text })
      setLastAction(`not applied: ${text}`)
      return null
    }
  }, [])

  const undo = React.useCallback(() => void run(api.undo()), [run])
  const redo = React.useCallback(() => void run(api.redo()), [run])
  React.useLayoutEffect(() => {
    undoRef.current = undo
  })

  const openProject = React.useCallback(async (path: string) => {
    try {
      const s = await api.openProject(path)
      setSnapshot(s)
      setFrame(0)
      setLayer(null)
      setPicked(null)
    } catch (e) {
      toast.error("Cannot open the project", { description: errorText(e) })
    }
  }, [])

  // ---- what the agent sees: ui.json ------------------------------------------------------------
  React.useEffect(() => {
    if (!info || !isTauri()) return
    const seconds = toSeconds(info, t)
    api
      .uiReport({
        t,
        seconds,
        frame,
        playing,
        layer,
        camera,
        property: picked?.pointer ?? null,
        effect: picked?.effect ?? null,
        key: picked?.keyIndex ?? null,
        visible,
        looping: looping ? [info.start_time, info.duration] : null,
        note: note.trim() ? note : null,
        note_at: noteAt,
      })
      .catch(() => {})
    // While playing the Rust side writes at most every 2 s; reporting every frame is cheap.
  }, [
    info,
    t,
    frame,
    playing,
    layer,
    camera,
    picked,
    visible,
    looping,
    note,
    noteAt,
  ])

  const value: Studio = {
    snapshot,
    info,
    page,
    setPage,
    layer,
    camera,
    select,
    picked,
    setPicked,
    frame,
    t,
    seek,
    playing,
    togglePlay,
    setPlaying,
    looping,
    setLooping,
    scale,
    setScale,
    blurPaused,
    setBlurPaused,
    visible,
    setVisible,
    note,
    noteAt,
    setNote,
    renderStatus,
    audioState,
    lastAction,
    paletteOpen,
    setPaletteOpen,
    run,
    undo,
    redo,
    openProject,
  }
  return (
    <StudioContext.Provider value={value}>{children}</StudioContext.Provider>
  )
}
