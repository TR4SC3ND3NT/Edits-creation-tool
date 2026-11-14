// Where the agent is working, for the editor: a tiny store outside React
// (so any component can read it with one hook call, without a provider).
//
// Two sources feed it: the tool calls of the agent panel (ACP updates with
// a `presence` the Rust side computed from the call's arguments) and the
// activity log `.edits/activity.jsonl` that `edits-mcp` writes for agents
// working outside the studio. Changes the agent made are found by comparing
// the project before and after its edit (`bridge.tsx`).
//
// The editor calls:
//   usePresence({ layer: "d" })                 a layer row, a layer header
//   usePresence({ pointer: prop.pointer })      an inspector field
//   usePresence({ effect: { layer, index } })   an effect card
//   usePresence({ frame: true })                the viewer ("looking at frame 60")
//   usePresenceState() + presenceFor(state, target)   in a loop (rows)
//   useAgentMarker()                            the time the agent targets (timeline)
// Every hit fades after `HOLD` ms.

import * as React from "react"

/** How long a highlight stays, in ms. */
export const HOLD = 4500

export type StepStatus = "running" | "done" | "failed"

export type Step = {
  id: string
  source: "panel" | "log"
  /** Who: "Claude", "Codex", "Agent". */
  actor: string
  /** The edits tool, or null for another tool (Read, Bash, ...). */
  tool: string | null
  /** What a person reads: "applying flash at b12 on layer d". */
  label: string
  status: StepStatus
  layers: string[]
  pointers: string[]
  t: number | null
  frame: number | null
  /** A background job the call started (render_video). */
  job: string | null
  started: number
  ended: number | null
  error: string | null
}

export type ValueChange = {
  from: string
  to: string
  actor: string
  at: number
}

export type PresenceState = {
  steps: Step[]
  /** Canonical pointer (see `canonical`) -> what the agent changed there. */
  changes: Record<string, ValueChange>
  /** `layer` or `layer/effects/i` -> when the agent added it. */
  added: Record<string, { actor: string; at: number }>
  follow: boolean
  /** Bumped on every change and when a highlight fades. */
  version: number
  now: number
}

export type PresenceTarget = {
  layer?: string
  pointer?: string
  effect?: { layer: string; index: number }
  frame?: boolean
}

export type PresenceHit = {
  actor: string
  label: string
  /** The step is still running. */
  running: boolean
  /** A value the agent changed, as text. */
  from?: string
  to?: string
  /** The agent added this (an effect, a layer). */
  added?: boolean
  frame?: number | null
}

let doc: unknown = null
let state: PresenceState = {
  steps: [],
  changes: {},
  added: {},
  follow: false,
  version: 0,
  now: Date.now(),
}
const listeners = new Set<() => void>()
let fadeTimer: number | null = null

function readFollow() {
  try {
    return localStorage.getItem("edits-agent-follow") === "1"
  } catch {
    return false
  }
}
state.follow = readFollow()

function emit(next: Partial<PresenceState>) {
  state = { ...state, ...next, version: state.version + 1, now: Date.now() }
  listeners.forEach((l) => l())
  scheduleFade()
}

/** The next time something fades, and a timer to re-render then. */
function scheduleFade() {
  if (fadeTimer != null) window.clearTimeout(fadeTimer)
  const now = Date.now()
  const times: number[] = []
  for (const s of state.steps.slice(-8)) {
    if (s.ended != null && now - s.ended < HOLD) times.push(s.ended + HOLD)
    else if (s.status === "running" && now - s.started < HOLD)
      times.push(s.started + HOLD)
  }
  for (const c of Object.values(state.changes))
    if (now - c.at < HOLD) times.push(c.at + HOLD)
  for (const a of Object.values(state.added))
    if (now - a.at < HOLD * 2) times.push(a.at + HOLD * 2)
  if (!times.length) return
  const wait = Math.max(50, Math.min(...times) - now + 20)
  fadeTimer = window.setTimeout(() => {
    fadeTimer = null
    emit({})
  }, wait)
}

function subscribe(l: () => void) {
  listeners.add(l)
  return () => listeners.delete(l)
}

// ---- pointers ---------------------------------------------------------------------------------

type Doc = { layers?: { id?: string; effects?: { kind?: string }[] }[] }

/**
 * One form for every pointer that names the same place: layers by id,
 * effects by index (`/layers/@d/effects/@blur/params/radius` and
 * `/layers/3/effects/1/params/radius` both become `/layers/d/effects/1/params/radius`).
 */
