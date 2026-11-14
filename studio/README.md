# Edits Studio

The desktop app of the `edits` engine: Tauri v2, React, shadcn/ui. What it
does, the system packages it needs and every page are in
[docs/GUI.md](../docs/GUI.md).

```
bun install
bun run tauri build --no-bundle        # ../target/release/edits-studio
bun run tauri dev -- -- "$PWD/../demo/project.json"   # hot reload, debug panel (Ctrl+D)
bun run typecheck && bun run lint && bun run build
(cd src-tauri && cargo test)
```

- `src/` the frontend: `components/ui` as the shadcn CLI generates it,
  `components/pages` one file per page, `components/edit` the viewer,
  transport, timeline and inspector, `lib/store.tsx` the shared state,
  `lib/api.ts` the typed Tauri commands.
- `src-tauri/` the Rust side: project, frames, edits, `ui.json`, renders and
  comparisons (see the module docs in `src/lib.rs`). Not a member of the root
  Cargo workspace.

Scaffolded with [create-tauri-ui](https://github.com/agmmnn/tauri-ui) (MIT);
see `THIRD_PARTY.md` at the root.
