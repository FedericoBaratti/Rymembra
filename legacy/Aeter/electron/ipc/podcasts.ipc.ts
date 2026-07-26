import { handle } from './handle'
import {
  searchPodcasts,
  addPodcast,
  removePodcast,
  refreshPodcast,
  getPodcasts,
  getPodcastEpisodes,
  getLatestEpisodes,
  setEpisodeProgress
} from '../modules/podcasts/podcasts'

// Podcast subscriptions (RSS, keyless). Thin pass-throughs to modules/podcasts.
export function registerPodcastIpc(): void {
  handle('searchPodcasts', (_e, term: string) => searchPodcasts(term))
  handle('addPodcast', (_e, feedUrl: string) => addPodcast(feedUrl))
  handle('removePodcast', (_e, podcastId: number) => removePodcast(podcastId))
  handle('refreshPodcast', (_e, podcastId: number) => refreshPodcast(podcastId))
  handle('getPodcasts', () => getPodcasts())
  handle('getPodcastEpisodes', (_e, podcastId: number) => getPodcastEpisodes(podcastId))
  handle('getLatestEpisodes', (_e, limit?: number) => getLatestEpisodes(limit))
  handle('setEpisodeProgress', (_e, episodeId: number, progressSec: number, played: boolean) =>
    setEpisodeProgress(episodeId, progressSec, played)
  )
}
