import type { ReactNode } from "react";
import "./Field.css";

/** A small label over its value. */
export function Field({ label, children, className }: { label: ReactNode; children: ReactNode; className?: string }) {
  return <div className={className ? `ds-field ${className}` : "ds-field"}>
    <span className="ds-field__label">{label}</span>
    <div className="ds-field__value">{children}</div>
  </div>;
}
