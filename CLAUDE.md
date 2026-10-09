# CLAUDE.md

Before working on this repo, read [AGENTS.md](AGENTS.md). It has the project goals, the
current architecture, the Bevy 0.16 → 0.20 upgrade plan and the coding guidelines.

Quick facts:
- Rust + Bevy 3D city builder. The code is on **Bevy 0.16**; the target is the latest (0.20).
- The design notes are in `notes.md`.
- `assets/models/*.glb` come from purchased packs and are git-ignored. Never commit them.
- The user often has uncommitted WIP (terrain/rivers). Don't discard or reformat it.
