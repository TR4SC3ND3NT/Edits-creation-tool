// The message box: `@` to include context (layers, effects, keyframes, the
// current frame, the selection, files of the project folder), `/` for the
// agent's commands, images by paste or drop or the current frame, the
// session's mode, model and options, send and stop.

import * as React from "react"
import {
  ArrowUpIcon,
  AtSignIcon,
  BrainIcon,
  CameraIcon,
  CpuIcon,
  FileIcon,
  ImageIcon,
  KeyRoundIcon,
  LayersIcon,
  MousePointerClickIcon,
  ShieldIcon,
  SlashIcon,
  SparklesIcon,
  SquareIcon,
  XIcon,
  ZapIcon,
} from "lucide-react"

import {
  agentApi,
  type ConfigOption,
  type ContentBlock,
  type SelectOption,
} from "@/agent/api"
import { useAgent } from "@/agent/store"
import { Badge } from "@/components/ui/badge"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupTextarea,
} from "@/components/ui/input-group"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { cachedFrame, loadFrame } from "@/lib/frames"
import { useStudio } from "@/lib/store"
import { fmtTime } from "@/lib/time"

/** Something `@` put into the message. */
type Mention = {
  key: string
  label: string
  group: string
  icon: React.ComponentType<{ className?: string }>
  /** Builds the blocks sent with the message. */
  blocks: () => Promise<ContentBlock[]>
}

type Attachment = { key: string; name: string; data: string; mimeType: string }

const toBase64 = (dataUrl: string) => dataUrl.slice(dataUrl.indexOf(",") + 1)

async function bitmapPng(b: ImageBitmap): Promise<string> {
  const c = document.createElement("canvas")
  c.width = b.width
  c.height = b.height
  c.getContext("2d")?.drawImage(b, 0, 0)
  return toBase64(c.toDataURL("image/png"))
}

function useCurrentFramePng() {
  const s = useStudio()
  return React.useCallback(async () => {
    if (!s.snapshot || !s.info) return null
    const req = {
      revision: s.snapshot.revision,
      frame: s.frame,
      scale: s.scale,
      blur: false,
    }
    const b =
      cachedFrame(req) ?? (await loadFrame(req, false).catch(() => null))
    if (!b) return null
    return { data: await bitmapPng(b), frame: s.frame, t: s.t }
  }, [s.snapshot, s.info, s.frame, s.scale, s.t])
}

function optionsOf(
  o: ConfigOption & { type: "select" }
): { group: string | null; options: SelectOption[] }[] {
  const opts = o.options as unknown[]
  if (
    opts.length &&
    typeof opts[0] === "object" &&
    opts[0] &&
    "group" in (opts[0] as object)
  )
    return (opts as { name: string; options: SelectOption[] }[]).map((g) => ({
      group: g.name,
      options: g.options,
    }))
  return [{ group: null, options: opts as SelectOption[] }]
}

const CATEGORY_ICON: Record<
  string,
  React.ComponentType<{ className?: string }>
> = {
  mode: ShieldIcon,
  model: CpuIcon,
  thought_level: BrainIcon,
  model_config: ZapIcon,
}

/** One session option as a select (or a switch for a boolean). */
function OptionControl({
  o,
  onChange,
}: {
  o: ConfigOption
  onChange: (v: string | boolean) => void
}) {
  const Icon = CATEGORY_ICON[o.category ?? ""] ?? SparklesIcon
  if (o.type === "boolean")
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <label className="flex h-7 items-center gap-1.5 rounded-md px-1.5 text-xs text-muted-foreground" />
          }
        >
          <Icon className="size-3.5" />
          {o.name}
          <Switch
            size="sm"
            checked={o.currentValue}
            onCheckedChange={(v) => onChange(!!v)}
          />
        </TooltipTrigger>
        <TooltipContent className="max-w-64">
          {o.description ?? o.name}
        </TooltipContent>
      </Tooltip>
    )
  const groups = optionsOf(o)
  const all = groups.flatMap((g) => g.options)
  return (
    <Select
      items={all.map((x) => ({ label: x.name, value: x.value }))}
      value={o.currentValue}
      onValueChange={(v) => v != null && onChange(String(v))}
    >
      <SelectTrigger
        size="sm"
        className="h-7 max-w-40 gap-1 border-none px-1.5 text-xs shadow-none"
        aria-label={o.name}
      >
        <Icon className="size-3.5 text-muted-foreground" />
        <SelectValue />
      </SelectTrigger>
      <SelectContent className="max-w-80">
        {groups.map((g, i) => (
          <SelectGroup key={i}>
            <SelectLabel>{g.group ?? o.name}</SelectLabel>
            {g.options.map((x) => (
              <SelectItem key={x.value} value={x.value}>
                <span className="flex flex-col">
                  <span>{x.name}</span>
                  {x.description && (
                    <span className="text-xs text-muted-foreground">
                      {x.description}
                    </span>
                  )}
                </span>
              </SelectItem>
            ))}
          </SelectGroup>
        ))}
      </SelectContent>
    </Select>
  )
}

