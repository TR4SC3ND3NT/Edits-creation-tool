// The agent panel's state: the connection to an ACP adapter, the threads
// (sessions) and what each shows (messages, thoughts, tool calls, the plan,
// permission questions), the session's modes, models and options, and the
// presence steps the tool calls feed (`presence.ts`).

import * as React from "react"
import { listen } from "@tauri-apps/api/event"
import { isTauri } from "@tauri-apps/api/core"

import {
  agentApi,
  agentErrorText,
  isAgentError,
  type ActivityEntry,
  type Adapter,
  type AgentError,
  type AgentEvent,
  type AvailableCommand,
  type ConfigOption,
  type ContentBlock,
  type InitializeResponse,
  type Models,
  type Modes,
  type PermissionOption,
  type PlanEntry,
  type Presence,
  type SessionResponse,
  type ToolCall,
} from "@/agent/api"
import { presence, type Step } from "@/agent/presence"

export type Item =
  | { kind: "user"; id: string; blocks: ContentBlock[] }
  | { kind: "agent"; id: string; text: string; messageId?: string | null }
  | { kind: "thought"; id: string; text: string }
  | { kind: "tool"; id: string; call: ToolCall; presence: Presence | null }
  | {
      kind: "permission"
      id: string
      toolCall: ToolCall
      options: PermissionOption[]
      answer: string | null
      presence: Presence | null
    }
  | {
      kind: "notice"
      id: string
      severity: string
      title: string
      description?: string | null
    }
  | { kind: "error"; id: string; text: string }

export type Thread = {
  sessionId: string
  title: string
  items: Item[]
  plan: PlanEntry[]
  commands: AvailableCommand[]
  modes: Modes | null
  models: Models | null
  configOptions: ConfigOption[]
  usage: {
    used: number
    size: number
    cost?: { amount: number; currency: string } | null
  } | null
  busy: boolean
  /** What the agent is told about the studio; sent with the first message unless `told`. */
  note: string
  told: boolean
  mcp: boolean
  stopReason: string | null
}

export type Connection =
  | { status: "idle" }
  | { status: "connecting"; adapter: string }
  | {
      status: "ready"
      adapter: Adapter
      init: InitializeResponse
      generation: number
    }
  | {
      status: "login"
      adapter: Adapter
      init: InitializeResponse | null
      error: AgentError
    }
  | { status: "error"; adapter: Adapter | null; error: AgentError }

export type KnownThread = {
  sessionId: string
  title: string
  updatedAt: string | null
}

type AgentStore = {
  adapters: Adapter[]
  adapterId: string
  setAdapterId: (id: string) => void
  customCommand: string
  setCustomCommand: (c: string) => void
  connection: Connection
  connect: () => Promise<void>
  disconnect: () => Promise<void>
  agentName: string
  threads: Record<string, Thread>
  current: string | null
  thread: Thread | null
  known: KnownThread[]
  refreshKnown: () => Promise<void>
  newThread: () => Promise<void>
  openThread: (sessionId: string) => Promise<void>
  send: (blocks: ContentBlock[]) => Promise<void>
  cancel: () => Promise<void>
  answer: (key: string, option: string | null) => Promise<void>
  setMode: (mode: string) => Promise<void>
  setModel: (model: string) => Promise<void>
  setConfig: (id: string, value: string | boolean) => Promise<void>
  open: boolean
  setOpen: (open: boolean) => void
}

const AgentContext = React.createContext<AgentStore | null>(null)

export function useAgent() {
  const s = React.useContext(AgentContext)
  if (!s) throw new Error("useAgent outside AgentProvider")
  return s
}

function read(key: string, fallback: string) {
  try {
    return localStorage.getItem(key) ?? fallback
  } catch {
    return fallback
  }
}
function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    /* kept for this run only */
  }
}

let serial = 0
const uid = (p: string) => `${p}${++serial}`

function emptyThread(r: SessionResponse, title = "New thread"): Thread {
  return {
    sessionId: r.sessionId,
    title,
    items: [],
    plan: [],
    commands: [],
    modes: r.modes ?? null,
    models: r.models ?? null,
    configOptions: r.configOptions ?? [],
    usage: null,
    busy: false,
    note: r.studio?.note ?? "",
    told: r.studio?.told ?? true,
    mcp: r.studio?.mcp ?? false,
    stopReason: null,
  }
}

function textOf(blocks: ContentBlock[]) {
  return blocks
    .map((b) => (b.type === "text" ? b.text : ""))
    .join(" ")
    .trim()
}

