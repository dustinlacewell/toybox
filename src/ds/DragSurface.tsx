import { useRef, type ReactNode, type PointerEvent } from "react";
import "./DragSurface.css";

/** Presentation gesture only: emits pixel deltas and suppresses drag clicks. */
export function DragSurface({children,onDrag,onSecondaryDrag}:{children:ReactNode;onDrag:(dx:number,dy:number)=>void;onSecondaryDrag?:(dx:number,dy:number)=>void}) {
  const gesture=useRef<{id:number;button:number;x:number;y:number;startX:number;startY:number;dragged:boolean}|undefined>(undefined);
  const suppressClick=useRef(false),lastDrag=useRef(-Infinity);
  const finish=(event:PointerEvent<HTMLSpanElement>,cancel=false)=>{
    const current=gesture.current;if(!current||current.id!==event.pointerId)return;
    gesture.current=undefined;suppressClick.current=!cancel&&current.dragged;
    if(current.dragged)lastDrag.current=performance.now();
    if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);
  };
  return <span className="ds-drag-surface" onDragStart={event=>event.preventDefault()}
    onPointerDown={event=>{
      if((event.button!==0&&!(event.button===2&&onSecondaryDrag))||event.pointerType==="touch"||event.ctrlKey||event.shiftKey||event.metaKey)return;
      gesture.current={id:event.pointerId,button:event.button,x:event.clientX,y:event.clientY,startX:event.clientX,startY:event.clientY,dragged:false};suppressClick.current=false;
      event.currentTarget.setPointerCapture(event.pointerId);event.currentTarget.closest("button")?.focus({preventScroll:true});event.preventDefault();
    }}
    onPointerMove={event=>{
      const current=gesture.current;if(!current||current.id!==event.pointerId)return;
      if(!current.dragged&&Math.hypot(event.clientX-current.startX,event.clientY-current.startY)<4)return;
      current.dragged=true;(current.button===2?onSecondaryDrag:onDrag)?.(event.clientX-current.x,event.clientY-current.y);current.x=event.clientX;current.y=event.clientY;
    }}
    onPointerUp={event=>finish(event)} onPointerCancel={event=>finish(event,true)} onLostPointerCapture={event=>finish(event,true)}
    onContextMenu={event=>{
      // A secondary drag ends in a context-menu event that belongs to the drag; a plain right-click is the owner's.
      if(!onSecondaryDrag)return;
      event.preventDefault();if(performance.now()-lastDrag.current<400)event.stopPropagation();
    }}
    onClickCapture={event=>{const suppress=suppressClick.current;suppressClick.current=false;if(suppress&&event.detail!==0){event.preventDefault();event.stopPropagation();}}}
    onDoubleClickCapture={event=>{if(performance.now()-lastDrag.current<400){event.preventDefault();event.stopPropagation();}}}>
    {children}
  </span>;
}
