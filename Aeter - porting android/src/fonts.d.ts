// The @fontsource packages ship pure CSS with no type declarations. Static
// side-effect imports slip through tsc, but the dynamic import() calls that
// lazy-load the skin fonts (SKIN_FONTS in lib/skins.ts) need these ambient
// module declarations to resolve.
declare module '@fontsource/*'
declare module '@fontsource-variable/*'
