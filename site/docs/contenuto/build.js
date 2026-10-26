/* Capitolo 14 — costruire, provare, rilasciare: comandi, prove, strumenti, CI, firma, e come si scrive questa documentazione. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "build",
  title: "Building, testing, releasing",
  lead: "The commands, the test suites, the repository checks in `strumenti/`, the CI and release pipeline, update signing, and how this documentation is written and checked.",
  entries: [
    {
      id: "building",
      title: "Building and running",
      src: ["apps/desktop/package.json", "Cargo.toml", "rust-toolchain.toml", "apps/desktop/src-tauri/build.rs", "apps/desktop/vite.config.ts", "README.md"],
      body: [
        "Requirements: Rust stable ≥ 1.90 (`rust-version`; CI pins 1.97.1), Node.js 24 (the web tests use Node's own type stripping), Windows — the only installer target is NSIS.",
        {
          table: {
            head: ["Command (from `apps/desktop`)", "What"],
            rows: [
              ["`npm install`, `npm run dev`", "Vite on port 5173 (`strictPort`) + `tauri dev`"],
              ["`npm run build`", "`tauri build`: the NSIS installer, whose hook `src-tauri/installer/precedente.nsh` removes the previous version"],
              ["`npm run verify`", "Everything CI checks, offline: versions, translations, docs, CSS classes, `tsc` + Vite build, web tests, tool tests, `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`"],
              ["`npm run version:set <x.y.z>`", "Rewrites the version in `Cargo.toml`, `package.json`, `tauri.conf.json`, `Cargo.lock`"],
              ["`npm run licenze`", "Regenerates `THIRD-PARTY-NOTICES.md`"],
              ["`cargo deny check` (root)", "Licences, advisories, sources of Rust dependencies"],
            ],
          },
        },
        "`build.rs` generates the Tauri context, re-exports `RYMEMBRA_GOOGLE_CLIENT_ID` / `_SECRET` so `option_env!` can see them, and links the VC++ runtime statically in release builds only (static CRT broke doctests). Running the app on a copy of the data: set `RYMEMBRA_DATI` to another folder ([Data](#data)).",
      ],
      see: ["testing", "tooling", "ci"],
    },
    {
      id: "testing",
      title: "Tests",
      src: ["core/rymembra-domain/tests", "core/rymembra-skin/tests", "core/rymembra-sync/tests/convergenza.rs", "apps/desktop/test", "apps/desktop/test/risolvi.mjs", "strumenti/docs.test.js"],
      body: [
        "About 3,100 Rust tests and 300 web tests run in `npm run verify`, all without network access: the «Listen together» tests open sockets only on 127.0.0.1, never on all interfaces.",
        {
          ul: [
            "**Unit tests** live beside the code (`mod tests` / `mod prove`) — most modules prove the specific bug their doc comment describes.",
            "**Golden parity** (`core/rymembra-domain/tests/parita_*.rs`): text folding and keys, whole scan plans and album grouping compared with outputs recorded from the previous TypeScript implementation (`tests/golden/*.json`). Changing one of these behaviours means a deliberate, documented divergence.",
            "**Skins**: `fedelta.rs` (plain vs the old stylesheet), `migrazione.rs` (format 1 goldens in `tests/oro/`), `attivazione.rs` (flash rule, budgets, install exam).",
            "**Sync convergence** (`rymembra-sync/tests/convergenza.rs`): seeded random histories merged in every order must converge.",
            "**Database**: migrations from version 0 on in-memory databases, schema checks, SQLite version floor (3.53.2), error translation built from synthetic SQLite codes.",
            "**Audio**: decoding real fixture files (FLAC, Opus, …), gapless seams across sample rates, crossfade power conservation — as pure functions or with the engine on test streams.",
            "**Web** (`apps/desktop/test`, 38 suites): `node --test` over `.ts` files with a resolve hook (`risolvi.mjs`); they test the DOM-free modules (selection, sparse lists, modal rules, tour rules, Studio patch/history/prompt, …).",
          ],
        },
        "Some old file tests point at `Z:/non/esiste`, so on a machine where `Z:` is a mapped network drive that is off, `cargo test` waits for SMB timeouts.",
      ],
      see: ["building", "ci"],
    },
    {
      id: "tooling",
      title: "Repository checks (strumenti)",
      src: ["strumenti/classi.js", "strumenti/lingue.js", "strumenti/docs.js", "strumenti/sito.js", "strumenti/versione.js", "strumenti/licenze.js", "strumenti/manifesto.js", "strumenti/firma.js", "strumenti/finto-aggiornamento.js"],
      body: [
        "Plain CommonJS scripts with no dependencies, paths resolved from `__dirname`:",
        {
          table: {
            head: ["Script", "Checks or does"],
            rows: [
              ["`classi.js`", "Every class in TSX exists in `stile.css` or the skin registry; every registry part is emitted by the app; no dead CSS; every `z-index` is a layer token; every duration goes through `--motion-scale`; full-cover positioned layers declare `pointer-events`"],
              ["`lingue.js`", "Every language has exactly the keys of `it.json`; plural forms per CLDR category; every error code has an `errors.<code>` sentence (`--scrivi` fills missing keys)"],
              ["`docs.js`", "This documentation — see [Writing these docs](#writing-docs)"],
              ["`sito.js`", "Builds the website into `_site/` from `site/` — the landing page in four languages from one template and `site/testi/*.json`, one page per docs chapter, the sitemap, and copies the soundtrack in `site/audio/` (“Machina” by Scott Buckley, CC BY 4.0, fetched only when played) — then checks the result: the same keys in every language, no text left unused, every internal link, `src` and `data-src` and every anchor resolves, every `ANCORE_GUIDA` anchor reaches an entry, nothing loaded from another domain (`--controlla` builds in a temporary folder; `--versione` sets the release to announce; `--installer` points at the downloaded installer, whose size and SHA-256 the download section then shows)"],
              ["`versione.js`", "The version agrees across `Cargo.toml`, `package.json`, `tauri.conf.json`, `Cargo.lock`, or rewrites it"],
              ["`licenze.js`", "Generates `THIRD-PARTY-NOTICES.md` from `cargo metadata` (Windows target) and production npm packages"],
              ["`manifesto.js`", "Writes the updater's `latest.json` (version, signature, installer URL, notes from the CHANGELOG) and checks the signing key id"],
              ["`firma.js`", "Pre-flight of the signing secret (decodes, has the minisign header, password without stray whitespace); never prints it"],
              ["`finto-aggiornamento.js`", "A fake GitHub on `127.0.0.1:8787` to test updates end to end (`--guasta-firma` for the negative case), with the debug-only `RYMEMBRA_AGGIORNAMENTI_ENDPOINT`"],
            ],
          },
        },
        "`docs.test.js`, `lingue.test.js`, `manifesto.test.js` and `sito.test.js` test the tools themselves (`npm run test:strumenti`).",
      ],
      see: ["ci", "writing-docs", "language"],
    },
    {
      id: "ci",
      title: "CI and the release pipeline",
      src: [".github/workflows/verify.yml", ".github/workflows/release.yml", ".github/workflows/sito.yml", "deny.toml"],
      body: [
        "Every action is pinned to a commit hash (tags can be moved; see the March 2025 `tj-actions` incident), with the version in a comment.",
        {
          ul: [
            "**`verify.yml`** — on push, pull request, manual, or called by the release. Job `verifica` (Windows, toolchain 1.97.1, `RUSTFLAGS=-D warnings`): versions, translations, CSS classes, docs, the website build, tool tests, fmt, clippy and tests with `--locked`, web build and tests, and a check that `THIRD-PARTY-NOTICES.md` is up to date. Job `dipendenze`: `cargo deny check` (Windows target only; licence allow-list without GPL/LGPL; yanked crates denied; crates.io only) and `npm audit --omit=dev`. Job `niente-di-vietato`: fails on bundled binaries, on code that names YouTube downloaders or lyrics scrapers, and on missing public documents.",
            "**`release.yml`** — on a `v*` tag: `tag` (validate the tag, versions must match) → `verifica` (the whole verify on the tag) → `costruisci` (Windows, no cache, `--locked`, installer built **without** the signing key and without updater artifacts) → `firma` (a clean runner that compiles nothing: `npm ci --ignore-scripts`, `firma.js`, `tauri signer sign --app-version`, `manifesto.js`) → `pubblica` (the only job with `contents: write`) creating a **draft** release.",
            "**`sito.yml`** — on `release: published`, or by hand: downloads that release's installer, builds `_site/` from `site/` with `strumenti/sito.js --versione <tag> --installer <folder>` and publishes it to GitHub Pages. The version is the tag of the published release, not the one in the code, so the site never announces — or links — an installer that does not exist yet.",
          ],
        },
        "The release starts as a draft on purpose: `/releases/latest` skips drafts, so no installed copy sees an update until a person publishes it.",
      ],
      see: ["signing", "tooling"],
    },
    {
      id: "signing",
      title: "Update signing",
      src: ["strumenti/manifesto.js", "strumenti/firma.js", "apps/desktop/src-tauri/tauri.conf.json", ".github/workflows/release.yml", "README.md"],
      body: [
        "Updates are signed with a minisign key pair generated once (`npm run tauri signer generate -- -w ../../.chiavi/rymembra.key`; `.chiavi/` is git-ignored). The public key is `plugins.updater.pubkey` in `tauri.conf.json`; the private key and its password are the repository secrets `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD`. Losing the private key means no installed copy can ever update again.",
        "Since 3.0 the signature is bound to the version (`tauri signer sign --app-version <version>`), and `requireSignedVersion: true` makes the updater refuse a signature that does not match the version the manifest announces — a tampered manifest cannot pair a new version number with an older installer. `manifesto.js` checks that the key id in the signature matches the public key compiled into the app before publishing, and stops if a signature lacks the version. Code signing for SmartScreen is not done (no certificate).",
      ],
      see: ["update", "ci"],
    },
    {
      id: "writing-docs",
      title: "Writing these docs",
      src: ["site/docs/indice.json", "site/docs/pagina.html", "site/docs/docs.js", "site/docs/docs.css", "site/docs/contenuto", "strumenti/docs.js", "strumenti/sito.js", "apps/desktop/src-tauri/src/comandi.rs"],
      body: [
        "These pages are static HTML built by `strumenti/sito.js`: one page per chapter (`/docs/<chapter>/`), in the order of `site/docs/indice.json`, from the chapter files in `site/docs/contenuto/`, laid out by `site/docs/pagina.html`. Each chapter file pushes `{ id, title, lead, entries }` into `window.RYMEMBRA_DOCS`; an entry is `{ id, title, src, body, see }`. `docs.js` only adds the search (over `ricerca.json`, fetched the first time you type), the entry being read in the index, and the menu on a phone.",
        {
          table: {
            head: ["Block", "Renders as"],
            rows: [
              ["a string", "paragraph"],
              ["`{ h }`", "sub-heading"],
              ["`{ note }`", "call-out"],
              ["`{ ul }` / `{ ol }`", "list"],
              ["`{ code }`", "preformatted block (no inline markup)"],
              ["`{ table: { head, rows } }`", "table, inline markup in cells"],
            ],
          },
        },
        "Inline markup: `**bold**`, `*italic*`, backticks for code, `[text](https://…)` and `[text](#entry-id)`. `src` lists repository paths shown as the entry's sources.",
        "`node strumenti/docs.js` (also in `npm run verify` and CI) fails if a chapter file is not listed in `indice.json`, an id repeats, a block has an unknown shape, a table row has the wrong width, bold or code is unbalanced, a `see` or `#` link points nowhere, **a `src` path does not exist in the repository**, or an anchor of `ANCORE_GUIDA` has no entry. That list in `comandi.rs` is what the «?» on each Settings card may open (`apri_guida`, always `…/docs/#<entry>`): a script in the head of `/docs/` sends the anchor on to the chapter page that holds it, and `strumenti/sito.js` checks that every one of them arrives. Renaming a source file or an entry id is caught here.",
      ],
      see: ["tooling", "repository"],
    },
  ],
});
