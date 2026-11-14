import * as React from "react"
import { CircleAlertIcon, Grid3x3Icon, MousePointer2Icon } from "lucide-react"

import { useEditor } from "@/components/edit/editor-context"
import { TransformOverlay } from "@/components/edit/transform-overlay"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Spinner } from "@/components/ui/spinner"
import { Toggle } from "@/components/ui/toggle"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cachedFrame, isPending, loadFrame, nearestCached } from "@/lib/frames"
import { useStudio } from "@/lib/store"
import { lastFrame } from "@/lib/time"
import { cn } from "@/lib/utils"

/** Frames asked for ahead of the playhead while playing. */
const AHEAD = 4

/** Rule of thirds, and the action (90 %) and title (80 %) safe areas. */
function Guides() {
  return (
    <div className="pointer-events-none absolute inset-0" data-slot="guides">
      {[1, 2].map((i) => (
        <React.Fragment key={i}>
          <div
            className="absolute inset-y-0 border-l border-foreground/30"
            style={{ left: `${(i * 100) / 3}%` }}
          />
          <div
            className="absolute inset-x-0 border-t border-foreground/30"
            style={{ top: `${(i * 100) / 3}%` }}
          />
        </React.Fragment>
      ))}
      <div className="absolute inset-[5%] border border-dashed border-foreground/30" />
      <div className="absolute inset-[10%] border border-dashed border-foreground/20" />
    </div>
  )
}

