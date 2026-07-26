import { lazy, Suspense } from 'react'
import { Routes, Route, Navigate } from 'react-router-dom'
import Sidebar from '@/components/layout/Sidebar'
import BottomNav from '@/components/layout/BottomNav'
import LanOfflineBanner from '@/components/layout/LanOfflineBanner'
import BackendDownBanner from '@/components/layout/BackendDownBanner'
import { useRouteTransition } from '@/components/layout/RouteTransitions'
import PlayerBar from '@/components/player/PlayerBar'
import NowPlaying from '@/components/player/NowPlaying'
import FullscreenVisualizer from '@/components/player/FullscreenVisualizer'
import SleepSheet from '@/components/player/SleepSheet'
import SearchOverlay from '@/components/library/SearchOverlay'
import ToastHost from '@/components/ui/ToastHost'
import ErrorBoundary from '@/components/ui/ErrorBoundary'
import Home from '@/pages/Home'
import Library from '@/pages/Library'
import Albums from '@/pages/Albums'
import AlbumDetail from '@/pages/AlbumDetail'
import ArtistDetail from '@/pages/ArtistDetail'
import Playlists from '@/pages/Playlists'
import PlaylistDetail from '@/pages/PlaylistDetail'
import Liked from '@/pages/Liked'
import Podcasts from '@/pages/Podcasts'
import PodcastDetail from '@/pages/PodcastDetail'
import Settings from '@/pages/Settings'
import PairDevice from '@/pages/PairDevice'
import { useAppBootstrap } from '@/hooks/useAppBootstrap'
import { useAccentColor } from '@/hooks/useAccentColor'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { useAndroidBackButton } from '@/hooks/useAndroidBackButton'
import { useAutoRadio } from '@/hooks/useAutoRadio'
import { useUiStore } from '@/store/useUiStore'
import { useTourStore } from '@/store/useTourStore'
import { isMobile } from '@/lib/platform'
import { isLanModeActive } from '@/lib/lanClient'

// Superfici mount-on-open: chunk separati caricati alla prima apertura (file
// locali ⇒ latenza ~0), fuori dal bundle di avvio. Ognuna è montata qui SOLO
// quando il suo flag è attivo, altrimenti React.lazy scaricherebbe comunque il
// chunk al primo render (i componenti internamente fanno solo `return null`).
const QueueDrawer = lazy(() => import('@/components/player/QueueDrawer'))
const EqualizerPanel = lazy(() => import('@/components/player/EqualizerPanel'))
const LyricsScreen = lazy(() => import('@/components/player/LyricsScreen'))
const MetadataEditor = lazy(() => import('@/components/library/MetadataEditor'))
const BatchMetadataEditor = lazy(() => import('@/components/library/BatchMetadataEditor'))
const LyricsEditor = lazy(() => import('@/components/player/LyricsEditor'))
const TourOverlay = lazy(() => import('@/components/onboarding/TourOverlay'))
const Stats = lazy(() => import('@/pages/Stats'))

export default function App(): React.JSX.Element {
  useAppBootstrap()
  useAccentColor()
  useKeyboardShortcuts()
  useAndroidBackButton()
  useAutoRadio()
  const fullscreenViz = useUiStore((s) => s.fullscreenViz)
  const queueOpen = useUiStore((s) => s.queueOpen)
  const eqOpen = useUiStore((s) => s.eqOpen)
  const lyricsOpen = useUiStore((s) => s.lyricsOpen)
  const editTrackId = useUiStore((s) => s.editTrackId)
  const batchEditTrackIds = useUiStore((s) => s.batchEditTrackIds)
  const lyricsEditTrackId = useUiStore((s) => s.lyricsEditTrackId)
  const tourActive = useTourStore((s) => s.active)
  const displayedLocation = useRouteTransition()

  return (
    <div className="app-shell relative flex h-full flex-col">
      <div className="ambient-backdrop" />

      {/* Frameless window drag strip (desktop only) */}
      {!isMobile && <div className="drag-region absolute top-0 left-0 right-0 h-9 z-40" />}

      <div className="relative z-10 flex min-h-0 flex-1">
        {!isMobile && <Sidebar />}
        <main
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
          style={{ containerType: 'inline-size', containerName: 'content' }}
        >
          {/* Page-level boundary: a route crash keeps the player and sidebar alive */}
          <ErrorBoundary resetKey={displayedLocation.pathname}>
            <Routes location={displayedLocation}>
              <Route path="/" element={<Navigate to="/home" replace />} />
              <Route path="/home" element={<Home />} />
              <Route path="/library" element={<Library />} />
              <Route path="/albums" element={<Albums />} />
              <Route path="/albums/:id" element={<AlbumDetail />} />
              {/* no artist grid; the detail stays reachable from tracks/search */}
              <Route path="/artists" element={<Navigate to="/library" replace />} />
              <Route path="/artists/:name" element={<ArtistDetail />} />
              <Route path="/playlists" element={<Playlists />} />
              <Route path="/playlists/:id" element={<PlaylistDetail />} />
              <Route path="/liked" element={<Liked />} />
              <Route
                path="/stats"
                element={
                  <Suspense
                    fallback={
                      <div className="flex flex-col gap-2 p-6">
                        {Array.from({ length: 8 }).map((_, i) => (
                          <div key={i} className="skeleton h-10" />
                        ))}
                      </div>
                    }
                  >
                    <Stats />
                  </Suspense>
                }
              />
              {/* The LAN bridge has no podcast methods: BottomNav already hides
                  the tab, but the route stays reachable via deep links — send
                  it home instead of rendering a page of rejected calls. */}
              <Route path="/podcasts" element={isLanModeActive() ? <Navigate to="/home" replace /> : <Podcasts />} />
              <Route
                path="/podcasts/:id"
                element={isLanModeActive() ? <Navigate to="/home" replace /> : <PodcastDetail />}
              />
              <Route path="/settings" element={<Settings />} />
              <Route path="/settings/pair-device" element={<PairDevice />} />
            </Routes>
          </ErrorBoundary>
        </main>
      </div>

      <Suspense fallback={null}>{queueOpen && <QueueDrawer />}</Suspense>
      <BackendDownBanner />
      <LanOfflineBanner />
      <PlayerBar />
      {isMobile && <NowPlaying />}
      {isMobile && <Suspense fallback={null}>{lyricsOpen && <LyricsScreen />}</Suspense>}
      {isMobile && <SleepSheet />}
      {isMobile && <BottomNav />}

      <SearchOverlay />
      <Suspense fallback={null}>
        {eqOpen && <EqualizerPanel />}
        {editTrackId != null && <MetadataEditor />}
        {batchEditTrackIds != null && <BatchMetadataEditor />}
      </Suspense>
      {fullscreenViz && <FullscreenVisualizer />}
      {/* after the visualizer so the editor stays usable in fullscreen */}
      <Suspense fallback={null}>
        {lyricsEditTrackId != null && <LyricsEditor />}
        {tourActive && <TourOverlay />}
      </Suspense>
      <ToastHost />
    </div>
  )
}
