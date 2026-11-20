/**
 * Where a floating menu opened at a point goes so it stays inside the viewport: at the point when it fits,
 * flipped to the other side of the point when it would cross an edge, and pinned at the margin when the
 * viewport is too small for it.
 */
export function placeMenu(
  point: { x: number; y: number },
  size: { width: number; height: number },
  viewport: { width: number; height: number },
  margin: number,
): { left: number; top: number } {
  return {
    left: placeAxis(point.x, size.width, viewport.width, margin),
    top: placeAxis(point.y, size.height, viewport.height, margin),
  };
}

function placeAxis(at: number, length: number, room: number, margin: number): number {
  const start = at + length > room - margin ? at - length : at;
  return Math.max(margin, Math.min(start, room - margin - length));
}
