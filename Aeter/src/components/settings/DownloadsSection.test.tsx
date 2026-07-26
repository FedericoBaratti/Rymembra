import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import '@/i18n'
import { mockAether } from '@/test/setup'
import { useSettingsStore } from '@/store/useSettingsStore'
import { TEST_SETTINGS } from '@/test/fixtures'
import DownloadsSection from './DownloadsSection'

const BIN_OK = { found: true, dir: 'C:\\bin' }
const STATUS = {
  'yt-dlp': BIN_OK,
  ffmpeg: BIN_OK,
  spotdl: BIN_OK,
  fpcalc: { found: false, dir: 'C:\\bin' }
}

beforeEach(() => {
  useSettingsStore.setState({ settings: { ...TEST_SETTINGS } })
})

describe('DownloadsSection', () => {
  it('renders the external tools status chips', async () => {
    mockAether({ getBinaryStatus: vi.fn(() => Promise.resolve(STATUS)) })
    render(<DownloadsSection />)
    expect(await screen.findByText('Strumenti esterni')).toBeInTheDocument()
    expect(screen.getByText('fpcalc')).toBeInTheDocument()
    expect(screen.getByText('spotdl')).toBeInTheDocument()
  })

  it('disables the yt-dlp button while updating and shows the new version', async () => {
    let resolve!: (v: { updated: boolean; version: string }) => void
    const updateYtDlp = vi.fn(
      () => new Promise<{ updated: boolean; version: string }>((r) => (resolve = r))
    )
    mockAether({ getBinaryStatus: vi.fn(() => Promise.resolve(STATUS)), updateYtDlp })
    render(<DownloadsSection />)

    const btn = screen.getByRole('button', { name: /Aggiorna yt-dlp/ })
    await userEvent.click(btn)
    expect(btn).toBeDisabled()
    expect(updateYtDlp).toHaveBeenCalledOnce()

    resolve({ updated: true, version: '2026.06.01' })
    await waitFor(() => expect(btn).toBeEnabled())
    expect(screen.getByText('yt-dlp aggiornato: 2026.06.01')).toBeInTheDocument()
  })

  it('shows the translated IPC error when the update fails', async () => {
    const updateYtDlp = vi.fn(() =>
      Promise.reject(new Error("Error invoking remote method 'updateYtDlp': Error: DL_YTDLP_TIMEOUT"))
    )
    mockAether({ getBinaryStatus: vi.fn(() => Promise.resolve(STATUS)), updateYtDlp })
    render(<DownloadsSection />)

    await userEvent.click(screen.getByRole('button', { name: /Aggiorna yt-dlp/ }))
    expect(
      await screen.findByText('Timeout: yt-dlp non ha risposto entro 30 secondi.')
    ).toBeInTheDocument()
  })
})
