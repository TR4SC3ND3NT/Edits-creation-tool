// The inspector's sections for what a layer kind has of its own: the
// picture of an image or video, the words of a text, the shapes of a shape
// layer, the emitter of particles, the colour of a solid, the faces of a
// cube, the comp of a comp layer; and the masks and the track matte every
// layer can have. These are plain fields of the layer's JSON: each change
// is one `set` (or `remove`) through `edit_ops`, one entry of the history.

import { open } from "@tauri-apps/plugin-dialog"
import { convertFileSrc } from "@tauri-apps/api/core"
import {
  CircleIcon,
  ImageUpIcon,
  MinusIcon,
  PlusIcon,
  SquareIcon,
  StarIcon,
  Trash2Icon,
} from "lucide-react"

import {
  Choice,
  ColorField,
  FieldRow,
  Section,
  UnitInput,
} from "@/components/edit/fields"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { tidy } from "@/lib/time"

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Raw = Record<string, any>

const IMAGE_EXT = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"]

export async function pickPicture(): Promise<string | null> {
  const p = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Pictures", extensions: IMAGE_EXT }],
  })
  return typeof p === "string" ? p : null
}

/** Edit fields of one layer by JSON pointer. */
export function useLayerEdit(id: string) {
  const s = useStudio()
  const base = `/layers/@${id}`
  return {
    set: (field: string, value: unknown, label?: string) =>
      void s.run(
        api.editOps(
          [{ op: "set", pointer: `${base}/${field}`, value }],
          label ??
            `${id}: ${field.replaceAll("/", ".")} = ${JSON.stringify(value)}`
        )
      ),
    remove: (field: string, label?: string) =>
      void s.run(
        api.editOps(
          [{ op: "remove", pointer: `${base}/${field}` }],
          label ?? `${id}: remove ${field.replaceAll("/", ".")}`
        )
      ),
  }
}

/** A number of the layer's JSON: a field, or a badge when it is keyframed (edit those keys in the file). */
function Num({
  v,
  dflt,
  unit,
  step,
  label,
  onCommit,
}: {
  v: unknown
  dflt: number
  unit?: string
  step?: number
  label: string
  onCommit: (n: number) => void
}) {
  if (v != null && typeof v === "object")
    return (
      <Badge variant="outline" title="Keyframed in the file">
        animated · {Array.isArray((v as Raw).keys) ? (v as Raw).keys.length : 0}{" "}
        keys
      </Badge>
    )
  return (
    <UnitInput
      className="flex-1"
      value={typeof v === "number" ? v : dflt}
      unit={unit}
      step={step}
      label={label}
      onCommit={onCommit}
    />
  )
}

function colorOf(v: unknown, dflt: number[]): number[] | null {
  if (Array.isArray(v) && v.every((x) => typeof x === "number")) return v
  if (v == null) return dflt
  return null
}

// ---- pictures ------------------------------------------------------------------------------

