"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

/**
 * Mount `children` only once they are near the viewport.
 *
 * Above-the-fold panels stay eager. Everything else waits, so the first
 * screen does not sit behind a fan-out of API calls the user cannot see yet.
 * `rootMargin` starts the fetch before the section scrolls into view.
 */
export function Defer({
  children,
  minHeight = 320,
  rootMargin = "720px",
}: {
  children: ReactNode;
  minHeight?: number;
  rootMargin?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [show, setShow] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      setShow(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setShow(true);
          observer.disconnect();
        }
      },
      { rootMargin },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [rootMargin]);

  return (
    <div ref={ref} style={show ? undefined : { minHeight }}>
      {show ? children : null}
    </div>
  );
}
