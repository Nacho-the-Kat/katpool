"use client";

import { type ReactNode } from "react";
import { motion } from "framer-motion";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";

/** A titled content panel with an optional actions slot (range toggles etc.). */
export function Panel({
  title,
  description,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, ease: "easeOut" }}
    >
      <Card className={cn("overflow-hidden", className)}>
        {(title || actions) && (
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border p-5">
            <div>
              {title ? <h3 className="text-base font-semibold tracking-tight">{title}</h3> : null}
              {description ? (
                <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>
              ) : null}
            </div>
            {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
          </div>
        )}
        <div className={cn("p-5", bodyClassName)}>{children}</div>
      </Card>
    </motion.div>
  );
}
