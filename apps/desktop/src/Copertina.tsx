/**
 * Una copertina, o il posto dove sarebbe.
 *
 * Sta in un modulo suo perché la usano sei posti — la griglia, l'elenco, il
 * lettore, la colonna, lo schermo intero e il mosaico degli artisti — e perché
 * il fallback conta: metà della libreria vera non ha un'immagine, e un `<img>`
 * rotto è più brutto di un quadrato con una nota.
 *
 * # Miniatura o piena
 *
 * Il nucleo scrive due file per copertina: l'originale e una miniatura da 160
 * pixel di lato (`MAX_LATO_MINIATURA` in `core/rymembra-app/src/covers.rs`). Fino
 * a qui l'interfaccia chiedeva **sempre** la miniatura, anche per la copertina
 * di «In riproduzione», che è larga trecentosessanta e su uno schermo a 150%
 * sono cinquecentoquaranta pixel veri: un JPEG da 160 ingrandito tre volte e
 * mezzo, cioè la ragione singola per cui l'app sembrava meno curata di quanto
 * fosse.
 *
 * `piena` è la scelta, e va fatta a mano perché la risposta giusta dipende da
 * quante immagini ci sono insieme: tre copertine grandi sono tre richieste,
 * novecento miniature in una griglia sono novecento. La regola è «piena dove la
 * copertina **è** il soggetto» — schermo intero, terza colonna, testata di un
 * album — e miniatura dove è un'etichetta.
 *
 * # La scheda: una terza misura, scelta dal motore
 *
 * Le schede delle griglie stanno in mezzo: troppo grandi per la miniatura (una
 * scheda è larga dai 164 pixel, e a 200% sono più del doppio dei 160 della
 * miniatura), troppe per la piena. `misura="scheda"` dà al motore le tre misure
 * in `srcset` e la larghezza della scheda in `sizes`, e la scelta la fa lui con
 * la densità vera dello schermo: la media da 320 a 100%, la piena a 200%. I
 * conti stanno in `copertine.ts`. Nell'anteprima dello Studio no: là le
 * copertine sono disegni finti, e di misure ne hanno una sola.
 *
 * # Una copertina che non arriva
 *
 * Mostra il quadrato con la nota, come una copertina che non c'è. Un `<img>`
 * il cui file manca mostrava l'icona dell'immagine rotta con il titolo
 * dell'album accanto — si vedeva nella terza colonna — cioè un guasto dove c'era
 * soltanto un disco senza immagine.
 *
 * # Perché la dissolvenza è uno stato e non un attributo
 *
 * Perché l'attributo non si cancellava. `data-pronta` lo scriveva `onLoad` con
 * un `setAttribute`, cioè a mano, fuori dal modello di React: il nodo restava lo
 * stesso quando cambiava `src` — ed è quel che succede a **ogni cambio di brano**
 * nella copertina del lettore e in quella della terza colonna — e un attributo
 * scritto a mano su un nodo riusato nessuno lo togliva.
 *
 * Il danno non era la dissolvenza che funziona una volta sola. Era che un `<img>`
 * riusato **continua a mostrare l'immagine di prima** finché la nuova non è
 * decodificata, e con `data-pronta` ancora addosso la mostrava a piena opacità:
 * per qualche decina di millisecondi il lettore diceva il brano nuovo con la
 * copertina del precedente, che è un errore — non una transizione.
 *
 * Due cose lo chiudono, e servono entrambe. `key={url}` perché l'elemento sia
 * nuovo quando l'indirizzo è nuovo: un `<img>` appena nato non ha niente da
 * mostrare, quindi la copertina vecchia sparisce nell'istante in cui il brano
 * cambia. E lo stato che ricorda **quale** indirizzo è arrivato, invece di un
 * booleano: così «pronta» si spegne da sé al cambio di `url`, senza un effetto
 * che lo azzeri e senza il fotogramma in cui l'immagine nuova si vede opaca
 * prima che l'azzeramento arrivi.
 *
 * # Quando l'`alt` tace
 *
 * Quasi sempre, una copertina sta **accanto** al suo titolo: la riga
 * dell'elenco, la scheda della griglia, il lettore, lo schermo intero. Lì un
 * `alt` col nome dell'album fa leggere il nome due volte — una per l'immagine,
 * una per il testo — e dentro un bottone lo raddoppia nel suo nome: «Kind of
 * Blue Kind of Blue Miles Davis». Chi la disegna accanto al titolo passa
 * `accanto`, e l'immagine diventa decorazione (`alt=""`), che è quel che è.
 * Dove la copertina sta da sola, l'`alt` resta il titolo.
 */
import { useState } from "react";

import { indirizzoCopertina, sizesPer, srcsetCopertina } from "./copertine";
import { useDati } from "./dati";
import { useCambio, type AttributiSuono } from "./segnali";

