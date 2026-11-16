export function desktopAction() {
  if(typeof window!=="undefined")window.dispatchEvent(new Event("toybox-desktop-action"));
}
