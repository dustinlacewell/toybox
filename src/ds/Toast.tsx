import type { ReactNode } from "react";
import "./Toast.css";

/** Non-modal transient status. The caller owns its content and lifetime. */
export function Toast({label,children,position="bottom-right"}:{label:string;children:ReactNode;position?:"bottom-right"|"top-right"}) {
  return <section className="ds-toast" data-position={position} role="status" aria-label={label}>{children}</section>;
}
