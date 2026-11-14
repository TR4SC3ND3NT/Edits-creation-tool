# How manga and art edits are made, and how to read one

A guide for an agent that recreates a short beat-synced anime or manga edit
("manga edit", "panel edit", "art edit") with the `edits` engine: what editors
do, which of our presets and effects does the same thing, and how to recognise
each move in a reference by combining `edits-analyze` signals with looking at
frames. Numbers marked *source* come from the tutorials and papers listed at
the end; numbers marked *start* are our starting points to be checked against
the reference with `reference_compare`. Times are in beats, sizes in percent
of the frame height, as everywhere in the project format
([FORMAT.md](FORMAT.md)).

## 1. Picture selection and order

Editors build the edit around one subject (a character, a rivalry, an arc)
and pick pictures with strong visuals: reveals, expressive close-ups, impact
frames (*source*: clippingcalc). Their sources are manga panels (often
coloured by fans), official colour arts, cover and volume arts, and emblems or
logos cut out on black. In a manga edit the picture types play different
roles:

| Role | Picture | Where it goes |
| --- | --- | --- |
| Hook | The strongest full art or a face close-up | Shot 1 or 2: "no slow intro", the hook lands in the first 0-2 s (*source*: clippingcalc) |
| Establish | Full colour arts, medium shots, the character in pose | The build: one picture per beat or per two beats |
| Detail | Panel crops of eyes, hands, mouth, the menacing "ドドド" letters | Fast bursts (half-beat cuts) before a drop |
| Symbol | An emblem, a logo, a notebook, a title on black | Breathing points: a held shot with a glow, often after a burst, often repeated |
| Payoff | The best art again, larger or in colour after a black-and-white run | The drop and the last bar; edits loop, so the end leads back to the start |

Ordering rules that hold across tutorials and the reference:

- **Alternate scale.** Wide art, then close-up, then wide; two close-ups of
  the same size in a row read as a jump cut.
- **Alternate tone.** A colour art, then a black-and-white or inverted frame.
  The reference alternates bright (flash, threshold) and dark (vignetted card,
  emblem on black) frames almost cut by cut.
- **Pair the two characters by section.** In a two-character edit (Light and
  Kira) each character holds a phrase (2-4 bars) and they swap at the phrase
  boundary; on the reference's contact sheet the four rows (4 s each) go
  Light, Kira, Light, Kira.
- **Reuse.** The same emblem or art comes back two or three times, each time
  treated differently (normal, then inverted, then glowing). The engine's
  slots make this cheap: one slot, several layers.
- **Use neighbouring pictures, not a stretched one.** When a moment needs
  more time, AMV editors take the next panel of the same action rather than
  holding one longer (*source*: r/amv "scene shifting").

## 2. Timing rules

Run `audio_analyze` (tempo, bars, kick/snare/hat onsets, sections, the drop)
first; the rules are written against its output.

- **Cut on the beat grid.** The common pattern is one new picture per beat or
  per downbeat, with movement inside the shot carrying the energy between cuts;
  cutting on every beat (or every half beat) is kept for short bursts such as
  the bar before a drop (*source*: beatsyncpro, cursa). At 120-170 BPM one
  beat is 0.35-0.5 s; the reference's contact sheet (one frame every 0.67 s)
  shows a new picture in almost every cell, so its shots are shorter than
  that. `beats_cut` makes the grid.
- **Instrument to move** (*source*: beat2cut, beatsyncpro): kick -> zoom
  punch or cut; snare -> flash or a bigger hit (shake, invert); hats ->
  nothing, or a small flicker in a burst. Not every beat gets an effect;
  overuse is the most common complaint about beat sync (*source*:
  animemusicvideos.org). `beats_accent` with the band names does this.
- **Build-up**: shorten the shots (1 beat -> 1/2 -> 1/4), add a slow push-in
  or a rising blur, desaturate. **Drop**: a white flash on the first beat, the
  hook picture back, the strongest shake. A flash "right before the drop or
  peak" is the classic accent (*source*: Boris FX).
