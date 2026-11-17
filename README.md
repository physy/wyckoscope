# Wyckoscope

A desktop crystal-structure viewer written in Rust.

Wyckoscope opens CIF, mCIF, and VESTA files and provides an interactive 3D view for atoms, bonds, polyhedra, vectors, and crystallographic axes.

## Features

- CIF / mCIF / VESTA import
- Space-group and magnetic-symmetry expansion
- PBR atom rendering, configurable lighting, backgrounds, and projection
- Bonds, coordination polyhedra, vectors, supercells, and unit cells
- Structure report with crystallographic and magnetic space-group metadata
- High-resolution PNG export
- Project save/load and undo/redo

## Run

```bash
cargo run --release
```

Open a structure with **File → Open structure** or drag a file onto the window.

## Controls

- Drag: rotate
- Shift-drag or right-drag: pan
- Scroll: zoom
- Click an atom: select it
- Shift-click: add or remove selected atoms

## Development

```bash
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

VESTA density and isosurface data are not imported.
