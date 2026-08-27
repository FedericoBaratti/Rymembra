/**
 * L'editor di un token.
 *
 * # Il buco che chiude
 *
 * L'albero di sinistra elencava i cinquantotto token per gruppo, col pallino
 * «dichiarato» e il segno dell'obbligatorietà, **e cliccarli non faceva niente**:
 * il gestore era `"name" in voce ? setParteScelta(voce.name) : undefined`, e per
 * i token quel ramo restituiva `undefined`. Cambiare il colore d'accento — che è
 * la prima cosa che chiunque voglia fare a una skin — richiedeva di scendere
 * nella vista Documento e scrivere JSON a mano.
 *
 * # Perché non c'è una tabella di cinquantotto voci
 *
 * Perché sarebbe una seconda copia del registro, e le due copie divergono al
 * primo token aggiunto. Qui non si nomina nessun token: si legge `kind`,
 * `min`/`max`, `required` e `description`, che il registro dichiara già e che
 * `studio_registro` porta di là. `ControlloPerTipo` sceglie il controllo. Gli
 * undici token del ritmo e della profondità sono arrivati senza che questo file
 * li conoscesse, ed è la prova che il meccanismo funziona.
 *
 * # Le due colonne
 *
 * Scuro e chiaro accanto, non due schermate. `themes.light` è un secondo insieme
 * di token che vale **solo** in tema chiaro, e la domanda che ci si pone davanti
 * è sempre «e nell'altro tema?»: separarli vorrebbe dire tenere a mente il primo
 * mentre si sceglie il secondo. La colonna chiara si accende solo se la skin
 * dichiara la capacità, perché altrimenti quel che ci si scrive non lo legge
 * nessuno.
 */
import type { TokenRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { ControlloPerTipo } from "./controlli";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";
import { descrizioneToken } from "./vocabolario";

/** Come si legge un tipo, per chi non conosce i nomi del crate. */
function comeSiChiama(): Readonly<Record<string, string>> {
  return {
    color: t("studio.kind.color"),
    length: t("studio.kind.length"),
    duration: t("studio.kind.duration"),
    easing: t("studio.kind.easing"),
    number: t("studio.kind.number"),
    fontStack: t("studio.kind.fontStack"),
    shadow: t("studio.kind.shadow"),
  };
}

export function Token({
  definizione,
  valore,
  valoreChiaro,
  chiaroPromesso,
  tokens,
  tavolozza,
  onScrivi,
  onScriviChiaro,
  onVaiAlJson,
}: {
  definizione: TokenRegistro | null;
  valore: unknown;
  valoreChiaro: unknown;
  /** La skin dichiara `capabilities.light`. */
  chiaroPromesso: boolean;
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  onScrivi: (valore: unknown) => void;
  onScriviChiaro: (valore: unknown) => void;
  onVaiAlJson: (percorso: string) => void;
}) {
  if (definizione === null) {
    return (
      <aside className="ispettore">
        <div className="niente-scelto">
          <p>{t("studio.token.none")}</p>
          <p className="sotto">{t("studio.token.none.hint")}</p>
        </div>
      </aside>
    );
  }

  const dichiarato = valore !== undefined;
  const dichiaratoChiaro = valoreChiaro !== undefined;
  const limitato = definizione.min !== null && definizione.max !== null;

  return (
    <aside className="ispettore editor-token">
      <header className="testa-ispettore">
        <code className="nome-parte">{definizione.id}</code>
        <span className="gruppo-parte">{definizione.group}</span>
        {definizione.required && (
          <span
            className="chip-livello"
            title={t("studio.token.required.title")}
          >
            {t("studio.token.required")}
          </span>
        )}
        <p className="descrizione">
          {descrizioneToken(definizione.id, definizione.description)}
        </p>
      </header>

      <div className="corpo-ispettore">
        <div className="proprieta">
          <div className="proprieta-riga">
            <span className="che">{t("studio.token.dark")}</span>
            <div className="come">
              <ControlloPerTipo
                tipo={definizione.kind}
                valore={valore ?? null}
                token={definizione}
                tokens={tokens}
                tavolozza={tavolozza}
                onCambia={onScrivi}
              />
            </div>
          </div>

          <div
            className="proprieta-riga"
            data-spento={!chiaroPromesso || undefined}
          >
            <span className="che">{t("studio.token.light")}</span>
            <div className="come">
              {chiaroPromesso ? (
                <ControlloPerTipo
                  tipo={definizione.kind}
                  valore={valoreChiaro ?? null}
                  token={definizione}
                  tokens={tokens}
                  tavolozza={tavolozza}
                  onCambia={onScriviChiaro}
                />
              ) : (
                <p className="nota">
                  <Trans
                    k="studio.token.noLight"
                    v={{ token: <code>capabilities.light</code> }}
                  />
                </p>
              )}
            </div>
          </div>
        </div>

        {/* Il contratto in chiaro. Il nome della variabile CSS non è un
            dettaglio da nascondere: è quel che si cerca in `stile.css` quando ci
            si chiede chi legge questo token, ed è l'unico ponte fra il documento
            e il foglio. */}
        <div className="patto-token section-card">
          <div className="riga-patto">
            <span className="titolino">{t("studio.token.writes")}</span>
            <code>{definizione.css}</code>
          </div>
          <div className="riga-patto">
            <span className="titolino">{t("studio.token.wants")}</span>
            <span>{comeSiChiama()[definizione.kind] ?? definizione.kind}</span>
          </div>
          {limitato && (
            <div className="riga-patto">
              <span className="titolino">{t("studio.token.between")}</span>
              <span>
                {t("studio.token.range", {
                  min: definizione.min ?? 0,
                  max: definizione.max ?? 0,
                })}
                {definizione.kind === "length" ? " px" : ""}
                {/* Gli estremi non sono un capriccio dell'editor: è il
                    validatore che rifiuta quel che ne esce, e il cursore ne
                    prende i capi proprio per non offrire un valore che il
                    salvataggio respinge. */}
              </span>
            </div>
          )}
          {!dichiarato && (
            <p className="nota">{t("studio.token.undeclared")}</p>
          )}
        </div>

        <div className="azioni-nodo">
          <button
            type="button"
            className="pillola btn-ghost"
            disabled={!dichiarato && !dichiaratoChiaro}
            onClick={() => {
              onScrivi(null);
              if (dichiaratoChiaro) onScriviChiaro(null);
            }}
          >
            <Icona nome="i-x" dim={13} />
            {t("studio.token.reset")}
          </button>
          <button
            type="button"
            className="pillola btn-ghost"
            title={t("studio.token.seeJson.title")}
            onClick={() => onVaiAlJson(`tokens.${definizione.id}`)}
          >
            <Icona nome="i-text" dim={13} />
            {t("studio.token.seeJson")}
          </button>
        </div>
      </div>
    </aside>
  );
}
