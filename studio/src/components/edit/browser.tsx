// The browser on the left of the Edit page: the project's pictures and
// everything that can be put on them, by tab (Media, Effects, Transitions,
// Presets, Text, Shapes, Audio). Each item is a card with a small preview
// rendered by edits-render (made once, kept in the user's cache) and one
// line saying what it does. Drag a card onto a clip of the timeline or the
// viewer, or double-click it to apply it to the selected layer at the
// playhead.

import * as React from "react"
import { open } from "@tauri-apps/plugin-dialog"
import { convertFileSrc, isTauri } from "@tauri-apps/api/core"
import {
  AudioLinesIcon,
  CircleIcon,
  EllipsisIcon,
  ImageIcon,
  ImagePlusIcon,
  ImageUpIcon,
  MinusIcon,
  MusicIcon,
  ScissorsIcon,
  SearchIcon,
  SparklesIcon,
  SquareIcon,
  StarIcon,
  TypeIcon,
  WandSparklesIcon,
  ZapIcon,
} from "lucide-react"

import {
  DRAG_MIME,
  useEditor,
  type BrowserItem,
} from "@/components/edit/editor-context"
import { pickPicture } from "@/components/edit/kind-sections"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
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
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
} from "@/components/ui/input-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { api, type Preset } from "@/lib/api"
import { CATEGORY_LABEL, useEffects, usePresets } from "@/lib/library"
import { useStudio } from "@/lib/store"
import { fmtTime } from "@/lib/time"
import { cn } from "@/lib/utils"

const TABS = [
  { id: "media", label: "Media", icon: ImageIcon },
  { id: "effects", label: "Effects", icon: WandSparklesIcon },
  { id: "transitions", label: "Transitions", icon: ScissorsIcon },
  { id: "presets", label: "Presets", icon: ZapIcon },
  { id: "text", label: "Text", icon: TypeIcon },
  { id: "shapes", label: "Shapes", icon: StarIcon },
  { id: "audio", label: "Audio", icon: MusicIcon },
] as const
type Tab = (typeof TABS)[number]["id"]

const pretty = (name: string) =>
  name.replace(/^transition:/, "").replaceAll("_", " ")

/** A preview picture, asked for when the card first scrolls into view. */
function Thumb({
  kind,
  name,
  src,
  icon: Icon,
}: {
  kind?: string
  name?: string
  src?: string | null
  icon?: React.ComponentType<{ className?: string }>
}) {
  const ref = React.useRef<HTMLDivElement>(null)
  const [url, setUrl] = React.useState<string | null>(src ?? null)
  const [failed, setFailed] = React.useState(false)
  React.useEffect(() => {
    if (src !== undefined || !kind || !name || !isTauri()) return
    const el = ref.current
    if (!el) return
    let live = true
    const io = new IntersectionObserver(([e]) => {
      if (!e.isIntersecting) return
      io.disconnect()
      api
        .thumbnail(kind, name)
        .then((p) => live && setUrl(`${convertFileSrc(p)}`))
        .catch(() => live && setFailed(true))
    })
    io.observe(el)
    return () => {
      live = false
      io.disconnect()
    }
  }, [kind, name, src])
  return (
    <div
      ref={ref}
      className="relative aspect-video w-full overflow-hidden rounded-md border bg-muted"
    >
      {url ? (
        <img
          src={url}
          alt=""
          draggable={false}
          className="size-full object-cover"
        />
      ) : Icon || failed ? (
        <div className="flex size-full items-center justify-center text-muted-foreground">
          {Icon ? (
            <Icon className="size-6" />
          ) : (
            <WandSparklesIcon className="size-5" />
          )}
        </div>
      ) : (
        <Skeleton className="size-full rounded-none" />
      )}
    </div>
  )
}

/** One item of the browser (`data-slot="browser-card"`). */
function Card({
  item,
  title,
  description,
  thumb,
  badge,
  menu,
}: {
  item: BrowserItem
  title: string
  description: string
  thumb: React.ReactNode
  badge?: string
  menu?: React.ReactNode
}) {
  const ed = useEditor()
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            role="button"
            tabIndex={0}
            data-slot="browser-card"
            data-item={`${item.type}:${item.name}`}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(DRAG_MIME, JSON.stringify(item))
              e.dataTransfer.effectAllowed = "copy"
            }}
            onDoubleClick={() => void ed.apply(item)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void ed.apply(item)
            }}
            className="group/card relative flex cursor-grab flex-col gap-1.5 rounded-lg p-1.5 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring active:cursor-grabbing"
          />
        }
      >
        {thumb}
        <div className="flex min-w-0 items-center gap-1 px-0.5">
          <span className="truncate text-xs font-medium">{title}</span>
          {badge && (
            <Badge
              variant="outline"
              className="ml-auto h-4 shrink-0 px-1 text-[10px]"
            >
              {badge}
            </Badge>
          )}
        </div>
        <p className="line-clamp-1 px-0.5 text-[11px] leading-tight text-muted-foreground">
          {description}
        </p>
        {menu && (
          <div className="absolute top-2.5 right-2.5 opacity-0 group-hover/card:opacity-100 has-[[data-popup-open]]:opacity-100">
            {menu}
          </div>
        )}
      </TooltipTrigger>
      <TooltipContent side="right" className="max-w-64">
        {description}
        <span className="mt-1 block opacity-70">
          Double-click: apply to the selected layer at the playhead · drag onto
          a clip or the viewer
        </span>
      </TooltipContent>
    </Tooltip>
  )
}

