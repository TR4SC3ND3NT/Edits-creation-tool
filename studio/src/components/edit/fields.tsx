import * as React from "react"
import { DiamondIcon, RepeatIcon, WavesIcon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
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

/** A number that is typed: Enter or leaving the field commits, Esc goes back. */
export function NumberInput({
  value,
  onCommit,
  className,
  digits = 3,
  disabled,
  ...props
}: {
  value: number
  onCommit: (v: number) => void
  className?: string
  digits?: number
  disabled?: boolean
} & Omit<React.ComponentProps<typeof Input>, "value" | "onChange">) {
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
      }}
    />
  )
}

const AXES = ["x", "y", "z", "w"]
const RGB = ["r", "g", "b", "a"]

/** One animatable property: its name (click to pick it), its value at the playhead, and Key here. */
export function PropRow({
  prop,
  picked,
  onPick,
  onCommit,
  onKeyHere,
}: {
  prop: PropDto
  picked: boolean
  onPick: () => void
  onCommit: (value: number[]) => void
  onKeyHere: () => void
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
  const names = prop.unit === "color" ? RGB : AXES
  const unit = unitLabel(prop.unit)
  React.useEffect(() => setLive(null), [prop.value, prop.base])
  return (
    <div
      className={cn(
        "group/prop flex min-h-8 items-center gap-2 rounded-md px-1",
        picked && "bg-muted"
      )}
    >
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              onClick={onPick}
              className={cn(
                "flex w-24 shrink-0 items-center gap-1 truncate text-left text-sm",
                prop.set ? "text-foreground" : "text-muted-foreground"
              )}
            />
          }
        >
          <span className="truncate">{prop.name}</span>
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
        {single && range ? (
          <>
            <Slider
              className="min-w-12 flex-1"
              min={range[0]}
              max={range[1]}
              step={prop.unit === "count" ? 1 : (range[1] - range[0]) / 200}
              value={[live ?? value[0]]}
              onValueChange={(v) =>
                setLive(Array.isArray(v) ? v[0] : (v as number))
              }
              onValueCommitted={(v) =>
                onCommit([Array.isArray(v) ? v[0] : (v as number)])
              }
            />
            <NumberInput
              className="w-16 shrink-0"
              value={live ?? value[0]}
              onCommit={(v) => onCommit([v])}
              aria-label={prop.name}
            />
          </>
        ) : (
          value.map((v, i) => (
            <NumberInput
              key={i}
              className="min-w-0 flex-1"
              value={v}
              aria-label={`${prop.name} ${names[i] ?? i}`}
              title={names[i]}
              onCommit={(nv) =>
                onCommit(value.map((x, j) => (j === i ? nv : x)))
              }
            />
          ))
        )}
      </div>
      {unit && !(single && range) ? (
        <span className="w-8 shrink-0 text-xs text-muted-foreground">
          {unit}
        </span>
      ) : null}
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              variant="ghost"
              size="icon-xs"
              className={cn(
                "shrink-0",
                prop.key_here == null &&
                  !prop.animated &&
                  "opacity-0 group-hover/prop:opacity-100"
              )}
              onClick={onKeyHere}
              disabled={prop.key_here != null}
            />
          }
        >
          <DiamondIcon
            className={cn(
              prop.key_here != null && "fill-current",
              !prop.animated && "text-muted-foreground"
            )}
          />
        </TooltipTrigger>
        <TooltipContent>
          {prop.key_here != null
            ? "A key is at the playhead"
            : "Key here: add a key holding this value at the playhead"}
        </TooltipContent>
      </Tooltip>
    </div>
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

export function UnitBadge({ unit }: { unit: string }) {
  const u = unitLabel(unit)
  return u ? (
    <Badge variant="outline" className="h-4 px-1 text-[10px]">
      {u}
    </Badge>
  ) : null
}
