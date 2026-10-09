# CLAUDE.md

Before working on this repo, read [AGENTS.md](AGENTS.md). It has the project goals, the
current architecture, Bevy upgrade notes and the coding guidelines.

Quick facts:
- Rust + Bevy 3D city builder on **Bevy 0.20**, pinned nightly toolchain.
- `noiz` is `[patch]`ed to a local port at `../noiz-bevy-0.20`. Remove the patch once upstream supports 0.20.
- The design notes are in `notes.md`.
- `assets/models/*.glb` come from purchased packs and are git-ignored. Never commit them.
- The user often has uncommitted WIP (terrain/rivers). Don't discard or reformat it.