export function PictureSection({ raw }: { raw: Raw }) {
  const s = useStudio()
  const ed = useLayerEdit(raw.id)
  const slots = s.snapshot?.slots ?? []
  const slot = slots.find((x) => x.name === raw.slot)
  const video = raw.kind === "video"
  return (
    <Section title={video ? "Video" : "Picture"}>
      <div className="flex gap-3 px-1 pb-2">
        <div className="aspect-video w-28 shrink-0 overflow-hidden rounded-md border bg-muted">
          {slot?.file && slot.exists && !video && (
            <img
              src={convertFileSrc(slot.file)}
              alt=""
              className="size-full object-cover"
            />
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <Choice
            label="Slot"
            value={raw.slot ?? null}
            options={slots.map((x) => x.name)}
            onChange={(v) => ed.set("slot", v, `slot of ${raw.id}: ${v}`)}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={!slot || video}
            onClick={() =>
              pickPicture().then(
                (p) => p && void s.run(api.slotReplace(raw.slot, p))
              )
            }
          >
            <ImageUpIcon data-icon="inline-start" />
            Replace picture…
          </Button>
          <p
            className="truncate text-xs text-muted-foreground"
            title={slot?.path ?? ""}
          >
            {slot?.path ?? "empty slot (placeholder)"}
          </p>
        </div>
      </div>
      {video && (
        <>
          <FieldRow
            label="speed"
            hint="Playback rate: 0.5 half speed, 2 double."
          >
            <Num
              v={raw.speed}
              dflt={1}
              unit="×"
              step={0.05}
              label="speed"
              onCommit={(n) => ed.set("speed", n)}
            />
          </FieldRow>
          <FieldRow
            label="time remap"
            hint="Seconds of the clip to show, as a function of project time; overrides speed. Keyed in the file."
          >
            {raw.time_remap ? (
              <>
                <Badge variant="outline">keyed</Badge>
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={() => ed.remove("time_remap")}
                >
                  Remove
                </Button>
              </>
            ) : (
              <span className="text-sm text-muted-foreground">none</span>
            )}
          </FieldRow>
          <FieldRow
            label="frame blend"
            hint="What shows between two source frames in slow motion."
          >
            <Choice
              value={raw.frame_blend ?? "none"}
              options={["none", "mix", "flow"]}
              onChange={(v) => ed.set("frame_blend", v)}
            />
          </FieldRow>
          <FieldRow label="reverse">
            <Switch
              size="sm"
              checked={!!raw.reverse}
              onCheckedChange={(v) => ed.set("reverse", v)}
            />
          </FieldRow>
          <FieldRow label="loop">
            <Switch
              size="sm"
              checked={!!raw.loop}
              onCheckedChange={(v) => ed.set("loop", v)}
            />
          </FieldRow>
          <FieldRow label="volume">
            <Num
              v={raw.volume}
              dflt={1}
              step={0.1}
              label="volume"
              onCommit={(n) => ed.set("volume", n)}
            />
          </FieldRow>
        </>
      )}
      <FieldRow
        label="frame rate"
        hint="Posterize time: hold each picture for 1/fps s (12 = on twos). Empty: the project's."
      >
        <Num
          v={raw.fps}
          dflt={s.info?.fps ?? 0}
          unit="fps"
          step={1}
          label="fps"
          onCommit={(n) => (n > 0 ? ed.set("fps", n) : ed.remove("fps"))}
        />
      </FieldRow>
    </Section>
  )
}

// ---- solid ---------------------------------------------------------------------------------

export function SolidSection({ raw }: { raw: Raw }) {
  const ed = useLayerEdit(raw.id)
  const c = colorOf(raw.color, [0.5, 0.5, 0.5, 1])
  return (
    <Section title="Solid">
      <FieldRow label="colour">
        {c ? (
          <ColorField
            value={c}
            label="colour"
            onCommit={(v) => ed.set("color", v)}
          />
        ) : (
          <Badge variant="outline">animated</Badge>
        )}
      </FieldRow>
    </Section>
  )
}

// ---- text ----------------------------------------------------------------------------------

const WEIGHTS = ["100", "200", "300", "400", "500", "600", "700", "800", "900"]

export function TextSection({ raw }: { raw: Raw }) {
  const ed = useLayerEdit(raw.id)
  const fill = colorOf(raw.fill, [1, 1, 1])
  const stroke: Raw | null = raw.stroke ?? null
  const animators: Raw[] = raw.animators ?? []
  return (
    <>
      <Section title="Text">
        <div className="px-1 pb-1">
          <Textarea
            key={raw.text}
            defaultValue={raw.text}
            rows={2}
            className="min-h-14 text-sm"
            aria-label="Text"
            onBlur={(e) =>
              e.currentTarget.value !== raw.text &&
              ed.set("text", e.currentTarget.value, `text of ${raw.id}`)
            }
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey))
                e.currentTarget.blur()
            }}
          />
        </div>
        <FieldRow
          label="font"
          hint="A .ttf or .otf file next to the project; empty: Inter (built in)."
        >
          <span className="truncate text-sm">{raw.font ?? "Inter"}</span>
          {raw.font && (
            <Button variant="ghost" size="xs" onClick={() => ed.remove("font")}>
              Inter
            </Button>
          )}
        </FieldRow>
        <FieldRow label="size" hint="The em, in % of the frame's height.">
          <Num
            v={raw.size}
            dflt={10}
            unit="%h"
            step={0.5}
            label="size"
            onCommit={(n) => ed.set("size", n)}
          />
        </FieldRow>
        <FieldRow label="weight">
          <Choice
            value={raw.weight != null ? String(raw.weight) : null}
            placeholder="font default"
            options={WEIGHTS}
            onChange={(v) => ed.set("weight", Number(v))}
          />
        </FieldRow>
        <FieldRow label="align">
          <ToggleGroup
            multiple={false}
            variant="outline"
            size="sm"
            value={[raw.align ?? "center"]}
            onValueChange={(v) => v[0] && ed.set("align", v[0])}
          >
            <ToggleGroupItem value="left" className="px-2 text-xs">
              Left
            </ToggleGroupItem>
            <ToggleGroupItem value="center" className="px-2 text-xs">
              Centre
            </ToggleGroupItem>
            <ToggleGroupItem value="right" className="px-2 text-xs">
              Right
            </ToggleGroupItem>
          </ToggleGroup>
        </FieldRow>
        <FieldRow
          label="tracking"
          hint="Extra space after every character, % of height."
        >
          <Num
            v={raw.tracking}
            dflt={0}
            unit="%h"
            step={0.1}
            label="tracking"
            onCommit={(n) => ed.set("tracking", n)}
          />
        </FieldRow>
        <FieldRow
          label="line height"
          hint="Baseline to baseline, in multiples of the size."
        >
          <Num
            v={raw.line_height}
            dflt={1.2}
            unit="×"
            step={0.05}
            label="line height"
            onCommit={(n) => ed.set("line_height", n)}
          />
        </FieldRow>
        <FieldRow label="fill">
          {fill ? (
            <ColorField
              value={fill}
              label="fill"
              onCommit={(v) => ed.set("fill", v)}
            />
          ) : (
            <Badge variant="outline">animated</Badge>
          )}
        </FieldRow>
        <FieldRow
          label="stroke"
          hint="An outline beneath the letters, width in % of height."
        >
          <Switch
            size="sm"
            checked={!!stroke}
            onCheckedChange={(on) =>
              on
                ? ed.set("stroke", { color: [0, 0, 0], width: 0.6 })
                : ed.remove("stroke")
            }
          />
          {stroke && (
            <>
              <ColorField
                value={colorOf(stroke.color, [0, 0, 0]) ?? [0, 0, 0]}
                label="stroke colour"
                onCommit={(v) => ed.set("stroke/color", v)}
              />
              <UnitInput
                className="w-20"
                value={typeof stroke.width === "number" ? stroke.width : 0.6}
                unit="%h"
                step={0.1}
                label="stroke width"
                onCommit={(n) => ed.set("stroke/width", n)}
              />
            </>
          )}
        </FieldRow>
      </Section>
      <Section
        title="Animators"
        count={animators.length}
        defaultOpen={animators.length > 0}
      >
        {animators.length === 0 && (
          <p className="px-1 text-sm text-muted-foreground">
            Letter-by-letter animation. Apply a text preset (pop in, typewriter,
            wave) from the Text tab of the browser.
          </p>
        )}
        {animators.map((a, i) => (
          <div key={i} className="flex min-h-8 items-center gap-2 px-1">
            <span className="flex-1 truncate text-sm">
              {a.name ?? `animator ${i + 1}`}
            </span>
            <Badge variant="outline">{a.selector?.kind ?? "range"}</Badge>
            <Switch
              size="sm"
              checked={a.enabled !== false}
              onCheckedChange={(on) => ed.set(`animators/${i}/enabled`, on)}
              aria-label="Enabled"
            />
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Remove the animator"
              onClick={() =>
                ed.remove(
                  `animators/${i}`,
                  `${raw.id}: remove animator ${i + 1}`
                )
              }
            >
              <Trash2Icon />
            </Button>
          </div>
        ))}
      </Section>
    </>
  )
}

