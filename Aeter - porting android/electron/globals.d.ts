/**
 * AcoustID application API key embedded at build time so audio-fingerprint
 * enrichment is keyless for end users. Injected via the `define` block of
 * electron.vite.config.ts (desktop main) and vite.config.node-backend.ts
 * (Android backend), sourced from the ACOUSTID_APP_KEY build env var.
 *
 * Empty string when no build-time key was provided; `undefined` in
 * environments without the define (e.g. vitest) — always read it behind a
 * `typeof` guard, which is safe on an undeclared identifier.
 */
declare const __ACOUSTID_APP_KEY__: string | undefined

/**
 * App version injected at build time via the `define` block of
 * vite.config.node-backend.ts (and the desktop main config in the desktop
 * tree). `undefined` where the define is absent (e.g. vitest) — read behind a
 * `typeof` guard.
 */
declare const __APP_VERSION__: string | undefined
