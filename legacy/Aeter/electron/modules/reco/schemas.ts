import { z } from 'zod'

// Runtime validation of recommendation API payloads. Unknown keys are stripped
// (keeps cached payloads small). No electron imports — unit-tested in isolation.

// --- ListenBrainz labs: similar-recordings (keyless) ---
// GET https://labs.api.listenbrainz.org/similar-recordings/json?recording_mbids=<mbid>&algorithm=<algo>
// Returns a top-level array of rows; the reference row (the seed itself) carries
// a `comment` field, the similar rows carry recording_mbid + a numeric score.
export const LbSimilarRowSchema = z.object({
  recording_mbid: z.string(),
  score: z.number().nullish(),
  recording_name: z.string().nullish(),
  artist_credit_name: z.string().nullish(),
  // present only on the reference (seed) row — used to skip it
  comment: z.string().nullish()
})
export const LbSimilarSchema = z.array(LbSimilarRowSchema)
export type LbSimilarRow = z.infer<typeof LbSimilarRowSchema>

// --- ListenBrainz: LB Radio (keyless) ---
// GET https://api.listenbrainz.org/1/explore/lb-radio?prompt=<prompt>&mode=easy|medium|hard
// Returns a JSPF playlist; each track's `identifier` is the recording MBID URL
// (string or array of strings), `title`/`creator` are the recording/artist names.
const JspfTrackSchema = z.object({
  identifier: z.union([z.string(), z.array(z.string())]).nullish(),
  title: z.string().nullish(),
  creator: z.string().nullish()
})
export const LbRadioSchema = z.object({
  payload: z
    .object({
      jspf: z
        .object({
          playlist: z.object({ track: z.array(JspfTrackSchema).nullish() }).nullish()
        })
        .nullish()
    })
    .nullish()
})
export type LbRadio = z.infer<typeof LbRadioSchema>

// --- Last.fm: track.getSimilar / artist.getSimilar (free API key, no user auth) ---
// `match` comes back as a stringified float; coerce to number.
const matchNum = z.coerce.number().catch(0)

export const LastfmSimilarTracksSchema = z.object({
  similartracks: z
    .object({
      track: z
        .array(
          z.object({
            name: z.string(),
            mbid: z.string().nullish(),
            match: matchNum.nullish(),
            artist: z.object({ name: z.string(), mbid: z.string().nullish() }).nullish()
          })
        )
        .nullish()
    })
    .nullish()
})
export type LastfmSimilarTracks = z.infer<typeof LastfmSimilarTracksSchema>

export const LastfmSimilarArtistsSchema = z.object({
  similarartists: z
    .object({
      artist: z
        .array(z.object({ name: z.string(), mbid: z.string().nullish(), match: matchNum.nullish() }))
        .nullish()
    })
    .nullish()
})
export type LastfmSimilarArtists = z.infer<typeof LastfmSimilarArtistsSchema>
