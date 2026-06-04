import type { ReactNode } from "react";

interface Props {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

/** A right-side overlay panel. Clicking the scrim closes it. */
export function Drawer({ open, onClose, children }: Props) {
  if (!open) return null;
  return (
    <div
      className="ds-drawer-scrim"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="ds-drawer">{children}</div>
    </div>
  );
}
