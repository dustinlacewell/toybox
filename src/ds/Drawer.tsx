import { useEffect, useRef, type ReactNode } from "react";

interface Props {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  side?: "left"|"right";
  label?: string;
}

/** Controlled overlay panel. Direction, content and visibility belong to its owner. */
export function Drawer({ open, onClose, children, side="right", label }: Props) {
  const panel=useRef<HTMLDivElement>(null);
  useEffect(()=>{if(!open)return;const previous=document.activeElement as HTMLElement|null;panel.current?.focus();return()=>{if(previous?.isConnected)previous.focus();};},[open]);
  if (!open) return null;
  return (
    <div
      className={`ds-drawer-scrim${side==="left"?" ds-drawer-scrim--left":""}`}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div ref={panel} tabIndex={-1} className="ds-drawer" role="dialog" aria-modal="true" aria-label={label} onKeyDown={event=>{
        if(event.key==="Escape"){event.stopPropagation();onClose();}
        if(event.key==="Tab"){
          const items=event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),a[href],[tabindex="0"]');
          const first=items[0],last=items[items.length-1];
          if(!first){event.preventDefault();return;}
          if(event.shiftKey&&(document.activeElement===first||document.activeElement===panel.current)){event.preventDefault();last.focus();}
          else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}
        }
      }}>{children}</div>
    </div>
  );
}
