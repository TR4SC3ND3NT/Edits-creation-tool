import * as React from "react"
import { Bar, BarChart, CartesianGrid, XAxis } from "recharts"
import {
  BotIcon,
  HistoryIcon,
  Redo2Icon,
  RotateCcwIcon,
  Undo2Icon,
  UserIcon,
} from "lucide-react"

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
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart"
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
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
import { api, type HistoryItem } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { ago } from "@/lib/time"

const chartConfig = {
  agent: { label: "Agent", color: "var(--primary)" },
  window: { label: "You", color: "var(--muted-foreground)" },
} satisfies ChartConfig

function Actor({ actor }: { actor: string }) {
  const agent = actor === "agent"
  const you = actor === "window"
  return (
    <Badge variant="outline" className="px-1.5 text-muted-foreground">
      {agent ? <BotIcon /> : <UserIcon />}
      {agent ? "agent" : you ? "you" : actor}
    </Badge>
  )
}

/** The edit history shared with the agent (`.edits/history/`). */
export function ActivityPage() {
  const s = useStudio()
  const [now, setNow] = React.useState(() => Date.now())
  React.useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 5000)
    return () => window.clearInterval(id)
  }, [])
  const h = s.snapshot?.history ?? { undo: [], redo: [] }

  // Edits per minute over the last hour of activity, by who made them.
  const data = React.useMemo(() => {
    const times = h.undo.filter((e) => e.time != null) as (HistoryItem & {
      time: number
    })[]
    if (!times.length) return []
    const end = Math.max(...times.map((e) => e.time))
    const start = Math.max(
      Math.min(...times.map((e) => e.time)),
      end - 60 * 60 * 1000
    )
    const minute = 60 * 1000
    const n = Math.max(1, Math.ceil((end - start) / minute) + 1)
    const bins = Array.from({ length: Math.min(n, 60) }, (_, i) => ({
      at: start + i * minute,
      agent: 0,
      window: 0,
    }))
    for (const e of times) {
      const i = Math.floor((e.time - start) / minute)
      if (i < 0 || i >= bins.length) continue
      if (e.actor === "agent") bins[i].agent++
      else bins[i].window++
    }
    return bins
  }, [h.undo])

  const agentCount = h.undo.filter((e) => e.actor === "agent").length
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="flex flex-col gap-4 py-4 md:gap-6 md:py-6">
        <div className="px-4 lg:px-6">
          <Card className="@container/card">
            <CardHeader>
              <CardTitle>Edits per minute</CardTitle>
              <CardDescription>
                {h.undo.length} edits in the history: {agentCount} by the agent,{" "}
                {h.undo.length - agentCount} by you. Undo takes back the newest,
                whoever made it.
              </CardDescription>
              <CardAction className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!h.undo.length}
                  onClick={s.undo}
                >
                  <Undo2Icon data-icon="inline-start" />
                  Undo
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!h.redo.length}
                  onClick={s.redo}
                >
                  <Redo2Icon data-icon="inline-start" />
                  Redo
                </Button>
              </CardAction>
            </CardHeader>
            <CardContent className="px-2 pt-4 sm:px-6 sm:pt-6">
              {data.length ? (
                <ChartContainer
                  config={chartConfig}
                  className="aspect-auto h-[200px] w-full"
                >
                  <BarChart data={data}>
                    <CartesianGrid vertical={false} />
                    <XAxis
                      dataKey="at"
                      tickLine={false}
                      axisLine={false}
                      tickMargin={8}
                      minTickGap={32}
                      tickFormatter={(v) =>
                        new Date(v).toLocaleTimeString("en-GB", {
                          hour: "2-digit",
                          minute: "2-digit",
                        })
                      }
                    />
                    <ChartTooltip
                      cursor={false}
                      content={
                        <ChartTooltipContent
                          labelFormatter={(_, p) =>
                            new Date(
                              p?.[0]?.payload?.at ?? 0
                            ).toLocaleTimeString("en-GB", {
                              hour: "2-digit",
                              minute: "2-digit",
                            })
                          }
                          indicator="dot"
                        />
                      }
                    />
                    <ChartLegend content={<ChartLegendContent />} />
                    <Bar
                      dataKey="window"
                      stackId="a"
                      fill="var(--color-window)"
                      radius={[0, 0, 4, 4]}
                    />
                    <Bar
                      dataKey="agent"
                      stackId="a"
                      fill="var(--color-agent)"
                      radius={[4, 4, 0, 0]}
                    />
                  </BarChart>
                </ChartContainer>
              ) : (
                <Empty className="h-[200px]">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <HistoryIcon />
                    </EmptyMedia>
                    <EmptyTitle>No edits yet</EmptyTitle>
                    <EmptyDescription>
                      Edits made here or by the agent's project_edit appear
                      here.
                    </EmptyDescription>
                  </EmptyHeader>
                </Empty>
              )}
            </CardContent>
          </Card>
        </div>
        <div className="flex flex-col gap-3 px-4 lg:px-6">
          <h2 className="text-base font-medium">History</h2>
          <div className="overflow-hidden rounded-lg border">
            <Table>
              <TableHeader className="bg-muted">
                <TableRow>
                  <TableHead className="w-12">#</TableHead>
                  <TableHead className="w-28">Who</TableHead>
                  <TableHead>Change</TableHead>
                  <TableHead className="w-32">When</TableHead>
                  <TableHead className="w-40 text-right" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {h.redo.map((e, i) => (
                  <TableRow key={e.file} className="text-muted-foreground">
                    <TableCell>
                      <Badge variant="secondary">undone</Badge>
                    </TableCell>
                    <TableCell>
                      <Actor actor={e.actor} />
                    </TableCell>
                    <TableCell className="whitespace-normal line-through">
                      {e.readable}
                    </TableCell>
                    <TableCell>{ago(e.time, now)}</TableCell>
                    <TableCell className="text-right">
                      {i === 0 && (
                        <Button variant="outline" size="xs" onClick={s.redo}>
                          <Redo2Icon data-icon="inline-start" />
                          Redo
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
                {h.undo.map((e, i) => (
                  <TableRow key={e.file}>
                    <TableCell className="text-muted-foreground tabular-nums">
                      {h.undo.length - i}
                    </TableCell>
                    <TableCell>
                      <Actor actor={e.actor} />
                    </TableCell>
                    <TableCell className="whitespace-normal">
                      <Tooltip>
                        <TooltipTrigger render={<span />}>
                          {e.readable}
                        </TooltipTrigger>
                        <TooltipContent className="max-w-96">
                          {e.summary}
                        </TooltipContent>
                      </Tooltip>
                    </TableCell>
                    <TableCell
                      className="text-muted-foreground"
                      title={e.time ? new Date(e.time).toLocaleString() : ""}
                    >
                      {ago(e.time, now)}
                    </TableCell>
                    <TableCell className="text-right">
                      {i === 0 ? (
                        <Button variant="outline" size="xs" onClick={s.undo}>
                          <Undo2Icon data-icon="inline-start" />
                          Undo
                        </Button>
                      ) : (
                        <Tooltip>
                          <TooltipTrigger
                            render={
                              <Button
                                variant="ghost"
                                size="xs"
                                onClick={() => void s.run(api.restore(e.file))}
                              />
                            }
                          >
                            <RotateCcwIcon data-icon="inline-start" />
                            Restore
                          </TooltipTrigger>
                          <TooltipContent>
                            Make the version before this edit the project again
                            (as a new edit, which Undo takes back)
                          </TooltipContent>
                        </Tooltip>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
                {!h.undo.length && !h.redo.length && (
                  <TableRow>
                    <TableCell
                      colSpan={5}
                      className="h-24 text-center text-muted-foreground"
                    >
                      The history is empty.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </div>
    </div>
  )
}
