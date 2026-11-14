import * as React from "react"
import { PlusIcon, SearchIcon, WandSparklesIcon } from "lucide-react"
import { toast } from "sonner"

import { NumberInput } from "@/components/edit/fields"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
} from "@/components/ui/input-group"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { Slider } from "@/components/ui/slider"
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  api,
  errorText,
  type EffectSpec,
  type Preset,
  type Variable,
} from "@/lib/api"
import {
  CATEGORY_LABEL,
  unitLabel,
  useEffects,
  usePresets,
} from "@/lib/library"
import { useStudio } from "@/lib/store"
import { fmtTime, tidy } from "@/lib/time"

const matches = (q: string, ...texts: string[]) => {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean)
  const hay = texts.join(" ").toLowerCase()
  return words.every((w) => hay.includes(w))
}

function VariableField({
  v,
  value,
  onChange,
}: {
  v: Variable
  value: unknown
  onChange: (x: unknown) => void
}) {
  const label = (
    <FieldLabel>
      {v.name}
      {v.unit && (
        <span className="font-normal text-muted-foreground">{v.unit}</span>
      )}
    </FieldLabel>
  )
  if (v.type === "boolean")
    return (
      <Field orientation="horizontal">
        <Switch checked={!!value} onCheckedChange={onChange} />
        <div className="flex flex-col gap-1">
          {label}
          <FieldDescription>{v.doc}</FieldDescription>
        </div>
      </Field>
    )
  if (v.type === "enum" || (v.type === "string" && v.options.length))
    return (
      <Field>
        {label}
        <Select
          items={v.options.map((o) => ({ label: o, value: o }))}
          value={String(value)}
          onValueChange={(x) => x && onChange(x)}
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {v.options.map((o) => (
                <SelectItem key={o} value={o}>
                  {o}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
        <FieldDescription>{v.doc}</FieldDescription>
      </Field>
    )
  if (v.type === "number" || v.type === "integer") {
    const d = Number(v.default ?? 0)
    const lo = v.min ?? (d >= 0 ? 0 : d * 4)
    const hi = v.max ?? (d > 0 ? d * 4 : Math.max(lo + 1, 1))
    const n = Number(value)
    return (
      <Field>
        {label}
        <div className="flex items-center gap-3">
          <Slider
            className="flex-1"
            min={lo}
            max={hi}
            step={v.type === "integer" ? 1 : (hi - lo) / 200}
            value={[Math.min(hi, Math.max(lo, n))]}
            onValueChange={(x) => onChange(Array.isArray(x) ? x[0] : x)}
          />
          <NumberInput className="h-8 w-20" value={n} onCommit={onChange} />
        </div>
        <FieldDescription>
          {v.doc} Range {tidy(lo)}–{tidy(hi)}, default {tidy(d)}.
        </FieldDescription>
      </Field>
    )
  }
  if (Array.isArray(value))
    return (
      <Field>
        {label}
        <div className="flex gap-2">
          {(value as number[]).map((x, i) => (
            <NumberInput
              key={i}
              className="h-8 flex-1"
              value={x}
              onCommit={(nv) =>
                onChange((value as number[]).map((y, j) => (j === i ? nv : y)))
              }
            />
          ))}
        </div>
        <FieldDescription>{v.doc}</FieldDescription>
      </Field>
    )
  return (
    <Field>
      {label}
      <Input
        value={String(value ?? "")}
        onChange={(e) => onChange(e.target.value)}
      />
      <FieldDescription>{v.doc}</FieldDescription>
    </Field>
  )
}

function PresetSheet({
  preset,
  onClose,
}: {
  preset: Preset | null
  onClose: () => void
}) {
  const s = useStudio()
  const [values, setValues] = React.useState<Record<string, unknown>>({})
  const [target, setTarget] = React.useState("selected")
  const [at, setAt] = React.useState("playhead")
  React.useEffect(() => {
    if (!preset) return
    setValues(
      Object.fromEntries(preset.variables.map((v) => [v.name, v.default]))
    )
    setAt(
      preset.transition || preset.tags.includes("intro") ? "cut" : "playhead"
    )
    setTarget("selected")
  }, [preset])
  const apply = () => {
    if (!preset) return
    api
      .presetApply({
        name: preset.name,
        target,
        selected: s.layer,
        at,
        playhead: s.t,
        values,
      })
      .then((d) => {
        toast.success(`Applied ${preset.name}`, { description: d.summary })
        onClose()
      })
      .catch((e) =>
        toast.error(`Cannot apply ${preset.name}`, {
          description: errorText(e),
        })
      )
  }
  const on =
    target === "selected"
      ? (s.layer ?? "the picture on screen")
      : target === "auto"
        ? "the picture on screen"
        : "an accents layer on top"
  return (
    <Sheet open={!!preset} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-[26rem] sm:max-w-md">
        {preset && (
          <>
            <SheetHeader>
              <SheetTitle>{preset.name}</SheetTitle>
              <SheetDescription>{preset.description}</SheetDescription>
              <div className="flex flex-wrap gap-1.5 pt-1">
                {preset.tags.map((t) => (
                  <Badge key={t} variant="outline">
                    {t}
                  </Badge>
                ))}
              </div>
            </SheetHeader>
            <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-4">
              <FieldGroup>
                {preset.target === "layer" && (
                  <Field>
                    <FieldLabel>On</FieldLabel>
                    <ToggleGroup
                      multiple={false}
                      variant="outline"
                      value={[target]}
                      onValueChange={(v) => v[0] && setTarget(v[0])}
                    >
                      <ToggleGroupItem value="selected">Layer</ToggleGroupItem>
                      <ToggleGroupItem value="auto">Screen</ToggleGroupItem>
                      <ToggleGroupItem value="fx">Accents</ToggleGroupItem>
                    </ToggleGroup>
                    <FieldDescription>Changes {on}.</FieldDescription>
                  </Field>
                )}
                <Field>
                  <FieldLabel>At</FieldLabel>
                  <ToggleGroup
                    multiple={false}
                    variant="outline"
                    value={[at]}
                    onValueChange={(v) => v[0] && setAt(v[0])}
                  >
                    <ToggleGroupItem value="playhead">Playhead</ToggleGroupItem>
                    <ToggleGroupItem value="cut">Layer's cut</ToggleGroupItem>
                  </ToggleGroup>
                  <FieldDescription>
                    {at === "playhead"
                      ? `At ${fmtTime(s.info, s.t)} (frame ${s.frame}).`
                      : "At the in of the layer it changes."}
                  </FieldDescription>
                </Field>
              </FieldGroup>
              {preset.variables.length > 0 && (
                <>
                  <Separator />
                  <FieldGroup>
                    {preset.variables.map((v) => (
                      <VariableField
                        key={v.name}
                        v={v}
                        value={values[v.name]}
                        onChange={(x) =>
                          setValues((o) => ({ ...o, [v.name]: x }))
                        }
                      />
                    ))}
                  </FieldGroup>
                </>
              )}
              {preset.credit && (
                <p className="text-xs text-muted-foreground">{preset.credit}</p>
              )}
            </div>
            <SheetFooter>
              <Button onClick={apply}>
                <WandSparklesIcon data-icon="inline-start" />
                Apply at {at === "playhead" ? fmtTime(s.info, s.t) : "the cut"}
              </Button>
              <Button variant="outline" onClick={onClose}>
                Cancel
              </Button>
            </SheetFooter>
          </>
        )}
      </SheetContent>
    </Sheet>
  )
}

function PresetTable({
  list,
  onOpen,
}: {
  list: Preset[]
  onOpen: (p: Preset) => void
}) {
  return (
    <div className="overflow-hidden rounded-lg border">
      <Table>
        <TableHeader className="bg-muted">
          <TableRow>
            <TableHead className="w-48">Preset</TableHead>
            <TableHead className="w-28">Group</TableHead>
            <TableHead>Description</TableHead>
            <TableHead className="w-24 text-right">Variables</TableHead>
            <TableHead className="w-24">Source</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {list.length ? (
            list.map((p) => (
              <TableRow
                key={p.name}
                className="cursor-pointer"
                onClick={() => onOpen(p)}
              >
                <TableCell className="font-medium">
                  {p.name.replace("transition:", "")}
                </TableCell>
                <TableCell>
                  <Badge variant="outline" className="text-muted-foreground">
                    {p.group}
                  </Badge>
                </TableCell>
                <TableCell className="whitespace-normal text-muted-foreground">
                  {p.description}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {p.variables.length}
                </TableCell>
                <TableCell className="text-muted-foreground">
                  {p.origin}
                </TableCell>
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={5} className="h-24 text-center">
                No results.
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}

function EffectTable({ list }: { list: EffectSpec[] }) {
  const s = useStudio()
  return (
    <div className="overflow-hidden rounded-lg border">
      <Table>
        <TableHeader className="bg-muted">
          <TableRow>
            <TableHead className="w-40">Effect</TableHead>
            <TableHead className="w-24">Category</TableHead>
            <TableHead>Parameters</TableHead>
            <TableHead className="w-36" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {list.length ? (
            list.map((e) => (
              <TableRow key={e.kind}>
                <TableCell className="align-top">
                  <div className="font-medium">{e.kind}</div>
                  <div className="line-clamp-3 max-w-56 text-xs whitespace-normal text-muted-foreground">
                    {e.doc}
                  </div>
                </TableCell>
                <TableCell className="align-top">
                  <Badge variant="outline" className="text-muted-foreground">
                    {CATEGORY_LABEL[e.category] ?? e.category}
                  </Badge>
                </TableCell>
                <TableCell className="align-top whitespace-normal">
                  <div className="flex flex-wrap gap-1.5">
                    {e.params.map((p) => (
                      <Badge
                        key={p.name}
                        variant="secondary"
                        className="font-normal"
                        title={p.doc}
                      >
                        {p.name}
                        <span className="text-muted-foreground">
                          {p.default.map((d) => tidy(d)).join(", ")}
                          {unitLabel(p.unit) ? ` ${unitLabel(p.unit)}` : ""}
                          {p.range
                            ? ` · ${tidy(p.range[0])}–${Number.isFinite(p.range[1]) ? tidy(p.range[1]) : "∞"}`
                            : ""}
                        </span>
                      </Badge>
                    ))}
                    {e.options.map(([name, values]) => (
                      <Badge
                        key={name}
                        variant="outline"
                        className="font-normal"
                      >
                        {name}: {values.join(" | ")}
                      </Badge>
                    ))}
                  </div>
                </TableCell>
                <TableCell className="text-right align-top">
                  <Button
                    variant="outline"
                    size="xs"
                    disabled={!s.layer || e.needs_source}
                    onClick={() =>
                      s.layer && void s.run(api.effectAdd(s.layer, e.kind))
                    }
                  >
                    <PlusIcon data-icon="inline-start" />
                    {s.layer ? `Add to ${s.layer}` : "Select a layer"}
                  </Button>
                </TableCell>
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={4} className="h-24 text-center">
                No results.
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}

/** The libraries: presets (built-in and the project's), transitions and the effect registry. */
export function LibraryPage() {
  const s = useStudio()
  const presets = usePresets(s.snapshot?.path ?? null)
  const effects = useEffects()
  const [q, setQ] = React.useState("")
  const [open, setOpen] = React.useState<Preset | null>(null)
  const plain = presets.filter(
    (p) => !p.transition && matches(q, p.name, p.description, p.tags.join(" "))
  )
  const transitions = presets.filter(
    (p) => p.transition && matches(q, p.name, p.description)
  )
  const fx = effects.filter((e) =>
    matches(q, e.kind, e.category, e.doc, e.params.map((p) => p.name).join(" "))
  )
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <Tabs
        defaultValue="presets"
        className="w-full flex-col justify-start gap-6 py-4 md:py-6"
      >
        <div className="flex items-center justify-between gap-4 px-4 lg:px-6">
          <TabsList className="**:data-[slot=badge]:size-5 **:data-[slot=badge]:rounded-full **:data-[slot=badge]:bg-muted-foreground/30 **:data-[slot=badge]:px-1">
            <TabsTrigger value="presets">
              Presets <Badge variant="secondary">{plain.length}</Badge>
            </TabsTrigger>
            <TabsTrigger value="transitions">
              Transitions{" "}
              <Badge variant="secondary">{transitions.length}</Badge>
            </TabsTrigger>
            <TabsTrigger value="effects">
              Effects <Badge variant="secondary">{fx.length}</Badge>
            </TabsTrigger>
          </TabsList>
          <InputGroup className="max-w-sm">
            <InputGroupInput
              placeholder="Search names, tags and descriptions…"
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
            <InputGroupAddon>
              <SearchIcon />
            </InputGroupAddon>
          </InputGroup>
        </div>
        <TabsContent
          value="presets"
          className="flex flex-col gap-3 px-4 lg:px-6"
        >
          <p className="text-sm text-muted-foreground">
            Click a preset to set its variables and apply it at the playhead (
            {fmtTime(s.info, s.t)}) on{" "}
            {s.layer ? `layer ${s.layer}` : "the picture on screen"}. One edit,
            undone with Ctrl+Z.
          </p>
          <PresetTable list={plain} onOpen={setOpen} />
        </TabsContent>
        <TabsContent
          value="transitions"
          className="flex flex-col gap-3 px-4 lg:px-6"
        >
          <p className="text-sm text-muted-foreground">
            A transition goes on the layer that comes in, at its cut.
          </p>
          <PresetTable list={transitions} onOpen={setOpen} />
        </TabsContent>
        <TabsContent
          value="effects"
          className="flex flex-col gap-3 px-4 lg:px-6"
        >
          <p className="text-sm text-muted-foreground">
            The effect registry of the engine: every parameter with its default,
            unit and range.
          </p>
          <EffectTable list={fx} />
        </TabsContent>
      </Tabs>
      <PresetSheet preset={open} onClose={() => setOpen(null)} />
    </div>
  )
}
