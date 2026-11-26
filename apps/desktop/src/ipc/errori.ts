/** Parte del contratto con il nucleo: l'errore come arriva, e come si legge. La riesporta `ipc.ts`: si importa da lì. */
import { tSe } from "../lingue";
import { chiaveDelTestoErrore, chiaveSecondoIDatiMobili } from "../lontano";
import type { DatiMobili } from "./lontano";

/** Un errore, come lo manda il nucleo. */
export interface ErroreIpc {
  code: string;
  domain: string;
  severity: "info" | "warning" | "error" | "fatal";
  retryable: boolean;
  i18nKey: string;
  /**
   * Quel che ha scritto un servizio remoto (OpenRouter, OpenAI…), com'è. Mai
   * testo di Rymembra: il dettaglio di chi sviluppa resta nel diario e non
   * arriva qui (`errore.rs`).
   */
  serviceText: string | null;
  /** I dati che la frase del codice nomina (`{nome}`), sicuri da mostrare. */
  values: Record<string, string>;
  cause: string | null;
}

/** È un errore del nucleo, o qualcosa di inatteso? */
export function eErroreIpc(value: unknown): value is ErroreIpc {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof value.code === "string" &&
    "domain" in value &&
    typeof value.domain === "string" &&
    "severity" in value &&
    typeof value.severity === "string"
  );
}

/**
 * Riprovare ha senso?
 *
 * La regola sta qui e non accanto a ogni tasto «Riprova» perché a deciderlo è
 * il catalogo del nucleo — `is_retryable` in `errors/catalog.rs` — e chi la
 * ricopia a mano la ricopia sbagliata alla terza volta. Un `unknown` che non è
 * un errore del nucleo non è ritentabile: di lui non si sa niente, e offrire di
 * rifare un gesto di cui non si conosce l'esito è peggio che non offrirlo.
 */
export function eRitentabile(value: unknown): boolean {
  return eErroreIpc(value) && value.retryable;
}

/**
 * Il testo da mostrare per un errore qualsiasi.
 *
 * # La tabella che c'era qui, e perché non c'è più
 *
 * Stava scritta a mano — venticinque messaggi italiani chiavati per codice — e
 * il campo che li avrebbe resi inutili arrivava dal nucleo da sempre senza che
 * nessuno lo leggesse: `ErroreIpc.i18nKey`, generato in
 * `rymembra-domain/src/errors/catalog.rs` come `concat!("errors.", <codice>)`.
 * Erano le stesse due tabelle da tenere allineate che il catalogo del nucleo
 * esiste per non avere: venticinque voci qui contro centoquattro codici di là,
 * e ogni codice nuovo compariva a schermo come `spotify.notPublic` finché
 * qualcuno non si ricordava di `ipc.ts`.
 *
 * Adesso la chiave la decide il nucleo e il testo lo decide il catalogo delle
 * lingue, dove sono tutti e centoquattro. Un codice aggiunto di là senza testo
 * di qua ripiega su `message`, e in sviluppo lo dice in console.
 *
 * # Perché la chiave viene prima di `message`
 *
 * Perché il testo libero che il nucleo aggiunge — quello del servizio remoto — è
 * una riga in più, non la frase: quella la dice il catalogo, nella lingua di chi
 * legge. I dati che la frase nomina (`{ottenuto}`) arrivano in `values`.
 */
export function testoErrore(value: unknown): string {
  if (eErroreIpc(value)) {
    // La chiave è quella del codice, tranne dove la frase dipende da una scelta di
    // chi legge: «sei sui dati mobili» con «Chiedi» non è «Solo Wi-Fi» (S11-4).
    return tSe(chiaveDelTestoErrore(value.i18nKey), value.serviceText ?? value.cause ?? value.code, valoriDi(value));
  }
  return value instanceof Error ? value.message : String(value);
}

/** I dati dei segnaposto, o niente: un errore fabbricato a mano può non averli. */
function valoriDi(errore: ErroreIpc): Record<string, string> | undefined {
  return typeof errore.values === "object" && errore.values !== null ? errore.values : undefined;
}