export function Copertina({
  hash,
  titolo,
  classe = "copertina",
  piena = false,
  misura,
  larghezza,
  suono,
  cambio = false,
  accanto = false,
}: {
  hash: string | null;
  titolo: string;
  classe?: string;
  /** L'originale invece della miniatura. Vedi la nota del modulo. */
  piena?: boolean;
  /**
   * `"scheda"` per le griglie: le tre misure in `srcset`, e la scelta al
   * motore. Senza, vale `piena`. Vedi «La scheda» nella nota del modulo.
   */
  misura?: "scheda" | undefined;
  /**
   * Quanto è larga la copertina sullo schermo, in pixel CSS: diventa `sizes`.
   * Solo con `misura="scheda"`; chi disegna una griglia la sa, perché ha
   * misurato le colonne.
   */
  larghezza?: number | null | undefined;
  /**
   * In riproduzione o in pausa, per chi ritrae quel che suona: la skin ci
   * aggancia il vinile che gira. Vedi `segnali.ts`.
   */
  suono?: AttributiSuono;
  /**
   * `data-cambio` a ogni copertina nuova. Solo per chi mostra quel che suona:
   * in una griglia la copertina cambia perché la riga è stata riciclata, e
   * quello non è un cambio da festeggiare.
   */
  cambio?: boolean;
  /**
   * Il titolo è già scritto accanto: l'immagine è decorazione, e `alt` resta
   * vuoto. Vedi «Quando l'`alt` tace» nella nota del modulo.
   */
  accanto?: boolean;
}) {
  /**
   * L'indirizzo che è arrivato — o che è fallito — non il fatto che qualcosa
   * sia arrivato.
   *
   * `pronta` e `fallita` si calcolano confrontandolo con quello che si sta
   * chiedendo: al cambio di `url` tornano false senza che nessuno le azzeri. Uno
   * stato solo per le due cose, perché ogni riga dell'elenco porta una
   * copertina, e uno stato in più per riga è quel che le liste lunghe non si
   * possono permettere.
   */
  const [esito, setEsito] = useState<{ url: string; riuscita: boolean } | null>(null);
  const dati = useDati();
  const conSrcset = misura === "scheda" && !dati.anteprima && hash !== null;
  const url = conSrcset
    ? indirizzoCopertina(hash, "media")
    : dati.copertina(hash, !piena);
  const pronta = esito !== null && esito.url === url;
  const fallita = pronta && !esito.riuscita;
  // Si chiede sempre, anche quando non serve: un hook non si salta.
  const lato = useCambio(hash);
  const segnali = { ...suono, "data-cambio": cambio ? lato : undefined };
  if (!url || fallita) {
    return (
      <div className={`${classe} vuota`} aria-hidden="true" {...segnali}>
        ♪
      </div>
    );
  }
  return (
    <img
      /* L'elemento è nuovo quando l'indirizzo è nuovo: vedi la nota del modulo.
         La chiave sta qui e non sul chiamante perché è questo nodo che deve
         nascere di nuovo, e i chiamanti hanno già le loro chiavi di lista. */
      key={url}
      /* `foto` accanto alla classe di chi la usa, e non al posto: serve alla
         sola regola della dissolvenza, che altrimenti andrebbe ripetuta per
         ognuno dei sei nomi che passano di qui. */
      className={`${classe} foto`}
      data-pronta={pronta ? "" : undefined}
      {...segnali}
      src={url}
      /* Le tre misure e la larghezza della scheda: sceglie il motore, che
         conosce la densità vera. Vedi «La scheda» nella nota del modulo. */
      srcSet={conSrcset ? srcsetCopertina(hash) : undefined}
      sizes={conSrcset ? sizesPer(larghezza) : undefined}
      alt={accanto ? "" : titolo}
      /* `lazy`: una griglia di novecento album non deve chiedere novecento
         immagini all'apertura, ma quelle che entrano nello schermo. */
      loading="lazy"
      decoding="async"
      draggable={false}
      /* La dissolvenza sta nel CSS; qui c'è solo il fatto. Un errore non è una
         copertina pronta: è il segnaposto, vedi «Una copertina che non arriva». */
      onLoad={() => setEsito({ url, riuscita: true })}
      onError={() => setEsito({ url, riuscita: false })}
    />
  );
}

/**
 * La stessa copertina, sfocata fino a non essere più un'immagine.
 *
 * È l'ambiente dietro la terza colonna e dietro lo schermo intero, ed è il
 * posto in cui questa interfaccia si concede l'unico colore imprevedibile che
 * ha. Non è una tinta *estratta*: è l'immagine, e il motivo per cui è meglio è
 * che una copertina non ha un colore solo — un disco rosso con la fascia gialla
 * dà un ambiente rosso **e** giallo, dalla parte giusta.
 *
 * Sempre la miniatura, anche dove la copertina accanto è piena: sotto una
 * sfocatura da sessanta pixel i dettagli di un originale da mille sono byte
 * chiesti al disco per essere buttati.
 *
 * Sopra ci resta `np-scrim`, che è neutro. Questo strato non tocca mai il
 * testo, ed è la stessa regola che valeva per `--hero-rgb` — che infatti resta,
 * sotto, come fondo di quando la copertina non c'è.
 */
export function Sfocata({
  hash,
  classe,
}: {
  hash: string | null;
  classe: string;
}) {
  const url = useDati().copertina(hash);
  if (!url) return null;
  return (
    <img
      className={classe}
      src={url}
      alt=""
      aria-hidden="true"
      decoding="async"
      draggable={false}
    />
  );
}
