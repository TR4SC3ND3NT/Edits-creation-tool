import * as React from "react"
import { CirclePlusIcon, UndoIcon } from "lucide-react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import { api, errorText } from "@/lib/api"
import { useStudio, type Page } from "@/lib/store"

export function NavMain({
  items,
}: {
  items: {
    page: Page
    title: string
    icon?: React.ReactNode
  }[]
}) {
  const { page, setPage, snapshot, undo } = useStudio()
  const quickRender = async () => {
    setPage("renders")
    try {
      const job = await api.renderStart(null, null)
      toast("Render started", { description: job.output })
    } catch (e) {
      toast.error("Cannot render", { description: errorText(e) })
    }
  }
  const badge = (p: Page) => {
    if (!snapshot) return null
    if (p === "edit" && snapshot.problems.some((x) => x.error))
      return snapshot.problems.filter((x) => x.error).length
    if (p === "activity" && snapshot.history.undo.length)
      return snapshot.history.undo.length
    return null
  }
  return (
    <SidebarGroup>
      <SidebarGroupContent className="flex flex-col gap-2">
        <SidebarMenu>
          <SidebarMenuItem className="flex items-center gap-2">
            <SidebarMenuButton
              tooltip="Quick Render"
              disabled={!snapshot}
              onClick={quickRender}
              className="min-w-8 bg-primary text-primary-foreground duration-200 ease-linear hover:bg-primary/90 hover:text-primary-foreground active:bg-primary/90 active:text-primary-foreground"
            >
              <CirclePlusIcon />
              <span>Quick Render</span>
            </SidebarMenuButton>
            <Button
              size="icon"
              className="size-8 group-data-[collapsible=icon]:opacity-0"
              variant="outline"
              disabled={!snapshot?.history.undo.length}
              onClick={undo}
            >
              <UndoIcon />
              <span className="sr-only">Undo</span>
            </Button>
          </SidebarMenuItem>
        </SidebarMenu>
        <SidebarMenu>
          {items.map((item) => (
            <SidebarMenuItem key={item.page}>
              <SidebarMenuButton
                tooltip={item.title}
                isActive={page === item.page}
                onClick={() => setPage(item.page)}
              >
                {item.icon}
                <span>{item.title}</span>
              </SidebarMenuButton>
              {badge(item.page) != null && (
                <SidebarMenuBadge>{badge(item.page)}</SidebarMenuBadge>
              )}
            </SidebarMenuItem>
          ))}
        </SidebarMenu>
      </SidebarGroupContent>
    </SidebarGroup>
  )
}
