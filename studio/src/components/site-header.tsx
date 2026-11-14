import { CircleAlertIcon, Redo2Icon, SearchIcon, Undo2Icon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { Separator } from "@/components/ui/separator"
import { SidebarTrigger } from "@/components/ui/sidebar"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { PAGES, useStudio } from "@/lib/store"

export function SiteHeader() {
  const { page, snapshot, undo, redo, setPaletteOpen, setPage } = useStudio()
  const title = PAGES.find((p) => p.id === page)?.title ?? ""
  const errors = snapshot?.problems.filter((p) => p.error).length ?? 0
  const nextUndo = snapshot?.history.undo[0]
  const nextRedo = snapshot?.history.redo[0]
  const who = (actor: string) =>
    actor === "window"
      ? "your edit"
      : actor === "agent"
        ? "the agent's edit"
        : `${actor}'s edit`
  return (
    <header className="flex h-(--header-height) shrink-0 items-center gap-2 border-b transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-(--header-height)">
      <div className="flex w-full items-center gap-1 px-4 lg:gap-2 lg:px-6">
        <SidebarTrigger className="-ml-1" />
        <Separator
          orientation="vertical"
          className="mx-2 h-4 data-vertical:self-auto"
        />
        <h1 className="text-base font-medium">{title}</h1>
        {snapshot && (
          <div className="ml-2 hidden min-w-0 items-center gap-2 md:flex">
            <span
              className="truncate text-sm text-muted-foreground"
              title={snapshot.path}
            >
              {snapshot.dir.split("/").pop()} / {snapshot.file_name}
            </span>
            <Badge variant="outline" className="tabular-nums">
              rev {snapshot.revision}
            </Badge>
            {errors > 0 && (
              <Badge
                variant="destructive"
                className="cursor-pointer"
                onClick={() => setPage("edit")}
              >
                <CircleAlertIcon />
                {errors} error{errors === 1 ? "" : "s"}
              </Badge>
            )}
          </div>
        )}
        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            className="w-60 justify-start text-muted-foreground"
            onClick={() => setPaletteOpen(true)}
          >
            <SearchIcon data-icon="inline-start" />
            Search commands…
            <KbdGroup className="ml-auto">
              <Kbd>Ctrl</Kbd>
              <Kbd>K</Kbd>
            </KbdGroup>
          </Button>
          <Separator
            orientation="vertical"
            className="mx-1 h-4 data-vertical:self-auto"
          />
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-8"
                  disabled={!nextUndo}
                  onClick={undo}
                />
              }
            >
              <Undo2Icon />
              <span className="sr-only">Undo</span>
            </TooltipTrigger>
            <TooltipContent>
              {nextUndo
                ? `Undo ${who(nextUndo.actor)}: ${nextUndo.readable}`
                : "Nothing to undo"}{" "}
              · Ctrl+Z
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-8"
                  disabled={!nextRedo}
                  onClick={redo}
                />
              }
            >
              <Redo2Icon />
              <span className="sr-only">Redo</span>
            </TooltipTrigger>
            <TooltipContent>
              {nextRedo
                ? `Redo ${who(nextRedo.actor)}: ${nextRedo.readable}`
                : "Nothing to redo"}{" "}
              · Ctrl+Shift+Z
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
    </header>
  )
}
