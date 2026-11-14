import * as React from "react"
import { listen } from "@tauri-apps/api/event"
import { isTauri } from "@tauri-apps/api/core"
import { open } from "@tauri-apps/plugin-dialog"
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts"
import {
  CrosshairIcon,
  FolderOpenIcon,
  MinusIcon,
  ScanEyeIcon,
  TrendingDownIcon,
  TrendingUpIcon,
} from "lucide-react"
import { toast } from "sonner"

import { pendingCompare } from "@/components/pages/renders-page"
import { useReports } from "@/components/section-cards"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
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
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  api,
  errorText,
  type Analysis,
  type CompareRun,
  type RenderOutput,
  type Report,
  type Signature,
} from "@/lib/api"
import { useStudio } from "@/lib/store"
import { ago, firstFrameFrom, tidy } from "@/lib/time"

const DIMENSIONS: {
  id: string
  title: string
  keys: string[]
  what: string
}[] = [
  {
    id: "overall",
    title: "Overall",
    keys: ["overall"],
    what: "Mean of every category that could be compared",
  },
  {
    id: "timing",
    title: "Timing",
    keys: ["tempo", "cuts"],
    what: "Tempo and cuts matched within a frame",
  },
  {
    id: "exposure",
    title: "Exposure",
    keys: ["exposure", "flashes"],
    what: "Exposure curves and flash peaks",
  },
  {
    id: "motion",
    title: "Motion",
    keys: ["scale"],
    what: "Zoom (scale) curves per shot",
  },
  {
    id: "colour",
    title: "Colour",
    keys: ["saturation", "tone_hits"],
    what: "Saturation and tone hits",
  },
  { id: "blur", title: "Blur", keys: ["blur"], what: "Blur curves per shot" },
]

const SIGNALS = [
  { id: "stops", label: "Exposure", unit: "stops" },
  { id: "blur", label: "Blur", unit: "% h" },
  { id: "sat", label: "Saturation", unit: "×" },
  { id: "scale", label: "Scale", unit: "×" },
  { id: "lum", label: "Luminance", unit: "0–255" },
] as const

type SignalId = (typeof SIGNALS)[number]["id"]

function score(
  scores: Record<string, number | null>,
  overall: number,
  keys: string[]
) {
  if (keys[0] === "overall") return overall
  const v = keys.map((k) => scores[k]).filter((x): x is number => x != null)
  return v.length ? v.reduce((a, b) => a + b, 0) / v.length : null
}

const chartConfig = {
  reference: { label: "Reference", color: "var(--primary)" },
  render: { label: "Render", color: "var(--muted-foreground)" },
} satisfies ChartConfig

