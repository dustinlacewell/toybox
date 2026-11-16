import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { IconButton } from "./IconButton";

interface Props {
  size?: "compact" | "medium" | "wide";
  fullScreen?: boolean;
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  /** Optional action row pinned at the bottom. */
  footer?: ReactNode;
}

/**
 * A centred modal for work that needs room. Portalled to the body so a Drawer
 * cannot clip it; its scrim sits above the drawer's. Clicking the scrim or
 * pressing Escape closes it. The body does not scroll: the content owns that.
 */
export function Dialog({ open, onClose, title, children, footer, fullScreen, size = "wide" }: Props) {
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    panel.current?.focus();
    return () => { if (previous?.isConnected) previous.focus(); };
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return createPortal(
    <div
      className="ds-dialog-scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div ref={panel} tabIndex={-1} className={`ds-dialog ds-dialog--${size}${fullScreen ? " ds-dialog--full" : ""}`} role="dialog" aria-modal="true" aria-label={title} onKeyDown={event => {
        if (event.key !== "Tab") return;
        const items = [...event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],[tabindex="0"]')].filter(item => item.getClientRects().length);
        const first = items[0], last = items[items.length - 1];
        if (!first) { event.preventDefault(); return; }
        if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }}>
        <div className="ds-dialog__header">
          <span className="ds-dialog__title">{title}</span>
          <IconButton icon={X} label="Close" onClick={onClose} />
        </div>
        <div className="ds-dialog__body">{children}</div>
        {footer && <div className="ds-dialog__footer">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}
