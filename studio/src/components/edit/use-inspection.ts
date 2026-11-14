import * as React from "react"

import { api, errorText, type Inspection } from "@/lib/api"
import { useStudio } from "@/lib/store"

/** The selected layer (or the camera) evaluated at the playhead, read again on every new version. */
export function useInspection() {
  const { snapshot, layer, camera, t, playing } = useStudio()
  const [data, setData] = React.useState<Inspection | null>(null)
  const [error, setError] = React.useState<string | null>(null)
  const revision = snapshot?.revision
  React.useEffect(() => {
    if (playing || revision == null || (!layer && !camera)) {
      if (!layer && !camera) setData(null)
      return
    }
    let live = true
    api
      .inspect(layer, t)
      .then((d) => live && (setData(d), setError(null)))
      .catch((e) => live && setError(errorText(e)))
    return () => {
      live = false
    }
  }, [revision, layer, camera, t, playing])
  return { data, error }
}
