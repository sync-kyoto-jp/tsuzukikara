import { RefObject, useCallback, useState } from "react";

/** Width in px remembered in localStorage (a per-machine view preference); `null` = default. */
export function useStoredWidth(key: string): [number | null, (w: number | null) => void] {
  const [width, setWidth] = useState<number | null>(() => {
    try {
      const v = Number(localStorage.getItem(key));
      return Number.isFinite(v) && v > 0 ? v : null;
    } catch {
      return null;
    }
  });
  const update = useCallback(
    (w: number | null) => {
      setWidth(w);
      try {
        if (w == null) localStorage.removeItem(key);
        else localStorage.setItem(key, String(Math.round(w)));
      } catch {
        // Storage may be unavailable; the width then lasts for this run only.
      }
    },
    [key],
  );
  return [width, update];
}

interface SplitterProps {
  /** The grid container; positions are measured from its left edge. */
  containerRef: RefObject<HTMLElement | null>;
  /** The left pane, measured when the width is still the CSS default. */
  leftRef: RefObject<HTMLElement | null>;
  width: number | null;
  onChange: (width: number | null) => void;
  minLeft: number;
  minRight: number;
  label: string;
}

const KEY_STEP = 24;
const KEY_STEP_LARGE = 96;

/** Vertical drag handle between two panes. Double-click resets to the default width. */
export function Splitter({ containerRef, leftRef, width, onChange, minLeft, minRight, label }: SplitterProps) {
  const [dragging, setDragging] = useState(false);

  const clamp = useCallback(
    (w: number) => {
      const total = containerRef.current?.getBoundingClientRect().width ?? w + minRight;
      return Math.max(minLeft, Math.min(w, total - minRight));
    },
    [containerRef, minLeft, minRight],
  );

  function onPointerDown(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
  }

  function onPointerMove(e: React.PointerEvent<HTMLDivElement>) {
    if (!dragging || !containerRef.current) return;
    const left = containerRef.current.getBoundingClientRect().left;
    onChange(clamp(e.clientX - left));
  }

  function onPointerUp(e: React.PointerEvent<HTMLDivElement>) {
    if (!dragging) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    setDragging(false);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    const step = e.shiftKey ? KEY_STEP_LARGE : KEY_STEP;
    const current = width ?? leftRef.current?.getBoundingClientRect().width ?? minLeft;
    if (e.key === "ArrowLeft") onChange(clamp(current - step));
    else if (e.key === "ArrowRight") onChange(clamp(current + step));
    else if (e.key === "Home") onChange(clamp(minLeft));
    else if (e.key === "End") onChange(clamp(Number.MAX_SAFE_INTEGER));
    else if (e.key === "Escape" || e.key === "Enter") onChange(null);
    else return;
    e.preventDefault();
  }

  return (
    <div
      className={`splitter${dragging ? " dragging" : ""}`}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={width != null ? Math.round(width) : undefined}
      tabIndex={0}
      title={label}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => onChange(null)}
      onKeyDown={onKeyDown}
    />
  );
}
