// Typed wrappers of the Tauri commands in `src-tauri/src/commands.rs`.
// Every call goes through `trackedInvoke`, so it shows in the debug panel.

import { invoke } from "@tauri-apps/api/core"

import { trackedInvoke } from "@/lib/tauri"

export type Problem = {
  error: boolean
  path: string
  message: string
  hint: string | null
}

export type Info = {
  width: number
  height: number
  fps: number
  duration: number
  time_base: "beats" | "seconds"
  bpm: number | null
  beat_offset: number
  frames: number
  seconds: number
  start_time: number
  end_time: number
  audio: { path: string; exists: boolean; offset: number; gain: number } | null
  motion_blur: boolean
}

export type Mark = {
  t: number
  shape: "linear" | "hold" | "eased"
  what: string[]
  effect: boolean
}

export type Row = {
  id: string
  kind: string
  label: string
  index: number | null
  start: number
  end: number
  enabled: boolean
  effects: string[]
  blend: string | null
  marks: Mark[]
  transition: [string, number] | null
}

export type LayerBrief = {
  id: string
  index: number
  kind: string
  label: string
  name: string | null
  start: number
  end: number
  enabled: boolean
  blend: string
  effects: { kind: string; enabled: boolean }[]
  keys: number
  transition: string | null
  slot: string | null
}

export type HistoryItem = {
  file: string
  time: number | null
  actor: string
  summary: string
  readable: string
}

export type Snapshot = {
  path: string
  dir: string
  file_name: string
  revision: number
  text_hash: string | null
  rejected: boolean
  problems: Problem[]
  info: Info | null
  rows: Row[]
  layers: LayerBrief[]
  markers: { t: number; label: string }[]
  slots: {
    name: string
    path: string | null
    exists: boolean
    fit: string
    used_by: string[]
  }[]
  history: { undo: HistoryItem[]; redo: HistoryItem[] }
  doc: Record<string, unknown> | null
}

export type Reason =
  | { kind: "opened" }
  | { kind: "window"; summary: string }
  | { kind: "edit"; actor: string; summary: string }
  | { kind: "undo"; actor: string; summary: string }
  | { kind: "reloaded" }
  | { kind: "invalid"; errors: number }
  | { kind: "problems" }

export type ProjectEvent = { snapshot: Snapshot; reason: Reason }

export type PropKey =
  | { type: "transform"; name: string }
  | { type: "effect"; index: number; kind: string; param: string }
  | { type: "camera"; name: string }

export type KeyDto = { t: number; v: number[]; ease: string }

export type PropDto = {
  key: PropKey
  name: string
  pointer: string
  value: number[]
  base: number[]
  set: boolean
  animated: boolean
  shake: boolean
  looped: boolean
  keys: KeyDto[]
  key_here: number | null
  segment: number | null
  unit: string
  range: [number, number] | null
  default: number[]
  doc: string
}

export type EffectDto = {
  index: number
  kind: string
  category: string
  doc: string
  enabled: boolean
  known: boolean
  params: PropDto[]
  options: { name: string; values: string[]; value: string; doc: string }[]
}

export type Inspection = {
  t: number
  layer: {
    id: string
    index: number
    kind: string
    label: string
    name: string | null
    start: number
    end: number | null
    enabled: boolean
    blend: string
    visible_now: boolean
    slot: string | null
    color: [number, number, number, number] | null
    transition: {
      kind: string
      duration: number
      ease: string
      doc: string
    } | null
  } | null
  transform: PropDto[]
  effects: EffectDto[]
  camera: PropDto[]
}

export type EditDone = { summary: string; warnings: string[]; revision: number }

export type EffectSpec = {
  kind: string
  category: string
  doc: string
  params: {
    name: string
    default: number[]
    unit: string
    range: [number, number] | null
    doc: string
  }[]
  options: [string, string[]][]
  reads_time: boolean
  credit: string
  needs_source: boolean
}

export type Variable = {
  name: string
  type:
    "number" | "integer" | "boolean" | "color" | "vector" | "enum" | "string"
  default: unknown
  min: number | null
  max: number | null
  unit: string | null
  doc: string
  options: string[]
}

export type Preset = {
  name: string
  description: string
  tags: string[]
  group: string
  target: "layer" | "project"
  transition: boolean
  variables: Variable[]
  credit: string | null
  origin: "builtin" | "transition" | "project"
}

export type RenderStatus = {
  adapter: string | null
  backend: string | null
  software: boolean
  error: string | null
  last_ms: number | null
  prepared: number | null
  prepare_error: string | null
  cached_frames: number
  cached_bytes: number
}

export type JobStatus = {
  output: string
  running: boolean
  done: number
  total: number
  line: string
  elapsed: number
  outcome: "ok" | "failed" | "cancelled" | null
  error: string | null
}

export type RenderOutput = {
  path: string
  name: string
  folder: string
  bytes: number
  modified: number
}

export type Signature = {
  f0: number
  lum: number[]
  stops: number[]
  blur: number[]
  sat: number[]
  scale: (number | null)[]
}

export type Analysis = {
  path: string
  video: string
  width: number
  height: number
  fps: number
  frames: number
  bpm: number
  offset_seconds: number
  shots: {
    id: number
    f0: number
    f1: number
    beat0: number
    beat1: number
    status: string
    events: string[]
  }[]
  signature: Signature
  notes: string[]
}

export type ReportBrief = {
  path: string
  time: number
  overall: number
  scores: Record<string, number | null>
  candidate: string
  reference: string
}

