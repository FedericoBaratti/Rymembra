import { useGridArrowNav } from '@/hooks/useGridArrowNav'

/**
 * Fluid auto-fill card grid. Cards must be direct children (buttons/links)
 * for useGridArrowNav's focus traversal. Density responds to the "content"
 * container via --card-min (see .media-grid in global.css).
 */
export default function MediaGrid({
  children,
  dense = false
}: {
  children: React.ReactNode
  dense?: boolean
}): React.JSX.Element {
  const onKeyDown = useGridArrowNav()
  return (
    <div className={`media-grid stagger ${dense ? 'media-grid--dense' : ''}`} onKeyDown={onKeyDown}>
      {children}
    </div>
  )
}
