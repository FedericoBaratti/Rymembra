import { useEffect, useRef, useState } from 'react'
import { NavLink } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  Home,
  LibraryBig,
  Disc3,
  ListMusic,
  Podcast,
  Settings,
  AudioWaveform,
  Pin,
  PinOff
} from 'lucide-react'
import { useLibraryStore } from '@/store/useLibraryStore'
import { useUiStore } from '@/store/useUiStore'
import Tooltip from '@/components/ui/Tooltip'

const NAV_ITEMS = [
  { to: '/home', icon: Home, key: 'nav.home' },
  { to: '/library', icon: LibraryBig, key: 'nav.library' },
  { to: '/albums', icon: Disc3, key: 'nav.albums' },
  { to: '/playlists', icon: ListMusic, key: 'nav.playlists' },
  { to: '/podcasts', icon: Podcast, key: 'nav.podcasts' }
] as const

const BOTTOM_ITEMS = [
  { to: '/settings', icon: Settings, key: 'nav.settings' }
] as const

const PIN_BREAKPOINT = '(min-width: 1100px)'

function NavItem({
  to,
  icon: Icon,
  label,
  expanded
}: {
  to: string
  icon: typeof LibraryBig
  label: string
  expanded: boolean
}): React.JSX.Element {
  return (
    <NavLink
      to={to}
      title={expanded ? undefined : label}
      className={({ isActive }) =>
        `no-drag group flex h-10 items-center overflow-hidden rounded-lg text-[13px] font-medium transition-all duration-150 ${
          expanded ? 'gap-3 px-3' : 'justify-center gap-0 px-0'
        } ${
          isActive
            ? 'bg-[var(--accent-soft)] text-[var(--accent)] shadow-[inset_0_0_0_1px_var(--accent-soft)]'
            : 'text-text-2 hover:bg-white/[0.06] hover:text-text-1'
        }`
      }
    >
      {({ isActive }) => (
        <>
          <Icon
            size={18}
            strokeWidth={2}
            className={`shrink-0 transition-transform duration-150 group-hover:scale-105 ${
              isActive ? 'drop-shadow-[0_0_6px_var(--accent-glow)]' : ''
            }`}
          />
          <span
            className={`whitespace-nowrap transition-[opacity,transform] duration-200 ${
              expanded ? 'translate-x-0 opacity-100' : 'pointer-events-none -translate-x-2 opacity-0'
            }`}
            style={{ width: expanded ? 'auto' : 0 }}
          >
            {label}
          </span>
        </>
      )}
    </NavLink>
  )
}

