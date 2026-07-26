// Re-export shim: the byte-identical shared sync module `sync/fetchMissing.ts`
// imports the by-metadata download helper from `../download/externalDownload`
// (its location in the desktop project). On the Android port the primitive lives
// in `reco/externalDownload.ts`, so this file just forwards it — keeping
// fetchMissing.ts identical across both projects.
export { downloadExternalTrack, type ExternalTrackMeta } from '../reco/externalDownload'
