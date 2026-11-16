import { create } from "zustand";
import { persist } from "zustand/middleware";

export type MaterialPreviewStyle = "sphere" | "cube";

/** UI preferences belong to the application, outside the controlled controls. */
export const usePreviewPreferences = create(persist<{
  materialStyle: MaterialPreviewStyle;
  cardSize: number;
  setCardSize: (size:number) => void;
  setMaterialStyle: (style: MaterialPreviewStyle) => void;
}>((set) => ({
  materialStyle: "sphere",
  cardSize: 168,
  setCardSize: size => set({cardSize:Math.max(96,Math.min(384,size))}),
  setMaterialStyle: materialStyle => set({ materialStyle }),
}), {
  name: "toybox-preview-preferences",
  merge: (saved, current) => {
    const value=saved as {materialStyle?:unknown;cardSize?:unknown}|undefined;
    return {...current,materialStyle:value?.materialStyle==="cube"?"cube":"sphere",cardSize:typeof value?.cardSize==="number"&&Number.isFinite(value.cardSize)?Math.max(96,Math.min(384,value.cardSize)):168};
  },
}));
