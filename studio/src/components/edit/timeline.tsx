import * as React from "react"
import {
  BoxIcon,
  EyeIcon,
  EyeOffIcon,
  FlagIcon,
  ImageIcon,
  PlusIcon,
  SquareIcon,
  WandSparklesIcon,
  ZoomInIcon,
  ZoomOutIcon,
  ScanIcon,
} from "lucide-react"

import { AgentRowGlow, AgentTimeMarker } from "@/agent/presence-ui"
import { KindIcon } from "@/components/edit/kind-icon"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { api, type Mark, type Row } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { cn } from "@/lib/utils"
import { fmtTime, frameAt, frameTime, tidy } from "@/lib/time"

const HEADER = 184
const ROW = 36
const RULER = 28
const MARKERS = 22

type Drag = {
  id: string
  kind: "move" | "trim_in" | "trim_out" | "keys"
  grab: number
  now: number
  from: number | null
  alt: boolean
}

function snap(
  t: number,
  beats: boolean,
  alt: boolean,
  toFrame: (t: number) => number
) {
  if (beats && !alt) return Math.round(t * 4) / 4
  return toFrame(t)
}

function KeyGlyph({
  mark,
  x,
  onPointerDown,
}: {
  mark: Mark
  x: number
  onPointerDown: (e: React.PointerEvent) => void
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            onPointerDown={onPointerDown}
            className="absolute top-1/2 z-10 flex size-3 -translate-x-1/2 -translate-y-1/2 cursor-ew-resize items-center justify-center"
            style={{ left: x }}
          />
        }
      >
        <span
          className={cn(
            "block bg-foreground",
            mark.shape === "linear" && "size-2 rotate-45 rounded-[1px]",
            mark.shape === "hold" && "size-2 rounded-[1px]",
            mark.shape === "eased" && "size-2 rounded-full",
            mark.effect && "bg-muted-foreground"
          )}
        />
      </TooltipTrigger>
      <TooltipContent>
        {tidy(mark.t, 3)} · {mark.what.join(", ")} · {mark.shape}
      </TooltipContent>
    </Tooltip>
  )
}

