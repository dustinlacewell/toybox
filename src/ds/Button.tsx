import type { ButtonHTMLAttributes } from "react";

type Variant = "default" | "primary" | "ghost";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
}

export function Button({ variant = "default", className = "", ...rest }: Props) {
  const variantClass = variant === "default" ? "" : `ds-button--${variant}`;
  return <button className={`ds-button ${variantClass} ${className}`} {...rest} />;
}
