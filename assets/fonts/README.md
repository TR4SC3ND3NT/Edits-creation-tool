# Bundled fonts

Fonts that ship inside the programs, used unmodified. Do not add fonts here
for a project: a text layer's `font` is a path relative to the project file,
so a project's fonts live next to it.

| File | Font | Used by | Licence |
| --- | --- | --- | --- |
| `InterVariable.ttf` | Inter 4.1, variable (`wght` 100–900, `opsz` 14–32), from `Inter-4.1.zip` of https://github.com/rsms/inter/releases/tag/v4.1 | `edits-render`: the default font of text layers (`include_bytes!` in `src/text.rs`); `edits-gui`: the interface font (`src/theme.rs`) | SIL Open Font License 1.1, no Reserved Font Name: `Inter-OFL.txt` |

| `LiberationSans-BoldItalic.ttf` | Liberation Sans Bold Italic 2.1.5 (Red Hat; metrically compatible with Arial and Helvetica), from the Debian package `fonts-liberation` 1:2.1.5-3 (upstream https://github.com/liberationfonts) | `edits-render`: text layers with `"font": "builtin:sans-bold-italic"` (`include_bytes!` in `src/text.rs`), a free stand-in for Nimbus Sans / Helvetica Bold Italic | SIL Open Font License 1.1, Reserved Font Names Liberation, Arimo, Tinos, Cousine: `Liberation-OFL.txt` |

SHA-256 of the files as downloaded:

```
4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf  InterVariable.ttf
c80fa7f2ffa0e01d4d8dcd6a6d1e43eda665222d1b4db597dde1174c456006cf  LiberationSans-BoldItalic.ttf
```

Both are used unmodified, so the Reserved Font Names do not restrict them.

The OFL allows the font to be embedded in and distributed with software, as
long as the licence text goes with it and the font is not sold on its own.
