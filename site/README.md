# Toybox website

Run `pnpm install --frozen-lockfile` and `pnpm build` from this directory.

The homepage embeds `/demo/` in an iframe. That page renders the actual app's
`LibraryView`, design system, sidebar, viewers, settings, and plugin picker.
The iframe isolates app styling and dialogs from the marketing page. Do not
recreate app layouts in the demo.

`astro.config.mjs` substitutes browser adapters only at the native-service
boundary. The demo uses bundled fixtures and in-memory catalog changes;
operations requiring the local filesystem point visitors to the desktop app.
Importer manifests and icons come from the bundled plugins.

`scripts/generate-demo.mjs` generates small example assets and original demo
motion for the bundled Quaternius CC0 mannequin. `_library-samples.json` holds
curated ambientCG materials and Kenney sounds copied from the owner's library,
retaining their existing source, publisher, URL, and CC0 license metadata.
Material textures are resized to 512 pixels for the website. Character credits
and license are in `public/demo-assets/samples/models/`.

`_thumbnails.json` maps sample IDs to images produced by the real app renderer.
These avoid making visitors wait for initial rendering. Hover previews and
thumbnail regeneration after changing preferences still use the app renderer.

GitHub Pages deploys on relevant changes to `master`, on a published release,
or through the Deploy site workflow's manual action. Changes to shared app UI
therefore update the website without a separate demo-layout edit.