/** The frame under the playhead, rendered by `edits-render` on the Rust side, with guides, transform handles and drops from the browser. */
export function Viewer() {
  const { snapshot, info, frame, playing, scale, blurPaused } = useStudio()
  const ed = useEditor()
  const stage = React.useRef<HTMLDivElement>(null)
  const canvas = React.useRef<HTMLCanvasElement>(null)
  const [box, setBox] = React.useState({ w: 0, h: 0 })
  const [shown, setShown] = React.useState<number | null>(null)
  const [loading, setLoading] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)
  const [zoom, setZoom] = React.useState("fit")
  const [handles, setHandles] = React.useState(true)
  const drawn = React.useRef<ImageBitmap | null>(null)
  const wanted = React.useRef("")
  const inflight = React.useRef(0)

  React.useEffect(() => {
    const el = stage.current
    if (!el) return
    const ro = new ResizeObserver(([e]) =>
      setBox({ w: e.contentRect.width, h: e.contentRect.height })
    )
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const aspect = info ? info.width / info.height : 1
  const fit = React.useMemo(() => {
    const pad = 20
    const w = Math.max(0, box.w - pad * 2)
    const h = Math.max(0, box.h - pad * 2)
    const f = w / h > aspect ? { w: h * aspect, h } : { w, h: w / aspect }
    if (zoom === "fit" || !info) return f
    const z = Number(zoom) / (window.devicePixelRatio || 1)
    return { w: info.width * z, h: info.height * z }
  }, [box, aspect, zoom, info])

  const draw = React.useCallback((b: ImageBitmap | null) => {
    const c = canvas.current
    if (!c || !b) return
    drawn.current = b
    const dpr = window.devicePixelRatio || 1
    const w = Math.max(1, Math.round(c.clientWidth * dpr))
    const h = Math.max(1, Math.round(c.clientHeight * dpr))
    if (c.width !== w || c.height !== h) {
      c.width = w
      c.height = h
    }
    const ctx = c.getContext("2d")
    if (!ctx) return
    ctx.imageSmoothingQuality = "high"
    try {
      ctx.drawImage(b, 0, 0, w, h)
    } catch {
      // A bitmap of an older version was closed meanwhile: the new frame is on its way.
      drawn.current = null
    }
  }, [])

  // Redraw on resize.
  React.useEffect(() => draw(drawn.current), [fit, draw])

  // While a control is dragged, its preview replaces the frame.
  const live = ed.live
  React.useEffect(() => {
    if (live && live.frame === frame) draw(live.bitmap)
  }, [live, frame, draw])

  const revision = snapshot?.revision ?? 0
  const blur = !playing && blurPaused && !!info?.motion_blur
  // A preview that ended without an edit: back to the frame as it is.
  React.useEffect(() => {
    if (live) return
    const hit = cachedFrame({ revision, frame, scale, blur })
    if (hit) draw(hit)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [live])
  React.useEffect(() => {
    if (!info || !revision) return
    const req = { revision, frame, scale, blur }
    const k = `${revision}:${frame}:${scale}:${blur}`
    wanted.current = k
    const hit = cachedFrame(req)
    if (hit) {
      draw(hit)
      setShown(frame)
      setLoading(false)
    } else {
      const near = nearestCached(req, playing ? 25 : 0)
      if (near) {
        draw(near.bitmap)
        setShown(near.frame)
      }
      if (!playing) {
        setLoading(true)
        loadFrame(req, false)
          .then((b) => {
            if (b && wanted.current === k) {
              draw(b)
              setShown(frame)
              setError(null)
            }
          })
          .catch((e) => setError(String(e)))
          .finally(() => {
            if (wanted.current === k) setLoading(false)
          })
      }
    }
    if (playing) {
      // Keep a few frames ahead in flight; the render thread renders them in order.
      for (
        let f = frame;
        f <= Math.min(lastFrame(info), frame + 3 * AHEAD) &&
        inflight.current < AHEAD;
        f++
      ) {
        const r = { revision, frame: f, scale, blur: false }
        if (cachedFrame(r) || isPending(r)) continue
        inflight.current++
        loadFrame(r, true)
          .catch(() => null)
          .finally(() => {
            inflight.current--
          })
      }
    }
  }, [info, revision, frame, scale, blur, playing, draw])

  // Browser items dropped on the picture apply to the selected layer (or the picture on screen).
  const { registerDrop, apply, itemDrag } = ed
  React.useEffect(
    () =>
      registerDrop((x, y, item) => {
        const el = document.elementFromPoint(x, y)
        if (!el?.closest("[data-slot=viewer]")) return false
        void apply(item)
        return true
      }),
    [registerDrop, apply]
  )
  const dropping =
    !!itemDrag &&
    !!document
      .elementFromPoint(itemDrag.x, itemDrag.y)
      ?.closest("[data-slot=viewer]")

  const errors = snapshot?.problems.filter((p) => p.error) ?? []
  return (
    <div
      ref={stage}
      data-slot="viewer"
      className={cn(
        "relative flex min-h-0 flex-1 items-center justify-center bg-muted/30",
        zoom === "fit" ? "overflow-hidden" : "overflow-auto"
      )}
    >
      {errors.length > 0 && (
        <Alert
          variant="destructive"
          className="absolute top-12 right-3 left-3 z-10 w-auto"
        >
          <CircleAlertIcon />
          <AlertTitle>
            The file on disk has {errors.length} error
            {errors.length === 1 ? "" : "s"}; showing revision{" "}
            {snapshot?.revision}
          </AlertTitle>
          <AlertDescription>
            <span className="line-clamp-2">
              {errors[0].path}: {errors[0].message}
              {errors[0].hint ? ` (${errors[0].hint})` : ""}
            </span>
          </AlertDescription>
        </Alert>
      )}
      <div className="absolute top-2 right-2 z-20 flex items-center gap-1 rounded-lg border bg-background/90 p-0.5 backdrop-blur">
        <Tooltip>
          <TooltipTrigger
            render={
              <Toggle
                size="sm"
                pressed={handles}
                onPressedChange={setHandles}
                aria-label="Transform handles"
              />
            }
          >
            <MousePointer2Icon />
          </TooltipTrigger>
          <TooltipContent>
            Handles on the selected layer: drag to move, a corner to scale, the
            knob to rotate
          </TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Toggle
                size="sm"
                pressed={ed.guides}
                onPressedChange={ed.setGuides}
                aria-label="Guides"
              />
            }
          >
            <Grid3x3Icon />
          </TooltipTrigger>
          <TooltipContent>Rule of thirds and safe areas</TooltipContent>
        </Tooltip>
        <ToggleGroup
          multiple={false}
          size="sm"
          value={[zoom]}
          onValueChange={(v) => v[0] && setZoom(v[0])}
          aria-label="Zoom"
        >
          <ToggleGroupItem value="fit" className="px-2 text-xs">
            Fit
          </ToggleGroupItem>
          <ToggleGroupItem value="0.5" className="px-2 text-xs">
            50%
          </ToggleGroupItem>
          <ToggleGroupItem value="1" className="px-2 text-xs">
            100%
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div
        className={cn(
          "relative shrink-0 rounded-md border bg-background shadow-xs",
          dropping && "ring-2 ring-ring"
        )}
        style={{ width: fit.w, height: fit.h }}
      >
        <canvas ref={canvas} className="size-full rounded-md" />
        {ed.guides && <Guides />}
        {handles && <TransformOverlay width={fit.w} height={fit.h} />}
        {shown == null && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
            <Spinner className="mr-2" /> Rendering…
          </div>
        )}
      </div>
      <div className="absolute bottom-2 left-2 flex items-center gap-2">
        {live && <Badge variant="secondary">preview</Badge>}
        {!live && shown != null && shown !== frame && (
          <Badge variant="secondary" className="tabular-nums">
            showing frame {shown}
          </Badge>
        )}
        {loading && shown != null && (
          <Badge variant="outline">
            <Spinner /> rendering
          </Badge>
        )}
        {error && <Badge variant="destructive">{error}</Badge>}
      </div>
    </div>
  )
}
