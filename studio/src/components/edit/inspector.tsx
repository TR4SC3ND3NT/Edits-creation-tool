import * as React from "react"
import {
  closestCenter,
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core"
import { restrictToVerticalAxis } from "@dnd-kit/modifiers"
import {
  arrayMove,
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import {
  ArrowDownIcon,
  ArrowUpIcon,
  ChevronRightIcon,
  CopyIcon,
  EllipsisIcon,
  GripVerticalIcon,
  MousePointerClickIcon,
  PlusIcon,
  Trash2Icon,
  TriangleAlertIcon,
  XIcon,
} from "lucide-react"

import { useEditor } from "@/components/edit/editor-context"
import {
  Choice,
  FieldRow,
  PropRow,
  Section,
  UnitInput,
} from "@/components/edit/fields"
import {
  CompSection,
  CubeSection,
  MaskSection,
  MatteSection,
  ParticlesSection,
  PictureSection,
  ShapeSection,
  SolidSection,
  TextSection,
  useLayerEdit,
} from "@/components/edit/kind-sections"
import { KindIcon } from "@/components/edit/kind-icon"
import { useInspection } from "@/components/edit/use-inspection"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
import {
  Item,
  ItemContent,
  ItemDescription,
  ItemMedia,
  ItemTitle,
} from "@/components/ui/item"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Switch } from "@/components/ui/switch"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  api,
  type EffectDto,
  type Inspection,
  type PropDto,
  type PropKey,
} from "@/lib/api"
import { CATEGORY_LABEL, useEffects, usePresets } from "@/lib/library"
import { useStudio } from "@/lib/store"
import { firstFrameFrom, fmtTime, tidy } from "@/lib/time"
import { cn } from "@/lib/utils"

const BLENDS = [
  "normal",
  "add",
  "screen",
  "multiply",
  "overlay",
  "lighten",
  "darken",
  "difference",
  "color_burn",
  "linear_burn",
  "darker_color",
  "color_dodge",
  "linear_dodge",
  "lighter_color",
  "soft_light",
  "hard_light",
  "vivid_light",
  "linear_light",
  "pin_light",
  "hard_mix",
  "exclusion",
  "subtract",
  "divide",
  "hue",
  "saturation",
  "color",
  "luminosity",
]
const EASES = [
  "linear",
  "hold",
  "in",
  "out",
  "in_out",
  "quad_out",
  "quint_out",
  "expo_in",
  "expo_out",
  "sine_in_out",
  "back_out",
  "elastic_out",
  "bounce_out",
  "decay",
  "bezier",
]

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Raw = Record<string, any>

const sameKey = (a: PropKey | undefined, b: PropKey | undefined) =>
  !!a && !!b && JSON.stringify(a) === JSON.stringify(b)

/** What every property row needs: pick, commit, keys and the live preview. */
function useProps(layerId: string | null) {
  const s = useStudio()
  const ed = useEditor()
  return (p: PropDto, effect: number | null) => ({
    prop: p,
    picked: sameKey(p.key, s.picked?.key),
    onPick: () =>
      s.setPicked(
        sameKey(s.picked?.key, p.key)
          ? null
          : { key: p.key, pointer: p.pointer, effect, keyIndex: p.key_here }
      ),
    onCommit: (value: number[]) =>
      void s.run(api.propSet(layerId, p.key, s.t, value)),
    onKeyHere: () => void s.run(api.propKeyHere(layerId, p.key, s.t)),
    onRemoveKey: () => void s.run(api.propRemoveKey(layerId, p.key, s.t)),
    onPreview: (value: number[]) => ed.previewProp(layerId, p.key, value),
    onPreviewEnd: () => {},
  })
}

// ---- keys of the picked property ------------------------------------------------------------

