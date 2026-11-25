import { X, type LucideIcon } from "lucide-react";
import "./SelectionBar.css";

/** One action of the bar. `primary` is the action most often taken; a disabled one shows its reason as a tooltip. */
export interface SelectionAction { id: string; label: string; icon?: LucideIcon; tone?: "danger"; primary?: boolean; disabled?: boolean; reason?: string }

/**
 * A floating bar over the bottom of its positioned parent while several things are selected: how many, the actions
 * that apply to them, and a button to let the selection go. Its owner decides when it shows and what each action does.
 * In a narrow parent the actions show their icons only, with the label as the tooltip.
 */
export function SelectionBar({ label, actions, onAction, onClear, clearLabel = "Clear selection" }: {
  label: string; actions: readonly SelectionAction[]; onAction: (id: string) => void; onClear: () => void; clearLabel?: string;
}) {
  return <div className="ds-selection-bar-dock">
    <div className="ds-selection-bar" role="toolbar" aria-label={`${label}: actions`}>
      <span className="ds-selection-bar__label" aria-live="polite">{label}</span>
      {actions.map(({ id, label: text, icon: Icon, tone, primary, disabled, reason }) => (
        <button key={id} type="button" disabled={disabled} aria-label={text} title={disabled && reason ? `${text} (${reason})` : text}
          className={`ds-button ${primary ? "ds-button--primary" : "ds-button--ghost"} ds-selection-bar__action${tone === "danger" ? " is-danger" : ""}`}
          onClick={() => onAction(id)}>
          {Icon && <Icon size={15} strokeWidth={1.75} aria-hidden />}<span className="ds-selection-bar__text">{text}</span>
        </button>
      ))}
      <span className="ds-selection-bar__divider" aria-hidden />
      <button type="button" className="ds-icon-button ds-selection-bar__clear" aria-label={clearLabel} title={`${clearLabel} (Esc)`} onClick={onClear}><X size={16} strokeWidth={1.75} /></button>
    </div>
  </div>;
}
