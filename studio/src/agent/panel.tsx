// The agent panel on the right of every page: Claude Code or Codex (or any
// ACP agent) in a thread, as in Zed's agent panel. Header: the agent, the
// threads, "follow agent", Chat / Steps, collapse. Body: the thread, the
// plan, the message box. Before a connection: which agent, how it starts,
// and how to log in.

import * as React from "react"
import {
  BotIcon,
  CheckIcon,
  CopyIcon,
  CrosshairIcon,
  HistoryIcon,
  LogInIcon,
  MessageSquarePlusIcon,
  MessagesSquareIcon,
  PanelRightCloseIcon,
  PlugIcon,
  PowerIcon,
  RotateCwIcon,
  SquareTerminalIcon,
  TriangleAlertIcon,
} from "lucide-react"

import { agentApi, agentErrorText, type AuthMethod } from "@/agent/api"
import { Composer } from "@/agent/composer"
import {
  Md,
  NoticeItem,
  PermissionCard,
  PlanCard,
  Thought,
  ToolCard,
  UserMessage,
} from "@/agent/messages"
import { useFollowAgent } from "@/agent/presence"
import { AgentAvatar, AgentSteps } from "@/agent/presence-ui"
import { useAgent, type Item } from "@/agent/store"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Toggle } from "@/components/ui/toggle"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

function Kbdish({ children }: { children: React.ReactNode }) {
  return (
    <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">
      {children}
    </code>
  )
}

/** A line of text with `code` parts. */
function Inline({ text }: { text: string }) {
  return (
    <>
      {text
        .split(/(`[^`]+`)/)
        .map((part, i) =>
          part.startsWith("`") ? (
            <Kbdish key={i}>{part.slice(1, -1)}</Kbdish>
          ) : (
            <span key={i}>{part}</span>
          )
        )}
    </>
  )
}

function CopyButton({ text }: { text: string }) {
  const [done, setDone] = React.useState(false)
  return (
    <Button
      variant="ghost"
      size="icon-xs"
      aria-label="Copy"
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setDone(true)
          window.setTimeout(() => setDone(false), 1500)
        })
      }}
    >
      {done ? <CheckIcon /> : <CopyIcon />}
    </Button>
  )
}

// ---- not connected ---------------------------------------------------------------------------------------

