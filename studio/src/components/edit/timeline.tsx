// The timeline of the Edit page, drawn by the studio with shadcn's tokens:
// a toolbar, a ruler in bars and beats, the song (sections, downbeats,
// beats, kicks and snares from the analysis), its waveform, and one track
// per layer (top of the picture first) with its clip: a strip of its
// picture, its effects as chips, its transition at the start and its keys.
// A track opens into keyframe lanes, one per animated property.
//
// Mouse: click a clip to select it (Shift adds), drag it to move it (with
// its keys; every selected clip moves together), its edges to trim, a key
// to move it; drag on empty track space for a marquee; the ruler scrubs.
// Moves snap to the song's beats, the playhead and other clips' edges
// within 8 px (N or the magnet turns it off: frames then). Nothing is
// written until the mouse is released, and then it is one edit.

import * as React from "react"
import {
  AudioLinesIcon,
  BoxIcon,
  ChevronRightIcon,
  CircleIcon,
  EyeIcon,
  EyeOffIcon,
  FlagIcon,
  ImageIcon,
  KeyboardIcon,
  MagnetIcon,
  MinusIcon,
  PlusIcon,
  ScanIcon,
  ScissorsIcon,
  SparklesIcon,
  SquareIcon,
  StarIcon,
  Trash2Icon,
  TypeIcon,
  WandSparklesIcon,
  WavesIcon,
  ZapIcon,
  ZoomInIcon,
  ZoomOutIcon,
} from "lucide-react"
import { convertFileSrc } from "@tauri-apps/api/core"

import {
  DRAG_MIME,
  useEditor,
  type BrowserItem,
} from "@/components/edit/editor-context"
import { KindIcon } from "@/components/edit/kind-icon"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Separator } from "@/components/ui/separator"
import { Spinner } from "@/components/ui/spinner"
import { Toggle } from "@/components/ui/toggle"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { api, type Lane, type Row } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { cn } from "@/lib/utils"
import { fmtTime, frameAt, frameTime, tidy, toSeconds } from "@/lib/time"

const HEADER = 196
const RULER = 26
const SONG = 22
const WAVE = 34
const ROW = 46
const LANE = 22
const SNAP_PX = 8

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Raw = Record<string, any>

type Drag =
  | {
      kind: "move" | "trim_in" | "trim_out"
      id: string
      grab: number
      now: number
      snapped: number | null
    }
  | {
      kind: "keys"
      id: string
      from: number
      grab: number
      now: number
      snapped: number | null
    }
  | {
      kind: "lane"
      id: string | null
      lane: Lane
      from: number
      grab: number
      now: number
      snapped: number | null
    }

type Marquee = { x0: number; y0: number; x1: number; y1: number; add: boolean }

function Tip({
  tip,
  children,
}: {
  tip: React.ReactNode
  children: React.ReactElement
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipContent>{tip}</TooltipContent>
    </Tooltip>
  )
}

function Diamond({
  shape,
  x,
  selected,
  muted,
  onPointerDown,
  title,
}: {
  shape: "linear" | "hold" | "eased"
  x: number
  selected?: boolean
  muted?: boolean
  onPointerDown: (e: React.PointerEvent) => void
  title: string
}) {
  return (
    <button
      type="button"
      data-slot="key"
      title={title}
      onPointerDown={onPointerDown}
      className="absolute top-1/2 z-10 flex size-4 -translate-x-1/2 -translate-y-1/2 cursor-ew-resize items-center justify-center"
      style={{ left: x }}
    >
      <span
        className={cn(
          "block size-2 border border-background",
          shape === "linear" && "rotate-45 rounded-[1px]",
          shape === "hold" && "rounded-[1px]",
          shape === "eased" && "rounded-full",
          muted ? "bg-muted-foreground" : "bg-foreground",
          selected && "bg-primary ring-2 ring-ring"
        )}
      />
    </button>
  )
}

/** The strip of pictures along a clip. */
function ClipStrip({
  raw,
  slots,
}: {
  raw: Raw | undefined
  slots: { name: string; file: string | null; exists: boolean }[]
}) {
  if (!raw) return null
  const slotName: string | undefined =
    raw.kind === "image" || raw.kind === "video"
      ? raw.slot
      : raw.kind === "cube"
        ? raw.faces?.[0]
        : undefined
  const slot = slots.find((x) => x.name === slotName)
  if (slot?.file && slot.exists && raw.kind !== "video")
    return (
      <div
        className="pointer-events-none absolute inset-0 opacity-60"
        style={{
          backgroundImage: `url("${convertFileSrc(slot.file)}")`,
          backgroundSize: "auto 100%",
          backgroundRepeat: "repeat-x",
        }}
      />
    )
  if (raw.kind === "solid" && Array.isArray(raw.color))
    return (
      <div
        className="pointer-events-none absolute inset-y-0 left-0 w-3 border-r"
        style={{
          background: `rgb(${raw.color
            .slice(0, 3)
            .map((c: number) => Math.round(c * 255))
            .join(",")})`,
        }}
      />
    )
  return null
}

