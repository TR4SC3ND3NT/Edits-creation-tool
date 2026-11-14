import * as React from "react"
import { listen } from "@tauri-apps/api/event"
import { isTauri } from "@tauri-apps/api/core"
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener"
import {
  EllipsisVerticalIcon,
  FilmIcon,
  PlayIcon,
  ScanEyeIcon,
  SquareIcon,
} from "lucide-react"
import { toast } from "sonner"

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
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { api, errorText, type JobStatus, type RenderOutput } from "@/lib/api"
import { useStudio } from "@/lib/store"
import { ago, bytes } from "@/lib/time"

export const pendingCompare = { video: null as string | null }

const SCALES = [
  { label: "Full size (1×)", value: "1" },
  { label: "Half size (0.5×)", value: "0.5" },
  { label: "Quarter size (0.25×)", value: "0.25" },
]

/** Render to MP4 through `edits render`, and the videos rendered so far. */
export function RendersPage() {
  const s = useStudio()
  const [outputs, setOutputs] = React.useState<RenderOutput[]>([])
  const [job, setJob] = React.useState<JobStatus | null>(null)
  const [output, setOutput] = React.useState("")
  const [scale, setScale] = React.useState("1")
  const [edits, setEdits] = React.useState<string | null>(null)

  const refresh = React.useCallback(() => {
    if (!isTauri()) return
    api
      .rendersList()
      .then((r) => {
        setOutputs(r.outputs)
        setJob((j) => j ?? r.job)
        setEdits(r.edits)
        setOutput((o) => {
          if (o) return o
          const d = r.default_output ?? ""
          const dir = s.snapshot ? `${s.snapshot.dir}/` : ""
          return dir && d.startsWith(dir) ? d.slice(dir.length) : d
        })
      })
      .catch(() => {})
  }, [s.snapshot])
  React.useEffect(refresh, [refresh, s.snapshot?.path])
  React.useEffect(() => {
    if (!isTauri()) return
    let un: (() => void) | undefined
    listen<JobStatus>("render-job", (e) => {
      setJob(e.payload)
      if (!e.payload.running) {
        refresh()
        setOutput("")
        if (e.payload.outcome === "ok")
          toast.success("Render finished", { description: e.payload.output })
        else if (e.payload.outcome === "failed")
          toast.error("Render failed", { description: e.payload.error ?? "" })
        else toast("Render cancelled")
      }
    }).then((u) => (un = u))
    return () => un?.()
  }, [refresh])

  const start = async () => {
    try {
      setJob(await api.renderStart(output || null, Number(scale)))
    } catch (e) {
      toast.error("Cannot render", { description: errorText(e) })
    }
  }
  const pct = job && job.total ? Math.round((job.done / job.total) * 100) : 0
  const left =
    job && job.running && job.done > 0
      ? (job.elapsed / job.done) * (job.total - job.done)
      : null
  const fail = (e: unknown) =>
    toast.error("Cannot open it", { description: errorText(e) })

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4 md:gap-6 md:p-6">
      <Card>
        <CardHeader>
          <CardTitle>Render video</CardTitle>
          <CardDescription>
            Runs <code>edits render</code> on the project file as it is on disk,
            with its audio and motion blur, to an H.264 MP4.
          </CardDescription>
          <CardAction>
            <Badge variant="outline">
              {edits ? "edits found" : "edits not found"}
            </Badge>
          </CardAction>
        </CardHeader>
        <CardContent>
          <FieldGroup className="grid gap-4 md:grid-cols-[1fr_14rem]">
            <Field>
              <FieldLabel htmlFor="render-output">Output file</FieldLabel>
              <Input
                id="render-output"
                value={output}
                onChange={(e) => setOutput(e.target.value)}
                placeholder="renders/edit.mp4"
                className="ui-selectable"
              />
              <FieldDescription>
                Relative paths are inside the project's folder.
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>Size</FieldLabel>
              <Select
                items={SCALES}
                value={scale}
                onValueChange={(v) => v && setScale(v)}
              >
                <SelectTrigger className="w-full">
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
              <FieldDescription>
                {s.info
                  ? `${Math.round(s.info.width * Number(scale))} × ${Math.round(s.info.height * Number(scale))}, ${s.info.frames} frames`
                  : ""}
              </FieldDescription>
            </Field>
          </FieldGroup>
        </CardContent>
        <CardFooter className="flex-col items-stretch gap-3 border-t">
          {job && (
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between text-sm">
                <span className="truncate text-muted-foreground">
                  {job.output}
                </span>
                <span className="tabular-nums">
                  {job.running
                    ? `frame ${job.done} / ${job.total || "…"}`
                    : job.outcome === "ok"
                      ? `done in ${job.elapsed.toFixed(1)} s`
                      : job.outcome}
                </span>
              </div>
              <Progress value={job.outcome === "ok" ? 100 : pct} />
              <div className="flex justify-between text-xs text-muted-foreground tabular-nums">
                <span>{job.error ?? job.line}</span>
                <span>{left != null ? `${Math.ceil(left)} s left` : ""}</span>
              </div>
            </div>
          )}
          <div className="flex justify-end gap-2">
            {job?.running ? (
              <Button variant="outline" onClick={() => api.renderCancel()}>
                <SquareIcon data-icon="inline-start" />
                Cancel
              </Button>
            ) : (
              <Button onClick={start} disabled={!edits}>
                <PlayIcon data-icon="inline-start" />
                Render
              </Button>
            )}
          </div>
        </CardFooter>
      </Card>
      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-medium">Outputs</h2>
          <span className="text-sm text-muted-foreground">
            Videos next to the project and in its renders/ folder
          </span>
        </div>
        {outputs.length === 0 ? (
          <Empty className="border border-dashed">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <FilmIcon />
              </EmptyMedia>
              <EmptyTitle>No renders yet</EmptyTitle>
              <EmptyDescription>
                Render the project and the video appears here.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="overflow-hidden rounded-lg border">
            <Table>
              <TableHeader className="bg-muted">
                <TableRow>
                  <TableHead>File</TableHead>
                  <TableHead>Folder</TableHead>
                  <TableHead className="text-right">Size</TableHead>
                  <TableHead>Rendered</TableHead>
                  <TableHead className="w-10" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {outputs.map((o) => (
                  <TableRow key={o.path}>
                    <TableCell className="font-medium">{o.name}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {o.folder}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {bytes(o.bytes)}
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {ago(o.modified)}
                    </TableCell>
                    <TableCell>
                      <DropdownMenu>
                        <DropdownMenuTrigger
                          render={
                            <Button
                              variant="ghost"
                              size="icon"
                              className="size-8 text-muted-foreground"
                            />
                          }
                        >
                          <EllipsisVerticalIcon />
                          <span className="sr-only">Open menu</span>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-48">
                          <DropdownMenuItem
                            onClick={() => openPath(o.path).catch(fail)}
                          >
                            Play
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onClick={() => revealItemInDir(o.path).catch(fail)}
                          >
                            Show in folder
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => {
                              pendingCompare.video = o.path
                              s.setPage("reference")
                            }}
                          >
                            <ScanEyeIcon />
                            Compare with reference
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </div>
    </div>
  )
}