export default function Sidebar(): React.JSX.Element {
  const { t } = useTranslation()
  const scanProgress = useLibraryStore((s) => s.scanProgress)
  const pinned = useUiStore((s) => s.sidebarPinned)
  const setPinned = useUiStore((s) => s.setSidebarPinned)
  const [hovered, setHovered] = useState(false)
  const [canPin, setCanPin] = useState(() => window.matchMedia(PIN_BREAKPOINT).matches)
  const enterTimer = useRef<number | undefined>(undefined)

  useEffect(() => {
    const mq = window.matchMedia(PIN_BREAKPOINT)
    const onChange = (): void => setCanPin(mq.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  const isPinned = pinned && canPin

  // The floating player reads --shell-left; a media query in global.css
  // gates the expanded value to ≥1100px, matching canPin.
  useEffect(() => {
    document.documentElement.toggleAttribute('data-sidebar-pinned', pinned)
  }, [pinned])

  useEffect(() => () => window.clearTimeout(enterTimer.current), [])

  const onEnter = (): void => {
    if (isPinned) return
    enterTimer.current = window.setTimeout(() => setHovered(true), 250)
  }
  const onLeave = (): void => {
    window.clearTimeout(enterTimer.current)
    setHovered(false)
  }
  // Keyboard access: expand as soon as focus lands inside the rail (no hover
  // delay), collapse only when focus leaves the whole container.
  const onFocusIn = (): void => {
    if (isPinned) return
    window.clearTimeout(enterTimer.current)
    setHovered(true)
  }
  const onFocusOut = (e: React.FocusEvent<HTMLDivElement>): void => {
    if (e.currentTarget.contains(e.relatedTarget as Node | null)) return
    window.clearTimeout(enterTimer.current)
    setHovered(false)
  }

  const expanded = isPinned || hovered
  const scanPct = scanProgress?.total
    ? Math.round((scanProgress.current / scanProgress.total) * 100)
    : null

  return (
    <div
      className="relative h-full shrink-0 transition-[width] duration-200"
      style={{ width: isPinned ? 'var(--rail-w-expanded)' : 'var(--rail-w)' }}
      onMouseEnter={onEnter}
      onMouseLeave={onLeave}
      onFocus={onFocusIn}
      onBlur={onFocusOut}
    >
      <aside
        className={`vt-sidebar absolute inset-y-0 left-0 z-30 flex flex-col overflow-hidden border-r pb-3 pt-10 ${
          expanded ? 'px-3' : 'px-3.5'
        }`}
        style={{
          width: expanded ? 'var(--rail-w-expanded)' : 'var(--rail-w)',
          transition:
            'width 220ms var(--ease-out-expo), padding 220ms var(--ease-out-expo), background 220ms var(--ease-out-expo), box-shadow 220ms var(--ease-out-expo)',
          borderColor: 'var(--hairline)',
          background:
            hovered && !isPinned ? 'rgba(14, 14, 20, 0.85)' : 'var(--sidebar-bg)',
          backdropFilter: hovered && !isPinned ? 'blur(24px) saturate(1.5)' : undefined,
          boxShadow: hovered && !isPinned ? 'var(--shadow-3)' : undefined
        }}
      >
        <div className={`mb-6 flex items-center gap-2.5 ${expanded ? 'px-3' : 'justify-center'}`}>
          <div
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
            style={{
              background: 'linear-gradient(135deg, var(--accent), rgba(var(--accent-rgb) / 0.5))',
              boxShadow: '0 0 18px var(--accent-glow)',
              transition: 'background 800ms ease, box-shadow 800ms ease'
            }}
          >
            <AudioWaveform size={17} color="white" strokeWidth={2.4} />
          </div>
          {expanded && (
            <>
              <span className="flex-1 whitespace-nowrap text-[15px] font-bold tracking-tight">
                Aether
              </span>
              {canPin && (
                <Tooltip label={isPinned ? t('nav.unpin') : t('nav.pin')} side="bottom">
                  <button
                    className="icon-btn no-drag h-7 w-7"
                    onClick={() => setPinned(!pinned)}
                    aria-label={isPinned ? t('nav.unpin') : t('nav.pin')}
                    aria-pressed={isPinned}
                  >
                    {isPinned ? <PinOff size={14} /> : <Pin size={14} />}
                  </button>
                </Tooltip>
              )}
            </>
          )}
        </div>

        <nav className="flex flex-col gap-1" data-tour="sidebar-nav">
          {NAV_ITEMS.map((item) => (
            <NavItem
              key={item.to}
              to={item.to}
              icon={item.icon}
              label={t(item.key)}
              expanded={expanded}
            />
          ))}
        </nav>

        <div className={`my-4 h-px ${expanded ? 'mx-3' : 'mx-1'}`} style={{ background: 'var(--hairline)' }} />

        <nav className="flex flex-col gap-1">
          {BOTTOM_ITEMS.map((item) => (
            <NavItem
              key={item.to}
              to={item.to}
              icon={item.icon}
              label={t(item.key)}
              expanded={expanded}
            />
          ))}
        </nav>

        <div className="flex-1" />

        {scanProgress &&
          (expanded ? (
            <div className="fade-in mx-1 mb-1 rounded-lg bg-white/[0.04] p-3">
              <div className="mb-1.5 whitespace-nowrap text-[11px] text-text-2">
                {t('library.scanning', {
                  current: scanProgress.current,
                  total: scanProgress.total || '…'
                })}
              </div>
              <div className="h-1 overflow-hidden rounded-full bg-white/10">
                <div
                  className="h-full rounded-full transition-[width] duration-300"
                  style={{
                    width: scanPct != null ? `${scanPct}%` : '30%',
                    background: 'var(--accent)'
                  }}
                />
              </div>
            </div>
          ) : (
            <div
              className="fade-in mx-auto mb-1 h-1 w-8 overflow-hidden rounded-full bg-white/10"
              title={t('library.scanning', {
                current: scanProgress.current,
                total: scanProgress.total || '…'
              })}
            >
              <div
                className="h-full rounded-full transition-[width] duration-300"
                style={{
                  width: scanPct != null ? `${scanPct}%` : '30%',
                  background: 'var(--accent)'
                }}
              />
            </div>
          ))}
      </aside>
    </div>
  )
}