/** The name for the agent of a log line: its MCP client, when it said. */
function actorOf(e: ActivityEntry): string {
  const c = (e as ActivityEntry & { client?: string }).client ?? ""
  if (/claude/i.test(c)) return "Claude"
  if (/codex/i.test(c)) return "Codex"
  if (/gemini/i.test(c)) return "Gemini"
  return "Agent"
}

function logStep(
  e: ActivityEntry
): Omit<Step, "started" | "ended" | "status" | "error"> {
  return {
    id: `log:${e.call}`,
    source: "log",
    actor: actorOf(e),
    tool: e.tool,
    label: e.label,
    layers: e.layers ?? [],
    pointers: e.pointers ?? [],
    t: e.t ?? null,
    frame: e.frame ?? null,
    job: e.job ?? null,
  }
}

/** Apply one ACP session update to a thread. */
export function applyUpdate(
  th: Thread,
  u: AgentEvent & { type: "update" }
): Thread {
  const up = u.update
  const items = th.items.slice()
  const last = items[items.length - 1]
  switch (up.sessionUpdate) {
    case "agent_message_chunk":
    case "agent_thought_chunk":
    case "user_message_chunk": {
      const c = (up as { content: ContentBlock }).content
      const mid = (up as { messageId?: string | null }).messageId ?? null
      if (up.sessionUpdate === "user_message_chunk") {
        if (last?.kind === "user" && !mid)
          items[items.length - 1] = { ...last, blocks: [...last.blocks, c] }
        else items.push({ kind: "user", id: uid("u"), blocks: [c] })
        return { ...th, items }
      }
      const text = c.type === "text" ? c.text : ""
      const kind =
        up.sessionUpdate === "agent_message_chunk" ? "agent" : "thought"
      if (
        last &&
        last.kind === kind &&
        (kind === "thought" ||
          !mid ||
          (last as { messageId?: string | null }).messageId === mid)
      ) {
        items[items.length - 1] = {
          ...last,
          text: (last as { text: string }).text + text,
        } as Item
      } else {
        items.push(
          kind === "agent"
            ? { kind, id: uid("a"), text, messageId: mid }
            : { kind, id: uid("t"), text }
        )
      }
      return { ...th, items }
    }
    case "tool_call":
    case "tool_call_update": {
      const call = up as unknown as ToolCall
      const i = items.findIndex(
        (x) => x.kind === "tool" && x.id === call.toolCallId
      )
      if (i >= 0) {
        const old = items[i] as Item & { kind: "tool" }
        const merged: ToolCall = { ...old.call }
        for (const [k, v] of Object.entries(call))
          if (v != null && k !== "sessionUpdate")
            (merged as Record<string, unknown>)[k] = v
        items[i] = {
          ...old,
          call: merged,
          presence: u.presence ?? old.presence,
        }
      } else {
        items.push({
          kind: "tool",
          id: call.toolCallId,
          call: { ...call },
          presence: u.presence,
        })
      }
      return { ...th, items }
    }
    case "plan":
      return { ...th, plan: (up as { entries: PlanEntry[] }).entries }
    case "available_commands_update":
      return {
        ...th,
        commands: (up as { availableCommands: AvailableCommand[] })
          .availableCommands,
      }
    case "current_mode_update":
      return th.modes
        ? {
            ...th,
            modes: {
              ...th.modes,
              currentModeId: (up as { currentModeId: string }).currentModeId,
            },
          }
        : th
    case "config_option_update":
      return {
        ...th,
        configOptions: (up as { configOptions: ConfigOption[] }).configOptions,
      }
    case "session_info_update": {
      const title = (up as { title?: string | null }).title
      return title ? { ...th, title } : th
    }
    case "usage_update": {
      const x = up as {
        used: number
        size: number
        cost?: { amount: number; currency: string } | null
      }
      return { ...th, usage: { used: x.used, size: x.size, cost: x.cost } }
    }
    case "notice": {
      const n = up as {
        severity: string
        title: string
        description?: string | null
      }
      items.push({
        kind: "notice",
        id: uid("n"),
        severity: n.severity,
        title: n.title,
        description: n.description,
      })
      return { ...th, items }
    }
    default:
      return th
  }
}

