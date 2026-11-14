#!/usr/bin/env node
// A fake ACP agent for trying the studio's agent panel without a login, and
// for screenshots: it speaks the Agent Client Protocol on stdio like the
// Claude Code and Codex adapters do, and really edits the project through
// the `edits` MCP server the studio passes to its session.
//
//   In the panel: agent "Custom ACP command", command
//     node studio/scripts/fake-acp-agent.mjs
//   FAKE_ACP_LOGIN=1   answer session/new with "log in first" (ACP -32000)
//   FAKE_ACP_DELAY=ms  pause between steps (default 900)
//
// As an agent in a terminal (no ACP; an MCP client of edits-mcp, introducing
// itself as claude-code), for the studio's activity log:
//   node studio/scripts/fake-acp-agent.mjs --external <project folder> [edits-mcp]
//
// No dependencies; Node 18 or newer.

import { spawn } from "node:child_process"
import { createInterface } from "node:readline"

const DELAY = Number(process.env.FAKE_ACP_DELAY ?? 900)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

// ---- an MCP client of edits-mcp ----------------------------------------------------------------

function mcpClient(command, args, env, cwd, clientName) {
  const child = spawn(command, args, {
    cwd,
    env: { ...process.env, ...env },
    stdio: ["pipe", "pipe", "inherit"],
  })
  let next = 1
  const waiting = new Map()
  createInterface({ input: child.stdout }).on("line", (line) => {
    let m
    try {
      m = JSON.parse(line)
    } catch {
      return
    }
    if (m.id != null && waiting.has(m.id)) {
      waiting.get(m.id)(m)
      waiting.delete(m.id)
    }
  })
  const request = (method, params) =>
    new Promise((resolve) => {
      const id = next++
      waiting.set(id, resolve)
      child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n")
    })
  const ready = request("initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: clientName, version: "0.0.0" },
  }).then(() => child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n"))
  return {
    async call(name, args) {
      await ready
      const r = await request("tools/call", { name, arguments: args })
      return r.result ?? { isError: true, content: [{ type: "text", text: r.error?.message ?? "error" }] }
    },
    close() {
      child.stdin.end()
    },
  }
}

// ---- external mode: an agent in a terminal ----------------------------------------------------------------

if (process.argv[2] === "--external") {
  const dir = process.argv[3] ?? "."
  const bin = process.argv[4] ?? process.env.EDITS_MCP_BIN ?? "edits-mcp"
  const mcp = mcpClient(bin, ["--root", dir], {}, dir, "claude-code")
  const p = "project.json"
  const steps = [
    ["project_describe", { project: p }],
    ["project_edit", { project: p, ops: [{ op: "set", pointer: "/layers/@cube/effects/@glow/params/intensity", value: 1.2 }] }],
    ["preset_apply", { project: p, preset: "shake", layer: "cube", at: "b2" }],
    ["render_frame", { project: p, frame: 70 }],
  ]
  for (const [tool, args] of steps) {
    const r = await mcp.call(tool, args)
    console.error(`${tool}: ${r.isError ? "failed" : "ok"}`)
    await sleep(DELAY * 2)
  }
  mcp.close()
  process.exit(0)
}

// ---- the ACP agent ---------------------------------------------------------------------------------------------------

const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...m }) + "\n")
const update = (sessionId, u) => send({ method: "session/update", params: { sessionId, update: u } })
const answers = new Map()
let asked = 0
const ask = (method, params) =>
  new Promise((resolve) => {
    const id = `ask-${++asked}`
    answers.set(id, resolve)
    send({ id, method, params })
  })

const sessions = new Map()
const MODES = [
  { id: "default", name: "Default", description: "Ask before edits" },
  { id: "acceptEdits", name: "Accept Edits", description: "Edit without asking" },
  { id: "plan", name: "Plan Mode", description: "Plan, do not edit" },
  { id: "bypassPermissions", name: "Bypass Permissions", description: "Never ask" },
]
const options = (s) => [
  {
    id: "model",
    name: "Model",
    category: "model",
    type: "select",
    currentValue: s.model,
    options: [
      { value: "default", name: "Default (recommended)" },
      { value: "large", name: "Large" },
      { value: "small", name: "Small, fast" },
    ],
  },
  {
    id: "effort",
    name: "Effort",
    category: "thought_level",
    type: "select",
    currentValue: s.effort,
    options: ["low", "medium", "high", "max"].map((v) => ({ value: v, name: v[0].toUpperCase() + v.slice(1) })),
  },
  { id: "fast", name: "Fast mode", category: "model_config", type: "boolean", currentValue: s.fast },
]

