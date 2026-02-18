import { useCallback, useEffect, useMemo, useRef, useState } from "react";

export type CommandPaletteItem = {
  id: string;
  label: string;
  description?: string;
  shortcut?: string;
  action: () => void;
  secondaryAction?: { label: string; action: (e: React.MouseEvent) => void };
};

type CommandPaletteProps = {
  open: boolean;
  title: string;
  items: CommandPaletteItem[];
  query: string;
  onQueryChange: (value: string) => void;
  onClose: () => void;
};

export function CommandPalette({
  open,
  title,
  items,
  query,
  onQueryChange,
  onClose,
}: CommandPaletteProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      return items;
    }
    return items.filter((item) => {
      return `${item.label} ${item.description ?? ""}`.toLowerCase().includes(q);
    });
  }, [items, query]);

  const [activeIndex, setActiveIndex] = useState(0);

  const clampedActiveIndex =
    filtered.length === 0 ? 0 : Math.min(activeIndex, filtered.length - 1);

  useEffect(() => {
    if (!open) {
      return;
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActiveIndex((current) => Math.min(current + 1, Math.max(filtered.length - 1, 0)));
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveIndex((current) => Math.max(current - 1, 0));
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        const active = filtered[clampedActiveIndex];
        if (active) {
          active.action();
          onClose();
        }
        return;
      }
      if (event.key === "Tab" && panelRef.current) {
        const focusable = panelRef.current.querySelectorAll<HTMLElement>(
          'button, input, [tabindex]:not([tabindex="-1"])'
        );
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [clampedActiveIndex, filtered, onClose, open]);

  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  if (!open) {
    return null;
  }

  return (
    <div
      className="palette-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onClick={handleBackdropClick}
    >
      <div className="palette-panel" ref={panelRef}>
        <header className="palette-header">
          <strong>{title}</strong>
          <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label="Close palette">
            Esc
          </button>
        </header>

        <input
          className="palette-search"
          value={query}
          onChange={(event) => {
            setActiveIndex(0);
            onQueryChange(event.target.value);
          }}
          placeholder="Search commands or sessions"
          aria-label="Search commands or sessions"
          autoFocus
        />

        <div className="palette-list" role="listbox" aria-label="Results">
          {filtered.length === 0 ? (
            <div className="empty-state compact">No results.</div>
          ) : (
            filtered.map((item, index) => (
              <div
                key={item.id}
                role="option"
                aria-selected={index === clampedActiveIndex}
                className={`palette-item ${index === clampedActiveIndex ? "is-active" : ""}`}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => {
                  item.action();
                  onClose();
                }}
              >
                <div className="palette-item-main">
                  <div className="palette-item-title">{item.label}</div>
                  {item.description ? <div className="palette-item-description">{item.description}</div> : null}
                </div>
                {item.secondaryAction ? (
                  <button
                    className="btn btn-ghost btn-sm palette-item-secondary"
                    onClick={(e) => { e.stopPropagation(); item.secondaryAction!.action(e); }}
                    aria-label={item.secondaryAction.label}
                    title={item.secondaryAction.label}
                  >
                    {item.secondaryAction.label}
                  </button>
                ) : null}
                {item.shortcut ? <kbd>{item.shortcut}</kbd> : null}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
