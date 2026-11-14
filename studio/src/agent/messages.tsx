// What a thread shows: the person's messages, the agent's text (markdown),
// its thinking (collapsed), tool calls as cards with their status, diffs and
// output, permission questions with their buttons, and the plan.

import * as React from "react"
import Markdown from "react-markdown"
import remarkGfm from "remark-gfm"
import {
  BrainIcon,
  CheckIcon,
  ChevronRightIcon,
  CircleCheckIcon,
  CircleDashedIcon,
  CircleDotIcon,
  CircleXIcon,
  FileTextIcon,
  GlobeIcon,
  ListTodoIcon,
  MapPinIcon,
  PencilIcon,
  SearchIcon,
  ShieldQuestionIcon,
  SquareTerminalIcon,
  Trash2Icon,
  TriangleAlertIcon,
  WrenchIcon,
  XIcon,
} from "lucide-react"
import { cn } from "cn"

import type {
  ContentBlock,
  PlanEntry,
  ToolCall,
  ToolContent,
} from "@/agent/api"
import type { Item } from "@/agent/store"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { Spinner } from "@/components/ui/spinner"

// ---- markdown --------------------------------------------------------------------------------------

export function Md({ text }: { text: string }) {
  return (
    <div className="flex flex-col gap-2 text-sm leading-relaxed break-words">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          p: (p) => <p className="leading-relaxed" {...p} />,
          a: ({ href, children }) => (
            <a
              href={href}
              className="underline underline-offset-2"
              target="_blank"
              rel="noreferrer"
            >
              {children}
            </a>
          ),
          ul: (p) => <ul className="ml-4 list-disc space-y-1" {...p} />,
          ol: (p) => <ol className="ml-4 list-decimal space-y-1" {...p} />,
          h1: (p) => <h3 className="text-base font-semibold" {...p} />,
          h2: (p) => <h3 className="text-sm font-semibold" {...p} />,
          h3: (p) => <h4 className="text-sm font-semibold" {...p} />,
          blockquote: (p) => (
            <blockquote
              className="border-l-2 pl-3 text-muted-foreground"
              {...p}
            />
          ),
          table: (p) => (
            <div className="overflow-x-auto">
              <table className="w-full text-xs" {...p} />
            </div>
          ),
          th: (p) => (
            <th className="border-b px-2 py-1 text-left font-medium" {...p} />
          ),
          td: (p) => <td className="border-b px-2 py-1" {...p} />,
          pre: ({ children }) => (
            <pre className="overflow-x-auto rounded-md border bg-muted/50 p-3 font-mono text-xs leading-normal">
              {children}
            </pre>
          ),
          code: ({ className, children }) =>
            className ? (
              <code className={cn("font-mono", className)}>{children}</code>
            ) : (
              <code className="rounded bg-muted px-1 py-0.5 font-mono text-[0.8em]">
                {children}
              </code>
            ),
        }}
      >
        {text}
      </Markdown>
    </div>
  )
}

// ---- the person ---------------------------------------------------------------------------------------

export function UserMessage({ blocks }: { blocks: ContentBlock[] }) {
  const text = blocks
    .filter((b) => b.type === "text")
    .map((b) => (b as { text: string }).text)
    .join("\n")
  const images = blocks.filter((b) => b.type === "image") as {
    data: string
    mimeType: string
  }[]
  const refs = blocks.filter(
    (b) => b.type === "resource" || b.type === "resource_link"
  )
  return (
    <div className="ml-8 flex flex-col items-end gap-1.5">
      {images.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5">
          {images.map((im, i) => (
            <img
              key={i}
              src={`data:${im.mimeType};base64,${im.data}`}
              className="h-16 rounded-md border object-cover"
              alt="attached"
            />
          ))}
        </div>
      )}
      {refs.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1">
          {refs.map((r, i) => (
            <Badge key={i} variant="outline" className="font-normal">
              @{refName(r)}
            </Badge>
          ))}
        </div>
      )}
      {text && (
        <div className="rounded-lg bg-muted px-3 py-2 text-sm whitespace-pre-wrap">
          {text}
        </div>
      )}
    </div>
  )
}

