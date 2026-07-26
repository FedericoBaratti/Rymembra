import { useTranslation } from 'react-i18next'
import { Search } from 'lucide-react'
import { useUiStore } from '@/store/useUiStore'

/**
 * Opens the global search overlay (tracks/albums/artists). Dropped into mobile
 * page headers so search is reachable from Home/Albums/Playlists/Podcasts, not
 * only the Library tab. Desktop keeps Ctrl/Cmd+F, so callers gate this by
 * isMobile.
 */
export default function SearchButton(): React.JSX.Element {
  const { t } = useTranslation()
  return (
    <button
      className="icon-btn h-11 w-11"
      onClick={() => useUiStore.getState().setSearchOpen(true)}
      aria-label={t('search.placeholder')}
    >
      <Search size={20} />
    </button>
  )
}
