import type { HTMLAttributes } from "react";
import "./Tag.css";

/** A presentation label; callers own its meaning and any filtering behavior. */
export function Tag({className="",...props}:HTMLAttributes<HTMLSpanElement>) {
  return <span className={`ds-tag ${className}`} {...props}/>;
}
