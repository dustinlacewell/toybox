/** The host-built components every plugin panel and asset viewer receives (`ctx.widgets`). */

import type { PanelWidgets } from "@ldlework/toybox-sdk";
import { AssetThumbnailWidget } from "./AssetThumbnailWidget";
import { MaterialPreviewWidget } from "./MaterialPreviewWidget";

export const PANEL_WIDGETS: PanelWidgets = { MaterialPreview: MaterialPreviewWidget, AssetThumbnail: AssetThumbnailWidget };
