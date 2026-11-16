import type { ReactNode, MouseEvent } from "react";

/** A controlled visual item. Selection and activation are separate callbacks. */
export function PreviewTile({ title, subtitle, preview, active, selected, onPress, onOpen, onSelectedChange, actions, badge, previewAction, previewAspectRatio=1, onPreviewHoverChange }: {
  title: string; subtitle?: ReactNode; preview: ReactNode; active: boolean; selected: boolean;
  onPress: () => void; onOpen: () => void; onSelectedChange: (selected: boolean, shiftKey?: boolean) => void;
  actions?: ReactNode; badge?: ReactNode; previewAction?: ReactNode;
  previewAspectRatio?: number;
  /** Presentation intent only; the caller decides whether/how to animate. */
  onPreviewHoverChange?: (hovered:boolean)=>void;
}) {
  const press = (event: MouseEvent) => {
    if (event.shiftKey || event.ctrlKey || event.metaKey) onSelectedChange(!selected, event.shiftKey);
    else onPress();
  };
  return <article className={`ds-preview-tile${active ? " is-active" : ""}${selected ? " is-selected" : ""}`}
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
