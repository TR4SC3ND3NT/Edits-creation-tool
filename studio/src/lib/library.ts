import * as React from "react"
import { isTauri } from "@tauri-apps/api/core"

import { api, type EffectSpec, type Preset } from "@/lib/api"

let effects: Promise<EffectSpec[]> | null = null

/** The effect registry (it never changes while the app runs). */
export function useEffects(): EffectSpec[] {
  const [list, setList] = React.useState<EffectSpec[]>([])
  React.useEffect(() => {
    if (!isTauri()) return
    effects ??= api.effectsList()
    effects.then(setList).catch(() => {})
  }, [])
  return list
}

/** The presets the project can use; read again when the project's path changes or `bump` changes. */
export function usePresets(projectPath: string | null, bump = 0): Preset[] {
  const [list, setList] = React.useState<Preset[]>([])
  React.useEffect(() => {
    if (!isTauri()) return
    api
      .presetsList()
      .then(setList)
      .catch(() => {})
  }, [projectPath, bump])
  return list
}

export const CATEGORY_LABEL: Record<string, string> = {
  color: "Colour",
  blur: "Blur",
  light: "Light",
  distort: "Distort",
  stylize: "Stylize",
  time: "Time",
  generate: "Generate",
  custom: "Custom",
}

/** A unit for a value field: `%h` → `% h`, `deg` → `°`. */
export function unitLabel(unit: string) {
  switch (unit) {
    case "%h":
      return "% h"
    case "deg":
      return "°"
    case "x":
      return "×"
    case "pos":
      return "units"
    case "time":
      return "beats"
    case "px1080":
      return "px"
    case "count":
      return "#"
    default:
      return unit
  }
}
