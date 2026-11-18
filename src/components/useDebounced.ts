import { useEffect, useState } from "react";

/** `value`, a moment after it stops changing. The first value is returned at once. */
export function useDebounced<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    if (Object.is(settled, value)) return;
    const timer = window.setTimeout(() => setSettled(value), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms, settled]);
  return settled;
}
