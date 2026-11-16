import { useEffect, useState, type ReactNode } from "react";
import { FolderOpen } from "lucide-react";
import { Button, IconButton, PageHeader, Pane, Workbench, Segmented } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import { sourceLabel } from "../domain/facets";
import type { Asset } from "../domain/catalog";
import { revealAsset } from "../services/reveal";
import { FavoriteStar } from "./FavoriteStar";
import { useFavorites } from "./useFavorites";
import { AssetProperties } from "./AssetProperties";
import "./ViewerFrame.css";

interface Props { asset: Asset; onClose: () => void; children?: ReactNode; sidebar?: ReactNode; sidebarLabel?:string; sidebarTabs?:{value:string;label:string;content:ReactNode}[]; detailsExtra?:ReactNode; headerActions?: ReactNode; footer?: ReactNode; onExport?: () => void; }
export function ViewerFrame({ asset, onClose, children, sidebar, sidebarLabel="Preview", sidebarTabs, detailsExtra, headerActions, footer, onExport }: Props) {
  const { toggleFavorite } = useFavorites();
  const [tab,setTab]=useState("details");
  const tabs=sidebarTabs??(sidebar?[{value:"preview",label:sidebarLabel,content:sidebar}]:[]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", key); return () => window.removeEventListener("keydown", key);
  }, [onClose]);
  return <div className="viewer" role="dialog" aria-modal="true" aria-label={asset.name}>
    <Workbench header={<PageHeader leading={<Button variant="ghost" onClick={onClose}>← Back</Button>} title={variantInfo(asset.name, asset.kind).base} metadata={sourceLabel(asset)}>
      {headerActions}<FavoriteStar favorited={asset.user.favorite} onToggle={() => toggleFavorite(asset.id)} /><IconButton icon={FolderOpen} label="Show source files" onClick={() => void revealAsset(asset)} />
    </PageHeader>} inspector={<Pane footer={onExport && <Button variant="primary" onClick={onExport}>Export</Button>}>
      <div className="viewer-panel-tabs"><Segmented label="Item properties" value={tab} options={[{value:"details",label:"Details"},...tabs.map(({value,label})=>({value,label}))]} onChange={setTab}/></div>
      {tab==="details"?<><AssetProperties asset={asset} showFileAction={false}/>{detailsExtra}</>:tabs.find(item=>item.value===tab)?.content}
    </Pane>}
      footer={footer && <div className="viewer__footer">{footer}</div>}>
      <div className="viewer__stage">{children}</div>
    </Workbench>
  </div>;
}
