import type { CSSProperties, ReactNode } from "react";

interface Props {
  dir?: "row" | "col";
  gap?: number;
  align?: CSSProperties["alignItems"];
  justify?: CSSProperties["justifyContent"];
  grow?: boolean;
  wrap?: boolean;
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}

export function Stack({
  dir = "col",
  gap = 0,
  align,
  justify,
  grow,
  wrap,
  className = "",
  style,
  children,
}: Props) {
  return (
    <div
      className={`ds-stack ds-stack--${dir} ${className}`}
      style={{
        gap,
        alignItems: align,
        justifyContent: justify,
        flex: grow ? 1 : undefined,
        flexWrap: wrap ? "wrap" : undefined,
        ...style,
      }}
    >
      {children}
    </div>
  );
}
