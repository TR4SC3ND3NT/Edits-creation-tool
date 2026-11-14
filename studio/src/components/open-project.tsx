import { open } from "@tauri-apps/plugin-dialog"
import { ClapperboardIcon, FolderOpenIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty"
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

export function OpenProject() {
  const { openProject } = useStudio()
  return (
    <div className="flex flex-1 items-center justify-center p-6">
      <Empty className="max-w-md border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <ClapperboardIcon />
          </EmptyMedia>
          <EmptyTitle>No project open</EmptyTitle>
          <EmptyDescription>
            Open an <code>edits.project/1</code> file, or start the studio with
            one: <code>edits gui demo/project.json</code>. Make a demo with{" "}
            <code>edits demo demo</code>.
          </EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button
            onClick={() =>
              pickProject().then((p) => {
                if (p) void openProject(p)
              })
            }
          >
            <FolderOpenIcon data-icon="inline-start" />
            Open project…
          </Button>
        </EmptyContent>
      </Empty>
    </div>
  )
}
