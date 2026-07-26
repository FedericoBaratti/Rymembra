import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import '@/i18n'
import { mockAether } from '@/test/setup'
import { useUiStore } from '@/store/useUiStore'
import type { SearchResults, ExternalRecoTrack, Track } from '@shared/types'
import SearchOverlay from './SearchOverlay'

// Regression test for the stale-response race: on variable (mobile) latency an
// OLDER search response can resolve AFTER a newer one; the overlay must ignore
// it instead of overwriting the results of the latest term.

function mkTrack(id: number, title: string): Track {
  return {
    id,
    title,
    artist: 'Artista',
    album: 'Album',
    duration: 200,
    cover_art_hash: null
  } as unknown as Track
}

const emptyResults: SearchResults = { tracks: [], albums: [], artists: [] }

function resultsWith(title: string): SearchResults {
  return { ...emptyResults, tracks: [mkTrack(1, title)] }
}

type Deferred<T> = { promise: Promise<T>; resolve: (v: T) => void }
function deferred<T>(): Deferred<T> {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

// useUiStore is persisted (zustand/persist → localStorage), but this jsdom
// environment ships a broken localStorage (setItem is not a function) and
// persist captures the storage object at store CREATION — so the stand-in must
// be installed before the store module is imported (vi.hoisted runs pre-import).
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

beforeEach(() => {
  useUiStore.setState({ searchOpen: true })
})

describe('SearchOverlay stale-response guard', () => {
  it('ignores an older local search response that resolves after a newer one', async () => {
    const byTerm = new Map<string, Deferred<SearchResults>>()
    const search = vi.fn((term: string) => {
      const d = deferred<SearchResults>()
      byTerm.set(term, d)
      return d.promise
    })
    mockAether({
      search,
      searchExternalCatalog: vi.fn(() => Promise.resolve([] as ExternalRecoTrack[]))
    })

    render(
      <MemoryRouter>
        <SearchOverlay />
      </MemoryRouter>
    )
    const input = screen.getByPlaceholderText(/cerca/i)

    fireEvent.change(input, { target: { value: 'a' } })
    await waitFor(() => expect(search).toHaveBeenCalledWith('a'))

    fireEvent.change(input, { target: { value: 'ab' } })
    await waitFor(() => expect(search).toHaveBeenCalledWith('ab'))

    // Newest response lands first…
    byTerm.get('ab')!.resolve(resultsWith('Risultato Nuovo'))
    expect(await screen.findByText('Risultato Nuovo')).toBeInTheDocument()

    // …then the stale one for "a" resolves late: it must be dropped.
    byTerm.get('a')!.resolve(resultsWith('Risultato Vecchio'))
    await new Promise((r) => setTimeout(r, 30))
    expect(screen.queryByText('Risultato Vecchio')).not.toBeInTheDocument()
    expect(screen.getByText('Risultato Nuovo')).toBeInTheDocument()
  })

  it('ignores a stale external-catalog response the same way', async () => {
    const byTerm = new Map<string, Deferred<ExternalRecoTrack[]>>()
    const searchExternalCatalog = vi.fn((term: string) => {
      const d = deferred<ExternalRecoTrack[]>()
      byTerm.set(term, d)
      return d.promise
    })
    mockAether({
      search: vi.fn(() => Promise.resolve(emptyResults)),
      searchExternalCatalog
    })

    render(
      <MemoryRouter>
        <SearchOverlay />
      </MemoryRouter>
    )
    const input = screen.getByPlaceholderText(/cerca/i)

    fireEvent.change(input, { target: { value: 'x' } })
    await waitFor(() => expect(searchExternalCatalog).toHaveBeenCalledWith('x'), {
      timeout: 2000
    })
    fireEvent.change(input, { target: { value: 'xy' } })
    await waitFor(() => expect(searchExternalCatalog).toHaveBeenCalledWith('xy'), {
      timeout: 2000
    })

    const ext = (title: string): ExternalRecoTrack[] => [
      { title, artist: 'Web', owned: false } as unknown as ExternalRecoTrack
    ]
    byTerm.get('xy')!.resolve(ext('Web Nuovo'))
    expect(await screen.findByText('Web Nuovo')).toBeInTheDocument()

    byTerm.get('x')!.resolve(ext('Web Vecchio'))
    await new Promise((r) => setTimeout(r, 30))
    expect(screen.queryByText('Web Vecchio')).not.toBeInTheDocument()
    expect(screen.getByText('Web Nuovo')).toBeInTheDocument()
  })
})

describe('SearchOverlay web-search failure', () => {
  it('shows an error with a retry button instead of pretending "no results", and retries', async () => {
    let calls = 0
    const searchExternalCatalog = vi.fn((_term: string) => {
      calls++
      return calls === 1
        ? Promise.reject(new Error('EXT_SEARCH_FAILED'))
        : Promise.resolve([
            { title: 'Web Trovato', artist: 'Web', owned: false } as unknown as ExternalRecoTrack
          ])
    })
    mockAether({
      search: vi.fn(() => Promise.resolve(emptyResults)),
      searchExternalCatalog
    })

    render(
      <MemoryRouter>
        <SearchOverlay />
      </MemoryRouter>
    )
    fireEvent.change(screen.getByPlaceholderText(/cerca/i), { target: { value: 'query' } })

    // Failure → dedicated error row + retry, NOT the "no results" empty state
    const retry = await screen.findByRole('button', { name: /riprova/i }, { timeout: 2000 })
    expect(screen.queryByText(/nessun risultato per/i)).not.toBeInTheDocument()

    // Retry re-runs the same search and renders the results on success
    fireEvent.click(retry)
    expect(await screen.findByText('Web Trovato', undefined, { timeout: 2000 })).toBeInTheDocument()
    expect(searchExternalCatalog).toHaveBeenCalledTimes(2)
  })
})
