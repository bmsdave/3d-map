import type { MapHandle } from "../sdk";

/** Convert PointerEvent/WheelEvent client coords to canvas pixel coords. */
export function canvasPoint(
  canvas: HTMLCanvasElement,
  event: PointerEvent | WheelEvent | { clientX: number; clientY: number },
): [number, number] {
  const rect = canvas.getBoundingClientRect();
  return [
    (event.clientX - rect.left) * canvas.width / rect.width,
    (event.clientY - rect.top) * canvas.height / rect.height,
  ];
}

/**
 * Wire pointer + wheel navigation to a map.
 * Returns a disposer that removes all listeners and releases capture.
 * Caller should invoke it on unmount to avoid leaks (`home.ts:380`).
 */
export function attachNavigation(
  canvas: HTMLCanvasElement,
  map: MapHandle,
  refresh: () => void,
): () => void {
  let dragging = false;
  const onDown = (event: PointerEvent) => {
    dragging = true;
    canvas.setPointerCapture(event.pointerId);
    map.pointerDown(...canvasPoint(canvas, event), event.timeStamp);
  };
  const onMove = (event: PointerEvent) => {
    if (!dragging) return;
    map.pointerMove(...canvasPoint(canvas, event), event.timeStamp);
    map.render();
    refresh();
  };
  const onUp = () => {
    dragging = false;
    map.pointerUp();
  };
  const onWheel = (event: WheelEvent) => {
    event.preventDefault();
    map.wheel(...canvasPoint(canvas, event), event.deltaY, event.ctrlKey);
    map.render();
    refresh();
  };
  canvas.addEventListener("pointerdown", onDown);
  canvas.addEventListener("pointermove", onMove);
  canvas.addEventListener("pointerup", onUp);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  return () => {
    canvas.removeEventListener("pointerdown", onDown);
    canvas.removeEventListener("pointermove", onMove);
    canvas.removeEventListener("pointerup", onUp);
    canvas.removeEventListener("wheel", onWheel);
  };
}