async function chunks(sessionId, kind, text) {
  for (const piece of text.match(/.{1,24}(\s|$)|.+/gs) ?? []) {
    update(sessionId, { sessionUpdate: kind, content: { type: "text", text: piece } })
    await sleep(40)
  }
}

async function tool(s, id, name, title, kind, args, run, needsPermission = false) {
  update(s.id, {
    sessionUpdate: "tool_call",
    toolCallId: id,
    title,
    kind,
    status: "pending",
    rawInput: args,
    _meta: { claudeCode: { toolName: name } },
  })
  if (needsPermission && s.mode === "default") {
    const r = await ask("session/request_permission", {
      sessionId: s.id,
      toolCall: { toolCallId: id, title, kind, rawInput: args },
      options: [
        { optionId: "allow", name: "Allow", kind: "allow_once" },
        { optionId: "always", name: "Always allow", kind: "allow_always" },
        { optionId: "reject", name: "Reject", kind: "reject_once" },
      ],
    })
    const o = r.result?.outcome
    if (o?.outcome !== "selected" || o.optionId === "reject") {
      update(s.id, { sessionUpdate: "tool_call_update", toolCallId: id, status: "failed", content: [{ type: "content", content: { type: "text", text: "Rejected." } }] })
      return null
    }
    if (o.optionId === "always") s.mode = "acceptEdits"
  }
  update(s.id, { sessionUpdate: "tool_call_update", toolCallId: id, status: "in_progress" })
  await sleep(DELAY)
  const result = await run()
  const content = (result?.content ?? []).map((c) =>
    c.type === "image"
      ? { type: "content", content: { type: "image", data: c.data, mimeType: c.mimeType } }
      : { type: "content", content: { type: "text", text: String(c.text ?? "").slice(0, 1500) } }
  )
  update(s.id, {
    sessionUpdate: "tool_call_update",
    toolCallId: id,
    status: result?.isError ? "failed" : "completed",
    content,
  })
  return result
}

async function turn(s, prompt) {
  // Tool call ids are unique across turns, as a real agent's are.
  const n = `t${(s.turns = (s.turns ?? 0) + 1)}`
  const text = prompt.filter((b) => b.type === "text").map((b) => b.text).join(" ")
  const p = s.project
  const plan = (n) =>
    update(s.id, {
      sessionUpdate: "plan",
      entries: [
        { content: "Read the project", priority: "high", status: n > 0 ? "completed" : "in_progress" },
        { content: "Add a glow and a punch on layer d", priority: "high", status: n > 1 ? "completed" : n === 1 ? "in_progress" : "pending" },
        { content: "Look at frame 60", priority: "medium", status: n > 2 ? "completed" : n === 2 ? "in_progress" : "pending" },
      ],
    })
  await chunks(s.id, "agent_thought_chunk", `The person asks: "${text.slice(0, 80)}". I should read the project first, then change layer d around the drop and look at a frame to check it.`)
  await chunks(s.id, "agent_message_chunk", "I'll read the project, then give layer **d** a glow and a punch at beat 6, and check frame 60.\n\n")
  plan(0)
  await sleep(DELAY)
  await tool(s, `${n}-call-1`, "mcp__edits__project_describe", "mcp__edits__project_describe", "read", { project: p }, () => s.mcp.call("project_describe", { project: p }))
  if (s.cancelled) return "cancelled"
  plan(1)
  const edit = {
    project: p,
    ops: [
      { op: "set", pointer: "/layers/@d/effects/-", value: { kind: "glow", params: { intensity: 0.7, radius: 3, threshold: 0.65 } } },
      { op: "set", pointer: "/layers/@d/transform/position", value: [0.06, 0, 0] },
    ],
  }
  await tool(s, `${n}-call-2`, "mcp__edits__project_edit", "mcp__edits__project_edit", "edit", edit, () => s.mcp.call("project_edit", edit), true)
  if (s.cancelled) return "cancelled"
  await sleep(DELAY)
  const preset = { project: p, preset: "punch", layer: "d", at: "b6" }
  await tool(s, `${n}-call-3`, "mcp__edits__preset_apply", "mcp__edits__preset_apply", "edit", preset, () => s.mcp.call("preset_apply", preset), true)
  if (s.cancelled) return "cancelled"
  plan(2)
  await sleep(DELAY)
  const frame = { project: p, frame: 60 }
  await tool(s, `${n}-call-4`, "mcp__edits__render_frame", "mcp__edits__render_frame", "read", frame, () => s.mcp.call("render_frame", frame))
  if (s.cancelled) return "cancelled"
  plan(3)
  await chunks(
    s.id,
    "agent_message_chunk",
    "Done. Layer `d` now has:\n\n- a **glow** (intensity 0.7, radius 3)\n- a small shift right (`position` → `[0.06, 0, 0]`)\n- a **punch** at beat 6\n\n```json\n{ \"op\": \"set\", \"pointer\": \"/layers/@d/effects/-\", \"value\": { \"kind\": \"glow\" } }\n```\n\nFrame 60 looks brighter on the striped sun; say if the glow is too strong."
  )
  update(s.id, { sessionUpdate: "usage_update", used: 18342, size: 200000, cost: { amount: 0.04, currency: "USD" } })
  return "end_turn"
}

