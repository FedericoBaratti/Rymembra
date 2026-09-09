/**
 * L'editor di un token.
 *
 * # Il buco che chiude
 *
 * L'albero di sinistra elencava i token del registro per gruppo, col pallino
 * «dichiarato» e il segno dell'obbligatorietà, **e cliccarli non faceva niente**:
 * il gestore era `"name" in voce ? setParteScelta(voce.name) : undefined`, e per
 * i token quel ramo restituiva `undefined`. Cambiare il colore d'accento — che è
 * la prima cosa che chiunque voglia fare a una skin — richiedeva di scendere
 * nella vista Documento e scrivere JSON a mano.
 *
 * # Perché non c'è una tabella di settantatré voci
 *
 * Perché sarebbe una seconda copia del registro, e le due copie divergono al
 * primo token aggiunto. Qui non si nomina nessun token: si legge `kind`,
 * `min`/`max`, `required` e `description`, che il registro dichiara già e che
 * `studio_registro` porta di là. `ControlloPerTipo` sceglie il controllo. Gli
 * undici token del ritmo e della profondità sono arrivati senza che questo file
 * li conoscesse, e i quindici della scena dello spettro dopo di loro: sono la
 * prova che il meccanismo funziona.
 *
 * # La striscia dei preset, e perché non nomina neanche quelli
 *
 * Stessa regola, un giro più in là. Un preset porta con sé il `group` su cui
 * agisce — nello stesso vocabolario di `TokenRegistro.group`, che è la sola
 * ragione per cui quel campo esiste — e la striscia si costruisce confrontando
 * quel campo col gruppo del token aperto. Quindi qui dentro non compare né
 * «Classico» né `canvas.viz.haze`: compare «i preset di questo gruppo», e il
 * giorno che qualcuno scriverà in `preset.rs` un blocco per il ritmo o per le
 * superfici, quel blocco apparirà da solo sotto i token del ritmo e delle
 * superfici, senza che questo file cambi di una riga.
 *
 * La striscia sta sotto i due controlli e non sopra, ed è deliberato: un preset
 * riscrive **tutto il gruppo**, cioè anche il token che si sta guardando e ogni
 * altro che gli sta accanto nell'albero, aperto o no. È un gesto più grosso di
 * quello per cui si è aperto questo pannello, e va incontrato dopo aver visto
 * cosa c'è, non prima.
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
import type { PresetRegistro, TokenRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { ControlloPerTipo } from "./controlli";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";
import { descrizionePreset, descrizioneToken } from "./vocabolario";

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
  presets,
  tavolozza,
  onPreset,
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
  /**
   * Tutti i preset del registro, non quelli di questo gruppo.
   *
   * Arrivano interi apposta: il filtro è una riga sola e sta qui, dove si sa
   * qual è il gruppo aperto. Chiederlo già filtrato a chi monta il componente
   * vorrebbe dire che anche lui deve saperlo, cioè due posti che sanno la stessa
   * cosa invece di uno.
   */
  presets: readonly PresetRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  /** Applica un blocco di valori: è **un** passo di annullo, non uno per voce. */
  onPreset: (preset: PresetRegistro) => void;
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
  /* Il filtro, che è tutto il meccanismo: il confronto è fra due stringhe che
     escono dalla stessa funzione di `studio.rs`, quindi non c'è una tabella da
     tenere allineata — c'è un `===`. */
  const pronti = presets.filter((p) => p.group === definizione.group);

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

        {/* I blocchi pronti per il gruppo aperto.

            Non si disegna la striscia vuota: dodici gruppi su tredici non
            hanno preset, e una riga che dice «nessun preset» ripetuta sotto ogni
            colore sarebbe rumore permanente per un'informazione che non serve a
            nessuno — chi cerca un preset e non lo trova ha già la risposta.

            I nomi passano da `descrizionePreset`, non da `preset.nome`: il nome
            del crate è il ripiego, e la riga del catalogo — quando c'è — è
            quella che parla la lingua di chi guarda. */}
        {pronti.length > 0 && (
          <div className="preset-token section-card">
            <span className="titolino">{t("studio.preset.title")}</span>
            <div className="azioni-nodo">
              {pronti.map((preset) => (
                <button
                  key={preset.id}
                  type="button"
                  className="pillola btn-ghost"
                  title={t("studio.preset.apply.title", {
                    quanti: preset.valori.length,
                  })}
                  onClick={() => onPreset(preset)}
                >
                  {descrizionePreset(preset.id, preset.nome)}
                </button>
              ))}
            </div>
            {/* Quel che il bottone fa oltre a quel che si vede: riscrive anche i
                token che non sono aperti. Detto prima, non scoperto dopo. */}
            <p className="nota">
              {t("studio.preset.hint", { gruppo: definizione.group })}
            </p>
          </div>
        )}

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
