import { z } from 'zod'

// Runtime validation of external API payloads. Unknown keys are stripped,
// which also keeps cached payloads small. No electron imports (unit-tested).

export const AcoustidLookupSchema = z.object({
  results: z
    .array(
      z.object({
        score: z.number(),
        recordings: z.array(z.object({ id: z.string() })).optional()
      })
    )
    .optional()
})
export type AcoustidLookup = z.infer<typeof AcoustidLookupSchema>

export const MbRecordingSchema = z.object({
  id: z.string(),
  title: z.string(),
  score: z.number().optional(),
  /** Recording length in milliseconds (used for duration matching). */
  length: z.number().nullish(),
  'artist-credit': z.array(z.object({ name: z.string() })).optional(),
  releases: z
    .array(
      z.object({
        id: z.string(),
        title: z.string(),
        date: z.string().optional(),
        status: z.string().nullish(),
        'release-group': z
          .object({ id: z.string(), 'primary-type': z.string().nullish() })
          .nullish(),
        media: z
          .array(
            z.object({
              'track-offset': z.number().optional(),
              position: z.number().optional()
            })
          )
          .optional()
      })
    )
    .optional()
})
export type MbRecording = z.infer<typeof MbRecordingSchema>

export const MbSearchSchema = z.object({
  recordings: z.array(MbRecordingSchema).optional()
})
export type MbSearch = z.infer<typeof MbSearchSchema>

export const LastfmTopTagsSchema = z.object({
  toptags: z
    .object({
      tag: z
        .array(z.object({ name: z.string(), count: z.number().optional() }))
        .optional()
    })
    .optional()
})
export type LastfmTopTags = z.infer<typeof LastfmTopTagsSchema>

export const LrclibGetSchema = z.object({
  syncedLyrics: z.string().nullish(),
  plainLyrics: z.string().nullish()
})
export type LrclibGet = z.infer<typeof LrclibGetSchema>
