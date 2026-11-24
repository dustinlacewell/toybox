import { useEffect, useId, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { ReactNode } from "react";
import { formatSliderValue, IDLE_GESTURE, parseSliderText, sliderGesture, snapValue, type SliderInput } from "./sliderValue";

interface Props {
  label: ReactNode;
  value: number;
  min: number;
  max: number;
  /** Granularity of the value. Defaults to 1. */
  step?: number;
  /** Every change, continuously while the handle is dragged. */
  onChange: (value: number) => void;
  /** A finished change: the handle let go (wherever the pointer is then), a key press, or a number typed and
   *  confirmed (Enter or leaving the box). */
  onCommit?: (value: number) => void;
  disabled?: boolean;
  /** Shown after the label, e.g. a "changed" mark or a reset button. */
  extra?: ReactNode;
}

/** A labelled range with a number box beside it. Controlled: it shows `value` and emits the next one. */
export function Slider({ label, value, min, max, step = 1, onChange, onCommit, disabled = false, extra }: Props) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  // The handle's gesture (see `sliderGesture`); the owner may not have re-rendered with a drafted value yet.
  const gesture = useRef(IDLE_GESTURE);
  const commitRef = useRef(onCommit);
  commitRef.current = onCommit;
  /** Stops listening for a release outside the range, during a drag. */
  const unlisten = useRef<(() => void) | null>(null);
  const feed = (input: SliderInput) => {
    const next = sliderGesture(gesture.current, input);
    gesture.current = next.state;
    if (!next.state.dragging) { unlisten.current?.(); unlisten.current = null; }
    if (next.commit !== null) commitRef.current?.(next.commit);
  };
  useEffect(() => () => { unlisten.current?.(); }, []);

  const down = (event: PointerEvent<HTMLInputElement>) => {
    // Keep receiving the pointer's events when it leaves the range, so a release anywhere ends the drag.
    try { event.currentTarget.setPointerCapture(event.pointerId); } catch { /* not capturable: the window listener below still sees the release */ }
    feed({ kind: "down" });
    if (!unlisten.current) {
      const up = () => feed({ kind: "up" });
      window.addEventListener("pointerup", up, true);
      window.addEventListener("pointercancel", up, true);
      unlisten.current = () => { window.removeEventListener("pointerup", up, true); window.removeEventListener("pointercancel", up, true); };
    }
  };
  const moved = (next: number) => {
    const snapped = snapValue(next, min, max, step);
    if (snapped === value && gesture.current.pending === null && !gesture.current.dragging) return;
    if (snapped !== value) onChange(snapped);
    feed({ kind: "change", value: snapped });
  };
  const emit = (next: number) => {
    const snapped = snapValue(next, min, max, step);
    if (snapped === value) return;
    onChange(snapped);
    onCommit?.(snapped);
  };
  const confirm = () => {
    if (draft === null) return;
    const typed = parseSliderText(draft);
    setDraft(null);
    if (typed !== null) emit(typed);
  };
  const boxKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") { event.preventDefault(); confirm(); }
    else if (event.key === "Escape" && draft !== null) { event.preventDefault(); event.stopPropagation(); setDraft(null); }
    else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      event.preventDefault();
      const from = (draft !== null ? parseSliderText(draft) : null) ?? value;
      setDraft(null);
      emit(from + (event.key === "ArrowUp" ? 1 : -1) * step * (event.shiftKey ? 10 : 1));
    }
  };
  return <div className={`ds-slider${disabled ? " is-disabled" : ""}`}>
    <div className="ds-slider__head">
      <label className="ds-slider__label" id={`${id}-label`} htmlFor={`${id}-range`}>{label}</label>
      {extra}
    </div>
    <div className="ds-slider__controls">
      <input id={`${id}-range`} className="ds-slider__range" type="range" min={min} max={max} step={step} value={value} disabled={disabled}
        onChange={(event) => moved(Number(event.currentTarget.value))}
        onPointerDown={down}
        onPointerUp={() => feed({ kind: "up" })} onPointerCancel={() => feed({ kind: "up" })} onLostPointerCapture={() => feed({ kind: "up" })}
        onBlur={() => feed({ kind: "blur" })} />
      <input className="ds-input ds-slider__value" type="text" inputMode="decimal" aria-labelledby={`${id}-label`} disabled={disabled}
        value={draft ?? formatSliderValue(value, step)}
        onChange={(event) => setDraft(event.currentTarget.value)}
        onFocus={(event) => event.currentTarget.select()}
        onBlur={confirm} onKeyDown={boxKey} />
    </div>
  </div>;
}