function Setup() {
  const a = useAgent()
  const [setup, setSetup] = React.useState<{
    mcp: string | null
    skill: string | null
    projectDir: string | null
  } | null>(null)
  React.useEffect(() => {
    agentApi
      .setup()
      .then(setSetup)
      .catch(() => {})
  }, [])
  const adapter = a.adapters.find((x) => x.id === a.adapterId)
  const c = a.connection
  return (
    <div className="flex flex-col gap-3 p-3">
      <Card size="sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BotIcon className="size-4" />
            Work with an agent
          </CardTitle>
          <CardDescription>
            Chat with Claude Code or Codex here. It edits this project through
            the edits tools, and you see where it works: the layer, the field,
            the time, the frame it looks at.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-xs">
          <Select
            items={a.adapters.map((x) => ({ label: x.name, value: x.id }))}
            value={a.adapterId}
            onValueChange={(v) => v && a.setAdapterId(String(v))}
          >
            <SelectTrigger className="w-full" aria-label="Agent">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                {a.adapters.map((x) => (
                  <SelectItem key={x.id} value={x.id}>
                    {x.name}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          {a.adapterId === "custom" && (
            <Input
              value={a.customCommand}
              placeholder="a command that speaks ACP on stdio"
              onChange={(e) => a.setCustomCommand(e.target.value)}
              className="font-mono text-xs"
            />
          )}
          {adapter && adapter.command.length > 0 && (
            <div className="flex items-center gap-1 text-muted-foreground">
              <span className="shrink-0">Runs</span>
              <Kbdish>
                {adapter.command.map((w) => w.split("/").pop()).join(" ")}
              </Kbdish>
              <Badge variant="outline" className="ml-auto shrink-0">
                {adapter.source === "npx"
                  ? "via npx"
                  : adapter.source === "path"
                    ? "installed"
                    : "set by env"}
              </Badge>
            </div>
          )}
          {adapter?.install && (
            <p className="text-muted-foreground">
              <Inline text={adapter.install} />
            </p>
          )}
          <div className="flex flex-col gap-1 text-muted-foreground">
            <span className="flex items-center gap-1.5">
              {setup?.mcp ? (
                <CheckIcon className="size-3.5" />
              ) : (
                <TriangleAlertIcon className="size-3.5" />
              )}
              {setup?.mcp
                ? "edits MCP server found"
                : "edits MCP server not built: cargo build --release -p edits-mcp"}
            </span>
            <span className="flex items-center gap-1.5">
              {setup?.skill ? (
                <CheckIcon className="size-3.5" />
              ) : (
                <TriangleAlertIcon className="size-3.5" />
              )}
              {setup?.skill
                ? "edits skill found"
                : "edits skill not found (set EDITS_SKILL)"}
            </span>
          </div>
        </CardContent>
        <CardFooter className="gap-2">
          <Button
            className="flex-1"
            disabled={
              c.status === "connecting" ||
              !adapter ||
              (adapter.command.length === 0 && a.adapterId !== "custom")
            }
            onClick={() => void a.connect()}
          >
            {c.status === "connecting" ? (
              <Spinner data-icon="inline-start" />
            ) : (
              <PlugIcon data-icon="inline-start" />
            )}
            {c.status === "connecting"
              ? `Starting ${adapter?.name ?? "the agent"}…`
              : `Connect ${adapter?.name ?? ""}`}
          </Button>
        </CardFooter>
      </Card>
      {c.status === "connecting" && adapter?.source === "npx" && (
        <p className="px-1 text-xs text-muted-foreground">
          The first start downloads the adapter with npx; it can take a minute.
        </p>
      )}
      {c.status === "error" && (
        <Alert variant="destructive">
          <TriangleAlertIcon />
          <AlertTitle>{agentErrorText(c.error)}</AlertTitle>
          {c.error.stderr.length > 0 && (
            <AlertDescription>
              <pre className="max-h-40 overflow-auto font-mono text-[11px] whitespace-pre-wrap">
                {c.error.stderr.slice(-8).join("\n")}
              </pre>
            </AlertDescription>
          )}
        </Alert>
      )}
    </div>
  )
}

function Login() {
  const a = useAgent()
  const c = a.connection
  const [busy, setBusy] = React.useState<string | null>(null)
  const [cmds, setCmds] = React.useState<Record<string, string>>({})
  const [note, setNote] = React.useState<string | null>(null)
  const methods: AuthMethod[] =
    c.status === "login" ? (c.init?.authMethods ?? []) : []
  React.useEffect(() => {
    for (const m of methods)
      if (m.type === "terminal")
        agentApi
          .login(m.id, false)
          .then((r) => setCmds((x) => ({ ...x, [m.id]: r.command.join(" ") })))
          .catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [c.status])
  if (c.status !== "login") return null
  return (
    <div className="flex flex-col gap-3 p-3">
      <Card size="sm">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <LogInIcon className="size-4" />
            Log in to {c.adapter.name}
          </CardTitle>
          <CardDescription>{c.error.message}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2 text-xs">
          <ol className="ml-4 list-decimal space-y-1.5">
            {c.adapter.login.map((l, i) => (
              <li key={i}>
                <Inline text={l} />
              </li>
            ))}
          </ol>
          {methods.length > 0 && (
            <div className="flex flex-col gap-1.5 pt-1">
              {methods.map((m) =>
                m.type === "terminal" ? (
                  <div
                    key={m.id}
                    className="flex flex-col gap-1 rounded-md border p-2"
                  >
                    <div className="flex items-center gap-2">
                      <SquareTerminalIcon className="size-3.5" />
                      <span className="font-medium">{m.name}</span>
                      <Button
                        size="xs"
                        variant="outline"
                        className="ml-auto"
                        disabled={busy === m.id}
                        onClick={() => {
                          setBusy(m.id)
                          agentApi
                            .login(m.id, true)
                            .then((r) =>
                              setNote(
                                `Opened ${r.terminal}: log in there, then press Retry.`
                              )
                            )
                            .catch((e) => setNote(agentErrorText(e)))
                            .finally(() => setBusy(null))
                        }}
                      >
                        Open terminal
                      </Button>
                    </div>
                    {cmds[m.id] && (
                      <div className="flex items-center gap-1">
                        <Kbdish>{cmds[m.id]}</Kbdish>
                        <CopyButton text={cmds[m.id]} />
                      </div>
                    )}
                    {m.description && (
                      <span className="text-muted-foreground">
                        {m.description}
                      </span>
                    )}
                  </div>
                ) : (
                  <Button
                    key={m.id}
                    variant="outline"
                    size="sm"
                    disabled={busy === m.id || m.id === "api-key"}
                    onClick={() => {
                      setBusy(m.id)
                      agentApi
                        .authenticate(m.id)
                        .then(() => a.newThread())
                        .catch((e) => setNote(agentErrorText(e)))
                        .finally(() => setBusy(null))
                    }}
                  >
                    {busy === m.id ? (
                      <Spinner data-icon="inline-start" />
                    ) : (
                      <LogInIcon data-icon="inline-start" />
                    )}
                    {m.id === "api-key"
                      ? `${m.name}: set it in your environment, not here`
                      : `Log in: ${m.name}`}
                  </Button>
                )
              )}
            </div>
          )}
          {note && <p className="text-muted-foreground">{note}</p>}
          <p className="text-muted-foreground">
            The studio never asks for or stores a key: the agent uses its own
            login.
          </p>
        </CardContent>
        <CardFooter className="gap-2">
          <Button className="flex-1" onClick={() => void a.newThread()}>
            <RotateCwIcon data-icon="inline-start" />
            Retry
          </Button>
          <Button variant="outline" onClick={() => void a.disconnect()}>
            Back
          </Button>
        </CardFooter>
      </Card>
    </div>
  )
}

// ---- the thread ----------------------------------------------------------------------------------------------

function ItemView({ item, live }: { item: Item; live: boolean }) {
  const a = useAgent()
  switch (item.kind) {
    case "user":
      return <UserMessage blocks={item.blocks} />
    case "agent":
      return <Md text={item.text} />
    case "thought":
      return <Thought text={item.text} live={live} />
    case "tool":
      return (
        <ToolCard
          call={item.call}
          tool={item.presence?.tool ?? null}
          label={item.presence?.label ?? null}
        />
      )
    case "permission":
      return (
        <PermissionCard
          item={item}
          agent={a.agentName}
          onAnswer={(o) => void a.answer(item.id, o)}
        />
      )
    case "notice":
      return (
        <NoticeItem
          severity={item.severity}
          title={item.title}
          description={item.description}
        />
      )
    case "error":
      return (
        <Alert variant="destructive">
          <TriangleAlertIcon />
          <AlertTitle>{item.text}</AlertTitle>
        </Alert>
      )
  }
}

function tokens(n: number) {
  return n >= 1000 ? `${Math.round(n / 100) / 10}k` : String(n)
}

function ThreadView() {
  const a = useAgent()
  const th = a.thread
  const end = React.useRef<HTMLDivElement>(null)
  const count = th?.items.length ?? 0
  const lastLen =
    th?.items[count - 1]?.kind === "agent"
      ? (th.items[count - 1] as { text: string }).text.length
      : 0
  React.useEffect(() => {
    end.current?.scrollIntoView({ block: "end" })
  }, [count, lastLen, th?.plan.length])
  if (!th) return null
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 p-3">
          {th.items.length === 0 && (
            <div className="flex flex-col items-center gap-2 py-8 text-center text-xs text-muted-foreground">
              <AgentAvatar name={a.agentName} size="default" />
              <span>
                Ask {a.agentName} to change the edit: "flash on every snare in
                bar 3", "make d punch harder at the drop". Type{" "}
                <Kbdish>@</Kbdish> for a layer, an effect or the current frame,{" "}
                <Kbdish>/</Kbdish> for commands.
              </span>
              {!th.mcp && (
                <Badge variant="outline" className="gap-1">
                  <TriangleAlertIcon />
                  the edits MCP server is not built: the agent cannot use the
                  edits tools
                </Badge>
              )}
            </div>
          )}
          {th.items.map((it, i) => (
            <ItemView
              key={it.id}
              item={it}
              live={th.busy && i === th.items.length - 1}
            />
          ))}
          {th.busy && (
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <Spinner className="size-3.5" /> {a.agentName} is working…
            </div>
          )}
          {th.stopReason && th.stopReason !== "end_turn" && !th.busy && (
            <Badge variant="outline" className="self-start">
              stopped: {th.stopReason.replace(/_/g, " ")}
            </Badge>
          )}
          <div ref={end} />
        </div>
      </ScrollArea>
      <div className="flex flex-col gap-2 border-t p-3">
        {th.plan.length > 0 && <PlanCard entries={th.plan} />}
        <Composer />
        {th.usage && (
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground tabular-nums">
            <span>
              {tokens(th.usage.used)} / {tokens(th.usage.size)} tokens
            </span>
            {th.usage.cost && (
              <span>
                · {th.usage.cost.amount.toFixed(2)} {th.usage.cost.currency}
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

// ---- the panel -------------------------------------------------------------------------------------------------

export function AgentPanel() {
  const a = useAgent()
  const [follow, setFollow] = useFollowAgent()
  const [tab, setTab] = React.useState("chat")
  const c = a.connection
  const ready = c.status === "ready"
  const title = a.thread?.title ?? (ready ? "New thread" : "Agent")
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex h-11 shrink-0 items-center gap-1.5 border-b px-2">
        <AgentAvatar name={a.agentName} live={a.thread?.busy} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">{title}</div>
          <div className="truncate text-[11px] text-muted-foreground">
            {ready
              ? `${c.adapter.name}${c.init.agentInfo?.version ? ` ${c.init.agentInfo.version}` : ""}`
              : c.status === "login"
                ? "needs a login"
                : c.status === "connecting"
                  ? "starting…"
                  : "not connected"}
          </div>
        </div>
        <Tooltip>
          <TooltipTrigger
            render={
              <Toggle
                size="sm"
                pressed={follow}
                onPressedChange={setFollow}
                aria-label="Follow agent"
                className="h-7 px-1.5"
              />
            }
          >
            <CrosshairIcon />
          </TooltipTrigger>
          <TooltipContent>
            Follow the agent: move the playhead and the selection to what it
            works on
          </TooltipContent>
        </Tooltip>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Threads"
                onClick={() => void a.refreshKnown()}
              />
            }
          >
            <MessagesSquareIcon />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-64">
            <DropdownMenuGroup>
              <DropdownMenuItem
                disabled={!ready}
                onClick={() => void a.newThread()}
              >
                <MessageSquarePlusIcon />
                New thread
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuLabel>Threads</DropdownMenuLabel>
              {Object.values(a.threads).map((t) => (
                <DropdownMenuItem
                  key={t.sessionId}
                  onClick={() => void a.openThread(t.sessionId)}
                >
                  {t.sessionId === a.current ? (
                    <CheckIcon />
                  ) : (
                    <span className="size-4" />
                  )}
                  <span className="truncate">{t.title}</span>
                </DropdownMenuItem>
              ))}
              {a.known
                .filter((k) => !a.threads[k.sessionId])
                .slice(0, 12)
                .map((k) => (
                  <DropdownMenuItem
                    key={k.sessionId}
                    disabled={!ready}
                    onClick={() => void a.openThread(k.sessionId)}
                  >
                    <HistoryIcon />
                    <span className="truncate">{k.title}</span>
                  </DropdownMenuItem>
                ))}
            </DropdownMenuGroup>
            {ready && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => void a.disconnect()}>
                  <PowerIcon />
                  Disconnect {c.adapter.name}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="New thread"
                disabled={!ready}
                onClick={() => void a.newThread()}
              />
            }
          >
            <MessageSquarePlusIcon />
          </TooltipTrigger>
          <TooltipContent>New thread</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Hide the agent panel"
                onClick={() => a.setOpen(false)}
              />
            }
          >
            <PanelRightCloseIcon />
          </TooltipTrigger>
          <TooltipContent>Hide (Ctrl+L)</TooltipContent>
        </Tooltip>
      </div>
      <Tabs
        value={tab}
        onValueChange={(v) => setTab(String(v))}
        className="flex min-h-0 flex-1 flex-col gap-0"
      >
        <div className="border-b px-2 py-1.5">
          <TabsList className="h-7 w-full">
            <TabsTrigger value="chat">Chat</TabsTrigger>
            <TabsTrigger value="steps">Steps</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="chat" className="flex min-h-0 flex-1 flex-col">
          {ready ? (
            <ThreadView />
          ) : c.status === "login" ? (
            <Login />
          ) : (
            <Setup />
          )}
        </TabsContent>
        <TabsContent value="steps" className="min-h-0 flex-1">
          <ScrollArea className="h-full">
            <AgentSteps />
          </ScrollArea>
        </TabsContent>
      </Tabs>
    </div>
  )
}
