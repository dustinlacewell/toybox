import type { ReactNode } from "react";

/** Groups IconButtons into one segmented control: shared borders, no gaps. */
export function IconStrip({ children }: { children: ReactNode }) {
  return <div className="ds-icon-strip">{children}</div>;
}