// ---- shapes --------------------------------------------------------------------------------

const NEW_SHAPES: Record<string, Raw> = {
  rect: {
    type: "rect",
    size: [0.4, 0.25],
    stroke: { color: [1, 1, 1], width: 0.8 },
  },
  ellipse: {
    type: "ellipse",
    size: [0.3, 0.3],
    stroke: { color: [1, 1, 1], width: 0.8 },
  },
  polystar: {
    type: "polystar",
    points: 5,
    outer_radius: 0.15,
    inner_radius: 0.06,
    fill: { color: [1, 1, 1] },
  },
  line: {
    type: "line",
    from: [-0.25, 0],
    to: [0.25, 0],
    stroke: { color: [1, 1, 1], width: 0.6, cap: "round" },
  },
}

export function ShapeSection({ raw }: { raw: Raw }) {
  const ed = useLayerEdit(raw.id)
  const shapes: Raw[] = raw.shapes ?? []
  return (
    <Section
      title="Shapes"
      count={shapes.length}
      action={
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button variant="ghost" size="icon-xs" aria-label="Add a shape" />
            }
          >
            <PlusIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {[
              ["rect", "Rectangle", SquareIcon],
              ["ellipse", "Ellipse", CircleIcon],
              ["polystar", "Star", StarIcon],
              ["line", "Line", MinusIcon],
            ].map(([k, label, Icon]) => {
              const I = Icon as typeof SquareIcon
              return (
                <DropdownMenuItem
                  key={k as string}
                  onClick={() =>
                    ed.set(
                      "shapes/-",
                      NEW_SHAPES[k as string],
                      `${raw.id}: add a ${label}`
                    )
                  }
                >
                  <I />
                  {label as string}
                </DropdownMenuItem>
              )
            })}
          </DropdownMenuContent>
        </DropdownMenu>
      }
    >
      {shapes.map((sh, i) => {
        const fill = sh.fill ? colorOf(sh.fill.color, [1, 1, 1]) : null
        const stroke = sh.stroke ? colorOf(sh.stroke.color, [1, 1, 1]) : null
        return (
          <div
            key={i}
            className="mb-1 flex flex-col gap-0.5 rounded-md border p-1.5"
          >
            <div className="flex items-center gap-2 px-1">
              <span className="flex-1 text-sm font-medium">
                {sh.type ?? "shape"} {i + 1}
              </span>
              {(sh.operators ?? []).map((o: Raw, j: number) => (
                <Badge key={j} variant="outline">
                  {o.kind}
                </Badge>
              ))}
              <Switch
                size="sm"
                checked={sh.enabled !== false}
                onCheckedChange={(on) => ed.set(`shapes/${i}/enabled`, on)}
                aria-label="Enabled"
              />
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="Remove the shape"
                onClick={() =>
                  ed.remove(`shapes/${i}`, `${raw.id}: remove shape ${i + 1}`)
                }
              >
                <Trash2Icon />
              </Button>
            </div>
            <FieldRow label="fill">
              <Switch
                size="sm"
                checked={!!sh.fill}
                onCheckedChange={(on) =>
                  on
                    ? ed.set(`shapes/${i}/fill`, { color: [1, 1, 1] })
                    : ed.remove(`shapes/${i}/fill`)
                }
              />
              {sh.fill && fill && (
                <ColorField
                  value={fill}
                  label="fill"
                  onCommit={(v) => ed.set(`shapes/${i}/fill/color`, v)}
                />
              )}
            </FieldRow>
            <FieldRow label="stroke">
              <Switch
                size="sm"
                checked={!!sh.stroke}
                onCheckedChange={(on) =>
                  on
                    ? ed.set(`shapes/${i}/stroke`, {
                        color: [1, 1, 1],
                        width: 0.8,
                      })
                    : ed.remove(`shapes/${i}/stroke`)
                }
              />
              {sh.stroke && stroke && (
                <>
                  <ColorField
                    value={stroke}
                    label="stroke"
                    onCommit={(v) => ed.set(`shapes/${i}/stroke/color`, v)}
                  />
                  <UnitInput
                    className="w-20"
                    value={
                      typeof sh.stroke.width === "number" ? sh.stroke.width : 1
                    }
                    unit="%h"
                    step={0.1}
                    label="stroke width"
                    onCommit={(n) => ed.set(`shapes/${i}/stroke/width`, n)}
                  />
                </>
              )}
            </FieldRow>
            {(sh.type === "rect" || sh.type === "ellipse") &&
              Array.isArray(sh.size) && (
                <FieldRow label="size">
                  {[0, 1].map((k) => (
                    <UnitInput
                      key={k}
                      className="flex-1"
                      value={sh.size[k] ?? sh.size[0]}
                      unit={k ? "H" : "W"}
                      step={0.01}
                      label={`size ${k ? "height" : "width"}`}
                      onCommit={(n) =>
                        ed.set(`shapes/${i}/size`, [
                          k === 0 ? n : sh.size[0],
                          k === 1 ? n : (sh.size[1] ?? sh.size[0]),
                        ])
                      }
                    />
                  ))}
                </FieldRow>
              )}
            {sh.type === "polystar" && (
              <FieldRow label="points">
                <Num
                  v={sh.points}
                  dflt={5}
                  step={1}
                  label="points"
                  onCommit={(n) =>
                    ed.set(`shapes/${i}/points`, Math.max(3, Math.round(n)))
                  }
                />
              </FieldRow>
            )}
          </div>
        )
      })}
    </Section>
  )
}

