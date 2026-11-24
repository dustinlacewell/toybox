/** The number arithmetic of `Slider`, kept apart so it can be tested on its own. */

/** Decimal places of `step` (0.25 → 2, 1 → 0, 1e-3 → 3). */
export function stepDecimals(step: number): number {
  if (!Number.isFinite(step) || step <= 0) return 0;
  const text = String(step);
  const exponent = /e-(\d+)$/.exec(text);
  if (exponent) return Number(exponent[1]);
  const dot = text.indexOf(".");
  return dot < 0 ? 0 : text.length - dot - 1;
}

/** `value` kept within `min`..`max` and on the grid of `step` counted from `min`; without a usable step only
 *  clamped. Rounded to the step's decimals, so 0.1 + 0.2 shows as 0.3. */
export function snapValue(value: number, min: number, max: number, step?: number): number {
  if (!Number.isFinite(value)) return min;
  const low = Math.min(min, max), high = Math.max(min, max);
  let next = Math.min(high, Math.max(low, value));
  if (step && step > 0) {
    const decimals = Math.max(stepDecimals(step), stepDecimals(low));
    next = Number((low + Math.round((next - low) / step) * step).toFixed(decimals));
    if (next > high) next = Number((next - step).toFixed(decimals));
  }
  return next;
}

/** The number typed into a slider's box (a comma is read as a decimal point), or null when it is not one. */
export function parseSliderText(text: string): number | null {
  const trimmed = text.trim().replace(",", ".");
  if (!trimmed) return null;
  const value = Number(trimmed);
  return Number.isFinite(value) ? value : null;
}

/** How a slider's box shows `value`: with the step's decimals, and no "-0". */
export function formatSliderValue(value: number, step?: number): string {
  const text = step && step > 0 ? value.toFixed(stepDecimals(step)) : String(Number(value.toPrecision(6)));
  return Number(text) === 0 ? text.replace(/^-/, "") : text;
}

/** Where a slider's handle gesture is: dragged (pointer down on the range) or not, and the value moved to and not
 *  yet committed. */
export interface SliderGesture {
  dragging: boolean;
  pending: number | null;
}

export const IDLE_GESTURE: SliderGesture = { dragging: false, pending: null };

/** What happens to a slider's range: the pointer goes down on it, its value changes, the pointer is released
 *  (anywhere: the range captures the pointer, and a release, cancel or lost capture all count), or it loses focus. */
export type SliderInput = { kind: "down" } | { kind: "change"; value: number } | { kind: "up" } | { kind: "blur" };

/** One step of a slider's gesture, and the value to commit then, if any. While dragging, changes are only drafted
 *  and the last one is committed when the pointer is released (or focus leaves). A change with no drag — a key
 *  press (arrows, Page Up/Down, Home, End) or assistive technology — is committed at once, so a held key commits
 *  each step it makes. */
export function sliderGesture(state: SliderGesture, input: SliderInput): { state: SliderGesture; commit: number | null } {
  switch (input.kind) {
    case "down": return { state: { dragging: true, pending: state.pending }, commit: null };
    case "change": return state.dragging ? { state: { dragging: true, pending: input.value }, commit: null } : { state: IDLE_GESTURE, commit: input.value };
    case "up":
    case "blur": return { state: IDLE_GESTURE, commit: state.pending };
  }
}
