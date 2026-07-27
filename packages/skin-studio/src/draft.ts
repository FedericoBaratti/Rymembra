/**
 * La bozza: il modello di modifica dello Studio.
 *
 * Lavora sulla forma **sorgente**, non su quella validata, e questa non è una
 * scelta di comodo: l'ho scoperta sbagliando nel formato di pacchetto. Nella forma
 * validata un colore è `{r:139,g:124,b:246,a:1}`, quindi un editor che modificasse
 * quella produrrebbe un manifest che la validazione rifiuta. E c'è una ragione più
 * profonda: la sorgente è ciò che l'autore ha scritto — `"#8b7cf6"` — ed è quello
 * che deve rivedere in un campo di testo, non la sua traduzione in canali.
 *
 * Il ciclo è: si modifica la sorgente, si valida, e se valida si compila. Ogni
 * modifica produce un esito completo — errori, avvisi, contrasto, costo, CSS — così
 * l'anteprima dal vivo e il pannello diagnostico leggono lo stesso oggetto e non
 * possono mostrare cose diverse.
 */

import type { AppError } from '@aether/core'
import {
  TOKENS,
  compileSkin,
  checkSkin,
  parseSkin,
  type SkinDocument,
  type SkinWarning,
  type TokenId
} from '@aether/skin'
import { auditContrast, type ContrastReport } from './contrast'

/** La sorgente su cui la bozza lavora: JSON come lo scrive un autore. */
export type SkinSource = Record<string, unknown>

export interface DraftState {
  readonly source: SkinSource
  /** Presente quando la sorgente valida. */
  readonly document: SkinDocument | null
  /** Presente quando non valida. Porta il campo e il motivo. */
  readonly error: AppError | null
  readonly warnings: readonly SkinWarning[]
  /** Il CSS compilato, pronto per `adoptedStyleSheets`. Vuoto se non valida. */
  readonly css: string
  /** Costo degli effetti, per il budget. */
  readonly cost: number
  readonly contrast: ContrastReport | null
  readonly contrastLight: ContrastReport | null
  /** Se la bozza differisce dalla sorgente da cui è partita. */
  readonly dirty: boolean
}

/**
 * Valuta una sorgente.
 *
 * Non lancia in nessun caso: una sorgente a metà modifica è invalida per la
 * maggior parte del tempo — l'utente sta scrivendo — e un editor che si rompe
 * mentre si scrive è inutilizzabile. Lo stato invalido è uno stato normale, con
 * il suo messaggio.
 */
export function evaluate(source: SkinSource, dirty: boolean): DraftState {
  const parsed = parseSkin(source)

  if (!parsed.ok) {
    return {
      source,
      document: null,
      error: parsed.error,
      warnings: [],
      css: '',
      cost: 0,
      contrast: null,
      contrastLight: null,
      dirty
    }
  }

  const document = parsed.value
  const compiled = compileSkin(document)

  return {
    source,
    document,
    error: compiled.ok ? null : compiled.error,
    warnings: checkSkin(document),
    css: compiled.ok ? compiled.value.css : '',
    cost: compiled.ok ? compiled.value.cost : 0,
    contrast: auditContrast(document, 'base'),
    // Il tema chiaro si verifica sempre, anche quando l'autore sta lavorando sullo
    // scuro: è la variante che si prova meno, e quindi quella che ha il contrasto
    // peggiore.
    contrastLight: document.themes?.light !== undefined ? auditContrast(document, 'light') : null,
    dirty
  }
}

export interface Draft {
  readonly state: DraftState
  /** Cambia un token. `undefined` lo rimuove, tornando al valore ereditato. */
  setToken(id: TokenId, value: unknown): Draft
  /** Cambia un token della variante chiara. */
  setThemeToken(id: TokenId, value: unknown): Draft
  /** Cambia un colore della tavolozza locale. */
  setPaletteColor(name: string, value: string | undefined): Draft
  /** Cambia un campo dei metadati. */
  setMeta(field: 'name' | 'author' | 'version' | 'description' | 'license', value: string): Draft
  /** Sostituisce la sorgente per intero: serve all'importazione e all'annulla. */
  replace(source: SkinSource): Draft
  /** Riporta la bozza al punto di partenza. */
  reset(): Draft
  /** La sorgente da scrivere nel pacchetto. */
  toSource(): SkinSource
}

