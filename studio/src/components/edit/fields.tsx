import * as React from "react"
import {
  ChevronRightIcon,
  DiamondIcon,
  RepeatIcon,
  WavesIcon,
} from "lucide-react"

import { presenceRing, usePresence } from "@/agent/presence"
import { PresenceValue } from "@/agent/presence-ui"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { Input } from "@/components/ui/input"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
} from "@/components/ui/input-group"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import type { PropDto } from "@/lib/api"
import { unitLabel } from "@/lib/library"
import { tidy } from "@/lib/time"
import { cn } from "@/lib/utils"

/** A typed number: Enter or leaving the field commits, Esc goes back, ↑/↓ step (Shift ×10). */
export function NumberInput({
  value,
  onCommit,
  className,
  digits = 3,
  disabled,
  step,
  ...props
}: {
  value: number
  onCommit: (v: number) => void
  className?: string
  digits?: number
  disabled?: boolean
  step?: number
} & Omit<React.ComponentProps<typeof Input>, "value" | "onChange" | "step">) {
  const shown = String(tidy(value, digits))
  const [text, setText] = React.useState(shown)
  const [focused, setFocused] = React.useState(false)
  React.useEffect(() => {
    if (!focused) setText(shown)
  }, [shown, focused])
  const commit = () => {
    const v = Number(text.replace(",", "."))
    if (
      text.trim() !== "" &&
      Number.isFinite(v) &&
      String(tidy(v, digits)) !== shown
    )
      onCommit(v)
    else setText(shown)
  }
  return (
    <Input
      {...props}
      disabled={disabled}
      inputMode="decimal"
      className={cn("h-7 px-2 text-right text-sm tabular-nums", className)}
      value={focused ? text : shown}
      onFocus={(e) => {
        setFocused(true)
        setText(shown)
        e.currentTarget.select()
      }}
      onBlur={() => {
        setFocused(false)
        commit()
      }}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur()
        if (e.key === "Escape") {
          setText(shown)
          setFocused(false)
          e.currentTarget.blur()
        }
        if ((e.key === "ArrowUp" || e.key === "ArrowDown") && step) {
          e.preventDefault()
          const v = Number(text.replace(",", ".")) || 0
          const d =
            (e.key === "ArrowUp" ? 1 : -1) * step * (e.shiftKey ? 10 : 1)
          setText(String(tidy(v + d, digits)))
        }
      }}
    />
  )
}

/** A number with its unit inside the field. */
export function UnitInput({
  value,
  unit,
  onCommit,
  className,
  label,
  step,
}: {
  value: number
  unit?: string
  onCommit: (v: number) => void
  className?: string
  label: string
  step?: number
}) {
  const shown = String(tidy(value, 3))
  const [text, setText] = React.useState(shown)
  const [focused, setFocused] = React.useState(false)
  React.useEffect(() => {
    if (!focused) setText(shown)
  }, [shown, focused])
  const commit = () => {
    const v = Number(text.replace(",", "."))
    if (
      text.trim() !== "" &&
      Number.isFinite(v) &&
      String(tidy(v, 3)) !== shown
    )
      onCommit(v)
    else setText(shown)
  }
  return (
    <InputGroup className={cn("h-7", className)}>
      <InputGroupInput
        aria-label={label}
        inputMode="decimal"
        className="h-7 px-1.5 text-right text-sm tabular-nums"
        value={focused ? text : shown}
        onFocus={(e) => {
          setFocused(true)
          setText(shown)
          e.currentTarget.select()
        }}
        onBlur={() => {
          setFocused(false)
          commit()
        }}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur()
          if (e.key === "Escape") {
            setText(shown)
            setFocused(false)
            e.currentTarget.blur()
          }
          if ((e.key === "ArrowUp" || e.key === "ArrowDown") && step) {
            e.preventDefault()
            const v = Number(text.replace(",", ".")) || 0
            const d =
              (e.key === "ArrowUp" ? 1 : -1) * step * (e.shiftKey ? 10 : 1)
            setText(String(tidy(v + d, 3)))
          }
        }}
      />
      {unit ? (
        <InputGroupAddon align="inline-end" className="pr-2 text-xs">
          {unit}
        </InputGroupAddon>
      ) : null}
    </InputGroup>
  )
}

