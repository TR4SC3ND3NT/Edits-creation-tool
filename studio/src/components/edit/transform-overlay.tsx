import * as React from "react"

import { useEditor } from "@/components/edit/editor-context"
import { useInspection } from "@/components/edit/use-inspection"
import { api, type PropDto, type PropKey } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { tidy } from "@/lib/time"

type Box = { w: number; h: number }

const T = (name: string): PropKey => ({ type: "transform", name })

/** The size of a layer's box in composition units (the frame is 1 high). */
function layerBox(
  kind: string,
  raw: Record<string, unknown> | undefined,
  aspect: number
): Box {
  switch (kind) {
    case "image":
    case "video":
    case "solid":
    case "adjustment":
    case "comp":
      return { w: aspect, h: 1 }
    case "cube": {
      const size = (raw?.size as number[] | undefined) ?? [0.6, 0.6, 0.6]
      return { w: size[0] ?? 0.6, h: size[1] ?? 0.6 }
    }
    case "text": {
      const text = String(raw?.text ?? "")
      const size = Number(raw?.size ?? 10) / 100
      const lines = text.split("\n")
      const longest = Math.max(1, ...lines.map((l) => l.length))
      return {
        w: Math.min(aspect, longest * size * 0.62),
        h: lines.length * size * 1.2,
      }
    }
    default:
      return { w: 0.3, h: 0.3 }
  }
}

function value(p: PropDto | undefined, fallback: number[]) {
  if (!p) return fallback
  return p.base.length ? p.base : p.value
}

type Drag =
  | { kind: "move"; x0: number; y0: number; pos: number[] }
  | { kind: "scale"; d0: number; sc: number[] }
  | { kind: "rotate"; a0: number; rot: number[] }

/**
 * Handles on the selected layer in the viewer: drag inside the box to move
 * it, a corner to scale it, the knob above it to turn it. The change is
 * previewed while dragging and written once on release (`prop_set`: a key
 * at the playhead when the property is keyed, else the constant).
 */
