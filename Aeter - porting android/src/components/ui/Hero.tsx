import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { ArrowLeft } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { usePagePalette } from '@/hooks/usePalette'
import { isMobile } from '@/lib/platform'
import CoverImage from '@/components/ui/CoverImage'

/**
 * Edge-to-edge immersive page hero: blurred artwork backdrop tinted by the
 * artwork's own palette (--hero-rgb via usePagePalette), foreground artwork
 * card, oversized title and an action row.
 */
export default function Hero({
  image,
  artwork,
  fallbackIcon: Icon,
  eyebrow,
  title,
  meta,
  actions,
  shape = 'square',
  paletteHash
}: {
  image?: string | null
  /** Custom artwork node (e.g. playlist mosaic) — replaces the default <img> tile */
  artwork?: React.ReactNode
  fallbackIcon: LucideIcon
  eyebrow?: string
  title: string
  meta?: React.ReactNode
  actions?: React.ReactNode
  shape?: 'square' | 'circle'
  paletteHash?: string | null
}): React.JSX.Element {
  const navigate = useNavigate()
  const { t } = useTranslation()
  const tint = usePagePalette(paletteHash)
  const radius = shape === 'circle' ? '9999px' : 'var(--radius-card)'

  return (
    <section className="relative shrink-0 overflow-hidden" style={tint}>
      {image && (
        <img
          src={image}
          alt=""
          aria-hidden
          draggable={false}
          className={`absolute inset-0 h-full w-full scale-110 object-cover opacity-40 saturate-150 ${
            isMobile ? 'blur-xl' : 'blur-3xl'
          }`}
        />
      )}
      <div className="hero-scrim" />

      <div className={`relative px-[var(--content-x)] pb-6 ${isMobile ? 'pt-3' : 'pt-11'}`}>
        <button
          className={`icon-btn no-drag mb-3 ${isMobile ? 'h-11 w-11' : 'h-8 w-8'}`}
          onClick={() => navigate(-1)}
          aria-label={t('common.back')}
        >
          <ArrowLeft size={isMobile ? 20 : 16} />
        </button>

        <div className="flex flex-wrap items-end gap-x-6 gap-y-4">
          <div
            className="hero-art shrink-0 overflow-hidden bg-surface-3"
            style={{
              width: 'clamp(120px, 18cqw, 200px)',
              aspectRatio: '1',
              borderRadius: radius,
              boxShadow: 'var(--shadow-3), 0 0 40px var(--accent-soft)'
            }}
          >
            {artwork ?? (
              <CoverImage
                src={image}
                eager
                className="h-full w-full object-cover"
                fallback={
                  <div className="flex h-full w-full items-center justify-center">
                    <Icon size={44} className="text-text-3" />
                  </div>
                }
              />
            )}
          </div>

          <div className="min-w-0 flex-1 pb-1" style={{ minWidth: 'min(260px, 100%)' }}>
            {eyebrow && (
              <div className="hero-eyebrow mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-text-3">
                {eyebrow}
              </div>
            )}
            <h1
              className="page-title truncate font-extrabold"
              data-text={title}
              style={{ fontSize: 'clamp(28px, 5cqw, 56px)', letterSpacing: '-0.03em', lineHeight: 1.1 }}
            >
              {title}
            </h1>
            {meta && <div className="mt-2 text-[13.5px] text-text-2">{meta}</div>}
            {actions && <div className="mt-4 flex flex-wrap items-center gap-2">{actions}</div>}
          </div>
        </div>
      </div>
    </section>
  )
}
