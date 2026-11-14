import * as React from "react"
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  MouseSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type UniqueIdentifier,
} from "@dnd-kit/core"
import { restrictToVerticalAxis } from "@dnd-kit/modifiers"
import {
  arrayMove,
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import {
  columnFilteringFeature,
  columnVisibilityFeature,
  createColumnHelper,
  createFilteredRowModel,
  createPaginatedRowModel,
  createSortedRowModel,
  FlexRender,
  rowPaginationFeature,
  rowSelectionFeature,
  rowSortingFeature,
  tableFeatures,
  useTable,
  type ColumnFiltersState,
  type ColumnVisibilityState,
  type Row,
  type SortingState,
} from "@tanstack/react-table"
import {
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ChevronsLeftIcon,
  ChevronsRightIcon,
  CircleCheckIcon,
  CircleOffIcon,
  Columns3Icon,
  EllipsisVerticalIcon,
  GripVerticalIcon,
  PlusIcon,
} from "lucide-react"

import { KindIcon } from "@/components/edit/kind-icon"
import { NumberInput } from "@/components/edit/fields"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
  DrawerTrigger,
} from "@/components/ui/drawer"
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { api, type LayerBrief } from "@/lib/api"
import { useIsMobile } from "@/hooks/use-mobile"
import { useStudio } from "@/lib/store"
import { firstFrameFrom, fmtTime, tidy } from "@/lib/time"

const features = tableFeatures({
  columnFilteringFeature,
  columnVisibilityFeature,
  rowPaginationFeature,
  rowSelectionFeature,
  rowSortingFeature,
  filteredRowModel: createFilteredRowModel(),
  paginatedRowModel: createPaginatedRowModel(),
  sortedRowModel: createSortedRowModel(),
})

const columnHelper = createColumnHelper<typeof features, LayerBrief>()

function DragHandle({ id }: { id: string }) {
  const { attributes, listeners } = useSortable({ id })
  return (
    <Button
      {...attributes}
      {...listeners}
      variant="ghost"
      size="icon"
      className="size-7 text-muted-foreground hover:bg-transparent"
    >
      <GripVerticalIcon className="size-3 text-muted-foreground" />
      <span className="sr-only">Drag to reorder</span>
    </Button>
  )
}