const hex2 = (x: number) =>
  Math.round(Math.max(0, Math.min(1, x)) * 255)
    .toString(16)
    .padStart(2, "0")

export function toHex(c: number[]) {
  return `#${hex2(c[0] ?? 0)}${hex2(c[1] ?? 0)}${hex2(c[2] ?? 0)}`
}

function fromHex(h: string): number[] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(h.trim())
  if (!m) return null
  const n = parseInt(m[1], 16)
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => tidy(v / 255, 4))
}

/** A colour: a swatch that opens sliders per channel and a hex field. Values are 0..1. */
export function ColorField({
  value,
  onCommit,
  onPreview,
  label,
}: {
  value: number[]
  onCommit: (v: number[]) => void
  onPreview?: (v: number[]) => void
  label: string
}) {
  const [live, setLive] = React.useState<number[] | null>(null)
  const v = live ?? value
  React.useEffect(() => setLive(null), [value])
  const names = ["R", "G", "B", "A"]
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button
            variant="outline"
            size="sm"
            className="h-7 flex-1 justify-start gap-2 px-2 font-normal"
            aria-label={label}
          />
        }
      >
        <span
          className="size-4 shrink-0 rounded-sm border"
          style={{ background: toHex(v), opacity: v[3] ?? 1 }}
        />
        <span className="font-mono text-xs tabular-nums">{toHex(v)}</span>
        {v.length > 3 && (
          <span className="ml-auto text-xs text-muted-foreground tabular-nums">
            {Math.round((v[3] ?? 1) * 100)}%
          </span>
        )}
      </PopoverTrigger>
      <PopoverContent className="w-64" align="end">
        <div className="flex flex-col gap-3">
          <div
            className="h-10 rounded-md border"
            style={{ background: toHex(v), opacity: v[3] ?? 1 }}
          />
          {v.map((c, i) => (
            <div key={i} className="flex items-center gap-2">
              <span className="w-3 text-xs text-muted-foreground">
                {names[i]}
              </span>
              <Slider
                className="flex-1"
                min={0}
                max={1}
                step={1 / 255}
                value={[c]}
                onValueChange={(x) => {
                  const n = Array.isArray(x) ? x[0] : (x as number)
                  const next = v.map((y, j) => (j === i ? tidy(n, 4) : y))
                  setLive(next)
                  onPreview?.(next)
                }}
                onValueCommitted={(x) => {
                  const n = Array.isArray(x) ? x[0] : (x as number)
                  onCommit(v.map((y, j) => (j === i ? tidy(n, 4) : y)))
                }}
              />
              <span className="w-9 text-right text-xs tabular-nums">
                {Math.round(c * 255)}
              </span>
            </div>
          ))}
          <Input
            className="h-7 font-mono text-sm"
            defaultValue={toHex(v)}
            key={toHex(v)}
            aria-label={`${label} hex`}
            onKeyDown={(e) => {
              if (e.key !== "Enter") return
              const rgb = fromHex(e.currentTarget.value)
              if (rgb) onCommit(v.length > 3 ? [...rgb, v[3]] : rgb)
            }}
          />
        </div>
      </PopoverContent>
    </Popover>
  )
}

const AXES = ["X", "Y", "Z", "W"]

/**
 * One animatable property (the inspector field): its name (click to pick
 * it; its keys go to the Keys list), its value at the playhead (a slider
 * and a number for values with a range, a field per axis otherwise, a
 * swatch for colours) with its unit, and the key diamond (filled: a key at
 * the playhead, click to remove it; hollow: click to add one).
 */
