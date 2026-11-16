import type { ReactNode } from "react";
import { NavigationItem } from "./Workbench";
import "./CategoryLayout.css";

export interface CategoryOption { id: string; label: string; icon?: ReactNode; }

/** Controlled category navigation beside an independently scrolling content panel. */
export function CategoryLayout({ options, value, onChange, label, children }: {
  options: readonly CategoryOption[]; value: string; onChange: (id: string) => void;
  label: string; children: ReactNode;
}) {
  return <div className="ds-category-layout">
    <nav className="ds-category-layout__navigation" aria-label={label}>
      {options.map(option => <NavigationItem key={option.id} label={<span className="ds-category-layout__label">{option.icon}{option.label}</span>} active={option.id === value} onPress={() => onChange(option.id)}/>)}
    </nav>
    <section className="ds-category-layout__content" aria-label={options.find(option => option.id === value)?.label}>{children}</section>
  </div>;
}
