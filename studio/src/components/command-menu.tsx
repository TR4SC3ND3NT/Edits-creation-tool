import {
  BoxIcon,
  ChevronFirstIcon,
  ChevronLastIcon,
  CircleDotIcon,
  CopyIcon,
  EyeIcon,
  FilmIcon,
  FlagIcon,
  LayersIcon,
  PauseIcon,
  PlayIcon,
  PlusIcon,
  Redo2Icon,
  RepeatIcon,
  SparklesIcon,
  SquareIcon,
  Trash2Icon,
  Undo2Icon,
  WandSparklesIcon,
} from "lucide-react"
import { toast } from "sonner"

import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
  Command,
} from "@/components/ui/command"
import { api, errorText } from "@/lib/api"
import { CATEGORY_LABEL, useEffects, usePresets } from "@/lib/library"
import { PAGES, useStudio } from "@/lib/store"
import { firstFrameFrom, fmtTime, lastFrame } from "@/lib/time"

/** Ctrl+K: everything the studio can do, by name. */
export function CommandMenu() {
  const s = useStudio()
  const effects = useEffects()
  const presets = usePresets(s.snapshot?.path ?? null)
  const close = () => s.setPaletteOpen(false)
  const act = (f: () => void) => () => {
    close()
    f()
  }
  const selected = s.snapshot?.layers.find((l) => l.id === s.layer) ?? null
  const slots = s.snapshot?.slots ?? []
  return (
    <CommandDialog
      open={s.paletteOpen}
      onOpenChange={s.setPaletteOpen}
      title="Command palette"
      description="Search for a command to run"
    >
      <Command>
        <CommandInput placeholder="Type a command or search…" />
        <CommandList className="max-h-96">
          <CommandEmpty>No results found.</CommandEmpty>
          <CommandGroup heading="Go to">
            {PAGES.map((p, i) => (
              <CommandItem
                key={p.id}
                value={`page ${p.title}`}
                onSelect={act(() => s.setPage(p.id))}
              >
                <LayersIcon />
                {p.title}
                <CommandShortcut>Ctrl {i + 1}</CommandShortcut>
              </CommandItem>
            ))}
          </CommandGroup>
          {s.info && (
            <CommandGroup heading="Playback">
              <CommandItem value="play pause" onSelect={act(s.togglePlay)}>
                {s.playing ? <PauseIcon /> : <PlayIcon />}
                {s.playing ? "Pause" : "Play"}
                <CommandShortcut>Space</CommandShortcut>
              </CommandItem>
              <CommandItem
                value="start first frame"
                onSelect={act(() => s.seek(0))}
              >
                <ChevronFirstIcon />
                Go to the start
                <CommandShortcut>Home</CommandShortcut>
              </CommandItem>
              <CommandItem
                value="end last frame"
                onSelect={act(() => s.seek(lastFrame(s.info!)))}
              >
                <ChevronLastIcon />
                Go to the last frame
                <CommandShortcut>End</CommandShortcut>
              </CommandItem>
              <CommandItem
                value="loop"
                onSelect={act(() => s.setLooping(!s.looping))}
              >
                <RepeatIcon />
                {s.looping ? "Stop looping" : "Loop"}
                <CommandShortcut>L</CommandShortcut>
              </CommandItem>
              {s.snapshot?.markers.map((m) => (
                <CommandItem
                  key={`m${m.t}`}
                  value={`marker ${m.label}`}
                  onSelect={act(() => s.seek(firstFrameFrom(s.info!, m.t)))}
                >
                  <FlagIcon />
                  Go to {m.label} ({fmtTime(s.info, m.t)})
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          <CommandGroup heading="Edit">
            <CommandItem
              value="undo"
              disabled={!s.snapshot?.history.undo.length}
              onSelect={act(s.undo)}
            >
              <Undo2Icon />
              Undo{" "}
              {s.snapshot?.history.undo[0]
                ? `(${s.snapshot.history.undo[0].readable})`
                : ""}
              <CommandShortcut>Ctrl Z</CommandShortcut>
            </CommandItem>
            <CommandItem
              value="redo"
              disabled={!s.snapshot?.history.redo.length}
              onSelect={act(s.redo)}
            >
              <Redo2Icon />
              Redo
              <CommandShortcut>Ctrl Shift Z</CommandShortcut>
            </CommandItem>
            <CommandItem
              value="render video export"
              onSelect={act(() => s.setPage("renders"))}
            >
              <FilmIcon />
              Render video…
            </CommandItem>
          </CommandGroup>
          {s.snapshot && (
            <CommandGroup heading="Add layer">
              <CommandItem
                value="add solid layer"
                onSelect={act(
                  () => void s.run(api.layerAdd("solid", null, s.t, s.layer))
                )}
              >
                <SquareIcon />
                Add a solid layer
              </CommandItem>
              <CommandItem
                value="add adjustment layer"
                onSelect={act(
                  () =>
                    void s.run(api.layerAdd("adjustment", null, s.t, s.layer))
                )}
              >
                <SparklesIcon />
                Add an adjustment layer
              </CommandItem>
              {slots.map((sl) => (
                <CommandItem
                  key={`img${sl.name}`}
                  value={`add image layer ${sl.name}`}
                  onSelect={act(
                    () =>
                      void s.run(api.layerAdd("image", sl.name, s.t, s.layer))
                  )}
                >
                  <PlusIcon />
                  Add an image layer of slot {sl.name}
                </CommandItem>
              ))}
              {slots.map((sl) => (
                <CommandItem
                  key={`cube${sl.name}`}
                  value={`add cube ${sl.name}`}
                  onSelect={act(
                    () =>
                      void s.run(api.layerAdd("cube", sl.name, s.t, s.layer))
                  )}
                >
                  <BoxIcon />
                  Add a cube with slot {sl.name}
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {selected && (
            <CommandGroup heading={`Layer ${selected.id}`}>
              <CommandItem
                value="enable disable layer"
                onSelect={act(() => void s.run(api.layerToggle(selected.id)))}
              >
                <EyeIcon />
                {selected.enabled ? "Disable" : "Enable"} {selected.id}
              </CommandItem>
              <CommandItem
                value="duplicate layer"
                onSelect={act(
                  () => void s.run(api.layerDuplicate(selected.id))
                )}
              >
                <CopyIcon />
                Duplicate {selected.id}
              </CommandItem>
              <CommandItem
                value="delete layer"
                onSelect={act(() => void s.run(api.layerRemove(selected.id)))}
              >
                <Trash2Icon />
                Delete {selected.id}
                <CommandShortcut>Del</CommandShortcut>
              </CommandItem>
              {effects
                .filter((e) => !e.needs_source)
                .map((e) => (
                  <CommandItem
                    key={`fx${e.kind}`}
                    value={`add effect ${e.kind} ${e.category}`}
                    onSelect={act(
                      () => void s.run(api.effectAdd(selected.id, e.kind))
                    )}
                  >
                    <CircleDotIcon />
                    Add {e.kind} to {selected.id}
                    <CommandShortcut>
                      {CATEGORY_LABEL[e.category] ?? e.category}
                    </CommandShortcut>
                  </CommandItem>
                ))}
            </CommandGroup>
          )}
          {s.snapshot && (
            <CommandGroup heading="Select">
              {s.snapshot.layers.map((l) => (
                <CommandItem
                  key={`sel${l.id}`}
                  value={`select ${l.id} ${l.label}`}
                  onSelect={act(() => s.select(l.id))}
                >
                  <LayersIcon />
                  Select {l.id}
                  <CommandShortcut>{l.kind}</CommandShortcut>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          <CommandSeparator />
          {s.snapshot && presets.length > 0 && (
            <CommandGroup heading="Apply preset at the playhead">
              {presets.map((p) => (
                <CommandItem
                  key={`p${p.name}`}
                  value={`preset ${p.name} ${p.tags.join(" ")}`}
                  onSelect={act(() => {
                    api
                      .presetApply({
                        name: p.name,
                        target: s.layer ? "selected" : "auto",
                        selected: s.layer,
                        at: p.transition ? "cut" : "playhead",
                        playhead: s.t,
                        values: {},
                      })
                      .then((d) =>
                        toast(`Applied ${p.name}`, { description: d.summary })
                      )
                      .catch((e) =>
                        toast.error(`Cannot apply ${p.name}`, {
                          description: errorText(e),
                        })
                      )
                  })}
                >
                  <WandSparklesIcon />
                  {p.name}
                  <CommandShortcut>{p.group}</CommandShortcut>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
