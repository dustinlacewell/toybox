export const PREVIEW_BRIGHTNESS = { normal: 1, dim: .3, veryDim: .05 } as const;
export type PreviewBrightness = keyof typeof PREVIEW_BRIGHTNESS;
