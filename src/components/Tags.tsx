import { useEffect, useRef, useState } from "react";
import { api, TagUsage, TagView } from "../api";
import { useI18n } from "../i18n";

export const TAG_PALETTE = [
  "#ef4444", "#f97316", "#f59e0b", "#84cc16", "#22c55e", "#14b8a6",
  "#06b6d4", "#3b82f6", "#6366f1", "#a855f7", "#ec4899", "#64748b",
];

/** Black or white, whichever reads better on `hex` (WCAG relative luminance). */
function textColorFor(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const channel = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const lum = 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255);
  return lum > 0.4 ? "#111827" : "#ffffff";
}

export function TagChip({ tag, onRemove, small }: { tag: TagView; onRemove?: () => void; small?: boolean }) {
  const { t } = useI18n();
  return (
    <span
      className={`tag-chip${small ? " small" : ""}`}
      style={{ background: tag.color, color: textColorFor(tag.color) }}
      title={tag.name}
    >
      {tag.name}
      {onRemove && (
        <button
          className="tag-chip-remove"
          aria-label={t("tags.remove", { name: tag.name })}
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
        >
          ×
        </button>
      )}
    </span>
  );
}

export function ColorPicker({ value, onChange }: { value: string; onChange: (c: string) => void }) {
  const { t } = useI18n();
  return (
    <div className="color-picker" role="radiogroup" aria-label={t("tags.color")}>
      {TAG_PALETTE.map((c) => (
        <button
          key={c}
          role="radio"
          aria-checked={value.toLowerCase() === c}
          className={`swatch${value.toLowerCase() === c ? " selected" : ""}`}
          style={{ background: c }}
          onClick={() => onChange(c)}
          title={c}
        />
      ))}
      <label className="swatch custom" title={t("tags.customColor")}>
        <input type="color" value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    </div>
  );
}

interface PickerProps {
  assigned: TagView[];
  onChange: (tags: TagView[]) => void;
  onError: (e: unknown) => void;
}

/** "Add tag" button with a popover listing existing tags and a form to create a new one. */
export function TagPicker({ assigned, onChange, onError }: PickerProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [all, setAll] = useState<TagUsage[]>([]);
  const [name, setName] = useState("");
  const [color, setColor] = useState(TAG_PALETTE[7]);
  const ref = useRef<HTMLDivElement>(null);
  // Held in a ref so an inline callback from the parent does not re-run the effect each render.
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  useEffect(() => {
    if (!open) return;
    api.listTags().then(setAll).catch((e) => onErrorRef.current(e));
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const available = all.filter((tag) => !assigned.some((a) => a.id === tag.id));

  async function create() {
    try {
      const tag = await api.createTag(name, color);
      setName("");
      onChange([...assigned, tag]);
      setOpen(false);
    } catch (e) {
      onError(e);
    }
  }

  return (
    <div className="tag-picker" ref={ref}>
      <button className="tag-add" onClick={() => setOpen((o) => !o)}>
        + {t("tags.add")}
      </button>
      {open && (
        <div className="popover">
          {available.length > 0 && (
            <div className="tag-options">
              {available.map((tag) => (
                <button
                  key={tag.id}
                  className="tag-option"
                  onClick={() => {
                    onChange([...assigned, tag]);
                    setOpen(false);
                  }}
                >
                  <TagChip tag={tag} small />
                </button>
              ))}
            </div>
          )}
          <div className="popover-section">
            <div className="muted small">{t("tags.newTag")}</div>
            <input
              value={name}
              placeholder={t("tags.namePlaceholder")}
              maxLength={40}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && name.trim() && create()}
            />
            <ColorPicker value={color} onChange={setColor} />
            <div className="actions">
              {name.trim() && <TagChip tag={{ id: 0, name: name.trim(), color }} small />}
              <button className="primary" disabled={!name.trim()} onClick={create}>
                {t("tags.create")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Name + color form for an existing tag. */
export function TagEditor({
  tag,
  onSaved,
  onCancel,
  onError,
}: {
  tag: TagView;
  onSaved: (tag: TagView) => void;
  onCancel: () => void;
  onError: (e: unknown) => void;
}) {
  const { t } = useI18n();
  const [name, setName] = useState(tag.name);
  const [color, setColor] = useState(tag.color);

  async function save() {
    try {
      onSaved(await api.updateTag(tag.id, name, color));
    } catch (e) {
      onError(e);
    }
  }

  return (
    <div className="tag-editor">
      <input
        autoFocus
        value={name}
        maxLength={40}
        placeholder={t("tags.namePlaceholder")}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && name.trim()) save();
          if (e.key === "Escape") onCancel();
        }}
      />
      <ColorPicker value={color} onChange={setColor} />
      <div className="actions">
        <TagChip tag={{ id: tag.id, name: name.trim() || tag.name, color }} />
        <button className="primary" onClick={save} disabled={!name.trim()}>
          {t("session.save")}
        </button>
        <button onClick={onCancel}>{t("session.cancel")}</button>
      </div>
      <p className="muted small">{t("tags.editHint")}</p>
    </div>
  );
}

/** Tag chip that opens an editor for its name and color when clicked. */
export function EditableTagChip({
  tag,
  onRemove,
  onEdited,
  onError,
}: {
  tag: TagView;
  onRemove?: () => void;
  onEdited: (tag: TagView) => void;
  onError: (e: unknown) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div className="tag-picker" ref={ref}>
      {/* A span, not a button: the chip's own remove button cannot be nested in a button. */}
      <span
        className="link-button"
        role="button"
        tabIndex={0}
        title={t("tags.editTag")}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && setOpen((o) => !o)}
      >
        <TagChip tag={tag} onRemove={onRemove} />
      </span>
      {open && (
        <div className="popover">
          <TagEditor
            tag={tag}
            onSaved={(edited) => {
              setOpen(false);
              onEdited(edited);
            }}
            onCancel={() => setOpen(false)}
            onError={onError}
          />
        </div>
      )}
    </div>
  );
}
