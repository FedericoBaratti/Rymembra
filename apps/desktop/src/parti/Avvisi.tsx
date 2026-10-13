/**
 * I tre livelli con cui questo flusso dice che qualcosa non è andato liscio.
 *
 * # Perché tre, e perché prima non erano un sistema
 *
 * Prima c'erano undici `.avviso-monco` ambra sparsi in quattro file — sei in
 * `Account.tsx`, due in `ImportaLink`, due in `ImportaPlaylist`, uno
 * nell'elenco — più un `.errore` rosso e qualche `<details>` grigio.
 * `.avviso-monco` non compariva da nessun'altra parte nell'applicazione: era
 * un'invenzione di questo flusso, cresciuta undici volte senza mai diventare una
 * regola. Undici riquadri dello stesso colore per undici cose diverse vogliono
 * dire che il colore non dice niente.
 *
 * # Il livello si legge, non si decide
 *
 * `ErroreIpc` porta già `severity` e `retryable`, e il catalogo del nucleo li usa
 * con precisione: `download.noResults` è **info** e non ritentabile perché
 * cercato-e-non-c'è non è un guasto; un 403 è **warning** e ritentabile perché
 * passa da sé; un binario che manca è **error** e non ritentabile perché finché
 * non lo si installa non succede niente. Mappare quei due campi su tre superfici
 * è tutto quel che serve — e tenere qui una lista nostra di «cosa è grave»
 * sarebbe una regola di dominio dentro la finestra, cioè una regola che
 * divergerebbe dal catalogo al primo codice nuovo.
 *
 * # Cosa distingue i tre
 *
 * Non la gravità in astratto, ma **cosa può fare chi legge**:
 *
 * 1. `nota` — guarda che è andata così. Non c'è un gesto, e non se ne finge uno.
 * 2. `avviso` — un gesto la ripara, e il gesto sta dentro il riquadro.
 * 3. `blocco` — si ripara fuori di qui: un percorso, un indirizzo, una
 *    condizione. Il tasto primario resta spento, perché riprovare non serve.
 *
 * `esito` è il quarto e non è un livello d'errore: compare una volta sola in
 * tutto il flusso, per il viaggio di ritorno.
 *
 * # E uno che non è di questo flusso
 *
 * In fondo c'è `AvvisoDatabase`, l'avviso globale di database danneggiato. Non
 * usa i tre livelli — non è il guasto di un gesto, è una condizione della
 * sessione — ma è un avviso, e questo è il file degli avvisi.
 */
import { useEffect, useState, type ReactNode } from "react";

import {
  dettaglioErrore,
  eErroreIpc,
  eRitentabile,
  ipc,
  testoErrore,
  type SaluteDatabase,
} from "../ipc";
import { Icona, type NomeIcona } from "./Icone";
import { t } from "../lingue";
import { useAscolto } from "../pagine";

export type Livello = "nota" | "avviso" | "blocco" | "esito";

/**
 * Il livello di un errore del nucleo.
 *
 * `error` e `fatal` cadono su `blocco`, e con loro il `warning` che riprovando
 * non passa — offrire «Riprova» lì vorrebbe dire mandare qualcuno a premere un
 * tasto che non fa niente. Quel che non è un errore del nucleo è un `blocco`
 * per prudenza: di un guasto che non sappiamo leggere non possiamo promettere
 * che riprovare basti.
 */
function livelloDi(errore: unknown): Livello {
  if (!eErroreIpc(errore)) return "blocco";
  switch (errore.severity) {
    case "info":
      return "nota";
    case "warning":
      return errore.retryable ? "avviso" : "blocco";
    default:
      return "blocco";
  }
}

/**
 * Il simbolo di ognuno.
 *
 * `blocco` e `avviso` condividono `i-alert`: nello sprite non c'è un secondo
 * segnale, e aggiungerne uno è una richiesta al registro dei simboli — si chiede,
 * non si fa di passaggio. Il colore li distingue, ed è quel che li distingueva
 * comunque.
 */
const ICONE: Readonly<Record<Livello, NomeIcona>> = {
  nota: "i-mark",
  avviso: "i-alert",
  blocco: "i-alert",
  esito: "i-check",
};

/**
 * Il riquadro.
 *
 * La `nota` non ha riquadro né colore: è testo secondario, e incorniciarla
 * vorrebbe dire dare a «cercato, non c'è» lo stesso peso di «la playlist di
 * destinazione è automatica».
 *
 * `azione` sta **dentro**: la frase e il gesto che la risolve non si separano
 * mai, perché un avviso in cima e il suo tasto in fondo alla finestrella sono
 * due cose che chi legge deve ricucire da sé.
 */
export function Avviso({
  livello,
  children,
  azione,
}: {
  livello: Livello;
  children: ReactNode;
  azione?: ReactNode | undefined;
}) {
  // Un `div` e non un `p`, benché `.nota` nasca su un paragrafo: dentro una nota
  // ci finisce un `<details>` — i nomi dei file illeggibili oltre i tre — e un
  // `<details>` dentro un `<p>` non è HTML valido. Il browser chiuderebbe il
  // paragrafo da sé, e il resto della nota si ritroverebbe fuori dal riquadro
  // senza che nessuno abbia scritto niente di sbagliato. `.nota` non guarda il
  // nome dell'elemento, e in giro per l'albero sta già su tutti e due.
  if (livello === "nota") {
    return (
      <div className="nota" data-livello="nota">
        {children}
        {azione}
      </div>
    );
  }
  return (
    <div
      className="avviso"
      data-livello={livello}
      // `alert` interrompe chi ascolta, `status` aspetta una pausa. Un blocco
      // ferma il gesto in corso e va sentito adesso; un avviso no.
      role={livello === "blocco" ? "alert" : "status"}
    >
      <Icona nome={ICONE[livello]} dim={15} />
      <div className="dentro">
        <p>{children}</p>
        {azione !== undefined && <div className="azioni">{azione}</div>}
      </div>
    </div>
  );
}

