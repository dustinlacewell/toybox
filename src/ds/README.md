# Design-system boundary

This directory owns visual tokens, layout slots and generic interaction controls.
It imports only React, React DOM, generic icons, and other files in this directory.
It does not import Toybox's catalog, store, services, SDK domain contracts, or plugins.

Controls receive their value and emit the next value through a callback. They do
not persist selections or fetch application data. Internal state is limited to
presentation resources, such as loading an image or maintaining an element ref.

- `Workbench`, `PageHeader`, and `Pane` arrange caller-supplied content.
- `NavigationItem` receives its label, active state, and action.
- `PreviewTile` receives its title, preview, active/selected states, and callbacks.
  Its optional `onPreviewHoverChange` emits pointer intent without loading or
  owning playback. Generic preview action and aspect-ratio props remain separate
  from application asset types.
  Its optional `onContextMenu` emits the point a context menu opens at (the tile's
  centre when opened from the keyboard) and suppresses the browser's own menu.
- `ContextMenu` receives a point, heading/separator/item entries, and pick and close
  callbacks. It places itself inside the viewport, owns the keyboard and the active
  row, and asks to close on an outside press, scroll, resize or window blur.
- `SelectionBar` receives a label, generic actions (id, label, icon, tone, primary,
  disabled with a reason), and action and clear callbacks. It floats over the bottom
  of its positioned parent (`--selection-bar-bottom` moves it up) and shows icons only
  when the parent is narrow. Its owner decides when it shows and what each action does.
- `ThumbnailStrip` receives generic options, a value, and `onChange`.
- `PropertyRow` receives a label, value, and optional action.
- `Field` receives a label and shows its children as the value under it.
- `Chip` receives a label, a tone, and optional press and remove callbacks. The press
  callback gets the mouse event, so its owner can read modifier keys.
- `SuggestionList` receives rows, the active row, and pick and hover callbacks. It
  keeps the focus in its owner's text field. `useActiveRow` holds the active row
  and moves it with the arrow keys.
- `Segmented`, `Select`, `Checkbox`, and `TextInput` are controlled inputs.
- `Slider` is a controlled labelled range with a number box. It emits `onChange`
  continuously while dragging and `onCommit` once a change is finished.
- `ConfirmationDialog` receives generic rows, copy, busy/error state and callbacks.
  It arranges the confirmation and destructive action without performing it.
- `Waveform` receives amplitude peaks, duration, position and a seek callback.
- `AudioTransport` receives playback values and emits play, seek, volume and loop
  intent. Its owner loads media and manages the audio element.
- `ImageViewport` receives a URL, channel, and scale. It knows nothing about maps,
  materials, or source files.
- `DragSurface` emits pixel deltas after a small drag threshold and suppresses
  clicks caused by dragging. Its owner decides what moves; modifier clicks pass
  through to the surrounding selection control. Optional `onSecondaryDrag`
  emits right-button deltas and suppresses the browser context menu.

Application components in `src/components` and plugin panels translate their own
domain data into these props. They own state, file resolution, import/export
operations, and persistence. Plugins share the same primitives through
`@ldlework/toybox-sdk/ui`; SDK asset contracts remain a separate entry point.

`tests/design-system.test.mjs` checks imports and the controlled selection contract.
