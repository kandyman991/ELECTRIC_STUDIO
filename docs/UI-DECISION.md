# M1b: choosing the editor's UI technology

Both shells are thin layers over the same `ecad_editor::Session`: they forward input and paint
the `Frame` they get back. Tools, snapping, hit-testing, undo, pan/zoom and saving are shared
code, so the two behave identically. The choice is only about the window around the editor.

![The same editing session in both shells](images/m1b-egui-vs-tauri.png)

*Same scripted session in each: place -K2 and -S2, wire them (W4), drag -K1 (its wires reroute
live), switch to the Wire tool.*

## Try them

```sh
cargo run -p ecad-egui  -- ~/ElectricStudio/Sample    # native egui window
cargo run -p ecad-tauri -- ~/ElectricStudio/Sample    # Tauri window (needs WebKitGTK on Linux)
```

Without a folder argument each opens `~/ElectricStudio/Sample`, creating the sample project
there if needed. Both edit the same folder format, so you can open one project in each.

| Action | Input |
| --- | --- |
| Select, move | Click, drag (snaps to the 4 mm grid) |
| Rotate, delete | R, Delete |
| Place a symbol | Toolbar symbol button, then click |
| Draw a wire | W (or Wire), click a connection point, then another |
| Undo, redo, save | Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z), Ctrl+S |
| Pan, zoom, fit | Middle-drag, wheel, F |
| Rename | Select, type a tag in the right panel, Enter |

## Comparison

| | egui | Tauri |
| --- | --- | --- |
| How it draws | In-process, every frame from the Rust data | Page sent to a web view as JSON, drawn on a canvas |
| Data per page change | None (same process) | ~10 KB for the sample page; a dense page will be hundreds of KB per drag step, so it will need incremental or binary updates |
| Panels, forms, tables (properties, parts, reports) | Workable, plainer look, all in Rust | HTML/CSS: richer and faster to style; fits the HTML prototypes already made for HMI Studio |
| Code in a second language | None | HTML/CSS/JS (or TypeScript/React later) |
| Linux runtime needs | OpenGL | WebKitGTK (on Ubuntu desktop by default) |
| Windows runtime needs | OpenGL | WebView2 (built into Windows 10/11) |
| Same stack as HMI Studio | No | Yes (Tauri 2) |
| Cold build on CI | Smaller | Larger (WebKitGTK headers, more crates) |

## Recommendation

**Tauri**, with one condition. Most of the product's remaining screens are forms and tables:
properties, parts catalogue, reports, terminal plans, project structure. HTML does those
better and faster, and it matches HMI Studio's stack and the HTML prototypes you already
like. The cost is the canvas data path: before large pages, the page must move to the web view
incrementally (only what changed) or in a binary form. That's planned work for M1c.

Choose **egui** instead if you'd rather keep the whole product in Rust and value the canvas
being in-process over richer panels.
