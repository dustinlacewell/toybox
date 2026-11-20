import { useEffect, useState, type KeyboardEvent } from "react";

/**
 * The highlighted row of a list of `count` rows, moved by ArrowUp/ArrowDown (wrapping round) and set back to
 * `initial` whenever `reset` changes. An `initial` of -1 starts with no row highlighted. `arrows` handles an
 * arrow key and says whether it did.
 */
export function useActiveRow(count: number, reset: unknown, initial = 0) {
  const [active, setActive] = useState(initial);
  useEffect(() => setActive(initial), [reset, initial]);
  const rows = Math.max(count, 1);
  const arrows = (event: KeyboardEvent): boolean => {
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (!step) return false;
    event.preventDefault();
    setActive((at) => at < 0 ? (step > 0 ? 0 : rows - 1) : (at + step + rows) % rows);
    return true;
  };
  return { active, setActive, arrows };
}
