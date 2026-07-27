/**
 * La fetta verticale.
 *
 * Non è l'interfaccia di Aether: è la prova che l'architettura regge da un capo
 * all'altro. Mostra le quattro cose che nel legacy non si potevano vedere da
 * dentro l'app:
 *
 *   1. lo stato del database, ANCHE quando non si è aperto — il caso in cui prima
 *      il renderer restava sugli scheletri per sempre, senza messaggio;
 *   2. le capacità della piattaforma, invece di indovinarle;
 *   3. le ultime righe di log, dal ring buffer, senza rileggere il file;
 *   4. la skin attiva, compilata dal formato dichiarativo e applicata come foglio
 *      adottato.
 *
 * La convenzione di stato è quella della Fase 7: ogni cosa caricata ha
 * `{ data, status, error }`. Nel legacy solo uno store su dieci lo faceva; gli
 * altri ripiegavano su `loaded: true` con lista vuota, che rende un guasto
 * indistinguibile da una libreria vuota.
 */

import { useCallback, useEffect, useState } from 'react'
import { applySkinCss, markActiveSkin } from './skinRuntime'

type Status = 'idle' | 'loading' | 'ready' | 'error'

interface Async<T> {
  data: T | null
  status: Status
  /** Il CODICE dell'errore, non una frase: la frase la costruisce la UI. */
  error: string | null
}

const empty = <T,>(): Async<T> => ({ data: null, status: 'idle', error: null })

interface DbStatus {
  status: 'closed' | 'open' | 'failed'
  version: number
  fts5: boolean
  path: string
  errorCode?: string
}

interface Capabilities {
  label: string
  fts5: boolean
  spawn: boolean
  skinStudio: boolean
  lanServer: boolean
}

interface LogRow {
  ts: number
  level: string
  scope: string
  message: string
  errorCode?: string
}

interface Settings {
  skin: string
  theme: 'dark' | 'light'
  volume: number
  motionIntensity: 'none' | 'essential' | 'full' | 'maximum'
}

interface SkinSummary {
  id: string
  name: string
  author: string
  version: string
  builtin: boolean
}

/** Il tipo di `window.aether`, derivato a mano qui perché la fetta è piccola. */
interface AetherBridge {
  'diagnostics:db': () => Promise<DbStatus>
  'diagnostics:capabilities': () => Promise<Capabilities>
  'diagnostics:recentLogs': (input: { limit: number }) => Promise<LogRow[]>
  'diagnostics:reopenDb': () => Promise<DbStatus>
  'settings:get': () => Promise<Settings>
  'settings:set': (patch: Partial<Settings>) => Promise<Settings>
  'skins:list': () => Promise<SkinSummary[]>
  'skins:css': (input: { id: string }) => Promise<{ id: string; css: string; cost: number }>
  'library:counts': () => Promise<{ tracks: number; albums: number; artists: number }>
  onFatal: (handler: (payload: unknown) => void) => () => void
}

declare global {
  interface Window {
    aether: AetherBridge
  }
}

/**
 * Legge l'errore come CODICE.
 *
 * Il preload ricostruisce un AppError con tutti i suoi campi, quindi qui si legge
 * `code` e non si spreme un messaggio con una regex — che è ciò che
 * `src/lib/ipcError.ts` faceva, con due tabelle scritte a mano per rimettere
 * insieme l'identità dell'errore.
 */
function errorCodeOf(cause: unknown): string {
  if (typeof cause === 'object' && cause !== null && 'code' in cause) {
    return String((cause as { code: unknown }).code)
  }
  return 'internal.unexpected'
}

