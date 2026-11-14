import type { CSSProperties } from "react"

import { AgentDock } from "@/agent/dock"
import { AppSidebar } from "@/components/app-sidebar"
import { CommandMenu } from "@/components/command-menu"
import { Shortcuts } from "@/components/shortcuts"
import { SiteHeader } from "@/components/site-header"
import { OpenProject } from "@/components/open-project"
import { ActivityPage } from "@/components/pages/activity-page"
import { EditPage } from "@/components/pages/edit-page"
import { LibraryPage } from "@/components/pages/library-page"
import { OverviewPage } from "@/components/pages/overview-page"
import { ReferencePage } from "@/components/pages/reference-page"
import { RendersPage } from "@/components/pages/renders-page"
import { useTheme } from "@/components/theme-provider"
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { StudioProvider, useStudio } from "@/lib/store"

function Page() {
  const { page, snapshot } = useStudio()
  if (!snapshot) return <OpenProject />
  switch (page) {
    case "edit":
      return <EditPage />
    case "overview":
      return <OverviewPage />
    case "reference":
      return <ReferencePage />
    case "library":
      return <LibraryPage />
    case "activity":
      return <ActivityPage />
    case "renders":
      return <RendersPage />
  }
}

function ThemedToaster() {
  const { theme } = useTheme()
  return <Toaster theme={theme} position="bottom-right" />
}

export default function App() {
  return (
    <TooltipProvider>
      <StudioProvider>
        <SidebarProvider
          style={
            {
              "--sidebar-width": "calc(var(--spacing) * 64)",
              "--header-height": "calc(var(--spacing) * 12)",
            } as CSSProperties
          }
        >
          <AppSidebar variant="inset" />
          <SidebarInset className="h-svh overflow-hidden md:h-[calc(100svh-1rem)]">
            <SiteHeader />
            <AgentDock>
              <Page />
            </AgentDock>
          </SidebarInset>
        </SidebarProvider>
        <CommandMenu />
        <Shortcuts />
        <ThemedToaster />
      </StudioProvider>
    </TooltipProvider>
  )
}
