/**
 * Sink su file con rotazione.
 *
 * Il legacy non ce l'aveva: `logWarn`/`logError` scrivevano su `console`, e nel
 * pacchetto distribuito la console non esiste. Quindi di un guasto sul computer di
 * un utente non restava NIENTE — nessun file, nessun report, nessun modo di
 * chiedere «cosa dicono i log?».
 *
 * Le scelte non ovvie, tutte per la stessa ragione (un logger non deve mai essere
 * il motivo per cui qualcosa fallisce):
 *
 *   - la scrittura è accodata e sincrona a lotti, non un `writeFileSync` per riga:
 *     una scansione da 100.000 file emetterebbe centomila aperture di file;
 *   - ma il lotto ha anche una SCADENZA, non solo una dimensione. Senza, un avvio
 *     che emette dodici righe `info` e poi si blocca lascia il file di log vuoto —
 *     l'ho verificato al primo avvio dell'app, e il file vuoto è precisamente il
 *     guasto silenzioso che questo sink esiste per eliminare;
 *   - la rotazione guarda la dimensione, non la data: un ciclo di errori può
 *     riempire un disco in minuti, e un log giornaliero non lo fermerebbe;
 *   - ogni errore di I/O viene ingoiato. Se il disco è pieno, il log è la cosa
 *     meno importante che si sta perdendo.
 */

import { appendFileSync, existsSync, renameSync, statSync, unlinkSync } from 'node:fs'
import { join } from 'node:path'
import { formatLogLine, type LogRecord, type LogSink } from '../../logger'

export interface FileSinkOptions {
  readonly directory: string
  /** Dimensione oltre la quale il file ruota. Default 2 MB. */
  readonly maxBytes?: number
  /** Quanti file storici tenere. Default 3. */
  readonly keep?: number
  /** Righe accumulate prima di scrivere. Default 24. */
  readonly batchSize?: number
  /** Scadenza del lotto: oltre questa, si scrive comunque. Default 1500ms. */
  readonly flushIntervalMs?: number
}

export function createFileSink(options: FileSinkOptions): LogSink {
  const maxBytes = options.maxBytes ?? 2 * 1024 * 1024
  const keep = options.keep ?? 3
  const batchSize = options.batchSize ?? 24
  const flushIntervalMs = options.flushIntervalMs ?? 1500
  const current = join(options.directory, 'aether.log')

  let pending: string[] = []
  let timer: ReturnType<typeof setTimeout> | null = null

  const cancelTimer = (): void => {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  const flush = (): void => {
    cancelTimer()
    if (pending.length === 0) return
    const payload = `${pending.join('\n')}\n`
    pending = []
    try {
      rotateIfNeeded()
      appendFileSync(current, payload, 'utf8')
    } catch {
      // Il disco può essere pieno o il file bloccato da un antivirus: in nessuno
      // dei due casi il logging deve propagare un errore a chi stava lavorando.
    }
  }

  const rotateIfNeeded = (): void => {
    try {
      if (!existsSync(current) || statSync(current).size < maxBytes) return
      // Il più vecchio si butta, gli altri scalano di uno.
      const oldest = join(options.directory, `aether.${keep}.log`)
      if (existsSync(oldest)) unlinkSync(oldest)
      for (let index = keep - 1; index >= 1; index--) {
        const from = join(options.directory, `aether.${index}.log`)
        if (existsSync(from)) renameSync(from, join(options.directory, `aether.${index + 1}.log`))
      }
      renameSync(current, join(options.directory, 'aether.1.log'))
    } catch {
      // Una rotazione fallita significa un file che cresce, non un'app che cade.
    }
  }

  return {
    write(record: LogRecord) {
      pending.push(formatLogLine(record))

      // Un errore non aspetta il lotto: se l'app sta cadendo, quella riga deve
      // essere già su disco. È la differenza fra avere e non avere la causa.
      if (pending.length >= batchSize || record.level === 'error' || record.level === 'fatal') {
        flush()
        return
      }

      if (timer === null) {
        timer = setTimeout(flush, flushIntervalMs)
        // Un lotto in attesa non deve tenere in vita il processo alla chiusura:
        // il flush di `will-quit` lo svuota comunque.
        ;(timer as { unref?: () => void }).unref?.()
      }
    },
    flush
  }
}
