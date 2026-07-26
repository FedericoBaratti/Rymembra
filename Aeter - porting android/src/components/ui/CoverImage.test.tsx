// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import CoverImage from './CoverImage'

describe('CoverImage', () => {
  it('renders the fallback when src is missing', () => {
    render(<CoverImage src={null} fallback={<span data-testid="fb" />} />)
    expect(screen.getByTestId('fb')).toBeTruthy()
    expect(document.querySelector('img')).toBeNull()
  })

  it('renders the image when src is set', () => {
    render(<CoverImage src="blob:cover-1" className="h-4" fallback={<span data-testid="fb" />} />)
    const img = document.querySelector('img')
    expect(img?.getAttribute('src')).toBe('blob:cover-1')
    expect(img?.getAttribute('draggable')).toBe('false')
    expect(screen.queryByTestId('fb')).toBeNull()
  })

  it('swaps to the fallback on load error, and retries on a new src', () => {
    const { rerender } = render(
      <CoverImage src="blob:cover-1" fallback={<span data-testid="fb" />} />
    )
    fireEvent.error(document.querySelector('img')!)
    expect(screen.getByTestId('fb')).toBeTruthy()
    expect(document.querySelector('img')).toBeNull()

    rerender(<CoverImage src="blob:cover-2" fallback={<span data-testid="fb" />} />)
    expect(document.querySelector('img')?.getAttribute('src')).toBe('blob:cover-2')
  })

  it('fades in after load', () => {
    render(<CoverImage src="blob:cover-1" />)
    const img = document.querySelector('img')!
    expect(img.style.opacity).toBe('0')
    fireEvent.load(img)
    expect(img.style.opacity).toBe('1')
  })
})
