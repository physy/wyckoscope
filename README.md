# Wyckoscope

A visual-first desktop viewer for CIF and VESTA crystal structures, written in Rust.

## Current features

- CIF 1.x and commensurate mCIF parsing, including magnetic moments and time-reversal-aware magnetic symmetry expansion
- Space-group expansion from explicit CIF operations or International Tables metadata
- VESTA structure and display import: `CELLP`, `STRUC`, `SYMOP`, `VECTR`/`VECTT`, `SBOND`, `BKGRC`, and `STYLE` vector scale
- PBR atom rendering with HDR tonemapping, shadows, and silhouette outlines
- Element-colored split bonds and unit-cell rendering
- VESTA-style unit-cell controls: hidden/single/all cells, solid/dotted/dashed lines, width, and color
- Arbitrary A × B × C supercells (with a 150,000 displayed-atom safety limit)
- Per-element visibility, color, and display-radius controls
- Clickable atom selection with coordination numbers, two-atom distances, and three-atom angles
- Semi-transparent coordination polyhedra generated from the selected atom's bonded neighbours
- VESTA-style A1/A2 bond rules with element or site-label matching, distance ranges, and all-site coordination polyhedra
- Magnetic-moment arrows in lattice-vector notation, attachable per site or per selected atom; site vectors transform as axial vectors under spatial symmetry
- Ball-and-stick, space-filling, and element-colored wireframe atom display modes
- Free trackball rotation, pan, zoom, crystallographic-axis presets, and a continuous strong-perspective-to-orthographic projection control
- A draggable, resizable inspector separated from the rendering viewport, plus a transparent lower-left 3D a/b/c indicator with configurable colors, label size, and font family
- White/dark/custom backgrounds and configurable ground color
- Detailed lighting controls: ambient light, Point/Spot key light, camera-following or fixed positioning with azimuth/elevation offsets, color/intensity/range/source radius, spot cone and target, shadow biases, directional fill light, independent axis lighting, exposure, and HDR tone mapping
- High-resolution PNG export up to 8192 × 8192
- Drag-and-drop and a non-blocking native file picker

## Run

```bash
cargo run --release
```

The first build takes a while because the GPU rendering stack is compiled from source.

### macOS app bundle

```bash
./scripts/package-macos.sh
open dist/Wyckoscope.app
```

## Controls

- Left drag: free rotation
- Click an atom: select it; Shift-click: add/remove selections (up to three)
- Middle/right drag: pan
- Mouse wheel: zoom
- `F`: reset view
- `O`: open file

The left inspector is split into Structure, View, Elements, and Export tabs.

## Source layout

- `main.rs`: application construction and Bevy system scheduling
- `state.rs`: shared resources, components, and settings
- `scene.rs`: scene setup, atoms, bonds, unit cells, and ground geometry
- `camera.rs`: orbit controls, projection blending, viewports, and orientation axes
- `lighting.rs`: ambient, key, fill, axis-light, exposure, and tone-mapping synchronization
- `file_io.rs`: drag-and-drop and non-blocking open/save dialogs
- `export.rs`: off-screen high-resolution PNG rendering
- `ui.rs`: inspector tabs and overlay labels

VESTA density/isosurface data is intentionally not imported yet.
