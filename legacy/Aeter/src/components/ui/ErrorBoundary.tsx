import { Component, type ErrorInfo, type ReactNode } from 'react'
import { AlertTriangle, RotateCcw } from 'lucide-react'
import i18n from '@/i18n'

interface Props {
  children: ReactNode
  /** Change this value (e.g. route pathname) to retry rendering after a crash. */
  resetKey?: string
}

interface State {
  error: Error | null
}

export default class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[ErrorBoundary]', error, info.componentStack)
  }

  componentDidUpdate(prevProps: Props): void {
    if (this.state.error && prevProps.resetKey !== this.props.resetKey) {
      this.setState({ error: null })
    }
  }

  render(): ReactNode {
    if (!this.state.error) return this.props.children
    return (
      <div className="flex h-full min-h-0 flex-1 flex-col items-center justify-center gap-4 p-8 text-center">
        <AlertTriangle size={40} className="text-amber-400" />
        <div>
          <h2 className="text-lg font-semibold">{i18n.t('errors.boundary_title')}</h2>
          <p className="mt-1 max-w-md text-sm opacity-70">{i18n.t('errors.boundary_detail')}</p>
          <p className="mt-2 max-w-md break-all font-mono text-xs opacity-50">
            {this.state.error.message}
          </p>
        </div>
        <button
          onClick={() => window.location.reload()}
          className="flex items-center gap-2 rounded-lg bg-white/10 px-4 py-2 text-sm hover:bg-white/15"
        >
          <RotateCcw size={16} />
          {i18n.t('errors.boundary_reload')}
        </button>
      </div>
    )
  }
}
