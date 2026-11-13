# Bundled fonts

Fonts that ship inside the programs, used unmodified. Do not add fonts here
for a project: a text layer's `font` is a path relative to the project file,
so a project's fonts live next to it.

| File | Font | Used by | Licence |
| --- | --- | --- | --- |
| `InterVariable.ttf` | Inter 4.1, variable (`wght` 100–900, `opsz` 14–32), from `Inter-4.1.zip` of https://github.com/rsms/inter/releases/tag/v4.1 | `edits-render`: the default font of text layers (`include_bytes!` in `src/text.rs`); `edits-gui`: the interface font (`src/theme.rs`) | SIL Open Font License 1.1, no Reserved Font Name: `Inter-OFL.txt` |

SHA-256 of the file as downloaded:

```
4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf  InterVariable.ttf
```

The OFL allows the font to be embedded in and distributed with software, as
long as the licence text goes with it and the font is not sold on its own.
