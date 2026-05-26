# Noted

A minimal, themable Markdown notes app for macOS, built in Rust with
[egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe).

Three built-in themes:

- **CandyCane** — clean white with red serif accents
- **Win95** — pure retro nostalgia
- **Dracula** — dark, classic developer palette

## Install (macOS)

Grab the latest `Noted-x.y.z.dmg` from
[Releases](https://github.com/danbednarski/noted/releases), open it,
and drag **Noted.app** into **Applications**.

The build is ad-hoc signed (not notarized), so the first launch
requires a right-click → **Open** to bypass Gatekeeper.

## Build from source

```bash
cargo run --release        # run directly
cargo bundle --release     # produce target/release/bundle/osx/Noted.app
```

## Package a DMG

```bash
cargo install cargo-bundle
brew install create-dmg
cargo bundle --release
codesign --force --deep --sign - target/release/bundle/osx/Noted.app
create-dmg \
  --volname "Noted" \
  --window-size 600 380 \
  --icon "Noted.app" 160 180 \
  --app-drop-link 440 180 \
  --hide-extension "Noted.app" \
  --no-internet-enable \
  dist/Noted.dmg \
  target/release/bundle/osx/Noted.app
```

## License

MIT
