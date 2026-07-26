import { isMobile } from '@/lib/platform'

export type TourPlacement = 'top' | 'bottom' | 'left' | 'right'

/** Chrome panel a step needs open before its target can be measured. */
export type TourOpenUi = 'search' | 'nowPlaying' | 'queue' | 'eq'

export interface TourStep {
  /** i18n key segment: tour.<id>.title / tour.<id>.body */
  id: string
  /** Chapter this step belongs to: i18n key tour.sections.<section> */
  section: string
  /** Navigate here before measuring the target */
  route?: string
  /** data-tour attribute of the highlighted element; absent = centered step */
  target?: string
  /** Open this chrome panel before measuring (and close it on leave) */
  openUi?: TourOpenUi
  placement?: TourPlacement
  /** Breathing room around the spotlight hole, px */
  padding?: number
  /** Spotlight hole border-radius, px */
  radius?: number
}

/**
 * Touch-first tour for the Android port. Only covers features actually
 * reachable by touch (no Ctrl+F search, no sleep timer / fullscreen visualizer
 * / synced lyrics — those live in the desktop PlayerBar only). Organised into
 * chapters (sections) so the overlay can show a jumpable index.
 */
const MOBILE_STEPS: TourStep[] = [
  { id: 'welcome', section: 'intro' },
  { id: 'nav', section: 'navigate', route: '/library', target: 'sidebar-nav', placement: 'top' },
  { id: 'library', section: 'library', route: '/library', target: 'library-page', placement: 'top', padding: 0, radius: 0 },
  { id: 'search', section: 'library', route: '/library', target: 'search-button', placement: 'bottom' },
  { id: 'albums', section: 'collections', route: '/albums', target: 'albums-grid', placement: 'top', padding: 0, radius: 0 },
  { id: 'playlists', section: 'collections', route: '/playlists', target: 'playlist-actions', placement: 'bottom' },
  { id: 'mini_player', section: 'player', target: 'mini-player', placement: 'top' },
  { id: 'now_playing', section: 'player', openUi: 'nowPlaying', target: 'np-transport', placement: 'top' },
  { id: 'player_tools', section: 'player', openUi: 'nowPlaying', target: 'np-tools', placement: 'bottom' },
  { id: 'downloader', section: 'get_music', route: '/settings', target: 'settings-youtube', placement: 'top' },
  { id: 'spotify_import', section: 'get_music', route: '/settings', target: 'settings-spotify', placement: 'top' },
  { id: 'enrichment', section: 'smart_meta', route: '/settings', target: 'settings-enrichment', placement: 'top' },
  { id: 'duplicates', section: 'smart_meta', route: '/settings', target: 'settings-duplicates', placement: 'top' },
  { id: 'appearance', section: 'personalize', route: '/settings', target: 'settings-appearance', placement: 'top' },
  { id: 'integrations', section: 'personalize', route: '/settings', target: 'settings-integrations', placement: 'top' },
  { id: 'folders', section: 'personalize', route: '/settings', target: 'settings-folders', placement: 'top' },
  { id: 'finish', section: 'done' }
]

/** Legacy desktop tour (kept so the Electron build doesn't regress). */
const DESKTOP_STEPS: TourStep[] = [
  { id: 'welcome', section: 'intro' },
  { id: 'sidebar', section: 'navigate', route: '/library', target: 'sidebar-nav', placement: 'right' },
  { id: 'library', section: 'library', route: '/library', target: 'library-page', placement: 'bottom', padding: 0, radius: 0 },
  { id: 'search', section: 'library' },
  { id: 'player', section: 'player', target: 'player-controls', placement: 'top' },
  { id: 'player_extras', section: 'player', target: 'player-extras', placement: 'top' },
  { id: 'downloader', section: 'get_music', route: '/settings', target: 'settings-youtube', placement: 'top' },
  { id: 'settings', section: 'personalize', route: '/settings', target: 'settings-folders', placement: 'bottom' },
  { id: 'finish', section: 'done' }
]

/** Active step list for the current platform. */
export function getTourSteps(): TourStep[] {
  return isMobile ? MOBILE_STEPS : DESKTOP_STEPS
}

export interface TourSection {
  /** i18n key segment: tour.sections.<id> */
  id: string
  /** Index of the first step in this section (for jump-to-chapter) */
  firstStep: number
}

/** Ordered chapters derived from the active step list. */
export function getTourSections(): TourSection[] {
  const steps = getTourSteps()
  const out: TourSection[] = []
  steps.forEach((s, i) => {
    if (!out.some((sec) => sec.id === s.section)) out.push({ id: s.section, firstStep: i })
  })
  return out
}
