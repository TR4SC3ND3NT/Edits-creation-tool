// The agent panel's Tauri commands (`src-tauri/src/agent/commands.rs`) and
// the Agent Client Protocol (ACP) shapes the panel reads. ACP is described at
// https://agentclientprotocol.com; only the fields the panel uses are typed.

import { trackedInvoke } from "@/lib/tauri"

// ---- ACP ------------------------------------------------------------------------------------

export type ContentBlock =
  | { type: "text"; text: string }
  | { type: "image"; data: string; mimeType: string; uri?: string | null }
  | { type: "audio"; data: string; mimeType: string }
  | {
      type: "resource_link"
      uri: string
      name: string
      title?: string | null
      mimeType?: string | null
    }
  | {
      type: "resource"
      resource: {
        uri: string
        text?: string
        blob?: string
        mimeType?: string | null
      }
    }

export type ToolKind =
  | "read"
  | "edit"
  | "delete"
  | "move"
  | "search"
  | "execute"
  | "think"
  | "fetch"
  | "switch_mode"
  | "other"

export type ToolStatus = "pending" | "in_progress" | "completed" | "failed"

export type ToolContent =
  | { type: "content"; content: ContentBlock }
  | { type: "diff"; path: string; oldText?: string | null; newText: string }
  | { type: "terminal"; terminalId: string }

export type ToolCall = {
  toolCallId: string
  title?: string | null
  name?: string | null
  kind?: ToolKind | null
  status?: ToolStatus | null
  content?: ToolContent[] | null
  locations?: { path: string; line?: number | null }[] | null
  rawInput?: unknown
  rawOutput?: unknown
  _meta?: Record<string, unknown> | null
}

export type PlanEntry = {
  content: string
  priority: "high" | "medium" | "low"
  status: "pending" | "in_progress" | "completed"
}

export type AvailableCommand = {
  name: string
  description: string
  input?: { hint: string } | null
}

export type SelectOption = {
  value: string
  name: string
  description?: string | null
}

export type ConfigOption = {
  id: string
  name: string
  description?: string | null
  category?: string | null
} & (
  | {
      type: "select"
      currentValue: string
      options:
        | SelectOption[]
        | { group: string; name: string; options: SelectOption[] }[]
    }
  | { type: "boolean"; currentValue: boolean }
)

export type SessionMode = {
  id: string
  name: string
  description?: string | null
}
export type Modes = { currentModeId: string; availableModes: SessionMode[] }
export type Models = {
  currentModelId: string
  availableModels: {
    modelId: string
    name: string
    description?: string | null
  }[]
}

export type SessionUpdate =
  | ({
      sessionUpdate:
        "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk"
    } & {
      content: ContentBlock
      messageId?: string | null
    })
  | ({ sessionUpdate: "tool_call" } & ToolCall)
  | ({ sessionUpdate: "tool_call_update" } & ToolCall)
  | { sessionUpdate: "plan"; entries: PlanEntry[] }
  | {
      sessionUpdate: "available_commands_update"
      availableCommands: AvailableCommand[]
    }
  | { sessionUpdate: "current_mode_update"; currentModeId: string }
  | { sessionUpdate: "config_option_update"; configOptions: ConfigOption[] }
  | {
      sessionUpdate: "session_info_update"
      title?: string | null
      updatedAt?: string | null
    }
  | {
      sessionUpdate: "usage_update"
      used: number
      size: number
      cost?: { amount: number; currency: string } | null
    }
  | {
      sessionUpdate: "notice"
      severity: string
      title: string
      description?: string | null
    }
  | { sessionUpdate: string; [k: string]: unknown }

export type PermissionOption = {
  optionId: string
  name: string
  kind: "allow_once" | "allow_always" | "reject_once" | "reject_always"
}

export type AuthMethod = {
  id: string
  name: string
  description?: string | null
  type?: string
  args?: string[]
}

export type InitializeResponse = {
  protocolVersion: number
  agentInfo?: { name: string; title?: string | null; version: string } | null
  authMethods?: AuthMethod[]
  agentCapabilities?: {
    loadSession?: boolean
    promptCapabilities?: {
      image?: boolean
      audio?: boolean
      embeddedContext?: boolean
    }
    sessionCapabilities?: Record<string, unknown>
  }
}

