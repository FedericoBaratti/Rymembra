/**
 * L'ingresso: da JSON sconosciuto a documento valido, o a un errore che dice cosa
 * rifiutare.
 *
 * Il messaggio conta più del solito. Chi crea una skin deve sapere COSA non va —
 * quale token, quale valore — non solo che il pacchetto è stato rifiutato. Nel
 * legacy il problema non si poneva perché una skin era CSS: un valore sbagliato
 * non veniva rifiutato affatto, veniva ignorato dal browser, e il risultato era
 * una skin rotta in un punto senza alcun messaggio da nessuna parte.
 */

import { AppError } from '@aether/core'
import { err, ok, type Result } from '@aether/core'
import type { z } from 'zod'
import {
  SKIN_FORMAT_VERSION,
  skinDocumentSchema,
  type SkinDocument
} from './schema'
import { REQUIRED_TOKEN_IDS, TOKENS, type TokenId } from './tokens'

/** Un problema in un punto preciso del documento. */
export interface SkinIssue {
  /** Il percorso nel documento: `tokens.color.accent`. */
  readonly path: string
  readonly message: string
}

export interface SkinWarning extends SkinIssue {
  readonly kind: 'missingRequiredToken' | 'unusedPattern' | 'costBudget'
}

function formatIssues(error: z.ZodError): SkinIssue[] {
  return error.issues.slice(0, 20).map((issue) => ({
    path: issue.path.length > 0 ? issue.path.join('.') : '(radice)',
    message: issue.message
  }))
}

/**
 * Valida un documento skin.
 *
 * La versione del formato si controlla PRIMA dello schema, e separatamente: un
 * pacchetto scritto da una versione futura dell'app non è malformato, è solo più
 * nuovo, e i due casi vogliono messaggi diversi. È la stessa distinzione che nel
 * database separa `db.migrationFailed` da `db.versionAhead`.
 */
export function parseSkin(input: unknown): Result<SkinDocument, AppError> {
  const format = readFormat(input)
  if (format !== null && format !== SKIN_FORMAT_VERSION) {
    return err(
      AppError.of('skin.formatUnsupported', {
        found: format,
        supported: SKIN_FORMAT_VERSION
      })
    )
  }

  const parsed = skinDocumentSchema.safeParse(input)
  if (!parsed.success) {
    const issues = formatIssues(parsed.error)
    return err(
      AppError.of(
        'skin.manifestInvalid',
        {
          detail: issues
            .map((issue) => `${issue.path}: ${issue.message}`)
            .join('; ')
        },
        { context: { issues } }
      )
    )
  }

  return ok(parsed.data)
}

function readFormat(input: unknown): number | null {
  if (typeof input !== 'object' || input === null) return null
  const value = (input as { format?: unknown }).format
  return typeof value === 'number' ? value : null
}

/**
 * Gli avvisi: cose che non impediscono di usare la skin ma che chi la crea deve
 * vedere.
 *
 * Il primo è quello che nel legacy costava un mese di silenzio. Un token
 * obbligatorio non dichiarato non è un errore — la skin erediterà quello di base
 * — ma quasi sempre è una dimenticanza, e il risultato è una superficie che resta
 * del colore sbagliato in una schermata che si visita raramente.
 */
export function checkSkin(skin: SkinDocument): SkinWarning[] {
  const warnings: SkinWarning[] = []
  const declared = new Set(Object.keys(skin.tokens))

  for (const id of REQUIRED_TOKEN_IDS) {
    if (!declared.has(id)) {
      warnings.push({
        kind: 'missingRequiredToken',
        path: `tokens.${id}`,
        message: `${TOKENS[id].description} Non dichiarato: erediterà il valore di base.`
      })
    }
  }

  // Un tema chiaro dichiarato ma vuoto è una promessa non mantenuta: l'interruttore
  // del tema apparirebbe e non farebbe niente.
  if (skin.capabilities.light && (skin.themes?.light === undefined)) {
    warnings.push({
      kind: 'missingRequiredToken',
      path: 'themes.light',
      message: 'La skin dichiara di avere una variante chiara ma non la definisce.'
    })
  }

  return warnings
}

/** I token che una skin dichiara. Serve allo Studio e al confronto fra librerie. */
export function declaredTokens(skin: SkinDocument): TokenId[] {
  return Object.keys(skin.tokens) as TokenId[]
}