// ---- particles -----------------------------------------------------------------------------

export function ParticlesSection({ raw }: { raw: Raw }) {
  const ed = useLayerEdit(raw.id)
  const color = colorOf(raw.color, [1, 1, 1])
  const fields: [string, number, string, number, string][] = [
    ["rate", 0, "/beat", 0.5, "Particles per time unit, evenly spaced."],
    ["life", 2, "beats", 0.25, "How long each particle lives."],
    ["speed", 0.2, "h/s", 0.02, "Initial speed, heights per second."],
    ["direction", -90, "°", 5, "0 right, 90 down, -90 up."],
    ["spread", 360, "°", 5, "Width of the cone around the direction."],
    ["size", 1, "%h", 0.1, "Diameter, % of height."],
    ["drag", 0, "", 0.1, "Velocity decays toward gravity/drag."],
    ["seed", 0, "", 1, "Another number gives other particles."],
  ]
  return (
    <Section title="Particles">
      <FieldRow label="sprite">
        <Choice
          value={raw.sprite ?? "dot"}
          options={["dot", "spark", "star", "petal"]}
          onChange={(v) => ed.set("sprite", v)}
        />
      </FieldRow>
      {fields.map(([name, dflt, unit, step, hint]) => (
        <FieldRow key={name} label={name} hint={hint}>
          <Num
            v={raw[name]}
            dflt={dflt}
            unit={unit}
            step={step}
            label={name}
            onCommit={(n) => ed.set(name, n)}
          />
        </FieldRow>
      ))}
      <FieldRow
        label="gravity"
        hint="Pull down (negative: up), heights per second squared."
      >
        <Num
          v={
            typeof raw.gravity === "number"
              ? raw.gravity
              : raw.gravity == null
                ? 0
                : {}
          }
          dflt={0}
          step={0.05}
          label="gravity"
          onCommit={(n) => ed.set("gravity", n)}
        />
      </FieldRow>
      <FieldRow label="colour">
        {color ? (
          <ColorField
            value={color}
            label="colour"
            onCommit={(v) => ed.set("color", v)}
          />
        ) : (
          <Badge variant="outline">animated</Badge>
        )}
      </FieldRow>
    </Section>
  )
}

