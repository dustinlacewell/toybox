import type { MouseEvent } from "react";
import { X } from "lucide-react";
import "./Chip.css";

interface Props {
  label: string;
  /** `accent` sets a chip apart from its neutral neighbours. */
  tone?: "neutral" | "accent";
  title?: string;
  role?: string;
  /** Makes the label a button; the event carries the modifier keys. */
  onPress?: (event: MouseEvent<HTMLButtonElement>) => void;
  /** Adds a small × after the label. */
  onRemove?: () => void;
  removeLabel?: string;
  disabled?: boolean;
}

/** A rounded label; optionally pressable and removable. Callers own its meaning. */
export function Chip({ label, tone = "neutral", title, role, onPress, onRemove, removeLabel, disabled }: Props) {
  return <span className={`ds-chip ds-chip--${tone}`} role={role}>
    {onPress ? <button type="button" className="ds-chip__label" title={title} onClick={onPress}>{label}</button>
      : <span className="ds-chip__label" title={title}>{label}</span>}
    {onRemove && <button type="button" className="ds-chip__remove" aria-label={removeLabel ?? `Remove ${label}`} title="Remove" disabled={disabled} onClick={onRemove}><X size={11} aria-hidden="true" /></button>}
  </span>;
}