export function TransformOverlay({
  width,
  height,
}: {
  width: number
  height: number
}) {
  const s = useStudio()
  const ed = useEditor()
  const { data } = useInspection()
  const [drag, setDrag] = React.useState<{ d: Drag; now: number[] } | null>(
    null
  )
  const info = s.info
  const l = data?.layer
  if (!info || !l || !s.layer || s.playing || !l.visible_now) return null
  const aspect = info.width / info.height
  const unit = height
  const raw = (
    (s.snapshot?.doc?.layers as Record<string, unknown>[] | undefined) ?? []
  ).find((x) => x.id === l.id)
  const box = layerBox(l.kind, raw, aspect)
  const props = Object.fromEntries(data.transform.map((p) => [p.name, p]))
  let pos = value(props.position, [0, 0, 0])
  let sc = value(props.scale, [1])
  let rot = value(props.rotation, [0, 0, 0])
  const anchor = value(props.anchor, [0, 0])
  if (drag) {
    if (drag.d.kind === "move") pos = drag.now
    if (drag.d.kind === "scale") sc = drag.now
    if (drag.d.kind === "rotate") rot = drag.now
  }
  const sx = sc[0] ?? 1
  const sy = sc.length > 1 ? sc[1] : sx
  const rz = ((rot[2] ?? 0) * Math.PI) / 180
  const toScreen = (qx: number, qy: number) => {
    const x = (qx - anchor[0]) * sx
    const y = (qy - anchor[1]) * sy
    const X = pos[0] + x * Math.cos(rz) - y * Math.sin(rz)
    const Y = pos[1] + x * Math.sin(rz) + y * Math.cos(rz)
    return [width / 2 + X * unit, height / 2 + Y * unit]
  }
  const corners = [
    toScreen(-box.w / 2, -box.h / 2),
    toScreen(box.w / 2, -box.h / 2),
    toScreen(box.w / 2, box.h / 2),
    toScreen(-box.w / 2, box.h / 2),
  ]
  const centre = toScreen(0, 0)
  const top = toScreen(0, -box.h / 2)
  const knob = [top[0] + Math.sin(rz) * 24, top[1] - Math.cos(rz) * 24]

  const local = (e: React.PointerEvent | PointerEvent, el: Element) => {
    const r = el.getBoundingClientRect()
    return [e.clientX - r.left, e.clientY - r.top]
  }
  const start = (e: React.PointerEvent<SVGElement>, kind: Drag["kind"]) => {
    if (e.button !== 0) return
    e.stopPropagation()
    e.preventDefault()
    const svg =
      (e.currentTarget as SVGElement).ownerSVGElement ?? e.currentTarget
    const [px, py] = local(e, svg)
    const id = l.id
    const t = s.t
    let d: Drag
    if (kind === "move") d = { kind, x0: px, y0: py, pos: [...pos] }
    else if (kind === "scale")
      d = {
        kind,
        d0: Math.max(4, Math.hypot(px - centre[0], py - centre[1])),
        sc: [...sc],
      }
    else
      d = {
        kind,
        a0: Math.atan2(py - centre[1], px - centre[0]),
        rot: rot.length >= 3 ? [...rot] : [0, 0, rot[0] ?? 0],
      }
    let now: number[] =
      d.kind === "move"
        ? [...d.pos]
        : d.kind === "scale"
          ? [...d.sc]
          : [...d.rot]
    let moved = false
    const name =
      kind === "move" ? "position" : kind === "scale" ? "scale" : "rotation"
    const move = (ev: PointerEvent) => {
      const [x, y] = local(ev, svg)
      if (d.kind === "move") {
        let nx = d.pos[0] + (x - d.x0) / unit
        let ny = d.pos[1] + (y - d.y0) / unit
        if (ev.shiftKey) {
          if (Math.abs(x - d.x0) > Math.abs(y - d.y0)) ny = d.pos[1]
          else nx = d.pos[0]
        }
        now = [tidy(nx, 4), tidy(ny, 4), ...d.pos.slice(2)]
      } else if (d.kind === "scale") {
        const k = Math.hypot(x - centre[0], y - centre[1]) / d.d0
        now = d.sc.map((v, i) => (i < 2 ? tidy(v * k, 4) : v))
      } else {
        let deg =
          ((Math.atan2(y - centre[1], x - centre[0]) - d.a0) * 180) / Math.PI
        if (ev.shiftKey) deg = Math.round(deg / 15) * 15
        now = [d.rot[0], d.rot[1], tidy(d.rot[2] + deg, 2)]
      }
      moved = true
      setDrag({ d, now })
      ed.previewProp(id, T(name), now)
    }
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      setDrag(null)
      if (moved) void s.run(api.propSet(id, T(name), t, now))
      else ed.endPreview()
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
    setDrag({ d, now })
  }

  const path =
    corners.map((c, i) => `${i ? "L" : "M"}${c[0]},${c[1]}`).join(" ") + "Z"
  return (
    <svg
      className="absolute inset-0 size-full overflow-visible"
      width={width}
      height={height}
      data-slot="transform-overlay"
    >
      <path
        d={path}
        className="cursor-move fill-transparent stroke-primary"
        strokeWidth={1}
        onPointerDown={(e) => start(e, "move")}
      />
      <line
        x1={top[0]}
        y1={top[1]}
        x2={knob[0]}
        y2={knob[1]}
        className="stroke-primary"
        strokeWidth={1}
      />
      <circle
        cx={knob[0]}
        cy={knob[1]}
        r={5}
        className="cursor-grab fill-background stroke-primary"
        onPointerDown={(e) => start(e, "rotate")}
      >
        <title>Rotate (Shift: 15° steps)</title>
      </circle>
      <circle
        cx={centre[0]}
        cy={centre[1]}
        r={3}
        className="pointer-events-none fill-primary"
      />
      {corners.map((c, i) => (
        <rect
          key={i}
          x={c[0] - 4.5}
          y={c[1] - 4.5}
          width={9}
          height={9}
          rx={1.5}
          className="cursor-nwse-resize fill-background stroke-primary"
          onPointerDown={(e) => start(e, "scale")}
        >
          <title>Scale</title>
        </rect>
      ))}
      {drag && (
        <text
          x={centre[0] + 8}
          y={centre[1] - 8}
          className="fill-foreground text-[11px] tabular-nums"
        >
          {drag.d.kind === "move"
            ? `${tidy(drag.now[0], 3)}, ${tidy(drag.now[1], 3)}`
            : drag.d.kind === "scale"
              ? `× ${tidy(drag.now[0], 3)}`
              : `${tidy(drag.now[2], 1)}°`}
        </text>
      )}
    </svg>
  )
}
