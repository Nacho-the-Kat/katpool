import { Badge } from "@/components/ui/badge";
import type { BlockStatus } from "@/lib/api/types";

const LABELS: Record<BlockStatus, { label: string; variant: "default" | "secondary" | "success" | "warning" | "destructive" | "outline" }> = {
  found: { label: "Found", variant: "outline" },
  submitted_to_node: { label: "Submitted", variant: "secondary" },
  confirmed_blue: { label: "Confirmed", variant: "default" },
  matured: { label: "Matured", variant: "success" },
  orphaned: { label: "Orphaned", variant: "destructive" },
};

/** A colored badge for a block lifecycle status. */
export function BlockStatusBadge({ status }: { status: BlockStatus }) {
  const meta = LABELS[status];
  return <Badge variant={meta.variant}>{meta.label}</Badge>;
}