function refName(b: ContentBlock) {
  if (b.type === "resource_link") return b.title ?? b.name
  if (b.type === "resource") {
    const u = b.resource.uri
    return decodeURIComponent(u.split("#").pop() ?? u).replace(
      /^\/layers\/@?/,
      ""
    )
  }
  return ""
}

// ---- thinking ---------------------------------------------------------------------------------------------

export function Thought({ text, live }: { text: string; live: boolean }) {
  return (
    <Collapsible>
      <CollapsibleTrigger className="group/t flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground">
        <ChevronRightIcon className="size-3.5 transition-transform group-data-[panel-open]/t:rotate-90" />
        <BrainIcon className="size-3.5" />
        {live ? "Thinking…" : "Thought"}
      </CollapsibleTrigger>
      <CollapsibleContent>
        <div className="mt-1.5 border-l-2 pl-3 text-xs text-muted-foreground">
          <Md text={text} />
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}

// ---- tool calls -------------------------------------------------------------------------------------------

const KIND_ICON: Record<string, React.ComponentType<{ className?: string }>> = {
  read: FileTextIcon,
  edit: PencilIcon,
  delete: Trash2Icon,
  move: MapPinIcon,
  search: SearchIcon,
  execute: SquareTerminalIcon,
  think: BrainIcon,
  fetch: GlobeIcon,
}

export function StatusBadge({ status }: { status?: string | null }) {
  switch (status) {
    case "completed":
      return (
        <Badge variant="secondary" className="gap-1">
          <CheckIcon />
          done
        </Badge>
      )
    case "failed":
      return (
        <Badge variant="destructive" className="gap-1">
          <XIcon />
          failed
        </Badge>
      )
    case "in_progress":
      return (
        <Badge variant="outline" className="gap-1">
          <Spinner className="size-3" />
          running
        </Badge>
      )
    default:
      return (
        <Badge variant="outline" className="gap-1 text-muted-foreground">
          <CircleDashedIcon />
          pending
        </Badge>
      )
  }
}

/** A tool's name as the person reads it: `edits · project_edit`, `Read`, `Bash`. */
export function toolTitle(call: ToolCall, tool: string | null) {
  if (tool) return `edits · ${tool}`
  return call.title || call.name || "tool"
}

function json(v: unknown) {
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

/** A line diff, by the longest common subsequence (small texts), else the whole of both. */
export function diffLines(
  a: string,
  b: string
): { t: " " | "-" | "+"; s: string }[] {
  const x = a.split("\n")
  const y = b.split("\n")
  if (x.length * y.length > 250_000)
    return [
      ...x.map((s) => ({ t: "-" as const, s })),
      ...y.map((s) => ({ t: "+" as const, s })),
    ]
  const n = x.length
  const m = y.length
  const L: number[][] = Array.from({ length: n + 1 }, () =>
    new Array(m + 1).fill(0)
  )
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      L[i][j] =
        x[i] === y[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1])
  const out: { t: " " | "-" | "+"; s: string }[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      out.push({ t: " ", s: x[i] })
      i++
      j++
    } else if (L[i + 1][j] >= L[i][j + 1]) out.push({ t: "-", s: x[i++] })
    else out.push({ t: "+", s: y[j++] })
  }
  while (i < n) out.push({ t: "-", s: x[i++] })
  while (j < m) out.push({ t: "+", s: y[j++] })
  return out
}