export function canonical(pointer: string, d: unknown = doc): string {
  const tokens = pointer
    .replace(/^#/, "")
    .split("/")
    .filter((t, i) => i > 0 || t !== "")
  const layers = (d as Doc | null)?.layers ?? []
  if (tokens[0] !== "layers" || tokens.length < 2) return "/" + tokens.join("/")
  let li = -1
  const lt = tokens[1]
  if (lt.startsWith("@")) {
    tokens[1] = lt.slice(1)
    li = layers.findIndex((l) => l.id === tokens[1])
  } else if (/^\d+$/.test(lt)) {
    li = Number(lt)
    tokens[1] = layers[li]?.id ?? lt
  }
  if (tokens[2] === "effects" && tokens[3]?.startsWith("@")) {
    const kind = tokens[3].slice(1)
    const i = (layers[li]?.effects ?? []).findIndex((e) => e.kind === kind)
    tokens[3] = i >= 0 ? String(i) : kind
  }
  return "/" + tokens.join("/")
}

/** The value at a pointer of the document, if any. */
export function valueAt(pointer: string, d: unknown = doc): unknown {
  const tokens = canonical(pointer, d).split("/").slice(1)
  let v: unknown = d
  for (const t of tokens) {
    if (Array.isArray(v)) {
      const i = /^\d+$/.test(t) ? Number(t) : v.findIndex((x) => x?.id === t)
      v = v[i]
    } else if (v && typeof v === "object") {
      v = (v as Record<string, unknown>)[
        t.replace(/~1/g, "/").replace(/~0/g, "~")
      ]
    } else return undefined
  }
  return v
}

/** A value as a short text: `1.2`, `[0.2, 0, 0]`, `2 keys`. */
export function brief(v: unknown): string {
  if (v === undefined) return "—"
  if (typeof v === "number") return String(Math.round(v * 1000) / 1000)
  if (typeof v === "string") return v.length > 18 ? v.slice(0, 17) + "…" : v
  if (typeof v === "boolean") return v ? "on" : "off"
  if (Array.isArray(v)) {
    if (v.every((x) => typeof x === "number") && v.length <= 4)
      return `[${v.map((x) => brief(x)).join(", ")}]`
    return `${v.length} items`
  }
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>
    if (Array.isArray(o.keys)) return `${o.keys.length} keys`
    if (typeof o.kind === "string") return o.kind
    return "{…}"
  }
  return String(v)
}

// ---- writing --------------------------------------------------------------------------------------

export const presence = {
  /** The project as the agent sees it now (for pointers and old values). */
  setDoc(d: unknown) {
    doc = d
  },
  getDoc() {
    return doc
  },
  getState() {
    return state
  },
  /** A tool call started (or was seen again with more detail). */
  begin(
    step: Omit<Step, "started" | "ended" | "status" | "error"> & Partial<Step>
  ) {
    const now = Date.now()
    const i = state.steps.findIndex((s) => s.id === step.id)
    const steps = state.steps.slice()
    if (i >= 0) {
      const old = steps[i]
      steps[i] = {
        ...old,
        ...step,
        // An update without a title (tool_call_update) keeps the step's label and tool.
        label: step.label || old.label,
        tool: step.tool ?? old.tool,
        layers: step.layers.length ? step.layers : old.layers,
        pointers: step.pointers.length ? step.pointers : old.pointers,
        t: step.t ?? old.t,
        frame: step.frame ?? old.frame,
        status: step.status ?? old.status,
        started: old.started,
        ended: step.ended ?? old.ended,
        error: step.error ?? old.error,
      }
    } else {
      steps.push({
        status: "running",
        started: now,
        ended: null,
        error: null,
        ...step,
      })
    }
    emit({ steps: steps.slice(-150) })
  },
  /** A tool call ended. */
  end(
    id: string,
    ok: boolean,
    extra?: { error?: string | null; job?: string | null }
  ) {
    const i = state.steps.findIndex((s) => s.id === id)
    if (i < 0) return
    const steps = state.steps.slice()
    steps[i] = {
      ...steps[i],
      status: ok ? "done" : "failed",
      ended: Date.now(),
      error: extra?.error ?? null,
      job: extra?.job ?? steps[i].job,
    }
    emit({ steps })
  },
  /** Steps from before the studio was looking: listed, never highlighted. */
  history(old: Step[]) {
    const known = new Set(state.steps.map((s) => s.id))
    const add = old.filter((s) => !known.has(s.id))
    if (add.length) emit({ steps: [...add, ...state.steps].slice(-150) })
  },
  /** Values the agent changed, with the value before and after. */
  changed(
    list: { pointer: string; from: unknown; to: unknown }[],
    actor: string
  ) {
    const at = Date.now()
    const changes = { ...state.changes }
    for (const c of list)
      changes[canonical(c.pointer)] = {
        from: brief(c.from),
        to: brief(c.to),
        actor,
        at,
      }
    emit({ changes })
  },
  /** Effects and layers the agent added: `d` or `d/effects/2`. */
  addedBy(keys: string[], actor: string) {
    const at = Date.now()
    const added = { ...state.added }
    for (const k of keys) added[k] = { actor, at }
    emit({ added })
  },
  setFollow(on: boolean) {
    try {
      localStorage.setItem("edits-agent-follow", on ? "1" : "0")
    } catch {
      /* the choice is kept for this run only */
    }
    emit({ follow: on })
  },
  clear() {
    emit({ steps: [], changes: {}, added: {} })
  },
}

