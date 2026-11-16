import type { ButtonHTMLAttributes, ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: LucideIcon;
  /** Caller-supplied presentation glyph, for example an image. */
  glyph?: ReactNode;
  /** Accessible label; also drives the native tooltip. */
  label: string;
  /** Segmented-toolbar on-state: paints the whole cell with the accent swatch. */
  active?: boolean;
}

/** Square, icon-only button. Pair inside an IconStrip for a segmented toolbar. */
export function IconButton({ icon: Icon, glyph, label, active = false, className = "", ...rest }: Props) {
  const activeClass = active ? "ds-icon-button--active" : "";
  return (
    <button
      type="button"
      className={`ds-icon-button ${activeClass} ${className}`}
      aria-label={label}
      title={label}
      {...rest}
    >
      {glyph??(Icon&&<Icon size={16} strokeWidth={1.75} />)}
    </button>
  );
}
