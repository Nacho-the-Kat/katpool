import { Brand } from "./brand";
import { SidebarNav } from "./sidebar-nav";
import { PoolPulse } from "./pool-pulse";

/** Fixed desktop sidebar (lg+). */
export function Sidebar() {
  return (
    <aside className="dark sticky top-0 hidden h-screen w-64 shrink-0 flex-col border-r border-border bg-brand-bg px-4 py-5 lg:flex">
      <Brand className="px-1.5" />
      <div className="mt-8 flex-1">
        <p className="px-3 pb-2 text-[0.6875rem] font-medium uppercase tracking-[0.12em] text-muted-foreground/70">
          Navigation
        </p>
        <SidebarNav />
      </div>
      <PoolPulse />
    </aside>
  );
}