function Diff({
  path,
  oldText,
  newText,
}: {
  path: string
  oldText?: string | null
  newText: string
}) {
  const lines = diffLines(oldText ?? "", newText)
  // Unchanged runs longer than 6 lines are folded to their ends.
  const shown: ({ t: " " | "-" | "+"; s: string } | { fold: number })[] = []
  let run: { t: " " | "-" | "+"; s: string }[] = []
  const flush = (end: boolean) => {
    if (run.length > 6) {
      shown.push(...(shown.length ? run.slice(0, 2) : []))
      shown.push({ fold: run.length - (shown.length ? 2 : 0) - (end ? 0 : 2) })
      if (!end) shown.push(...run.slice(-2))
    } else shown.push(...run)
    run = []
  }
  for (const l of lines) {
    if (l.t === " ") run.push(l)
    else {
      flush(false)
      shown.push(l)
    }
  }
  flush(true)
  return (
    <div className="overflow-hidden rounded-md border">
      <div className="border-b bg-muted/50 px-2 py-1 font-mono text-[11px] text-muted-foreground">
        {path}
      </div>
      <pre className="max-h-72 overflow-auto p-0 font-mono text-[11px] leading-snug">
        {shown.map((l, i) =>
          "fold" in l ? (
            <div key={i} className="px-2 text-muted-foreground">
              ⋯ {l.fold} unchanged lines
            </div>
          ) : (
            <div
              key={i}
              className={cn(
                "px-2 whitespace-pre-wrap",
                l.t === "-" && "bg-destructive/10 text-destructive",
                l.t === "+" && "bg-primary/10"
              )}
            >
              {l.t} {l.s}
            </div>
          )
        )}
      </pre>
    </div>
  )
}

function Content({ c }: { c: ToolContent }) {
  if (c.type === "diff")
    return <Diff path={c.path} oldText={c.oldText} newText={c.newText} />
  if (c.type === "terminal")
    return (
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <SquareTerminalIcon className="size-3.5" /> terminal {c.terminalId}
      </div>
    )
  const b = c.content
  if (b.type === "image")
    return (
      <img
        src={`data:${b.mimeType};base64,${b.data}`}
        alt="tool output"
        className="max-h-56 rounded-md border object-contain"
      />
    )
  if (b.type === "text") {
    const t = b.text.trim()
    if (t.startsWith("```") || /\n/.test(t) === false) return <Md text={t} />
    return (
      <pre className="max-h-60 overflow-auto rounded-md bg-muted/50 p-2 font-mono text-[11px] whitespace-pre-wrap">
        {t}
      </pre>
    )
  }
  if (b.type === "resource_link")
    return <Badge variant="outline">{b.title ?? b.name}</Badge>
  return null
}

export function ToolCard({
  call,
  tool,
  label,
}: {
  call: ToolCall
  tool: string | null
  label: string | null
}) {
  const Icon = (call.kind && KIND_ICON[call.kind]) || WrenchIcon
  const hasBody =
    (call.content?.length ?? 0) > 0 ||
    call.rawInput != null ||
    call.rawOutput != null
  return (
    <Collapsible>
      <Card size="sm" className="gap-0 py-0">
        <CollapsibleTrigger
          className="group/tc flex w-full items-center gap-2 px-3 py-2 text-left text-sm"
          disabled={!hasBody}
        >
          <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform group-data-[panel-open]/tc:rotate-90" />
          <Icon className="size-4 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1">
            <span className="block truncate font-medium">
              {toolTitle(call, tool)}
            </span>
            {label && (
              <span className="block truncate text-xs text-muted-foreground">
                {label}
              </span>
            )}
          </span>
          <StatusBadge status={call.status} />
        </CollapsibleTrigger>
        <CollapsibleContent>
          <CardContent className="flex flex-col gap-2 border-t px-3 py-2">
            {call.locations?.map((l, i) => (
              <div
                key={i}
                className="truncate font-mono text-[11px] text-muted-foreground"
              >
                {l.path}
                {l.line != null ? `:${l.line}` : ""}
              </div>
            ))}
            {call.rawInput != null && (
              <pre className="max-h-48 overflow-auto rounded-md bg-muted/50 p-2 font-mono text-[11px]">
                {json(call.rawInput)}
              </pre>
            )}
            {call.content?.map((c, i) => (
              <Content key={i} c={c} />
            ))}
            {!call.content?.length && call.rawOutput != null && (
              <pre className="max-h-48 overflow-auto rounded-md bg-muted/50 p-2 font-mono text-[11px] whitespace-pre-wrap">
                {typeof call.rawOutput === "string"
                  ? call.rawOutput
                  : json(call.rawOutput)}
              </pre>
            )}
          </CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  )
}

