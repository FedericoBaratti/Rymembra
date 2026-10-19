/* Capitolo 10 — le skin: il documento, il compilatore, il movimento, l'impaginazione, il tema, l'accento, i pacchetti, la migrazione. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "skins",
  title: "Skins",
  lead: "`rymembra-skin`: a skin is typed data, never CSS. The document and its registries, the only CSS author, motion and its safety rules, the layout tree, themes and accents, packages, and the migration from format 1.",
  entries: [
    {
      id: "skin",
      title: "The skin document (format 2)",
      src: ["core/rymembra-skin/src/lib.rs", "core/rymembra-skin/src/document.rs", "core/rymembra-skin/src/tokens.rs", "core/rymembra-skin/src/parts.rs", "core/rymembra-skin/src/stati.rs", "core/rymembra-skin/src/values.rs", "core/rymembra-skin/src/effects.rs", "core/rymembra-skin/src/vicini.rs", "core/rymembra-skin/skins/plain.json", "apps/desktop/src-tauri/src/skin.rs"],
      body: [
        "**A skin is data, not CSS.** Colours enter as channels, lengths as number + unit, curves as four numbers; the compiler is the only author of CSS, so `url()` beacons, `@import` and attribute-exfiltrating selectors have no channel to travel through. The guarantee holds only if every field is typed, which is why even harmless things like `calc()` and CSS colour names are refused.",
        { h: "Shape" },
        "`skin.json`, `SKIN_FORMAT_VERSION` = 2. Root keys in canonical order: `format`, `id`, `meta`, `scheme` (`dark`/`light`), `capabilities` (`dynamicAccent`), `fonts` (≤ 8 families: `src`, `weight`, `style`), `palette`, `patterns`, `tokens`, `variant` (overrides of `tokens`, `palette`, `patterns`, `parts` for the opposite scheme — no motion), `platforms`, `motion`, `layout`, `parts`. Every block refuses unknown keys and suggests the nearest valid name (`vicini::forse`, «did you mean»).",
        {
          table: {
            head: ["Registry", "Size", "What"],
            rows: [
              ["`TOKENS`", "73", "Design tokens (colour 22, `canvas.viz.*` 20, layout 5, motion 5, space 5, depth 4, radius 4, shadow 4, font 3, glow 1); kinds colour, length, duration, easing, number, font stack, shadow; 27 have min/max bounds"],
              ["`PARTS`", "101", "Restylable components in 10 groups (shell, nav, page, controls, lists, cards, player, column, now playing, overlays)"],
              ["`PartState`", "10", "hover, pressed, selected, current, open, checked, playing, paused, focus, disabled — in cascade order, all rules inside `:where()` so specificity is equal"],
              ["`Effect`", "12", "solid, linear/radial/conic gradient, hairline grid, scanlines, stripes, dot grid, vignette, chamfer, blur-behind, image"],
              ["`WIDGETS`", "14", "Layout widgets — see [Layout](#layout)"],
            ],
          },
        },
        "A part can restyle background (effect stacks), border, radius, shadow, focus ring, clip, filters, text colour and size (9–72 px), plus `before`/`after` layers, `states` and `motion` — **never** width, height, margin, padding, position or display, so a skin cannot break the layout. Values: colours as hex/`rgb()`/`rgba()`, `{\"$token\":…}`, `{\"$palette\":…, opacity}` or the cover-dynamic accent; `{\"$pattern\":…}`; easings and animations by name.",
        { h: "Validation never fails" },
        "`leggi_skin` returns every error with a dotted path, a code and a position (`posizioni` maps the path back to line and column, in UTF-16 units for the editor); `check_skin` returns warnings: contrast below `CONTRASTO_MINIMO` = 4.5 for text on `color.surface.0..2`, focus ring below `CONTRASTO_ANELLO` = 3.0 (WCAG 1.4.11), and cost over budget. Built-in skins: `plain` (the reference, `skins/plain.json`), `vinile`, `neon`.",
        { h: "In the app" },
        "`skin.rs` compiles the active skin (`skin.active`, default `plain`) and returns `Skin { css, layout, presenze, … }`; the frontend writes the CSS into one `<style id=\"skin-attiva\">` and sets `data-skin` on `<html>` ([Frontend](#frontend)). `stile.css` carries a generated copy of `plain` under `:root` so the first frame is styled before IPC answers, and a test keeps it identical to the compiler's output; `tauri.conf.json`'s `backgroundColor` is likewise pinned to `plain`. A chosen skin whose package disappeared falls back to `plain` while keeping your choice; an installed skin that no longer validates is listed as «open in the Studio».",
      ],
      see: ["skin-compiler", "motion", "layout", "skin-package", "studio"],
    },
    {
      id: "skin-compiler",
      title: "From document to CSS",
      src: ["core/rymembra-skin/src/compile.rs", "core/rymembra-skin/src/effects.rs", "core/rymembra-skin/src/compile/tests.rs", "core/rymembra-skin/tests/fedelta.rs"],
      body: [
        "`compile_skin` is infallible and returns `CompiledSkin { id, css, cost, shell_cost, motion_cost, dynamic_tokens, presenze }`.",
        {
          table: {
            head: ["Scope (`Ambito`)", "Root selector", "Draft resources", "Forced states"],
            rows: [
              ["`app`", "`:root[data-skin='<id>']`", "no", "no"],
              ["`prova` («try on app»)", "`:root[data-skin='<id>']`", "yes", "no"],
              ["`tela` (Studio canvas)", "`:root[data-skin='<id>']`", "yes", "yes"],
              ["`anteprima` (cards)", "`.anteprima[data-skin='<id>']`", "yes", "yes"],
            ],
          },
        },
        {
          ul: [
            "Tokens become `--skin-*` custom properties; the variant is emitted under `[data-theme='<opposite scheme>']`, mobile overrides under `[data-mobile]`. Derived values (e.g. `--accent-rgb` triples, `--shell-left`, `--player-clearance`) are computed by the compiler, not authored.",
            "Each part compiles to rules on its class; states map to DOM state (`:hover`, `[aria-current]`, `[aria-selected]`, `[aria-pressed]`, `:disabled`, `[data-playing]`…), with `:where()` so later rules win by order.",
            "**Only names reach the CSS** — id, version, palette, pattern, easing, animation, font and asset names — and each passes `nel_suo_alfabeto` at the point of insertion. The header comment carries only id and version (the name used to be there and allowed a `*/` injection). The only `url()` points to `http://rymembra-skin.localhost` (`ORIGINE_RISORSE`). Keyframes are named `skin-<id>-anim-<name>`, fonts `skin-<id>-<family>`.",
            "Effects have a cost class — cheap 1, paint 3, composited 4, GPU 10 — and each surface has `SURFACE_COST_BUDGET` = 10 (exactly one `backdrop-filter`). Cost, target property, name and compilation are four exhaustive `match`es: an effect without a cost does not compile.",
          ],
        },
        "`tests/fedelta.rs` compares `plain` against the old app's `global.css` rule by rule, listing the deliberate departures; `compile/tests.rs` covers scopes, escaping and motion rules.",
      ],
      see: ["skin", "motion", "accent"],
    },
    {
      id: "motion",
      title: "Motion, the flash rule and reduced motion",
      src: ["core/rymembra-skin/src/movimento.rs", "core/rymembra-skin/src/document.rs", "core/rymembra-skin/src/compile.rs", "core/rymembra-skin/tests/attivazione.rs", "core/rymembra-app/src/preferenze.rs", "apps/desktop/src/aspetto.ts", "apps/desktop/src/presenza.tsx"],
      body: [
        "Animations are declared once in `motion.animations` (≤ `MAX_ANIMAZIONI` = 64; 2–12 keyframes, the first at 0; duration ≤ 10 s, delay ≤ 5 s, iterations ≤ 16 or `infinite`) and bound by parts to **triggers**: the 10 states plus 5 moments — enter, exit, share (flying between screens), in-view, change. The principle: a skin animates **states of the DOM, not events of the application**; an infinite loop is allowed only on a state (a record spinning while `playing`, which pauses rather than restarts). Keyframes move, scale, rotate, skew, blur, recolour and clip. Staggers are capped at 400 ms over 12 items.",
        { h: "Always stoppable" },
        {
          ul: [
            "Every duration is emitted as `calc(<t> * var(--motion-scale, 1))`, and a test asserts none escapes. `--motion-scale` comes from `motion.intensity`: `none` 0, `essential` 0.5, `full` 1, `maximum` 1.25.",
            "For **every** animated part the compiler writes `animation: none !important` under `@media (prefers-reduced-motion: reduce)`, under `:root[data-motion-utente='ridotto']` (the in-app preference `ui.reducedMotion`) and under `[data-motion='none']` — infinite loops included, and a skin cannot opt out. View-transition rules get the same treatment.",
            "The user preference can only **reduce**: «as the system» or «reduce everything»; the OS setting always wins.",
          ],
        },
        { h: "The flash rule" },
        "A repeating change of light (opacity, blur, brightness, saturation, hue, background/text/border colour, shadow; not transforms or clip) with a half-cycle shorter than `MEZZO_CICLO_MINIMO_MS` = 333 ms — more than about 1.5 flashes per second, stricter than WCAG 2.3.1 because area and luminance are unknown — is an **error**, even in an animation that is declared but unused. Three or fewer iterations never count. The check accounts for `alternate`, the jump back at the end of a cycle, and `essential` halving durations.",
        { h: "Budgets" },
        "Motion costs: `MOTION_COST_BUDGET` = 16 per part and `WINDOW_MOTION_BUDGET` = 96 for the window, an infinite loop costing `COSTO_CICLO` = 4. Over budget is a warning shown while you spend it; over twice the budget (`OLTRE_IL_BUDGET` = 2) the skin is refused at install. Between 1× and 2×, or with illegible text or an invisible focus ring, installing asks first (`ConfermaSkin.tsx`, from `EsameIpc.daConfermare`).",
        { h: "Presence" },
        "Parts that declare enter/exit/share rules are listed in `CompiledSkin.presenze`; the frontend wraps them in React `<ViewTransition>` only then (`presenza.tsx`), with classes `vt-<part>-enter|exit|share`. Content changes alternate `data-cambio=\"a|b\"` so keyframes restart, and `--indice` drives staggers.",
      ],
      see: ["skin-compiler", "views"],
    },
    {
      id: "layout",
      title: "The layout tree",
      src: ["core/rymembra-skin/src/layout.rs", "apps/desktop/src/Impaginazione.tsx", "apps/desktop/src/studio/AlberoScafale.tsx"],
      body: [
        "`layout.shell` describes where things are, with a closed vocabulary that cannot overlap or overflow: flex rows, columns and scroll zones (no grid, no absolute positioning), gaps from `none` to `xl`, alignment and distribution options.",
        {
          ul: [
            "Each container has exactly one `Fill` child; fixed sizes are 24–480 px (or rem) and the fixed siblings together stay within `SHELL_FIXED_BUDGET` = 640 px.",
            "At most `MAX_SHELL_DEPTH` = 6 levels and `MAX_SHELL_NODES` = 64 nodes; `SHELL_COST_BUDGET` = 24.",
            "**14 widgets**: ambient, navigation, bottom-nav, page-header, content, player, column, transport, scrubber, queue, selection-bar, now-playing, cover-large, rating. Some are essential (navigation, playback, content must exist). Prefabs are reusable subtrees.",
          ],
        },
        "`default_shell(player, sidebar)` builds the default tree (a skin without `layout.shell` costs 3). The compiler emits each zone as `[data-nodo='0-1-2'] { display: flex … }` with `container-type: inline-size` on the zone that holds `content`. In the window `Impaginazione.tsx` renders the tree: zones as `div.zona`, widgets through a `WIDGET` registry mirroring `WIDGETS` (an unknown name renders a visible placeholder); overlays stay outside it.",
      ],
      see: ["views", "studio"],
    },
    {
      id: "theme",
      title: "Theme and variant",
      src: ["apps/desktop/src/tema.ts", "apps/desktop/src/aspettoFinestra.ts", "core/rymembra-app/src/preferenze.rs", "core/rymembra-skin/src/document.rs"],
      body: [
        "`ui.theme` is `scuro`, `chiaro` or `sistema`. A skin declares its base `scheme` and may provide a `variant` for the other one. `temaMostrato(tema, skin)` resolves what is shown: the wanted scheme (`sistema` follows `prefers-color-scheme` and changes live) if it equals the skin's base or the skin has a variant, otherwise the base. `applicaTema` writes `data-theme=\"light|dark\"` on `<html>`, which the compiled variant rules select; Settings disables the choice a skin cannot honour and says which variant is missing.",
        "The theme moved from `localStorage` into `settings` so backup and profile carry it; the old `rymembra.tema` key is read once as a fallback.",
      ],
      see: ["skin", "accent"],
    },
    {
      id: "accent",
      title: "Accent from the cover",
      src: ["core/rymembra-skin/src/dinamico.rs", "core/rymembra-app/src/tinta.rs", "apps/desktop/src-tauri/src/skin.rs", "apps/desktop/src/aspetto.ts"],
      body: [
        "With `skin.dynamicAccent` on (default off), the accent follows the playing cover: `tinta::dominante` picks the cover's hue ([Covers](#covers)) and `dinamico::accento_sicuro` keeps that **hue** while moving lightness in OKLCH until the colour reaches the required contrast against every surface it appears on as text, and against the text drawn on top of it; chroma is reduced to fit sRGB and capped at `TETTO_CROMA` = 0.20. If no lightness works, the answer is `None` and the skin's own accent stays. A skin can refuse with `capabilities.dynamicAccent: false`, and that wins over the preference. Text never follows the cover.",
        "`accento_copertina` returns the variables, which `applicaAccento` writes on the root's inline style. The Studio's palette generator (`tavolozza_da`) uses the same rule to build a legible palette from one colour or the current cover.",
      ],
      see: ["covers", "theme", "studio"],
    },
    {
      id: "skin-package",
      title: "Packages, installation and the skin protocol",
      src: ["core/rymembra-skin/src/package.rs", "apps/desktop/src-tauri/src/skin.rs", "apps/desktop/src/ConfermaSkin.tsx", "core/rymembra-cloud/src/pacchetti.rs"],
      body: [
        "A `.ryskin` is a zip with a **closed list** of entry names: `skin.json` (required), `preview.png`, and `assets/<name>.<ext>` one level deep with a constrained file name. Types are recognised by magic bytes — PNG, JPEG, WebP, WOFF2; no SVG. All limits are checked from the zip index **before** decompressing:",
        {
          table: {
            head: ["Limit", "Value"],
            rows: [
              ["`MAX_ARCHIVE_BYTES`", "20 MiB"],
              ["`MAX_TOTAL_BYTES` (uncompressed)", "60 MiB"],
              ["`MAX_ENTRY_BYTES`", "8 MiB"],
              ["`MAX_MANIFEST_BYTES`", "512 KiB"],
              ["`MAX_ENTRIES`", "64"],
              ["`MAX_COMPRESSION_RATIO`", "200"],
            ],
          },
        },
        "Writing deflates the manifest and stores assets uncompressed. Installing (`skin_installa`, also by dropping a file on the window) validates, runs the exam (refuse above 2× budget; ask when `daConfermare`), and copies the package to `<data>/skin/<id>.ryskin`; built-in skins cannot be uninstalled.",
        "Fonts and images are served by the custom scheme `rymembra-skin`: `http://rymembra-skin.localhost/<id>/<name>` from an installed package, `…/bozza/<id>/<name>` from a Studio draft. The handler re-checks the id alphabet, the closed name list and the type from the bytes; no path is ever composed from unchecked input.",
      ],
      see: ["skin", "security"],
    },
    {
      id: "skin-migration",
      title: "From format 1 to format 2",
      src: ["core/rymembra-skin/src/migrazione.rs", "core/rymembra-skin/tests/migrazione.rs", "core/rymembra-skin/tests/oro"],
      body: [
        "Format 1 documents are migrated when read (`MIGRAZIONI`), before validation, and look as they did:",
        {
          table: {
            head: ["Format 1", "Format 2"],
            rows: [
              ["`capabilities.light` + `themes.light`", "`scheme` + `variant.tokens`"],
              ["`capabilities.mobile`", "removed (`platforms.mobile`)"],
              ["`layout.player`, `layout.sidebar`", "a full `layout.shell`"],
              ["`routeTransition`", "`motion.route`"],
              ["`filter`", "`backdrop`"],
              ["`layer`", "`after`"],
              ["`states.active`", "`states.current`"],
              ["`animations`", "`motion`"],
            ],
          },
        },
        "Parts that format 1 shared under one class are split, copying the style to each new part name (`COPIE`). `forma_canonica` writes keys in canonical order. `tests/migrazione.rs` compiles golden format-1 skins (`tests/oro/*.v1.json`) and compares the CSS with the frozen format-1 output as (rule, property, value) triples, with the allowed differences listed. Format 2 skins do not open in Rymembra 2.x, which is why the format change made this release a major ([CHANGELOG](https://github.com/FedericoBaratti/Rymembra/blob/main/CHANGELOG.md)).",
      ],
      see: ["skin"],
    },
  ],
});