function withPatch(base: SkinSource, original: SkinSource, patch: SkinSource): Draft {
  const next = { ...base, ...patch }
  return makeDraft(next, original)
}

/** Confronto strutturale, per sapere se la bozza è sporca. */
function sameSource(a: SkinSource, b: SkinSource): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

function makeDraft(source: SkinSource, original: SkinSource): Draft {
  const state = evaluate(source, !sameSource(source, original))

  return {
    state,

    setToken(id, value) {
      const tokens = { ...((source['tokens'] as SkinSource) ?? {}) }
      // Rimuovere invece di scrivere `undefined`: uno schema `strict()` accetta la
      // chiave assente, non la chiave con valore indefinito, e "non l'ho scelto"
      // deve essere distinguibile da "l'ho scelto vuoto".
      if (value === undefined) delete tokens[id]
      else tokens[id] = value
      return withPatch(source, original, { tokens })
    },

    setThemeToken(id, value) {
      const themes = { ...((source['themes'] as SkinSource) ?? {}) }
      const light = { ...((themes['light'] as SkinSource) ?? {}) }
      if (value === undefined) delete light[id]
      else light[id] = value

      if (Object.keys(light).length === 0) {
        delete themes['light']
        // Se non resta niente nel tema chiaro, sparisce anche il blocco: un
        // `themes: {}` vuoto è rumore in un file che qualcuno leggerà.
        return withPatch(
          source,
          original,
          Object.keys(themes).length === 0 ? { themes: undefined } : { themes }
        )
      }

      themes['light'] = light
      return withPatch(source, original, { themes })
    },

    setPaletteColor(name, value) {
      const palette = { ...((source['palette'] as SkinSource) ?? {}) }
      if (value === undefined) delete palette[name]
      else palette[name] = value
      return withPatch(
        source,
        original,
        Object.keys(palette).length === 0 ? { palette: undefined } : { palette }
      )
    },

    setMeta(field, value) {
      const meta = { ...((source['meta'] as SkinSource) ?? {}) }
      meta[field] = value
      return withPatch(source, original, { meta })
    },

    replace: (next) => makeDraft(next, original),
    reset: () => makeDraft(original, original),
    toSource: () => source
  }
}

export function createDraft(source: SkinSource): Draft {
  return makeDraft(source, source)
}

/**
 * Fork di una skin esistente, comprese quelle di serie.
 *
 * È possibile solo perché le skin sono dati: nel legacy erano CSS nel bundle, e
 * "parti da cyberpunk e cambia due colori" significava copiare 1.879 righe di CSS
 * a mano.
 *
 * L'id cambia per forza: due skin con lo stesso id collidono nel selettore e nel
 * nome di file. E `basedOn` registra la provenienza — serve all'allineamento fra
 * PC e telefono per sapere che due skin sono parenti.
 */
export function forkSkin(source: SkinSource, newId: string, newName: string): Draft {
  const meta = { ...((source['meta'] as SkinSource) ?? {}) }
  const originalId = source['id']

  return createDraft({
    ...source,
    id: newId,
    meta: {
      ...meta,
      name: newName,
      // La versione ricomincia: è una skin nuova, non un aggiornamento di quella
      // da cui viene.
      version: '1.0.0',
      ...(typeof originalId === 'string' ? { basedOn: originalId } : {})
    }
  })
}

/**
 * I token di un gruppo con il loro valore corrente, per costruire un pannello.
 *
 * Il pannello si genera dal registro invece di essere scritto a mano: aggiungere
 * un token lo rende automaticamente editabile, che è metà del motivo per cui il
 * registro esiste.
 */
export function panelFor(
  state: DraftState,
  ids: readonly TokenId[]
): readonly {
  id: TokenId
  kind: string
  description: string
  required: boolean
  value: unknown
  inherited: boolean
}[] {
  const tokens = (state.source['tokens'] as SkinSource) ?? {}
  return ids.map((id) => {
    const definition = TOKENS[id]
    const value = tokens[id]
    return {
      id,
      kind: definition.kind,
      description: definition.description,
      required: definition.required,
      value,
      inherited: value === undefined
    }
  })
}
