import {
  ChevronFirstIcon,
  ChevronLastIcon,
  PauseIcon,
  PlayIcon,
  RepeatIcon,
  StepBackIcon,
  StepForwardIcon,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Toggle } from "@/components/ui/toggle"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { useStudio } from "@/lib/store"
import { barBeat, clock, lastFrame, tidy, toSeconds } from "@/lib/time"

const SCALES = [
  { label: "0.25×", value: "0.25" },
  { label: "0.5×", value: "0.5" },
  { label: "1×", value: "1" },
]

function Tip({ tip, children }: { tip: string; children: React.ReactElement }) {
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipContent>{tip}</TooltipContent>
    </Tooltip>
  )
}

export function Transport() {
  const s = useStudio()
  const { info, frame, t, playing } = s
  if (!info) return null
  const beats = info.time_base === "beats"
  return (
    <div className="@container/transport shrink-0 border-t">
      <div className="grid h-12 grid-cols-[1fr_auto_1fr] items-center gap-3 px-3">
        <div className="flex min-w-0 items-center gap-2 overflow-hidden text-sm whitespace-nowrap tabular-nums">
          <span className="font-medium">{clock(toSeconds(info, t))}</span>
          <span className="hidden text-muted-foreground @2xl/transport:inline">
            / {clock(info.seconds)}
          </span>
          {beats && (
            <Tooltip>
              <TooltipTrigger
                render={<Badge variant="outline" className="tabular-nums" />}
              >
                b{tidy(t, 2).toFixed(2)}
              </TooltipTrigger>
              <TooltipContent>
                Beat {tidy(t, 3)} · bar {barBeat(Math.max(0, t))}
              </TooltipContent>
            </Tooltip>
          )}
          <Tooltip>
            <TooltipTrigger
              render={
                <Badge
                  variant="outline"
                  className="hidden tabular-nums @3xl/transport:inline-flex"
                />
              }
            >
              f{frame}
            </TooltipTrigger>
            <TooltipContent>
              Frame {frame} of {lastFrame(info)}
            </TooltipContent>
          </Tooltip>
        </div>
        <div className="flex items-center gap-2">
          <ButtonGroup>
            <Tip tip="Start · Home">
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => s.seek(0)}
              >
                <ChevronFirstIcon />
              </Button>
            </Tip>
            <Tip tip="One frame back · ← (Shift: one beat)">
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => {
                  s.setPlaying(false)
                  s.seek(frame - 1)
                }}
              >
                <StepBackIcon />
              </Button>
            </Tip>
            <Tip tip={playing ? "Pause · Space" : "Play · Space"}>
              <Button variant="outline" size="icon-sm" onClick={s.togglePlay}>
                {playing ? <PauseIcon /> : <PlayIcon />}
              </Button>
            </Tip>
            <Tip tip="One frame forward · → (Shift: one beat)">
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => {
                  s.setPlaying(false)
                  s.seek(frame + 1)
                }}
              >
                <StepForwardIcon />
              </Button>
            </Tip>
            <Tip tip="Last frame · End">
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => s.seek(lastFrame(info))}
              >
                <ChevronLastIcon />
              </Button>
            </Tip>
          </ButtonGroup>
          <Tip tip="Loop · L">
            <Toggle
              variant="outline"
              size="sm"
              pressed={s.looping}
              onPressedChange={s.setLooping}
              aria-label="Loop"
            >
              <RepeatIcon />
            </Toggle>
          </Tip>
        </div>
        <div className="flex min-w-0 items-center justify-end gap-2">
          <Tip tip="Use the project's motion blur while paused (playback is always without it)">
            <Toggle
              variant="outline"
              size="sm"
              pressed={s.blurPaused}
              onPressedChange={s.setBlurPaused}
              disabled={!info.motion_blur}
            >
              Blur
            </Toggle>
          </Tip>
          <Select
            items={SCALES}
            value={String(s.scale)}
            onValueChange={(v) => v && s.setScale(Number(v))}
          >
            <SelectTrigger size="sm" className="w-20" aria-label="Preview size">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {SCALES.map((x) => (
                  <SelectItem key={x.value} value={x.value}>
                    {x.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        </div>
      </div>
    </div>
  )
}
