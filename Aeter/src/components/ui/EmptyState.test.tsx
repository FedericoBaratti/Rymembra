import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Music2 } from 'lucide-react'
import EmptyState from './EmptyState'

describe('EmptyState', () => {
  it('renders title only', () => {
    render(<EmptyState icon={Music2} title="Libreria vuota" />)
    expect(screen.getByRole('heading', { name: 'Libreria vuota' })).toBeInTheDocument()
  })

  it('renders subtitle and action when provided', () => {
    render(
      <EmptyState
        icon={Music2}
        title="Nessun brano"
        subtitle="Aggiungi una cartella"
        action={<button>Scegli cartella</button>}
      />
    )
    expect(screen.getByText('Aggiungi una cartella')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Scegli cartella' })).toBeInTheDocument()
  })
})
