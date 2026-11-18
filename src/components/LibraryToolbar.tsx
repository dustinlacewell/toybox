import { Button, IconButton, PageHeader, TextInput } from "@ldlework/toybox-sdk/ui";
import { PanelRight } from "lucide-react";
import { KindSwitch } from "./KindSwitch";

interface Props {
  title?: string; searchText: string; onSearchChange: (text: string) => void;
  visibleCount: number; visibleApproximate?: boolean; totalCount: number; selectedCount: number;
  onClearSelection: () => void; onExport: () => void; onImport?: () => void;
  filtersOpen?: boolean; onToggleFilters?: () => void;
  propertiesOpen?: boolean; onToggleProperties?: () => void;
}
export function LibraryToolbar({ title, searchText, onSearchChange, visibleCount, visibleApproximate, totalCount, selectedCount, onExport, onImport, filtersOpen, onToggleFilters, propertiesOpen=false, onToggleProperties }: Props) {
  return <PageHeader title={title ?? <KindSwitch />} metadata={visibleCount === totalCount && !visibleApproximate ? totalCount.toLocaleString() : `${visibleCount.toLocaleString()}${visibleApproximate ? "+" : ""} of ${totalCount.toLocaleString()}`}>
    <TextInput aria-label={`Search ${title?.toLowerCase() ?? "assets"}`} placeholder={`Search ${title?.toLowerCase() ?? "assets"}`} value={searchText} onChange={(e) => onSearchChange(e.currentTarget.value)} style={{ width: 260, maxWidth: "35%" }} />
    {onToggleFilters && <Button variant="ghost" aria-expanded={filtersOpen} onClick={onToggleFilters}>Filter</Button>}
    {onImport && <Button variant="primary" onClick={onImport}>Import</Button>}
    {onToggleProperties&&<IconButton icon={PanelRight} label={propertiesOpen?"Hide properties":"Show properties"} active={propertiesOpen} aria-expanded={propertiesOpen} onClick={onToggleProperties}/>}
    {!title && <Button variant="primary" disabled={!selectedCount} onClick={onExport}>Export</Button>}
  </PageHeader>;
}
