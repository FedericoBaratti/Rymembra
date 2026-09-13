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
 * pixel di lato (`MAX_LATO_MINIATURA` in `core/aether-app/src/covers.rs`). Fino
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
 */
import { useState } from "react";

import { urlCopertina } from "./aspetto";

export function Copertina({
  hash,
  titolo,
  classe = "copertina",
  piena = false,
}: {
  hash: string | null;
  titolo: string;
  classe?: string;
  /** L'originale invece della miniatura. Vedi la nota del modulo. */
  piena?: boolean;
}) {
  /**
   * L'indirizzo che è arrivato, non il fatto che qualcosa sia arrivato.
   *
   * `pronta` si calcola confrontandolo con quello che si sta chiedendo: al cambio
   * di `url` torna falsa senza che nessuno la azzeri.
   */
  const [caricato, setCaricato] = useState<string | null>(null);
  const url = urlCopertina(hash, !piena);
  const pronta = caricato !== null && caricato === url;
  if (!url) {
    return (
      <div className={`${classe} vuota`} aria-hidden="true">
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
      src={url}
      alt={titolo}
      /* `lazy`: una griglia di novecento album non deve chiedere novecento
         immagini all'apertura, ma quelle che entrano nello schermo. */
      loading="lazy"
      decoding="async"
      draggable={false}
      /* La dissolvenza sta nel CSS; qui c'è solo il fatto. `onError` la accende
         lo stesso: un'immagine che non arriva deve mostrare il suo fondo, non
         restare un buco invisibile per sempre. */
      onLoad={() => setCaricato(url)}
      onError={() => setCaricato(url)}
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
  const url = urlCopertina(hash);
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
