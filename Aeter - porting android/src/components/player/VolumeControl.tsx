import { useState } from 'react'
import { Volume2, Volume1, VolumeX } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { usePlayerStore } from '@/store/usePlayerStore'

/** Mute toggle + vertical slider revealed on hover. */
export default function VolumeControl(): React.JSX.Element {
  const { t } = useTranslation()
  const volume = usePlayerStore((s) => s.volume)
  const muted = usePlayerStore((s) => s.muted)
  const setVolume = usePlayerStore((s) => s.setVolume)
  const toggleMute = usePlayerStore((s) => s.toggleMute)
  const [open, setOpen] = useState(false)

  const Icon = muted || volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2

  return (
    <div
      className="relative flex items-center"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
    >
      {open && (
        <div className="absolute bottom-full left-1/2 z-30 -translate-x-1/2 pb-2">
          <div
            className="glass-modal scale-in flex h-[120px] w-9 items-center justify-center rounded-xl py-3"
            style={{ boxShadow: 'var(--shadow-2)' }}
          >
            <input
              type="range"
              min={0}
              max={100}
              value={muted ? 0 : Math.round(volume * 100)}
              onChange={(e) => setVolume(Number(e.target.value) / 100)}
              className="volume-slider"
              aria-label={t('player.mute')}
            />
          </div>
        </div>
      )}
      <button
        className="icon-btn no-drag h-9 w-9"
        onClick={toggleMute}
        aria-label={`${t('player.mute')} (M)`}
        data-active={muted}
      >
        <Icon size={17} />
      </button>
      <style>{`
        .volume-slider {
          -webkit-appearance: none;
          width: 96px;
          height: 4px;
          transform: rotate(-90deg);
          background: linear-gradient(to right, var(--accent) ${muted ? 0 : volume * 100}%, rgba(255,255,255,0.15) ${muted ? 0 : volume * 100}%);
          border-radius: 2px;
          outline: none;
        }
        .volume-slider::-webkit-slider-thumb {
          -webkit-appearance: none;
          width: 12px;
          height: 12px;
          border-radius: 50%;
          background: white;
          box-shadow: 0 0 8px var(--accent-glow);
          cursor: pointer;
        }
      `}</style>
    </div>
  )
}