export function AgentProvider({ children }: { children: React.ReactNode }) {
  const [adapters, setAdapters] = React.useState<Adapter[]>([])
  const [adapterId, setAdapterIdState] = React.useState(() =>
    read("edits-agent-adapter", "claude")
  )
  const [customCommand, setCustomCommandState] = React.useState(() =>
    read("edits-agent-custom", "")
  )
  const [connection, setConnection] = React.useState<Connection>({
    status: "idle",
  })
  const [threads, setThreads] = React.useState<Record<string, Thread>>({})
  const [current, setCurrent] = React.useState<string | null>(null)
  const [known, setKnown] = React.useState<KnownThread[]>([])
  const [open, setOpenState] = React.useState(
    () => read("edits-agent-open", "1") === "1"
  )
  const generation = React.useRef(0)

  const setAdapterId = React.useCallback((id: string) => {
    setAdapterIdState(id)
    write("edits-agent-adapter", id)
  }, [])
  const setCustomCommand = React.useCallback((c: string) => {
    setCustomCommandState(c)
    write("edits-agent-custom", c)
  }, [])
  const setOpen = React.useCallback((o: boolean) => {
    setOpenState(o)
    write("edits-agent-open", o ? "1" : "0")
  }, [])

  React.useEffect(() => {
    if (!isTauri()) return
    agentApi
      .adapters(customCommand || null)
      .then(setAdapters)
      .catch(() => {})
  }, [customCommand])

  const adapter = adapters.find((a) => a.id === adapterId) ?? null
  const agentName =
    connection.status === "ready"
      ? connection.adapter.agent
      : (adapter?.agent ?? "Agent")
  const agentNameRef = React.useRef(agentName)
  React.useLayoutEffect(() => {
    agentNameRef.current = agentName
  })

  const patch = React.useCallback((id: string, f: (t: Thread) => Thread) => {
    setThreads((all) => (all[id] ? { ...all, [id]: f(all[id]) } : all))
  }, [])

  // ---- events from the Rust side -----------------------------------------------------------
  React.useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    // An effect cleaned up before `listen` resolved (StrictMode) must still unlisten.
    let gone = false
    listen<AgentEvent>("agent", (e) => {
      const ev = e.payload
      if (ev.type === "activity") {
        const mine = ev.entries.filter((x) => x.via !== "studio")
        if (ev.replay) {
          const steps: Step[] = []
          for (const x of mine) {
            if (x.phase === "start")
              steps.push({
                ...logStep(x),
                status: "done",
                started: 0,
                ended: 0,
                error: null,
              })
          }
          presence.history(steps)
          return
        }
        for (const x of mine) {
          if (x.phase === "start") presence.begin(logStep(x))
          else {
            presence.begin({ ...logStep(x), status: "running" })
            presence.end(`log:${x.call}`, x.ok !== false, {
              error: x.error,
              job: x.job,
            })
          }
        }
        return
      }
      if (ev.type === "permission_done") {
        setThreads((all) => {
          const out: Record<string, Thread> = {}
          for (const [k, t] of Object.entries(all))
            out[k] = {
              ...t,
              items: t.items.map((it) =>
                it.kind === "permission" && it.id === ev.key && !it.answer
                  ? { ...it, answer: "cancelled" }
                  : it
              ),
            }
          return out
        })
        return
      }
      if ("generation" in ev && ev.generation !== generation.current) return
      if (ev.type === "update") {
        patch(ev.sessionId, (t) => applyUpdate(t, ev))
        const up = ev.update as ToolCall & { sessionUpdate: string }
        if (
          up.sessionUpdate === "tool_call" ||
          up.sessionUpdate === "tool_call_update"
        ) {
          const p = ev.presence
          const running =
            up.status === "in_progress" ||
            up.status === "pending" ||
            up.status == null
          presence.begin({
            id: up.toolCallId,
            source: "panel",
            actor: agentNameRef.current,
            tool: p?.tool ?? null,
            label: p?.label ?? up.title ?? "",
            layers: p?.layers ?? [],
            pointers: p?.pointers ?? [],
            t: p?.t ?? null,
            frame: p?.frame ?? null,
            job: null,
            ...(running ? { status: "running" as const } : {}),
          })
          if (up.status === "completed" || up.status === "failed") {
            const out = up.rawOutput as { job?: string } | undefined
            presence.end(up.toolCallId, up.status === "completed", {
              job: out?.job ?? null,
            })
          }
        }
      } else if (ev.type === "permission") {
        patch(ev.sessionId, (t) => ({
          ...t,
          items: [
            ...t.items,
            {
              kind: "permission",
              id: ev.key,
              toolCall: ev.toolCall,
              options: ev.options,
              answer: null,
              presence: ev.presence,
            },
          ],
        }))
      } else if (ev.type === "exit") {
        setConnection((c) => ({
          status: "error",
          adapter: c.status === "ready" ? c.adapter : null,
          error: {
            message: `The agent stopped${ev.code != null ? ` (exit code ${ev.code})` : ""}.`,
            code: ev.code,
            auth_required: false,
            data: null,
            stderr: ev.stderr,
          },
        }))
        setThreads((all) => {
          const out: Record<string, Thread> = {}
          for (const [k, t] of Object.entries(all))
            out[k] = { ...t, busy: false }
          return out
        })
      }
    }).then((u) => (gone ? u() : (unlisten = u)))
    return () => {
      gone = true
      unlisten?.()
    }
  }, [patch])

  // ---- threads ------------------------------------------------------------------------------
  const projectKey = React.useRef("")
  const remember = React.useCallback((t: KnownThread) => {
    setKnown((k) => {
      const next = [t, ...k.filter((x) => x.sessionId !== t.sessionId)].slice(
        0,
        30
      )
      write(`edits-agent-threads:${projectKey.current}`, JSON.stringify(next))
      return next
    })
  }, [])

  const refreshKnown = React.useCallback(async () => {
    const setup = await agentApi.setup().catch(() => null)
    projectKey.current = setup?.projectDir ?? ""
    const local = ((): KnownThread[] => {
      try {
        return JSON.parse(
          read(`edits-agent-threads:${projectKey.current}`, "[]")
        )
      } catch {
        return []
      }
    })()
    const listed = await agentApi.sessionList().catch(() => null)
    const remote: KnownThread[] = (listed?.sessions ?? []).map((s) => ({
      sessionId: s.sessionId,
      title: s.title ?? s.sessionId.slice(0, 8),
      updatedAt: s.updatedAt ?? null,
    }))
    const seen = new Set<string>()
    setKnown(
      [...local, ...remote].filter((t) =>
        seen.has(t.sessionId) ? false : (seen.add(t.sessionId), true)
      )
    )
  }, [])

  const fail = React.useCallback(
    (e: unknown, a: Adapter | null, init: InitializeResponse | null) => {
      const err: AgentError = isAgentError(e)
        ? e
        : {
            message: agentErrorText(e),
            code: null,
            auth_required: false,
            data: null,
            stderr: [],
          }
      if (err.auth_required && a)
        setConnection({ status: "login", adapter: a, init, error: err })
      else setConnection({ status: "error", adapter: a, error: err })
    },
    []
  )

  const newThread = React.useCallback(async () => {
    if (connection.status !== "ready" && connection.status !== "login") return
    const a = connection.adapter
    const init = connection.init
    try {
      const r = await agentApi.sessionNew()
      if (connection.status === "login" && init)
        setConnection({
          status: "ready",
          adapter: a,
          init,
          generation: generation.current,
        })
      setThreads((all) => ({ ...all, [r.sessionId]: emptyThread(r) }))
      setCurrent(r.sessionId)
    } catch (e) {
      fail(e, a, init)
    }
  }, [connection, fail])

  const connect = React.useCallback(async () => {
    const a = adapters.find((x) => x.id === adapterId) ?? null
    setConnection({ status: "connecting", adapter: adapterId })
    setThreads({})
    setCurrent(null)
    try {
      const r = await agentApi.connect(
        adapterId,
        adapterId === "custom" ? customCommand : null
      )
      generation.current = r.generation
      setConnection({
        status: "ready",
        adapter: r.adapter,
        init: r.init,
        generation: r.generation,
      })
      try {
        const s = await agentApi.sessionNew()
        setThreads({ [s.sessionId]: emptyThread(s) })
        setCurrent(s.sessionId)
        void refreshKnown()
      } catch (e) {
        fail(e, r.adapter, r.init)
      }
    } catch (e) {
      fail(e, a, null)
    }
  }, [adapters, adapterId, customCommand, fail, refreshKnown])

  const disconnect = React.useCallback(async () => {
    generation.current = -1
    await agentApi.disconnect().catch(() => {})
    setConnection({ status: "idle" })
    setThreads({})
    setCurrent(null)
  }, [])

  const openThread = React.useCallback(
    async (sessionId: string) => {
      if (threads[sessionId]) {
        setCurrent(sessionId)
        return
      }
      const title =
        known.find((k) => k.sessionId === sessionId)?.title ?? "Thread"
      // Updates of the replayed thread arrive while the load runs: make room first.
      setThreads((all) => ({
        ...all,
        [sessionId]: emptyThread(
          { sessionId, studio: { note: "", told: true, mcp: true } },
          title
        ),
      }))
      setCurrent(sessionId)
      try {
        const r = await agentApi.sessionLoad(sessionId)
        patch(sessionId, (t) => ({
          ...t,
          modes: r.modes ?? t.modes,
          models: r.models ?? t.models,
          configOptions: r.configOptions ?? t.configOptions,
        }))
      } catch (e) {
        patch(sessionId, (t) => ({
          ...t,
          items: [
            ...t.items,
            {
              kind: "error",
              id: uid("e"),
              text: `Cannot reopen this thread: ${agentErrorText(e)}`,
            },
          ],
        }))
      }
    },
    [threads, known, patch]
  )

  const thread = current ? (threads[current] ?? null) : null

  const send = React.useCallback(
    async (blocks: ContentBlock[]) => {
      if (!thread || thread.busy || !blocks.length) return
      const id = thread.sessionId
      const first = thread.items.every((x) => x.kind !== "user")
      const prompt: ContentBlock[] =
        first && !thread.told && thread.note
          ? [
              {
                type: "text",
                text: `<studio-context>\n${thread.note}\n</studio-context>`,
              },
              ...blocks,
            ]
          : blocks
      patch(id, (t) => ({
        ...t,
        busy: true,
        stopReason: null,
        title:
          first && t.title === "New thread"
            ? textOf(blocks).slice(0, 48) || t.title
            : t.title,
        items: [...t.items, { kind: "user", id: uid("u"), blocks }],
      }))
      if (first)
        remember({
          sessionId: id,
          title: textOf(blocks).slice(0, 48) || "Thread",
          updatedAt: new Date().toISOString(),
        })
      try {
        const r = await agentApi.prompt(id, prompt)
        patch(id, (t) => ({ ...t, busy: false, stopReason: r.stopReason }))
      } catch (e) {
        if (isAgentError(e) && e.auth_required && connection.status === "ready")
          fail(e, connection.adapter, connection.init)
        patch(id, (t) => ({
          ...t,
          busy: false,
          items: [
            ...t.items,
            { kind: "error", id: uid("e"), text: agentErrorText(e) },
          ],
        }))
      }
    },
    [thread, patch, remember, connection, fail]
  )

  const cancel = React.useCallback(async () => {
    if (!thread) return
    await agentApi.cancel(thread.sessionId).catch(() => {})
  }, [thread])

  const answer = React.useCallback(
    async (key: string, option: string | null) => {
      setThreads((all) => {
        const out: Record<string, Thread> = {}
        for (const [k, t] of Object.entries(all))
          out[k] = {
            ...t,
            items: t.items.map((it) =>
              it.kind === "permission" && it.id === key
                ? { ...it, answer: option ?? "cancelled" }
                : it
            ),
          }
        return out
      })
      await agentApi.permission(key, option).catch(() => {})
    },
    []
  )

  const setMode = React.useCallback(
    async (mode: string) => {
      if (!thread) return
      const id = thread.sessionId
      patch(id, (t) =>
        t.modes ? { ...t, modes: { ...t.modes, currentModeId: mode } } : t
      )
      await agentApi.setMode(id, mode).catch(() => {})
    },
    [thread, patch]
  )

  const setModel = React.useCallback(
    async (model: string) => {
      if (!thread) return
      const id = thread.sessionId
      patch(id, (t) =>
        t.models ? { ...t, models: { ...t.models, currentModelId: model } } : t
      )
      await agentApi.setModel(id, model).catch(() => {})
    },
    [thread, patch]
  )

  const setConfig = React.useCallback(
    async (config: string, value: string | boolean) => {
      if (!thread) return
      const id = thread.sessionId
      patch(id, (t) => ({
        ...t,
        configOptions: t.configOptions.map((o) =>
          o.id === config ? ({ ...o, currentValue: value } as ConfigOption) : o
        ),
      }))
      try {
        const r = await agentApi.setConfig(id, config, value)
        if (r?.configOptions)
          patch(id, (t) => ({ ...t, configOptions: r.configOptions! }))
      } catch {
        /* the agent keeps its value; the next update shows it */
      }
    },
    [thread, patch]
  )

  const value: AgentStore = {
    adapters,
    adapterId,
    setAdapterId,
    customCommand,
    setCustomCommand,
    connection,
    connect,
    disconnect,
    agentName,
    threads,
    current,
    thread,
    known,
    refreshKnown,
    newThread,
    openThread,
    send,
    cancel,
    answer,
    setMode,
    setModel,
    setConfig,
    open,
    setOpen,
  }
  return <AgentContext.Provider value={value}>{children}</AgentContext.Provider>
}
