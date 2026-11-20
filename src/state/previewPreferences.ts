import { create } from "zustand";
import { persist } from "zustand/middleware";

export type MaterialPreviewStyle = "sphere" | "cube";

/** Tile size at 100% zoom. */
export const CARD_SIZE_DEFAULT = 168;

/** UI preferences belong to the application, outside the controlled controls. */
export const usePreviewPreferences = create(persist<{
  materialStyle: MaterialPreviewStyle;
  cardSize: number;
  setCardSize: (size:number) => void;
  setMaterialStyle: (style: MaterialPreviewStyle) => void;
  /** The preview tray at the bottom of the library: one line, or open with the details. */
  trayOpen: boolean;
  setTrayOpen: (open: boolean) => void;
}>((set) => ({
  materialStyle: "sphere",
  cardSize: CARD_SIZE_DEFAULT,
  trayOpen: false,
  setTrayOpen: trayOpen => set({ trayOpen }),
  setCardSize: size => set({cardSize:Math.max(96,Math.min(384,size))}),
  setMaterialStyle: materialStyle => set({ materialStyle }),
}), {
  name: "toybox-preview-preferences",
  merge: (saved, current) => {
    const value=saved as {materialStyle?:unknown;cardSize?:unknown;trayOpen?:unknown}|undefined;
    return {...current,materialStyle:value?.materialStyle==="cube"?"cube":"sphere",cardSize:typeof value?.cardSize==="number"&&Number.isFinite(value.cardSize)?Math.max(96,Math.min(384,value.cardSize)):CARD_SIZE_DEFAULT,trayOpen:value?.trayOpen===true};
  },
}));
