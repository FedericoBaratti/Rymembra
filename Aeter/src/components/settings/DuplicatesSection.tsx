import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Copy, Loader2, Merge } from 'lucide-react'
import type { DuplicateGroup } from '@shared/types'
import { Section, FieldRow } from './controls'
import DuplicateMergeDialog from '@/components/library/DuplicateMergeDialog'
import { formatBytes } from '@/lib/format'

export default function DuplicatesSection({ index = 0 }: { index?: number }): React.JSX.Element {
  const { t } = useTranslation()
  const [dupes, setDupes] = useState<DuplicateGroup[] | null>(null)
  const [dupesBusy, setDupesBusy] = useState(false)
  const [mergeGroup, setMergeGroup] = useState<DuplicateGroup | null>(null)

  const findDupes = async (): Promise<void> => {
    setDupesBusy(true)
    try {
      setDupes(await window.aether.findDuplicates())
    } finally {
      setDupesBusy(false)
    }
  }

  return (
    <>
      <Section title={t('settings.section_duplicates')} icon={Copy} index={index}>
        <FieldRow label={t('settings.find_duplicates')}>
          <button
            className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium disabled:opacity-50"
            onClick={() => void findDupes()}
            disabled={dupesBusy}
          >
            {dupesBusy ? <Loader2 size={13} className="animate-spin" /> : <Copy size={13} />}
            {t('settings.find_duplicates')}
          </button>
        </FieldRow>
        {dupes != null &&
          (dupes.length === 0 ? (
            <div className="text-[12.5px] text-text-3">{t('settings.no_duplicates')}</div>
          ) : (
            <div className="flex flex-col gap-2">
              <div className="text-[12.5px] text-text-2">
                {t('settings.duplicates_found', { count: dupes.length })}
              </div>
              {dupes.map((group, gi) => (
                <div key={gi} className="rounded-lg bg-white/[0.03] p-3">
                  {group.tracks.map((tr) => (
                    <div key={tr.id} className="flex items-center justify-between gap-3 py-0.5 text-[12px]">
                      <span className="truncate">
                        {tr.artist} — {tr.title}
                      </span>
                      <span className="tnum shrink-0 text-text-3">
                        {tr.bitrate ? `${Math.round(tr.bitrate / 1000)} kbps` : '?'} ·{' '}
                        {formatBytes(tr.file_size)}
                      </span>
                    </div>
                  ))}
                  <button
                    className="mt-2 flex items-center gap-1.5 rounded-lg bg-[var(--accent-soft)] px-2.5 py-1.5 text-[12px] font-medium text-[var(--accent)] transition-colors hover:brightness-110"
                    onClick={() => setMergeGroup(group)}
                  >
                    <Merge size={12} /> {t('merge.open')}
                  </button>
                </div>
              ))}
            </div>
          ))}
      </Section>

      {mergeGroup && (
        <DuplicateMergeDialog
          group={mergeGroup}
          onClose={() => setMergeGroup(null)}
          onMerged={() => void findDupes()}
        />
      )}
    </>
  )
}
