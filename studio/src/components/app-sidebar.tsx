import * as React from "react"
import {
  ActivityIcon,
  AudioLinesIcon,
  ClapperboardIcon,
  CommandIcon,
  FileJsonIcon,
  FilmIcon,
  FolderOpenIcon,
  GaugeIcon,
  LayoutDashboardIcon,
  ScanEyeIcon,
  SlidersHorizontalIcon,
  SparklesIcon,
} from "lucide-react"

import { NavDocuments } from "@/components/nav-documents"
import { NavMain } from "@/components/nav-main"
import { NavSecondary } from "@/components/nav-secondary"
import { NavUser } from "@/components/nav-user"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import { useStudio, type Page } from "@/lib/store"
import { pickProject } from "@/components/open-project"

const NAV: { page: Page; title: string; icon: React.ReactNode }[] = [
  { page: "edit", title: "Edit", icon: <SlidersHorizontalIcon /> },
  { page: "overview", title: "Overview", icon: <LayoutDashboardIcon /> },
  { page: "reference", title: "Reference", icon: <ScanEyeIcon /> },
  { page: "library", title: "Presets & Effects", icon: <SparklesIcon /> },
  { page: "activity", title: "Activity", icon: <ActivityIcon /> },
  { page: "renders", title: "Renders", icon: <FilmIcon /> },
]

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const { snapshot, setPaletteOpen, openProject } = useStudio()
  const files = snapshot
    ? [
        {
          name: snapshot.file_name,
          path: snapshot.path,
          icon: <FileJsonIcon />,
        },
        ...(snapshot.info?.audio
          ? [
              {
                name: snapshot.info.audio.path.split("/").pop() ?? "audio",
                path: snapshot.info.audio.path,
                icon: <AudioLinesIcon />,
              },
            ]
          : []),
        {
          name: ".edits (history, ui.json)",
          path: `${snapshot.dir}/.edits`,
          icon: <GaugeIcon />,
        },
      ]
    : []
  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton className="data-[slot=sidebar-menu-button]:p-1.5!">
              <ClapperboardIcon className="size-5!" />
              <span className="text-base font-semibold">Edits Studio</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <NavMain items={NAV} />
        {files.length > 0 && <NavDocuments items={files} />}
        <NavSecondary
          className="mt-auto"
          items={[
            {
              title: "Command palette",
              icon: <CommandIcon />,
              onClick: () => setPaletteOpen(true),
              shortcut: "Ctrl K",
            },
            {
              title: "Open project…",
              icon: <FolderOpenIcon />,
              onClick: () => {
                pickProject().then((p) => {
                  if (p) void openProject(p)
                })
              },
            },
          ]}
        />
      </SidebarContent>
      <SidebarFooter>
        <NavUser />
      </SidebarFooter>
    </Sidebar>
  )
}