- **Velocity**: the clip or picture moves slowly, rushes through the beat and
  slows again; speed graphs go from about 0.1x to 10x (*source*: FlexClip).
  Our `velocity` preset (curve `snap`, zoom 1.2) does it to a still.
- **Frame offsets.** Editors sometimes cut a frame or two before or after the
  beat because it feels more intentional (*source*: Adobe community). The
  analysis reports the per-cut offset; keep it rather than snapping blindly.
- **Shot length in this genre**: hook and symbol shots 1-2 beats; bursts 1/2
  to 1/4 beat; nothing longer than one bar (4 beats) except an intro or the
  end of the loop (*start*).

Text is rare in manga edits: the letters are usually part of the art (sound
effects, speech balloons). When editors add text it is a name, a lyric word
or a quote on a beat, appearing with a typewriter or flicker (our
`typewriter`, `random_flicker`, `pop_in`).

## 3. Effect vocabulary

| Move | Typical parameters | In `edits` |
| --- | --- | --- |
| White flash | 1-6 frames, 0.1-0.2 s (*source*: clippingcalc, nerdschalk); 1-3 frame white inserts on trap hits (*source*: beatsyncpro) | `flash` (strength 0.85, decay 0.5 beat); a solid white 1-frame layer; `transition:flash_fade`, `transition:overexposure` |
| Burn to ink | The art goes white and only black lines survive, then returns | `flash_burn` (stops 4), `threshold_hit` |
| Zoom punch | About 1.1x on emphasis (*source*: course page); 110-120% (*start*) | `punch` (amount 1.15, decay 0.5 beat) |
| Shake | Sapphire S_Shake: amplitude 6 falling to 0 over about 20 frames, frequency 15, rotation random amplitude 5, motion blur on (*source*: Boris FX); scale 105-110% to hide the edges (*source*: aejuice); under 1 s, strength 30-50% in CapCut (*source*: capcuteffectspro) | `shake` (amplitude 3% of height, rotation 2 deg, frequency 14/s, decay 1 beat; zooms in to hide edges); `transition:camera_shake` |
| Exposure shake / flicker shake | Shake plus brightness jumps (pack presets "Fast Expo Shake", "flicker shake", *source*: Klain pack); Twitch "light" and "slide" operators (*source*: Video Copilot) | `shake` + `flash` on the same beat, or `exposure` keyed per frame |
| Invert, threshold, posterise | One frame to half a beat on a snare | `invert_hit` (0.25 beat), `threshold_hit`, `posterize` keyed, `bw_hit` |
| Duotone / tinted frame | Two colours mapped to dark and light (the reference's green Killer Queen frame) | `gradient_map` (shadows, highlights), `tint`, `hue` |
| RGB split | A few pixels to 1-2% of height, decaying | `rgb_split` preset (1.5), `chromatic_aberration` |
| Blur | Gaussian in and out; directional for whips; zoom blur through cuts | `blur_in`, `blur_out`, `directional_blur`, `zoom_blur`, `transition:cross_zoom`, `transition:whip_pan` |
| Glow | Bloom on bright parts; coloured glow around a card or an emblem | `glow`, `bloom`, `glow_pulse`; `outline` + `glow` with a colour on the card layer |
| 3D card / tilt | A picture on a card with a thin border, tilted 15-40 deg, turning or drifting (*start*) | an `image` layer with `transform.rotation [x,y,z]`, `flip_3d`, `drop_shadow`, `outline` |
| Cube | Two to four faces turning a quarter turn per beat | `cube` layer, `cube_turn`, `transition:cube` |
| Panel slide / reveal | Panels slide in from the side, split screens, masks | `slide_in`, `slide_out`, `split_panels`, `iris_in`, `transition:directional` |
| Screentone, ink | Halftone dots, printed-page look | `screentone`, `halftone`, `ink` |
| Speed and focus lines | Radial lines around a face on a hit | `speed_lines`, `burst_lines` |
| Paper / handwriting texture | Multiply, Overlay or Darken blend for a printed page (*source*: CapCut manga panel guide) | `overlay_texture` with a texture slot, `grain`, `film_damage` |
| Vignette | Heavy, dark corners on every frame of a "dark" edit | `vignette` on an adjustment layer, `vignette_pulse` |
| Colour grade | Contrast up, clips tinted to one look (*source*: clippingcalc) | `levels`, `brightness_contrast`, `lut`, `vibrance` |
| Low frame rate | Stutter, art "on twos" | the layer's `fps` |

## 4. Recognising the moves from frames

The analysis measures what has a number (cuts, scale, shift, gain, blur
radius, saturation, inversion) and marks the rest `needs_agent`
([ANALYSIS.md](ANALYSIS.md)). Looking at frames fills in the *kind* of move;
the signals keep the *numbers*. Never ask the model for speeds, easings or
exact frame counts: video LLMs are weakest at shot speed and motion when
frames are sampled uniformly (VEU-Bench), and vision models miss fine detail
where shapes overlap or are close (VLMs are blind).

### What to sample

For every shot from `reference_read` section `shots`:

1. A **strip** of 6 frames: the last frame of the previous shot, the cut
   frame, cut+1, cut+2, the shot's reference frame, and the last frame. One
   strip per image, tiles labelled with frame number and beat (set-of-mark
   style labels help grounding; *source*: SoM). Use `ffmpeg -vf
   select=...,tile=6x1` or the proxies in `analysis/proxies/`.
2. **Every frame** around flagged events (flash, tone hit, `bridged` track
   frames): 1-frame hits are invisible at 1 fps sampling.
3. **Crops**: a corner crop for overlays and texture, the card edge for
   borders and glow, enlarged 2-3x. Thin strokes and outlines need a crop.
4. Keep each image at or under 1568 px on the long edge (Claude resizes
   larger ones; newer models keep up to 2576 px), and at most 20 images per
   request, or the per-image limit drops (*source*: Claude vision docs). One
   strip per shot is cheaper and more reliable than many single frames.

### Questions per shot (fixed vocabulary; answer "none" or "unsure" when in doubt)

- **Q1 picture**: `panel_bw`, `panel_colour`, `full_art`, `close_up`,
  `detail_crop`, `emblem`, `text_card`, `solid`. Which character, if known.
- **Q2 framing**: `full_frame`, `inset_card`, `tilted_card`, `cube`,
  `split`, `picture_in_picture`. Border: `none`, `thin_light`, `thin_dark`,
  `glow:<colour>`.
- **Q3 tone, per tile**: `normal`, `inverted`, `threshold`, `posterized`,
  `duotone:<dark>/<light>`, `tinted:<colour>`, `desaturated`, `burnt_white`.
- **Q4 light**: `white_flash`, `black_flash`, `glow`, `light_rays`,
  `flare`, `none`; which tiles.
- **Q5 motion** (kind only): `static`, `punch_in`, `zoom_out`, `push_in`,
  `shake`, `slide:<dir>`, `spin`, `3d_turn:<axis>`, `whip:<dir>`.
- **Q6 blur**: `none`, `gaussian`, `directional:<angle>`, `zoom`, `spin`.
- **Q7 overlays**: `paper`, `handwriting`, `grain`, `scratches`,
  `speed_lines`, `focus_lines`, `screentone`, `particles`, `caption`,
  `sfx_in_art`. Static or moving; lighter than the picture, darker, or both
  (both suggests `difference` or `overlay` blend).
- **Q8 entry**: `hard_cut`, `flash_cut`, `dissolve`, `whip`,
  `zoom_through`, `cube`, `push`, `glitch`.

Ask for the tile numbers that show each answer. Each answer is then checked
against a signal before it enters the project.

### Making the labels reliable

- **Cross-check with signals**: `inverted` must match a negative correlation
  in the tone track; `white_flash` an exposure peak; `shake` a residual in
  the curves; `punch_in` a scale key. Disagreement -> look again at every
  frame of that span, do not average.
- **At least two tiles** for a label of a lasting state; a 1-frame label
  needs the every-frame view.
- **Side by side**: to check a recreation, put the reference frame and the
  render at the same beat in one image, left and right, and ask what differs
  in the fixed vocabulary (comparative prompts on concatenated pairs are how
  MLLM-CompBench tests this). `reference_compare` gives the numbers; the
  picture tells you which move is wrong.
- **One question per region**: ask about the card border on the card crop,
  the overlay on the corner crop.
- **Vocabulary from the registry**: map every label to a preset or effect
  name above, so an answer is an action.

### Prior work

Per-shot attribute extraction for transferring an editing style from one
video to new footage (framing, content, speed, lighting) is in *Automatic
Non-Linear Video Editing Transfer*. *Edit3K* learns representations of 3,094
CapCut editing components (effects, animations, transitions, filters,
stickers, text) that separate the effect from the footage, the same
"picture cancels" idea our analysis uses; *AutoTransition* recommends
transitions from shots and audio. *VEU-Bench* (CVPR 2025) tests video LLMs
on shot size, angle, motion, speed, cuts and transitions: some score below
chance, worst on speed and motion. The common pipeline is shot detection
(TransNetV2, recall-biased threshold 0.5) then one or a few keyframes per
shot to a VLM; detectors miss gradual transitions that VLMs catch and VLMs
miss hard cuts (TransVLM), which argues for our split: signals for cuts and
numbers, the model for kinds. *AutoMatch* and *HarmonySet* give large sets of
cut times against music beats.

### The reference, read this way

From the 24-frame contact sheet of the Kira edit: a black lead-in; a heavy
vignette on every frame; a static overlay of handwriting columns and an
ornament top left that is light on dark frames and dark on light frames;
inset colour cards with a thin light border (Light and Misa art), one of
which burns to white while a red glow around it grows; a tilted card
(about 30 deg) with a threshold art; burnt-white close-ups between colour
arts; inverted and posterised colour frames (cyan, magenta, yellow); a
yellow glowing emblem on black, shown twice; full-frame Kira arts and panel
crops with "ドドド" letters; a picture in picture (a panel inside a zoomed
panel); a green duotone frame near the end.

## 5. Test pictures

Do not commit them (`/assets` is ignored, see [CLAUDE.md](../CLAUDE.md)).

- **Best: the user supplies them.** The arts of Light, L and Kira are
  copyrighted (Shueisha, Ohba/Obata, Araki); the user can use their own
  copies, scans of their volumes, or screenshots for a local test.
- **Quick stand-ins from the reference itself**: `edits-analyze analyze`
  writes one still per shot to `analysis/proxies/` (`draft --proxies` uses
  them). They contain the overlay and vignette, so they only test timing.
- **Fetchable sources, checked 2026-11-14**: Danbooru's `robots.txt`
  disallows everything but its front page and `*.json`, so no. Safebooru's
  `robots.txt` allows its API (`index.php?page=dapi&s=post&q=index&tags=...&json=1`
  answers), and its posts are tagged by character (`yagami_light`,
  `l_(death_note)`, `kira_yoshikage`), but most posts are fan art reposted
  without the artist's permission; use it only if the user agrees, at a low
  rate, and keep files local. AniList's site asks for `ai-train=no,
  use=reference` and blocks AI crawlers; its API serves small character
  portraits only. MangaDex sits behind a bot challenge. Not downloaded.
- **For automated tests** keep generating pictures in code (`edits demo`).

## 6. Gaps in our engine

1. **No colour-look classification in the analysis.** `tone` measures
   saturation, inversion, threshold and posterise; duotone, gradient maps,
   tints and hue shifts are not detected, though the renderer has
   `gradient_map`, `tint` and `hue`.
2. **Cards, tilts and insets are not measured.** They are `needs_agent`:
   no card corners, no homography to 3D rotation, no border or glow colour,
   and the same art on a card and full frame lands in two slots.
3. **Overlays are only half recovered.** The layers stage finds thin bright
   strokes; a dark or two-tone texture (the handwriting here) is lost, the
   blend mode is a guess, and there is no built-in paper or handwriting
   texture or font (`overlay_texture` needs a picture; the only font is
   Inter).
4. **No structured vision step.** There is no command that writes per-shot
   strips and crops, and no place in `reference_analysis.json` for labels in
   a fixed vocabulary; the analyst writes free text. A `strips` command and a
   `labels` section (Q1-Q8 above, with tile evidence) would close this.
5. **Picture roles are free text.** A slot has only a `note`; nothing says
   "this slot wants a close-up" or "an emblem on black", so replacing
   pictures cannot be checked or automated.
6. Smaller: shake frequency is not estimated; directional, zoom and spin
   blur are not told apart; dissolves are not modelled; a 1-frame hit on a
   flash peak is lost (all in [ANALYSIS.md](ANALYSIS.md)).

## Sources

- Boris FX, shake effect in After Effects: https://borisfx.com/blog/shake-effect-after-effects-tutorial/
- AEJuice, vibration shake: https://aejuice.com/blog/how-to-add-a-vibration-shake-in-after-effects
- ClippingCalc, how to make anime edits: https://clippingcalc.com/guides/anime-edits-guide
- Beat2Cut, beat-sync guide: https://beat2cut.com/blog/beat-sync-video-editing-complete-guide/
- BeatSyncPro, trap and boom-bap editing guides: https://beatsyncpro.ai/blog/trap-music-video-editing-guide-2026.html, https://beatsyncpro.ai/blog/boom-bap-music-video-editing-guide-2026.html
- Cursa, beat-based editing in CapCut: https://cursa.app/en/page/beat-based-editing-in-capcut-syncing-cuts-transitions-and-motion-to-music
- AnimeMusicVideos.org forum, syncing to beats: https://www.animemusicvideos.org/forum/viewtopic.php?t=64149
- r/amv, scene shifting: https://reddit.rtrace.io/r/amv/comments/xjbhtg/scene_shifting_the_easiest_mistake_all_video
- Adobe community, cutting just off the beat: https://community.adobe.com/t5/after-effects-discussions/need-help-with-editing-music-video-concept/m-p/15035504
- FlexClip, velocity edits: https://www.flexclip.com/learn/how-to-make-a-velocity-edit-on-capcut.html
- Nerdschalk, velocity in CapCut (flash length): https://nerdschalk.com/how-to-do-the-velocity-trend-using-capcut/
- CapCut shake settings: https://capcuteffectspro.blogspot.com/2025/04/create-trending-camera-shake-effect.html
- Zoom punch 1.1x: https://goviralontiktok.lovable.app/course/cadence-capcut/the-capcut-stack
- CapCut manga panel animation (paper texture blends): https://vediting.home.blog/2025/12/07/how-to-create-ai-manga-panel-animation-in-capcut/
- Klain shake pack: https://payhip.com/b/4ZE8J
- Video Copilot Twitch: https://www.videocopilot.net/products/twitch/features
- Wikipedia, anime music video (MMV, 3D styles): https://en.wikipedia.org/wiki/Anime_music_video
- Automatic Non-Linear Video Editing Transfer: https://arxiv.org/abs/2105.06988
- Edit3K: https://arxiv.org/abs/2403.16048
- AutoTransition (ECCV 2022): https://arxiv.org/abs/2207.13479
- VEU-Bench (CVPR 2025): https://arxiv.org/abs/2504.17828
- TransNet V2: https://arxiv.org/abs/2008.04838
- Kairos (TransNetV2 settings): https://arxiv.org/abs/2609.08755
- TransVLM: https://chence17.github.io/TransVLM/
- AutoMatch: https://arxiv.org/abs/2303.01884
- HarmonySet: https://arxiv.org/abs/2503.01725
- Set-of-Mark prompting: https://arxiv.org/abs/2310.11441
- MLLM-CompBench: https://arxiv.org/abs/2407.16837
- Vision language models are blind: https://arxiv.org/abs/2407.06581
- Claude vision docs (image size and count limits): https://platform.claude.com/docs/en/build-with-claude/vision