/**
 * La riga in più: quel che il servizio ha scritto, con parole sue.
 *
 * # Perché `testoErrore` da sola non bastava
 *
 * Perché `tSe` usa il ripiego **solo quando la chiave manca**, e per un codice
 * del catalogo la chiave c'è sempre. Quindi il testo del servizio — l'unico campo
 * che porta qualcosa di specifico a questo guasto — non arrivava mai a schermo:
 * restava scritto in un record che l'interfaccia riceveva e non leggeva.
 *
 * È solo il testo di un **servizio remoto**: il nucleo non manda più una riga
 * sua sotto la frase (era in italiano anche con l'interfaccia in inglese, R10).
 *
 * Il caso che lo rendeva evidente è OpenRouter. La frase del catalogo dice
 * «questo fornitore non conosce quel modello», che è vero e non serve; il
 * servizio aveva scritto «use this slug instead: minimax/minimax-m3», che è la
 * correzione da copiare. Sono due frasi diverse e servono tutte e due — quella
 * generale spiega cosa è successo, questa dice cosa fare.
 *
 * `null` quando non c'è niente da aggiungere, o quando quel che ci sarebbe
 * ripete la frase del catalogo: due volte la stessa cosa in due riquadri
 * insegna a non leggere il secondo.
 */
export function dettaglioErrore(value: unknown): string | null {
  if (!eErroreIpc(value) || typeof value.serviceText !== "string") return null;
  const dettaglio = value.serviceText.trim();
  if (dettaglio === "" || dettaglio === testoErrore(value)) return null;
  return dettaglio;
}

/** Un guasto pronto da disegnare: la frase, la riga in più, e il codice. */
export interface Guasto {
  /** Quel che dice il catalogo, tradotto. */
  testo: string;
  /** Quel che ha scritto il servizio, quando ha scritto qualcosa. */
  dettaglio: string | null;
  /**
   * Il codice del catalogo, quando il guasto viene dal nucleo: è quel che
   * decide quale gesto la fascia può offrire accanto alla frase (vedi
   * `azioneDelGuasto` in `App.tsx`). Assente o `null` per un errore
   * qualunque, o per un guasto composto a mano.
   */
  codice?: string | null;
  /**
   * La chiave della frase e i dati che nomina, come li ha mandati il nucleo: da
   * qui [`guastoAdesso`] ricompone `testo` quando la frase dipende da una scelta
   * di chi legge e quella cambia a fascia aperta. Assenti per un guasto che non
   * viene dal nucleo.
   */
  chiave?: string | null;
  valori?: Record<string, string> | undefined;
}

/**
 * Le due frasi di un guasto, insieme.
 *
 * # Perché una coppia e non una stringa
 *
 * Perché la fascia dell'applicazione teneva `testoErrore(e)` in uno stato di
 * tipo `string`, e da lì in poi del guasto non restava altro: né il codice, né
 * il dettaglio. Era una scelta ragionevole finché il dettaglio non esisteva —
 * e quando ha cominciato a esistere, l'unico posto dove non poteva arrivare era
 * proprio quello che si vede più spesso.
 */
export function guastoDa(value: unknown): Guasto | null {
  if (value === null || value === undefined) return null;
  return {
    testo: testoErrore(value),
    dettaglio: dettaglioErrore(value),
    codice: eErroreIpc(value) ? value.code : null,
    chiave: eErroreIpc(value) ? value.i18nKey : null,
    valori: eErroreIpc(value) ? valoriDi(value) : undefined,
  };
}

/**
 * Lo stesso guasto con la frase che l'impostazione «Dati mobili» di **adesso**
 * sceglie (AVD-2).
 *
 * La frase di «sei sui dati mobili» è una per «Solo Wi-Fi» e un'altra per «Chiedi»
 * (`chiaveSecondoIDatiMobili`). `guastoDa` la sceglie quando il guasto nasce, e la
 * fascia resta aperta finché qualcuno non la chiude: chi sceglieva «Solo Wi-Fi»
 * dalla finestrella continuava a leggere «premi play e ti chiederà se usare i dati
 * mobili». Quel che non dipende dall'impostazione torna com'era, lo stesso oggetto.
 */
export function guastoAdesso(guasto: Guasto | null, datiMobili: DatiMobili | null): Guasto | null {
  if (guasto === null || guasto.chiave === undefined || guasto.chiave === null) return guasto;
  const chiave = chiaveSecondoIDatiMobili(guasto.chiave, datiMobili);
  const testo = tSe(chiave, guasto.testo, guasto.valori);
  return testo === guasto.testo ? guasto : { ...guasto, testo };
}