export function PropRow({
  prop,
  picked,
  onPick,
  onCommit,
  onKeyHere,
  onRemoveKey,
  onPreview,
  onPreviewEnd,
  label,
}: {
  prop: PropDto
  picked: boolean
  onPick: () => void
  onCommit: (value: number[]) => void
  onKeyHere: () => void
  onRemoveKey?: () => void
  /** The value while a slider is dragged (not written). */
  onPreview?: (value: number[]) => void
  onPreviewEnd?: () => void
  label?: string
}) {
  const [live, setLive] = React.useState<number | null>(null)
  const value = prop.base.length ? prop.base : prop.value
  const single = value.length === 1
  const range =
    prop.range &&
    Number.isFinite(prop.range[0]) &&
    Number.isFinite(prop.range[1])
      ? prop.range
      : null
  const unit = unitLabel(prop.unit)
  React.useEffect(() => setLive(null), [prop.value, prop.base])
  // The agent working on this value: a ring, and its old -> new value under the row.
  const agent = usePresence({ pointer: prop.pointer })
  const keyed = prop.key_here != null
  const step =
    prop.unit === "count"
      ? 1
      : range
        ? (range[1] - range[0]) / 200
        : prop.unit === "deg"
          ? 1
          : 0.01
  return (
    <div
      data-slot="inspector-field"
      data-prop={prop.pointer}
      className={cn(
        "group/prop flex min-h-8 flex-wrap items-center gap-x-2 rounded-md px-1",
        picked && "bg-muted",
        presenceRing(agent)
      )}
    >
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              onClick={onPick}
              className={cn(
                "flex w-20 shrink-0 items-center gap-1 truncate text-left text-sm",
                // In a narrow inspector a vector's name goes above its fields.
                value.length > 2 && "@max-xs/inspector:w-full",
                prop.set ? "text-foreground" : "text-muted-foreground"
              )}
            />
          }
        >
          <span className="truncate">
            {label ?? prop.name.replaceAll("_", " ")}
          </span>
          {value.length > 1 && unit && unit.length <= 2 ? (
            <span className="shrink-0 text-xs text-muted-foreground">
              {unit}
            </span>
          ) : null}
          {prop.shake && (
            <WavesIcon className="size-3 shrink-0 text-muted-foreground" />
          )}
          {prop.looped && (
            <RepeatIcon className="size-3 shrink-0 text-muted-foreground" />
          )}
        </TooltipTrigger>
        <TooltipContent side="left" className="max-w-72">
          {prop.doc || prop.name}
          {` Default ${prop.default.map((d) => tidy(d)).join(", ")}${unit ? ` ${unit}` : ""}.`}
          {prop.animated
            ? ` ${prop.keys.length} keys.`
            : prop.set
              ? " Constant."
              : " Not set in the file."}
        </TooltipContent>
      </Tooltip>
      <div className="flex min-w-0 flex-1 items-center gap-1.5">
        {prop.unit === "color" ? (
          <ColorField
            value={value}
            label={prop.name}
            onCommit={onCommit}
            onPreview={onPreview}
          />
        ) : single && range ? (
          <>
            <Slider
              className="min-w-12 flex-1"
              min={range[0]}
              max={range[1]}
              step={step}
              value={[live ?? value[0]]}
              onValueChange={(v) => {
                const n = Array.isArray(v) ? v[0] : (v as number)
                setLive(n)
                onPreview?.([n])
              }}
              onValueCommitted={(v) => {
                const n = Array.isArray(v) ? v[0] : (v as number)
                onPreviewEnd?.()
                onCommit([tidy(n, 4)])
              }}
            />
            <UnitInput
              className="w-20 shrink-0"
              value={live ?? value[0]}
              unit={unit}
              step={step}
              onCommit={(v) => onCommit([v])}
              label={prop.name}
            />
          </>
        ) : (
          value.map((v, i) => (
            <UnitInput
              key={i}
              className="min-w-0 flex-1"
              value={v}
              step={step}
              unit={value.length > 1 ? AXES[i] : unit}
              label={`${prop.name} ${AXES[i] ?? i}`}
              onCommit={(nv) =>
                onCommit(value.map((x, j) => (j === i ? nv : x)))
              }
            />
          ))
        )}
      </div>
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              variant="ghost"
              size="icon-xs"
              className={cn(
                "shrink-0",
                !keyed &&
                  !prop.animated &&
                  "opacity-40 group-hover/prop:opacity-100"
              )}
              onClick={keyed ? onRemoveKey : onKeyHere}
              aria-label={keyed ? "Remove the key here" : "Add a key here"}
              aria-pressed={keyed}
            />
          }
        >
          <DiamondIcon
            className={cn(
              keyed && "fill-current",
              !prop.animated && "text-muted-foreground"
            )}
          />
        </TooltipTrigger>
        <TooltipContent>
          {keyed
            ? "A key is at the playhead: click to remove it"
            : prop.animated
              ? "Animated: click to add a key at the playhead"
              : "Click to animate: a key at the playhead holding this value"}
        </TooltipContent>
      </Tooltip>
      {agent && (
        <div className="basis-full pb-1 pl-20">
          <PresenceValue hit={agent} />
        </div>
      )}
    </div>
  )
}

