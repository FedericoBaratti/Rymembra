import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import '@/i18n'
import { mockAether } from '@/test/setup'
import type { Track, LyricsResult } from '@shared/types'

// useUiStore and usePlayerStore are zustand/persist stores → persist captures the
// storage object at store CREATION, and jsdom ships a broken localStorage. Install
// a working stand-in before the store modules are imported (vi.hoisted is pre-import).
vi.hoisted(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    writable: true,
    value: {
      getItem: (k: string) => storage.get(k) ?? null,
      setItem: (k: string, v: string) => void storage.set(k, v),
      removeItem: (k: string) => void storage.delete(k),
      clear: () => storage.clear(),
      key: () => null,
      get length() {
        return storage.size
      }
    }
  })
})

// Scrubber renders to a <canvas> (null 2D context in jsdom) — stub it out; these
// tests are about the surrounding controls, not the waveform.
vi.mock('@/components/player/Scrubber', () => ({ default: () => null }))

import { useUiStore } from '@/store/useUiStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import NowPlaying from './NowPlaying'
import LyricsScreen from './LyricsScreen'
import SleepSheet from './SleepSheet'

function mkTrack(over: Partial<Track> = {}): Track {
  return {
    id: 1,
    title: 'Titolo',
    artist: 'Artista',
    album: 'Album',
    duration: 200,
    cover_art_hash: null,
    ...over
  } as unknown as Track
}

beforeEach(() => {
  mockAether()
  useUiStore.setState({
    nowPlayingOpen: false,
    lyricsOpen: false,
    sleepMenuOpen: false,
    eqOpen: false,
    queueOpen: false
  })
  usePlayerStore.setState({ currentTrack: mkTrack(), isPlaying: false, volume: 0.8, muted: false })
})

describe('NowPlaying mobile tools', () => {
  it('exposes lyrics, sleep and volume controls for a library track', () => {
    useUiStore.setState({ nowPlayingOpen: true })
    render(
      <MemoryRouter>
        <NowPlaying />
      </MemoryRouter>
    )
    // lyrics (MicVocal) button, aria-label = lyrics.title
    expect(screen.getByRole('button', { name: 'Testi' })).toBeInTheDocument()
    // sleep (Moon) button, aria-label = sleep.title
    expect(screen.getByRole('button', { name: /sleep timer/i })).toBeInTheDocument()
    // in-app volume slider + mute
    expect(screen.getByRole('slider', { name: /volume/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Muto' })).toBeInTheDocument()
  })

  it('hides library-only tools (lyrics) for a podcast episode but keeps volume', () => {
    usePlayerStore.setState({
      currentTrack: mkTrack({ id: -5, stream_url: 'https://example.com/ep.mp3' } as Partial<Track>)
    })
    useUiStore.setState({ nowPlayingOpen: true })
    render(
      <MemoryRouter>
        <NowPlaying />
      </MemoryRouter>
    )
    expect(screen.queryByRole('button', { name: 'Testi' })).not.toBeInTheDocument()
    expect(screen.getByRole('slider', { name: /volume/i })).toBeInTheDocument()
  })
})

describe('LyricsScreen', () => {
  it('renders synced lyrics fetched from getLyrics', async () => {
    const lyrics: LyricsResult = {
      synced: [
        { time: 0, text: 'Prima riga' },
        { time: 5, text: 'Seconda riga' }
      ],
      plain: null
    }
    mockAether({ getLyrics: vi.fn(() => Promise.resolve(lyrics)) })
    useUiStore.setState({ lyricsOpen: true })
    render(
      <MemoryRouter>
        <LyricsScreen />
      </MemoryRouter>
    )
    expect(await screen.findByText('Prima riga')).toBeInTheDocument()
    expect(screen.getByText('Seconda riga')).toBeInTheDocument()
  })

  it('shows an empty state with a fetch button when there are no lyrics', async () => {
    mockAether({ getLyrics: vi.fn(() => Promise.resolve({ synced: null, plain: null })) })
    useUiStore.setState({ lyricsOpen: true })
    render(
      <MemoryRouter>
        <LyricsScreen />
      </MemoryRouter>
    )
    expect(await screen.findByRole('button', { name: /cerca testo online/i })).toBeInTheDocument()
  })
})

describe('SleepSheet', () => {
  it('renders the timer options and the off entry when open', () => {
    useUiStore.setState({ sleepMenuOpen: true })
    render(<SleepSheet />)
    expect(screen.getByText(/15 minuti/i)).toBeInTheDocument()
    expect(screen.getByText(/90 minuti/i)).toBeInTheDocument()
    expect(screen.getByText(/disattivato/i)).toBeInTheDocument()
  })

  it('renders nothing while closed', () => {
    useUiStore.setState({ sleepMenuOpen: false })
    const { container } = render(<SleepSheet />)
    expect(container).toBeEmptyDOMElement()
  })
})
