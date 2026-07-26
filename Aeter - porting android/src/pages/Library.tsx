import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { LibraryBig, FolderPlus, Search, ArrowUpDown, ChevronDown, ChevronUp } from 'lucide-react'
import type { TrackQuery } from '@shared/types'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import BottomSheet from '@/components/ui/BottomSheet'
import { ListSkeleton } from '@/components/ui/Skeletons'
import TrackList from '@/components/library/TrackList'
import { useLibraryStore } from '@/store/useLibraryStore'
import { useUiStore } from '@/store/useUiStore'
import { isMobile } from '@/lib/platform'
import { select as hapticSelect } from '@/lib/haptics'

const SORT_FIELDS: NonNullable<TrackQuery['sortBy']>[] = [
  'title',
  'artist',
  'album',
  'year',
  'duration',
  'rating'
]

export default function Library(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const tracks = useLibraryStore((s) => s.tracks)
  const loaded = useLibraryStore((s) => s.loaded)
  const scanProgress = useLibraryStore((s) => s.scanProgress)
  const trackQuery = useLibraryStore((s) => s.trackQuery)
  const setSort = useLibraryStore((s) => s.setSort)
  const refreshAll = useLibraryStore((s) => s.refreshAll)
  const [sortOpen, setSortOpen] = useState(false)

  // Search lives in the Library header: this button opens the global search
  // overlay (tracks/albums/artists). Always visible so search is reachable from
  // the page on every platform (desktop also has Ctrl+F); the tour anchors here.
  const actions = (
    <>
      {/* Mobile: la riga header colonne è nascosta sul telefono (list item a
          due righe), quindi l'ordinamento passa da questo bottone. */}
      {isMobile && (
        <button
          className="icon-btn h-11 w-11"
          onClick={() => setSortOpen(true)}
          aria-label={t('library.sort')}
        >
          <ArrowUpDown size={20} />
        </button>
      )}
      <button
        className="icon-btn h-11 w-11"
        data-tour="search-button"
        onClick={() => useUiStore.getState().setSearchOpen(true)}
        aria-label={t('search.placeholder')}
      >
        <Search size={20} />
      </button>
    </>
  )

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-tour="library-page">
      <PageHeader
        title={t('library.title')}
        subtitle={t('library.tracks_count', { count: tracks.length })}
        actions={actions}
      />
      {!loaded ? (
        <ListSkeleton />
      ) : tracks.length === 0 && !scanProgress ? (
        <EmptyState
          icon={LibraryBig}
          title={t('library.empty_title')}
          subtitle={t('library.empty_subtitle')}
          action={
            <button
              className="btn-accent mt-1 flex items-center gap-2 rounded-lg px-4 py-2 text-[13px]"
              onClick={() => navigate('/settings')}
            >
              <FolderPlus size={15} />
              {t('library.add_folder')}
            </button>
          }
        />
      ) : (
        <TrackList tracks={tracks} sortable className="px-3" onPullRefresh={refreshAll} />
      )}

      {isMobile && (
        <BottomSheet open={sortOpen} onClose={() => setSortOpen(false)} title={t('library.sort')}>
          <div className="px-3 pb-2">
            {SORT_FIELDS.map((key) => {
              const active = trackQuery.sortBy === key
              return (
                <button
                  key={key}
                  className={`flex min-h-[48px] w-full items-center justify-between rounded-lg px-3 text-left text-[14px] transition-colors active:bg-white/[0.07] ${
                    active ? 'text-[var(--accent)]' : ''
                  }`}
                  onClick={() => {
                    hapticSelect()
                    setSort(key) // stesso campo → inverte la direzione (come l'header desktop)
                  }}
                >
                  {t(`library.col_${key}`)}
                  {active &&
                    (trackQuery.sortDir === 'asc' ? (
                      <ChevronUp size={16} />
                    ) : (
                      <ChevronDown size={16} />
                    ))}
                </button>
              )
            })}
          </div>
        </BottomSheet>
      )}
    </div>
  )
}
