import type { ReactNode, MouseEvent } from "react";

/** A controlled visual item. Selection and activation are separate callbacks. */
export function PreviewTile({ title, subtitle, preview, active, selected, onPress, onOpen, onSelectedChange, actions, badge, previewAction, previewAspectRatio=1, onPreviewHoverChange, onContextMenu }: {
  title: string; subtitle?: ReactNode; preview: ReactNode; active: boolean; selected: boolean;
  onPress: () => void; onOpen: () => void; onSelectedChange: (selected: boolean, shiftKey?: boolean) => void;
  actions?: ReactNode; badge?: ReactNode; previewAction?: ReactNode;
  previewAspectRatio?: number;
  /** Presentation intent only; the caller decides whether/how to animate. */
  onPreviewHoverChange?: (hovered:boolean)=>void;
  /** The point a context menu for this tile opens at; the browser's own menu does not show. */
  onContextMenu?: (point: { x: number; y: number }) => void;
}) {
  const press = (event: MouseEvent) => {
    if (event.shiftKey || event.ctrlKey || event.metaKey) onSelectedChange(!selected, event.shiftKey);
    else onPress();
  };
  return <article className={`ds-preview-tile${active ? " is-active" : ""}${selected ? " is-selected" : ""}`}
    onContextMenu={onContextMenu && ((event) => { event.preventDefault(); onContextMenu(menuPoint(event)); })}
    onPointerEnter={event=>{if(event.pointerType!=="touch")onPreviewHoverChange?.(true);}}
    onPointerLeave={()=>onPreviewHoverChange?.(false)}>
    <div className="ds-preview-tile__image" style={{aspectRatio:previewAspectRatio}}>
      <button type="button" className="ds-preview-tile__open" onClick={press} onDoubleClick={onOpen} title={title} aria-label={title}>{preview}</button>
      <label className="ds-preview-tile__check"><input type="checkbox" checked={selected} onChange={(e) => onSelectedChange(e.currentTarget.checked, (e.nativeEvent as globalThis.MouseEvent | undefined)?.shiftKey)} aria-label={`Select ${title}`} /></label>
      {badge && <span className="ds-preview-tile__badge">{badge}</span>}
      {previewAction && <div className="ds-preview-tile__preview-action">{previewAction}</div>}
      {actions && <div className="ds-preview-tile__actions">{actions}</div>}
    </div>
    <button type="button" className="ds-preview-tile__caption" onClick={press} onDoubleClick={onOpen}>
      <span title={title}>{title}</span>{subtitle && <small>{subtitle}</small>}
    </button>
  </article>;
}

/** The pointer, or the tile's centre when the menu was asked for from the keyboard (no right button, no point). */
function menuPoint(event: MouseEvent<HTMLElement>): { x: number; y: number } {
  if (event.button === 2 && (event.clientX || event.clientY)) return { x: event.clientX, y: event.clientY };
  const rect = event.currentTarget.getBoundingClientRect();
  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
}

export interface ThumbnailOption { value: string; label: string; preview: ReactNode; detail?: ReactNode; }

export function ThumbnailStrip({ options, value, onChange, label }: {
  options: ThumbnailOption[]; value: string; onChange: (value: string) => void; label: string;
}) {
  return <div className="ds-thumbnail-strip" role="group" aria-label={label}>
    {options.map((option) => <button type="button" key={option.value} onClick={() => onChange(option.value)} aria-pressed={option.value === value}>
      <span className="ds-thumbnail-strip__preview">{option.preview}</span><span>{option.label}</span>{option.detail && <small>{option.detail}</small>}
    </button>)}
  </div>;
}
