import {
  CpuIcon,
  EllipsisVerticalIcon,
  GpuIcon,
  HardDriveIcon,
  MonitorIcon,
  MoonIcon,
  SunIcon,
  TimerIcon,
  Volume2Icon,
} from "lucide-react"

import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import { useTheme } from "@/components/theme-provider"
import { useStudio } from "@/lib/store"
import { bytes } from "@/lib/time"

/** The footer of the sidebar: the renderer the preview runs on. */
export function NavUser() {
  const { isMobile } = useSidebar()
  const { renderStatus: rs, audioState } = useStudio()
  const { theme, setTheme } = useTheme()
  const name =
    rs?.adapter ??
    (rs?.error ? "No graphics adapter" : "Starting the renderer…")
  const kind = rs?.error
    ? "preview unavailable"
    : rs?.software
      ? "software renderer"
      : rs?.adapter
        ? "GPU renderer"
        : "…"
  const Icon = rs?.software || rs?.error ? CpuIcon : GpuIcon
  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <SidebarMenuButton size="lg" className="aria-expanded:bg-muted" />
            }
          >
            <Avatar className="size-8 rounded-lg">
              <AvatarFallback className="rounded-lg">
                <Icon className="size-4" />
              </AvatarFallback>
            </Avatar>
            <div className="grid flex-1 text-left text-sm leading-tight">
              <span className="truncate font-medium">{name}</span>
              <span className="truncate text-xs text-foreground/70">
                {kind}
              </span>
            </div>
            <EllipsisVerticalIcon className="ml-auto size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="min-w-64"
            side={isMobile ? "bottom" : "right"}
            align="end"
            sideOffset={4}
          >
            <DropdownMenuGroup>
              <DropdownMenuLabel className="p-0 font-normal">
                <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                  <Avatar className="size-8">
                    <AvatarFallback className="rounded-lg">
                      <Icon className="size-4" />
                    </AvatarFallback>
                  </Avatar>
                  <div className="grid flex-1 text-left text-sm leading-tight">
                    <span className="truncate font-medium">{name}</span>
                    <span className="truncate text-xs text-muted-foreground">
                      {rs?.error ?? `${rs?.backend ?? ""} · ${kind}`}
                    </span>
                  </div>
                </div>
              </DropdownMenuLabel>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem disabled>
                <TimerIcon />
                Last frame{" "}
                {rs?.last_ms != null ? `${rs.last_ms.toFixed(0)} ms` : "–"}
              </DropdownMenuItem>
              <DropdownMenuItem disabled>
                <HardDriveIcon />
                Cache{" "}
                {rs
                  ? `${rs.cached_frames} frames · ${bytes(rs.cached_bytes)}`
                  : "–"}
              </DropdownMenuItem>
              <DropdownMenuItem disabled>
                <Volume2Icon />
                {audioState}
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem
                onClick={() => setTheme(theme === "light" ? "dark" : "light")}
              >
                {theme === "light" ? <MoonIcon /> : <SunIcon />}
                {theme === "light" ? "Dark theme" : "Light theme"}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setTheme("system")}>
                <MonitorIcon />
                Follow the system
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}
