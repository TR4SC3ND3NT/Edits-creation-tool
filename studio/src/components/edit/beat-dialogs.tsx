// "Beat cut" and "Accent": the dialogs of `edits beats cut` and
// `edits beats accent`, calling the same functions (`beatops::cut`,
// `beatops::times`, `preset::apply`) on the Rust side. Before the button is
// pressed they say what they will do, from a dry run.

import * as React from "react"

import { useEditor } from "@/components/edit/editor-context"
import { Choice } from "@/components/edit/fields"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Slider } from "@/components/ui/slider"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api, errorText, type AccentRequest, type CutRequest } from "@/lib/api"
import { usePresets } from "@/lib/library"
import { useStudio } from "@/lib/store"
import { tidy } from "@/lib/time"

/** A dry run of `request`, again whenever it changes. */
function useDry<T>(
  open: boolean,
  request: T,
  run: (r: T) => Promise<{ count: number }>
) {
  const [text, setText] = React.useState<{ ok: boolean; text: string } | null>(
    null
  )
  const key = JSON.stringify(request)
  React.useEffect(() => {
    if (!open) return
    let live = true
    const id = window.setTimeout(() => {
      run(request)
        .then((d) => live && setText({ ok: true, text: String(d.count) }))
        .catch((e) => live && setText({ ok: false, text: errorText(e) }))
    }, 150)
    return () => {
      live = false
      window.clearTimeout(id)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, key])
  return text
}

function RangeFields({
  from,
  to,
  setFrom,
  setTo,
}: {
  from: string
  to: string
  setFrom: (v: string) => void
  setTo: (v: string) => void
}) {
  return (
    <div className="grid grid-cols-2 gap-3">
      <Field>
        <FieldLabel htmlFor="beat-from">From</FieldLabel>
        <Input
          id="beat-from"
          value={from}
          placeholder="start"
          onChange={(e) => setFrom(e.target.value)}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="beat-to">To</FieldLabel>
        <Input
          id="beat-to"
          value={to}
          placeholder="end"
          onChange={(e) => setTo(e.target.value)}
        />
      </Field>
    </div>
  )
}

const ON = ["beats", "offbeats", "downbeats", "kicks", "snares", "hats"]

export function BeatCutDialog() {
  const s = useStudio()
  const ed = useEditor()
  const slots = s.snapshot?.slots ?? []
  const [picked, setPicked] = React.useState<string[] | null>(null)
  const [mode, setMode] = React.useState("every")
  const [every, setEvery] = React.useState("1")
  const [on, setOn] = React.useState("kicks")
  const [from, setFrom] = React.useState("")
  const [to, setTo] = React.useState("")
  const [strength, setStrength] = React.useState(0.3)
  const [prefix, setPrefix] = React.useState("cut")
  const open = ed.cutOpen
  React.useEffect(() => {
    if (!open) return
    setFrom(ed.range ? `b${tidy(ed.range[0], 3)}` : "")
    setTo(ed.range ? `b${tidy(ed.range[1], 3)}` : "")
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])
  const chosen = picked ?? slots.map((x) => x.name)
  const request: CutRequest = {
    slots: chosen,
    every: mode === "every" ? Number(every) : null,
    on: mode === "on" ? on : null,
    from,
    to,
    min_strength: strength,
    prefix,
  }
  const dry = useDry(open, request, api.beatsCutDry)
  const go = async () => {
    const done = await s.run(api.beatsCut(request))
    if (done) ed.setCutOpen(false)
  }
  return (
    <Dialog open={open} onOpenChange={ed.setCutOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Beat cut</DialogTitle>
          <DialogDescription>
            One picture per beat (or bar, or hit) over a range, cycling through
            the pictures you tick. Adds image layers.
          </DialogDescription>
        </DialogHeader>
        <FieldGroup className="gap-4">
          <Field>
            <FieldLabel>Pictures, in order</FieldLabel>
            <div className="flex flex-wrap gap-x-4 gap-y-2">
              {slots.map((x) => (
                <Label
                  key={x.name}
                  className="flex items-center gap-2 font-normal"
                >
                  <Checkbox
                    checked={chosen.includes(x.name)}
                    onCheckedChange={(c) =>
                      setPicked(
                        c
                          ? [...chosen, x.name]
                          : chosen.filter((n) => n !== x.name)
                      )
                    }
                  />
                  {x.name}
                </Label>
              ))}
            </div>
          </Field>
          <Field>
            <FieldLabel>Cut</FieldLabel>
            <ToggleGroup
              multiple={false}
              variant="outline"
              size="sm"
              value={[mode]}
              onValueChange={(v) => v[0] && setMode(v[0])}
            >
              <ToggleGroupItem value="every">Every N beats</ToggleGroupItem>
              <ToggleGroupItem value="on">On the song</ToggleGroupItem>
            </ToggleGroup>
            {mode === "every" ? (
              <ToggleGroup
                multiple={false}
                variant="outline"
                size="sm"
                value={[every]}
                onValueChange={(v) => v[0] && setEvery(v[0])}
              >
                {[
                  ["0.5", "½ beat"],
                  ["1", "1 beat"],
                  ["2", "2 beats"],
                  ["4", "1 bar"],
                ].map(([v, l]) => (
                  <ToggleGroupItem key={v} value={v}>
                    {l}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            ) : (
              <div className="flex items-center gap-3">
                <Choice
                  value={on}
                  options={ON}
                  onChange={setOn}
                  label="Cut on"
                />
                <span className="text-xs text-muted-foreground">
                  min strength
                </span>
                <Slider
                  className="w-24"
                  min={0}
                  max={1}
                  step={0.05}
                  value={[strength]}
                  onValueChange={(v) =>
                    setStrength(Array.isArray(v) ? v[0] : (v as number))
                  }
                />
              </div>
            )}
          </Field>
          <RangeFields from={from} to={to} setFrom={setFrom} setTo={setTo} />
          <Field>
            <FieldLabel htmlFor="cut-prefix">Layer names</FieldLabel>
            <Input
              id="cut-prefix"
              value={prefix}
              onChange={(e) => setPrefix(e.target.value)}
            />
          </Field>
          <p
            className={
              dry && !dry.ok
                ? "text-sm text-destructive"
                : "text-sm text-muted-foreground"
            }
            role="status"
          >
            {dry ? (dry.ok ? `Adds ${dry.text} layers.` : dry.text) : "…"}
          </p>
        </FieldGroup>
        <DialogFooter>
          <Button variant="outline" onClick={() => ed.setCutOpen(false)}>
            Cancel
          </Button>
          <Button onClick={() => void go()} disabled={!dry?.ok}>
            Cut
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function AccentDialog() {
  const s = useStudio()
  const ed = useEditor()
  const presets = usePresets(s.snapshot?.path ?? null).filter(
    (p) => !p.transition && !p.tags.includes("text")
  )
  const analysed = !!ed.song?.marks
  const [preset, setPreset] = React.useState("punch")
  const [on, setOn] = React.useState<string | null>(null)
  const [target, setTarget] = React.useState("auto")
  const [nth, setNth] = React.useState("1")
  const [from, setFrom] = React.useState("")
  const [to, setTo] = React.useState("")
  const [strength, setStrength] = React.useState(0.3)
  const open = ed.accentOpen
  React.useEffect(() => {
    if (!open) return
    setFrom(ed.range ? `b${tidy(ed.range[0], 3)}` : "")
    setTo(ed.range ? `b${tidy(ed.range[1], 3)}` : "")
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])
  const when = on ?? (analysed ? "kicks" : "beats")
  const request: AccentRequest = {
    preset,
    on: when,
    target,
    selected: s.layer,
    from,
    to,
    min_strength: strength,
    every_nth: Number(nth),
    values: {},
  }
  const dry = useDry(open, request, api.beatsAccentDry)
  const go = async () => {
    const done = await s.run(api.beatsAccent(request))
    if (done) ed.setAccentOpen(false)
  }
  const desc = presets.find((p) => p.name === preset)?.description
  return (
    <Dialog open={open} onOpenChange={ed.setAccentOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Accent</DialogTitle>
          <DialogDescription>
            Put a preset on every kick, snare, beat or downbeat of a range, as
            one edit.
          </DialogDescription>
        </DialogHeader>
        <FieldGroup className="gap-4">
          <Field>
            <FieldLabel>Preset</FieldLabel>
            <Choice
              value={preset}
              options={presets.map((p) => p.name)}
              onChange={setPreset}
              label="Preset"
            />
            {desc && <p className="text-xs text-muted-foreground">{desc}</p>}
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field>
              <FieldLabel>On</FieldLabel>
              <Choice
                value={when}
                options={[...ON, "drop"]}
                onChange={setOn}
                label="On"
              />
            </Field>
            <Field>
              <FieldLabel>Keep</FieldLabel>
              <Choice
                value={nth}
                options={[
                  { value: "1", label: "every one" },
                  { value: "2", label: "every other" },
                  { value: "4", label: "every fourth" },
                ]}
                onChange={setNth}
                label="Keep"
              />
            </Field>
          </div>
          {!analysed && ["kicks", "snares", "hats", "drop"].includes(when) && (
            <p className="text-xs text-muted-foreground">
              Kicks, snares, hats and the drop need the song analysis (Audio
              tab, or Analyse on the timeline).
            </p>
          )}
          <Field>
            <FieldLabel>On which layer</FieldLabel>
            <ToggleGroup
              multiple={false}
              variant="outline"
              size="sm"
              value={[target]}
              onValueChange={(v) => v[0] && setTarget(v[0])}
            >
              <ToggleGroupItem value="auto">Picture on screen</ToggleGroupItem>
              <ToggleGroupItem value="selected" disabled={!s.layer}>
                {s.layer ? `Layer ${s.layer}` : "Selected layer"}
              </ToggleGroupItem>
              <ToggleGroupItem value="fx">Whole frame</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <RangeFields from={from} to={to} setFrom={setFrom} setTo={setTo} />
          {["kicks", "snares", "hats"].includes(when) && (
            <Field>
              <FieldLabel>
                Skip hits weaker than {Math.round(strength * 100)}%
              </FieldLabel>
              <Slider
                min={0}
                max={1}
                step={0.05}
                value={[strength]}
                onValueChange={(v) =>
                  setStrength(Array.isArray(v) ? v[0] : (v as number))
                }
              />
            </Field>
          )}
          <p
            className={
              dry && !dry.ok
                ? "text-sm text-destructive"
                : "text-sm text-muted-foreground"
            }
            role="status"
          >
            {dry
              ? dry.ok
                ? `${preset.replaceAll("_", " ")} at ${dry.text} times.`
                : dry.text
              : "…"}
          </p>
        </FieldGroup>
        <DialogFooter>
          <Button variant="outline" onClick={() => ed.setAccentOpen(false)}>
            Cancel
          </Button>
          <Button onClick={() => void go()} disabled={!dry?.ok}>
            Accent
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