function KeysTable({ prop, layer }: { prop: PropDto; layer: string | null }) {
  const s = useStudio()
  if (!prop.keys.length)
    return (
      <p className="px-1 text-sm text-muted-foreground">
        {prop.name} is not animated. Click its diamond to add a key at the
        playhead.
      </p>
    )
  return (
    <div className="overflow-hidden rounded-lg border">
      <Table>
        <TableHeader className="bg-muted">
          <TableRow>
            <TableHead className="w-14">Time</TableHead>
            <TableHead>Value</TableHead>
            <TableHead className="w-28">Ease out</TableHead>
            <TableHead className="w-8" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {prop.keys.map((k, i) => (
            <TableRow
              key={`${k.t}-${i}`}
              data-state={prop.key_here === i ? "selected" : undefined}
            >
              <TableCell className="tabular-nums">
                <Button
                  variant="link"
                  size="xs"
                  className="px-0"
                  onClick={() => s.info && s.seek(firstFrameFrom(s.info, k.t))}
                >
                  {tidy(k.t, 3)}
                </Button>
              </TableCell>
              <TableCell className="max-w-24 truncate tabular-nums">
                {k.v.map((v) => tidy(v)).join(", ")}
              </TableCell>
              <TableCell>
                <Choice
                  value={k.ease}
                  options={EASES}
                  onChange={(v) =>
                    void s.run(api.propEase(layer, prop.key, i, v))
                  }
                  label="Ease"
                />
              </TableCell>
              <TableCell>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label="Remove key"
                  onClick={() =>
                    void s.run(api.propRemoveKey(layer, prop.key, k.t))
                  }
                >
                  <XIcon />
                </Button>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

// ---- effects ---------------------------------------------------------------------------------

function AddEffect({ layer }: { layer: string }) {
  const s = useStudio()
  const effects = useEffects()
  const [open, setOpen] = React.useState(false)
  const groups = Object.entries(
    effects
      .filter((e) => !e.needs_source)
      .reduce<Record<string, typeof effects>>((acc, e) => {
        ;(acc[e.category] ??= []).push(e)
        return acc
      }, {})
  )
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="icon-xs" aria-label="Add effect" />
        }
      >
        <PlusIcon />
      </PopoverTrigger>
      <PopoverContent className="w-72 p-0" align="end">
        <Command>
          <CommandInput placeholder="Search effects…" />
          <CommandList>
            <CommandEmpty>No effect found.</CommandEmpty>
            {groups.map(([cat, list]) => (
              <CommandGroup key={cat} heading={CATEGORY_LABEL[cat] ?? cat}>
                {list.map((e) => (
                  <CommandItem
                    key={e.kind}
                    value={`${e.kind} ${e.category}`}
                    onSelect={() => {
                      setOpen(false)
                      void s.run(api.effectAdd(layer, e.kind))
                    }}
                  >
                    <span className="truncate">
                      {e.kind.replaceAll("_", " ")}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function EffectCard({
  e,
  layer,
  count,
  rowProps,
}: {
  e: EffectDto
  layer: string
  count: number
  rowProps: ReturnType<typeof useProps>
}) {
  const s = useStudio()
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: String(e.index) })
  const animated = e.params.filter((p) => p.animated).length
  return (
    <Collapsible
      defaultOpen={count <= 2}
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      data-slot="effect-card"
      className={cn(
        "group/fx rounded-lg border bg-card",
        !e.enabled && "opacity-60",
        isDragging && "z-10 shadow-md"
      )}
    >
      <div className="flex h-9 items-center gap-1 pr-1 pl-0.5">
        <button
          type="button"
          className="flex h-7 w-5 cursor-grab items-center justify-center text-muted-foreground active:cursor-grabbing"
          aria-label="Drag to reorder"
          {...attributes}
          {...listeners}
        >
          <GripVerticalIcon className="size-3.5" />
        </button>
        <CollapsibleTrigger
          render={
            <button
              type="button"
              className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-sm font-medium"
            />
          }
        >
          <ChevronRightIcon className="size-4 shrink-0 text-muted-foreground transition-transform group-data-[open]/fx:rotate-90" />
          <span className="truncate">{e.kind.replaceAll("_", " ")}</span>
          <span className="truncate text-xs font-normal text-muted-foreground">
            {CATEGORY_LABEL[e.category] ?? e.category}
          </span>
          {animated > 0 && (
            <Badge variant="outline" className="h-4 px-1 text-[10px]">
              {animated} keyed
            </Badge>
          )}
        </CollapsibleTrigger>
        <Switch
          size="sm"
          checked={e.enabled}
          onCheckedChange={() => void s.run(api.effectToggle(layer, e.index))}
          aria-label="Enabled"
        />
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button variant="ghost" size="icon-xs" aria-label="Effect menu" />
            }
          >
            <EllipsisIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-40">
            <DropdownMenuItem
              disabled={e.index === 0}
              onClick={() => void s.run(api.effectMove(layer, e.index, true))}
            >
              <ArrowUpIcon />
              Move up
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={e.index === count - 1}
              onClick={() => void s.run(api.effectMove(layer, e.index, false))}
            >
              <ArrowDownIcon />
              Move down
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              variant="destructive"
              onClick={() => void s.run(api.effectRemove(layer, e.index))}
            >
              <Trash2Icon />
              Remove
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <CollapsibleContent className="flex flex-col gap-0.5 px-1.5 pb-2">
        <p className="px-1 pb-1 text-xs text-muted-foreground">{e.doc}</p>
        {e.params.map((p) => (
          <PropRow key={p.name} {...rowProps(p, e.index)} />
        ))}
        {e.options.map((o) => (
          <FieldRow
            key={o.name}
            label={o.name.replaceAll("_", " ")}
            hint={o.doc}
          >
            <Choice
              value={o.value}
              options={o.values}
              onChange={(v) =>
                void s.run(api.effectOption(layer, e.index, o.name, v))
              }
            />
          </FieldRow>
        ))}
      </CollapsibleContent>
    </Collapsible>
  )
}

function EffectsSection({
  data,
  raw,
}: {
  data: Inspection
  raw: Raw | undefined
}) {
  const s = useStudio()
  const l = data.layer!
  const rowProps = useProps(l.id)
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } })
  )
  const id = React.useId()
  const onDragEnd = (ev: DragEndEvent) => {
    const { active, over } = ev
    if (!over || active.id === over.id || !raw) return
    const from = Number(active.id)
    const to = Number(over.id)
    const list = (raw.effects as unknown[]) ?? []
    void s.run(
      api.editOps(
        [
          {
            op: "set",
            pointer: `/layers/@${l.id}/effects`,
            value: arrayMove(list, from, to),
          },
        ],
        `${l.id}: effect ${from + 1} to place ${to + 1}`
      )
    )
  }
  return (
    <Section
      title="Effects"
      count={data.effects.length}
      action={<AddEffect layer={l.id} />}
    >
      {data.effects.length === 0 ? (
        <p className="px-1 text-sm text-muted-foreground">
          None. Drag one from the Effects tab of the browser onto the clip or
          the viewer, double-click it there, or press +.
        </p>
      ) : (
        <p className="px-1 pb-1 text-xs text-muted-foreground">
          Applied top to bottom; drag the handle to reorder.
        </p>
      )}
      <DndContext
        id={id}
        sensors={sensors}
        collisionDetection={closestCenter}
        modifiers={[restrictToVerticalAxis]}
        onDragEnd={onDragEnd}
      >
        <SortableContext
          items={data.effects.map((e) => String(e.index))}
          strategy={verticalListSortingStrategy}
        >
          <div className="flex flex-col gap-1.5">
            {data.effects.map((e) => (
              <EffectCard
                key={`${e.index}-${e.kind}`}
                e={e}
                layer={l.id}
                count={data.effects.length}
                rowProps={rowProps}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>
    </Section>
  )
}

// ---- transition --------------------------------------------------------------------------------

function TransitionSection({ data }: { data: Inspection }) {
  const s = useStudio()
  const presets = usePresets(s.snapshot?.path ?? null)
  const l = data.layer!
  const ed = useLayerEdit(l.id)
  const kinds = presets
    .filter((p) => p.transition)
    .map((p) => p.name.replace("transition:", ""))
  const beats = s.info?.time_base === "beats" ? "beats" : "s"
  return (
    <Section
      title="Transition in"
      defaultOpen={!!l.transition}
      action={
        l.transition ? (
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Remove the transition"
            onClick={() =>
              ed.remove("transition", `remove the transition into ${l.id}`)
            }
          >
            <Trash2Icon />
          </Button>
        ) : null
      }
    >
      {l.transition ? (
        <>
          <FieldRow label="kind">
            <Choice
              value={l.transition.kind}
              options={kinds}
              onChange={(v) =>
                ed.set("transition/kind", v, `transition into ${l.id}: ${v}`)
              }
            />
          </FieldRow>
          <FieldRow label="duration">
            <UnitInput
              className="flex-1"
              value={l.transition.duration}
              unit={beats}
              step={0.25}
              label="duration"
              onCommit={(v) =>
                ed.set(
                  "transition/duration",
                  v,
                  `transition into ${l.id}: duration ${v}`
                )
              }
            />
          </FieldRow>
          <FieldRow label="ease">
            <Choice
              value={l.transition.ease || "linear"}
              options={EASES.filter((e) => e !== "decay" && e !== "bezier")}
              onChange={(v) => ed.set("transition/ease", v)}
            />
          </FieldRow>
          {l.transition.doc && (
            <p className="px-1 text-xs text-muted-foreground">
              {l.transition.doc}
            </p>
          )}
        </>
      ) : (
        <FieldRow label="none">
          <Choice
            value={null}
            placeholder="Add transition…"
            options={kinds}
            onChange={(v) =>
              void s.run(
                api.presetApply({
                  name: `transition:${v}`,
                  target: "selected",
                  selected: l.id,
                  at: "cut",
                  playhead: s.t,
                  values: { align: "after" },
                })
              )
            }
          />
        </FieldRow>
      )}
    </Section>
  )
}

// ---- layer --------------------------------------------------------------------------------------

function LayerSection({
  data,
  raw,
}: {
  data: Inspection
  raw: Raw | undefined
}) {
  const s = useStudio()
  const l = data.layer!
  const ed = useLayerEdit(l.id)
  const unit = s.info?.time_base === "beats" ? "beats" : "s"
  const opacity = data.transform.find((p) => p.name === "opacity")
  const rowProps = useProps(l.id)
  return (
    <Section title="Layer">
      <FieldRow label="in · out" hint="When the layer starts and ends.">
        <UnitInput
          className="flex-1"
          value={l.start}
          unit={unit}
          step={0.25}
          label="in"
          onCommit={(v) => ed.set("in", v, `in of ${l.id}: ${v}`)}
        />
        <UnitInput
          className="flex-1"
          value={l.end ?? s.info?.duration ?? 0}
          unit={unit}
          step={0.25}
          label="out"
          onCommit={(v) => ed.set("out", v, `out of ${l.id}: ${v}`)}
        />
      </FieldRow>
      <FieldRow label="blend">
        <Choice
          value={l.blend}
          options={BLENDS}
          onChange={(v) => ed.set("blend", v, `blend of ${l.id}: ${v}`)}
        />
      </FieldRow>
      {opacity && <PropRow {...rowProps(opacity, null)} />}
      <FieldRow
        label="visible"
        hint="Off: the layer is not drawn but still works as a track matte."
      >
        <Switch
          size="sm"
          checked={raw?.visible !== false}
          onCheckedChange={(on) =>
            on ? ed.remove("visible") : ed.set("visible", false)
          }
        />
      </FieldRow>
    </Section>
  )
}

function KindSection({ raw }: { raw: Raw }) {
  switch (raw.kind) {
    case "image":
    case "video":
      return <PictureSection raw={raw} />
    case "text":
      return <TextSection raw={raw} />
    case "shape":
      return <ShapeSection raw={raw} />
    case "particles":
      return <ParticlesSection raw={raw} />
    case "solid":
      return <SolidSection raw={raw} />
    case "cube":
      return <CubeSection raw={raw} />
    case "comp":
      return <CompSection raw={raw} />
    case "adjustment":
      return (
        <Section title="Adjustment">
          <p className="px-1 text-sm text-muted-foreground">
            Its effects apply to every layer beneath it while it is on.
          </p>
        </Section>
      )
    default:
      return null
  }
}

function ProjectSummary() {
  const { snapshot, info } = useStudio()
  if (!snapshot || !info) return null
  const rows: [string, string][] = [
    ["Size", `${info.width} × ${info.height}`],
    ["Frame rate", `${info.fps} fps`],
    [
      "Length",
      `${tidy(info.duration, 3)} ${info.time_base} · ${tidy(info.seconds, 2)} s`,
    ],
    [
      "Tempo",
      info.bpm
        ? `${tidy(info.bpm, 2)} BPM, beat 0 at ${tidy(info.beat_offset, 3)} s`
        : "seconds project",
    ],
    [
      "Audio",
      info.audio
        ? `${info.audio.path.split("/").pop()} from ${tidy(info.audio.offset, 3)} s`
        : "none",
    ],
    ["Motion blur", info.motion_blur ? "on" : "off"],
  ]
  return (
    <div className="flex flex-col gap-1">
      {rows.map(([k, v]) => (
        <div key={k} className="flex items-baseline gap-2 px-1 text-sm">
          <span className="w-24 shrink-0 text-muted-foreground">{k}</span>
          <span className="truncate tabular-nums">{v}</span>
        </div>
      ))}
    </div>
  )
}

/** The inspector: what is selected, grouped like the edit apps (Layer, the kind's own, Transform, Effects, Transition, Masks, Matte, Keys). */
export function Inspector() {
  const s = useStudio()
  const { data, error } = useInspection()
  const layerId = s.layer
  const problems = s.snapshot?.problems ?? []
  const rowProps = useProps(layerId)
  const all = data
    ? [
        ...data.transform,
        ...data.effects.flatMap((e) => e.params),
        ...data.camera,
      ]
    : []
  const picked = all.find((p) => sameKey(p.key, s.picked?.key)) ?? null

  const problemList = problems.length > 0 && (
    <Section title="Problems" count={problems.length}>
      {problems.slice(0, 8).map((p, i) => (
        <Item key={i} variant="outline" size="sm">
          <ItemMedia>
            <TriangleAlertIcon
              className={p.error ? "text-destructive" : "text-muted-foreground"}
            />
          </ItemMedia>
          <ItemContent>
            <ItemTitle className="text-xs">{p.path}</ItemTitle>
            <ItemDescription className="text-xs">
              {p.message}
              {p.hint ? ` — ${p.hint}` : ""}
            </ItemDescription>
          </ItemContent>
        </Item>
      ))}
    </Section>
  )

  if (!layerId && !s.camera) {
    return (
      <ScrollArea className="h-full" data-slot="inspector">
        <div className="flex flex-col">
          <div className="p-3">
            <Empty className="border border-dashed p-5">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <MousePointerClickIcon />
                </EmptyMedia>
                <EmptyTitle>Nothing selected</EmptyTitle>
                <EmptyDescription>
                  Click a clip on the timeline (or the picture in the viewer) to
                  edit it here.
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          </div>
          {problemList}
          <Section
            title="Project"
            action={
              <Button
                variant="ghost"
                size="xs"
                onClick={() => s.select(null, true)}
              >
                Camera
              </Button>
            }
          >
            <ProjectSummary />
          </Section>
        </div>
      </ScrollArea>
    )
  }

  const l = data?.layer
  const raw = l
    ? ((s.snapshot?.doc?.layers as Raw[] | undefined) ?? []).find(
        (x) => x.id === l.id
      )
    : undefined
  const transform = l ? data!.transform.filter((p) => p.name !== "opacity") : []
  return (
    <div className="flex h-full min-h-0 flex-col" data-slot="inspector">
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <KindIcon
          kind={l?.kind ?? "camera"}
          className="size-4 shrink-0 text-muted-foreground"
        />
        <span className="truncate font-medium">{l ? l.id : "Camera"}</span>
        {l && (
          <Badge variant="outline" className="shrink-0">
            {l.kind}
          </Badge>
        )}
        {l && !l.visible_now && (
          <Tooltip>
            <TooltipTrigger
              render={<Badge variant="secondary" className="shrink-0" />}
            >
              off screen
            </TooltipTrigger>
            <TooltipContent>
              The playhead is outside this layer: values are shown at{" "}
              {fmtTime(s.info, s.t)}
            </TooltipContent>
          </Tooltip>
        )}
        <div className="ml-auto flex items-center gap-1">
          {l && (
            <Tooltip>
              <TooltipTrigger render={<span className="flex items-center" />}>
                <Switch
                  size="sm"
                  checked={l.enabled}
                  onCheckedChange={() => void s.run(api.layerToggle(l.id))}
                  aria-label="Enabled"
                />
              </TooltipTrigger>
              <TooltipContent>
                {l.enabled ? "On: click to turn off" : "Off: click to turn on"}
              </TooltipContent>
            </Tooltip>
          )}
          {l && (
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label="Layer menu"
                  />
                }
              >
                <EllipsisIcon />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-44">
                <DropdownMenuItem
                  onClick={() => void s.run(api.layerDuplicate(l.id))}
                >
                  <CopyIcon />
                  Duplicate
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => void s.run(api.layerMove(l.id, true))}
                >
                  <ArrowUpIcon />
                  Bring forward
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => void s.run(api.layerMove(l.id, false))}
                >
                  <ArrowDownIcon />
                  Send backward
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onClick={() => void s.run(api.layerRemove(l.id))}
                >
                  <Trash2Icon />
                  Delete
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Deselect"
            onClick={() => s.select(null)}
          >
            <XIcon />
          </Button>
        </div>
      </div>
      {error && <p className="px-4 pt-3 text-sm text-destructive">{error}</p>}
      {data && (
        <ScrollArea className="min-h-0 flex-1">
          <div className="flex flex-col pb-6">
            <p className="px-4 pt-2 pb-1 text-xs text-muted-foreground tabular-nums">
              Values at {fmtTime(s.info, s.t)} · frame {s.frame}. A filled
              diamond is a key here.
            </p>
            {l && <LayerSection data={data} raw={raw} />}
            {l && raw && <KindSection raw={raw} />}
            <Section title={l ? "Transform" : "Camera"}>
              {(l ? transform : data.camera).map((p) => (
                <PropRow key={p.name} {...rowProps(p, null)} />
              ))}
            </Section>
            {l && <EffectsSection data={data} raw={raw} />}
            {l && <TransitionSection data={data} />}
            {l && raw && <MaskSection raw={raw} />}
            {l && raw && <MatteSection raw={raw} />}
            <Section
              title={
                picked
                  ? `Keys · ${picked.pointer.replace(/^\/layers\/@[^/]+\//, "")}`
                  : "Keys"
              }
              defaultOpen={false}
            >
              {picked ? (
                <KeysTable prop={picked} layer={layerId} />
              ) : (
                <p className="px-1 text-sm text-muted-foreground">
                  Click a property's name to list its keys here, with the ease
                  that leaves each one.
                </p>
              )}
            </Section>
            {problemList}
          </div>
        </ScrollArea>
      )}
    </div>
  )
}
