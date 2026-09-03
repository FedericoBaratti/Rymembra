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
 */
import type { ReactNode } from "react";

import { eErroreIpc, eRitentabile, testoErrore } from "../ipc";
import { Icona, type NomeIcona } from "./Icone";
import { t } from "../lingue";

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
export function livelloDi(errore: unknown): Livello {
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
    </Avviso>
  );
}