// ---- cube ----------------------------------------------------------------------------------

const FACES = ["front", "right", "back", "left", "top", "bottom"]

export function CubeSection({ raw }: { raw: Raw }) {
  const s = useStudio()
  const ed = useLayerEdit(raw.id)
  const faces: string[] = raw.faces ?? []
  const size: number[] = Array.isArray(raw.size) ? raw.size : [0.6, 0.6, 0.6]
  const slots = (s.snapshot?.slots ?? []).map((x) => x.name)
  return (
    <Section title="Cube">
      {FACES.map((f, i) => (
        <FieldRow key={f} label={f}>
          <Choice
            value={faces[i] ?? null}
            placeholder={faces[0] ? `${faces[0]} (as front)` : "slot"}
            options={slots}
            onChange={(v) => {
              const next = [...faces]
              while (next.length < i) next.push(faces[0] ?? v)
              next[i] = v
              ed.set("faces", next, `${raw.id}: ${f} face ${v}`)
            }}
          />
        </FieldRow>
      ))}
      <FieldRow label="size" hint="Width, height, depth in composition units.">
        {["W", "H", "D"].map((a, k) => (
          <UnitInput
            key={a}
            className="flex-1"
            value={size[k] ?? 0.6}
            unit={a}
            step={0.05}
            label={`size ${a}`}
            onCommit={(n) =>
              ed.set(
                "size",
                size.map((x, j) => (j === k ? n : x))
              )
            }
          />
        ))}
      </FieldRow>
    </Section>
  )
}

// ---- comp ----------------------------------------------------------------------------------

export function CompSection({ raw }: { raw: Raw }) {
  const s = useStudio()
  const ed = useLayerEdit(raw.id)
  const comps = Object.keys((s.snapshot?.doc?.comps as Raw | undefined) ?? {})
  return (
    <Section title="Comp">
      <FieldRow label="comp">
        <Choice
          value={raw.comp ?? null}
          options={comps}
          onChange={(v) => ed.set("comp", v)}
        />
      </FieldRow>
      <FieldRow label="offset" hint="Comp time at the layer's in.">
        <Num
          v={raw.offset}
          dflt={0}
          unit="beats"
          step={0.25}
          label="offset"
          onCommit={(n) => ed.set("offset", n)}
        />
      </FieldRow>
      <FieldRow label="speed">
        <Num
          v={raw.speed}
          dflt={1}
          unit="×"
          step={0.05}
          label="speed"
          onCommit={(n) => ed.set("speed", n)}
        />
      </FieldRow>
      <FieldRow label="loop">
        <Switch
          size="sm"
          checked={!!raw.loop}
          onCheckedChange={(v) => ed.set("loop", v)}
        />
      </FieldRow>
    </Section>
  )
}

