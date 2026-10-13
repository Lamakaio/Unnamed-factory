# CLAUDE.md

Before working on this repo, read [AGENTS.md](AGENTS.md). It has the project goals, the
current architecture, Bevy upgrade notes and the coding guidelines.

Quick facts:
- Rust + Bevy on **Bevy 0.20**, pinned nightly toolchain. Since 2026-10-13 the project is
  becoming an evolution simulation on a small living planet (design and decisions:
  `docs/living-world.md`); the city builder was retired.
- World generation lives in `crates/worldgen` (preview: `cargo run -p worldgen --release --example preview -- <out_dir> [seeds]`).
- The former city-builder design notes are in `notes.md`.
- `assets/models/*.glb` come from purchased packs and are git-ignored. Never commit them.
- The user often has uncommitted WIP (terrain/rivers). Don't discard or reformat it.
