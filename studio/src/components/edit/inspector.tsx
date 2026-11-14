import * as React from "react"
import {
  ArrowDownIcon,
  ArrowUpIcon,
  CopyIcon,
  EllipsisIcon,
  LocateFixedIcon,
  MousePointerClickIcon,
  PlusIcon,
  Trash2Icon,
  TriangleAlertIcon,
  XIcon,
} from "lucide-react"

import { presenceFor, presenceRing, usePresenceState } from "@/agent/presence"
import { AgentAddedBadge } from "@/agent/presence-ui"
import { KindIcon } from "@/components/edit/kind-icon"
import { NumberInput, PropRow, SectionTitle } from "@/components/edit/fields"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
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
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import { Switch } from "@/components/ui/switch"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  api,
  errorText,
  type Inspection,
  type PropDto,
  type PropKey,
} from "@/lib/api"
import { CATEGORY_LABEL, useEffects, usePresets } from "@/lib/library"
import { useStudio } from "@/lib/store"
import { cn } from "@/lib/utils"
import { firstFrameFrom, fmtTime, tidy } from "@/lib/time"

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

const sameKey = (a: PropKey | undefined, b: PropKey | undefined) =>
  !!a && !!b && JSON.stringify(a) === JSON.stringify(b)

function useInspection() {
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

function KeysTable({ prop, layer }: { prop: PropDto; layer: string | null }) {
  const s = useStudio()
  if (!prop.keys.length)
    return (
      <p className="px-1 text-sm text-muted-foreground">
        {prop.name} is not animated. Use the diamond (Key here) to add a key at
        the playhead.
      </p>
    )
  return (
    <div className="overflow-hidden rounded-lg border">
      <Table>
        <TableHeader className="bg-muted">
          <TableRow>
            <TableHead className="w-16">Time</TableHead>
            <TableHead>Value</TableHead>
            <TableHead className="w-32">Ease out</TableHead>
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
              <TableCell className="truncate tabular-nums">
                {k.v.map((v) => tidy(v)).join(", ")}
              </TableCell>
              <TableCell>
                <Select
                  items={EASES.map((e) => ({ label: e, value: e }))}
                  value={k.ease}
                  onValueChange={(v) =>
                    v &&
                    v !== k.ease &&
                    void s.run(api.propEase(layer, prop.key, i, v))
                  }
                >
                  <SelectTrigger size="sm" className="h-7 w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {EASES.map((e) => (
                        <SelectItem key={e} value={e}>
                          {e}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
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
      <PopoverTrigger render={<Button variant="outline" size="sm" />}>
        <PlusIcon data-icon="inline-start" />
        Add effect
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
                    <span className="truncate">{e.kind}</span>
                    <span className="ml-auto text-xs text-muted-foreground">
                      {e.params.length} params
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

function TransitionSection({ data }: { data: Inspection }) {
  const s = useStudio()
  const presets = usePresets(s.snapshot?.path ?? null)
  const l = data.layer!
  const kinds = presets
    .filter((p) => p.transition)
    .map((p) => p.name.replace("transition:", ""))
  const pointer = `/layers/@${l.id}/transition`
  return (
    <div className="flex flex-col gap-2">
      <SectionTitle
        action={
          l.transition ? (
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Remove the transition"
              onClick={() =>
                void s.run(
                  api.editOps(
                    [{ op: "remove", pointer }],
                    `remove the transition into ${l.id}`
                  )
                )
              }
            >
              <Trash2Icon />
            </Button>
          ) : null
        }
      >
        Transition in
      </SectionTitle>
      {l.transition ? (
        <>
          <div className="flex items-center gap-2 px-1">
            <span className="w-24 shrink-0 text-sm text-muted-foreground">
              kind
            </span>
            <Select
              items={kinds.map((k) => ({ label: k, value: k }))}
              value={l.transition.kind}
              onValueChange={(v) =>
                v &&
                void s.run(
                  api.editOps(
                    [{ op: "set", pointer: `${pointer}/kind`, value: v }],
                    `transition into ${l.id}: ${v}`
                  )
                )
              }
            >
              <SelectTrigger size="sm" className="h-7 flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {kinds.map((k) => (
                    <SelectItem key={k} value={k}>
                      {k}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center gap-2 px-1">
            <span className="w-24 shrink-0 text-sm text-muted-foreground">
              duration
            </span>
            <NumberInput
              className="flex-1"
              value={l.transition.duration}
              onCommit={(v) =>
                void s.run(
                  api.editOps(
                    [{ op: "set", pointer: `${pointer}/duration`, value: v }],
                    `transition into ${l.id}: duration ${v}`
                  )
                )
              }
            />
            <span className="w-8 text-xs text-muted-foreground">
              {s.info?.time_base === "beats" ? "beats" : "s"}
            </span>
          </div>
          {l.transition.doc && (
            <p className="px-1 text-xs text-muted-foreground">
              {l.transition.doc}
            </p>
          )}
        </>
      ) : (
        <div className="flex items-center gap-2 px-1">
          <span className="flex-1 text-sm text-muted-foreground">
            None: the layer cuts in at its in.
          </span>
          <Select
            items={kinds.map((k) => ({ label: k, value: k }))}
            value={null}
            onValueChange={(v) =>
              v &&
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
          >
            <SelectTrigger size="sm" className="h-7 w-36">
              <SelectValue placeholder="Add transition" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {kinds.map((k) => (
                  <SelectItem key={k} value={k}>
                    {k}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
      )}
    </div>
  )
}

function ProjectSummary() {
  const { snapshot, info } = useStudio()
  if (!snapshot || !info) return null
  const rows: [string, string][] = [
    ["Size", `${info.width} × ${info.height}`],
    ["Frame rate", `${info.fps} fps`],
    [
      "Length",
      `${tidy(info.duration, 3)} ${info.time_base} · ${tidy(info.seconds, 2)} s · ${info.frames} frames`,
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

export function Inspector() {
  const s = useStudio()
  const agent = usePresenceState()
  const { data, error } = useInspection()
  const layerId = s.layer
  const problems = s.snapshot?.problems ?? []
  const pick = (p: PropDto, effect: number | null) =>
    s.setPicked(
      sameKey(s.picked?.key, p.key)
        ? null
        : { key: p.key, pointer: p.pointer, effect, keyIndex: p.key_here }
    )
  const commit = (p: PropDto) => (value: number[]) =>
    void s.run(api.propSet(layerId, p.key, s.t, value))
  const keyHere = (p: PropDto) => () =>
    void s.run(api.propKeyHere(layerId, p.key, s.t))
  const all = data
    ? [
        ...data.transform,
        ...data.effects.flatMap((e) => e.params),
        ...data.camera,
      ]
    : []
  const picked = all.find((p) => sameKey(p.key, s.picked?.key)) ?? null

  const problemList = problems.length > 0 && (
    <div className="flex flex-col gap-2">
      <SectionTitle>Problems</SectionTitle>
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
    </div>
  )

  if (!layerId && !s.camera) {
    return (
      <ScrollArea className="h-full">
        <div className="flex flex-col gap-4 p-4">
          <Empty className="border border-dashed p-6">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <MousePointerClickIcon />
              </EmptyMedia>
              <EmptyTitle>Nothing selected</EmptyTitle>
              <EmptyDescription>
                Click a layer on the timeline to see and edit its values at the
                playhead.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
          {problemList}
          <div className="flex flex-col gap-2">
            <SectionTitle
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
              Project
            </SectionTitle>
            <ProjectSummary />
          </div>
        </div>
      </ScrollArea>
    )
  }

  const l = data?.layer
  const ptr = layerId ? `/layers/@${layerId}` : ""
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
        <KindIcon
          kind={l?.kind ?? "camera"}
          className="size-4 text-muted-foreground"
        />
        <span className="truncate font-medium">{l ? l.id : "Camera"}</span>
        {l && <Badge variant="outline">{l.label}</Badge>}
        {l && !l.visible_now && (
          <Badge variant="secondary">not on screen</Badge>
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
                {l.enabled
                  ? "Enabled: click to disable"
                  : "Disabled: click to enable"}
              </TooltipContent>
            </Tooltip>
          )}
          {l && (
            <DropdownMenu>
              <DropdownMenuTrigger
                render={<Button variant="ghost" size="icon-sm" />}
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
                  Move up
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => void s.run(api.layerMove(l.id, false))}
                >
                  <ArrowDownIcon />
                  Move down
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
        <Tabs
          defaultValue="properties"
          className="flex min-h-0 flex-1 flex-col gap-0"
        >
          <div className="px-3 pt-3">
            <TabsList className="w-full">
              <TabsTrigger value="properties">Properties</TabsTrigger>
              {l && (
                <TabsTrigger value="effects">
                  Effects{" "}
                  <Badge variant="secondary">{data.effects.length}</Badge>
                </TabsTrigger>
              )}
              <TabsTrigger value="keys">Keys</TabsTrigger>
            </TabsList>
          </div>
          <TabsContent value="properties" className="min-h-0 flex-1">
            <ScrollArea className="h-full">
              <div className="flex flex-col gap-4 p-3">
                <p className="px-1 text-xs text-muted-foreground tabular-nums">
                  Values at frame {s.frame} · {fmtTime(s.info, s.t)}
                </p>
                {l && (
                  <div className="flex flex-col gap-1">
                    <SectionTitle>Layer</SectionTitle>
                    <div className="flex items-center gap-2 px-1">
                      <span className="w-24 shrink-0 text-sm text-muted-foreground">
                        in · out
                      </span>
                      <NumberInput
                        className="flex-1"
                        value={l.start}
                        onCommit={(v) =>
                          void s.run(
                            api.editOps(
                              [{ op: "set", pointer: `${ptr}/in`, value: v }],
                              `in of ${l.id}: ${v}`
                            )
                          )
                        }
                      />
                      <NumberInput
                        className="flex-1"
                        value={l.end ?? s.info?.duration ?? 0}
                        onCommit={(v) =>
                          void s.run(
                            api.editOps(
                              [{ op: "set", pointer: `${ptr}/out`, value: v }],
                              `out of ${l.id}: ${v}`
                            )
                          )
                        }
                      />
                      <span className="w-8 text-xs text-muted-foreground">
                        {s.info?.time_base === "beats" ? "beats" : "s"}
                      </span>
                    </div>
                    <div className="flex items-center gap-2 px-1">
                      <span className="w-24 shrink-0 text-sm text-muted-foreground">
                        blend
                      </span>
                      <Select
                        items={BLENDS.map((b) => ({ label: b, value: b }))}
                        value={l.blend}
                        onValueChange={(v) =>
                          v &&
                          v !== l.blend &&
                          void s.run(
                            api.editOps(
                              [
                                {
                                  op: "set",
                                  pointer: `${ptr}/blend`,
                                  value: v,
                                },
                              ],
                              `blend of ${l.id}: ${v}`
                            )
                          )
                        }
                      >
                        <SelectTrigger size="sm" className="h-7 flex-1">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            {BLENDS.map((b) => (
                              <SelectItem key={b} value={b}>
                                {b}
                              </SelectItem>
                            ))}
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </div>
                    {l.slot != null && (
                      <div className="flex items-center gap-2 px-1">
                        <span className="w-24 shrink-0 text-sm text-muted-foreground">
                          slot
                        </span>
                        <Select
                          items={(s.snapshot?.slots ?? []).map((x) => ({
                            label: x.name,
                            value: x.name,
                          }))}
                          value={l.slot}
                          onValueChange={(v) =>
                            v &&
                            v !== l.slot &&
                            void s.run(
                              api.editOps(
                                [
                                  {
                                    op: "set",
                                    pointer: `${ptr}/slot`,
                                    value: v,
                                  },
                                ],
                                `slot of ${l.id}: ${v}`
                              )
                            )
                          }
                        >
                          <SelectTrigger size="sm" className="h-7 flex-1">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              {(s.snapshot?.slots ?? []).map((x) => (
                                <SelectItem key={x.name} value={x.name}>
                                  {x.name}
                                </SelectItem>
                              ))}
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      </div>
                    )}
                    {l.color && (
                      <div className="flex items-center gap-2 px-1">
                        <span className="w-24 shrink-0 text-sm text-muted-foreground">
                          color
                        </span>
                        {l.color.map((c, i) => (
                          <NumberInput
                            key={i}
                            className="min-w-0 flex-1"
                            value={c}
                            onCommit={(v) =>
                              void s.run(
                                api.editOps(
                                  [
                                    {
                                      op: "set",
                                      pointer: `${ptr}/color`,
                                      value: l.color!.map((x, j) =>
                                        j === i ? v : x
                                      ),
                                    },
                                  ],
                                  `color of ${l.id}`
                                )
                              )
                            }
                          />
                        ))}
                      </div>
                    )}
                  </div>
                )}
                {l && <Separator />}
                {l && <TransitionSection data={data} />}
                {l && <Separator />}
                <div className="flex flex-col gap-0.5">
                  <SectionTitle>{l ? "Transform" : "Camera"}</SectionTitle>
                  {(l ? data.transform : data.camera).map((p) => (
                    <PropRow
                      key={p.name}
                      prop={p}
                      picked={sameKey(p.key, s.picked?.key)}
                      onPick={() => pick(p, null)}
                      onCommit={commit(p)}
                      onKeyHere={keyHere(p)}
                    />
                  ))}
                </div>
                {problemList}
              </div>
            </ScrollArea>
          </TabsContent>
          {l && (
            <TabsContent value="effects" className="min-h-0 flex-1">
              <ScrollArea className="h-full">
                <div className="flex flex-col gap-3 p-3">
                  <SectionTitle action={<AddEffect layer={l.id} />}>
                    {data.effects.length} effect
                    {data.effects.length === 1 ? "" : "s"}, applied top to
                    bottom
                  </SectionTitle>
                  {data.effects.map((e) => (
                    <Card
                      key={`${e.index}-${e.kind}`}
                      size="sm"
                      className={cn(
                        e.enabled ? undefined : "opacity-60",
                        presenceRing(
                          presenceFor(agent, {
                            effect: { layer: l.id, index: e.index },
                          })
                        )
                      )}
                    >
                      <CardHeader>
                        <CardTitle className="flex items-center gap-2">
                          {e.kind}
                          <AgentAddedBadge layer={l.id} index={e.index} />
                          <Badge variant="outline">
                            {CATEGORY_LABEL[e.category] ?? e.category}
                          </Badge>
                        </CardTitle>
                        <CardDescription className="line-clamp-2">
                          {e.doc}
                        </CardDescription>
                        <CardAction className="flex items-center gap-1">
                          <Switch
                            size="sm"
                            checked={e.enabled}
                            onCheckedChange={() =>
                              void s.run(api.effectToggle(l.id, e.index))
                            }
                            aria-label="Enabled"
                          />
                          <DropdownMenu>
                            <DropdownMenuTrigger
                              render={<Button variant="ghost" size="icon-xs" />}
                            >
                              <EllipsisIcon />
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end" className="w-40">
                              <DropdownMenuItem
                                disabled={e.index === 0}
                                onClick={() =>
                                  void s.run(
                                    api.effectMove(l.id, e.index, true)
                                  )
                                }
                              >
                                <ArrowUpIcon />
                                Move up
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                disabled={e.index === data.effects.length - 1}
                                onClick={() =>
                                  void s.run(
                                    api.effectMove(l.id, e.index, false)
                                  )
                                }
                              >
                                <ArrowDownIcon />
                                Move down
                              </DropdownMenuItem>
                              <DropdownMenuSeparator />
                              <DropdownMenuItem
                                variant="destructive"
                                onClick={() =>
                                  void s.run(api.effectRemove(l.id, e.index))
                                }
                              >
                                <Trash2Icon />
                                Remove
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </CardAction>
                      </CardHeader>
                      <CardContent className="flex flex-col gap-0.5">
                        {e.params.map((p) => (
                          <PropRow
                            key={p.name}
                            prop={p}
                            picked={sameKey(p.key, s.picked?.key)}
                            onPick={() => pick(p, e.index)}
                            onCommit={commit(p)}
                            onKeyHere={keyHere(p)}
                          />
                        ))}
                        {e.options.map((o) => (
                          <div
                            key={o.name}
                            className="flex min-h-8 items-center gap-2 px-1"
                          >
                            <span
                              className="w-24 shrink-0 truncate text-sm text-muted-foreground"
                              title={o.doc}
                            >
                              {o.name}
                            </span>
                            <Select
                              items={o.values.map((v) => ({
                                label: v,
                                value: v,
                              }))}
                              value={o.value}
                              onValueChange={(v) =>
                                v &&
                                v !== o.value &&
                                void s.run(
                                  api.effectOption(l.id, e.index, o.name, v)
                                )
                              }
                            >
                              <SelectTrigger size="sm" className="h-7 flex-1">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectGroup>
                                  {o.values.map((v) => (
                                    <SelectItem key={v} value={v}>
                                      {v}
                                    </SelectItem>
                                  ))}
                                </SelectGroup>
                              </SelectContent>
                            </Select>
                          </div>
                        ))}
                      </CardContent>
                    </Card>
                  ))}
                </div>
              </ScrollArea>
            </TabsContent>
          )}
          <TabsContent value="keys" className="min-h-0 flex-1">
            <ScrollArea className="h-full">
              <div className="flex flex-col gap-3 p-3">
                <SectionTitle
                  action={
                    picked ? (
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={keyHere(picked)}
                        disabled={picked.key_here != null}
                      >
                        <LocateFixedIcon data-icon="inline-start" />
                        Key here
                      </Button>
                    ) : null
                  }
                >
                  {picked
                    ? `Keys of ${picked.pointer.replace(/^\/layers\/@/, "")}`
                    : "Keys"}
                </SectionTitle>
                {picked ? (
                  <KeysTable prop={picked} layer={layerId} />
                ) : (
                  <p className="px-1 text-sm text-muted-foreground">
                    Click a property's name on the Properties or Effects tab to
                    pick it; its keys are listed here.
                  </p>
                )}
                <Separator />
                <SectionTitle>Animated here</SectionTitle>
                <div className="flex flex-wrap gap-1.5">
                  {all
                    .filter((p) => p.animated)
                    .map((p) => (
                      <Badge
                        key={JSON.stringify(p.key)}
                        variant={
                          sameKey(p.key, s.picked?.key) ? "default" : "outline"
                        }
                        className="cursor-pointer"
                        onClick={() =>
                          pick(p, p.key.type === "effect" ? p.key.index : null)
                        }
                      >
                        {p.key.type === "effect"
                          ? `${p.key.kind}.${p.name}`
                          : p.name}{" "}
                        · {p.keys.length}
                      </Badge>
                    ))}
                </div>
              </div>
            </ScrollArea>
          </TabsContent>
        </Tabs>
      )}
    </div>
  )
}