/** One track of the timeline: the header and the clip of a layer (`data-slot="timeline-row"`). */
function TrackRow({
  row,
  raw,
  x,
  ppu,
  contentW,
  selected,
  primary,
  expanded,
  onExpand,
  ghost,
  dropping,
  children,
  onClipDown,
  onTrackDown,
  onDrop,
  onDragState,
  keyGlyphs,
}: {
  row: Row
  raw: Raw | undefined
  x: (t: number) => number
  ppu: number
  contentW: number
  selected: boolean
  primary: boolean
  expanded: boolean
  onExpand: () => void
  ghost: { start: number; end: number } | null
  dropping: boolean
  children?: React.ReactNode
  onClipDown: (e: React.PointerEvent<HTMLDivElement>) => void
  onTrackDown: (e: React.PointerEvent<HTMLDivElement>) => void
  onDrop: (e: React.DragEvent, item: BrowserItem) => void
  onDragState: (over: boolean) => void
  keyGlyphs: React.ReactNode
}) {
  const s = useStudio()
  const camera = row.index == null
  const name = row.label.startsWith(row.kind)
    ? row.label.slice(row.kind.length).trim()
    : row.label
  const slots = s.snapshot?.slots ?? []
  const w = Math.max(6, x(row.end) - x(row.start))
  return (
    <div
      data-slot="timeline-row"
      data-layer={row.id}
      className="group/row flex border-b"
      style={{ height: ROW }}
    >
      <div
        className={cn(
          "sticky left-0 z-20 flex shrink-0 cursor-pointer items-center gap-1.5 border-r bg-background pr-1.5 pl-1 text-sm",
          selected && "bg-muted"
        )}
        style={{ width: HEADER }}
        onClick={(e) =>
          camera
            ? s.select(null, true)
            : onClipDown(e as unknown as React.PointerEvent<HTMLDivElement>)
        }
      >
        <Button
          variant="ghost"
          size="icon-xs"
          className={cn("shrink-0", !row.lanes.length && "invisible")}
          aria-label={expanded ? "Hide keyframe lanes" : "Show keyframe lanes"}
          onClick={(e) => {
            e.stopPropagation()
            onExpand()
          }}
        >
          <ChevronRightIcon
            className={cn("transition-transform", expanded && "rotate-90")}
          />
        </Button>
        <KindIcon
          kind={row.kind}
          className={cn(
            "size-4 shrink-0 text-muted-foreground",
            selected && "text-foreground"
          )}
        />
        <div className="flex min-w-0 flex-col leading-tight">
          <span
            className={cn(
              "truncate text-[13px] font-medium",
              !row.enabled && "text-muted-foreground line-through"
            )}
          >
            {row.id}
          </span>
          <span className="truncate text-[11px] text-muted-foreground">
            {name || row.kind}
          </span>
        </div>
        {!camera && (
          <Button
            variant="ghost"
            size="icon-xs"
            className={cn(
              "ml-auto shrink-0",
              row.enabled && "opacity-0 group-hover/row:opacity-100"
            )}
            onClick={(e) => {
              e.stopPropagation()
              void s.run(api.layerToggle(row.id))
            }}
            aria-label={row.enabled ? "Turn off" : "Turn on"}
          >
            {row.enabled ? <EyeIcon /> : <EyeOffIcon />}
          </Button>
        )}
      </div>
      <div
        className="relative"
        style={{ width: contentW }}
        onPointerDown={onTrackDown}
        onDragOver={(e) => {
          if (camera || !e.dataTransfer.types.includes(DRAG_MIME)) return
          e.preventDefault()
          e.dataTransfer.dropEffect = "copy"
          onDragState(true)
        }}
        onDragLeave={() => onDragState(false)}
        onDrop={(e) => {
          onDragState(false)
          const text = e.dataTransfer.getData(DRAG_MIME)
          if (!text) return
          e.preventDefault()
          onDrop(e, JSON.parse(text) as BrowserItem)
        }}
      >
        {children}
        <div
          data-slot="clip"
          className={cn(
            "absolute inset-y-1 flex cursor-grab flex-col justify-between overflow-hidden rounded-md border bg-muted active:cursor-grabbing",
            selected && "border-primary",
            primary && "ring-1 ring-primary",
            dropping && "ring-2 ring-ring",
            !row.enabled && "opacity-40",
            camera && "cursor-default"
          )}
          style={{ left: x(row.start), width: w }}
          onPointerDown={onClipDown}
        >
          <ClipStrip raw={raw} slots={slots} />
          {row.transition && (
            <div
              className="pointer-events-none absolute inset-y-0 left-0 border-r border-dashed border-primary/60 bg-primary/20"
              style={{ width: Math.max(3, row.transition[1] * ppu) }}
            />
          )}
          <div className="relative flex min-w-0 items-center gap-1 px-1.5 pt-0.5">
            {row.transition && (
              <span className="shrink-0 rounded-sm bg-background/80 px-1 text-[10px] text-foreground">
                ⇢ {row.transition[0].replaceAll("_", " ")}
              </span>
            )}
            {raw?.kind === "text" && (
              <span className="truncate rounded-sm bg-background/80 px-1 text-[11px]">
                “{String(raw.text ?? "").replace(/\n/g, " ")}”
              </span>
            )}
          </div>
          <div className="relative flex min-w-0 items-center gap-0.5 overflow-hidden px-1 pb-0.5">
            {row.effects.map((fx, i) => (
              <span
                key={i}
                data-slot="effect-chip"
                className={cn(
                  "shrink-0 rounded-sm border bg-background/85 px-1 text-[10px] leading-4",
                  fx.endsWith("(off)") && "text-muted-foreground line-through"
                )}
              >
                {fx.replace(" (off)", "").replaceAll("_", " ")}
              </span>
            ))}
          </div>
          {!expanded && keyGlyphs}
        </div>
        {ghost && (
          <div
            className="pointer-events-none absolute inset-y-0.5 z-20 rounded-md border border-dashed border-primary"
            style={{
              left: x(ghost.start),
              width: Math.max(4, x(ghost.end) - x(ghost.start)),
            }}
          >
            <span className="absolute -top-0.5 left-1 rounded-sm bg-background px-0.5 text-[10px] text-primary tabular-nums">
              {tidy(ghost.start, 3)}–{tidy(ghost.end, 3)}
            </span>
          </div>
        )}
      </div>
    </div>
  )
}

