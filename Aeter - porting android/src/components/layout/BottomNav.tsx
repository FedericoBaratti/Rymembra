import { NavLink } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Home, LibraryBig, Disc3, ListMusic, Podcast, Settings } from 'lucide-react'
import { select } from '@/lib/haptics'
import { isLanModeActive } from '@/lib/lanClient'

/**
 * Mobile primary navigation. Replaces the desktop hover/pin Sidebar with a
 * fixed bottom bar (Android touch convention). Same routes and i18n keys as
 * Sidebar.tsx; only the chrome differs. Sits below the PlayerBar and respects
 * the gesture-nav safe area (env(safe-area-inset-bottom), see global.css).
 */
// `shortKey`: 6 items on a ~360dp screen leave ~60px per cell — long labels
// ("Impostazioni") overflow, so the bar shows a compact variant when provided.
// aria-label and the desktop Sidebar keep using the full `key`.
const ITEMS: readonly {
  to: string
  icon: typeof Home
  key: string
  shortKey?: string
}[] = [
  { to: '/home', icon: Home, key: 'nav.home' },
  { to: '/library', icon: LibraryBig, key: 'nav.library' },
  { to: '/albums', icon: Disc3, key: 'nav.albums' },
  { to: '/playlists', icon: ListMusic, key: 'nav.playlists' },
  { to: '/podcasts', icon: Podcast, key: 'nav.podcasts' },
  { to: '/settings', icon: Settings, key: 'nav.settings', shortKey: 'nav.settings_short' }
] as const

// LAN thin-client mode only supports play/browse/search: podcasts have no
// backing LAN route (electron/modules/lan/routes.ts never exposed them —
// out of scope per "solo riprodurre musica, libreria, ricerca"), and Albums
// is redundant with Library's own browsing. Settings stays — it's how
// pairing is managed (RemoteConnectionSection).
const LAN_MODE_ROUTES = new Set(['/home', '/library', '/playlists', '/settings'])

export default function BottomNav(): React.JSX.Element {
  const { t } = useTranslation()
  const items = isLanModeActive() ? ITEMS.filter((i) => LAN_MODE_ROUTES.has(i.to)) : ITEMS
  return (
    <nav
      className="bottom-nav fixed inset-x-0 bottom-0 z-30 flex items-stretch justify-around border-t"
      style={{ borderColor: 'var(--hairline)', background: 'var(--sidebar-bg)' }}
      data-tour="sidebar-nav"
    >
      {items.map(({ to, icon: Icon, key, shortKey }) => (
        <NavLink
          key={to}
          to={to}
          aria-label={t(key)}
          onClick={(e) => {
            select()
            // Android convention: re-tapping the active tab scrolls its page
            // back to the top. aria-current is set by NavLink while active.
            if (e.currentTarget.getAttribute('aria-current') === 'page') {
              document
                .querySelector('main .overflow-y-auto')
                ?.scrollTo({ top: 0, behavior: 'smooth' })
            }
          }}
          className={({ isActive }) =>
            `pressable flex min-h-[56px] min-w-[48px] flex-1 flex-col items-center justify-center gap-1 text-[12px] font-medium transition-colors ${
              isActive ? 'text-[var(--accent)]' : 'text-text-2'
            }`
          }
        >
          {({ isActive }) => (
            <>
              {/* Material 3 active pill (capsula 64×32dp) dietro la sola icona;
                  il wrapper relativo tiene la pill assoluta centrata sul glifo. */}
              <span className="relative flex h-8 w-16 max-w-full items-center justify-center">
                <span className="nav-pill" aria-hidden />
                {/* Icona "filled" da attiva (cue M3): il riempimento accent-soft è
                    applicato via CSS (.bottom-nav a[aria-current] svg) perché le
                    CSS var non si risolvono in un attributo SVG fill. */}
                <Icon
                  size={24}
                  strokeWidth={isActive ? 2.4 : 2}
                  className={`relative ${isActive ? 'drop-shadow-[0_0_6px_var(--accent-glow)]' : ''}`}
                />
              </span>
              <span className="max-w-full truncate px-0.5 leading-none">{t(shortKey ?? key)}</span>
            </>
          )}
        </NavLink>
      ))}
    </nav>
  )
}
