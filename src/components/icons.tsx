import {
  Box,
  Container,
  FolderInput,
  Globe,
  Laptop,
  Layers,
  Monitor,
  QrCode,
  Server,
  Smartphone,
  Tablet,
  Code2,
  Feather,
  Hexagon,
} from "lucide-react";
import type { ProjectKind, TargetKind } from "../lib/types";

export function GithubIcon({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.53-1.34-1.3-1.7-1.3-1.7-1.05-.72.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.04 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.58.23 2.75.11 3.04.74.81 1.19 1.83 1.19 3.09 0 4.42-2.7 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" />
    </svg>
  );
}

export function TargetIcon({ kind, size = 18 }: { kind: TargetKind | string; size?: number }) {
  switch (kind) {
    case "ssh":
      return <Server size={size} />;
    case "android":
      return <Smartphone size={size} />;
    case "ios":
      return <Tablet size={size} />;
    case "local":
      return <Monitor size={size} />;
    case "folder":
      return <FolderInput size={size} />;
    case "share":
      return <QrCode size={size} />;
    case "github":
      return <GithubIcon size={size} />;
    case "network":
      return <Laptop size={size} />;
    default:
      return <Globe size={size} />;
  }
}

export function KindIcon({ kind, size = 18 }: { kind: ProjectKind | string; size?: number }) {
  switch (kind) {
    case "flutter":
      return <Feather size={size} />;
    case "react-native":
      return <Layers size={size} />;
    case "tauri":
      return <Box size={size} />;
    case "node":
      return <Hexagon size={size} />;
    case "docker":
      return <Container size={size} />;
    default:
      return <Code2 size={size} />;
  }
}
