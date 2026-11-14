import * as React from "react"
import { open } from "@tauri-apps/plugin-dialog"
import {
  ClapperboardIcon,
  FilmIcon,
  FolderOpenIcon,
  ImagesIcon,
} from "lucide-react"
import { toast } from "sonner"

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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Spinner } from "@/components/ui/spinner"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api, errorText } from "@/lib/api"
import { useStudio } from "@/lib/store"

/** Ask for a project file. */
export async function pickProject(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "edits project", extensions: ["json"] }],
  })
  return typeof picked === "string" ? picked : null
}

const base = (p: string) => p.split(/[\\/]/).pop() ?? p

function PathButton({
  label,
  value,
  onPick,
}: {
  label: string
  value: string
  onPick: () => void
}) {
  return (
    <Button
      variant="outline"
      className="justify-start font-normal"
      onClick={onPick}
    >
      <span className="truncate">{value || label}</span>
    </Button>
  )
}

function NewFromPictures({
  open: isOpen,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  const [pictures, setPictures] = React.useState<string[]>([])
  const [song, setSong] = React.useState("")
  const [dir, setDir] = React.useState("")
  const [aspect, setAspect] = React.useState("9:16")
  const [busy, setBusy] = React.useState(false)
  const create = async () => {
    setBusy(true)
    try {
      await api.projectNew({ dir, pictures, audio: song || null, aspect })
      onOpenChange(false)
    } catch (e) {
      toast.error("Cannot make the project", { description: errorText(e) })
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New from pictures and a song</DialogTitle>
          <DialogDescription>
            The song is analysed for its tempo, then the pictures are cut one
            per beat for its length (at most 16 bars).
          </DialogDescription>
        </DialogHeader>
        <FieldGroup className="gap-4">
          <Field>
            <FieldLabel>Pictures</FieldLabel>
            <PathButton
              label="Choose pictures…"
              value={
                pictures.length
                  ? `${pictures.length} picture${pictures.length === 1 ? "" : "s"}: ${pictures.map(base).join(", ")}`
                  : ""
              }
              onPick={async () => {
                const p = await open({
                  multiple: true,
                  filters: [
                    {
                      name: "Pictures",
                      extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
                    },
                  ],
                })
                if (Array.isArray(p)) setPictures(p)
                else if (p) setPictures([p])
              }}
            />
          </Field>
          <Field>
            <FieldLabel>Song</FieldLabel>
            <PathButton
              label="Choose a song… (optional)"
              value={song ? base(song) : ""}
              onPick={async () => {
                const p = await open({
                  multiple: false,
                  filters: [
                    {
                      name: "Audio",
                      extensions: [
                        "mp3",
                        "wav",
                        "m4a",
                        "aac",
                        "flac",
                        "ogg",
                        "mp4",
                      ],
                    },
                  ],
                })
                if (typeof p === "string") setSong(p)
              }}
            />
          </Field>
          <Field>
            <FieldLabel>Folder for the project</FieldLabel>
            <PathButton
              label="Choose a folder…"
              value={dir}
              onPick={async () => {
                const p = await open({ directory: true, multiple: false })
                if (typeof p === "string") setDir(p)
              }}
            />
          </Field>
          <Field>
            <FieldLabel>Shape</FieldLabel>
            <ToggleGroup
              multiple={false}
              variant="outline"
              value={[aspect]}
              onValueChange={(v) => v[0] && setAspect(v[0])}
            >
              <ToggleGroupItem value="9:16">9:16 vertical</ToggleGroupItem>
              <ToggleGroupItem value="1:1">1:1</ToggleGroupItem>
              <ToggleGroupItem value="16:9">16:9</ToggleGroupItem>
            </ToggleGroup>
          </Field>
        </FieldGroup>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            disabled={!pictures.length || !dir || busy}
            onClick={() => void create()}
          >
            {busy && <Spinner data-icon="inline-start" />}
            {busy ? "Making…" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function NewFromReference({
  open: isOpen,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  const [video, setVideo] = React.useState("")
  const [dir, setDir] = React.useState("")
  const [busy, setBusy] = React.useState(false)
  const create = async () => {
    setBusy(true)
    try {
      await api.projectFromReference(video, dir)
      onOpenChange(false)
    } catch (e) {
      toast.error("Cannot draft from the reference", {
        description: errorText(e),
      })
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New from a reference edit</DialogTitle>
          <DialogDescription>
            A finished edit is analysed (cuts, flashes, zooms, tempo) and
            drafted as a project with stills of it as pictures; replace them
            with yours in Media. Takes about a minute for a 15 s clip.
          </DialogDescription>
        </DialogHeader>
        <FieldGroup className="gap-4">
          <Field>
            <FieldLabel>Reference video</FieldLabel>
            <PathButton
              label="Choose a video…"
              value={video ? base(video) : ""}
              onPick={async () => {
                const p = await open({
                  multiple: false,
                  filters: [
                    {
                      name: "Video",
                      extensions: ["mp4", "mov", "mkv", "webm"],
                    },
                  ],
                })
                if (typeof p === "string") setVideo(p)
              }}
            />
          </Field>
          <Field>
            <FieldLabel>Folder for the project</FieldLabel>
            <PathButton
              label="Choose a folder…"
              value={dir}
              onPick={async () => {
                const p = await open({ directory: true, multiple: false })
                if (typeof p === "string") setDir(p)
              }}
            />
          </Field>
        </FieldGroup>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            disabled={!video || !dir || busy}
            onClick={() => void create()}
          >
            {busy && <Spinner data-icon="inline-start" />}
            {busy ? "Analysing…" : "Draft"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** The first page: open a project, or make one. */
export function OpenProject() {
  const { openProject } = useStudio()
  const [pics, setPics] = React.useState(false)
  const [ref, setRef] = React.useState(false)
  const cards = [
    {
      icon: FolderOpenIcon,
      title: "Open project",
      text: "An edits.project/1 file: yours, the agent's, or the demo (edits demo demo).",
      action: "Open…",
      run: () => pickProject().then((p) => p && void openProject(p)),
    },
    {
      icon: ImagesIcon,
      title: "New from pictures + song",
      text: "Pick pictures and a song: they are cut one per beat, ready to add flashes and transitions.",
      action: "Start…",
      run: () => setPics(true),
    },
    {
      icon: FilmIcon,
      title: "New from reference",
      text: "Copy the timing and effects of a finished edit, then swap in your pictures.",
      action: "Start…",
      run: () => setRef(true),
    },
  ]
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-6 p-6">
      <div className="flex flex-col items-center gap-2 text-center">
        <ClapperboardIcon className="size-8 text-muted-foreground" />
        <h2 className="text-xl font-semibold">Edits Studio</h2>
        <p className="max-w-md text-sm text-muted-foreground">
          Beat-synced edits from pictures and a song. Everything you do here is
          written to one JSON file that an agent can edit with you.
        </p>
      </div>
      <div className="grid w-full max-w-3xl gap-4 md:grid-cols-3">
        {cards.map((c) => (
          <Card key={c.title} className="justify-between">
            <CardHeader>
              <c.icon className="mb-2 size-5 text-muted-foreground" />
              <CardTitle>{c.title}</CardTitle>
              <CardDescription>{c.text}</CardDescription>
            </CardHeader>
            <CardContent />
            <CardFooter>
              <Button
                className="w-full"
                variant={c.title === "Open project" ? "default" : "outline"}
                onClick={() => void c.run()}
              >
                {c.action}
              </Button>
            </CardFooter>
          </Card>
        ))}
      </div>
      <NewFromPictures open={pics} onOpenChange={setPics} />
      <NewFromReference open={ref} onOpenChange={setRef} />
    </div>
  )
}
