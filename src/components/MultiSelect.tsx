import { ReactNode, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";

export interface Option {
  value: string;
  label: string;
  /** Tooltip, e.g. the full project path. */
  title?: string;
  /** Rendered instead of the plain label (e.g. a colored tag pill). */
  render?: ReactNode;
}

interface Props {
  /** Shown when nothing is selected (meaning "all"). */
  allLabel: string;
  /** Prefix for the button when several values are selected. */
  label: string;
  options: Option[];
  selected: string[];
  onChange: (values: string[]) => void;
  /** Extra controls at the top of the popover (e.g. AND/OR). */
  header?: ReactNode;
}

/** Dropdown with a checkbox list. No selection means no filtering. */
export default function MultiSelect({ allLabel, label, options, selected, onChange, header }: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  const chosen = options.filter((o) => selected.includes(o.value));
  const buttonText =
    chosen.length === 0
      ? allLabel
      : chosen.length === 1
        ? chosen[0].label
        : t("search.selectedCount", { label, count: chosen.length });
  const visible = filter
    ? options.filter((o) => `${o.label} ${o.title ?? ""}`.toLowerCase().includes(filter.toLowerCase()))
    : options;

  function toggle(value: string) {
    onChange(selected.includes(value) ? selected.filter((v) => v !== value) : [...selected, value]);
  }

  return (
    <div className="multi-select" ref={ref}>
      <button
        className={`multi-select-button${chosen.length ? " active" : ""}`}
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        title={chosen.map((o) => o.label).join(", ") || allLabel}
      >
        <span className="multi-select-text">{buttonText}</span>
        <span aria-hidden>▾</span>
      </button>
      {open && (
        <div className="popover multi-select-popover">
          {header}
          {options.length > 8 && (
            <input
              autoFocus
              type="search"
              value={filter}
              placeholder={t("search.filterOptions")}
              onChange={(e) => setFilter(e.target.value)}
            />
          )}
          <ul className="multi-select-options" role="listbox" aria-multiselectable>
            {visible.length === 0 && <li className="muted small">{t("search.noOptions")}</li>}
            {visible.map((o) => (
              <li key={o.value}>
                <label className="checkbox-line option-line" title={o.title}>
                  <input type="checkbox" checked={selected.includes(o.value)} onChange={() => toggle(o.value)} />
                  {o.render ?? <span className="option-label">{o.label}</span>}
                </label>
              </li>
            ))}
          </ul>
          <div className="actions">
            <button className="small-button" onClick={() => onChange([])} disabled={selected.length === 0}>
              {t("search.clearSelection")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
