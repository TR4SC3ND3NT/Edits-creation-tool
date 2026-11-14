// What the editor draws where the agent works: its badge, the status strip
// with the current step, a marker on the timeline at the time it targets, a
// glow on the layer row, "Claude is looking at frame 60" on the viewer, the
// old → new value on an inspector field and "added by Claude" on an effect.
// Each piece reads `presence.ts` and is one line in the editor's code.

import * as React from "react"
import {
  CameraIcon,
  CircleCheckIcon,
  CircleXIcon,
  HistoryIcon,
} from "lucide-react"
import { cn } from "cn"

import {
  liveSteps,
  presenceRing,
  useAgentMarker,
  useAgentStatus,
  usePresence,
  usePresenceState,
  type PresenceHit,
  type Step,
} from "@/agent/presence"
import { AgentLogo } from "@/agent/agent-logo"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { Spinner } from "@/components/ui/spinner"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { useStudio } from "@/lib/store"
import { frameAt, lastFrame } from "@/lib/time"

/** The agent's avatar: its mark (Claude's logo, or the initial of another
 * agent), small, on the card colour with a thin ring. */
export function AgentAvatar({
  name,
  size = "sm",
  live,
}: {
  name: string
  size?: "sm" | "default"
  live?: boolean
}) {
  return (
    <Avatar
      size={size}
      className={cn(
        "ring-1 ring-border",
        size === "sm" && "size-5",
        live && "animate-pulse"
      )}
    >
      <AvatarFallback className="bg-card text-foreground">
        <AgentLogo name={name} className={size === "sm" ? "size-3" : "size-4"} />
      </AvatarFallback>
    </Avatar>
  )
}

/** "Claude" as a badge, with its avatar. */
export function AgentBadge({
  name,
  label,
  live,
}: {
  name: string
  label?: string
  live?: boolean
}) {
  return (
    <Badge
      variant="outline"
      className="h-6 gap-1.5 border-ring bg-background/90 pr-2 pl-0.5 shadow-sm backdrop-blur"
    >
      <AgentAvatar name={name} live={live} />
      <span className="font-medium">{name}</span>
      {label && (
        <span className="max-w-56 truncate font-normal text-muted-foreground">
          {label}
        </span>
      )}
    </Badge>
  )
}

/** Moves the playhead and the selection to a step. */
export function useJumpTo() {
  const s = useStudio()
  return React.useCallback(
    (st: Step) => {
      if (!s.info) return
      if (st.frame != null) s.seek(Math.min(st.frame, lastFrame(s.info)))
      else if (st.t != null) s.seek(frameAt(s.info, st.t))
      const layer = st.layers.find((l) =>
        s.snapshot?.layers.some((x) => x.id === l)
      )
      if (layer) s.select(layer)
      if (st.t != null || st.frame != null || layer) s.setPage("edit")
    },
    [s]
  )
}

/** The strip under the header: what the agent does now, with a progress bar while it runs. */
export function AgentStatusStrip() {
  const st = useAgentStatus()
  const jump = useJumpTo()
  if (!st) return null
  const running = st.status === "running"
  return (
    <div className="flex h-8 shrink-0 items-center gap-2 border-b bg-muted/40 px-4 text-xs">
      <AgentAvatar name={st.actor} live={running} />
      <button
        type="button"
        className="min-w-0 truncate text-left hover:underline"
        onClick={() => jump(st)}
        title="Show where"
      >
        <span className="font-medium">{st.actor}:</span> {st.label}
        {st.job ? ` (job ${st.job})` : ""}
      </button>
      {running ? (
        <Progress value={null} className="ml-auto w-32" />
      ) : st.status === "failed" ? (
        <Badge variant="destructive" className="ml-auto">
          <CircleXIcon />
          {st.error ? st.error.slice(0, 60) : "failed"}
        </Badge>
      ) : (
        <Badge variant="secondary" className="ml-auto">
          <CircleCheckIcon />
          done
        </Badge>
      )}
    </div>
  )
}

/** A dashed marker through the timeline at the time the agent targets. `x` maps project time to pixels. */
export function AgentTimeMarker({ x }: { x: (t: number) => number }) {
  const m = useAgentMarker()
  if (!m) return null
  return (
    <div
      className="pointer-events-none absolute top-0 bottom-0 z-30 border-l-2 border-dashed border-ring"
      style={{ left: x(m.t) }}
    >
      <div className="absolute top-0.5 left-1 whitespace-nowrap">
        <AgentBadge name={m.actor} />
      </div>
    </div>
  )
}

