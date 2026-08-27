/**
 * Come si ottiene l'archivio di Spotify, e perché è l'unica strada.
 *
 * # C'erano due strade
 *
 * Fino a poco fa questo file si chiamava `DueStrade.tsx` e disegnava una
 * tabella di confronto: l'archivio ZIP da una parte, il collegamento OAuth alla
 * Web API dall'altra. Il collegamento non c'è più, e la ragione non è tecnica —
 * funzionava.
 *
 * È che cosa si possa fare dei dati che la Web API restituisce lo decide lo
 * *Spotify Developer Policy*, e una libreria musicale che tiene per anni le
 * playlist e la cronologia di qualcuno non sta dentro quei limiti. L'archivio,
 * invece, Spotify lo consegna **all'utente**: è un suo diritto, non una
 * concessione — GDPR art. 20, portabilità dei dati — e portarselo dove vuole è
 * esattamente ciò che quell'articolo gli riconosce.
 *
 * Il confronto, per la cronaca, l'archivio lo vinceva su tre righe su cinque:
 * porta **anni** di cronologia contro gli ultimi cinquanta ascolti, non chiede
 * né client id né Premium né consenso, ed era l'unico dei due provato su un
 * account vero per intero. Perdeva sull'ISRC e sulla ripetibilità.
 *
 * # Perché tre passi numerati
 *
 * Perché sono tre cose da fare **in ordine**, su un sito che non è questo, e un
 * paragrafo che le contiene tutte e tre è un paragrafo che si rilegge da capo a
 * ogni ritorno dal browser. Numerarle costa niente e le rende ritrovabili al
 * punto in cui si era.
 *
 * # E la cosa più utile di tutte
 *
 * L'archivio arriva in giorni. Chiederlo **adesso** e tornare quando è arrivato
 * vale più di qualunque spiegazione, ed è il motivo per cui quella frase sta in
 * fondo, dove si guarda prima di chiudere.
 */
import { Icona } from "./Icone";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";

/** Un passo per ottenere l'archivio: cosa fare, e la cosa da non sbagliare. */
interface Passo {
  cosa: React.ReactNode;
  /** L'unico dettaglio che, sbagliato, fa arrivare la cosa sbagliata. */
  attenzione?: React.ReactNode;
}

/**
 * I tre passi.
 *
 * Una funzione e non una costante: i testi vengono dal catalogo delle lingue, e
 * una costante di modulo li fisserebbe sulla lingua del primo `import`.
 */
function passi(): readonly Passo[] {
  return [
    {
      cosa: (
        <Trans
          k="archive.step1"
          v={{
            sito: <span className="mono">spotify.com/account/privacy</span>,
          }}
        />
      ),
    },
    {
      cosa: (
        <Trans
          k="archive.step2"
          v={{
            dati: <strong>{t("archive.step2.account")}</strong>,
            cronologia: <strong>{t("archive.step2.history")}</strong>,
          }}
        />
      ),
      attenzione: t("archive.step2.warn"),
    },
    { cosa: t("archive.step3") },
  ];
}

export function ComeAvereLArchivio({
  onApriArchivio,
}: {
  onApriArchivio: () => void;
}) {
  return (
    <div className="archivio-spotify">
      <p className="nota">
        <Trans
          k="archive.intro"
          v={{ intero: <strong>{t("archive.intro.whole")}</strong> }}
        />
      </p>

      <ol className="passi">
        {passi().map((passo, i) => (
          // L'indice come chiave: sono tre voci costanti, scritte qui sopra e
          // mai riordinate. Inventare un identificativo per una lista che non
          // cambia mai sarebbe cerimonia.
          // eslint-disable-next-line react/no-array-index-key
          <li key={i}>
            {passo.cosa}
            {passo.attenzione && <small>{passo.attenzione}</small>}
          </li>
        ))}
      </ol>

      <div className="azioni">
        <button
          type="button"
          className="bottone primario"
          onClick={onApriArchivio}
        >
          <Icona nome="i-folder" dim={15} />
          {t("archive.open")}
        </button>
      </div>

      <p className="nota">
        <Trans
          k="archive.askNow"
          v={{ adesso: <strong>{t("archive.askNow.now")}</strong> }}
        />
      </p>
    </div>
  );
}
