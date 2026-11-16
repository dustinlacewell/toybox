# Independent animations and shared rigs

Model import extracts every usable transform clip into its own glTF and compact
binary buffer under `library/<pack>/animations/`. Empty takes are skipped.
Animation-only import performs the same extraction without adding model tiles.
A rescan migrates older embedded-clip entries automatically, with a source
manifest backup. Stable clip IDs, favourites, collections and tags survive.

Each animation contains its node hierarchy, authored rest transforms and sampled
tracks. It has no mesh, textures, skin weights, inverse-bind matrices or links
back to the model files. Removing the original model does not remove its imported
animations. Original model files are preserved, while model previews and exports
omit their embedded animations. Reimporting a model refreshes matching extracted
clip IDs; independently imported animations are not pruned when source takes
are removed. Renaming/reordering source clips may affect their index-based IDs.

The catalog has a rig registry. Model and animation bindings refer to registry
IDs and map canonical node paths to their own file-local node indices. Registry
identity uses names, parent relationships and rest matrices, including ancestor
coordinate frames. Skin names, node array order and TRS versus matrix encoding
do not affect identity. Equivalent profiles tolerate numerical differences of
1e-5. Skeletal rigs and transform-only node hierarchies are kept distinct.

Direct preview requires an unambiguous mapping for every animated node and its
ancestors, with matching parent relationships and rest frames. Extra unanimated
bones are allowed. Duplicate or missing names disable automatic matching;
topology alone never guesses which limb or finger to use. Different rest poses,
proportions or coordinate frames use rig-adapter retargeting when a supported
profile and neutral pose are available. See [Rig adapters](rig-adapters.md).

In the animation viewer, **Preview model** lists compatible models. **Skeleton
only** works without a model. In the model viewer, **Preview animation** lists
compatible independent clips; **Rest pose** clears playback. Choices are saved
as user metadata and do not change exports. Playback supports pause, seek, speed
and root motion controls. Rig hierarchies can be inspected in either viewer.
The live documents are checked again before preview composition, so external
edits cannot bypass cached compatibility.

Native animation exports contain one clip and its target hierarchy. The preview
model is never included. Saved exports continue to remember their type and plugin
settings. Import accepts glTF/GLB and FBX through the configured converter.
Morph-target clips and animation-pointer extensions need additional target
schemas; extraction currently reports them as unsupported rather than discarding
tracks or embedding a model. Object transform animations are supported separately
from skeletal animation.

A source entry may still declare an animation-bearing file:

```json
{"kind":"animation","id":"uid://hero-motions","pack":"characters","category":"animations","file":"Hero.gltf"}
```

Import replaces this entry with independent `kind: "animation"`,
`standalone: true` entries. Each extracted file records original source identity,
clip index/name and skeletal joint indices in `extras.toyboxAnimation`.
Models receive `animationsExtracted: true` after extraction. New imports replace
that marker and run extraction again. Level/size variants do not apply to clips.

Run the optional real-library check with `TOYBOX_TEST_CATALOG` set to the catalog
cache path:

```text
cargo test --lib independent_animation_library_and_cross_model_preview -- --ignored --nocapture
```

`TOYBOX_TEST_ARTIFACT_DIR` optionally saves a pure animation GLB and a compatible
cross-model preview fixture for browser verification.
## Hover previews

The model detail Animation tab uses a single-selection listbox. Each open model
has one parsed scene and texture set; clip changes bind animation tracks to its
existing nodes. Direct playback rechecks loaded rest matrices. Retargeting bakes
on a temporary skeleton and rebinds the resulting tracks, keeping the visible
model untouched while preparation runs. The eight most recent clips are cached
without retaining extra copies of model meshes or textures. Closing, rescanning,
changing rig adapters or editing the origin releases the session.

Full model and animation viewers share `ViewerSceneControls` and
`ViewerReferenceWidgets`: floor grid, origin axes, geometry center, bounds and
model/origin orbit target. Origin alignment remains model-only and starts
collapsed. Model, animation and material detail viewers share studio lighting,
image-based illumination and mouse navigation through `ViewerSceneEnvironment`.

Hovering an animation card loops its selected preview model in the card. Existing
rig-adapter mappings, remembered model choices and skeleton fallbacks also apply
here. Playback stays in place horizontally; jumps and limb motion remain intact.
The camera frames sampled motion bounds once and stays fixed through the loop.
Leaving the card returns to its thumbnail, with no selection or metadata changes.

Only one library hover preview renders at a time, at 30 fps. A short hover delay
avoids work while crossing cards. One WebGL canvas is reused, with a two-scene
cache; playback stops on exit, unmount or document hiding, and idle resources are
released after 15 seconds. Thumbnail and live preview lighting share a renderer
setup. Obsolete loads and retarget baking accept `AbortSignal` cancellation.

`PreviewTile.onPreviewHoverChange` is the design-system intent extension. Host
preview sources implement `HoverPreviewSource`: a key, revision values and a
cancellable `load` returning `LiveScene` (`subject`, `bounds`, `reset`, `tick`,
`dispose`). `LiveScenePreview` mounts that source; animation catalog translation
lives in `animationPreviewSource`. The generic controls do not import asset or
rig data. `loadAssetScene` now accepts structured `SceneLoadOptions`, shared by
the full viewer, still thumbnails and live playback.

Models and materials use the same hover host with `staticPreviewSource`. Dragging
rotates the model or material sphere around its geometry center. `DragSurface`
provides the generic gesture without selecting or opening the card after a drag.
These sources set `continuous: false`, so they redraw only on rotation or resize.
`LiveScene.prepare` receives the shared renderer for studio lighting, and optional
`rotate` receives pixel deltas. Material hover maps are capped at 1024 pixels;
full viewer textures retain their original resolution.

The top-right inspector keeps a live preview of the inspected item, using a
separate renderer so card hovers cannot displace it. It stops while the full
viewer is open. Left drag rotates; right drag vertically zooms (up moves closer,
down moves away), bounded to avoid losing the subject. The same mouse mapping
applies in full viewers, with middle drag for panning. Animated previews orbit
the camera so root-motion coordinates remain intact.

Settings → Material preview remembers Sphere or Cube locally and applies it to
live cards, the inspector and the full material viewer's initial shape. The
design system owns only gesture and segmented-control presentation; preference
storage, source selection, rendering and camera movement live in the app.
Changing the material style offers to rebuild material thumbnails in the
background. The scoped clear preserves other asset types' cached thumbnails;
new material thumbnails use the preferred shape, including after a restart.
