import React from 'react'
import ReactDOM from 'react-dom/client'
import { HashRouter } from 'react-router-dom'
import '@fontsource-variable/inter'
// Skin fonts (Nothing/Cyberpunk) load lazily on first activation — see
// SKIN_FONTS in lib/skins.ts. Only Inter, the base font, is eager.
import './styles/global.css'
import './i18n'
import App from './App'
import ErrorBoundary from '@/components/ui/ErrorBoundary'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <HashRouter>
        <App />
      </HashRouter>
    </ErrorBoundary>
  </React.StrictMode>
)