/** The timeline (`data-slot="timeline"`). */
export function Timeline() {
  const s = useStudio()
  const ed = useEditor()
  const { snapshot, info } = s
  const scroller = React.useRef<HTMLDivElement>(null)
  const [width, setWidth] = React.useState(800)
  const [zoom, setZoom] = React.useState(1)
  const [drag, setDrag] = React.useState<Drag | null>(null)
  const [marquee, setMarquee] = React.useState<Marquee | null>(null)
  const [expanded, setExpanded] = React.useState<Set<string>>(new Set())
  const [dropRow, setDropRow] = React.useState<string | null>(null)
  const [scrollLeft, setScrollLeft] = React.useState(0)

  React.useEffect(() => {
    const el = scroller.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Ctrl + wheel zooms about the pointer (a native listener: React's wheel events are passive).
  const zoomRef = React.useRef({ zoom, ppu: 1, t0: 0 })
  React.useEffect(() => {
    const el = scroller.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return
      e.preventDefault()
      const { zoom, ppu, t0 } = zoomRef.current
      const r = el.getBoundingClientRect()
      const px = e.clientX - r.left - HEADER
      const tUnder = t0 + (px + el.scrollLeft) / ppu
      const z = Math.min(64, Math.max(1, zoom * (e.deltaY < 0 ? 1.2 : 1 / 1.2)))
      setZoom(z)
      requestAnimationFrame(() => {
        const nppu = (ppu / zoom) * z
        el.scrollLeft = Math.max(0, (tUnder - t0) * nppu - px)
      })
    }
    el.addEventListener("wheel", onWheel, { passive: false })
    return () => el.removeEventListener("wheel", onWheel)
  }, [])

  const t0r = info ? Math.min(0, info.start_time) : 0
  const t1r = info ? Math.max(info.duration, info.end_time) : 1
  const ppuNow = (Math.max(200, width - HEADER - 16) / (t1r - t0r)) * zoom
  React.useLayoutEffect(() => {
    zoomRef.current = { zoom, ppu: ppuNow, t0: t0r }
  })

  if (!snapshot || !info) return null
  const beats = info.time_base === "beats"
  const t0 = Math.min(0, info.start_time)
  const t1 = Math.max(info.duration, info.end_time)
  const avail = Math.max(200, width - HEADER - 16)
  const ppu = (avail / (t1 - t0)) * zoom
  const contentW = (t1 - t0) * ppu
  const x = (t: number) => (t - t0) * ppu
  const tAt = (px: number) => t0 + px / ppu
  const toFrameTime = (t: number) =>
    frameTime(
      info,
      frameAt(
        info,
        t + 0.5 / info.fps / (beats && info.bpm ? 60 / info.bpm : 1)
      )
    )
  const marks = ed.song?.marks ?? null
  const rawLayers = (snapshot.doc?.layers as Raw[] | undefined) ?? []
  const rawOf = (id: string) => rawLayers.find((l) => l.id === id)

  // ---- snapping ------------------------------------------------------------------------------
  /** Where `t` snaps: (time, on the quarter-beat grid). */
  const snapT = (
    t: number,
    exclude: string | null
  ): { t: number; grid: boolean } => {
    if (!ed.snap) return { t: toFrameTime(t), grid: false }
    let best: { t: number; d: number; grid: boolean } | null = null
    const consider = (c: number, grid: boolean) => {
      const d = Math.abs(c - t) * ppu
      if (d <= SNAP_PX && (!best || d < best.d)) best = { t: c, d, grid }
    }
    for (const r of snapshot.rows) {
      if (r.id === exclude || r.index == null) continue
      consider(r.start, false)
      consider(r.end, false)
    }
    consider(s.t, false)
    for (const m of snapshot.markers) consider(m.t, false)
    if (marks) for (const b of marks.beats) consider(b, false)
    if (beats) consider(Math.round(t), true)
    const b = best as { t: number; d: number; grid: boolean } | null
    if (b) {
      const onGrid = beats && Math.abs(b.t * 4 - Math.round(b.t * 4)) < 1e-6
      return { t: b.t, grid: onGrid || b.grid }
    }
    return beats
      ? { t: Math.round(t * 4) / 4, grid: true }
      : { t: toFrameTime(t), grid: false }
  }

  // ---- the grid ------------------------------------------------------------------------------
  const ticks: { t: number; major: boolean; label: string | null }[] = []
  if (beats) {
    const step = ppu >= 80 ? 0.25 : ppu >= 14 ? 1 : 4
    for (let t = Math.ceil(t0 / step) * step; t <= t1 + 1e-9; t += step) {
      const bar = Math.abs(t / 4 - Math.round(t / 4)) < 1e-9
      const beat = Math.abs(t - Math.round(t)) < 1e-9
      ticks.push({
        t,
        major: bar,
        label: bar
          ? String(Math.round(t / 4) + 1)
          : beat && ppu >= 36
            ? `${Math.floor(t / 4 + 1e-9) + 1}.${Math.round(t - Math.floor(t / 4 + 1e-9) * 4) + 1}`
            : null,
      })
    }
  } else {
    const step = ppu >= 60 ? 0.5 : 1
    for (let t = Math.ceil(t0 / step) * step; t <= t1 + 1e-9; t += step) {
      const whole = Math.abs(t - Math.round(t)) < 1e-9
      ticks.push({ t, major: whole, label: whole ? `${Math.round(t)}s` : null })
    }
  }

  // ---- input ---------------------------------------------------------------------------------
  const pointT = (e: { clientX: number }) => {
    const el = scroller.current!
    const r = el.getBoundingClientRect()
    return tAt(e.clientX - r.left + el.scrollLeft - HEADER)
  }

  const scrub = (e: React.PointerEvent) => {
    if (e.button !== 0) return
    const move = (ev: { clientX: number }) => s.seek(frameAt(info, pointT(ev)))
    move(e)
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }

  const follow = (start: Drag, release: (d: Drag, moved: boolean) => void) => {
    let current = start
    let moved = false
    setDrag(current)
    const move = (ev: PointerEvent) => {
      const now = pointT(ev)
      if (Math.abs(now - current.grab) * ppu > 3) moved = true
      current = { ...current, now } as Drag
      setDrag(current)
    }
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      setDrag(null)
      release(current, moved)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }

  /** Where a dragged clip goes, snapped. */
  const span = (row: Row, d: Drag) => {
    const dt = d.now - d.grab
    if (d.kind === "move") {
      const a = snapT(row.start + dt, row.id)
      const b = snapT(row.end + dt, row.id)
      const useEnd =
        Math.abs(b.t - (row.end + dt)) < Math.abs(a.t - (row.start + dt))
      const start = useEnd ? b.t - (row.end - row.start) : a.t
      return {
        start,
        end: start + (row.end - row.start),
        grid: useEnd ? b.grid : a.grid,
      }
    }
    if (d.kind === "trim_in") {
      const a = snapT(row.start + dt, row.id)
      return {
        start: Math.min(a.t, row.end - 1 / info.fps),
        end: row.end,
        grid: a.grid,
      }
    }
    if (d.kind === "trim_out") {
      const b = snapT(row.end + dt, row.id)
      return {
        start: row.start,
        end: Math.max(b.t, row.start + 1 / info.fps),
        grid: b.grid,
      }
    }
    return null
  }

  const clipDown = (e: React.PointerEvent<HTMLDivElement>, row: Row) => {
    if (e.button !== 0) return
    e.stopPropagation()
    if (row.index == null) {
      s.select(null, true)
      return
    }
    const add = e.shiftKey || e.ctrlKey || e.metaKey
    const already = ed.selected.includes(row.id)
    if (add) {
      ed.pick(row.id, true)
      return
    }
    if (!already) ed.pick(row.id)
    else if (s.layer !== row.id) s.select(row.id)
    const el = e.currentTarget as HTMLElement
    const r = el.getBoundingClientRect()
    const edge = Math.min(8, r.width / 4)
    const inHeader = !el.closest("[data-slot=clip]")
    if (inHeader) return
    const kind =
      e.clientX - r.left < edge
        ? "trim_in"
        : r.right - e.clientX < edge
          ? "trim_out"
          : "move"
    const grab = pointT(e)
    const group =
      already && ed.selected.length > 1 && kind === "move"
        ? [...ed.selected]
        : null
    follow({ kind, id: row.id, grab, now: grab, snapped: null }, (d, moved) => {
      if (!moved) return
      const sp = span(row, d)
      if (!sp) return
      if (group) {
        void s.run(api.layersShift(group, tidy(sp.start - row.start, 6)))
        return
      }
      const ref = kind === "trim_out" ? row.end : row.start
      const target = kind === "trim_out" ? sp.end : sp.start
      // The Rust side snaps again: to quarter beats (grid) or to frames (anything else).
      void s.run(api.layerDrag(row.id, kind, null, ref, target, !sp.grid))
    })
  }

  const keysDown = (e: React.PointerEvent, row: Row, from: number) => {
    if (e.button !== 0 || row.index == null) return
    e.stopPropagation()
    ed.pick(row.id)
    const grab = pointT(e)
    follow(
      { kind: "keys", id: row.id, from, grab, now: grab, snapped: null },
      (d, moved) => {
        if (!moved) {
          s.seek(frameAt(info, from))
          return
        }
        const to = snapT(from + d.now - d.grab, null)
        void s.run(api.layerDrag(row.id, "keys", from, from, to.t, !to.grid))
      }
    )
  }

  const laneDown = (
    e: React.PointerEvent,
    row: Row,
    lane: Lane,
    from: number
  ) => {
    if (e.button !== 0) return
    e.stopPropagation()
    const id = row.index == null ? null : row.id
    ed.setKeyPicked({ layer: id, key: lane.key, t: from })
    const grab = pointT(e)
    follow(
      { kind: "lane", id, lane, from, grab, now: grab, snapped: null },
      (d, moved) => {
        if (!moved) {
          s.seek(frameAt(info, from))
          return
        }
        const to = snapT(from + d.now - d.grab, null)
        void s.run(api.keyMove(id, lane.key, from, tidy(to.t, 6)))
      }
    )
  }

  /** Empty track space: a marquee; a plain click moves the playhead. */
  const trackDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    const el = scroller.current!
    const r = el.getBoundingClientRect()
    const p = (ev: { clientX: number; clientY: number }) => ({
      x: ev.clientX - r.left + el.scrollLeft,
      y: ev.clientY - r.top + el.scrollTop,
    })
    const a = p(e)
    const add = e.shiftKey || e.ctrlKey
    let m: Marquee = { x0: a.x, y0: a.y, x1: a.x, y1: a.y, add }
    let moved = false
    const move = (ev: PointerEvent) => {
      const b = p(ev)
      if (Math.hypot(b.x - a.x, b.y - a.y) > 4) moved = true
      m = { ...m, x1: b.x, y1: b.y }
      if (moved) setMarquee(m)
    }
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      setMarquee(null)
      if (!moved) {
        s.seek(frameAt(info, pointT(e)))
        if (!add) ed.setSelected([])
        return
      }
      // Clips the box touches.
      const left = Math.min(m.x0, m.x1) - HEADER
      const right = Math.max(m.x0, m.x1) - HEADER
      const top = Math.min(m.y0, m.y1)
      const bottom = Math.max(m.y0, m.y1)
      const hit: string[] = []
      for (const [rowId, y0, y1] of rowBands) {
        const row = snapshot.rows.find((x) => x.id === rowId)
        if (!row || row.index == null) continue
        if (y1 < top || y0 > bottom) continue
        if (x(row.end) < left || x(row.start) > right) continue
        hit.push(row.id)
      }
      ed.setSelected(add ? [...new Set([...ed.selected, ...hit])] : hit)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }

  const dropOn = (e: React.DragEvent, row: Row, item: BrowserItem) => {
    if (item.type === "slot") {
      void s.run(
        api.layerAdd("image", item.name, Math.max(0, pointT(e)), row.id)
      )
      return
    }
    void ed.apply(item, row.id)
  }

  const toggleLanes = (id: string) =>
    setExpanded((cur) => {
      const n = new Set(cur)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })

  // ---- rows and their bands (for the marquee) ------------------------------------------------------
  const rowBands: [string, number, number][] = []
  let y = RULER + SONG + (info.audio?.exists ? WAVE : 0)
  for (const row of snapshot.rows) {
    rowBands.push([row.id, y, y + ROW])
    y += ROW + (expanded.has(row.id) ? row.lanes.length * LANE : 0)
  }

  const slots = snapshot.slots
  const add = (kind: string, slot: string | null) =>
    void s.run(api.layerAdd(kind, slot, s.t, s.layer))
  const addNew = (kind: string) => void ed.apply({ type: "layer", name: kind })
  const visibleFrom = tAt(scrollLeft)
  const visibleTo = tAt(scrollLeft + width)
  const inView = (t: number) => t >= visibleFrom - 1 && t <= visibleTo + 1

  // ---- the waveform -------------------------------------------------------------------------------
  const wave = ed.waveform
  let wavePath = ""
  if (wave && info.audio?.exists) {
    const step = 2
    const parts: string[] = []
    const vx0 = Math.max(0, Math.floor(scrollLeft / step) * step - 200)
    const vx1 = Math.min(contentW, vx0 + width + 400)
    const half = (WAVE - 6) / 2
    for (let px = vx0; px <= vx1; px += step) {
      const sec = toSeconds(info, tAt(px)) + info.audio.offset
      const secNext = toSeconds(info, tAt(px + step)) + info.audio.offset
      const i0 = Math.max(0, Math.floor(sec * wave.rate))
      const i1 = Math.max(i0 + 1, Math.floor(secNext * wave.rate))
      let peak = 0
      for (let i = i0; i < i1 && i < wave.peaks.length; i++)
        peak = Math.max(peak, wave.peaks[i])
      if (peak <= 0) continue
      const h = Math.max(0.5, peak * half)
      parts.push(`M${px},${WAVE / 2 - h}V${WAVE / 2 + h}`)
    }
    wavePath = parts.join("")
  }

  const rangeBox = ed.range && (
    <div
      className="pointer-events-none absolute inset-y-0 z-0 border-x border-primary/50 bg-primary/5"
      style={{
        left: x(ed.range[0]),
        width: Math.max(1, x(ed.range[1]) - x(ed.range[0])),
      }}
    />
  )

  const selectedSet = new Set(ed.selected)
  const groupDrag =
    drag &&
    drag.kind === "move" &&
    selectedSet.has(drag.id) &&
    ed.selected.length > 1

  return (
    <div className="flex h-full min-h-0 flex-col" data-slot="timeline">
      {/* toolbar */}
      <div className="flex h-11 shrink-0 items-center gap-1.5 overflow-x-auto border-b px-2">
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="outline" size="sm" />}>
            <PlusIcon data-icon="inline-start" />
            Add
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-56">
            <DropdownMenuGroup>
              <DropdownMenuLabel>Add at {fmtTime(info, s.t)}</DropdownMenuLabel>
              <DropdownMenuSub>
                <DropdownMenuSubTrigger disabled={!slots.length}>
                  <ImageIcon />
                  Picture
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  {slots.map((sl) => (
                    <DropdownMenuItem
                      key={sl.name}
                      onClick={() => add("image", sl.name)}
                    >
                      {sl.name}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
              <DropdownMenuItem onClick={() => addNew("text")}>
                <TypeIcon />
                Text
              </DropdownMenuItem>
              <DropdownMenuSub>
                <DropdownMenuSubTrigger>
                  <StarIcon />
                  Shape
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent>
                  <DropdownMenuItem onClick={() => addNew("shape:rect")}>
                    <SquareIcon />
                    Rectangle
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => addNew("shape:ellipse")}>
                    <CircleIcon />
                    Ellipse
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => addNew("shape:polystar")}>
                    <StarIcon />
                    Star
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => addNew("shape:line")}>
                    <MinusIcon />
                    Line
                  </DropdownMenuItem>
                </DropdownMenuSubContent>
              </DropdownMenuSub>
              <DropdownMenuItem onClick={() => addNew("particles")}>
                <SparklesIcon />
                Particles
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => add("solid", null)}>
              <SquareIcon />
              Solid
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => add("adjustment", null)}>
              <WandSparklesIcon />
              Adjustment layer
            </DropdownMenuItem>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger disabled={!slots.length}>
                <BoxIcon />
                Cube
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent>
                {slots.map((sl) => (
                  <DropdownMenuItem
                    key={sl.name}
                    onClick={() => add("cube", sl.name)}
                  >
                    {sl.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          </DropdownMenuContent>
        </DropdownMenu>
        <ButtonGroup>
          <Tip tip="Split the selected clip at the playhead · S">
            <Button
              variant="outline"
              size="icon-sm"
              onClick={ed.split}
              aria-label="Split"
            >
              <ScissorsIcon />
            </Button>
          </Tip>
          <Tip
            tip={`Delete the selected clips${ed.ripple ? " and close the gap" : ""} · Delete`}
          >
            <Button
              variant="outline"
              size="icon-sm"
              onClick={() => ed.remove()}
              disabled={!ed.selected.length}
              aria-label="Delete"
            >
              <Trash2Icon />
            </Button>
          </Tip>
        </ButtonGroup>
        <Tip tip="Ripple: deleting a clip moves the later clips back to close the gap">
          <Toggle
            variant="outline"
            size="sm"
            pressed={ed.ripple}
            onPressedChange={ed.setRipple}
            className="text-xs"
          >
            Ripple
          </Toggle>
        </Tip>
        <Tip
          tip={
            ed.snap
              ? "Snapping to beats, the playhead and clip edges · N"
              : "Snapping to frames only · N"
          }
        >
          <Toggle
            variant="outline"
            size="sm"
            pressed={ed.snap}
            onPressedChange={ed.setSnap}
            aria-label="Snap"
          >
            <MagnetIcon />
          </Toggle>
        </Tip>
        <Separator orientation="vertical" className="mx-1 h-5" />
        <Tip tip="One picture per beat (or bar, or kick) over a range: edits beats cut">
          <Button
            variant="outline"
            size="sm"
            onClick={() => ed.setCutOpen(true)}
          >
            <ScissorsIcon data-icon="inline-start" />
            Beat cut
          </Button>
        </Tip>
        <Tip tip="Put a preset on every kick, snare, beat or downbeat: edits beats accent">
          <Button
            variant="outline"
            size="sm"
            onClick={() => ed.setAccentOpen(true)}
          >
            <ZapIcon data-icon="inline-start" />
            Accent
          </Button>
        </Tip>
        {ed.range && (
          <Tip tip="In and out points (I / O); X clears them. Beat cut and Accent use this range.">
            <Badge
              variant="secondary"
              className="cursor-pointer tabular-nums"
              onClick={() => ed.setRange(null)}
            >
              {fmtTime(info, ed.range[0])} – {fmtTime(info, ed.range[1])} ✕
            </Badge>
          </Tip>
        )}
        {ed.selected.length > 1 && (
          <Badge variant="outline">{ed.selected.length} selected</Badge>
        )}
        <div className="ml-auto flex items-center gap-0.5">
          <span className="hidden pr-2 text-xs text-muted-foreground tabular-nums xl:inline">
            {snapshot.layers.length} layers ·{" "}
            {info.bpm ? `${tidy(info.bpm, 1)} BPM · ` : ""}
            {info.fps} fps
          </span>
          <Tip tip="Zoom out · Ctrl + wheel">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setZoom((z) => Math.max(1, z / 1.5))}
              aria-label="Zoom out"
            >
              <ZoomOutIcon />
            </Button>
          </Tip>
          <Tip tip="Zoom in · Ctrl + wheel">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setZoom((z) => Math.min(64, z * 1.5))}
              aria-label="Zoom in"
            >
              <ZoomInIcon />
            </Button>
          </Tip>
          <Tip tip="Fit the whole edit">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setZoom(1)}
              aria-label="Fit"
            >
              <ScanIcon />
            </Button>
          </Tip>
          <Tip tip="Keyboard shortcuts · ?">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => ed.setShortcutsOpen(true)}
              aria-label="Keyboard shortcuts"
            >
              <KeyboardIcon />
            </Button>
          </Tip>
        </div>
      </div>
      <div
        ref={scroller}
        className="relative min-h-0 flex-1 overflow-auto"
        onScroll={(e) => setScrollLeft(e.currentTarget.scrollLeft)}
      >
        <div
          className="relative"
          style={{ width: HEADER + contentW + 16, minHeight: "100%" }}
        >
          {/* ruler */}
          <div
            className="sticky top-0 z-30 flex border-b bg-background"
            style={{ height: RULER }}
          >
            <div
              className="sticky left-0 z-40 flex shrink-0 items-center gap-2 border-r bg-background px-3 text-xs text-muted-foreground tabular-nums"
              style={{ width: HEADER }}
            >
              <span className="font-medium text-foreground">
                {fmtTime(info, s.t)}
              </span>
              <span>{beats ? "bar.beat" : "seconds"}</span>
            </div>
            <div
              className="relative cursor-pointer"
              style={{ width: contentW }}
              onPointerDown={scrub}
            >
              {rangeBox}
              {ticks.map((k) => (
                <div
                  key={k.t}
                  className={cn(
                    "absolute bottom-0 border-l",
                    k.major
                      ? "h-full border-foreground/30"
                      : "h-1.5 border-border"
                  )}
                  style={{ left: x(k.t) }}
                >
                  {k.label && (
                    <span className="absolute top-1 left-1 text-[10px] leading-none text-muted-foreground tabular-nums">
                      {k.label}
                    </span>
                  )}
                </div>
              ))}
              {snapshot.markers.map((m) => (
                <div
                  key={`${m.t}${m.label}`}
                  className="absolute bottom-0 flex h-3 items-center gap-0.5 border-l border-foreground/60 pl-0.5 text-[10px] text-foreground"
                  style={{ left: x(m.t) }}
                  title={`marker ${m.label} at ${fmtTime(info, m.t)}`}
                >
                  <FlagIcon className="size-2.5" />
                  {m.label}
                </div>
              ))}
            </div>
          </div>
          {/* song */}
          <div
            className="flex border-b"
            style={{ height: SONG }}
            data-slot="song-lane"
          >
            <div
              className="sticky left-0 z-20 flex shrink-0 items-center gap-1.5 border-r bg-background px-3 text-xs text-muted-foreground"
              style={{ width: HEADER }}
            >
              <AudioLinesIcon className="size-3.5" />
              <span className="truncate">
                {marks ? "Beats" : "Beats (grid)"}
              </span>
              {!marks && info.audio?.exists && (
                <Button
                  variant="ghost"
                  size="xs"
                  className="ml-auto h-5 px-1.5 text-[11px]"
                  onClick={ed.analyze}
                  disabled={ed.song?.running}
                >
                  {ed.song?.running ? <Spinner /> : null}
                  {ed.song?.running ? "Analysing" : "Analyse"}
                </Button>
              )}
            </div>
            <div
              className="relative cursor-pointer overflow-hidden"
              style={{ width: contentW }}
              onPointerDown={scrub}
            >
              {rangeBox}
              {marks?.sections.map(([label, a, b], i) => (
                <div
                  key={`${label}${a}`}
                  className={cn(
                    "absolute inset-y-0 flex items-center border-l border-foreground/40 px-1 text-[10px] text-muted-foreground",
                    i % 2 ? "bg-muted/40" : "bg-muted/15"
                  )}
                  style={{ left: x(a), width: Math.max(0, x(b) - x(a)) }}
                >
                  <span className="truncate">{label}</span>
                </div>
              ))}
              {(marks
                ? marks.beats
                : Array.from({ length: Math.floor(t1) + 1 }, (_, i) => i)
              )
                .filter(inView)
                .map((b) => {
                  const down = marks
                    ? marks.downbeats.some((d) => Math.abs(d - b) < 1e-3)
                    : beats && b % 4 === 0
                  return (
                    <div
                      key={`b${b}`}
                      className={cn(
                        "absolute bottom-0 border-l",
                        down
                          ? "h-full border-foreground/50"
                          : "h-2 border-foreground/25"
                      )}
                      style={{ left: x(b) }}
                    />
                  )
                })}
              {marks?.kicks
                .filter(([t]) => inView(t))
                .map(([t, st]) => (
                  <div
                    key={`k${t}`}
                    className="absolute bottom-0.5 size-1.5 -translate-x-1/2 rounded-full bg-foreground"
                    style={{ left: x(t), opacity: 0.35 + 0.65 * st }}
                    title={`kick ${fmtTime(info, t)}`}
                  />
                ))}
              {marks?.snares
                .filter(([t]) => inView(t))
                .map(([t, st]) => (
                  <div
                    key={`s${t}`}
                    className="absolute top-0.5 size-1.5 -translate-x-1/2 rotate-45 bg-muted-foreground"
                    style={{ left: x(t), opacity: 0.35 + 0.65 * st }}
                    title={`snare ${fmtTime(info, t)}`}
                  />
                ))}
              {marks?.drop != null && (
                <div
                  className="absolute inset-y-0 flex items-center gap-0.5 border-l-2 border-foreground pl-0.5 text-[10px] font-medium"
                  style={{ left: x(marks.drop) }}
                >
                  <FlagIcon className="size-2.5" />
                  drop
                </div>
              )}
            </div>
          </div>
          {/* waveform */}
          {info.audio?.exists && (
            <div
              className="flex border-b"
              style={{ height: WAVE }}
              data-slot="waveform-lane"
            >
              <div
                className="sticky left-0 z-20 flex shrink-0 items-center gap-1.5 border-r bg-background px-3 text-xs text-muted-foreground"
                style={{ width: HEADER }}
              >
                <WavesIcon className="size-3.5" />
                <span className="truncate">
                  {info.audio.path.split("/").pop()}
                </span>
              </div>
              <div
                className="relative cursor-pointer"
                style={{ width: contentW }}
                onPointerDown={scrub}
              >
                {rangeBox}
                {wave ? (
                  <svg
                    className="absolute inset-0"
                    width={contentW}
                    height={WAVE}
                  >
                    <path
                      d={wavePath}
                      className="stroke-muted-foreground"
                      strokeWidth={1.5}
                    />
                  </svg>
                ) : (
                  <span className="absolute inset-0 flex items-center px-2 text-[11px] text-muted-foreground">
                    reading the song…
                  </span>
                )}
              </div>
            </div>
          )}
          {/* tracks */}
          {snapshot.rows.map((row) => {
            const sel = row.index != null ? selectedSet.has(row.id) : s.camera
            const ghost =
              drag &&
              (drag.kind === "move" ||
                drag.kind === "trim_in" ||
                drag.kind === "trim_out")
                ? groupDrag && selectedSet.has(row.id)
                  ? (() => {
                      const lead = snapshot.rows.find((r) => r.id === drag.id)
                      const sp = lead && span(lead, drag)
                      if (!sp || !lead) return null
                      const dt = sp.start - lead.start
                      return { start: row.start + dt, end: row.end + dt }
                    })()
                  : drag.id === row.id
                    ? span(row, drag)
                    : null
                : null
            const isOpen = expanded.has(row.id)
            const glyphs = (
              <div className="pointer-events-none absolute inset-x-0 bottom-0 h-3">
                {row.marks.map((m) => {
                  const moving =
                    drag?.kind === "keys" &&
                    drag.id === row.id &&
                    drag.from === m.t
                  const at = moving
                    ? snapT(m.t + drag.now - drag.grab, null).t
                    : m.t
                  return (
                    <span key={m.t} className="pointer-events-auto">
                      <Diamond
                        shape={m.shape}
                        x={x(at) - x(row.start)}
                        muted={m.effect}
                        title={`${fmtTime(info, m.t)} · ${m.what.join(", ")} · ${m.shape}`}
                        onPointerDown={(e) => keysDown(e, row, m.t)}
                      />
                    </span>
                  )
                })}
              </div>
            )
            return (
              <React.Fragment key={row.id}>
                <TrackRow
                  row={row}
                  raw={rawOf(row.id)}
                  x={x}
                  ppu={ppu}
                  contentW={contentW}
                  selected={sel}
                  primary={row.index != null && s.layer === row.id}
                  expanded={isOpen}
                  onExpand={() => toggleLanes(row.id)}
                  ghost={ghost}
                  dropping={dropRow === row.id}
                  onClipDown={(e) => clipDown(e, row)}
                  onTrackDown={trackDown}
                  onDrop={(e, item) => dropOn(e, row, item)}
                  onDragState={(over) => setDropRow(over ? row.id : null)}
                  keyGlyphs={glyphs}
                >
                  {rangeBox}
                  {ticks
                    .filter((k) => k.major)
                    .map((k) => (
                      <div
                        key={k.t}
                        className="pointer-events-none absolute inset-y-0 border-l border-border/60"
                        style={{ left: x(k.t) }}
                      />
                    ))}
                </TrackRow>
                {isOpen &&
                  row.lanes.map((lane) => (
                    <div
                      key={lane.name}
                      data-slot="keyframe-lane"
                      data-layer={row.id}
                      data-prop={lane.name}
                      className="flex border-b bg-muted/20"
                      style={{ height: LANE }}
                    >
                      <div
                        className="sticky left-0 z-20 flex shrink-0 items-center gap-1.5 border-r bg-background pr-2 pl-9 text-xs text-muted-foreground"
                        style={{ width: HEADER }}
                      >
                        <span className="truncate">
                          {lane.name.replaceAll("_", " ")}
                        </span>
                        <span className="ml-auto tabular-nums">
                          {lane.keys.length}
                        </span>
                      </div>
                      <div
                        className="relative"
                        style={{ width: contentW }}
                        onPointerDown={scrub}
                      >
                        <div
                          className="absolute top-1/2 h-px bg-border"
                          style={{
                            left: x(Math.min(...lane.keys.map((k) => k[0]))),
                            width: Math.max(
                              0,
                              x(Math.max(...lane.keys.map((k) => k[0]))) -
                                x(Math.min(...lane.keys.map((k) => k[0])))
                            ),
                          }}
                        />
                        {lane.keys.map(([t, shape], i) => {
                          const moving =
                            drag?.kind === "lane" &&
                            drag.lane.name === lane.name &&
                            drag.id === (row.index == null ? null : row.id) &&
                            drag.from === t
                          const at = moving
                            ? snapT(t + drag.now - drag.grab, null).t
                            : t
                          const picked =
                            ed.keyPicked &&
                            ed.keyPicked.t === t &&
                            JSON.stringify(ed.keyPicked.key) ===
                              JSON.stringify(lane.key) &&
                            ed.keyPicked.layer ===
                              (row.index == null ? null : row.id)
                          return (
                            <Diamond
                              key={`${t}-${i}`}
                              shape={shape}
                              x={x(at)}
                              selected={!!picked}
                              title={`${lane.name} at ${fmtTime(info, t)} · ${shape} · drag to move, Delete to remove`}
                              onPointerDown={(e) => laneDown(e, row, lane, t)}
                            />
                          )
                        })}
                      </div>
                    </div>
                  ))}
              </React.Fragment>
            )
          })}
          {snapshot.rows.length === 0 && (
            <div
              className="flex h-24 items-center justify-center text-sm text-muted-foreground"
              style={{ marginLeft: HEADER }}
            >
              No layers yet: drag a picture from Media, or use Beat cut.
            </div>
          )}
          {marquee && (
            <div
              className="pointer-events-none absolute z-40 border border-primary bg-primary/10"
              style={{
                left: Math.min(marquee.x0, marquee.x1),
                top: Math.min(marquee.y0, marquee.y1),
                width: Math.abs(marquee.x1 - marquee.x0),
                height: Math.abs(marquee.y1 - marquee.y0),
              }}
            />
          )}
          {/* playhead */}
          <div
            className="pointer-events-none absolute top-0 bottom-0 z-30 w-px bg-primary"
            style={{ left: HEADER + x(s.t) }}
          >
            <div className="absolute -top-px -left-[5px] h-2.5 w-[11px] rounded-b-sm bg-primary" />
          </div>
        </div>
      </div>
    </div>
  )
}