// ---- permission ---------------------------------------------------------------------------------------------

export function PermissionCard({
  item,
  agent,
  onAnswer,
}: {
  item: Item & { kind: "permission" }
  agent: string
  onAnswer: (option: string | null) => void
}) {
  const chosen = item.options.find((o) => o.optionId === item.answer)
  const title = item.presence?.label ?? item.toolCall.title ?? "a tool"
  return (
    <Card size="sm" className={cn("gap-2", !item.answer && "ring-2 ring-ring")}>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-sm">
          <ShieldQuestionIcon className="size-4 text-muted-foreground" />
          {item.presence ? `${agent} asks permission for ${title}` : `${agent} asks to run ${title}`}
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {item.toolCall.rawInput != null && (
          <pre className="max-h-32 overflow-auto rounded-md bg-muted/50 p-2 font-mono text-[11px]">
            {json(item.toolCall.rawInput)}
          </pre>
        )}
        {item.answer ? (
          <Badge
            variant={chosen?.kind.startsWith("allow") ? "secondary" : "outline"}
          >
            {chosen ? chosen.name : "Cancelled"}
          </Badge>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {item.options.map((o) => (
              <Button
                key={o.optionId}
                size="sm"
                variant={
                  o.kind === "allow_once"
                    ? "default"
                    : o.kind.startsWith("reject")
                      ? "outline"
                      : "secondary"
                }
                onClick={() => onAnswer(o.optionId)}
              >
                {o.kind.startsWith("allow") ? (
                  <CheckIcon data-icon="inline-start" />
                ) : (
                  <XIcon data-icon="inline-start" />
                )}
                {o.name}
              </Button>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

// ---- plan ---------------------------------------------------------------------------------------------------------

export function PlanCard({ entries }: { entries: PlanEntry[] }) {
  const done = entries.filter((e) => e.status === "completed").length
  return (
    <Collapsible defaultOpen>
      <CollapsibleTrigger className="group/p flex w-full items-center gap-2 text-xs text-muted-foreground hover:text-foreground">
        <ChevronRightIcon className="size-3.5 transition-transform group-data-[panel-open]/p:rotate-90" />
        <ListTodoIcon className="size-3.5" />
        Plan
        <span className="ml-auto tabular-nums">
          {done}/{entries.length}
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <ul className="mt-1.5 flex flex-col gap-1">
          {entries.map((e, i) => (
            <li key={i} className="flex items-start gap-2 text-xs">
              {e.status === "completed" ? (
                <CircleCheckIcon className="mt-px size-3.5 shrink-0 text-muted-foreground" />
              ) : e.status === "in_progress" ? (
                <CircleDotIcon className="mt-px size-3.5 shrink-0 text-primary" />
              ) : (
                <CircleDashedIcon className="mt-px size-3.5 shrink-0 text-muted-foreground" />
              )}
              <span
                className={cn(
                  e.status === "completed" &&
                    "text-muted-foreground line-through"
                )}
              >
                {e.content}
              </span>
            </li>
          ))}
        </ul>
      </CollapsibleContent>
    </Collapsible>
  )
}

// ---- notices -------------------------------------------------------------------------------------------------------

export function NoticeItem({
  severity,
  title,
  description,
}: {
  severity: string
  title: string
  description?: string | null
}) {
  return (
    <Alert variant={severity === "error" ? "destructive" : "default"}>
      {severity === "error" ? <CircleXIcon /> : <TriangleAlertIcon />}
      <AlertTitle>{title}</AlertTitle>
      {description && <AlertDescription>{description}</AlertDescription>}
    </Alert>
  )
}
