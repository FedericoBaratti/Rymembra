/**
 * L'intestazione di una pagina: chi sono, e i due comandi che valgono ovunque.
 *
 * # Perché l'aria sta qui e non fra le righe
 *
 * Un elenco di brani si legge scorrendo, e scorrere è più facile quando le
 * righe sono fitte: quaranta pixel di riga fanno stare venti brani in uno
 * schermo, sessanta ne fanno stare tredici. Ma un elenco fitto senza niente
 * sopra è un muro. Quindi l'aria si spende tutta qui — ventiquattro pixel sopra
 * il titolo, quattordici sotto — e nessuna fra una riga e l'altra.
 *
 * # Perché la ricerca è nell'intestazione e non nella barra in alto
 *
 * La barra in alto non c'è più: era alta cinquantotto pixel su tutta la
 * larghezza e conteneva un marchio, un campo e a volte un menù a tendina. Il
 * marchio è andato in cima alla navigazione, dove c'è già il suo spazio; il
 * campo è qui, dove sta la cosa su cui cerca.
 */
import { Icona } from "./Icone";
import { t } from "../lingue";

export function Intestazione({
  occhiello,
  titolo,
  sottotitolo,
  copertina,
  query,
  onQuery,
  ordinamento,
  azioni,
}: {
  /** L'etichetta piccola sopra il titolo. */
  occhiello?: string | undefined;
  titolo: string;
  sottotitolo?: string | undefined;
  /**
   * L'immagine a sinistra del titolo, dove la pagina ne ha una.
   *
   * Ce l'ha solo l'album aperto. Non è un banner: novantasei pixel accanto al
   * titolo, non trecento sopra — la pagina di un album è l'elenco dei suoi
   * brani, e un'immagine che spinge il primo brano sotto la piega ha invertito
   * il rapporto fra la copertina e quel che c'è dentro.
   */
  copertina?: React.ReactNode;
  /** Assente quando la pagina non è cercabile — Impostazioni, per dire. */
  query?: string | undefined;
  onQuery?: ((testo: string) => void) | undefined;
  /** Il bottone d'ordinamento, quando la pagina ne ha uno. */
  ordinamento?:
    | { etichetta: string; onApri: (e: React.MouseEvent) => void }
    | undefined;
  /** Quel che va a destra, prima della ricerca. */
  azioni?: React.ReactNode;
}) {
  return (
    <header className="intestazione-pagina page-header">
      {copertina}
      <div className="chi">
        {occhiello !== undefined && (
          <div className="occhiello hero-eyebrow">{occhiello}</div>
        )}
        <h1 className="titolo page-title">{titolo}</h1>
        {sottotitolo !== undefined && (
          <div className="sotto page-subtitle">{sottotitolo}</div>
        )}
      </div>

      <div className="comandi">
        {azioni}
        {ordinamento && (
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={ordinamento.onApri}
            aria-label={t("header.sort", { nome: ordinamento.etichetta })}
          >
            <Icona nome="i-sort" dim={15} />
            <span>{ordinamento.etichetta}</span>
            <Icona nome="i-chev-d" dim={13} />
          </button>
        )}
        {onQuery && (
          <div className="cerca">
            <Icona nome="i-search" dim={15} />
            <input
              className="campo field-input"
              type="search"
              placeholder={t("header.search")}
              value={query ?? ""}
              onChange={(e) => onQuery(e.target.value)}
              spellCheck={false}
              /* `data-cerca` invece di un ref passato da fuori: la scorciatoia
                 `/` deve poter raggiungere questo campo da qualunque schermata,
                 e far viaggiare un ref attraverso quattro componenti per un
                 tasto sarebbe più codice di quanto ne risolva. */
              data-cerca="1"
            />
            {/* Il suggerimento sparisce quando il campo è pieno: a quel punto
                chi legge sa già come ci è arrivato. */}
            {(query ?? "").length === 0 && (
              <kbd className="scorciatoia" aria-hidden="true">
                /
              </kbd>
            )}
          </div>
        )}
      </div>
    </header>
  );
}
