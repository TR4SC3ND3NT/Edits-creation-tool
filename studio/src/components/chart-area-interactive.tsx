"use client"

import * as React from "react"
import { Area, AreaChart, CartesianGrid, XAxis } from "recharts"

import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { cutTimes } from "@/components/section-cards"
import { useStudio } from "@/lib/store"
import { tidy } from "@/lib/time"

export const description = "Keys and cuts over the edit"

const chartConfig = {
  activity: {
    label: "Activity",
  },
  keys: {
    label: "Keys",
    color: "var(--primary)",
  },
  cuts: {
    label: "Cuts",
    color: "var(--primary)",
  },
} satisfies ChartConfig

/** How much happens in each bar, beat or half beat: keyframes set there and cuts made there. */
export function ChartAreaInteractive() {
  const { snapshot, info } = useStudio()
  const beats = info?.time_base === "beats"
  const ranges = beats
    ? [
        { value: "4", label: "Per bar" },
        { value: "1", label: "Per beat" },
        { value: "0.5", label: "Per half beat" },
      ]
    : [
        { value: "2", label: "Per 2 seconds" },
        { value: "1", label: "Per second" },
        { value: "0.5", label: "Per half second" },
      ]
  const [bin, setBin] = React.useState("1")

  const data = React.useMemo(() => {
    if (!snapshot || !info) return []
    const step = Number(bin)
    const t0 = Math.min(0, info.start_time)
    const n = Math.max(1, Math.ceil((info.duration - t0) / step))
    const rows = Array.from({ length: n }, (_, i) => ({
      t: t0 + i * step,
      keys: 0,
      cuts: 0,
    }))
    const at = (t: number) =>
      Math.min(n - 1, Math.max(0, Math.floor((t - t0) / step + 1e-9)))
    for (const r of snapshot.rows)
      for (const m of r.marks) rows[at(m.t)].keys += m.what.length
    for (const c of cutTimes(snapshot.layers, t0)) rows[at(c)].cuts += 1
    return rows
  }, [snapshot, info, bin])

  const label = (t: number) =>
    beats
      ? Number(bin) >= 4
        ? `bar ${Math.round(t / 4) + 1}`
        : `b${tidy(t, 2)}`
      : `${tidy(t, 2)} s`
  const total = data.reduce((a, d) => a + d.keys, 0)

  return (
    <Card className="@container/card">
      <CardHeader>
        <CardTitle>Edit activity</CardTitle>
        <CardDescription>
          <span className="hidden @[540px]/card:block">
            {total} keyframes and {data.reduce((a, d) => a + d.cuts, 0)} cuts
            across{" "}
            {beats
              ? `${tidy((info?.duration ?? 0) / 4, 2)} bars`
              : `${tidy(info?.duration ?? 0, 2)} s`}
          </span>
          <span className="@[540px]/card:hidden">Keys and cuts</span>
        </CardDescription>
        <CardAction>
          <ToggleGroup
            multiple={false}
            value={[bin]}
            onValueChange={(value) => setBin(value[0] ?? "1")}
            variant="outline"
            className="hidden *:data-[slot=toggle-group-item]:px-4! @[767px]/card:flex"
          >
            {ranges.map((r) => (
              <ToggleGroupItem key={r.value} value={r.value}>
                {r.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <Select
            items={ranges}
            value={bin}
            onValueChange={(value) => value !== null && setBin(value)}
          >
            <SelectTrigger
              className="flex w-40 **:data-[slot=select-value]:block **:data-[slot=select-value]:truncate @[767px]/card:hidden"
              size="sm"
              aria-label="Select a value"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="rounded-xl">
              {ranges.map((r) => (
                <SelectItem
                  key={r.value}
                  value={r.value}
                  className="rounded-lg"
                >
                  {r.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardAction>
      </CardHeader>
      <CardContent className="px-2 pt-4 sm:px-6 sm:pt-6">
        <ChartContainer
          config={chartConfig}
          className="aspect-auto h-[250px] w-full"
        >
          <AreaChart data={data}>
            <defs>
              <linearGradient id="fillKeys" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="5%"
                  stopColor="var(--color-keys)"
                  stopOpacity={1.0}
                />
                <stop
                  offset="95%"
                  stopColor="var(--color-keys)"
                  stopOpacity={0.1}
                />
              </linearGradient>
              <linearGradient id="fillCuts" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="5%"
                  stopColor="var(--color-cuts)"
                  stopOpacity={0.8}
                />
                <stop
                  offset="95%"
                  stopColor="var(--color-cuts)"
                  stopOpacity={0.1}
                />
              </linearGradient>
            </defs>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="t"
              tickLine={false}
              axisLine={false}
              tickMargin={8}
              minTickGap={32}
              tickFormatter={(v) => label(Number(v))}
            />
            <ChartTooltip
              cursor={false}
              content={
                <ChartTooltipContent
                  labelFormatter={(_, p) =>
                    label(Number(p?.[0]?.payload?.t ?? 0))
                  }
                  indicator="dot"
                />
              }
            />
            <Area
              dataKey="cuts"
              type="natural"
              fill="url(#fillCuts)"
              stroke="var(--color-cuts)"
              stackId="a"
            />
            <Area
              dataKey="keys"
              type="natural"
              fill="url(#fillKeys)"
              stroke="var(--color-keys)"
              stackId="a"
            />
          </AreaChart>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}
