# Importing Unity-packaged Synty animation sets

The source RARs contain `.unitypackage` archives. Extract the RARs to a staging
drive, preserving the originals. The import helper streams Unity's gzip tar
once and reads assets by GUID; it does not run packaged scripts or import demos.

```text
python scripts/import-synty-animations.py <unitypackage> <pack-slug> <stage-root> <FBX2glTF.exe>
```

The helper requires PyYAML. It converts only FBXs under `Animations/`, retaining
Polygon/Sidekick and masculine/feminine versions. Unity subclips become separate
named clips, with exact cut boundaries resampled using linear interpolation or
quaternion slerp. Static T-pose and death-pose assets become held pose clips.
Source GUIDs, asset paths, hashes and Unity clip settings remain in provenance.
If Unity's requested range exceeds the FBX take, only available frames are used
and the import report records the clamping. Git LFS pointers are reported as
missing data; no substitute animation is fabricated.

Combine the generated `<pack>-entries.json` arrays into the staging root's
`_library_config/catalog.json` as `{ "schemaVersion": 1, "assets": [...] }`.
Run the native `reindex` utility against that staging library and a temporary
cache path. This extracts pure standalone clips through the same code as normal
Toybox imports. Then publish the two prepared sets:

```text
python scripts/import-synty-animations.py --publish <stage-root> <live-library-root>
```

Publishing validates files, preserves existing entries, refuses conflicting
files, backs up the live manifest, and registers the clips under category
folders in the two packs. Run `reindex` on the live library and its real catalog
cache afterward to refresh rig compatibility while preserving user metadata.
The live library needs only the compact extracted clips; staging FBXs are not
runtime dependencies.

Run the cut/rotation/pose checks with:

```text
python scripts/test_synty_animation_import.py
```

The 2026-11-14 import registered 823 Base Locomotion clips and 176 Sword Combat
clips. Four additional declared sword clips were unavailable: the archive's
`A_Attack_HeavyCombo01B_RootMotion_Sword.fbx` is a Git LFS pointer, not FBX data.
Twelve declared ranges extended beyond their actual take and were limited to
available frames. Details are in the library's
`_library_config/synty-animation-import-report.json` and each clip's extras.
