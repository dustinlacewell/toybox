import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { IconButton } from "./IconButton";

interface Props {
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
export function Dialog({ open, onClose, title, children, footer }: Props) {
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
      <div className="ds-dialog" role="dialog" aria-modal="true" aria-label={title}>
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
