import { useEffect, useRef, useState } from "react";
export type ImageChannel = "all" | "r" | "g" | "b" | "a";
export type ImageScale = "fit" | "actual";

/** Generic image inspection; the caller controls the channel and scale. */
export function ImageViewport({ src, alt, channel = "all", scale = "fit" }: { src: string; alt: string; channel?: ImageChannel; scale?: ImageScale }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const [error, setError] = useState(false);
  useEffect(()=>{
    const element=viewport.current;
    if(scale!=="actual"||!element)return;
    const center=()=>{
      element.scrollLeft=Math.max(0,(element.scrollWidth-element.clientWidth)/2);
      element.scrollTop=Math.max(0,(element.scrollHeight-element.clientHeight)/2);
    };
    const observer=new ResizeObserver(center);
    observer.observe(element);if(canvas.current)observer.observe(canvas.current);
    center();return()=>observer.disconnect();
  },[scale,src,error]);
  useEffect(() => {
    let alive = true; setError(false);
    const image = new Image(); image.crossOrigin = "anonymous";
    image.onload = () => {
      if (!alive || !canvas.current) return;
      try {
        const target = canvas.current;
        target.width = image.naturalWidth; target.height = image.naturalHeight;
        const ctx = target.getContext("2d", { willReadFrequently: channel !== "all" });
        if (!ctx) throw new Error("Canvas unavailable");
        ctx.drawImage(image, 0, 0);
        if (channel !== "all") {
          const pixels = ctx.getImageData(0, 0, target.width, target.height);
          const component = { r: 0, g: 1, b: 2, a: 3 }[channel];
          for (let i = 0; i < pixels.data.length; i += 4) {
            const value = pixels.data[i + component];
            pixels.data[i] = value; pixels.data[i + 1] = value; pixels.data[i + 2] = value; pixels.data[i + 3] = 255;
          }
          ctx.putImageData(pixels, 0, 0);
        }
      } catch { setError(true); }
    };
    image.onerror = () => { if (alive) setError(true); }; image.src = src;
    return () => { alive = false; image.onload = null; image.onerror = null; };
  }, [src, channel]);
  return <div ref={viewport} className={`ds-image-viewport ds-image-viewport--${scale}`}>{error ? <span role="alert">Image unavailable</span> : <canvas ref={canvas} role="img" aria-label={alt} />}</div>;
}
