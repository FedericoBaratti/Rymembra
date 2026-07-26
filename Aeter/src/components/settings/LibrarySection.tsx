import { useTranslation } from 'react-i18next'
import { FolderOpen, FolderPlus, RefreshCw, X } from 'lucide-react'
import { Section } from './controls'
import { useSettingsStore } from '@/store/useSettingsStore'
import { useLibraryStore } from '@/store/useLibraryStore'

export default function LibrarySection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const scanProgress = useLibraryStore((s) => s.scanProgress)
  if (!settings) return null

  const addFolder = async (): Promise<void> => {
    const folder = await window.aether.pickFolder()
    if (folder && !settings.watchFolders.includes(folder)) {
      void update({ watchFolders: [...settings.watchFolders, folder] })
    }
  }

  return (
    <Section title={t('settings.section_library')} icon={FolderOpen} index={index} dataTour="settings-folders">
      <div>
        <div className="mb-2 flex items-center justify-between">
          <span className="text-[13.5px] font-medium">{t('settings.watch_folders')}</span>
          <div className="flex gap-2">
            <button
              className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12.5px] font-medium"
              onClick={() => void window.aether.rescanLibrary()}
              disabled={!!scanProgress}
            >
              <RefreshCw size={13} className={scanProgress ? 'animate-spin' : ''} />
              {t('settings.rescan')}
            </button>
            <button
              className="btn-accent flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12.5px]"
              onClick={() => void addFolder()}
            >
              <FolderPlus size={13} /> {t('settings.add_folder')}
            </button>
          </div>
        </div>
        {settings.watchFolders.length === 0 ? (
          <div className="rounded-lg bg-white/[0.03] px-3 py-2.5 text-[12.5px] text-text-3">—</div>
        ) : (
          settings.watchFolders.map((folder) => (
            <div
              key={folder}
              className="group mb-1 flex items-center justify-between rounded-lg bg-white/[0.03] px-3 py-2 text-[12.5px]"
            >
              <span className="truncate">{folder}</span>
              <button
                className="icon-btn h-6 w-6 opacity-0 group-hover:opacity-100"
                onClick={() =>
                  void update({ watchFolders: settings.watchFolders.filter((f) => f !== folder) })
                }
                title={t('settings.remove')}
                aria-label={`${t('settings.remove')} ${folder}`}
              >
                <X size={13} />
              </button>
            </div>
          ))
        )}
      </div>
    </Section>
  )
}
