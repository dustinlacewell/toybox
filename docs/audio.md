# Audio

Toybox stores sounds and music as independent `audio` assets. The Audio section
supports search, packs, categories, favourites, collections, selection and deletion.
Each tile shows the waveform, length and format. Open a tile to play/pause, scrub
the waveform (click or use its arrow/Home/End keys), restart, adjust volume or loop.
Closing the viewer stops playback. No sound plays automatically.
Every audio tile has a play/stop icon: one click previews it from the start,
without selecting it first or moving to the inspector. Click again to stop.
Tiles and the inspector share a single lazy player, so cueing another sound,
selecting a different item or opening the full player stops the previous audition.
Cueing preserves the current multi-selection. Audio files load only when cued.

The bundled **Audio folders** importer scans recursively for WAV, MP3, Ogg Vorbis
and FLAC. Choose the pack/category, listen to source files, select which ones to
bring in, and import. Source subfolders are preserved, so equal basenames in
different folders stay distinct. Stable IDs include the pack, category and source
relative path. An existing destination with different bytes is rejected rather
than overwritten. Originals are copied without conversion.

The native importer validates and decodes each file on a blocking worker. It
records duration, sample rate, channel count, optional PCM bit depth, byte size
and at most 512 amplitude peaks. Waveform accumulation is bounded while decoding,
including long tracks. Persisted metadata warms the decode cache on catalog load;
file size and modification time invalidate it when a source changes. Thumbnails
use those peaks and a 2D canvas, without decoding audio again or rendering a 3D
scene. Waveforms use one SVG path per layer rather than hundreds of DOM elements.

Use **Copy files** to export the original audio bytes. Saved exports and change
tracking work with audio. GLB and model-only exporters are excluded. Deleting
audio removes its registered file and cached thumbnail, keeping files referenced
by surviving assets or variants.

Manifest entries use the standard file layout:

```json
{"kind":"audio","id":"uid://my_sound","pack":"sound_pack","category":"effects","file":"doors/open.wav"}
```

The file lives at `library/sound_pack/effects/doors/open.wav`. Native indexing
populates `asset.audio`: `file`, `format`, `duration`, `sampleRate`, `channels`,
`bitDepth`, `byteLength`, `modifiedNs`, `peaks`. These facts are also available to
plugins as `AudioAssetView`. Import plugins declare `assetKinds: ["audio"]` and
commit `AudioSeedEntry` values. The SDK's `/ui` entry provides controlled
`Waveform` and `AudioTransport` controls; neither imports catalog or filesystem
code. Playback support follows the embedded browser; unsupported encodings show
an error and remain exportable in their original format.
