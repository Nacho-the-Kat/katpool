import { CheckCheck, CircleCheck, CircleDashed, CircleX, UploadCloud, type LucideIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { BlockStatus } from "@/lib/api/types";

type BadgeVariant = "default" | "secondary" | "success" | "warning" | "destructive" | "outline";

// Each status carries an icon as well as a color, so lifecycle state is never
// communicated by hue alone (color-blind-safe).
const LABELS: Record<BlockStatus, { label: string; variant: BadgeVariant; icon: LucideIcon }> = {
  found: { label: "Found", variant: "outline", icon: CircleDashed },
  submitted_to_node: { label: "Submitted", variant: "secondary", icon: UploadCloud },
  confirmed_blue: { label: "Confirmed", variant: "default", icon: CircleCheck },
  matured: { label: "Matured", variant: "success", icon: CheckCheck },
  orphaned: { label: "Orphaned", variant: "destructive", icon: CircleX },
};

/** A colored, icon-tagged badge for a block lifecycle status. */
export function BlockStatusBadge({ status }: { status: BlockStatus }) {
  const meta = LABELS[status];
  const Icon = meta.icon;
  return (
    <Badge variant={meta.variant}>
      <Icon className="size-3" />
      {meta.label}
    </Badge>
  );
}