function useAsync<T>(load: () => Promise<T>, deps: unknown[] = []): [Async<T>, () => void] {
  const [state, setState] = useState<Async<T>>(empty<T>())

  const run = useCallback(() => {
    setState((previous) => ({ ...previous, status: 'loading' }))
    load()
      .then((data) => setState({ data, status: 'ready', error: null }))
      // Un guasto NON diventa un dato vuoto: è la distinzione che nel legacy si
      // perdeva, e con lei la possibilità di dire all'utente cosa fare.
      .catch((cause: unknown) => setState({ data: null, status: 'error', error: errorCodeOf(cause) }))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)

  useEffect(run, [run])
  return [state, run]
}

export function App(): JSX.Element {
  const [db, reloadDb] = useAsync(() => window.aether['diagnostics:db']())
  const [caps] = useAsync(() => window.aether['diagnostics:capabilities']())
  const [settings, reloadSettings] = useAsync(() => window.aether['settings:get']())
  const [skins] = useAsync(() => window.aether['skins:list']())
  const [counts] = useAsync(() => window.aether['library:counts']())
  const [logs, reloadLogs] = useAsync(() =>
    window.aether['diagnostics:recentLogs']({ limit: 40 })
  )
  const [fatal, setFatal] = useState<string | null>(null)

  // Il guasto fatale arriva come evento e si vede: nel legacy il desktop non
  // aveva nessuna rete di sicurezza, quindi l'app moriva e basta.
  useEffect(() => window.aether.onFatal((payload) => setFatal(errorCodeOf(payload))), [])

  const activeSkin = settings.data?.skin ?? 'plain'
  const activeTheme = settings.data?.theme ?? 'dark'

  useEffect(() => {
    let cancelled = false
    void window.aether['skins:css']({ id: activeSkin })
      .then((compiled) => {
        if (cancelled) return
        applySkinCss(compiled.css)
        markActiveSkin(compiled.id, activeTheme)
      })
      .catch(() => {
        // Una skin che non compila non deve lasciare la finestra senza stile: si
        // resta su quella precedente, che è ancora adottata.
      })
    return () => {
      cancelled = true
    }
  }, [activeSkin, activeTheme])

  const setTheme = (theme: 'dark' | 'light'): void => {
    void window.aether['settings:set']({ theme }).then(() => reloadSettings())
  }

  return (
    <main
      style={{
        minHeight: '100%',
        padding: '32px',
        fontFamily: 'var(--font-sans)',
        background: 'var(--color-surface-0)',
        color: 'var(--color-text-1)'
      }}
    >
      <h1 style={{ fontSize: '22px', margin: '0 0 4px' }}>Aether — fetta verticale</h1>
      <p style={{ color: 'var(--color-text-2)', margin: '0 0 28px', fontSize: '14px' }}>
        Core riscritto, contratto IPC tipizzato, skin compilata dal formato dichiarativo.
      </p>

      {fatal !== null && (
        <Card title="Guasto di processo" tone="danger">
          <code>{fatal}</code>
          <p style={{ color: 'var(--color-text-2)', fontSize: '13px' }}>
            Il supervisor ha messo in salvo i dati e l&apos;app è ancora viva. Nel legacy
            questo era il punto in cui l&apos;applicazione moriva.
          </p>
        </Card>
      )}

      <div style={{ display: 'grid', gap: '16px', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))' }}>
        <Card title="Database">
          <Async state={db}>
            {(value) => (
              <>
                <Row label="stato" value={value.status} />
                <Row label="versione schema" value={String(value.version)} />
                <Row label="FTS5" value={value.fts5 ? 'sì' : 'no (ripiego afold)'} />
                {value.errorCode !== undefined && <Row label="errore" value={value.errorCode} />}
                <button type="button" onClick={() => void window.aether['diagnostics:reopenDb']().then(reloadDb)}>
                  Riprova ad aprire
                </button>
              </>
            )}
          </Async>
        </Card>

        <Card title="Capacità">
          <Async state={caps}>
            {(value) => (
              <>
                <Row label="piattaforma" value={value.label} />
                <Row label="FTS5" value={String(value.fts5)} />
                <Row label="spawn" value={String(value.spawn)} />
                <Row label="Skin Studio" value={String(value.skinStudio)} />
                <Row label="server LAN" value={String(value.lanServer)} />
              </>
            )}
          </Async>
        </Card>

        <Card title="Libreria">
          <Async state={counts}>
            {(value) => (
              <>
                <Row label="tracce" value={String(value.tracks)} />
                <Row label="album" value={String(value.albums)} />
                <Row label="artisti" value={String(value.artists)} />
              </>
            )}
          </Async>
        </Card>

        <Card title="Aspetto">
          <Async state={skins}>
            {(value) => (
              <>
                {value.map((skin) => (
                  <Row
                    key={skin.id}
                    label={skin.name}
                    value={`${skin.version}${skin.builtin ? ' · di serie' : ''}`}
                  />
                ))}
                <Row label="attiva" value={activeSkin} />
                <div style={{ display: 'flex', gap: '8px', marginTop: '10px' }}>
                  <button type="button" onClick={() => setTheme('dark')}>Scuro</button>
                  <button type="button" onClick={() => setTheme('light')}>Chiaro</button>
                </div>
              </>
            )}
          </Async>
        </Card>
      </div>

      <Card title="Ultime righe di log">
        <Async state={logs}>
          {(value) => (
            <>
              <button type="button" onClick={reloadLogs} style={{ marginBottom: '10px' }}>
                Aggiorna
              </button>
              <pre style={{ margin: 0, fontSize: '12px', color: 'var(--color-text-2)', overflowX: 'auto' }}>
                {value
                  .map((row) => `${row.level.padEnd(5)} [${row.scope}] ${row.message}`)
                  .join('\n')}
              </pre>
            </>
          )}
        </Async>
      </Card>
    </main>
  )
}

function Async<T>({
  state,
  children
}: {
  state: Async<T>
  children: (data: T) => JSX.Element
}): JSX.Element {
  if (state.status === 'loading' || state.status === 'idle') {
    return <p style={{ color: 'var(--color-text-3)', fontSize: '13px' }}>caricamento…</p>
  }
  if (state.status === 'error') {
    // Il codice si mostra: è confrontabile, cercabile e traducibile — dove una
    // frase spremuta da una regex non è nessuna delle tre.
    return (
      <p style={{ color: 'var(--danger)', fontSize: '13px' }}>
        errore: <code>{state.error}</code>
      </p>
    )
  }
  return state.data === null ? <p /> : children(state.data)
}

function Card({
  title,
  tone,
  children
}: {
  title: string
  tone?: 'danger'
  children: React.ReactNode
}): JSX.Element {
  return (
    <section
      style={{
        background: 'var(--color-surface-1)',
        border: `1px solid ${tone === 'danger' ? 'var(--danger)' : 'var(--hairline)'}`,
        borderRadius: 'var(--radius-card)',
        boxShadow: 'var(--shadow-1)',
        padding: '18px',
        marginTop: '16px'
      }}
    >
      <h2 style={{ fontSize: '13px', textTransform: 'uppercase', letterSpacing: '0.08em', margin: '0 0 12px', color: 'var(--color-text-2)' }}>
        {title}
      </h2>
      {children}
    </section>
  )
}

function Row({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', fontSize: '14px', padding: '3px 0' }}>
      <span style={{ color: 'var(--color-text-2)' }}>{label}</span>
      <span style={{ fontVariantNumeric: 'tabular-nums' }}>{value}</span>
    </div>
  )
}
