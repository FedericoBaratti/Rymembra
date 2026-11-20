/* Capitolo 14 — costruire, provare, rilasciare: comandi, prove, strumenti, CI, firma, e come si scrive questa documentazione. */
(window.RYMEMBRA_DOCS = window.RYMEMBRA_DOCS || []).push({
  id: "build",
  title: "Building, testing, releasing",
  lead: "The commands, the test suites, the repository checks in `strumenti/`, the CI and release pipeline, update signing, and how this documentation is written and checked.",
  entries: [
    {
      id: "building",
      title: "Building and running",
      src: ["apps/desktop/package.json", "Cargo.toml", "rust-toolchain.toml", "apps/desktop/src-tauri/build.rs", "apps/desktop/vite.config.ts", "strumenti/msix.js", "apps/desktop/src-tauri/msix/LEGGIMI.md", "README.md"],
      body: [
        "Requirements: Rust stable ≥ 1.91 (`rust-version`; CI pins 1.97.1), Node.js 24 (the web tests use Node’s own type stripping), Windows for the installer — its only Tauri target is NSIS — and, for the APK, the Android SDK and NDK. The Microsoft Store package (MSIX) is not a Tauri target: `strumenti/msix.js` builds it ([Repository checks](#tooling)).",
        {
          table: {
            head: ["Command (from `apps/desktop`)", "What"],
            rows: [
              ["`npm install`, `npm run dev`", "Vite on port 5173 (`strictPort`) + `tauri dev`"],
              ["`npx tauri build --config '{\"bundle\":{\"createUpdaterArtifacts\":false}}'`", "The NSIS installer, whose hook `src-tauri/installer/precedente.nsh` removes the previous version. `npm run build` is plain `tauri build`, which with `createUpdaterArtifacts` on wants to sign the update and stops without `TAURI_SIGNING_PRIVATE_KEY`; the override is the one `release.yml` uses ([Update signing](#signing))"],
              ["`npx tauri android build --apk --target aarch64 --debug`", "A debug APK for 64-bit phones, as the `android` job of CI builds it ([CI](#ci)). Needs the Android SDK and NDK and, on Windows, Developer Mode for the symbolic link the Tauri CLI makes, or the workaround in the README (*Known limits → Android → Builds*). The release APK is built and signed only by `release.yml`"],
              ["`npx tauri build --no-bundle -- --locked`, then `node strumenti/msix.js` (root)", "The MSIX package for the Microsoft Store, in `target/msix/`. The executable is the `--no-bundle` one, because a bundled build writes the installer's kind into it for the updater. Needs `makeappx` and `makepri` from the Windows SDK; `--solo-cartella` stops at the unpacked folder, which is how it is tried before the Store has signed it"],
              ["`npm run verify`", "Most of what the `verifica` job of CI checks, offline, with your own toolchain: versions, translations, docs, the website build, CSS classes, `tsc` + Vite build, web tests, tool tests, `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked`, `cargo test --workspace --locked`"],
              ["`npm run verify:ci`", "The Rust part as CI runs it: the toolchain pinned in `verify.yml` and `RUSTFLAGS=-D warnings`, so what CI would turn into an error is seen here first"],
              ["`npm run version:set <x.y.z>`", "Rewrites the version in `Cargo.toml`, `package.json`, `tauri.conf.json`, `Cargo.lock` (the eighteen workspace members) and `package-lock.json` (its root and `packages[\"\"]`); refuses suffixes, and a minor or patch above 999 — the APK `versionCode` is major·10⁶ + minor·10³ + patch"],
              ["`npm run licenze`", "Regenerates `THIRD-PARTY-NOTICES.md`"],
              ["`cargo deny check` (root)", "Licenses, advisories, sources of Rust dependencies"],
            ],
          },
        },
        "`build.rs` generates the Tauri context and links the VC++ runtime statically in release builds only (static CRT broke doctests). It passes no credentials: since 3.2 no OAuth client is compiled in for any service — the `RYMEMBRA_GOOGLE_CLIENT_ID` / `_SECRET` variables and `oauth.local.toml` are no longer read, and every user registers their own app ([Cloud folders](#cloud-folders)). Running the app on a copy of the data: set `RYMEMBRA_DATI` to another folder ([Data](#data)).",
      ],
      see: ["testing", "tooling", "ci"],
    },
    {
      id: "testing",
      title: "Tests",
      src: ["core/rymembra-domain/tests", "core/rymembra-skin/tests", "core/rymembra-sync/tests/convergenza.rs", "apps/desktop/test", "apps/desktop/test/risolvi.mjs", "strumenti/docs.test.js"],
      body: [
        "More than 4,000 Rust tests and about 900 web tests (151 files) run in `npm run verify`, all without network access: the «Listen together» tests open sockets only on 127.0.0.1, never on all interfaces.",
        {
          ul: [
            "**Unit tests** live beside the code (`mod tests` / `mod prove`) — most modules prove the specific bug their doc comment describes.",
            "**Golden parity** (`core/rymembra-domain/tests/parita_*.rs`): text folding and keys, whole scan plans and album grouping compared with outputs recorded from the previous TypeScript implementation (`tests/golden/*.json`). Changing one of these behaviors means a deliberate, documented divergence.",
            "**Skins**: `fedelta.rs` (plain vs the old stylesheet), `migrazione.rs` (format 1 goldens in `tests/oro/`), `attivazione.rs` (flash rule, budgets, install exam).",
            "**Sync convergence** (`rymembra-sync/tests/convergenza.rs`): seeded random histories merged in every order must converge.",
            "**Database**: migrations from version 0 on in-memory databases, schema checks, SQLite version floor (3.53.2), error translation built from synthetic SQLite codes.",
            "**Audio**: decoding real fixture files (FLAC, Opus, …), gapless seams across sample rates, crossfade power conservation — as pure functions or with the engine on test streams.",
            "**Web** (`apps/desktop/test`, 111 files): `node --test` over `.ts` files with a resolve hook (`risolvi.mjs`); they test the DOM-free modules (selection, sparse lists, modal rules, tour rules, Studio patch/history/prompt, …).",
          ],
        },
        "Some old file tests point at `Z:/non/esiste`, so on a machine where `Z:` is a mapped network drive that is off, `cargo test` waits for SMB timeouts.",
      ],
      see: ["building", "ci"],
    },
    {
      id: "tooling",
      title: "Repository checks (strumenti)",
      src: ["strumenti/classi.js", "strumenti/lingue.js", "strumenti/docs.js", "strumenti/sito.js", "strumenti/json.js", "strumenti/versione.js", "strumenti/licenze.js", "strumenti/manifesto.js", "strumenti/minisign.js", "strumenti/firma.js", "strumenti/apk.js", "strumenti/android-certificato.sha256", "strumenti/corpo.js", "strumenti/come-la-ci.js", "strumenti/finto-aggiornamento.js", "strumenti/msix.js"],
      body: [
        "Plain CommonJS scripts with no dependencies, paths resolved from `__dirname`:",
        {
          table: {
            head: ["Script", "Checks or does"],
            rows: [
              ["`classi.js`", "Every class in TSX exists in one of the sheets (`stile.css` or `stile/*.css`) or the skin registry; every registry part is emitted by the app; no dead CSS; every `z-index` is a layer token; every duration goes through `--motion-scale`; full-cover positioned layers declare `pointer-events`"],
              ["`lingue.js`", "Every language has exactly the keys of `it.json`; plural forms per CLDR category; every error code has an `errors.<code>` sentence (`--scrivi` fills missing keys); and the texts themselves — same placeholders everywhere, the quotes and spelling of each language, quoted labels that exist, no `1 lines`, no dead keys (exceptions with a reason in `lingue-eccezioni.json`)"],
              ["`docs.js`", "This documentation — see [Writing these docs](#writing-docs)"],
              ["`sito.js`", "Builds the website into `_site/` from `site/` — the landing page in four languages from one template and `site/testi/*.json`, one page per docs chapter, the sitemap, and copies the soundtrack in `site/audio/` (“Machina” by Scott Buckley, CC BY 4.0, fetched only when played) — then checks the result: the same keys in every language (a repeated key is an error, not the last one silently winning), the same placeholders and links in every translated text, no text left unused, every internal link, `src`, `data-src`, the search index and every anchor resolves, every `ANCORE_GUIDA` anchor reaches an entry, nothing loaded from another domain — any element's `src`, `poster`, `data` or `srcset`, CSS `url()` and `@import`, URLs in the copied JavaScript (`--controlla` builds in a temporary folder; `--versione` sets the release to announce; `--installer` points at the downloaded installer, whose size and SHA-256 the download section then shows; `--apk` does the same for the release's APK, which adds the phone's row — without it, as for a release with no APK, the page offers the computer only)"],
              ["`json.js`", "Reads a JSON text file refusing a key repeated in the same object, which `JSON.parse` would silently resolve to the last one"],
              ["`versione.js`", "The version agrees across `Cargo.toml`, `package.json`, `tauri.conf.json`, `Cargo.lock` and `package-lock.json`, every member inherits it, and it fits the APK `versionCode`; or rewrites it in all five"],
              ["`licenze.js`", "Generates `THIRD-PARTY-NOTICES.md` from `cargo metadata --locked` (Windows and Android targets) and production npm packages; a license-looking file above 256 KiB is listed as not reproduced and makes it exit with 1, instead of disappearing"],
              ["`manifesto.js`", "Writes the updater's `latest.json` (version, signature, installer URL, notes from the CHANGELOG), checks the signing key id and the signed version, and verifies the signature against the installer's bytes — the one in the bundle folder, the one about to be uploaded and, read back from disk, the one inside `latest.json`. The notes are the `###` titles of the version's section — `Security` ones first, then in file order, at most eight, without «The number», «Database» and «Known limits»"],
              ["`minisign.js`", "Verifies a minisign signature against bytes with Node's own `crypto` — BLAKE2b-512 of the file (algorithm `ED`) and Ed25519, plus the global signature over the trusted comment — the way the updater plugin and the phone do. Exact lengths, same key id, `ED` only; it returns `{ ok, motivo }` and never throws. `manifesto.js` and `apk.js` call it, so a `.sig` that is not of the file being published stops the release instead of being discovered by every installed copy. Its tests build signatures with a throwaway key (`firma-di-prova.js`) and read the real 3.2.0 signature against the repository's public key"],
              ["`apk.js`", "The APK, before and after signing (`controlla`, `verifica`), with `zipalign`, `aapt2` and `apksigner` from `ANDROID_BUILD_TOOLS`: every `LOAD` segment of every `lib/arm64-v8a/*.so` aligned to 16 KB and no other ABI; package, `versionName` and `versionCode` (major·10⁶ + minor·10³ + patch) right and not debuggable; R8's `mapping.txt` still has `Ponte.chiama` and the names Rust and wry call through JNI, and has not renamed any `native` function of `Ponte` (the list comes from the `external fun` in `Ponte.kt`, not from a hand-written one; one the mapping does not list at all is a notice, not an error); then one signer, not the debug key, the certificate's SHA-256 equal to `android-certificato.sha256`, and the `.apk.sig` made with the updater key for this version and this file and valid for the APK's bytes. `nome` prints the asset name, `Rymembra_X.Y.Z_android-arm64.apk`, the one `rilasci.rs::apk_per` picks; `segreti` checks the Android signing secrets from the environment before any build (see [CI and the release pipeline](#ci))"],
              ["`corpo.js`", "Writes the body of the GitHub release: first the section for whoever has a test build of the Android app (export the profile, uninstall, install, let Rymembra read your music, and only then read the profile — what the profile says about a track attaches only to a track the library has; what doesn't carry over), then Windows, then Android. The menu names come from `en.json`"],
              ["`come-la-ci.js`", "`npm run verify:ci`: fmt, clippy and tests with the toolchain and `RUSTFLAGS` read from `verify.yml`, and `--locked`, as CI runs them"],
              ["`firma.js`", "Pre-flight of the signing secret (decodes, has the minisign header, key and password without stray whitespace at the edges — an empty or missing one passes, as for a key generated without a password); never prints it. Runs in `segreti`, at the start of the release, and again before each signature"],
              ["`finto-aggiornamento.js`", "A fake GitHub on `127.0.0.1:8787` to test updates end to end (`--guasta-firma` for the negative case), with the debug-only `RYMEMBRA_AGGIORNAMENTI_ENDPOINT`"],
              ["`msix.js`", "Builds the Microsoft Store package: the `--no-bundle` executable, the resources listed in `bundle.resources`, the logos in `msix/Assets`, the marker `rymembra-store.txt` that turns off the update check ([Updates](#update)), and `AppxManifest.xml` with the identity Partner Center assigned — from `msix/identita.json`, `--nome`/`--editore`/`--nome-editore` or `MSIX_NOME`/`MSIX_EDITORE`/`MSIX_NOME_EDITORE` — refusing the example values. The version is `tauri.conf.json`'s with a fourth digit at zero (`3.1.0.0`); `makepri` indexes the logos and `makeappx` packs and validates. The package is left unsigned: the Store signs it after certification"],
            ],
          },
        },
        "Each tool has its `*.test.js` beside it (`npm run test:strumenti`), run on real or sampled outputs; `workflow.test.js`, `installatore.test.js` and `segnalazioni.test.js` read the workflows, the NSIS hook and the issue form as text and pin what must not go back (one toolchain everywhere, pinned actions, the APK job without secrets and its signing in the `release` environment, the installer's languages, the Aether check).",
      ],
      see: ["ci", "writing-docs", "language"],
    },
    {
      id: "ci",
      title: "CI and the release pipeline",
      src: [".github/workflows/verify.yml", ".github/workflows/release.yml", ".github/workflows/sito.yml", ".github/workflows/msix.yml", "deny.toml"],
      body: [
        "Every action is pinned to a commit hash (tags can be moved; see the March 2025 `tj-actions` incident), with the version in a comment.",
        {
          ul: [
            "**`verify.yml`** — on push, pull request, manual, or called by `release.yml` and `msix.yml`. Job `verifica` (Windows, 60 minutes, toolchain 1.97.1, `RUSTFLAGS=-D warnings`; `npm run verify:ci` runs the Rust part the same way locally): versions, translations, CSS classes, docs, the website build, tool tests, fmt, clippy and tests with `--locked`, web build and tests, and a check that `THIRD-PARTY-NOTICES.md` is up to date. Job `android` (Ubuntu, 75 minutes, the same toolchain): clippy for `aarch64-linux-android` on `rymembra-desktop` and `rymembra-android`, `tauri android build --apk --target aarch64 --debug`, and the Gradle lint. Job `dipendenze`: `cargo deny check` with `cargo-deny` 0.20.2 built from crates.io by `cargo install --locked` (not a container action that downloaded an unchecked binary; Windows and Android targets, from `deny.toml`; license allow-list without GPL/LGPL; yanked crates denied; crates.io only) and `npm audit --omit=dev`. Licenses, bans and sources always block; advisories (yanked crates included) and `npm audit` block on branches but are only reported when `release.yml` or `msix.yml` calls with `consultivo: true` — an advisory published on the day of the tag would otherwise stop a run that «Re-run» cannot restart, since the tag and its `deny.toml` do not change — and the outcome goes up as the `avvisi` output. Job `niente-di-vietato`: fails on bundled binaries, on code anywhere in `core` and `apps/desktop` — Kotlin included — that names YouTube downloaders or lyrics scrapers, and on missing public documents.",
            "**`release.yml`** — on a `v*` tag, or by hand from the tag itself (*Use workflow from → Tags*, with the same tag in the `tag` field): `tag` (a run from a branch stops here; validate the tag, versions must match; write the release body with `corpo.js`; `novita.js --se-minor` stops the tag if a minor or major version has no Novità) → `segreti` (`release` environment; two minutes, nothing installed, nothing compiled: `firma.js` on the minisign key and password, and `apk.js segreti` on the three Android secrets — present, the keystore is base64 of a keystore, `keytool` opens it with that password and alias, and the certificate is the one in `android-certificato.sha256` — so a missing environment or a wrong secret shows up now and not after the builds) → `verifica` (the whole verify on the tag, advisories consultive) → two branches. Windows: `costruisci` (no cache, 90 minutes, `--locked`, installer built **without** the signing key and without updater artifacts) → `firma` (`release` environment; a clean runner that compiles nothing: `npm ci --ignore-scripts`, `firma.js`, `tauri signer sign --app-version`, `manifesto.js`). Android: `apk` (`ubuntu-24.04`, no secrets, NDK 30.0.16248370 pinned through `sdkmanager` instead of the image's 27, `tauri android build --apk --split-per-abi --target aarch64`, R8 on; refuses a `keystore.properties`, checks the lockfile did not change and runs `apk.js controlla`; `mapping.txt` kept as a 90-day run artifact, not a release asset) → `firma-apk` (`release` environment: the keystore from `ANDROID_KEYSTORE_BASE64` written under `$RUNNER_TEMP` with `umask 077` and deleted in an `if: always()` step, passwords through `--ks-pass env:`, `apksigner sign` v2+v3 without the v4 `.idsig`, `tauri signer sign` for the `.apk.sig`, `apk.js verifica`). Then `pubblica` (the only job with `contents: write`) puts installer and APK in the same **draft** release with the generated body — headed by a note to whoever publishes if `verifica` found advisories — and stops if the tag's release is already published. `segreti`, `firma` and `firma-apk` are three approvals of the `release` environment: the first right after the tag, the other two when the builds are done.",
            "**`sito.yml`** — on `release: published`, or by hand. Two jobs: `costruisci` (`contents: read` only) downloads that release's installer and, if the release has one, its APK (checking the asset list first, so a network error never reads as «no APK»), builds `_site/` from `site/` with `strumenti/sito.js --versione <tag> --installer <folder> [--apk <folder>]` and uploads it as the Pages artifact; `pubblica` (`pages: write`, `id-token: write`, no checkout: no repository code runs there) publishes it to GitHub Pages. The version is the tag of the published release, not the one in the code, so the site never announces — or links — an installer that does not exist yet. After the upload `pubblica` fetches the home page from the published address until it is byte for byte the `index.html` just built, and fails if it never is: Pages can report a deployment as successful and keep serving the previous build, which is how the site went on announcing 3.0.2 for hours after 3.1.0 was published.",
            "**`msix.yml`** — by hand, on a tag that already exists: `tag` (the tag must match the versions) → `verifica` (the whole verify on that tag, advisories consultive as in `release.yml`: the outcome becomes a warning annotation at the top of `pacchetto`) → `pacchetto` (Windows, no cache, `tauri build --no-bundle -- --locked`, then `msix.js`), which uploads the unsigned `.msix` as the artifact `rymembra-msix`. No secrets and no write permission: the package goes to Partner Center by hand, and the Store signs it.",
          ],
        },
        "The release starts as a draft on purpose: `/releases/latest` skips drafts, so no installed copy sees an update until a person publishes it.",
      ],
      see: ["signing", "tooling"],
    },
    {
      id: "signing",
      title: "Update signing",
      src: ["strumenti/manifesto.js", "strumenti/firma.js", "strumenti/apk.js", "strumenti/android-certificato.sha256", "apps/desktop/src-tauri/tauri.conf.json", ".github/workflows/release.yml", "README.md"],
      body: [
        "Updates are signed with a minisign key pair generated once (`npm run tauri signer generate -- -w ../../.chiavi/rymembra.key`; `.chiavi/` is git-ignored). The public key is `plugins.updater.pubkey` in `tauri.conf.json`; the private key and its password are secrets of the GitHub environment `release`, `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD`, not repository secrets, which every workflow of every branch can read. The environment is configured on GitHub, not in the workflow: only tags `v*`, a required reviewer, and the secrets removed from the repository ones; only `segreti`, `firma` and `firma-apk` name it, and wait for the approval (`segreti` checks the five secrets two minutes after the tag; the other two sign). The same minisign key signs the APK (`….apk.sig`), which the phone verifies before handing it to Android's installer. Losing the private key means no installed copy can ever update again.",
        "Since 3.0 the signature is bound to the version (`tauri signer sign --app-version <version>`), and `requireSignedVersion: true` makes the updater refuse a signature that does not match the version the manifest announces — a tampered manifest cannot pair a new version number with an older installer. `manifesto.js` checks that the key id in the signature matches the public key compiled into the app before publishing, stops if a signature lacks the version, and verifies the signature itself against the installer's bytes (`minisign.js`; `apk.js verifica` does the same for the APK), so a signature that is not of the file being published never reaches a release. Code signing for SmartScreen is not done (no certificate). The APK is signed for Android with the release keystore (`ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` in the same environment) by `apksigner` in `firma-apk`, never by Gradle on the machine that compiled; its certificate's SHA-256 is pinned in `strumenti/android-certificato.sha256` and checked before publishing — Android accepts an update only from the certificate of the app already installed, so that key, like the minisign one, can never change. The Microsoft Store package carries none of this: the Store signs it after certification and keeps that copy up to date itself ([Updates](#update)).",
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
        "`node strumenti/docs.js` (also in `npm run verify` and CI) fails if a chapter file is not listed in `indice.json`, an id repeats, a block has an unknown shape, a table row has the wrong width, bold or code is unbalanced, a `see` or `#` link points nowhere, **a `src` path is not tracked in the repository** (checked against `git ls-files`, letter case included: Windows would accept `README.MD`, GitHub answers 404), or an anchor of `ANCORE_GUIDA` has no entry. That list in `comandi.rs` is what the «?» on each Settings card may open (`apri_guida`, always `…/docs/#<entry>`): a script in the head of `/docs/` sends the anchor on to the chapter page that holds it, and `strumenti/sito.js` checks that every one of them arrives. Renaming a source file or an entry id is caught here.",
      ],
      see: ["tooling", "repository"],
    },
  ],
});
