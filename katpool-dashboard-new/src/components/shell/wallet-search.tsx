"use client";

import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";
import { Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/** Global wallet search; routes to the miner page on submit. */
export function WalletSearch({ className }: { className?: string }) {
  const router = useRouter();
  const [value, setValue] = useState("");

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    const addr = value.trim();
    if (!addr) return;
    router.push(`/miners/${encodeURIComponent(addr)}`);
  }

  return (
    <form onSubmit={onSubmit} className={cn("relative w-full max-w-md", className)}>
      <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Search wallet address (kaspa:…)"
        spellCheck={false}
        autoComplete="off"
        className="pl-9"
        aria-label="Search wallet address"
      />
    </form>
  );
}
