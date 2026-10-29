# Space Analyzer

A desktop app that scans a folder and shows what's taking up space as a nested treemap. Each folder is a box sized by how much it holds, with its subfolders drawn inside it. Click any box to open that folder.

Written in Rust with [egui](https://github.com/emilk/egui).

## Build and run

You need a Rust toolchain (edition 2024, so Rust 1.85 or newer).

```sh
cargo run --release                 # opens a welcome screen: pick a folder or scan your home folder
cargo run --release -- /some/path   # scans that path straight away
```

Use `--release` for anything large. The debug build works but scans much more slowly.

## Using it

| Action | What it does |
| --- | --- |
| Click a box | Opens that folder in Finder (or your system's file manager) |
| ⌘-click / Ctrl-click a box | Zooms into that folder |
| Right-click a box | Menu: open, zoom in, or copy the path |
| Backspace or Esc | Goes up one level |
| Breadcrumbs / ⬆ Up | Jump back to any parent folder |
| Hover | Shows the full path and size |

The loose files directly inside a folder are grouped into a single "N files" box. Clicking it opens the folder that holds them.

## How sizes are counted

- **Disk usage, not file length.** Sizes are the space actually allocated on disk, so sparse and compressed files count for less than their nominal size.
- **One disk only.** The scan doesn't cross into other mounted volumes.
- **No double counting on macOS.** When scanning `/`, the `/System/Volumes/Data` mirror is skipped, since its contents are already reached through `/Users`, `/Applications` and the other firmlinks.
- **Symlinks aren't followed.** Hard-linked files are counted once per link.
- **Protected folders.** macOS hides some locations (Mail, Safari, parts of `~/Library`) unless the app running the scan has Full Disk Access. Grant it to your terminal in System Settings → Privacy & Security to include them.

## Code layout

- `src/scan.rs`: parallel filesystem scan (using rayon) into a flat tree of folders
- `src/treemap.rs`: squarified treemap layout
- `src/main.rs`: the window, drawing and controls

## License

MIT. See [LICENSE](LICENSE).
