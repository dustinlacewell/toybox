import type { InputHTMLAttributes } from "react";

export function TextInput({
  className = "",
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={`ds-input ${className}`} {...rest} />;
}
