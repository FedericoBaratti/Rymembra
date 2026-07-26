import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, Check, Download, Loader2, RefreshCw } from 'lucide-react'
import { Section, FieldRow, Switch, selectCls } from './controls'
import { useSettingsStore } from '@/store/useSettingsStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

type BinaryStatus = Awaited<ReturnType<typeof window.aether.getBinaryStatus>>

export default function DownloadsSection({ index = 0 }: { index?: number }): React.JSX.Element | null {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [ytdlpBusy, setYtdlpBusy] = useState(false)
  const [ytdlpMsg, setYtdlpMsg] = useState<string | null>(null)
  const [binaries, setBinaries] = useState<BinaryStatus | null>(null)

  useEffect(() => {
    window.aether.getBinaryStatus().then(setBinaries).catch(() => setBinaries(null))
  }, [])

  if (!settings) return null

  const updateYtdlp = async (): Promise<void> => {
    setYtdlpBusy(true)
    setYtdlpMsg(null)
    try {
      const res = await window.aether.updateYtDlp()
      setYtdlpMsg(t('settings.ytdlp_updated', { version: res.version }))
      toast.success(t('toast.ytdlp_updated'), res.version)
    } catch (err) {
      const msg = ipcErrorMessage(err)
      setYtdlpMsg(msg)
      toast.error(t('toast.ytdlp_update_failed'), msg)
    } finally {
      setYtdlpBusy(false)
    }
  }

  return (
    <Section title={t('settings.section_downloads')} icon={Download} index={index}>
      <FieldRow label={t('settings.download_folder')}>
        <button
          className="btn-ghost max-w-[280px] truncate rounded-lg px-3 py-2 text-[12.5px]"
          onClick={() =>
            void window.aether.pickFolder().then((f) => {
              if (f) void update({ downloadFolder: f })
            })
          }
        >
          {settings.downloadFolder || t('settings.choose')}
        </button>
      </FieldRow>
      <FieldRow label={t('settings.quality')}>
        <select
          className={selectCls}
          value={settings.downloadQuality}
          onChange={(e) => void update({ downloadQuality: e.target.value as typeof settings.downloadQuality })}
        >
          <option value="mp3-320">MP3 320 kbps</option>
          <option value="flac">FLAC</option>
          <option value="aac-256">AAC 256 kbps</option>
        </select>
      </FieldRow>
      <FieldRow label={t('settings.concurrency')}>
        <select
          className={selectCls}
          value={settings.downloadConcurrency}
          onChange={(e) => void update({ downloadConcurrency: Number(e.target.value) })}
        >
          {[1, 2, 3, 4, 5, 6, 8, 10].map((n) => (
            <option key={n} value={n}>{n}</option>
          ))}
        </select>
      </FieldRow>
      <FieldRow label={t('settings.auto_fix_youtube')} hint={t('settings.auto_fix_youtube_hint')}>
        <Switch
          checked={settings.autoFixYoutubeMetadata}
          label={t('settings.auto_fix_youtube')}
          onChange={(checked) => void update({ autoFixYoutubeMetadata: checked })}
        />
      </FieldRow>
      <FieldRow label="yt-dlp" hint={ytdlpMsg ?? undefined}>
        <button
          className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium disabled:opacity-50"
          onClick={() => void updateYtdlp()}
          disabled={ytdlpBusy}
        >
          {ytdlpBusy ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
          {t('settings.update_ytdlp')}
        </button>
      </FieldRow>
      {binaries && (
        <FieldRow label={t('settings.binaries_status')} hint={Object.values(binaries)[0]?.dir}>
          <div className="flex flex-wrap gap-2">
            {(Object.entries(binaries) as [string, { found: boolean; dir: string }][]).map(
              ([name, st]) => (
                <span
                  key={name}
                  className="flex items-center gap-1 rounded-lg bg-white/[0.04] px-2 py-1 text-[11.5px]"
                  title={st.found ? st.dir : t('errors.binary_missing', { name, dir: st.dir, url: '' })}
                >
                  {st.found ? (
                    <Check size={11} className="text-[var(--success)]" />
                  ) : (
                    <AlertTriangle size={11} className="text-amber-400" />
                  )}
                  {name}
                </span>
              )
            )}
          </div>
        </FieldRow>
      )}
    </Section>
  )
}
