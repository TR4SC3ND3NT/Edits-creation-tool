// Frames, seconds and project time (beats or seconds), as `engine::timemap`
// and `edits_core::Project` count them: a frame starts at frame / fps
// seconds, and in a beats project beat 0 is at `beat_offset` seconds.

import type { Info } from "@/lib/api"

export function toSeconds(info: Info, t: number) {
  return info.bpm ? info.beat_offset + (t * 60) / info.bpm : t
}

export function fromSeconds(info: Info, s: number) {
  return info.bpm ? ((s - info.beat_offset) * info.bpm) / 60 : s
}

export function frameTime(info: Info, frame: number) {
  return fromSeconds(info, frame / info.fps)
}

export function lastFrame(info: Info) {
  return Math.max(0, info.frames - 1)
}

/** The frame on screen at project time `t`. */
export function frameAt(info: Info, t: number) {
  const f = Math.floor(toSeconds(info, t) * info.fps + 1e-6)
  return Math.min(Math.max(0, f), lastFrame(info))
}

/** The first frame that starts at or after `t` (what a cut at `t` looks like). */
export function firstFrameFrom(info: Info, t: number) {
  const f = Math.ceil(toSeconds(info, t) * info.fps - 1e-6)
  return Math.min(Math.max(0, f), lastFrame(info))
}

/** One beat (one second in a seconds project) forward or back. */
export function jumpUnit(info: Info, frame: number, forward: boolean) {
  const t = frameTime(info, frame)
  let unit = forward ? Math.floor(t + 1e-6) + 1 : Math.ceil(t - 1e-6)
  for (let i = 0; i < 4096; i++) {
    const f = firstFrameFrom(info, unit)
    if (forward && (f > frame || f === lastFrame(info))) return f
    if (!forward && (f < frame || toSeconds(info, unit) <= 0))
      return Math.min(f, frame)
    unit += forward ? 1 : -1
  }
  return forward ? lastFrame(info) : 0
}

/** `00:01.48`: minutes, seconds, hundredths. */
export function clock(seconds: number) {
  const s = Math.max(0, seconds)
  const m = Math.floor(s / 60)
  const rest = s - m * 60
  return `${String(m).padStart(2, "0")}:${rest.toFixed(2).padStart(5, "0")}`
}

/** A number as the file would write it. */
export function tidy(x: number, digits = 3) {
  const r = Number(x.toFixed(digits))
  return Object.is(r, -0) ? 0 : r
}

export function fmtTime(info: Info | null, t: number) {
  if (!info) return `${tidy(t)}`
  return info.time_base === "beats" ? `b${tidy(t, 2)}` : `${tidy(t, 2)} s`
}

/** `1:3` for bar 1 beat 3 (1-based, four beats to a bar). */
export function barBeat(t: number) {
  const bar = Math.floor(t / 4) + 1
  const beat = Math.floor(t - (bar - 1) * 4) + 1
  return `${bar}:${beat}`
}

export function ago(ms: number | null, now = Date.now()) {
  if (ms == null) return ""
  const d = Math.max(0, Math.round((now - ms) / 1000))
  if (d < 5) return "just now"
  if (d < 60) return `${d} s ago`
  if (d < 3600) return `${Math.floor(d / 60)} min ago`
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`
  return `${Math.floor(d / 86400)} d ago`
}

export function bytes(n: number) {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
