import { Brand } from "./brand";
import { SidebarNav } from "./sidebar-nav";
import { PoolPulse } from "./pool-pulse";

/** Fixed desktop sidebar (lg+). */
export function Sidebar() {
  return (
    <aside className="sticky top-0 hidden h-screen w-64 shrink-0 flex-col border-r border-border px-4 py-5 lg:flex">
      <Brand className="px-2" />
      <div className="mt-8 flex-1">
        <p className="px-3 pb-2 text-xs font-medium uppercase tracking-wider text-muted-foreground/70">
          Navigation
        </p>
        <SidebarNav />
      </div>
      <PoolPulse />
    </aside>
  );
}
