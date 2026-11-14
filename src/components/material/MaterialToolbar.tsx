/**
 * Floating toolbar for the material preview: a shape switch (sphere / plane /
 * cube) and a tiling picker (texture repeat).
 */

import { Box, Circle, Square, type LucideIcon } from "lucide-react";

import type { PreviewShape } from "../../services/materialPreview";
import { IconButton, IconStrip, Select } from "@ldlework/toybox-sdk/ui";

export const TILING_VALUES = [1, 2, 4, 8] as const;
export type Tiling = (typeof TILING_VALUES)[number];

const SHAPES: { shape: PreviewShape; label: string; icon: LucideIcon }[] = [
  { shape: "sphere", label: "Sphere", icon: Circle },
  { shape: "plane", label: "Plane", icon: Square },
  { shape: "cube", label: "Cube", icon: Box },
];

const TILING_OPTIONS = TILING_VALUES.map((n) => ({ value: String(n), label: String(n) }));

interface Props {
  shape: PreviewShape;
  onShape: (shape: PreviewShape) => void;
  tiling: Tiling;
  onTiling: (tiling: Tiling) => void;
}

export function MaterialToolbar({ shape, onShape, tiling, onTiling }: Props) {
  return (
    <div className="mtoolbar">
      <IconStrip>
        {SHAPES.map(({ shape: key, label, icon }) => (
          <IconButton
            key={key}
            icon={icon}
            label={label}
            active={shape === key}
            onClick={() => onShape(key)}
          />
        ))}
      </IconStrip>
      <label className="mtoolbar__field">
        <span className="mtoolbar__label">Tiling</span>
        <Select
          value={String(tiling)}
          options={TILING_OPTIONS}
          onChange={(value) => onTiling(Number(value) as Tiling)}
        />
      </label>
    </div>
  );
}
