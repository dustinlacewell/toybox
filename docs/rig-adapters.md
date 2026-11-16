# Humanoid rig adapters

Rig adapters contribute named skeleton recognition and semantic bone mappings.
The host owns hierarchy validation, rest-axis correction, proportion scaling,
motion sampling, resource disposal, playback and preview controls. No domain
logic is added to the design system.

Retargeting is for inspection. It does not perform foot-contact IK, retarget
facial morphs, or replace the authored animation on export.

The bundled `com.toybox.humanoid-rigs` adapter recognizes Synty Polygon,
Synty Sidekick, Synty Unreal-style model rigs, and Quaternius Humanoid. Profiles
use exact aliases, required torso/head/limb bones and semantic parent rules.
Missing or ambiguous required names and inconsistent topology prevent automatic
matching. This version supports humanoids; it does not guess arbitrary skeletons
or retarget morph targets.

```json
{
  "id": "example.humanoid-rigs",
  "name": "My humanoid rigs",
  "version": "1.0.0",
  "kind": "rigAdapter",
  "entry": "dist/index.js",
  "permissions": {}
}
```

The entry default-exports `defineRigAdapter({ profiles: [...] })` from
`@ldlework/toybox-sdk`. Each `RigProfile` declares `id`, `name`, `bones` (semantic
role to exact aliases), `required`, `parents`, and optional `referencePoseNames`.
See the bundled adapter for all humanoid roles and an executable example.
Malformed adapters are isolated as plugin load errors. An adapter receives no
filesystem or export context; the host consumes its mapping data.

For Synty animations, reference poses come from the user's imported
`A_TPose_Neut` and `A_MOD_BL_TPose_Neut` clips. These are not bundled. The solver
validates the reference family, aligns humanoid frames and bone directions,
corrects rest-axis offsets, preserves target limb lengths, and scales hips/root
travel by leg proportions. It bakes a temporary display clip at 30 samples per
second. Original animation tracks and exported files remain unchanged.

The bundled preview character is Quaternius's Superhero Male from Universal Base
Characters Standard, explicitly licensed CC0 in the downloaded archive. It is
packed as a 741 KB GLB with neutral materials and no embedded animations or
textures. License text and provenance are shipped under `public/preview/`.
Source: https://quaternius.itch.io/universal-base-characters

Recognized Synty animations with their neutral pose available default to this
mannequin, in both the viewer and generated thumbnails. The picker also offers
mapped models from the user's library, direct compatible models, and Skeleton
only. Preferences are remembered per animation. Direct playback and retargeted
previews are labeled separately. Missing profiles or reference poses keep the
skeleton fallback available. The bundled mannequin is a viewer resource, not a
library record or an extra exported model.

Existing animation thumbnails regenerate once for the new preview renderer;
the catalog and user metadata are preserved. Future renderer changes can bump
the animation preview version in `commands/catalog.rs`.

Thumbnail generation samples one retargeted pose at a quarter of the clip's
duration, rather than baking the entire playback clip. The mannequin geometry
and materials are shared between skeleton clones, and neutral poses are cached
per rig binding. Raw preview sources use canonical paths confined to the library
root and do not deserialize the catalog on each read. Export validation still
uses catalog records. Concurrent thumbnail invalidation tolerates an already
removed file; filesystem failures include the operation and affected path.

Direct compatible-model previews also avoid catalog I/O: suggestions use the
loaded named hierarchy, ancestry and rest-frame data; the native pair reader
then independently rechecks both current files before composing them. This
includes clips that have no humanoid adapter. Raw and paired source reads run
on native blocking-worker threads. The thumbnail queue prepares at most three
scenes concurrently and captures/saves in order with one WebGL renderer. Pause
drains and disposes unused scenes; regeneration waits for the previous queue
to release its resources.

The November 15 trace (`Trace-20261115T002900.json`) exposed the direct-model
path: `compatible_previews` and `read_animation_preview` each took about 1.67 s.
The regression benchmark uses the same SimpleMilitary Pilot Brown/White clips:
bounded paired reads, live matching and composition take about 3 ms in the
debug build. Browser-side suggestions match all 60 native choices for each
clip and take 8–10 ms with the full library loaded. An isolated browser fixture
generated 30 thumbnails in 2.9 s with embedded resources and simulated 3 ms
native responses; this measures the renderer/pipeline, not live disk throughput.
