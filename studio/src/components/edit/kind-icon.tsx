import {
  BoxIcon,
  CameraIcon,
  ImageIcon,
  LayersIcon,
  ShapesIcon,
  SparklesIcon,
  SquareIcon,
  TypeIcon,
  VideoIcon,
  WandSparklesIcon,
} from "lucide-react"

/** The icon of a layer kind. */
export function KindIcon({
  kind,
  className,
}: {
  kind: string
  className?: string
}) {
  const Icon =
    {
      image: ImageIcon,
      solid: SquareIcon,
      cube: BoxIcon,
      adjustment: WandSparklesIcon,
      video: VideoIcon,
      particles: SparklesIcon,
      text: TypeIcon,
      comp: LayersIcon,
      shape: ShapesIcon,
      camera: CameraIcon,
    }[kind] ?? LayersIcon
  return <Icon className={className} />
}