function Grid({ children }: { children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(7.5rem,1fr))] gap-1">
      {children}
    </div>
  )
}

function Group({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-1">
      <h4 className="px-1.5 pt-2 text-xs font-medium text-muted-foreground">
        {title}
      </h4>
      <Grid>{children}</Grid>
    </div>
  )
}

const words = (q: string) => q.toLowerCase().split(/\s+/).filter(Boolean)
const matches = (q: string[], ...text: string[]) => {
  const hay = text.join(" ").toLowerCase()
  return q.every((w) => hay.includes(w))
}

function groupBy<T>(list: T[], key: (t: T) => string) {
  const out = new Map<string, T[]>()
  for (const x of list) {
    const k = key(x)
    out.set(k, [...(out.get(k) ?? []), x])
  }
  return [...out.entries()]
}

// ---- tabs ------------------------------------------------------------------------------------

function MediaTab({ q }: { q: string[] }) {
  const s = useStudio()
  const slots = (s.snapshot?.slots ?? []).filter((x) =>
    matches(q, x.name, x.path ?? "")
  )
  const add = async () => {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [
        {
          name: "Pictures",
          extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
        },
      ],
    })
    const files = Array.isArray(picked) ? picked : picked ? [picked] : []
    for (const f of files) await s.run(api.pictureAdd(f, s.t, true, s.layer))
  }
  return (
    <div className="flex flex-col gap-2">
      <Button
        variant="outline"
        size="sm"
        className="mx-1.5"
        onClick={() => void add()}
      >
        <ImagePlusIcon data-icon="inline-start" />
        Add pictures…
      </Button>
      {slots.length === 0 ? (
        <p className="px-2 text-sm text-muted-foreground">No pictures yet.</p>
      ) : (
        <Group title={`Slots · ${slots.length}`}>
          {slots.map((x) => (
            <Card
              key={x.name}
              item={{ type: "slot", name: x.name }}
              title={x.name}
              description={
                x.used_by.length
                  ? `Used by ${x.used_by.join(", ")}`
                  : x.path
                    ? "Not used yet: double-click to add it at the playhead"
                    : "Empty slot: the placeholder shows"
              }
              badge={x.used_by.length ? String(x.used_by.length) : undefined}
              thumb={
                <Thumb
                  src={x.file && x.exists ? convertFileSrc(x.file) : null}
                  icon={x.file && x.exists ? undefined : ImageIcon}
                />
              }
              menu={
                <DropdownMenu>
                  <DropdownMenuTrigger
                    render={
                      <Button
                        variant="secondary"
                        size="icon-xs"
                        aria-label={`${x.name} menu`}
                        onClick={(e) => e.stopPropagation()}
                      />
                    }
                  >
                    <EllipsisIcon />
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-48">
                    <DropdownMenuItem
                      onClick={() =>
                        pickPicture().then(
                          (p) => p && void s.run(api.slotReplace(x.name, p))
                        )
                      }
                    >
                      <ImageUpIcon />
                      Replace picture…
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onClick={() =>
                        void s.run(api.layerAdd("image", x.name, s.t, s.layer))
                      }
                    >
                      <ImagePlusIcon />
                      Add at {fmtTime(s.info, s.t)}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              }
            />
          ))}
        </Group>
      )}
    </div>
  )
}

function EffectsTab({ q }: { q: string[] }) {
  const effects = useEffects().filter(
    (e) => !e.needs_source && matches(q, e.kind, e.category, e.doc)
  )
  if (!effects.length) return <Nothing />
  return (
    <>
      {groupBy(effects, (e) => e.category).map(([cat, list]) => (
        <Group
          key={cat}
          title={`${CATEGORY_LABEL[cat] ?? cat} · ${list.length}`}
        >
          {list.map((e) => (
            <Card
              key={e.kind}
              item={{ type: "effect", name: e.kind }}
              title={pretty(e.kind)}
              description={e.doc}
              thumb={<Thumb kind="effect" name={e.kind} />}
            />
          ))}
        </Group>
      ))}
    </>
  )
}

