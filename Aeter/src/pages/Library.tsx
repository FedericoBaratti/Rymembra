import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { LibraryBig, FolderPlus } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import TrackList from '@/components/library/TrackList'
import { useLibraryStore } from '@/store/useLibraryStore'

export default function Library(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const tracks = useLibraryStore((s) => s.tracks)
  const loaded = useLibraryStore((s) => s.loaded)
  const scanProgress = useLibraryStore((s) => s.scanProgress)

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-tour="library-page">
      <PageHeader title={t('library.title')} subtitle={t('library.tracks_count', { count: tracks.length })} />
      {!loaded ? (
        <div className="flex flex-col gap-2 px-6">
          {Array.from({ length: 10 }).map((_, i) => (
            <div key={i} className="skeleton h-10" />
          ))}
        </div>
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
        <TrackList tracks={tracks} sortable className="px-3" />
      )}
    </div>
  )
}