export type Comparison = {
  reference: string
  candidate: string
  scores: Record<string, number | null>
  overall: number
  tempo: {
    reference_bpm: number
    candidate_bpm: number | null
    phase_offset_frames: number | null
    note: string
  }
  cuts: {
    reference: number
    candidate: number
    matched: number
    precision: number
    recall: number
    f1: number
  }
  flashes: { reference: number; candidate: number; matched: number; f1: number }
  tone_hits: {
    reference: number
    candidate: number
    matched: number
    f1: number
  }
  shots: {
    shot: number
    f0: number
    f1: number
    beat0: number
    beat1: number
  }[]
  differences: {
    category: string
    shot?: number
    severity: number
    text: string
  }[]
}

export type Report = {
  path: string
  comparison: Comparison
  candidate_signature: Signature | null
}

export type CompareRun = {
  running: boolean
  video: string | null
  error: string | null
  report: string | null
}

export type Startup = {
  project: string | null
  page: string | null
  select: string | null
  at: number | null
}

export type UiInputs = {
  t: number
  seconds: number
  frame: number
  playing: boolean
  layer: string | null
  camera: boolean
  property: string | null
  effect: number | null
  key: number | null
  visible: [number, number] | null
  looping: [number, number] | null
  note: string | null
  note_at: string | null
}

const call = <T>(command: string, args?: Record<string, unknown>) =>
  trackedInvoke<T>(command, args)

export const api = {
  startup: () => call<Startup>("startup"),
  openProject: (path: string) => call<Snapshot>("open_project", { path }),
  snapshot: () => call<Snapshot | null>("snapshot"),
  inspect: (layer: string | null, t: number) =>
    call<Inspection>("inspect", { layer, t }),
  editOps: (ops: unknown[], label?: string) =>
    call<EditDone>("edit_ops", { ops, label }),
  propSet: (layer: string | null, key: PropKey, t: number, value: number[]) =>
    call<EditDone>("prop_set", { layer, key, t, value }),
  propKeyHere: (layer: string | null, key: PropKey, t: number) =>
    call<EditDone>("prop_key_here", { layer, key, t }),
  propRemoveKey: (layer: string | null, key: PropKey, t: number) =>
    call<EditDone>("prop_remove_key", { layer, key, t }),
  propEase: (layer: string | null, key: PropKey, index: number, ease: string) =>
    call<EditDone>("prop_ease", { layer, key, index, ease }),
  layerAdd: (
    kind: string,
    slot: string | null,
    at: number,
    above: string | null
  ) => call<EditDone>("layer_add", { kind, slot, at, above }),
  layerDuplicate: (id: string) => call<EditDone>("layer_duplicate", { id }),
  layerRemove: (id: string) => call<EditDone>("layer_remove", { id }),
  layerToggle: (id: string) => call<EditDone>("layer_toggle", { id }),
  layerMove: (id: string, up: boolean) =>
    call<EditDone>("layer_move", { id, up }),
  layerReorder: (id: string, index: number) =>
    call<EditDone>("layer_reorder", { id, index }),
  layerDrag: (
    layer: string,
    kind: string,
    from: number | null,
    grab: number,
    now: number,
    alt: boolean
  ) => call<EditDone>("layer_drag", { layer, kind, from, grab, now, alt }),
  effectAdd: (layer: string, kind: string) =>
    call<EditDone>("effect_add", { layer, kind }),
  effectRemove: (layer: string, index: number) =>
    call<EditDone>("effect_remove", { layer, index }),
  effectToggle: (layer: string, index: number) =>
    call<EditDone>("effect_toggle", { layer, index }),
  effectMove: (layer: string, index: number, up: boolean) =>
    call<EditDone>("effect_move", { layer, index, up }),
  effectOption: (layer: string, index: number, name: string, value: string) =>
    call<EditDone>("effect_option", { layer, index, name, value }),
  undo: () => call<EditDone>("undo"),
  redo: () => call<EditDone>("redo"),
  restore: (file: string) => call<EditDone>("restore", { file }),
  effectsList: () => call<EffectSpec[]>("effects_list"),
  presetsList: () => call<Preset[]>("presets_list"),
  presetApply: (request: {
    name: string
    target: string
    selected: string | null
    at: string
    playhead: number
    values: Record<string, unknown>
  }) => call<EditDone>("preset_apply", { request }),
  // Sent on every playhead change: not tracked, or it would flood the debug panel.
  uiReport: (inputs: UiInputs) =>
    invoke<string | null>("ui_report", { inputs }),
  renderStatus: () => invoke<RenderStatus>("render_status"),
  rendersList: () =>
    call<{
      outputs: RenderOutput[]
      default_output: string | null
      job: JobStatus | null
      edits: string | null
    }>("renders_list"),
  renderStart: (output: string | null, scale: number | null) =>
    call<JobStatus>("render_start", { output, scale }),
  renderCancel: () => call<void>("render_cancel"),
  analysesFind: () => call<string[]>("analyses_find"),
  analysisLoad: (path: string) => call<Analysis>("analysis_load", { path }),
  reportsList: () => call<ReportBrief[]>("reports_list"),
  reportLoad: (path: string) => call<Report>("report_load", { path }),
  compareStatus: () => call<CompareRun>("compare_status"),
  compareStart: (analysis: string, video: string) =>
    call<void>("compare_start", { analysis, video }),
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e
  if (e instanceof Error) return e.message
  return JSON.stringify(e)
}
