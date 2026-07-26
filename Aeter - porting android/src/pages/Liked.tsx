import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Heart } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import TrackList from '@/components/library/TrackList'
import type { Track } from '@shared/types'

export default function Liked(): React.JSX.Element {
  const { t } = useTranslation()
  const [tracks, setTracks] = useState<Track[] | null>(null)

  useEffect(() => {
    let alive = true
    void window.aether
      .getLikedTracks()
      .then((rows) => alive && setTracks(rows))
      .catch(() => alive && setTracks([]))
    return () => {
      alive = false
    }
  }, [])

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('liked.title')}
        subtitle={t('liked.count', { count: tracks?.length ?? 0 })}
      />
      {tracks == null ? (
        <div className="flex flex-col gap-2 px-6">
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i} className="skeleton h-10" />
          ))}
        </div>
      ) : tracks.length === 0 ? (
        <EmptyState icon={Heart} title={t('liked.empty_title')} subtitle={t('liked.empty_subtitle')} />
      ) : (
        <TrackList tracks={tracks} className="px-3" />
      )}
    </div>
  )
}
