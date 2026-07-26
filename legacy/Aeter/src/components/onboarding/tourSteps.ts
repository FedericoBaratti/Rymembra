export type TourPlacement = 'top' | 'bottom' | 'left' | 'right'

export interface TourStep {
  /** i18n key segment: tour.<id>.title / tour.<id>.body */
  id: string
  /** Navigate here before measuring the target */
  route?: string
  /** data-tour attribute of the highlighted element; absent = centered step */
  target?: string
  placement?: TourPlacement
  /** Breathing room around the spotlight hole, px */
  padding?: number
  /** Spotlight hole border-radius, px */
  radius?: number
}

export const TOUR_STEPS: TourStep[] = [
  { id: 'welcome' },
  { id: 'sidebar', route: '/library', target: 'sidebar-nav', placement: 'right' },
  { id: 'library', route: '/library', target: 'library-page', placement: 'bottom', padding: 0, radius: 0 },
  { id: 'search' },
  { id: 'player', target: 'player-controls', placement: 'top' },
  { id: 'player_extras', target: 'player-extras', placement: 'top' },
  { id: 'downloader', route: '/download', target: 'download-input', placement: 'bottom' },
  { id: 'settings', route: '/settings', target: 'settings-folders', placement: 'bottom' },
  { id: 'finish' }
]