export type SessionResponse = {
  sessionId: string
  modes?: Modes | null
  models?: Models | null
  configOptions?: ConfigOption[] | null
  studio: { note: string; told: boolean; mcp: boolean }
}

// ---- the studio's side ------------------------------------------------------------------------

/** Where an edits tool call points (`edits_core::activity::target`). */
export type Presence = {
  tool: string
  label: string
  layers: string[]
  pointers: string[]
  time: string | null
  t: number | null
  frame: number | null
  args?: unknown
}

/** One line of `.edits/activity.jsonl` (format `edits.activity/1`). */
export type ActivityEntry = {
  at: string
  phase: "start" | "end"
  call: string
  tool: string
  actor: string
  label: string
  project?: string
  layers?: string[]
  pointers?: string[]
  time?: string
  t?: number
  frame?: number
  ok?: boolean
  error?: string
  ms?: number
  job?: string
  via?: string
}

export type AgentEvent =
  | {
      type: "update"
      generation: number
      sessionId: string
      update: SessionUpdate
      presence: Presence | null
    }
  | {
      type: "permission"
      generation: number
      key: string
      sessionId: string
      toolCall: ToolCall
      options: PermissionOption[]
      presence: Presence | null
    }
  | { type: "permission_done"; key: string }
  | { type: "exit"; generation: number; code: number | null; stderr: string[] }
  | {
      type: "notification"
      generation: number
      method: string
      params: unknown
    }
  | { type: "activity"; entries: ActivityEntry[]; replay: boolean }

export type Adapter = {
  id: "claude" | "codex" | "custom"
  name: string
  agent: string
  command: string[]
  source: "env" | "path" | "npx" | "missing"
  package: string | null
  install: string | null
  login: string[]
}

export type AgentError = {
  message: string
  code: number | null
  auth_required: boolean
  data: unknown
  stderr: string[]
}

export function agentErrorText(e: unknown): string {
  if (e && typeof e === "object" && "message" in e)
    return String((e as { message: unknown }).message)
  return String(e)
}

export function isAgentError(e: unknown): e is AgentError {
  return !!e && typeof e === "object" && "auth_required" in e
}

export const agentApi = {
  adapters: (custom?: string | null) =>
    trackedInvoke<Adapter[]>("agent_adapters", { custom: custom ?? null }),
  setup: () =>
    trackedInvoke<{
      projectDir: string | null
      mcp: string | null
      skill: string | null
      running: boolean
    }>("agent_setup"),
  connect: (adapter: string, command?: string | null) =>
    trackedInvoke<{
      adapter: Adapter
      init: InitializeResponse
      generation: number
    }>("agent_connect", { adapter, command: command ?? null }),
  disconnect: () => trackedInvoke<void>("agent_disconnect"),
  sessionNew: () => trackedInvoke<SessionResponse>("agent_session_new"),
  sessionLoad: (session: string) =>
    trackedInvoke<SessionResponse>("agent_session_load", { session }),
  sessionList: () =>
    trackedInvoke<{
      sessions: {
        sessionId: string
        title?: string | null
        updatedAt?: string | null
      }[]
      supported: boolean
    }>("agent_session_list"),
  prompt: (session: string, prompt: ContentBlock[]) =>
    trackedInvoke<{ stopReason: string }>("agent_prompt", { session, prompt }),
  cancel: (session: string) => trackedInvoke<void>("agent_cancel", { session }),
  permission: (key: string, option: string | null) =>
    trackedInvoke<void>("agent_permission", { key, option }),
  setMode: (session: string, mode: string) =>
    trackedInvoke<unknown>("agent_set_mode", { session, mode }),
  setModel: (session: string, model: string) =>
    trackedInvoke<unknown>("agent_set_model", { session, model }),
  setConfig: (session: string, config: string, value: string | boolean) =>
    trackedInvoke<{ configOptions?: ConfigOption[] }>("agent_set_config", {
      session,
      config,
      value,
    }),
  authenticate: (method: string) =>
    trackedInvoke<unknown>("agent_authenticate", { method }),
  login: (method: string, open: boolean) =>
    trackedInvoke<{ command: string[]; terminal: string | null }>(
      "agent_login",
      {
        method,
        open,
      }
    ),
  files: () => trackedInvoke<string[]>("agent_files"),
}
