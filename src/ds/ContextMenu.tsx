import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { LucideIcon } from "lucide-react";
import { placeMenu } from "./placeMenu";
import "./ContextMenu.css";

export type MenuEntry =
  | { kind: "heading"; label: string }
  | { kind: "separator" }
  | { kind: "item"; id: string; label: string; icon?: LucideIcon; tone?: "danger"; disabled?: boolean; reason?: string };

type MenuItem = Extract<MenuEntry, { kind: "item" }>;

/** Space kept between the menu and the viewport's edges. */
const MARGIN = 8;

/**
 * A menu floating at a point. It reports the picked item's id and asks to close; the owner decides what
 * either means. Pointer hover and the keyboard move one active row.
 */
export function ContextMenu({ at, entries, onPick, onClose, label }: {
  at: { x: number; y: number }; entries: MenuEntry[];
  onPick: (id: string) => void; onClose: () => void; label?: string;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);
  const position = usePlacement(menuRef, at, entries);
  const enabled = entries.flatMap((entry, index) => isEnabled(entry) ? [index] : []);
  const [active, setActive] = useState(enabled[0] ?? -1);
  useReturnFocus();
  // A hidden element cannot take the focus: it moves in once the menu is placed.
  useLayoutEffect(() => { if (position) (active >= 0 ? rowRefs.current[active] : menuRef.current)?.focus({ preventScroll: true }); }, [active, position]);
  useDismissal(menuRef, onClose);

  const pick = (index: number) => {
    const entry = entries[index];
    if (!isEnabled(entry)) return;
    onClose();
    onPick(entry.id);
  };
  const keyDown = (event: KeyboardEvent) => {
    const next = nextRow(event.key, active, enabled, entries);
    if (next !== undefined) { event.preventDefault(); setActive(next); return; }
    if (event.key === "Enter" || event.key === " ") { event.preventDefault(); if (active >= 0) pick(active); }
    else if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); event.stopPropagation(); onClose(); }
  };

  return createPortal(
    <div ref={menuRef} className="ds-context-menu" role="menu" aria-label={label} tabIndex={-1}
      style={position ? { left: position.left, top: position.top } : { left: at.x, top: at.y, visibility: "hidden" }}
      onKeyDown={keyDown} onContextMenu={(event) => event.preventDefault()}>
      {entries.map((entry, index) => entry.kind === "heading" ? <div key={index} className="ds-context-menu__heading" role="presentation">{entry.label}</div>
        : entry.kind === "separator" ? <div key={index} className="ds-context-menu__separator" role="separator" />
        : <Row key={entry.id} item={entry} active={index === active} rowRef={(element) => { rowRefs.current[index] = element; }}
            onHover={() => { if (!entry.disabled) setActive(index); }} onPick={() => pick(index)} />)}
    </div>,
    document.body,
  );
}

function Row({ item, active, rowRef, onHover, onPick }: {
  item: MenuItem; active: boolean; rowRef: (element: HTMLDivElement | null) => void; onHover: () => void; onPick: () => void;
}) {
  const Icon = item.icon;
  return <div ref={rowRef} role="menuitem" tabIndex={-1} aria-disabled={item.disabled || undefined}
    className={`ds-context-menu__item${item.tone === "danger" ? " is-danger" : ""}${active ? " is-active" : ""}`}
    onPointerMove={onHover} onClick={onPick}>
    <span className="ds-context-menu__icon">{Icon && <Icon size={16} aria-hidden="true" />}</span>
    <span className="ds-context-menu__label">{item.label}</span>
    {item.disabled && item.reason && <span className="ds-context-menu__reason">{item.reason}</span>}
  </div>;
}

function isEnabled(entry: MenuEntry | undefined): entry is MenuItem {
  return entry?.kind === "item" && !entry.disabled;
}

/** The row a navigation key moves to, over enabled rows only; undefined when the key does not navigate. */
function nextRow(key: string, active: number, enabled: number[], entries: MenuEntry[]): number | undefined {
  if (!enabled.length) return undefined;
  const at = enabled.indexOf(active);
  if (key === "ArrowDown") return enabled[(at + 1) % enabled.length];
  if (key === "ArrowUp") return enabled[(at - 1 + enabled.length) % enabled.length];
  if (key === "Home") return enabled[0];
  if (key === "End") return enabled[enabled.length - 1];
  if (key.length !== 1 || !key.trim()) return undefined;
  const letter = key.toLowerCase();
  const order = enabled.map((_, step) => enabled[(at + 1 + step) % enabled.length]);
  return order.find((index) => (entries[index] as MenuItem).label.toLowerCase().startsWith(letter));
}

/** Measured after the first layout, then moved inside the viewport; null until then, so it is hidden. */
function usePlacement(menuRef: RefObject<HTMLDivElement | null>, at: { x: number; y: number }, entries: MenuEntry[]) {
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const menu = menuRef.current; if (!menu) return;
    setPosition(placeMenu(at, { width: menu.offsetWidth, height: menu.offsetHeight }, { width: window.innerWidth, height: window.innerHeight }, MARGIN));
  }, [menuRef, at.x, at.y, entries]);
  return position;
}

/** The element focused before the menu opened gets the focus back when it closes. */
function useReturnFocus() {
  useLayoutEffect(() => {
    const previous = document.activeElement;
    return () => { if (previous instanceof HTMLElement && previous.isConnected) previous.focus({ preventScroll: true }); };
  }, []);
}

/** Pressing or right-clicking outside, scrolling, resizing or leaving the window closes the menu. */
function useDismissal(menuRef: RefObject<HTMLDivElement | null>, onClose: () => void) {
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const outside = (event: Event) => { if (!menuRef.current?.contains(event.target as Node)) close.current(); };
    const always = () => close.current();
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("contextmenu", outside, true);
    document.addEventListener("scroll", outside, true);
    window.addEventListener("blur", always);
    window.addEventListener("resize", always);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("contextmenu", outside, true);
      document.removeEventListener("scroll", outside, true);
      window.removeEventListener("blur", always);
      window.removeEventListener("resize", always);
    };
  }, [menuRef]);
}
