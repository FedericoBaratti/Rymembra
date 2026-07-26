import { lazy, Suspense } from 'react'
import { Routes, Route, Navigate } from 'react-router-dom'
import Sidebar from '@/components/layout/Sidebar'
import { useRouteTransition } from '@/components/layout/RouteTransitions'
import PlayerBar from '@/components/player/PlayerBar'
import QueueDrawer from '@/components/player/QueueDrawer'
import EqualizerPanel from '@/components/player/EqualizerPanel'
import FullscreenVisualizer from '@/components/player/FullscreenVisualizer'
import SearchOverlay from '@/components/library/SearchOverlay'
import MetadataEditor from '@/components/library/MetadataEditor'
import BatchMetadataEditor from '@/components/library/BatchMetadataEditor'
import LyricsEditor from '@/components/player/LyricsEditor'
import TourOverlay from '@/components/onboarding/TourOverlay'
import ToastHost from '@/components/ui/ToastHost'
import ErrorBoundary from '@/components/ui/ErrorBoundary'
import Library from '@/pages/Library'
import Albums from '@/pages/Albums'
import AlbumDetail from '@/pages/AlbumDetail'
import Artists from '@/pages/Artists'
import ArtistDetail from '@/pages/ArtistDetail'
import Playlists from '@/pages/Playlists'
import PlaylistDetail from '@/pages/PlaylistDetail'
import Downloader from '@/pages/Downloader'
import PhoneSync from '@/pages/PhoneSync'
import Settings from '@/pages/Settings'
import { useAppBootstrap } from '@/hooks/useAppBootstrap'
import { useAccentColor } from '@/hooks/useAccentColor'
import { useAutoRadio } from '@/hooks/useAutoRadio'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { useUiStore } from '@/store/useUiStore'

// Discovery/podcast pages are lazy so the extra network-bound surfaces don't
// weigh on first paint of the core library UI.
const Home = lazy(() => import('@/pages/Home'))
const Liked = lazy(() => import('@/pages/Liked'))
const Stats = lazy(() => import('@/pages/Stats'))
const Podcasts = lazy(() => import('@/pages/Podcasts'))
const PodcastDetail = lazy(() => import('@/pages/PodcastDetail'))

export default function App(): React.JSX.Element {
  useAppBootstrap()
  useAccentColor()
  useAutoRadio()
  useKeyboardShortcuts()
  const fullscreenViz = useUiStore((s) => s.fullscreenViz)
  const displayedLocation = useRouteTransition()

  return (
    <div className="app-shell relative flex h-full flex-col">
      <div className="ambient-backdrop" />

      {/* Frameless window drag strip */}
      <div className="drag-region absolute top-0 left-0 right-0 h-9 z-40" />

      <div className="relative z-10 flex min-h-0 flex-1">
        <Sidebar />
        <main
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
          style={{ containerType: 'inline-size', containerName: 'content' }}
        >
          {/* Page-level boundary: a route crash keeps the player and sidebar alive */}
          <ErrorBoundary resetKey={displayedLocation.pathname}>
            <Suspense fallback={null}>
              <Routes location={displayedLocation}>
                <Route path="/" element={<Navigate to="/home" replace />} />
                <Route path="/home" element={<Home />} />
                <Route path="/library" element={<Library />} />
                <Route path="/albums" element={<Albums />} />
                <Route path="/albums/:id" element={<AlbumDetail />} />
                <Route path="/artists" element={<Artists />} />
                <Route path="/artists/:name" element={<ArtistDetail />} />
                <Route path="/playlists" element={<Playlists />} />
                <Route path="/playlists/:id" element={<PlaylistDetail />} />
                <Route path="/liked" element={<Liked />} />
                <Route path="/stats" element={<Stats />} />
                <Route path="/podcasts" element={<Podcasts />} />
                <Route path="/podcasts/:id" element={<PodcastDetail />} />
                <Route path="/download" element={<Downloader />} />
                <Route path="/phone" element={<PhoneSync />} />
                <Route path="/settings" element={<Settings />} />
              </Routes>
            </Suspense>
          </ErrorBoundary>
        </main>
      </div>

      <QueueDrawer />
      <PlayerBar />

      <SearchOverlay />
      <EqualizerPanel />
      <MetadataEditor />
      <BatchMetadataEditor />
      {fullscreenViz && <FullscreenVisualizer />}
      {/* after the visualizer so the editor stays usable in fullscreen */}
      <LyricsEditor />
      <TourOverlay />
      <ToastHost />
    </div>
  )
}
