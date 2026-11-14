// Connects the presence store to the studio: it keeps the project the
// pointers are read against, finds what an agent's edit changed (values
// before and after, effects and layers it added) by comparing the project
// before and after, and moves the playhead and the selection to the agent's
// step when "follow agent" is on. Renders nothing.

import * as React from "react"

import { presence, usePresenceState } from "@/agent/presence"
import type { Snapshot } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { frameAt, lastFrame } from "@/lib/time"

type J = unknown
type Change = { pointer: string; from: J; to: J }

const isObj = (v: J): v is Record<string, J> =>
  !!v && typeof v === "object" && !Array.isArray(v)
const leafArray = (v: J) =>
  Array.isArray(v) && v.every((x) => typeof x !== "object" || x === null)
const same = (a: J, b: J) => JSON.stringify(a) === JSON.stringify(b)

function walk(a: J, b: J, path: string, out: Change[]) {
  if (out.length > 60 || same(a, b)) return
  if (
    isObj(a) &&
    isObj(b) &&
    !("keys" in a && Array.isArray(a.keys)) &&
    !("keys" in b && Array.isArray(b.keys))
  ) {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)]))
      walk(a[k], b[k], `${path}/${k}`, out)
    return
  }
  if (Array.isArray(a) && Array.isArray(b) && !leafArray(a) && !leafArray(b)) {
    const n = Math.max(a.length, b.length)
    for (let i = 0; i < n; i++) walk(a[i], b[i], `${path}/${i}`, out)
    return
  }
  out.push({ pointer: path, from: a, to: b })
}

/** What changed between two versions of the project: values, and effects and layers that are new. */
export function diffProjects(
  prev: J,
  next: J
): { changes: Change[]; added: string[] } {
  const pl = ((prev as { layers?: J[] })?.layers ?? []) as Record<string, J>[]
  const nl = ((next as { layers?: J[] })?.layers ?? []) as Record<string, J>[]
  const byId = new Map(pl.map((l) => [String(l.id), l]))
  const changes: Change[] = []
  const added: string[] = []
  for (const l of nl) {
    const id = String(l.id)
    const old = byId.get(id)
    if (!old) {
      added.push(id)
      continue
    }
    const oe = (old.effects as Record<string, J>[] | undefined) ?? []
    const ne = (l.effects as Record<string, J>[] | undefined) ?? []
    ne.forEach((e, i) => {
      const before = oe[i]
      if (!before || before.kind !== e.kind) {
        if (!oe.some((x, j) => j >= i && x.kind === e.kind && same(x, e)))
          added.push(`${id}/effects/${i}`)
      } else walk(before, e, `/layers/@${id}/effects/${i}`, changes)
    })
    const { effects: _a, ...restOld } = old
    const { effects: _b, ...restNew } = l
    void _a
    void _b
    walk(restOld, restNew, `/layers/@${id}`, changes)
  }
  const { layers: _c, ...po } = (prev ?? {}) as Record<string, J>
  const { layers: _d, ...no } = (next ?? {}) as Record<string, J>
  void _c
  void _d
  walk(po, no, "", changes)
  return { changes, added }
}

export function PresenceBridge() {
  const s = useStudio()
  const p = usePresenceState()
  const prev = React.useRef<Snapshot | null>(null)

  React.useEffect(() => {
    const snap = s.snapshot
    const old = prev.current
    prev.current = snap
    presence.setDoc(snap?.doc ?? null)
    if (!snap || !old || old.path !== snap.path) return
    const top = snap.history.undo[0]
    const was = old.history.undo[0]
    if (!top || top.file === was?.file || top.actor !== "agent") return
    const st = presence.getState().steps
    const recent = [...st]
      .reverse()
      .find((x) => Date.now() - (x.ended ?? x.started) < 20_000)
    const actor = recent?.actor ?? "Agent"
    const { changes, added } = diffProjects(old.doc, snap.doc)
    // Old values are read against the old project, new ones are in the change.
    presence.setDoc(snap.doc)
    if (changes.length) presence.changed(changes, actor)
    if (added.length) presence.addedBy(added, actor)
  }, [s.snapshot])

  // Follow the agent: the newest step that names a time, a frame or a layer.
  const seen = React.useRef<string | null>(null)
  const { info, seek, select, snapshot } = s
  React.useEffect(() => {
    const last = p.steps[p.steps.length - 1]
    const key = last
      ? `${last.id}:${last.t}:${last.frame}:${last.layers.join(",")}`
      : ""
    if (!last || key === seen.current || last.started === 0) return
    seen.current = key
    if (!p.follow || !info) return
    if (last.frame != null) seek(Math.min(last.frame, lastFrame(info)))
    else if (last.t != null) seek(frameAt(info, last.t))
    const layer = last.layers.find((l) =>
      snapshot?.layers.some((x) => x.id === l)
    )
    if (layer) select(layer)
  }, [p.steps, p.follow, info, seek, select, snapshot])

  return null
}
