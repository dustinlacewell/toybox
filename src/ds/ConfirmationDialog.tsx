import type { ReactNode } from "react";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import "./ConfirmationDialog.css";

/** Controlled confirmation layout. Callers supply content and own the action. */
export function ConfirmationDialog({ title, description, items, note, confirmLabel, busyLabel = "Working…", busy = false, disabled = false, error, onConfirm, onClose }: {
  title: string; description?: ReactNode;
  items?: { id: string; label: string; detail?: string; preview?: ReactNode }[];
  note?: ReactNode; confirmLabel: string; busyLabel?: string;
  busy?: boolean; disabled?: boolean; error?: string;
  onConfirm: () => void; onClose: () => void;
}) {
  return <Dialog open size="compact" title={title} onClose={() => { if (!busy) onClose(); }} footer={<>
    <Button autoFocus disabled={busy} onClick={onClose}>Cancel</Button>
    <Button variant="danger" disabled={busy || disabled} onClick={onConfirm}>{busy ? busyLabel : confirmLabel}</Button>
  </>}>
    <div className="ds-confirmation" aria-busy={busy}>
      {description && <p className="ds-confirmation__description">{description}</p>}
      {!!items?.length && <div className="ds-confirmation__items" role="list" aria-label="Selected items" tabIndex={0}>
        {items.map(item => <div key={item.id} className="ds-confirmation__item" role="listitem">
          {item.preview && <div className="ds-confirmation__preview">{item.preview}</div>}
          <div className="ds-confirmation__name"><span title={item.label}>{item.label}</span>{item.detail && <small>{item.detail}</small>}</div>
        </div>)}
      </div>}
      {note && <p className="ds-confirmation__note">{note}</p>}
      {error && <p className="ds-confirmation__error" role="alert">{error}</p>}
    </div>
  </Dialog>;
}
