// Preview frames: JPEG bytes from the render thread (`frame` command),
// decoded to ImageBitmaps and kept in a small cache keyed by revision,
// frame, scale and motion blur. The Rust side has its own, larger cache of
// the encoded frames, so a bitmap dropped here comes back quickly.

import { invoke } from "@tauri-apps/api/core"

export type FrameRequest = {
  revision: number
  frame: number
  scale: number
  blur: boolean
}

const key = (r: FrameRequest) =>
  `${r.revision}:${r.frame}:${r.scale}:${r.blur ? 1 : 0}`

const MAX_BITMAPS = 240
const bitmaps = new Map<string, ImageBitmap>()
const pending = new Map<string, Promise<ImageBitmap | null>>()

function remember(k: string, b: ImageBitmap) {
  bitmaps.delete(k)
  bitmaps.set(k, b)
  while (bitmaps.size > MAX_BITMAPS) {
    const oldest = bitmaps.keys().next().value
    if (oldest === undefined) break
    bitmaps.get(oldest)?.close()
    bitmaps.delete(oldest)
  }
}

export function cachedFrame(r: FrameRequest): ImageBitmap | null {
  const k = key(r)
  const b = bitmaps.get(k)
  if (b) {
    bitmaps.delete(k)
    bitmaps.set(k, b)
  }
  return b ?? null
}

/** The newest cached frame at or before `r.frame`, within `back` frames. */
export function nearestCached(
  r: FrameRequest,
  back = 12
): { frame: number; bitmap: ImageBitmap } | null {
  for (let f = r.frame; f >= Math.max(0, r.frame - back); f--) {
    const b = bitmaps.get(key({ ...r, frame: f }))
    if (b) return { frame: f, bitmap: b }
  }
  return null
}

export function isPending(r: FrameRequest) {
  return pending.has(key(r))
}

/** Render (or fetch from a cache) one frame. `prefetch` frames wait behind the current one. Resolves to null when superseded. */
export function loadFrame(
  r: FrameRequest,
  prefetch: boolean
): Promise<ImageBitmap | null> {
  const k = key(r)
  const hit = bitmaps.get(k)
  if (hit) return Promise.resolve(hit)
  const inflight = pending.get(k)
  if (inflight && prefetch) return inflight
  const p = invoke<ArrayBuffer>("frame", { ...r, prefetch })
    .then(async (buf) => {
      const bitmap = await createImageBitmap(
        new Blob([buf], { type: "image/jpeg" })
      )
      remember(k, bitmap)
      return bitmap
    })
    .catch((e: unknown) => {
      const text = String(e)
      if (text.includes("superseded") || text.includes("stale")) return null
      throw new Error(text)
    })
    .finally(() => {
      if (pending.get(k) === p) pending.delete(k)
    })
  pending.set(k, p)
  return p
}

export function clearPrefetch() {
  invoke("prefetch_clear").catch(() => {})
}

/** Forget bitmaps of other revisions. */
export function keepRevision(revision: number) {
  for (const [k, b] of bitmaps) {
    if (!k.startsWith(`${revision}:`)) {
      b.close()
      bitmaps.delete(k)
    }
  }
}
