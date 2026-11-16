# Plugin icons and compact library navigation

Plugins supply local artwork through their manifest:

```json
{
  "icon": "icons/model.svg",
  "assetKinds": ["model", "animation"],
  "assetIcons": {
    "model": "icons/model.svg",
    "animation": "icons/animation.svg"
  }
}
```

`assetIcons` overrides the shared plugin `icon` for a type. Icons are image files
inside the plugin directory, served by the existing plugin protocol. Include
them in the distributed plugin; bundled plugins include their icon directories
in Tauri resources. Missing or broken images use the host's type glyph.
For the current built-in types, importers supply navigation artwork; if multiple
importers declare an icon, plugin-ID order makes the provider deterministic.
This does not introduce another content type registry.

The library hides its sidebar at window widths of 1,000 pixels or less, showing
type and Favorites icon controls above the header instead. Maximize library,
beside the Toybox wordmark, enables the same layout at larger widths. Restore
sidebar returns the desktop layout; Show navigation opens a left drawer on
narrow windows. Sources, collections, saved exports and maintenance actions
remain available there. The layout mode does not change filtering or selection.

Design-system controls accept presentation glyphs and controlled visibility;
plugin lookup, type interpretation and responsive mode belong to the app.