// ---- masks and matte ------------------------------------------------------------------------

export function MaskSection({ raw }: { raw: Raw }) {
  const ed = useLayerEdit(raw.id)
  const masks: Raw[] = raw.masks ?? []
  const add = (shape: string) =>
    ed.set(
      "masks/-",
      shape === "ellipse"
        ? { shape, size: [0.8, 0.8], feather: 2 }
        : { shape, size: [0.8, 0.6] },
      `${raw.id}: add a ${shape} mask`
    )
  return (
    <Section
      title="Masks"
      count={masks.length}
      defaultOpen={masks.length > 0}
      action={
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button variant="ghost" size="icon-xs" aria-label="Add a mask" />
            }
          >
            <PlusIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => add("ellipse")}>
              <CircleIcon />
              Ellipse mask
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => add("rect")}>
              <SquareIcon />
              Rectangle mask
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => add("rounded_rect")}>
              <SquareIcon />
              Rounded mask
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      }
    >
      {masks.length === 0 && (
        <p className="px-1 text-sm text-muted-foreground">
          Masks limit where the layer shows: shapes in its own space.
        </p>
      )}
      {masks.map((m, i) => (
        <div
          key={i}
          className="mb-1 flex flex-col gap-0.5 rounded-md border p-1.5"
        >
          <div className="flex items-center gap-2 px-1">
            <span className="flex-1 text-sm font-medium">
              {String(m.shape).replace("_", " ")} {i + 1}
            </span>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Remove the mask"
              onClick={() =>
                ed.remove(`masks/${i}`, `${raw.id}: remove mask ${i + 1}`)
              }
            >
              <Trash2Icon />
            </Button>
          </div>
          <FieldRow label="mode">
            <Choice
              value={m.mode ?? "add"}
              options={["add", "subtract", "intersect", "difference"]}
              onChange={(v) => ed.set(`masks/${i}/mode`, v)}
            />
          </FieldRow>
          <FieldRow label="feather" hint="Soft edge, % of height.">
            <Num
              v={m.feather}
              dflt={0}
              unit="%h"
              step={0.5}
              label="feather"
              onCommit={(n) => ed.set(`masks/${i}/feather`, n)}
            />
          </FieldRow>
          <FieldRow label="inverted">
            <Switch
              size="sm"
              checked={!!(m.inverted ?? m.invert)}
              onCheckedChange={(v) => ed.set(`masks/${i}/inverted`, v)}
            />
          </FieldRow>
        </div>
      ))}
    </Section>
  )
}

export function MatteSection({ raw }: { raw: Raw }) {
  const s = useStudio()
  const ed = useLayerEdit(raw.id)
  const others = (s.snapshot?.layers ?? [])
    .filter((l) => l.id !== raw.id && l.kind !== "adjustment")
    .map((l) => l.id)
  const matte: Raw | null = raw.matte ?? null
  return (
    <Section title="Track matte" defaultOpen={!!matte}>
      <FieldRow
        label="source"
        hint="Another layer whose picture this one is cut to."
      >
        <Choice
          value={matte?.layer ?? "none"}
          options={[
            { value: "none", label: "none" },
            ...others.map((o) => ({ value: o, label: o })),
          ]}
          onChange={(v) =>
            v === "none"
              ? ed.remove("matte", `${raw.id}: no matte`)
              : ed.set(
                  "matte",
                  { layer: v, mode: matte?.mode ?? "alpha" },
                  `${raw.id}: matte ${v}`
                )
          }
        />
      </FieldRow>
      {matte && (
        <FieldRow label="mode">
          <Choice
            value={matte.mode ?? "alpha"}
            options={["alpha", "alpha_inverted", "luma", "luma_inverted"]}
            onChange={(v) => ed.set("matte/mode", v)}
          />
        </FieldRow>
      )}
      {matte && (
        <p className="px-1 text-xs text-muted-foreground">
          Hide the source with its visible switch off (Layer section) so it only
          works as a matte.
        </p>
      )}
    </Section>
  )
}

export function tidyNum(x: number) {
  return tidy(x, 3)
}
