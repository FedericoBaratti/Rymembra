import { useLayoutEffect, useState } from 'react'
import { flushSync } from 'react-dom'
import { useLocation, type Location } from 'react-router-dom'
import { useTourStore } from '@/store/useTourStore'

/**
 * Deferred-location pattern: route content renders against `displayed`,
 * which catches up to the live location inside a View Transition so route
 * swaps cross-fade (see ::view-transition rules in global.css). Falls back
 * to an instant swap when unsupported, under reduced motion, or while the
 * onboarding tour is driving navigation.
 */
export function useRouteTransition(): Location {
  const location = useLocation()
  const [displayed, setDisplayed] = useState(location)

  useLayoutEffect(() => {
    if (location.key === displayed.key) return
    const skip =
      !document.startViewTransition ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches ||
      useTourStore.getState().active
    if (skip) {
      setDisplayed(location)
      return
    }
    document.startViewTransition(() => {
      flushSync(() => setDisplayed(location))
    })
  }, [location, displayed.key])

  return displayed
}