function useColumns() {
  const s = useStudio()
  const setTime = (l: LayerBrief, field: "in" | "out", v: number) =>
    void s.run(
      api.editOps(
        [{ op: "set", pointer: `/layers/@${l.id}/${field}`, value: v }],
        `${field} of ${l.id}: ${v}`
      )
    )
  return React.useMemo(
    () =>
      columnHelper.columns([
        columnHelper.display({
          id: "drag",
          header: () => null,
          cell: ({ row }) => <DragHandle id={row.original.id} />,
        }),
        columnHelper.display({
          id: "select",
          header: ({ table }) => (
            <div className="flex items-center justify-center">
              <Checkbox
                checked={table.getIsAllPageRowsSelected()}
                indeterminate={
                  table.getIsSomePageRowsSelected() &&
                  !table.getIsAllPageRowsSelected()
                }
                onCheckedChange={(value) =>
                  table.toggleAllPageRowsSelected(!!value)
                }
                aria-label="Select all"
              />
            </div>
          ),
          cell: ({ row }) => (
            <div className="flex items-center justify-center">
              <Checkbox
                checked={row.getIsSelected()}
                onCheckedChange={(value) => row.toggleSelected(!!value)}
                aria-label="Select row"
              />
            </div>
          ),
          enableSorting: false,
          enableHiding: false,
        }),
        columnHelper.accessor("id", {
          header: "Layer",
          cell: ({ row }) => <TableCellViewer item={row.original} />,
          enableHiding: false,
        }),
        columnHelper.accessor("kind", {
          header: "Kind",
          cell: ({ row }) => (
            <div className="w-36">
              <Badge variant="outline" className="px-1.5 text-muted-foreground">
                <KindIcon kind={row.original.kind} />
                {row.original.label}
              </Badge>
            </div>
          ),
        }),
        columnHelper.accessor("enabled", {
          header: "Status",
          cell: ({ row }) => (
            <Badge variant="outline" className="px-1.5 text-muted-foreground">
              {row.original.enabled ? (
                <CircleCheckIcon className="fill-green-500 dark:fill-green-400" />
              ) : (
                <CircleOffIcon />
              )}
              {row.original.enabled ? "On" : "Off"}
            </Badge>
          ),
        }),
        columnHelper.accessor("effects", {
          header: "Effects",
          cell: ({ row }) => (
            <span className="line-clamp-1 text-muted-foreground">
              {row.original.effects.map((e) => e.kind).join(", ") || "–"}
            </span>
          ),
        }),
        columnHelper.accessor("start", {
          header: () => <div className="w-full text-right">In</div>,
          cell: ({ row }) => (
            <NumberInput
              className="h-8 w-16 border-transparent bg-transparent shadow-none hover:bg-input/30 focus-visible:border focus-visible:bg-background dark:bg-transparent dark:hover:bg-input/30 dark:focus-visible:bg-input/30"
              value={row.original.start}
              onCommit={(v) => setTime(row.original, "in", v)}
              aria-label={`in of ${row.original.id}`}
            />
          ),
        }),
        columnHelper.accessor("end", {
          header: () => <div className="w-full text-right">Out</div>,
          cell: ({ row }) => (
            <NumberInput
              className="h-8 w-16 border-transparent bg-transparent shadow-none hover:bg-input/30 focus-visible:border focus-visible:bg-background dark:bg-transparent dark:hover:bg-input/30 dark:focus-visible:bg-input/30"
              value={row.original.end}
              onCommit={(v) => setTime(row.original, "out", v)}
              aria-label={`out of ${row.original.id}`}
            />
          ),
        }),
        columnHelper.accessor("keys", {
          header: () => <div className="w-full text-right">Keys</div>,
          cell: ({ row }) => (
            <div className="text-right tabular-nums">{row.original.keys}</div>
          ),
        }),
        columnHelper.accessor("blend", {
          header: "Blend",
          cell: ({ row }) => (
            <span className="text-muted-foreground">{row.original.blend}</span>
          ),
        }),
        columnHelper.display({
          id: "actions",
          cell: ({ row }) => (
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="ghost"
                    className="flex size-8 text-muted-foreground data-open:bg-muted"
                    size="icon"
                  />
                }
              >
                <EllipsisVerticalIcon />
                <span className="sr-only">Open menu</span>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuItem
                  onClick={() => {
                    s.select(row.original.id)
                    if (s.info)
                      s.seek(firstFrameFrom(s.info, row.original.start))
                    s.setPage("edit")
                  }}
                >
                  Edit
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() =>
                    void s.run(api.layerDuplicate(row.original.id))
                  }
                >
                  Make a copy
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => void s.run(api.layerToggle(row.original.id))}
                >
                  {row.original.enabled ? "Disable" : "Enable"}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  onClick={() => void s.run(api.layerRemove(row.original.id))}
                >
                  Delete
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ),
        }),
      ]),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [s.info, s.run, s.select, s.seek, s.setPage]
  )
}

function DraggableRow({ row }: { row: Row<typeof features, LayerBrief> }) {
  const { transform, transition, setNodeRef, isDragging } = useSortable({
    id: row.original.id,
  })
  return (
    <TableRow
      data-state={row.getIsSelected() && "selected"}
      data-dragging={isDragging}
      ref={setNodeRef}
      className="relative z-0 data-[dragging=true]:z-10 data-[dragging=true]:opacity-80"
      style={{
        transform: CSS.Transform.toString(transform),
        transition: transition,
      }}
    >
      {row.getVisibleCells().map((cell) => (
        <TableCell key={cell.id}>
          <FlexRender cell={cell} />
        </TableCell>
      ))}
    </TableRow>
  )
}

