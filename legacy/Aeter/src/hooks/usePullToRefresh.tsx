import type { ReactNode } from 'react'

/**
 * Pull-to-refresh is a touch gesture with no desktop equivalent (the desktop UI
 * uses explicit refresh buttons instead), so this is a no-op here: it returns
 * empty pull props and no indicator. Kept so shared components can call it
 * unconditionally.
 */
export function usePullToRefresh(_onRefresh?: () => Promise<unknown> | void): {
  pullProps: Record<string, unknown>
  indicator: ReactNode
} {
  return { pullProps: {}, indicator: null }
}
