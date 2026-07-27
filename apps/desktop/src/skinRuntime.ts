/**
 * Applicare una skin, a runtime.
 *
 * Come funzionava prima: `applySkin()` in `src/lib/skins.ts` metteva un attributo
 * `data-skin` su `<html>`, e il CSS di tutte le skin era già nel bundle — tre
 * fogli caricati sempre, uno dei quali attivo. Aggiungerne una richiedeva di
 * toccare quattro file in due progetti.
 *
 * Come funziona ora: il CSS arriva compilato dal backend e si adotta come foglio
 * costruibile. Tre conseguenze concrete.
 *
 *   1. Sostituire il foglio è **istantaneo** e non ricarica niente: è ciò che
 *      rende possibile l'anteprima dal vivo dello Studio, dove ogni modifica
 *      ricompila e sostituisce a ~16ms.
 *   2. Un foglio adottato non tocca il DOM: nessun `<style>` che si accumula,
 *      nessun ordine di cascata da indovinare.
 *   3. Le skin non installate non costano niente. Prima erano tutte nel bundle.
 */

const SHEET_MARKER = 'aether-skin'

/** `adoptedStyleSheets` c'è dappertutto dove ci interessa, ma va comunque guardato. */
function supportsAdoptedSheets(): boolean {
  return (
    typeof CSSStyleSheet !== 'undefined' &&
    'replaceSync' in CSSStyleSheet.prototype &&
    'adoptedStyleSheets' in Document.prototype
  )
}

let sheet: CSSStyleSheet | null = null
let fallback: HTMLStyleElement | null = null

/**
 * Sostituisce il foglio della skin attiva.
 *
 * Sincrona di proposito: `replaceSync` applica lo stile prima del prossimo
 * fotogramma, quindi non esiste un istante in cui la finestra è dipinta a metà con
 * i colori vecchi e a metà con i nuovi. La variante asincrona (`replace`)
 * produrrebbe quel lampeggio.
 */
export function applySkinCss(css: string): void {
  if (supportsAdoptedSheets()) {
    if (sheet === null) {
      sheet = new CSSStyleSheet()
      document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet]
    }
    sheet.replaceSync(css)
    return
  }

  // Ripiego per un motore di rendering che non li supporta: un solo elemento
  // riusato, non uno per applicazione — altrimenti si accumulerebbero e l'ultimo
  // vincerebbe per ordine invece che per intenzione.
  if (fallback === null) {
    fallback = document.createElement('style')
    fallback.dataset['marker'] = SHEET_MARKER
    document.head.appendChild(fallback)
  }
  fallback.textContent = css
}

/**
 * Segna quale skin e quale tema sono attivi.
 *
 * Gli attributi vanno su `<html>` perché i selettori compilati sono
 * `:root[data-skin='x']`, e restano l'unico aggancio fra il documento e il foglio.
 */
export function markActiveSkin(id: string, theme: 'dark' | 'light'): void {
  document.documentElement.dataset['skin'] = id
  document.documentElement.dataset['theme'] = theme
}

/**
 * Il colore di fondo della finestra, per l'avvio a freddo successivo.
 *
 * È il bug che il piano segnala: `MainActivity.java:74` e `capacitor.config.ts:21`
 * cablano `#09090d`, che è il `surface-0` della skin *plain*. Con un'altra skin
 * l'avvio a freddo e l'overscroll lampeggiano del colore sbagliato. Leggerlo dallo
 * stile calcolato dopo l'applicazione è il modo di non doverlo mai duplicare.
 */
export function activeSurfaceColor(): string {
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue('--color-surface-0')
    .trim()
  return value.length > 0 ? value : '#09090d'
}