// ---- reading --------------------------------------------------------------------------------------

/** Steps that still glow: running ones, and ones that ended less than `HOLD` ago. */
export function liveSteps(s: PresenceState): Step[] {
  return s.steps.filter(
    (x) =>
      (x.status === "running" && s.now - x.started < 60_000) ||
      (x.ended != null && s.now - x.ended < HOLD)
  )
}

/** What the agent does at a target now, if anything. A non-hook form for loops. */
export function presenceFor(
  s: PresenceState,
  target: PresenceTarget
): PresenceHit | null {
  const live = liveSteps(s).reverse()
  if (target.frame) {
    const st = live.find(
      (x) => x.tool === "render_frame" || x.tool === "render_sheet"
    )
    return st
      ? {
          actor: st.actor,
          label: st.label,
          running: st.status === "running",
          frame: st.frame,
        }
      : null
  }
  if (target.effect) {
    const key = `${target.effect.layer}/effects/${target.effect.index}`
    const add = s.added[key]
    if (add && s.now - add.at < HOLD * 2)
      return { actor: add.actor, label: "added", running: false, added: true }
    const prefix = `/layers/${key}`
    const ch = Object.entries(s.changes).find(
      ([p, c]) =>
        (p === prefix || p.startsWith(prefix + "/")) && s.now - c.at < HOLD
    )
    if (ch) return { actor: ch[1].actor, label: "changed", running: false }
    const st = live.find((x) =>
      x.pointers.some((p) => {
        const c = canonical(p)
        return c === prefix || c.startsWith(prefix + "/")
      })
    )
    return st
      ? { actor: st.actor, label: st.label, running: st.status === "running" }
      : null
  }
  if (target.pointer) {
    const key = canonical(target.pointer)
    const ch = s.changes[key]
    if (ch && s.now - ch.at < HOLD)
      return {
        actor: ch.actor,
        label: `${ch.from} → ${ch.to}`,
        running: false,
        from: ch.from,
        to: ch.to,
      }
    const st = live.find((x) => x.pointers.some((p) => canonical(p) === key))
    return st
      ? { actor: st.actor, label: st.label, running: st.status === "running" }
      : null
  }
  if (target.layer) {
    const add = s.added[target.layer]
    const st = live.find((x) => x.layers.includes(target.layer!))
    if (st)
      return {
        actor: st.actor,
        label: st.label,
        running: st.status === "running",
      }
    if (add && s.now - add.at < HOLD * 2)
      return { actor: add.actor, label: "added", running: false, added: true }
    const prefix = `/layers/${target.layer}/`
    const ch = Object.entries(s.changes).find(
      ([p, c]) => p.startsWith(prefix) && s.now - c.at < HOLD
    )
    if (ch) return { actor: ch[1].actor, label: "changed", running: false }
    return null
  }
  return null
}

/** The whole state (re-renders on every change). */
export function usePresenceState(): PresenceState {
  return React.useSyncExternalStore(subscribe, () => state)
}

/** What the agent does at `target` now, or null. */
export function usePresence(target: PresenceTarget): PresenceHit | null {
  const s = usePresenceState()
  return presenceFor(s, target)
}

/** The newest step that is running or just ended, for the status strip. */
export function useAgentStatus(): Step | null {
  const s = usePresenceState()
  const live = liveSteps(s)
  return live[live.length - 1] ?? null
}

/** The time the agent targets now, for a marker on the timeline. */
export function useAgentMarker(): {
  t: number
  actor: string
  label: string
} | null {
  const s = usePresenceState()
  const st = liveSteps(s)
    .reverse()
    .find((x) => x.t != null)
  return st ? { t: st.t!, actor: st.actor, label: st.label } : null
}

export function useFollowAgent(): [boolean, (on: boolean) => void] {
  const s = usePresenceState()
  return [s.follow, presence.setFollow]
}

/** Classes for something the agent works on: a ring in the theme's ring colour. */
export function presenceRing(hit: PresenceHit | null): string {
  if (!hit) return ""
  return hit.running
    ? "ring-2 ring-ring ring-offset-1 ring-offset-background transition-shadow"
    : "ring-1 ring-ring/70 transition-shadow"
}