/**
 * Un errore del nucleo, disegnato al suo livello.
 *
 * Il testo resta quello del catalogo — le stringhe sono già scritte bene, e la
 * regola della casa è che ogni voce dica *cosa fare*. Quel che mancava era il
 * livello di disegno che distingue le tre cose.
 *
 * «Riprova» compare **solo** se il catalogo dice che riprovare ha senso: è la
 * ragione per cui questo tasto non va scritto a mano accanto a ogni errore. Chi
 * copia la regola a mano la copia sbagliata alla terza volta.
 */
export function AvvisoErrore({
  errore,
  onRiprova,
}: {
  errore: unknown;
  onRiprova?: (() => void) | undefined;
}) {
  const ritentabile = eRitentabile(errore);
  // Quel che il servizio ha scritto, quando ha scritto qualcosa. Vedi
  // `dettaglioErrore`: la frase del catalogo dice cosa è successo, questa dice
  // cosa fare, e per mesi è stata la seconda a non arrivare mai.
  const dettaglio = dettaglioErrore(errore);
  return (
    <Avviso
      livello={livelloDi(errore)}
      azione={
        ritentabile && onRiprova !== undefined ? (
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={onRiprova}
          >
            {t("common.retry")}
          </button>
        ) : undefined
      }
    >
      {testoErrore(errore)}
      {dettaglio !== null && <span className="dettaglio-errore">{dettaglio}</span>}
    </Avviso>
  );
}

/**
 * Il database della libreria è danneggiato: lo si dice in cima a ogni pagina,
 * e finché la sessione dura.
 *
 * # Perché un avviso suo, e non l'errore della fascia
 *
 * Il controllo lo fa il nucleo da solo, un minuto dopo l'avvio
 * (`manutenzione.rs`), e fino a qui l'esito si leggeva soltanto aprendo
 * Impostazioni › Libreria e dati: chi non ci andava continuava a votare, a
 * mettere cuori e a costruire playlist dentro un file rotto, cioè proprio le
 * cose che un file rotto può perdere. La fascia d'errore non andava bene: si
 * chiude con una X, e la prossima riga rossa la sostituisce. Questo invece non
 * si chiude e non si sostituisce — il danno non passa chiudendo un avviso, e
 * un avviso che sparisce mentre il danno resta direbbe il contrario del vero.
 *
 * # Perché chiede anche all'apertura
 *
 * L'evento `manutenzione:salute` passa una volta per sessione. Una finestra
 * ricaricata dopo quel minuto non lo riceve più, e senza una domanda sua
 * mostrerebbe una libreria sana sopra un database che il nucleo sa rotto. La
 * domanda (`manutenzione_salute`) costa la lettura di un `Mutex`, e una
 * risposta che arriva in ritardo può soltanto accendere l'avviso, mai spegnerlo
 * sopra un evento più recente.
 *
 * # Cosa dice di fare
 *
 * Le due strade di `db/manutenzione.rs`: il profilo, che mette al sicuro voti,
 * preferiti e playlist in un file a parte, e la copia `aether.db.schema-NNN.bak`
 * che il nucleo scrive prima di ogni migrazione. Prima il profilo: è l'unica
 * delle due che non perde quel che è cambiato dopo l'ultima migrazione. Il
 * tasto porta dove stanno tutte e due le cose — il percorso della cartella dati
 * e l'esportazione del profilo — invece di provare a fare da qui un gesto che
 * ha bisogno di un dialogo di sistema e di una decisione.
 */
export function AvvisoDatabase({ onVaiAiDati }: { onVaiAiDati: () => void }) {
  const [danneggiato, setDanneggiato] = useState(false);

  useEffect(() => {
    let vivo = true;
    ipc
      .manutenzioneSalute()
      .then((salute) => {
        if (vivo && salute.esito === "danneggiato") setDanneggiato(true);
      })
      // Un guasto qui non si mostra: sarebbe un avviso rosso per dire che non
      // si è riusciti a sapere se mostrarne uno.
      .catch(() => undefined);
    return () => {
      vivo = false;
    };
  }, []);

  useAscolto<SaluteDatabase>("manutenzione:salute", (salute) =>
    setDanneggiato(salute.esito === "danneggiato"),
  );

  if (!danneggiato) return null;
  const dove = `${t("nav.settings")} › ${t("settings.section.dati")}`;
  return (
    <div className="errore toast-card" role="alert" data-avviso="database">
      <Icona nome="i-alert" dim={16} />
      <span>
        <strong>{t("db.damaged.what")}</strong> {t("db.damaged.how", { dove })}
      </span>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        onClick={onVaiAiDati}
      >
        {t("db.damaged.go", { dove: t("settings.section.dati") })}
      </button>
    </div>
  );
}