function SimpleTable({
  head,
  rows,
  empty,
}: {
  head: string[]
  rows: React.ReactNode[][]
  empty: string
}) {
  return (
    <div className="overflow-hidden rounded-lg border">
      <Table>
        <TableHeader className="bg-muted">
          <TableRow>
            {head.map((h) => (
              <TableHead key={h}>{h}</TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length ? (
            rows.map((r, i) => (
              <TableRow key={i}>
                {r.map((c, j) => (
                  <TableCell key={j}>{c}</TableCell>
                ))}
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={head.length} className="h-24 text-center">
                {empty}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}

/** The layers of the project (top of the picture first), with effects, markers and slots on their own tabs. */
export function DataTable() {
  const s = useStudio()
  const snapshot = s.snapshot!
  // Top of the picture first, as on the timeline.
  const data = React.useMemo(
    () => [...snapshot.layers].reverse(),
    [snapshot.layers]
  )
  const [order, setOrder] = React.useState<LayerBrief[]>(data)
  React.useEffect(() => setOrder(data), [data])
  const columns = useColumns()
  const [rowSelection, setRowSelection] = React.useState({})
  const [columnVisibility, setColumnVisibility] =
    React.useState<ColumnVisibilityState>({})
  const [columnFilters, setColumnFilters] = React.useState<ColumnFiltersState>(
    []
  )
  const [sorting, setSorting] = React.useState<SortingState>([])
  const [pagination, setPagination] = React.useState({
    pageIndex: 0,
    pageSize: 10,
  })
  const sortableId = React.useId()
  const sensors = useSensors(
    useSensor(MouseSensor, {}),
    useSensor(TouchSensor, {}),
    useSensor(KeyboardSensor, {})
  )
  const dataIds = React.useMemo<UniqueIdentifier[]>(
    () => order.map(({ id }) => id),
    [order]
  )
  const table = useTable({
    features,
    data: order,
    columns,
    state: {
      sorting,
      columnVisibility,
      rowSelection,
      columnFilters,
      pagination,
    },
    getRowId: (row) => row.id,
    enableRowSelection: true,
    onRowSelectionChange: setRowSelection,
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    onColumnVisibilityChange: setColumnVisibility,
    onPaginationChange: setPagination,
  })
  function handleDragEnd(event: DragEndEvent) {
    const { active, over } = event
    if (active && over && active.id !== over.id) {
      const oldIndex = dataIds.indexOf(active.id)
      const newIndex = dataIds.indexOf(over.id)
      setOrder((d) => arrayMove(d, oldIndex, newIndex))
      // The table lists the top first; the file lists the bottom first.
      void s.run(
        api.layerReorder(String(active.id), order.length - 1 - newIndex)
      )
    }
  }
  const effects = snapshot.layers.flatMap((l) =>
    l.effects.map((e, i) => ({ layer: l.id, index: i, ...e }))
  )
  return (
    <Tabs defaultValue="layers" className="w-full flex-col justify-start gap-6">
      <div className="flex items-center justify-between px-4 lg:px-6">
        <Label htmlFor="view-selector" className="sr-only">
          View
        </Label>
        <Select
          defaultValue="layers"
          items={[
            { label: "Layers", value: "layers" },
            { label: "Effects", value: "effects" },
            { label: "Markers", value: "markers" },
            { label: "Slots", value: "slots" },
          ]}
        >
          <SelectTrigger
            className="flex w-fit @4xl/main:hidden"
            size="sm"
            id="view-selector"
          >
            <SelectValue placeholder="Select a view" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="layers">Layers</SelectItem>
              <SelectItem value="effects">Effects</SelectItem>
              <SelectItem value="markers">Markers</SelectItem>
              <SelectItem value="slots">Slots</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
        <TabsList className="hidden **:data-[slot=badge]:size-5 **:data-[slot=badge]:rounded-full **:data-[slot=badge]:bg-muted-foreground/30 **:data-[slot=badge]:px-1 @4xl/main:flex">
          <TabsTrigger value="layers">Layers</TabsTrigger>
          <TabsTrigger value="effects">
            Effects <Badge variant="secondary">{effects.length}</Badge>
          </TabsTrigger>
          <TabsTrigger value="markers">
            Markers <Badge variant="secondary">{snapshot.markers.length}</Badge>
          </TabsTrigger>
          <TabsTrigger value="slots">
            Slots <Badge variant="secondary">{snapshot.slots.length}</Badge>
          </TabsTrigger>
        </TabsList>
        <div className="flex items-center gap-2">
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button variant="outline" size="sm" />}
            >
              <Columns3Icon data-icon="inline-start" />
              Columns
              <ChevronDownIcon data-icon="inline-end" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-32">
              {table
                .getAllColumns()
                .filter(
                  (column) =>
                    typeof column.accessorFn !== "undefined" &&
                    column.getCanHide()
                )
                .map((column) => (
                  <DropdownMenuCheckboxItem
                    key={column.id}
                    className="capitalize"
                    checked={column.getIsVisible()}
                    onCheckedChange={(value) =>
                      column.toggleVisibility(!!value)
                    }
                  >
                    {column.id}
                  </DropdownMenuCheckboxItem>
                ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            variant="outline"
            size="sm"
            onClick={() =>
              void s.run(api.layerAdd("adjustment", null, s.t, s.layer))
            }
          >
            <PlusIcon />
            <span className="hidden lg:inline">Add Layer</span>
          </Button>
        </div>
      </div>
      <TabsContent
        value="layers"
        className="relative flex flex-col gap-4 overflow-auto px-4 lg:px-6"
      >
        <div className="overflow-hidden rounded-lg border">
          <DndContext
            collisionDetection={closestCenter}
            modifiers={[restrictToVerticalAxis]}
            onDragEnd={handleDragEnd}
            sensors={sensors}
            id={sortableId}
          >
            <Table>
              <TableHeader className="sticky top-0 z-10 bg-muted">
                {table.getHeaderGroups().map((headerGroup) => (
                  <TableRow key={headerGroup.id}>
                    {headerGroup.headers.map((header) => (
                      <TableHead key={header.id} colSpan={header.colSpan}>
                        {header.isPlaceholder ? null : (
                          <FlexRender header={header} />
                        )}
                      </TableHead>
                    ))}
                  </TableRow>
                ))}
              </TableHeader>
              <TableBody className="**:data-[slot=table-cell]:first:w-8">
                {table.getRowModel().rows?.length ? (
                  <SortableContext
                    items={dataIds}
                    strategy={verticalListSortingStrategy}
                  >
                    {table.getRowModel().rows.map((row) => (
                      <DraggableRow key={row.id} row={row} />
                    ))}
                  </SortableContext>
                ) : (
                  <TableRow>
                    <TableCell
                      colSpan={columns.length}
                      className="h-24 text-center"
                    >
                      No layers.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </DndContext>
        </div>
        <div className="flex items-center justify-between px-4">
          <div className="hidden flex-1 text-sm text-muted-foreground lg:flex">
            {table.getFilteredSelectedRowModel().rows.length} of{" "}
            {table.getFilteredRowModel().rows.length} row(s) selected.
          </div>
          <div className="flex w-full items-center gap-8 lg:w-fit">
            <div className="hidden items-center gap-2 lg:flex">
              <Label htmlFor="rows-per-page" className="text-sm font-medium">
                Rows per page
              </Label>
              <Select
                value={`${table.state.pagination.pageSize}`}
                onValueChange={(value) => table.setPageSize(Number(value))}
                items={[10, 20, 30, 40, 50].map((pageSize) => ({
                  label: `${pageSize}`,
                  value: `${pageSize}`,
                }))}
              >
                <SelectTrigger size="sm" className="w-20" id="rows-per-page">
                  <SelectValue placeholder={table.state.pagination.pageSize} />
                </SelectTrigger>
                <SelectContent side="top">
                  <SelectGroup>
                    {[10, 20, 30, 40, 50].map((pageSize) => (
                      <SelectItem key={pageSize} value={`${pageSize}`}>
                        {pageSize}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="flex w-fit items-center justify-center text-sm font-medium">
              Page {table.state.pagination.pageIndex + 1} of{" "}
              {Math.max(1, table.getPageCount())}
            </div>
            <div className="ml-auto flex items-center gap-2 lg:ml-0">
              <Button
                variant="outline"
                className="hidden h-8 w-8 p-0 lg:flex"
                onClick={() => table.setPageIndex(0)}
                disabled={!table.getCanPreviousPage()}
              >
                <span className="sr-only">Go to first page</span>
                <ChevronsLeftIcon />
              </Button>
              <Button
                variant="outline"
                className="size-8"
                size="icon"
                onClick={() => table.previousPage()}
                disabled={!table.getCanPreviousPage()}
              >
                <span className="sr-only">Go to previous page</span>
                <ChevronLeftIcon />
              </Button>
              <Button
                variant="outline"
                className="size-8"
                size="icon"
                onClick={() => table.nextPage()}
                disabled={!table.getCanNextPage()}
              >
                <span className="sr-only">Go to next page</span>
                <ChevronRightIcon />
              </Button>
              <Button
                variant="outline"
                className="hidden size-8 lg:flex"
                size="icon"
                onClick={() => table.setPageIndex(table.getPageCount() - 1)}
                disabled={!table.getCanNextPage()}
              >
                <span className="sr-only">Go to last page</span>
                <ChevronsRightIcon />
              </Button>
            </div>
          </div>
        </div>
      </TabsContent>
      <TabsContent value="effects" className="flex flex-col px-4 lg:px-6">
        <SimpleTable
          head={["Layer", "Effect", "Order", "Status"]}
          empty="No effects."
          rows={effects.map((e) => [
            <span className="font-medium">{e.layer}</span>,
            e.kind,
            <span className="tabular-nums">{e.index + 1}</span>,
            <Badge variant="outline" className="px-1.5 text-muted-foreground">
              {e.enabled ? (
                <CircleCheckIcon className="fill-green-500 dark:fill-green-400" />
              ) : (
                <CircleOffIcon />
              )}
              {e.enabled ? "On" : "Off"}
            </Badge>,
          ])}
        />
      </TabsContent>
      <TabsContent value="markers" className="flex flex-col px-4 lg:px-6">
        <SimpleTable
          head={["Marker", "Time", "Frame"]}
          empty="No markers. Analyse the song or add them to the file."
          rows={snapshot.markers.map((m) => [
            <Button
              variant="link"
              className="px-0 text-foreground"
              onClick={() =>
                s.info &&
                (s.seek(firstFrameFrom(s.info, m.t)), s.setPage("edit"))
              }
            >
              {m.label}
            </Button>,
            <span className="tabular-nums">{fmtTime(s.info, m.t)}</span>,
            <span className="tabular-nums">
              {s.info ? firstFrameFrom(s.info, m.t) : ""}
            </span>,
          ])}
        />
      </TabsContent>
      <TabsContent value="slots" className="flex flex-col px-4 lg:px-6">
        <SimpleTable
          head={["Slot", "Picture", "Fit", "Used by"]}
          empty="No slots."
          rows={snapshot.slots.map((sl) => [
            <span className="font-medium">{sl.name}</span>,
            <span
              className={sl.exists ? "text-foreground" : "text-destructive"}
            >
              {sl.path ?? "(empty)"}
            </span>,
            <span className="text-muted-foreground">{sl.fit}</span>,
            <span className="text-muted-foreground">
              {sl.used_by.join(", ") || "–"}
            </span>,
          ])}
        />
      </TabsContent>
    </Tabs>
  )
}

function TableCellViewer({ item }: { item: LayerBrief }) {
  const isMobile = useIsMobile()
  const s = useStudio()
  const [blend, setBlend] = React.useState(item.blend)
  const [start, setStart] = React.useState(item.start)
  const [end, setEnd] = React.useState(item.end)
  React.useEffect(() => {
    setBlend(item.blend)
    setStart(item.start)
    setEnd(item.end)
  }, [item])
  const save = () => {
    const ops = [
      ...(start !== item.start
        ? [{ op: "set", pointer: `/layers/@${item.id}/in`, value: start }]
        : []),
      ...(end !== item.end
        ? [{ op: "set", pointer: `/layers/@${item.id}/out`, value: end }]
        : []),
      ...(blend !== item.blend
        ? [{ op: "set", pointer: `/layers/@${item.id}/blend`, value: blend }]
        : []),
    ]
    if (ops.length)
      void s.run(api.editOps(ops, `${item.id}: in, out and blend`))
  }
  const row = s.snapshot?.rows.find((r) => r.id === item.id)
  return (
    <Drawer swipeDirection={isMobile ? "down" : "right"}>
      <DrawerTrigger
        render={
          <Button
            variant="link"
            className="w-fit px-0 text-left text-foreground"
          />
        }
      >
        {item.id}
      </DrawerTrigger>
      <DrawerContent>
        <DrawerHeader className="gap-1">
          <DrawerTitle>{item.id}</DrawerTitle>
          <DrawerDescription>
            {item.label} · {fmtTime(s.info, item.start)} to{" "}
            {fmtTime(s.info, item.end)}
          </DrawerDescription>
        </DrawerHeader>
        <div className="flex flex-col gap-4 overflow-y-auto px-4 text-sm">
          <div className="grid gap-2">
            <div className="flex gap-2 leading-none font-medium">
              {item.effects.length} effects · {item.keys} keys
              {item.transition ? ` · comes in with ${item.transition}` : ""}
            </div>
            <div className="text-muted-foreground">
              {item.effects.map((e) => e.kind).join(", ") || "No effects."} Keys
              at {row?.marks.map((m) => tidy(m.t, 2)).join(", ") || "none"}.
            </div>
          </div>
          <Separator />
          <form
            className="flex flex-col gap-4"
            onSubmit={(e) => {
              e.preventDefault()
              save()
            }}
          >
            <div className="grid grid-cols-2 gap-4">
              <div className="flex flex-col gap-3">
                <Label>In</Label>
                <NumberInput
                  className="h-8 text-left"
                  value={start}
                  onCommit={setStart}
                />
              </div>
              <div className="flex flex-col gap-3">
                <Label>Out</Label>
                <NumberInput
                  className="h-8 text-left"
                  value={end}
                  onCommit={setEnd}
                />
              </div>
            </div>
            <div className="flex flex-col gap-3">
              <Label>Blend</Label>
              <Select
                value={blend}
                onValueChange={(v) => v && setBlend(v)}
                items={[
                  "normal",
                  "add",
                  "screen",
                  "multiply",
                  "overlay",
                  "soft_light",
                  "difference",
                ].map((b) => ({ label: b, value: b }))}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {[
                      "normal",
                      "add",
                      "screen",
                      "multiply",
                      "overlay",
                      "soft_light",
                      "difference",
                    ].map((b) => (
                      <SelectItem key={b} value={b}>
                        {b}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
          </form>
        </div>
        <DrawerFooter>
          <DrawerClose render={<Button onClick={save} />}>Save</DrawerClose>
          <DrawerClose render={<Button variant="outline" />}>Done</DrawerClose>
        </DrawerFooter>
      </DrawerContent>
    </Drawer>
  )
}