/** A field that is not animatable: a label and its control. */
export function FieldRow({
  label,
  children,
  hint,
  pointer,
}: {
  label: string
  children: React.ReactNode
  hint?: string
  /** The value's JSON pointer, for the agent's highlight (one or several, space separated). */
  pointer?: string
}) {
  const ptrs = pointer?.split(" ") ?? []
  const a = usePresence({ pointer: ptrs[0] })
  const b = usePresence({ pointer: ptrs[1] })
  const agent = a ?? b
  return (
    <div
      data-slot="inspector-field"
      data-prop={pointer}
      className={cn(
        "flex min-h-8 flex-wrap items-center gap-x-2 rounded-md px-1",
        presenceRing(agent)
      )}
    >
      {hint ? (
        <Tooltip>
          <TooltipTrigger
            render={
              <span className="w-20 shrink-0 truncate text-sm text-muted-foreground" />
            }
          >
            {label}
          </TooltipTrigger>
          <TooltipContent side="left" className="max-w-72">
            {hint}
          </TooltipContent>
        </Tooltip>
      ) : (
        <span className="w-20 shrink-0 truncate text-sm text-muted-foreground">
          {label}
        </span>
      )}
      <div className="flex min-w-0 flex-1 items-center gap-1.5">{children}</div>
      {agent && (
        <div className="basis-full pb-1 pl-20">
          <PresenceValue hit={agent} />
        </div>
      )}
    </div>
  )
}

/** A collapsible section of the inspector, with a title, an optional count and actions. */
export function Section({
  title,
  count,
  action,
  children,
  defaultOpen = true,
}: {
  title: string
  count?: number
  action?: React.ReactNode
  children: React.ReactNode
  defaultOpen?: boolean
}) {
  return (
    <Collapsible defaultOpen={defaultOpen} className="group/section border-b">
      <div className="flex h-10 items-center gap-1 px-3">
        <CollapsibleTrigger
          render={
            <button
              type="button"
              className="flex flex-1 items-center gap-1.5 text-left text-sm font-medium"
            />
          }
        >
          <ChevronRightIcon className="size-4 text-muted-foreground transition-transform group-data-[open]/section:rotate-90" />
          {title}
          {count != null && (
            <Badge variant="secondary" className="ml-1 h-4 px-1.5 text-[10px]">
              {count}
            </Badge>
          )}
        </CollapsibleTrigger>
        <div className="flex items-center gap-1">{action}</div>
      </div>
      <CollapsibleContent className="flex flex-col gap-0.5 px-2 pb-3">
        {children}
      </CollapsibleContent>
    </Collapsible>
  )
}

export function SectionTitle({
  children,
  action,
}: {
  children: React.ReactNode
  action?: React.ReactNode
}) {
  return (
    <div className="flex h-8 items-center gap-2">
      <h3 className="text-sm font-medium">{children}</h3>
      <div className="ml-auto flex items-center gap-1">{action}</div>
    </div>
  )
}

/** A select of plain string values. */
export function Choice({
  value,
  options,
  onChange,
  placeholder,
  className,
  label,
}: {
  value: string | null
  options: (string | { value: string; label: string })[]
  onChange: (v: string) => void
  placeholder?: string
  className?: string
  label?: string
}) {
  const items = options.map((o) =>
    typeof o === "string" ? { value: o, label: o.replaceAll("_", " ") } : o
  )
  return (
    <Select
      items={items}
      value={value}
      onValueChange={(v) => v != null && v !== value && onChange(String(v))}
    >
      <SelectTrigger
        size="sm"
        className={cn("h-7 min-w-0 flex-1", className)}
        aria-label={label}
      >
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {items.map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

export function UnitBadge({ unit }: { unit: string }) {
  const u = unitLabel(unit)
  return u ? (
    <Badge variant="outline" className="h-4 px-1 text-[10px]">
      {u}
    </Badge>
  ) : null
}
