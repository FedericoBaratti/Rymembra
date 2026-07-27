/**
 * Percorsi della libreria: cosa è un brano, e quando due percorsi sono lo stesso.
 *
 * Sembra utilità da poco. È il posto dove il vecchio albero ha avuto i suoi due
 * difetti più cari, entrambi con lo stesso esito — righe cancellate per file che
 * esistevano ancora — ed entrambi invisibili finché non capitano al percorso
 * giusto:
 *
 *   - `startsWith` per dire «sta dentro la cartella sorvegliata»: `C:\Musica`
 *     comincia con `C:\Music`, quindi una scansione di `C:\Music` cancellava le
 *     righe della cartella accanto;
 *   - confronto esatto fra il percorso salvato nel database e quello prodotto
 *     dalla camminata: bastano una barra rovesciata contro una dritta, o una
 *     maiuscola, e il file «non risulta più trovato».
 *
 * Da qui la separazione fra le due nozioni di percorso. Quello **per il disco**
 * si usa così com'è, sempre: normalizzarlo per leggerlo sarebbe un altro modo di
 * sbagliare file. Quello **per l'appartenenza a un insieme** è una forma
 * canonica che serve solo a rispondere «è lo stesso percorso?», e non deve mai
 * finire in una `open()`.
 */

/** Le estensioni che consideriamo musica. Elenco chiuso, come nel vecchio albero. */
export const SUPPORTED_EXTENSIONS: readonly string[] = [
  'mp3',
  'flac',
  'm4a',
  'aac',
  'ogg',
  'wav',
  'aiff',
  'aif',
  'opus',
  'wma'
]

/**
 * La cartella dei doppioni scartati, dentro quella dei download.
 *
 * Esclusa dalla scansione e dalla sorveglianza: senza, un file spostato lì dalla
 * deduplicazione rientrerebbe in libreria alla scansione successiva, e l'utente
 * lo vedrebbe tornare da solo dopo averlo tolto.
 */
export const TRASH_DIR_NAME = '.trash'

/**
 * Sotto questa soglia non è un brano.
 *
 * Sono avanzi troncati — una conversione ffmpeg uccisa, un download interrotto —
 * e il punto è che i lettori di metadati spesso ci riescono lo stesso: senza
 * questa guardia entrano in libreria come tracce apparentemente normali, e il
 * guasto si scopre premendo play.
 */
export const MIN_TRACK_BYTES = 32 * 1024

/**
 * Le regole di confronto fra percorsi, dichiarate da chi chiama.
 *
 * `caseInsensitive` non ha un valore predefinito, ed è deliberato: sbagliarlo
 * perde dati in entrambe le direzioni, quindi non deve poter essere dimenticato.
 * A `false` su un filesystem che ignora le maiuscole, `C:\Music\a.mp3` nel
 * database e `C:\music\A.MP3` dalla camminata sono due cose diverse: la riga
 * viene cancellata e il file reinserito. A `true` su un filesystem che le
 * distingue, `a.mp3` e `A.mp3` sono due brani veri che collassano in uno.
 */
export interface PathRules {
  readonly caseInsensitive: boolean
}

/** L'ultimo segmento di un percorso, con qualunque separatore. */
export function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return cut < 0 ? path : path.slice(cut + 1)
}

/**
 * L'estensione in minuscolo, senza punto. Stringa vuota se non ce n'è.
 *
 * Si guarda solo l'ultimo segmento: una cartella chiamata `Album.2019` non deve
 * dare estensione `2019/traccia` a ciò che contiene. E un nome che comincia con
 * un punto e non ne ha altri (`.trashinfo`) non ha estensione, ha solo un nome.
 */
export function extensionOf(path: string): string {
  const name = baseName(path)
  const dot = name.lastIndexOf('.')
  if (dot <= 0) return ''
  return name.slice(dot + 1).toLowerCase()
}

export function isSupportedAudioPath(path: string): boolean {
  return SUPPORTED_EXTENSIONS.indexOf(extensionOf(path)) >= 0
}

/** I segmenti non vuoti di un percorso, con qualunque separatore. */
function segments(path: string): string[] {
  return path.split(/[\\/]+/).filter((part) => part.length > 0)
}

/** Il percorso attraversa la cartella dei doppioni scartati? */
export function isInTrash(path: string): boolean {
  return segments(path).indexOf(TRASH_DIR_NAME) >= 0
}

/**
 * La forma canonica per l'APPARTENENZA A UN INSIEME. Mai per leggere un file.
 *
 * Unifica i separatori e, dove il filesystem non distingue le maiuscole, piega
 * il caso. Serve a far combaciare il percorso salvato nel database con quello
 * appena prodotto dalla camminata: un confronto esatto fra i due li vedrebbe
 * diversi per una barra, e la scansione cancellerebbe la riga di un file che sta
 * ancora lì.
 */
export function pathKey(path: string, rules: PathRules): string {
  // Barre unificate e code di separatori tolte: `C:\Music\` e `C:/Music` sono lo
  // stesso posto.
  const unified = path.replace(/\\/g, '/').replace(/\/+$/, '')
  return rules.caseInsensitive ? unified.toLowerCase() : unified
}

/**
 * `path` è la cartella stessa, o qualcosa strettamente dentro?
 *
 * Il confine di separatore è tutto il punto: senza, `C:\Musica` risulterebbe
 * dentro `C:\Music` — sono due cartelle diverse, e nel vecchio albero questo
 * bastava a far cancellare le righe della seconda quando si scansionava la
 * prima.
 */
export function isUnder(path: string, folder: string, rules: PathRules): boolean {
  const child = pathKey(path, rules)
  const parent = pathKey(folder, rules)
  if (parent.length === 0) return false
  return child === parent || child.indexOf(`${parent}/`) === 0
}
