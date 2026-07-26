/**
 * App version injected at build time via the `define` block of
 * electron.vite.config.ts (main process). `undefined` in environments without
 * the define (e.g. vitest) — always read it behind a `typeof` guard, which is
 * safe on an undeclared identifier.
 */
declare const __APP_VERSION__: string | undefined
