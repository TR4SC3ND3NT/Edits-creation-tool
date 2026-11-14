"use client"

import * as React from "react"
import { isTauri } from "@tauri-apps/api/core"
import { MinusIcon, TrendingDownIcon, TrendingUpIcon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardAction,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { api, type ReportBrief } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { ago, tidy } from "@/lib/time"

/** Picture layers that start after the beginning: the cuts of the edit. */
export function cutTimes(
  layers: { kind: string; start: number; enabled: boolean }[],
  t0: number
) {
  const pictures = layers.filter(
    (l) => l.enabled && ["image", "video", "cube", "comp"].includes(l.kind)
  )
  return [
    ...new Set(pictures.map((l) => l.start).filter((t) => t > t0 + 1e-6)),
  ].sort((a, b) => a - b)
}

export function useReports(revision: number | undefined) {
  const [reports, setReports] = React.useState<ReportBrief[]>([])
  React.useEffect(() => {
    if (!isTauri()) return
    api
      .reportsList()
      .then(setReports)
      .catch(() => {})
  }, [revision])
  return reports
}

function Trend({ delta, unit = "%" }: { delta: number | null; unit?: string }) {
  if (delta == null) return null
  const Icon =
    delta > 0.0005
      ? TrendingUpIcon
      : delta < -0.0005
        ? TrendingDownIcon
        : MinusIcon
  return (
    <Badge variant="outline">
      <Icon />
      {delta > 0 ? "+" : ""}
      {unit === "%"
        ? `${(delta * 100).toFixed(1)}%`
        : `${tidy(delta, 2)}${unit}`}
    </Badge>
  )
}

export function SectionCards() {
  const { snapshot, info } = useStudio()
  const reports = useReports(snapshot?.revision)
  if (!snapshot || !info) return null
  const beats = info.time_base === "beats"
  const layers = snapshot.layers
  const effects = layers.reduce((n, l) => n + l.effects.length, 0)
  const keys = layers.reduce((n, l) => n + l.keys, 0)
  const enabled = layers.filter((l) => l.enabled).length
  const cuts = cutTimes(layers, Math.min(0, info.start_time))
  const transitions = layers.filter((l) => l.transition).length
  const spacing =
    cuts.length > 1
      ? (cuts[cuts.length - 1] - cuts[0]) / (cuts.length - 1)
      : null
  const last = reports.at(-1) ?? null
  const prev = reports.at(-2) ?? null
  return (
    <div className="grid grid-cols-1 gap-4 px-4 *:data-[slot=card]:bg-linear-to-t *:data-[slot=card]:from-primary/5 *:data-[slot=card]:to-card *:data-[slot=card]:shadow-xs lg:px-6 @xl/main:grid-cols-2 @5xl/main:grid-cols-5 dark:*:data-[slot=card]:bg-card">
      <Card className="@container/card">
        <CardHeader>
          <CardDescription>Duration</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {tidy(info.seconds, 2)} s
          </CardTitle>
          <CardAction>
            <Badge variant="outline">{info.frames} frames</Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="flex-col items-start gap-1.5 text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            {tidy(info.duration, 3)}{" "}
            {beats ? `beats · ${tidy(info.duration / 4, 2)} bars` : "seconds"}
          </div>
          <div className="text-muted-foreground">
            {info.width} × {info.height} at {info.fps} fps
          </div>
        </CardFooter>
      </Card>
      <Card className="@container/card">
        <CardHeader>
          <CardDescription>Tempo</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {info.bpm ? `${tidy(info.bpm, 2)} BPM` : "–"}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">{beats ? "beats" : "seconds"}</Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="flex-col items-start gap-1.5 text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            {info.bpm
              ? `${tidy(60 / info.bpm, 3)} s per beat`
              : "Timed in seconds"}
          </div>
          <div className="text-muted-foreground">
            {info.bpm
              ? `Beat 0 at ${tidy(info.beat_offset, 3)} s`
              : "No beat grid"}
          </div>
        </CardFooter>
      </Card>
      <Card className="@container/card">
        <CardHeader>
          <CardDescription>Layers</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {layers.length}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">{enabled} on</Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="flex-col items-start gap-1.5 text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            {effects} effects · {keys} keys
          </div>
          <div className="text-muted-foreground">
            {snapshot.slots.length} picture slots
          </div>
        </CardFooter>
      </Card>
      <Card className="@container/card">
        <CardHeader>
          <CardDescription>Cuts</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {cuts.length}
          </CardTitle>
          <CardAction>
            <Badge variant="outline">{transitions} transitions</Badge>
          </CardAction>
        </CardHeader>
        <CardFooter className="flex-col items-start gap-1.5 text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            {spacing != null
              ? `One every ${tidy(spacing, 2)} ${beats ? "beats" : "s"}`
              : cuts.length === 1
                ? "A single cut"
                : "No cuts"}
          </div>
          <div className="text-muted-foreground">
            {snapshot.markers.length} markers
          </div>
        </CardFooter>
      </Card>
      <Card className="@container/card">
        <CardHeader>
          <CardDescription>Match score</CardDescription>
          <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
            {last ? `${(last.overall * 100).toFixed(1)}%` : "–"}
          </CardTitle>
          <CardAction>
            {last && prev ? (
              <Trend delta={last.overall - prev.overall} />
            ) : null}
          </CardAction>
        </CardHeader>
        <CardFooter className="flex-col items-start gap-1.5 text-sm">
          <div className="line-clamp-1 flex gap-2 font-medium">
            {last && prev ? (
              last.overall >= prev.overall ? (
                <>
                  Closer to the reference <TrendingUpIcon className="size-4" />
                </>
              ) : (
                <>
                  Further from the reference{" "}
                  <TrendingDownIcon className="size-4" />
                </>
              )
            ) : last ? (
              "First comparison"
            ) : (
              "No comparison yet"
            )}
          </div>
          <div className="text-muted-foreground">
            {last
              ? `Compared ${ago(last.time)} · ${reports.length} reports`
              : "Compare a render on the Reference page"}
          </div>
        </CardFooter>
      </Card>
    </div>
  )
}
