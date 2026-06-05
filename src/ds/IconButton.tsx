import type { ButtonHTMLAttributes } from "react";
import type { LucideIcon } from "lucide-react";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: LucideIcon;
  /** Accessible label; also drives the native tooltip. */
  label: string;
  /** Segmented-toolbar on-state: paints the whole cell with the accent swatch. */
  active?: boolean;
}

/** Square, icon-only button. Pair inside an IconStrip for a segmented toolbar. */
export function IconButton({ icon: Icon, label, active = false, className = "", ...rest }: Props) {
  const activeClass = active ? "ds-icon-button--active" : "";
  return (
    <button
      className={`ds-icon-button ${activeClass} ${className}`}
      aria-label={label}
      title={label}
      {...rest}
    >
      <Icon size={16} strokeWidth={1.75} />
    </button>
  );
}
