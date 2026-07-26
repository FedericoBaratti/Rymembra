import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import i18n from '@/i18n'
import { mockAether } from '@/test/setup'
import { useSettingsStore } from '@/store/useSettingsStore'
import { TEST_SETTINGS } from '@/test/fixtures'
import AppearanceSection from './AppearanceSection'

beforeEach(() => {
  useSettingsStore.setState({ settings: { ...TEST_SETTINGS } })
})

afterEach(async () => {
  await i18n.changeLanguage('it')
})

describe('AppearanceSection', () => {
  it('persists the language change and switches i18n', async () => {
    const setSettings = vi.fn((patch: Partial<typeof TEST_SETTINGS>) =>
      Promise.resolve({ ...TEST_SETTINGS, ...patch })
    )
    mockAether({ setSettings })
    render(<AppearanceSection />)

    await userEvent.selectOptions(screen.getByDisplayValue('Italiano'), 'en')

    expect(setSettings).toHaveBeenCalledWith({ language: 'en' })
    expect(i18n.language).toBe('en')
  })

  it('persists the theme change', async () => {
    const setSettings = vi.fn((patch: Partial<typeof TEST_SETTINGS>) =>
      Promise.resolve({ ...TEST_SETTINGS, ...patch })
    )
    mockAether({ setSettings })
    render(<AppearanceSection />)

    await userEvent.selectOptions(screen.getByDisplayValue('Scuro'), 'light')

    expect(setSettings).toHaveBeenCalledWith({ theme: 'light' })
  })

  it('persists the skin change and applies it to the document', async () => {
    const setSettings = vi.fn((patch: Partial<typeof TEST_SETTINGS>) =>
      Promise.resolve({ ...TEST_SETTINGS, ...patch })
    )
    mockAether({ setSettings })
    render(<AppearanceSection />)

    await userEvent.selectOptions(screen.getByDisplayValue('Plain'), 'nothing')

    expect(setSettings).toHaveBeenCalledWith({ skin: 'nothing' })
    expect(document.documentElement.dataset.skin).toBe('nothing')
  })
})