function presetItem(p: Preset): BrowserItem {
  return {
    type: "preset",
    name: p.name,
    project: p.target === "project",
    text: p.tags.includes("text"),
  }
}

function TransitionsTab({ q, presets }: { q: string[]; presets: Preset[] }) {
  const list = presets.filter(
    (p) => p.transition && matches(q, p.name, p.description, ...p.tags)
  )
  if (!list.length) return <Nothing />
  return (
    <Group title={`Transitions · ${list.length}`}>
      {list.map((p) => {
        const kind = p.name.replace("transition:", "")
        return (
          <Card
            key={p.name}
            item={{ type: "transition", name: kind }}
            title={pretty(kind)}
            description={p.description}
            thumb={<Thumb kind="transition" name={kind} />}
          />
        )
      })}
    </Group>
  )
}

const GROUP_LABEL: Record<string, string> = {
  hit: "Hits",
  zoom: "Zoom",
  move: "Motion",
  blur: "Blur",
  "3d": "3D",
  look: "Looks",
  speed: "Speed",
  overlay: "Overlays",
  intro: "Reveals",
  particles: "Particles",
  shape: "Shapes",
  text: "Text",
}

function PresetsTab({ q, presets }: { q: string[]; presets: Preset[] }) {
  const list = presets.filter(
    (p) =>
      !p.transition &&
      !p.tags.includes("text") &&
      p.group !== "particles" &&
      p.group !== "shape" &&
      matches(q, p.name, p.description, ...p.tags)
  )
  if (!list.length) return <Nothing />
  return (
    <>
      {groupBy(list, (p) => p.group).map(([g, ps]) => (
        <Group key={g} title={`${GROUP_LABEL[g] ?? g} · ${ps.length}`}>
          {ps.map((p) => (
            <Card
              key={p.name}
              item={presetItem(p)}
              title={pretty(p.name)}
              description={p.description}
              badge={p.target === "project" ? "adds layer" : undefined}
              thumb={<Thumb kind="preset" name={p.name} />}
            />
          ))}
        </Group>
      ))}
    </>
  )
}

function TextTab({ q, presets }: { q: string[]; presets: Preset[] }) {
  const text = presets.filter(
    (p) => p.tags.includes("text") && matches(q, p.name, p.description)
  )
  return (
    <>
      {matches(q, "title text") && (
        <Group title="Add">
          <Card
            item={{ type: "layer", name: "text" }}
            title="Title"
            description="A text layer at the playhead, one bar long: edit its words in the inspector"
            thumb={<Thumb icon={TypeIcon} />}
          />
        </Group>
      )}
      {text.length > 0 && (
        <Group title={`Animations · ${text.length}`}>
          {text.map((p) => (
            <Card
              key={p.name}
              item={presetItem(p)}
              title={pretty(p.name)}
              description={p.description}
              thumb={<Thumb kind="preset" name={p.name} />}
            />
          ))}
        </Group>
      )}
    </>
  )
}

function ShapesTab({ q, presets }: { q: string[]; presets: Preset[] }) {
  const basic: [
    string,
    string,
    React.ComponentType<{ className?: string }>,
    string,
  ][] = [
    ["shape:rect", "Rectangle", SquareIcon, "A rectangle outline"],
    ["shape:ellipse", "Ellipse", CircleIcon, "A circle outline"],
    ["shape:polystar", "Star", StarIcon, "A filled five-point star"],
    ["shape:line", "Line", MinusIcon, "A straight line with round caps"],
    [
      "particles",
      "Particles",
      SparklesIcon,
      "Twinkles over the frame: an emitter to tune in the inspector",
    ],
  ]
  const ready = presets.filter(
    (p) =>
      (p.group === "shape" || p.group === "particles") &&
      matches(q, p.name, p.description, ...p.tags)
  )
  return (
    <>
      <Group title="Add">
        {basic
          .filter(([k, t]) => matches(q, k, t))
          .map(([k, t, icon, d]) => (
            <Card
              key={k}
              item={{ type: "layer", name: k }}
              title={t}
              description={d}
              thumb={<Thumb icon={icon} />}
            />
          ))}
      </Group>
      {ready.length > 0 && (
        <Group title={`Ready-made · ${ready.length}`}>
          {ready.map((p) => (
            <Card
              key={p.name}
              item={presetItem(p)}
              title={pretty(p.name)}
              description={p.description}
              badge="adds layer"
              thumb={<Thumb kind="preset" name={p.name} />}
            />
          ))}
        </Group>
      )}
    </>
  )
}

