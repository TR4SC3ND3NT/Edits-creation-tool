import * as React from "react"
import { CircleAlertIcon } from "lucide-react"

import { AgentViewing } from "@/agent/presence-ui"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Spinner } from "@/components/ui/spinner"
import { cachedFrame, isPending, loadFrame, nearestCached } from "@/lib/frames"
import { useStudio } from "@/lib/store"
import { lastFrame } from "@/lib/time"

/** Frames asked for ahead of the playhead while playing. */
const AHEAD = 4

/** The frame under the playhead, rendered by `edits-render` on the Rust side. */
export function Viewer() {
  const { snapshot, info, frame, playing, scale, blurPaused } = useStudio()
  const stage = React.useRef<HTMLDivElement>(null)
  const canvas = React.useRef<HTMLCanvasElement>(null)
  const [box, setBox] = React.useState({ w: 0, h: 0 })
  const [shown, setShown] = React.useState<number | null>(null)
  const [loading, setLoading] = React.useState(false)
  const [error, setError] = React.useState<string | null>(null)
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
    const pad = 24
    const w = Math.max(0, box.w - pad * 2)
    const h = Math.max(0, box.h - pad * 2)
    return w / h > aspect ? { w: h * aspect, h } : { w, h: w / aspect }
  }, [box, aspect])

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
    ctx.drawImage(b, 0, 0, w, h)
  }, [])

  // Redraw on resize.
  React.useEffect(() => draw(drawn.current), [fit, draw])

  const revision = snapshot?.revision ?? 0
  const blur = !playing && blurPaused && !!info?.motion_blur
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

  const errors = snapshot?.problems.filter((p) => p.error) ?? []
  return (
    <div
      ref={stage}
      className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden bg-muted/30"
    >
      {errors.length > 0 && (
        <Alert
          variant="destructive"
          className="absolute top-3 right-3 left-3 z-10 w-auto"
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
      <div
        className="relative overflow-hidden rounded-lg border bg-background shadow-xs"
        style={{ width: fit.w, height: fit.h }}
      >
        <canvas ref={canvas} className="size-full" />
        {shown == null && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
            <Spinner className="mr-2" /> Rendering…
          </div>
        )}
      </div>
      <AgentViewing />
      <div className="absolute bottom-3 left-3 flex items-center gap-2">
        {shown != null && shown !== frame && (
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
