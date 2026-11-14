"use client"

import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener"
import {
  CopyIcon,
  FolderIcon,
  MoreHorizontalIcon,
  SquareArrowOutUpRightIcon,
} from "lucide-react"
import { toast } from "sonner"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import { errorText } from "@/lib/api"

export function NavDocuments({
  items,
}: {
  items: {
    name: string
    path: string
    icon: React.ReactNode
  }[]
}) {
  const { isMobile } = useSidebar()
  const fail = (e: unknown) =>
    toast.error("Cannot open it", { description: errorText(e) })
  return (
    <SidebarGroup className="group-data-[collapsible=icon]:hidden">
      <SidebarGroupLabel>Project files</SidebarGroupLabel>
      <SidebarMenu>
        {items.map((item) => (
          <SidebarMenuItem key={item.path}>
            <SidebarMenuButton
              onClick={() => openPath(item.path).catch(fail)}
              title={item.path}
            >
              {item.icon}
              <span>{item.name}</span>
            </SidebarMenuButton>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <SidebarMenuAction
                    showOnHover
                    className="aria-expanded:bg-muted"
                  />
                }
              >
                <MoreHorizontalIcon />
                <span className="sr-only">More</span>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                className="w-40"
                side={isMobile ? "bottom" : "right"}
                align={isMobile ? "end" : "start"}
              >
                <DropdownMenuItem
                  onClick={() => openPath(item.path).catch(fail)}
                >
                  <SquareArrowOutUpRightIcon />
                  <span>Open</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => revealItemInDir(item.path).catch(fail)}
                >
                  <FolderIcon />
                  <span>Show in folder</span>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onClick={() =>
                    navigator.clipboard
                      .writeText(item.path)
                      .then(() =>
                        toast("Path copied", { description: item.path })
                      )
                      .catch(fail)
                  }
                >
                  <CopyIcon />
                  <span>Copy path</span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
        ))}
      </SidebarMenu>
    </SidebarGroup>
  )
}