export function Composer() {
  const a = useAgent()
  const s = useStudio()
  const th = a.thread
  const ready = a.connection.status === "ready" && !!th
  const [text, setText] = React.useState("")
  const [mentions, setMentions] = React.useState<Mention[]>([])
  const [images, setImages] = React.useState<Attachment[]>([])
  const [menu, setMenu] = React.useState<{
    kind: "@" | "/"
    query: string
    start: number
  } | null>(null)
  const [active, setActive] = React.useState(0)
  const [files, setFiles] = React.useState<string[]>([])
  const box = React.useRef<HTMLTextAreaElement>(null)
  const framePng = useCurrentFramePng()
  const caps =
    a.connection.status === "ready"
      ? a.connection.init.agentCapabilities?.promptCapabilities
      : undefined
  const canImage = caps?.image !== false
  const embedded = !!caps?.embeddedContext

  React.useEffect(() => {
    if (menu?.kind === "@")
      agentApi
        .files()
        .then(setFiles)
        .catch(() => {})
  }, [menu?.kind])

  // ---- what @ offers -------------------------------------------------------------------------------
  const resource = React.useCallback(
    (uri: string, value: unknown, label: string): ContentBlock => {
      const text = JSON.stringify(value, null, 2)
      return embedded
        ? {
            type: "resource",
            resource: { uri, mimeType: "application/json", text },
          }
        : { type: "text", text: `@${label}:\n\`\`\`json\n${text}\n\`\`\`` }
    },
    [embedded]
  )

  const candidates: Mention[] = React.useMemo(() => {
    const out: Mention[] = []
    const snap = s.snapshot
    if (!snap) return out
    const file = snap.file_name
    const doc = (snap.doc ?? {}) as { layers?: Record<string, unknown>[] }
    out.push({
      key: "frame",
      label: `frame ${s.frame}`,
      group: "Viewer",
      icon: CameraIcon,
      blocks: async () => {
        const png = await framePng()
        const head: ContentBlock = {
          type: "text",
          text: `The current frame: frame ${s.frame} (${fmtTime(s.info, s.t)}) of ${file}.`,
        }
        return png && canImage
          ? [head, { type: "image", data: png.data, mimeType: "image/png" }]
          : [head]
      },
    })
    out.push({
      key: "selection",
      label: "selection",
      group: "Viewer",
      icon: MousePointerClickIcon,
      blocks: async () => [
        {
          type: "text",
          text: `What I have selected in the studio: ${s.layer ? `layer ${s.layer}` : s.camera ? "the camera" : "nothing"}${s.picked ? `, property ${s.picked.pointer}` : ""}; playhead at frame ${s.frame} (${fmtTime(s.info, s.t)}).`,
        },
      ],
    })
    for (const l of snap.layers) {
      const json = doc.layers?.[l.index]
      out.push({
        key: `layer:${l.id}`,
        label: l.id,
        group: "Layers",
        icon: LayersIcon,
        blocks: async () => [
          resource(`edits://${file}#/layers/@${l.id}`, json, l.id),
        ],
      })
      const effects =
        (json?.effects as Record<string, unknown>[] | undefined) ?? []
      effects.forEach((e, i) =>
        out.push({
          key: `effect:${l.id}:${i}`,
          label: `${l.id}/${String(e.kind)}`,
          group: "Effects",
          icon: SparklesIcon,
          blocks: async () => [
            resource(
              `edits://${file}#/layers/@${l.id}/effects/${i}`,
              e,
              `${l.id}/${String(e.kind)}`
            ),
          ],
        })
      )
      const tr = (json?.transform as Record<string, unknown> | undefined) ?? {}
      for (const [p, v] of Object.entries(tr))
        if (
          v &&
          typeof v === "object" &&
          Array.isArray((v as { keys?: unknown }).keys)
        )
          out.push({
            key: `keys:${l.id}:${p}`,
            label: `${l.id}.${p} keys`,
            group: "Keyframes",
            icon: KeyRoundIcon,
            blocks: async () => [
              resource(
                `edits://${file}#/layers/@${l.id}/transform/${p}`,
                v,
                `${l.id}.${p}`
              ),
            ],
          })
    }
    for (const f of files)
      out.push({
        key: `file:${f}`,
        label: f,
        group: "Files",
        icon: f.match(/\.(png|jpe?g|webp|gif)$/i) ? ImageIcon : FileIcon,
        blocks: async () => [
          {
            type: "resource_link",
            uri: `file://${snap.dir}/${f}`,
            name: f,
          },
        ],
      })
    return out
  }, [
    s.snapshot,
    s.frame,
    s.t,
    s.info,
    s.layer,
    s.camera,
    s.picked,
    files,
    framePng,
    resource,
    canImage,
  ])

  const shown = React.useMemo(() => {
    if (!menu) return []
    const q = menu.query.toLowerCase()
    if (menu.kind === "/")
      return (th?.commands ?? [])
        .filter((c) => c.name.toLowerCase().includes(q))
        .slice(0, 50)
        .map((c) => ({
          key: `/${c.name}`,
          label: `/${c.name}`,
          hint: c.description,
          group: "Commands",
        }))
    return candidates
      .filter((c) => c.label.toLowerCase().includes(q))
      .slice(0, 60)
      .map((c) => ({
        key: c.key,
        label: c.label,
        hint: c.group,
        group: c.group,
      }))
  }, [menu, candidates, th?.commands])

  const updateMenu = (value: string, caret: number) => {
    const before = value.slice(0, caret)
    const at = before.match(/(^|\s)@([^\s@]*)$/)
    if (at) {
      setMenu({ kind: "@", query: at[2], start: caret - at[2].length - 1 })
      setActive(0)
      return
    }
    const slash = before.match(/^\/(\S*)$/)
    if (slash && (th?.commands.length ?? 0) > 0) {
      setMenu({ kind: "/", query: slash[1], start: 0 })
      setActive(0)
      return
    }
    setMenu(null)
  }

  const pick = (key: string) => {
    if (!menu) return
    const el = box.current
    const caret = el?.selectionStart ?? text.length
    if (menu.kind === "/") {
      const next = `${key} ` + text.slice(caret)
      setText(next)
    } else {
      const m = candidates.find((c) => c.key === key)
      if (!m) return
      const next =
        text.slice(0, menu.start) + `@${m.label} ` + text.slice(caret)
      setText(next)
      setMentions((ms) => (ms.some((x) => x.key === m.key) ? ms : [...ms, m]))
    }
    setMenu(null)
    requestAnimationFrame(() => el?.focus())
  }

  const addImageFiles = (list: FileList | File[]) => {
    for (const f of Array.from(list)) {
      if (!f.type.startsWith("image/")) continue
      const r = new FileReader()
      r.onload = () =>
        setImages((im) => [
          ...im,
          {
            key: `${f.name}-${im.length}-${Date.now()}`,
            name: f.name || "image",
            data: toBase64(String(r.result)),
            mimeType: f.type,
          },
        ])
      r.readAsDataURL(f)
    }
  }

  const attachFrame = async () => {
    const png = await framePng()
    if (png)
      setImages((im) => [
        ...im,
        {
          key: `frame-${png.frame}-${Date.now()}`,
          name: `frame ${png.frame}`,
          data: png.data,
          mimeType: "image/png",
        },
      ])
  }

  const submit = async () => {
    if (!ready || th!.busy) return
    const body = text.trim()
    if (!body && !images.length && !mentions.length) return
    const blocks: ContentBlock[] = []
    if (body) blocks.push({ type: "text", text: body })
    for (const m of mentions)
      if (body.includes(`@${m.label}`)) blocks.push(...(await m.blocks()))
    for (const im of images)
      blocks.push({ type: "image", data: im.data, mimeType: im.mimeType })
    setText("")
    setMentions([])
    setImages([])
    setMenu(null)
    void a.send(blocks)
  }

  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (menu && shown.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault()
        setActive(
          (i) =>
            (i + (e.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length
        )
        return
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault()
        pick(shown[Math.min(active, shown.length - 1)].key)
        return
      }
    }
    if (e.key === "Escape") {
      if (menu) setMenu(null)
      else if (th?.busy) void a.cancel()
      return
    }
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      void submit()
    }
  }

  const modeOption = th?.configOptions.find((o) => o.category === "mode")
  const options = th?.configOptions ?? []
  const placeholder = !ready
    ? "Connect an agent to start"
    : `Message ${a.agentName} — @ to include context, / for commands`

  return (
    <div className="relative flex flex-col gap-1.5">
      {menu && shown.length > 0 && (
        <div className="absolute right-0 bottom-full left-0 z-30 mb-1.5 overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-md">
          <Command
            shouldFilter={false}
            value={shown[Math.min(active, shown.length - 1)]?.key}
            className="max-h-72"
          >
            <CommandList>
              <CommandEmpty>Nothing matches</CommandEmpty>
              {[...new Set(shown.map((x) => x.group))].map((g) => (
                <CommandGroup key={g} heading={g}>
                  {shown
                    .filter((x) => x.group === g)
                    .map((x) => {
                      const Icon =
                        menu.kind === "/"
                          ? SlashIcon
                          : (candidates.find((c) => c.key === x.key)?.icon ??
                            AtSignIcon)
                      return (
                        <CommandItem
                          key={x.key}
                          value={x.key}
                          onMouseDown={(e) => e.preventDefault()}
                          onSelect={() => pick(x.key)}
                        >
                          <Icon />
                          <span className="truncate">
                            {menu.kind === "/" ? x.label : x.label}
                          </span>
                          {menu.kind === "/" && (
                            <span className="ml-auto truncate text-xs text-muted-foreground">
                              {x.hint}
                            </span>
                          )}
                        </CommandItem>
                      )
                    })}
                </CommandGroup>
              ))}
            </CommandList>
          </Command>
        </div>
      )}
      <InputGroup
        className="flex-col"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          if (e.dataTransfer.files.length) {
            e.preventDefault()
            addImageFiles(e.dataTransfer.files)
          }
        }}
      >
        {(mentions.length > 0 || images.length > 0) && (
          <InputGroupAddon align="block-start" className="flex-wrap gap-1">
            {mentions.map((m) => (
              <Badge
                key={m.key}
                variant="secondary"
                className="gap-1 font-normal"
              >
                <m.icon />@{m.label}
                <button
                  type="button"
                  aria-label="Remove"
                  onClick={() =>
                    setMentions((ms) => ms.filter((x) => x.key !== m.key))
                  }
                >
                  <XIcon className="size-3" />
                </button>
              </Badge>
            ))}
            {images.map((im) => (
              <Badge
                key={im.key}
                variant="secondary"
                className="h-6 gap-1 pl-0.5 font-normal"
              >
                <img
                  src={`data:${im.mimeType};base64,${im.data}`}
                  alt=""
                  className="h-5 w-7 rounded-sm object-cover"
                />
                {im.name}
                <button
                  type="button"
                  aria-label="Remove"
                  onClick={() =>
                    setImages((x) => x.filter((y) => y.key !== im.key))
                  }
                >
                  <XIcon className="size-3" />
                </button>
              </Badge>
            ))}
          </InputGroupAddon>
        )}
        <InputGroupTextarea
          ref={box}
          value={text}
          disabled={!ready}
          placeholder={placeholder}
          className="max-h-48 min-h-16 text-sm"
          onChange={(e) => {
            setText(e.target.value)
            updateMenu(
              e.target.value,
              e.target.selectionStart ?? e.target.value.length
            )
          }}
          onKeyDown={onKey}
          onPaste={(e) => {
            if (e.clipboardData.files.length) {
              e.preventDefault()
              addImageFiles(e.clipboardData.files)
            }
          }}
        />
        <InputGroupAddon align="block-end" className="gap-0.5">
          <Tooltip>
            <TooltipTrigger
              render={
                <InputGroupButton
                  size="icon-xs"
                  disabled={!ready}
                  onClick={() => {
                    const el = box.current
                    const next =
                      text + (text && !text.endsWith(" ") ? " @" : "@")
                    setText(next)
                    updateMenu(next, next.length)
                    el?.focus()
                  }}
                />
              }
            >
              <AtSignIcon />
            </TooltipTrigger>
            <TooltipContent>
              Include context: a layer, an effect, keys, the frame, a file
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <InputGroupButton
                  size="icon-xs"
                  disabled={!ready || !canImage || !s.snapshot}
                  onClick={() => void attachFrame()}
                />
              }
            >
              <CameraIcon />
            </TooltipTrigger>
            <TooltipContent>
              Attach the current frame as an image (or paste or drop one)
            </TooltipContent>
          </Tooltip>
          {th?.modes && !modeOption && (
            <Select
              items={th.modes.availableModes.map((m) => ({
                label: m.name,
                value: m.id,
              }))}
              value={th.modes.currentModeId}
              onValueChange={(v) => v && void a.setMode(String(v))}
            >
              <SelectTrigger
                size="sm"
                className="h-7 gap-1 border-none px-1.5 text-xs shadow-none"
                aria-label="Permission mode"
              >
                <ShieldIcon className="size-3.5 text-muted-foreground" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectLabel>Permission mode</SelectLabel>
                  {th.modes.availableModes.map((m) => (
                    <SelectItem key={m.id} value={m.id}>
                      <span className="flex flex-col">
                        <span>{m.name}</span>
                        {m.description && (
                          <span className="text-xs text-muted-foreground">
                            {m.description}
                          </span>
                        )}
                      </span>
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          )}
          {th?.models && !options.some((o) => o.category === "model") && (
            <Select
              items={th.models.availableModels.map((m) => ({
                label: m.name,
                value: m.modelId,
              }))}
              value={th.models.currentModelId}
              onValueChange={(v) => v && void a.setModel(String(v))}
            >
              <SelectTrigger
                size="sm"
                className="h-7 gap-1 border-none px-1.5 text-xs shadow-none"
                aria-label="Model"
              >
                <CpuIcon className="size-3.5 text-muted-foreground" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectLabel>Model</SelectLabel>
                  {th.models.availableModels.map((m) => (
                    <SelectItem key={m.modelId} value={m.modelId}>
                      {m.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          )}
          {options.map((o) => (
            <OptionControl
              key={o.id}
              o={o}
              onChange={(v) => void a.setConfig(o.id, v)}
            />
          ))}
          <div className="ml-auto" />
          {th?.busy ? (
            <Tooltip>
              <TooltipTrigger
                render={
                  <InputGroupButton
                    size="icon-xs"
                    variant="default"
                    onClick={() => void a.cancel()}
                  />
                }
              >
                <SquareIcon className="fill-current" />
              </TooltipTrigger>
              <TooltipContent>Stop (Esc)</TooltipContent>
            </Tooltip>
          ) : (
            <Tooltip>
              <TooltipTrigger
                render={
                  <InputGroupButton
                    size="icon-xs"
                    variant="default"
                    disabled={!ready || (!text.trim() && !images.length)}
                    onClick={() => void submit()}
                  />
                }
              >
                <ArrowUpIcon />
              </TooltipTrigger>
              <TooltipContent>
                Send (Enter; Shift+Enter for a new line)
              </TooltipContent>
            </Tooltip>
          )}
        </InputGroupAddon>
      </InputGroup>
    </div>
  )
}