function AudioTab() {
  const s = useStudio()
  const ed = useEditor()
  const info = s.info
  const song = ed.song
  const marks = song?.marks
  if (!info?.audio)
    return (
      <Empty className="border border-dashed p-4">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <MusicIcon />
          </EmptyMedia>
          <EmptyTitle>No song</EmptyTitle>
          <EmptyDescription>
            The project has no <code>audio</code>. Make a new project from
            pictures and a song, or ask the agent to set one.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  return (
    <div className="flex flex-col gap-3 px-1.5 pt-1">
      <div className="flex items-center gap-2 rounded-lg border p-2">
        <AudioLinesIcon className="size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">
            {info.audio.path.split("/").pop()}
          </p>
          <p className="truncate text-xs text-muted-foreground">
            {info.audio.exists
              ? `from ${info.audio.offset} s · ${s.audioState}`
              : "file missing"}
          </p>
        </div>
      </div>
      {marks ? (
        <p className="text-sm text-muted-foreground">{marks.summary}</p>
      ) : (
        <p className="text-sm text-muted-foreground">
          Analyse the song to see its beats, kicks, snares and sections on the
          timeline and to cut or accent on them.
        </p>
      )}
      <Button
        variant={marks ? "outline" : "default"}
        size="sm"
        disabled={song?.running || !info.audio.exists}
        onClick={ed.analyze}
      >
        {song?.running ? (
          <Spinner data-icon="inline-start" />
        ) : (
          <AudioLinesIcon data-icon="inline-start" />
        )}
        {song?.running
          ? "Analysing…"
          : marks
            ? "Analyse again"
            : "Analyse song"}
      </Button>
      <div className="grid grid-cols-2 gap-2">
        <Button variant="outline" size="sm" onClick={() => ed.setCutOpen(true)}>
          <ScissorsIcon data-icon="inline-start" />
          Beat cut…
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => ed.setAccentOpen(true)}
        >
          <ZapIcon data-icon="inline-start" />
          Accent…
        </Button>
      </div>
      {marks && marks.sections.length > 0 && (
        <div className="flex flex-col gap-1">
          <h4 className="text-xs font-medium text-muted-foreground">
            Sections
          </h4>
          {marks.sections.map(([label, from, to]) => (
            <Button
              key={`${label}${from}`}
              variant="ghost"
              size="sm"
              className="justify-between"
              onClick={() => {
                if (!info) return
                const sec = info.bpm
                  ? info.beat_offset + (from * 60) / info.bpm
                  : from
                s.seek(Math.round(sec * info.fps))
              }}
            >
              <span>{label}</span>
              <span className="text-xs text-muted-foreground tabular-nums">
                {fmtTime(info, from)} – {fmtTime(info, to)}
              </span>
            </Button>
          ))}
        </div>
      )}
    </div>
  )
}

function Nothing() {
  return (
    <p className="px-2 pt-2 text-sm text-muted-foreground">Nothing matches.</p>
  )
}

/** The left browser of the Edit page. */
export function Browser() {
  const s = useStudio()
  const presets = usePresets(s.snapshot?.path ?? null)
  const [tab, setTab] = React.useState<Tab>("effects")
  const [query, setQuery] = React.useState("")
  const q = words(query)
  return (
    <div className="flex h-full min-h-0 flex-col" data-slot="browser">
      <Tabs
        value={tab}
        onValueChange={(v) => setTab(v as Tab)}
        orientation="horizontal"
        className="gap-0"
      >
        <div className="border-b px-2 pt-2 pb-2">
          <TabsList className="grid h-auto w-full grid-cols-4 gap-0.5">
            {TABS.map((t) => (
              <TabsTrigger key={t.id} value={t.id} className="h-7 px-1 text-xs">
                <t.icon />
                <span className="truncate">{t.label}</span>
              </TabsTrigger>
            ))}
          </TabsList>
        </div>
      </Tabs>
      {tab !== "audio" && (
        <div className="px-2 pt-2">
          <InputGroup className="h-8">
            <InputGroupAddon>
              <SearchIcon />
            </InputGroupAddon>
            <InputGroupInput
              placeholder={`Search ${TABS.find((t) => t.id === tab)?.label.toLowerCase()}…`}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              aria-label="Search the browser"
            />
          </InputGroup>
        </div>
      )}
      <ScrollArea className="min-h-0 flex-1">
        <div className={cn("flex flex-col gap-1 p-1.5 pb-4")}>
          {tab === "media" && <MediaTab q={q} />}
          {tab === "effects" && <EffectsTab q={q} />}
          {tab === "transitions" && <TransitionsTab q={q} presets={presets} />}
          {tab === "presets" && <PresetsTab q={q} presets={presets} />}
          {tab === "text" && <TextTab q={q} presets={presets} />}
          {tab === "shapes" && <ShapesTab q={q} presets={presets} />}
          {tab === "audio" && <AudioTab />}
        </div>
      </ScrollArea>
    </div>
  )
}
