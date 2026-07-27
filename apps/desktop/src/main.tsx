/**
 * Il punto d'ingresso del renderer.
 *
 * Due cose che il legacy non faceva.
 *
 * **Il logger esiste anche qui.** Nel legacy il renderer non ne aveva nessuno,
 * solo `console.*` — quindi di un guasto nell'interfaccia non restava traccia da
 * nessuna parte. Lo stesso logger del backend gira anche qui, coi sink iniettati.
 *
 * **Un errore di rendering non lascia una finestra bianca.** `componentDidCatch`
 * nel legacy faceva solo `console.error`, e l'utente vedeva il nulla.
 */

import { StrictMode, Component, type ErrorInfo, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { configureLogger, createConsoleSink, logger } from '@aether/core'
import { App } from './App'

configureLogger({ minLevel: 'debug', sinks: [createConsoleSink()] })

const log = logger('renderer')

interface BoundaryState {
  code: string | null
}

class ErrorBoundary extends Component<{ children: ReactNode }, BoundaryState> {
  override state: BoundaryState = { code: null }

  static getDerivedStateFromError(cause: unknown): BoundaryState {
    const code =
      typeof cause === 'object' && cause !== null && 'code' in cause
        ? String((cause as { code: unknown }).code)
        : 'internal.unexpected'
    return { code }
  }

  override componentDidCatch(cause: unknown, info: ErrorInfo): void {
    // Nel logger, con lo stack dei componenti: è l'informazione che serve per
    // capire QUALE superficie è caduta, e che console.error da sola non conserva.
    log.error('superficie caduta', cause, { componentStack: info.componentStack })
  }

  override render(): ReactNode {
    if (this.state.code === null) return this.props.children
    return (
      <div style={{ padding: '32px', fontFamily: 'system-ui', color: '#fff', background: '#09090d', minHeight: '100vh' }}>
        <h1 style={{ fontSize: '18px' }}>Questa parte dell&apos;interfaccia non si è caricata</h1>
        <p style={{ opacity: 0.7, fontSize: '14px' }}>
          Codice: <code>{this.state.code}</code>. Il resto dell&apos;app continua a funzionare.
        </p>
        <button type="button" onClick={() => this.setState({ code: null })}>
          Riprova
        </button>
      </div>
    )
  }
}

const container = document.getElementById('root')
if (container === null) {
  // Senza questo, un index.html sbagliato darebbe una finestra bianca e nessun
  // messaggio: il tipo esatto di guasto silenzioso che stiamo eliminando.
  log.fatal('elemento #root assente: il documento non è quello atteso')
} else {
  createRoot(container).render(
    <StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </StrictMode>
  )
}
