/**
 * Side column listing a material's maps in stored order: role, file base name,
 * and the channel letter when the map reads one channel. A "3D" entry on top
 * and the selected row both return to the 3D preview. `selected` is an index
 * into `maps` (several roles can share one file), or null for the 3D preview.
 */

import type { ReactNode } from "react";

import type { MapRole, MaterialMap } from "../../domain/material";

interface Props {
  maps: MaterialMap[];
  selected: number | null;
  onSelect: (index: number | null) => void;
}

const ROLE_LABEL: Record<MapRole, string> = {
  albedo: "Albedo",
  normal: "Normal",
  roughness: "Roughness",
  metallic: "Metallic",
  ao: "AO",
  height: "Height",
  emissive: "Emissive",
  opacity: "Opacity",
};

const baseName = (file: string) => file.split(/[\\/]/).pop() ?? file;

export function MapList({ maps, selected, onSelect }: Props) {
  return (
    <ul className="maplist">
      <MapRow active={selected === null} onClick={() => onSelect(null)}>
        <span className="maplist__role">3D</span>
      </MapRow>
      {maps.map((map, index) => (
        <MapRow
          key={index}
          active={selected === index}
          onClick={() => onSelect(selected === index ? null : index)}
        >
          <span className="maplist__role">{ROLE_LABEL[map.role]}</span>
          {map.channel && <span className="maplist__channel">{map.channel.toUpperCase()}</span>}
          <span className="maplist__file">{baseName(map.file)}</span>
        </MapRow>
      ))}
    </ul>
  );
}

interface RowProps {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}

function MapRow({ active, onClick, children }: RowProps) {
  return (
    <li>
      <button
        className={`maplist__row ${active ? "is-active" : ""}`}
        onClick={onClick}
        aria-pressed={active}
      >
        {children}
      </button>
    </li>
  );
}
