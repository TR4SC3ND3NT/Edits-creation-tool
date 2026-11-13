# Fonts of the window

`edits-gui` embeds these files with `include_bytes!` (see `src/theme.rs`), so
the window looks the same on every machine. They are used unmodified.

| File | Font | Version | Source | Licence |
| --- | --- | --- | --- | --- |
| `InterVariable.ttf` | Inter (variable: `wght` 100–900, `opsz` 14–32) | 4.1 | `Inter-4.1.zip` from https://github.com/rsms/inter/releases/tag/v4.1 | SIL Open Font License 1.1, no Reserved Font Name: `Inter-OFL.txt` |
| `JetBrainsMono-Variable.ttf` | JetBrains Mono (variable: `wght` 100–800), renamed from `fonts/variable/JetBrainsMono[wght].ttf` | 2.304 | `JetBrainsMono-2.304.zip` from https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304 | SIL Open Font License 1.1: `JetBrainsMono-OFL.txt` |
| (in the `lucide-icons` crate) | Lucide icon font `lucide.ttf` | 1.72.0 | the crate `lucide-icons` 1.72.0 (https://crates.io/crates/lucide-icons), which ships the font of https://github.com/lucide-icons/lucide/releases/tag/1.72.0 | ISC, with the icons derived from Feather under MIT: `Lucide-LICENSE.txt`; the crate is "MIT AND ISC" |

SHA-256 of the files as downloaded:

```
4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf  InterVariable.ttf
662a196d58f1183bf2d77428b6d5283fe3f45161ab021bea4036bc98e5cac016  JetBrainsMono-Variable.ttf
aee3cd266701c853edb389d82f6c0c8c277345da599e9c43a30b09ef3f8b6a62  lucide.ttf (lucide-icons 1.72.0)
```

How they are used: Inter for all interface text (the body at weight 430,
medium 540, semibold 620, picked with the variable `wght` axis through
`FontTweak::coords`), JetBrains Mono for every number (value fields,
timecode, frame numbers: its digits all have the same width, so values do
not jitter while they change), Lucide for icons. egui's bundled fonts stay
behind them as a fallback for characters these lack.

The variable fonts are about 1.2 MB together; static files of the three
Inter weights alone would be about the same size. To update a font, replace
the file with one from the official release and change the table above.