/** A glow over a layer's track while the agent works on it; `left` places its badge (the bar's start). */
export function AgentRowGlow({
  layer,
  left = 0,
}: {
  layer: string
  left?: number
}) {
  const hit = usePresence({ layer })
  if (!hit) return null
  return (
    <div
      className={cn(
        "pointer-events-none absolute inset-0 z-10 rounded-sm bg-primary/5",
        presenceRing(hit)
      )}
    >
      <div className="absolute -top-px" style={{ left: Math.max(2, left) }}>
        <Tooltip>
          <TooltipTrigger
            render={<span className="pointer-events-auto inline-flex" />}
          >
            <AgentBadge
              name={hit.actor}
              live={hit.running}
              label={hit.added ? "added" : undefined}
            />
          </TooltipTrigger>
          <TooltipContent>
            {hit.actor}: {hit.label}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}

/** "Claude is looking at frame 60", in the viewer's corner. */
export function AgentViewing({
  className = "top-3 right-3",
}: {
  /** Where in the viewer (its position classes). */
  className?: string
}) {
  const hit = usePresence({ frame: true })
  if (!hit) return null
  return (
    <div className={cn("absolute z-20", className)}>
      <Badge
        variant="outline"
        className="h-7 gap-1.5 border-ring bg-background/90 pr-2.5 pl-0.5 shadow-sm backdrop-blur"
      >
        <AgentAvatar name={hit.actor} live={hit.running} />
        <CameraIcon className="size-3.5" />
        {hit.actor} is {hit.label.replace(/^looking/, "looking")}
        {hit.running && <Spinner className="size-3" />}
      </Badge>
    </div>
  )
}

/** The old → new value of a field the agent just changed, or its badge while it works there. */
export function PresenceValue({ hit }: { hit: PresenceHit | null }) {
  if (!hit) return null
  return (
    <Badge
      variant="outline"
      className="h-5 max-w-44 shrink-0 gap-1 border-ring px-1 font-normal"
    >
      <AgentAvatar name={hit.actor} />
      <span className="truncate tabular-nums">
        {hit.from != null ? `${hit.from} → ${hit.to}` : hit.label}
      </span>
    </Badge>
  )
}

/** "added by Claude" on an effect card the agent added; a ring while it changes it. */
export function AgentAddedBadge({
  layer,
  index,
}: {
  layer: string
  index: number
}) {
  const hit = usePresence({ effect: { layer, index } })
  if (!hit) return null
  return (
    <Badge variant="outline" className="gap-1 border-ring font-normal">
      <AgentAvatar name={hit.actor} live={hit.running} />
      {hit.added ? `added by ${hit.actor}` : `${hit.actor}: ${hit.label}`}
    </Badge>
  )
}

/** Ring classes for an effect card the agent works on (for a `className`). */
export function useEffectRing(layer: string, index: number) {
  return presenceRing(usePresence({ effect: { layer, index } }))
}

function ago(ms: number) {
  const s = Math.round((Date.now() - ms) / 1000)
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.round(s / 60)}m`
  return `${Math.round(s / 3600)}h`
}

/** The agent's steps, newest first; a click jumps to the time and layer of one. */
export function AgentSteps({ limit = 60 }: { limit?: number }) {
  const s = usePresenceState()
  const jump = useJumpTo()
  const live = new Set(liveSteps(s).map((x) => x.id))
  const steps = s.steps.slice(-limit).reverse()
  if (!steps.length)
    return (
      <div className="flex flex-col items-center gap-2 p-6 text-center text-xs text-muted-foreground">
        <HistoryIcon className="size-5" />
        The agent's steps show here: each edit, preset and frame it looks at,
        from this panel or from an agent working in a terminal through the edits
        MCP server.
      </div>
    )
  return (
    <div className="flex flex-col">
      {steps.map((st) => (
        <Button
          key={st.id}
          variant="ghost"
          className={cn(
            "h-auto justify-start gap-2 rounded-none px-3 py-1.5 text-left text-xs font-normal",
            live.has(st.id) && "bg-muted"
          )}
          onClick={() => jump(st)}
        >
          {st.status === "running" ? (
            <Spinner className="size-3.5 shrink-0" />
          ) : st.status === "failed" ? (
            <CircleXIcon className="size-3.5 shrink-0 text-destructive" />
          ) : (
            <CircleCheckIcon className="size-3.5 shrink-0 text-muted-foreground" />
          )}
          <span className="min-w-0 flex-1">
            <span className="block truncate">
              <span className="font-medium">{st.actor}</span> {st.label}
            </span>
            <span className="block truncate text-muted-foreground">
              {[
                st.tool ?? "",
                st.layers.length ? `layer ${st.layers.join(", ")}` : "",
                st.frame != null
                  ? `frame ${st.frame}`
                  : st.t != null
                    ? `t ${Math.round(st.t * 100) / 100}`
                    : "",
                st.source === "log" ? "terminal" : "",
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </span>
          {st.started > 0 && (
            <span className="shrink-0 text-muted-foreground tabular-nums">
              {ago(st.started)}
            </span>
          )}
        </Button>
      ))}
    </div>
  )
}