function SignatureChart({
  analysis,
  candidate,
}: {
  analysis: Analysis
  candidate: Signature | null
}) {
  const [signal, setSignal] = React.useState<SignalId>("stops")
  const meta = SIGNALS.find((x) => x.id === signal)!
  const beat = (f: number) =>
    ((f / analysis.fps - analysis.offset_seconds) * analysis.bpm) / 60
  const data = React.useMemo(() => {
    const ref = analysis.signature
    const n = (ref[signal] as (number | null)[]).length
    return Array.from({ length: n }, (_, i) => {
      const f = ref.f0 + i
      const c = candidate
        ? (candidate[signal] as (number | null)[])[f - candidate.f0]
        : undefined
      return {
        frame: f,
        beat: tidy(beat(f), 2),
        reference: (ref[signal] as (number | null)[])[i],
        render: c ?? null,
      }
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [analysis, candidate, signal])
  return (
    <Card className="@container/card">
      <CardHeader>
        <CardTitle>Signature over time</CardTitle>
        <CardDescription>
          <span className="hidden @[540px]/card:block">
            {meta.label} ({meta.unit}) of the reference
            {candidate ? " and the render" : ""}, per frame, against the
            reference's beats
          </span>
          <span className="@[540px]/card:hidden">{meta.label}</span>
        </CardDescription>
        <CardAction>
          <ToggleGroup
            multiple={false}
            value={[signal]}
            onValueChange={(v) => v[0] && setSignal(v[0] as SignalId)}
            variant="outline"
            className="hidden *:data-[slot=toggle-group-item]:px-3! @[767px]/card:flex"
          >
            {SIGNALS.map((x) => (
              <ToggleGroupItem key={x.id} value={x.id}>
                {x.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          <Select
            items={SIGNALS.map((x) => ({ label: x.label, value: x.id }))}
            value={signal}
            onValueChange={(v) => v && setSignal(v as SignalId)}
          >
            <SelectTrigger
              className="flex w-36 @[767px]/card:hidden"
              size="sm"
              aria-label="Signal"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="rounded-xl">
              {SIGNALS.map((x) => (
                <SelectItem key={x.id} value={x.id} className="rounded-lg">
                  {x.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardAction>
      </CardHeader>
      <CardContent className="px-2 pt-4 sm:px-6 sm:pt-6">
        <ChartContainer
          config={chartConfig}
          className="aspect-auto h-[260px] w-full"
        >
          <AreaChart data={data}>
            <defs>
              <linearGradient id="fillReference" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="5%"
                  stopColor="var(--color-reference)"
                  stopOpacity={0.8}
                />
                <stop
                  offset="95%"
                  stopColor="var(--color-reference)"
                  stopOpacity={0.1}
                />
              </linearGradient>
            </defs>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="beat"
              tickLine={false}
              axisLine={false}
              tickMargin={8}
              minTickGap={32}
              tickFormatter={(v) => `b${Math.round(Number(v) * 4) / 4}`}
            />
            <YAxis
              tickLine={false}
              axisLine={false}
              width={36}
              tickMargin={4}
              tickFormatter={(v) => String(tidy(Number(v), 2))}
            />
            <ChartTooltip
              cursor={false}
              content={
                <ChartTooltipContent
                  labelFormatter={(_, p) =>
                    `frame ${p?.[0]?.payload?.frame} · beat ${p?.[0]?.payload?.beat}`
                  }
                  indicator="dot"
                />
              }
            />
            <Area
              dataKey="reference"
              type="monotone"
              fill="url(#fillReference)"
              stroke="var(--color-reference)"
              connectNulls
            />
            {candidate && (
              <Area
                dataKey="render"
                type="monotone"
                fill="none"
                stroke="var(--color-render)"
                strokeDasharray="4 3"
                connectNulls
              />
            )}
            <ChartLegend content={<ChartLegendContent />} />
          </AreaChart>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}

/** A reference analysis and the comparisons of renders with it. */
export function ReferencePage() {
  const s = useStudio()
  const [analyses, setAnalyses] = React.useState<string[]>([])
  const [analysisPath, setAnalysisPath] = React.useState<string | null>(null)
  const [analysis, setAnalysis] = React.useState<Analysis | null>(null)
  const [reportPath, setReportPath] = React.useState<string | null>(null)
  const [report, setReport] = React.useState<Report | null>(null)
  const [outputs, setOutputs] = React.useState<RenderOutput[]>([])
  const [run, setRun] = React.useState<CompareRun | null>(null)
  const [bump, setBump] = React.useState(0)
  const reports = useReports((s.snapshot?.revision ?? 0) + bump * 1000)

  React.useEffect(() => {
    if (!isTauri()) return
    api.analysesFind().then((list) => {
      setAnalyses(list)
      setAnalysisPath((p) => p ?? list[0] ?? null)
    })
    api.rendersList().then((r) => setOutputs(r.outputs))
    api.compareStatus().then(setRun)
    let un: (() => void) | undefined
    listen<CompareRun>("compare", (e) => {
      setRun(e.payload)
      if (e.payload.report) {
        toast.success("Comparison finished", { description: e.payload.report })
        setReportPath(e.payload.report)
        setBump((b) => b + 1)
      } else if (e.payload.error)
        toast.error("Comparison failed", { description: e.payload.error })
    }).then((u) => (un = u))
    return () => un?.()
  }, [s.snapshot?.path])

  React.useEffect(() => {
    if (!reportPath && reports.length)
      setReportPath(reports[reports.length - 1].path)
  }, [reports, reportPath])

  React.useEffect(() => {
    if (!analysisPath) return setAnalysis(null)
    api
      .analysisLoad(analysisPath)
      .then(setAnalysis)
      .catch((e) =>
        toast.error("Cannot read the analysis", { description: errorText(e) })
      )
  }, [analysisPath])

  React.useEffect(() => {
    if (!reportPath) return setReport(null)
    api
      .reportLoad(reportPath)
      .then(setReport)
      .catch((e) =>
        toast.error("Cannot read the report", { description: errorText(e) })
      )
  }, [reportPath])

  const compare = (video: string) => {
    if (!analysisPath) return
    api
      .compareStart(analysisPath, video)
      .then(() => setRun({ running: true, video, error: null, report: null }))
      .catch((e) =>
        toast.error("Cannot compare", { description: errorText(e) })
      )
  }
  React.useEffect(() => {
    if (pendingCompare.video && analysisPath) {
      compare(pendingCompare.video)
      pendingCompare.video = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [analysisPath])

  const browse = async (what: "analysis" | "report") => {
    const p = await open({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    })
    if (typeof p !== "string") return
    if (what === "analysis") {
      setAnalyses((a) => (a.includes(p) ? a : [...a, p]))
      setAnalysisPath(p)
    } else setReportPath(p)
  }

  const idx = reports.findIndex((r) => r.path === reportPath)
  const prev = idx > 0 ? reports[idx - 1] : null
  const c = report?.comparison ?? null

  const jump = (shot: number | undefined, text: string) => {
    if (!s.info) return
    const sh =
      c?.shots.find((x) => x.shot === shot) ??
      analysis?.shots.find((x) => x.id === shot)
    let t: number | null = null
    if (sh)
      t =
        s.info.time_base === "beats"
          ? sh.beat0
          : sh.f0 / (analysis?.fps ?? s.info.fps)
    else {
      const m = /beat (-?[\d.]+)/.exec(text)
      if (m) t = s.info.time_base === "beats" ? Number(m[1]) : null
      const f = /frame (\d+)/.exec(text)
      if (t == null && f) t = Number(f[1]) / (analysis?.fps ?? s.info.fps)
    }
    if (t == null) return
    s.seek(firstFrameFrom(s.info, t))
    s.setPage("edit")
  }

  const name = (p: string) => p.split("/").slice(-2).join("/")
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="flex flex-col gap-4 py-4 md:gap-6 md:py-6">
        <div className="flex items-center gap-2 px-4 lg:px-6">
          <Select
            items={analyses.map((a) => ({ label: name(a), value: a }))}
            value={analysisPath}
            onValueChange={(v) => v && setAnalysisPath(v)}
          >
            <SelectTrigger size="sm" className="w-64" aria-label="Analysis">
              <SelectValue placeholder="No analysis found" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectLabel>reference_analysis.json</SelectLabel>
                {analyses.map((a) => (
                  <SelectItem key={a} value={a}>
                    {name(a)}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="sm"
            onClick={() => browse("analysis")}
          >
            <FolderOpenIcon data-icon="inline-start" />
            Analysis…
          </Button>
          <Select
            items={reports.map((r) => ({
              label: `${(r.overall * 100).toFixed(1)}% · ${ago(r.time)}`,
              value: r.path,
            }))}
            value={reportPath}
            onValueChange={(v) => v && setReportPath(v)}
          >
            <SelectTrigger size="sm" className="w-48" aria-label="Report">
              <SelectValue placeholder="No comparison yet" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectLabel>Comparisons</SelectLabel>
                {[...reports].reverse().map((r) => (
                  <SelectItem key={r.path} value={r.path}>
                    {(r.overall * 100).toFixed(1)}% · {ago(r.time)}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <Button variant="outline" size="sm" onClick={() => browse("report")}>
            <FolderOpenIcon data-icon="inline-start" />
            Report…
          </Button>
          <div className="ml-auto flex items-center gap-2">
            {run?.running && (
              <Badge variant="outline">
                <Spinner /> comparing {run.video?.split("/").pop()}
              </Badge>
            )}
            <Select
              items={outputs.map((o) => ({ label: o.name, value: o.path }))}
              value={null}
              onValueChange={(v) => v && compare(v)}
              disabled={!analysisPath || run?.running || !outputs.length}
            >
              <SelectTrigger
                size="sm"
                className="w-52"
                aria-label="Compare a render"
              >
                <ScanEyeIcon />
                <SelectValue
                  placeholder={
                    outputs.length
                      ? "Compare a render…"
                      : "No renders to compare"
                  }
                />
              </SelectTrigger>
              <SelectContent align="end">
                <SelectGroup>
                  <SelectLabel>Renders</SelectLabel>
                  {outputs.map((o) => (
                    <SelectItem key={o.path} value={o.path}>
                      {o.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>
        </div>

        {!analysis ? (
          <div className="px-4 lg:px-6">
            <Empty className="border border-dashed">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <ScanEyeIcon />
                </EmptyMedia>
                <EmptyTitle>No reference analysis</EmptyTitle>
                <EmptyDescription>
                  Measure a finished edit with{" "}
                  <code>edits-analyze analyze ref.mp4 -o analysis/</code> (or
                  the agent's <code>reference_analyze</code>); its{" "}
                  <code>reference_analysis.json</code> under the project's
                  folder is found here.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => browse("analysis")}
                >
                  <FolderOpenIcon data-icon="inline-start" />
                  Open an analysis…
                </Button>
              </EmptyContent>
            </Empty>
          </div>
        ) : (
          <>
            <div className="grid grid-cols-1 gap-4 px-4 *:data-[slot=card]:bg-linear-to-t *:data-[slot=card]:from-primary/5 *:data-[slot=card]:to-card *:data-[slot=card]:shadow-xs lg:px-6 @xl/main:grid-cols-3 @5xl/main:grid-cols-6 dark:*:data-[slot=card]:bg-card">
              {DIMENSIONS.map((d) => {
                const v = c ? score(c.scores, c.overall, d.keys) : null
                const p = prev ? score(prev.scores, prev.overall, d.keys) : null
                const delta = v != null && p != null ? v - p : null
                const Icon =
                  delta == null
                    ? null
                    : delta > 0.0005
                      ? TrendingUpIcon
                      : delta < -0.0005
                        ? TrendingDownIcon
                        : MinusIcon
                return (
                  <Card key={d.id} className="@container/card">
                    <CardHeader>
                      <CardDescription>{d.title}</CardDescription>
                      <CardTitle className="text-2xl font-semibold tabular-nums">
                        {v != null ? `${(v * 100).toFixed(1)}%` : "–"}
                      </CardTitle>
                      {Icon && delta != null && (
                        <CardAction>
                          <Badge variant="outline">
                            <Icon />
                            {delta > 0 ? "+" : ""}
                            {(delta * 100).toFixed(1)}
                          </Badge>
                        </CardAction>
                      )}
                    </CardHeader>
                    <CardFooter className="text-sm text-muted-foreground">
                      <span className="line-clamp-2">
                        {c ? d.what : "No comparison yet"}
                      </span>
                    </CardFooter>
                  </Card>
                )
              })}
            </div>
            <div className="px-4 lg:px-6">
              <SignatureChart
                analysis={analysis}
                candidate={report?.candidate_signature ?? null}
              />
            </div>
            <div className="flex flex-col gap-3 px-4 lg:px-6">
              <div className="flex items-center justify-between">
                <h2 className="text-base font-medium">Largest differences</h2>
                <span className="text-sm text-muted-foreground">
                  {analysis.shots.length} shots at {tidy(analysis.bpm, 2)} BPM
                  {c
                    ? ` · cuts ${c.cuts.matched}/${c.cuts.reference} · flashes ${c.flashes.matched}/${c.flashes.reference}`
                    : ""}
                </span>
              </div>
              <div className="overflow-hidden rounded-lg border">
                <Table>
                  <TableHeader className="bg-muted">
                    <TableRow>
                      <TableHead className="w-24">Severity</TableHead>
                      <TableHead className="w-28">Category</TableHead>
                      <TableHead className="w-16">Shot</TableHead>
                      <TableHead>Difference</TableHead>
                      <TableHead className="w-24" />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {c && c.differences.length ? (
                      c.differences.slice(0, 40).map((d, i) => (
                        <TableRow key={i}>
                          <TableCell>
                            <Badge
                              variant={
                                d.severity >= 2 ? "destructive" : "outline"
                              }
                              className="tabular-nums"
                            >
                              {d.severity.toFixed(1)}×
                            </Badge>
                          </TableCell>
                          <TableCell>
                            <Badge
                              variant="outline"
                              className="text-muted-foreground"
                            >
                              {d.category}
                            </Badge>
                          </TableCell>
                          <TableCell className="tabular-nums">
                            {d.shot ?? "–"}
                          </TableCell>
                          <TableCell className="whitespace-normal">
                            {d.text}
                          </TableCell>
                          <TableCell>
                            <Button
                              variant="outline"
                              size="xs"
                              onClick={() => jump(d.shot, d.text)}
                            >
                              <CrosshairIcon data-icon="inline-start" />
                              Jump to
                            </Button>
                          </TableCell>
                        </TableRow>
                      ))
                    ) : (
                      <TableRow>
                        <TableCell
                          colSpan={5}
                          className="h-24 text-center text-muted-foreground"
                        >
                          {c
                            ? "No differences above the tolerances."
                            : "Render the project and compare it with the reference to list the differences."}
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