export function Timeline() {
  const s = useStudio()
  const { snapshot, info } = s
  const scroller = React.useRef<HTMLDivElement>(null)
  const [width, setWidth] = React.useState(800)
  const [zoom, setZoom] = React.useState(1)
  const [drag, setDrag] = React.useState<Drag | null>(null)

  React.useEffect(() => {
    const el = scroller.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  if (!snapshot || !info) return null
  const beats = info.time_base === "beats"
  const t0 = Math.min(0, info.start_time)
  const t1 = Math.max(info.duration, info.end_time)
  const avail = Math.max(200, width - HEADER - 12)
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
          : beat && ppu >= 40
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

  // ---- input -------------------------------------------------------------------------------------
  const timeOf = (e: React.PointerEvent | PointerEvent) => {
    const el = scroller.current!
    const r = el.getBoundingClientRect()
    return tAt(e.clientX - r.left + el.scrollLeft - HEADER)
  }

  const scrub = (e: React.PointerEvent) => {
    if (e.button !== 0) return
    const move = (ev: PointerEvent | React.PointerEvent) =>
      s.seek(frameAt(info, timeOf(ev)))
    move(e)
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }

  const startDrag = (
    e: React.PointerEvent,
    row: Row,
    kind: Drag["kind"],
    from: number | null
  ) => {
    if (e.button !== 0 || row.index == null) return
    e.stopPropagation()
    s.select(row.id)
    const grab = timeOf(e)
    let current: Drag = {
      id: row.id,
      kind,
      grab,
      now: grab,
      from,
      alt: e.altKey,
    }
    let moved = false
    setDrag(current)
    const move = (ev: PointerEvent) => {
      const now = timeOf(ev)
      if (Math.abs(now - grab) * ppu > 2) moved = true
      current = { ...current, now, alt: ev.altKey }
      setDrag(current)
    }
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      setDrag(null)
      if (moved)
        void s.run(
          api.layerDrag(
            current.id,
            current.kind,
            current.from,
            current.grab,
            current.now,
            current.alt
          )
        )
      else if (kind === "keys" && from != null) s.seek(frameAt(info, from))
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }

  const barDown = (e: React.PointerEvent<HTMLDivElement>, row: Row) => {
    const r = e.currentTarget.getBoundingClientRect()
    const edge = Math.min(8, r.width / 4)
    const kind =
      e.clientX - r.left < edge
        ? "trim_in"
        : r.right - e.clientX < edge
          ? "trim_out"
          : "move"
    startDrag(e, row, kind, null)
  }

  /** Where a dragged bar goes, as the Rust side will snap it. */
  const ghost = (row: Row) => {
    if (!drag || drag.id !== row.id) return null
    const d = drag.now - drag.grab
    const sn = (t: number) => snap(t, beats, drag.alt, toFrameTime)
    if (drag.kind === "move") {
      const st = sn(row.start + d)
      return { start: st, end: row.end + (st - row.start) }
    }
    if (drag.kind === "trim_in")
      return {
        start: Math.min(sn(row.start + d), row.end - 1 / info.fps),
        end: row.end,
      }
    if (drag.kind === "trim_out")
      return {
        start: row.start,
        end: Math.max(sn(row.end + d), row.start + 1 / info.fps),
      }
    return null
  }

  const slots = snapshot.slots
  const add = (kind: string, slot: string | null) =>
    void s.run(api.layerAdd(kind, slot, s.t, s.layer))

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="outline" size="sm" />}>
            <PlusIcon data-icon="inline-start" />
            Layer
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-56">
            <DropdownMenuGroup>
              <DropdownMenuLabel>Add at {fmtTime(info, s.t)}</DropdownMenuLabel>
              <DropdownMenuItem onClick={() => add("solid", null)}>
                <SquareIcon />
                Solid
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => add("adjustment", null)}>
                <WandSparklesIcon />
                Adjustment layer
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuSub>
              <DropdownMenuSubTrigger disabled={!slots.length}>
                <ImageIcon />
                Image from slot
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
            <DropdownMenuSub>
              <DropdownMenuSubTrigger disabled={!slots.length}>
                <BoxIcon />
                Cube with slot
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
        <Tooltip>
          <TooltipTrigger render={<Badge variant="outline" />}>
            {beats ? "snap ¼ beat" : "snap to frames"}
          </TooltipTrigger>
          <TooltipContent>
            Drag a bar to move a layer with its keys, its edges to trim, a key
            to move it. Hold Alt to snap to frames.
          </TooltipContent>
        </Tooltip>
        <span className="text-sm text-muted-foreground tabular-nums">
          {snapshot.layers.length} layers · {tidy(info.duration, 2)}{" "}
          {beats ? "beats" : "s"}
          {info.bpm ? ` · ${tidy(info.bpm, 2)} BPM` : ""} · {info.fps} fps
        </span>
        <div className="ml-auto flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setZoom((z) => Math.max(1, z / 1.5))}
            aria-label="Zoom out"
          >
            <ZoomOutIcon />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setZoom((z) => Math.min(64, z * 1.5))}
            aria-label="Zoom in"
          >
            <ZoomInIcon />
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setZoom(1)}>
            <ScanIcon data-icon="inline-start" />
            Fit
          </Button>
        </div>
      </div>
      <div
        ref={scroller}
        className="relative min-h-0 flex-1 overflow-auto"
        onWheel={(e) => {
          if (e.ctrlKey) {
            e.preventDefault()
            setZoom((z) =>
              Math.min(64, Math.max(1, z * (e.deltaY < 0 ? 1.2 : 1 / 1.2)))
            )
          }
        }}
      >
        <div
          className="relative"
          style={{ width: HEADER + contentW + 12, minHeight: "100%" }}
        >
          {/* ruler */}
          <div
            className="sticky top-0 z-30 flex border-b bg-background"
            style={{ height: RULER }}
          >
            <div
              className="sticky left-0 z-40 flex shrink-0 items-center border-r bg-background px-3 text-xs font-medium text-muted-foreground"
              style={{ width: HEADER }}
            >
              {beats ? "Bars" : "Seconds"}
            </div>
            <div
              className="relative cursor-pointer"
              style={{ width: contentW }}
              onPointerDown={scrub}
            >
              {ticks.map((k) => (
                <div
                  key={k.t}
                  className={cn(
                    "absolute bottom-0 border-l",
                    k.major
                      ? "h-full border-foreground/25"
                      : "h-2 border-border"
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
            </div>
          </div>
          {/* markers */}
          <div className="flex border-b" style={{ height: MARKERS }}>
            <div
              className="sticky left-0 z-20 flex shrink-0 items-center border-r bg-background px-3 text-xs text-muted-foreground"
              style={{ width: HEADER }}
            >
              Markers
            </div>
            <div
              className="relative cursor-pointer"
              style={{ width: contentW }}
              onPointerDown={scrub}
            >
              {snapshot.markers.map((m) => (
                <div
                  key={`${m.t}${m.label}`}
                  className="absolute top-0 flex h-full items-center gap-1 border-l border-foreground/40 pl-1 text-[11px] text-muted-foreground"
                  style={{ left: x(m.t) }}
                >
                  <FlagIcon className="size-3" />
                  {m.label}
                </div>
              ))}
            </div>
          </div>
          {/* tracks */}
          {snapshot.rows.map((row) => {
            const selected = row.index != null ? s.layer === row.id : s.camera
            const g = ghost(row)
            return (
              <div
                key={row.id}
                className="group/row flex border-b"
                style={{ height: ROW }}
              >
                <div
                  className={cn(
                    "sticky left-0 z-20 flex shrink-0 cursor-pointer items-center gap-2 border-r bg-background px-3 text-sm",
                    selected && "bg-muted"
                  )}
                  style={{ width: HEADER }}
                  onClick={() =>
                    row.index == null ? s.select(null, true) : s.select(row.id)
                  }
                >
                  <KindIcon
                    kind={row.kind}
                    className={cn(
                      "size-4 shrink-0 text-muted-foreground",
                      selected && "text-foreground"
                    )}
                  />
                  <span
                    className={cn(
                      "max-w-28 shrink-0 truncate font-medium",
                      !row.enabled && "text-muted-foreground line-through"
                    )}
                  >
                    {row.id}
                  </span>
                  <span className="min-w-0 truncate text-xs text-muted-foreground">
                    {row.label.startsWith(row.kind)
                      ? row.label.slice(row.kind.length).trim() || row.kind
                      : row.label}
                  </span>
                  {row.index != null && (
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
                      aria-label={row.enabled ? "Disable" : "Enable"}
                    >
                      {row.enabled ? <EyeIcon /> : <EyeOffIcon />}
                    </Button>
                  )}
                </div>
                <div
                  className="relative"
                  style={{ width: contentW }}
                  onPointerDown={scrub}
                >
                  {ticks
                    .filter((k) => k.major)
                    .map((k) => (
                      <div
                        key={k.t}
                        className="pointer-events-none absolute inset-y-0 border-l border-border/60"
                        style={{ left: x(k.t) }}
                      />
                    ))}
                  <div
                    className={cn(
                      "absolute inset-y-1.5 flex cursor-grab items-center overflow-hidden rounded-md border bg-muted px-2 active:cursor-grabbing",
                      selected && "border-primary/70 bg-accent",
                      !row.enabled && "opacity-40",
                      row.index == null && "cursor-default"
                    )}
                    style={{
                      left: x(row.start),
                      width: Math.max(4, x(row.end) - x(row.start)),
                    }}
                    onPointerDown={(e) =>
                      row.index == null ? s.select(null, true) : barDown(e, row)
                    }
                  >
                    {row.transition && (
                      <div
                        className="pointer-events-none absolute inset-y-0 left-0 bg-primary/25"
                        style={{
                          width: Math.max(2, row.transition[1] * ppu),
                          clipPath: "polygon(0 100%, 100% 0, 100% 100%)",
                        }}
                        title={`transition ${row.transition[0]}`}
                      />
                    )}
                    <span className="relative truncate text-xs text-muted-foreground">
                      {[...(row.blend ? [row.blend] : []), ...row.effects].join(
                        " · "
                      )}
                    </span>
                  </div>
                  {row.index != null && (
                    <AgentRowGlow layer={row.id} left={x(row.start)} />
                  )}
                  {g && (
                    <div
                      className="pointer-events-none absolute inset-y-1 rounded-md border border-dashed border-primary"
                      style={{
                        left: x(g.start),
                        width: Math.max(4, x(g.end) - x(g.start)),
                      }}
                    >
                      <span className="absolute -top-0.5 left-1 text-[10px] text-primary tabular-nums">
                        {tidy(g.start, 3)}–{tidy(g.end, 3)}
                      </span>
                    </div>
                  )}
                  {row.marks.map((m) => {
                    const moving =
                      drag &&
                      drag.id === row.id &&
                      drag.kind === "keys" &&
                      drag.from === m.t
                    const at = moving
                      ? snap(
                          m.t + drag.now - drag.grab,
                          beats,
                          drag.alt,
                          toFrameTime
                        )
                      : m.t
                    return (
                      <KeyGlyph
                        key={m.t}
                        mark={m}
                        x={x(at)}
                        onPointerDown={(e) => startDrag(e, row, "keys", m.t)}
                      />
                    )
                  })}
                </div>
              </div>
            )
          })}
          <AgentTimeMarker x={(t) => HEADER + x(t)} />
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
