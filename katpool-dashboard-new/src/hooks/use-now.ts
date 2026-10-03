"use client";

type Listener = (now: number) => void;

const listeners = new Set<Listener>();
let timer: number | null = null;

function ensureClock(): void {
  if (timer != null || typeof window === "undefined") return;
  timer = window.setInterval(() => {
    const now = Date.now();
    listeners.forEach((listener) => listener(now));
  }, 1000);
}

function stopClock(): void {
  if (listeners.size > 0 || timer == null) return;
  window.clearInterval(timer);
  timer = null;
}

/** One shared 1s clock. Listeners paint themselves; React does not re-render. */
export function subscribeNow(listener: Listener): () => void {
  listeners.add(listener);
  ensureClock();
  return () => {
    listeners.delete(listener);
    stopClock();
  };
}
