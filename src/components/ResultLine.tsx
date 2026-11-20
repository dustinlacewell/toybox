/**
 * The line between the query and the grid: how many items show and what is left out, the order of the list, and
 * the size of the tiles (the same size Ctrl+wheel changes in the grid).
 */

import { useEffect, useRef, useState } from "react";
import { ArrowDownUp, Check, ChevronDown, Minus, Plus } from "lucide-react";

import type { SortOrder } from "../domain/facets";
import { CARD_SIZE_DEFAULT, usePreviewPreferences } from "../state/previewPreferences";
import "./ResultLine.css";

export interface SortChoice {
  key: string;
  label: string;
  /** Words for each direction: [ascending, descending]. */
  directions: [string, string];
  /** The direction a key starts in when chosen. */
  descending?: boolean;
  /** Section it is listed under (a type's own properties). */
  group?: string;
}

export const COMMON_SORTS: SortChoice[] = [
  { key: "name", label: "Name", directions: ["A → Z", "Z → A"] },
  { key: "added", label: "Date added", directions: ["Oldest first", "Newest first"], descending: true },
  { key: "changed", label: "Recently changed", directions: ["Longest ago first", "Latest first"], descending: true },
  { key: "publisher", label: "Publisher", directions: ["A → Z", "Z → A"] },
  { key: "source", label: "Source", directions: ["A → Z", "Z → A"] },
  { key: "format", label: "Format", directions: ["A → Z", "Z → A"] },
];

/** Zoom steps of the tile size, in percent of the default. */
const STEPS = [60, 75, 90, 100, 125, 150, 175, 200, 225];

interface Props {
  summary: string;
  /** What exclusions remove, when any are set; with a way to drop them all. */
  leftOut?: string;
  onKeepAll?: () => void;
  sort: SortOrder;
  choices: SortChoice[];
  onSort: (sort: SortOrder) => void;
}

export function ResultLine({ summary, leftOut, onKeepAll, sort, choices, onSort }: Props) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (event: MouseEvent) => { if (!box.current?.contains(event.target as Node)) setOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("mousedown", away); document.removeEventListener("keydown", escape); };
  }, [open]);
  const current = choices.find((choice) => choice.key === sort.key) ?? choices[0];
  const groups = [...new Set(choices.map((choice) => choice.group ?? ""))];

  const cardSize = usePreviewPreferences((state) => state.cardSize);
  const setCardSize = usePreviewPreferences((state) => state.setCardSize);
  const percent = Math.round((cardSize / CARD_SIZE_DEFAULT) * 100);
  const step = (direction: 1 | -1) => {
    const next = direction > 0 ? STEPS.find((value) => value > percent) : [...STEPS].reverse().find((value) => value < percent);
    if (next) setCardSize(Math.round((CARD_SIZE_DEFAULT * next) / 100));
  };

  return <div className="result-line">
    <span className="result-line__summary">{summary}</span>
    {leftOut && <><span className="result-line__sep" /><span>{leftOut}</span>{onKeepAll && <button type="button" className="result-line__link" onClick={onKeepAll}>Keep all</button>}</>}
    <span className="result-line__spacer" />
    <span className="result-line__sort" ref={box}>
      <button type="button" className={`result-line__button${open ? " is-open" : ""}`} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <ArrowDownUp size={13} aria-hidden="true" />Sort <b>{current.label}</b><span className="result-line__direction">{current.directions[sort.descending ? 1 : 0]}</span><ChevronDown size={13} aria-hidden="true" />
      </button>
      {open && <div className="sort-menu" role="menu">
        {groups.map((group) => <div key={group}>
          <div className="sort-menu__label">{group || "Sort by"}</div>
          {choices.filter((choice) => (choice.group ?? "") === group).map((choice) => <button type="button" role="menuitemradio" aria-checked={choice.key === current.key} key={choice.key} className="sort-menu__item"
            onClick={() => onSort({ key: choice.key, descending: choice.key === current.key ? sort.descending : !!choice.descending })}>
            <span className="sort-menu__check">{choice.key === current.key && <Check size={13} />}</span>{choice.label}
          </button>)}
        </div>)}
        <div className="sort-menu__directions" role="radiogroup" aria-label="Direction">
          {current.directions.map((words, index) => <button type="button" role="radio" key={words} aria-checked={sort.descending === (index === 1)} onClick={() => onSort({ key: current.key, descending: index === 1 })}>{words}</button>)}
        </div>
      </div>}
    </span>
    <span className="zoom" role="group" aria-label="Tile size">
      <button type="button" aria-label="Smaller tiles" disabled={percent <= STEPS[0]} onClick={() => step(-1)}><Minus size={13} /></button>
      <button type="button" className="zoom__value" title="Back to 100%" onClick={() => setCardSize(CARD_SIZE_DEFAULT)}>{percent}%</button>
      <button type="button" aria-label="Larger tiles" disabled={percent >= STEPS[STEPS.length - 1]} onClick={() => step(1)}><Plus size={13} /></button>
    </span>
  </div>;
}
