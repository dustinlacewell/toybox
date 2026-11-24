import { Fragment, type MouseEvent, type ReactNode } from "react";
import "./SuggestionList.css";

export interface SuggestionRow {
  key: string;
  label: ReactNode;
  icon?: ReactNode;
  count?: number;
  /** A key hint at the row's end. */
  hint?: ReactNode;
  /** A group heading shown above this row. */
  heading?: ReactNode;
}

interface Props {
  rows: SuggestionRow[];
  /** The highlighted row (see `useActiveRow`). */
  active: number;
  onPick: (index: number, event: MouseEvent<HTMLButtonElement>) => void;
  onHover?: (index: number) => void;
  /** Shown under the rows, for instance when there are none. */
  empty?: ReactNode;
  footer?: ReactNode;
  /** Opens under its anchor, or over it when the anchor sits low. */
  placement?: "below" | "above";
  className?: string;
  label?: string;
}

/** A popup list of choices under (or over) a text field. Pressing it never takes the focus from the field. */
export function SuggestionList({ rows, active, onPick, onHover, empty, footer, placement = "below", className = "", label }: Props) {
  return <div className={`ds-suggestions ds-suggestions--${placement} ${className}`} role="listbox" aria-label={label} onMouseDown={(event) => event.preventDefault()}>
    {rows.map((row, index) => <Fragment key={row.key}>
      {row.heading && <div className="ds-suggestions__label">{row.heading}</div>}
      <button type="button" className={`ds-suggestions__item${index === active ? " is-active" : ""}`}
        onMouseEnter={onHover && (() => onHover(index))} onClick={(event) => onPick(index, event)}>
        {row.icon}<span className="ds-suggestions__value">{row.label}</span>
        {row.count !== undefined && <span className="ds-suggestions__count">{row.count.toLocaleString()}</span>}
        {row.hint && <kbd>{row.hint}</kbd>}
      </button>
    </Fragment>)}
    {empty && <div className="ds-suggestions__empty">{empty}</div>}
    {footer && <div className="ds-suggestions__footer">{footer}</div>}
  </div>;
}