createInterface({ input: process.stdin }).on("line", async (line) => {
  let m
  try {
    m = JSON.parse(line)
  } catch {
    return
  }
  if (m.id != null && !m.method) {
    answers.get(m.id)?.(m)
    answers.delete(m.id)
    return
  }
  const { id, method, params = {} } = m
  const reply = (result) => send({ id, result })
  switch (method) {
    case "initialize":
      reply({
        protocolVersion: 1,
        agentInfo: { name: "fake-acp-agent", title: "Fake agent", version: "0.1.0" },
        agentCapabilities: {
          loadSession: false,
          promptCapabilities: { image: true, embeddedContext: true },
          sessionCapabilities: {},
        },
        authMethods: process.env.FAKE_ACP_LOGIN
          ? [{ id: "fake-login", name: "Log in with the fake agent", description: "Opens a terminal", type: "terminal", args: ["--login"] }]
          : [],
      })
      break
    case "session/new": {
      if (process.env.FAKE_ACP_LOGIN) {
        send({ id, error: { code: -32000, message: "Authentication required: log in to the fake agent first." } })
        break
      }
      const sid = `fake-${Date.now().toString(36)}`
      const server = (params.mcpServers ?? []).find((x) => x.name === "edits")
      const env = Object.fromEntries((server?.env ?? []).map((e) => [e.name, e.value]))
      const s = {
        id: sid,
        project: process.env.FAKE_ACP_PROJECT ?? "project.json",
        mode: "default",
        model: "default",
        effort: "high",
        fast: false,
        cancelled: false,
        mcp: server ? mcpClient(server.command, server.args, env, params.cwd, "fake-acp-agent") : { call: async () => ({ isError: true, content: [{ type: "text", text: "no edits MCP server" }] }) },
      }
      sessions.set(sid, s)
      reply({ sessionId: sid, modes: { currentModeId: s.mode, availableModes: MODES }, configOptions: options(s) })
      update(sid, {
        sessionUpdate: "available_commands_update",
        availableCommands: [
          { name: "review", description: "Review the edit against the song" },
          { name: "compact", description: "Summarise the thread to free context" },
          { name: "init", description: "Write notes for this project" },
          { name: "cost", description: "Show the tokens used" },
        ],
      })
      break
    }
    case "session/set_mode": {
      const s = sessions.get(params.sessionId)
      if (s) s.mode = params.modeId
      reply({})
      update(params.sessionId, { sessionUpdate: "current_mode_update", currentModeId: params.modeId })
      break
    }
    case "session/set_config_option": {
      const s = sessions.get(params.sessionId)
      if (s) s[params.configId] = params.value
      reply({ configOptions: s ? options(s) : [] })
      break
    }
    case "session/prompt": {
      const s = sessions.get(params.sessionId)
      if (!s) return send({ id, error: { code: -32602, message: "no such session" } })
      s.cancelled = false
      const stop = await turn(s, params.prompt ?? [])
      reply({ stopReason: s.cancelled ? "cancelled" : stop })
      break
    }
    case "session/cancel": {
      const s = sessions.get(params.sessionId)
      if (s) s.cancelled = true
      break
    }
    default:
      if (id != null) send({ id, error: { code: -32601, message: `${method} is not supported by the fake agent` } })
  }
})
