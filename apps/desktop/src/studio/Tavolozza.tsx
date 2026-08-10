/**
 * Studio · Tavolozza, capacità e temi.
 *
 * # Le capacità dicono cosa devono a chi le accende
 *
 * Accendere «variante chiara» non mette un `true` e basta: fa comparire subito
 * l'elenco dei dieci token che mancano. Una capacità dichiarata e vuota è una
 * promessa che salta fuori alla validazione — ed è esattamente il difetto che
 * `plain.json` aveva, con `light: true` e otto token su quaranta sovrascritti.
 *
 * # Una sorgente per token, non un interruttore per la skin
 *
 * `DynamicSource` sta **sul singolo token**: `color.accent` può seguire la
 * copertina mentre `color.text.1` resta fisso. È la differenza col vecchio
 * `supportsDynamicAccent`, che era un sì o un no per tutta la skin. Il testo non
 * segue mai la copertina, ed è la regola che tiene il contrasto sotto controllo
 * quando la tinta è imprevedibile.
 */
import type { Registro, Validazione } from "../ipc";
import { Icona } from "../parti/Icone";
import { leggi, scrivi, scriviIn, togliDa } from "./patch";

/** I dieci token che una variante chiara deve dichiarare per non essere vuota. */
const DIECI_DEL_CHIARO = [
  "color.surface.0",
  "color.surface.1",
  "color.surface.2",
  "color.surface.3",
  "color.text.1",
  "color.text.2",
  "color.text.3",
  "color.accent",
  "color.accent.soft",
  "color.accent.glow",
] as const;

/** Le sorgenti che una copertina può offrire. */
const SORGENTI = [
  "albumArt.vibrant",
  "albumArt.darkVibrant",
  "albumArt.lightVibrant",
  "albumArt.muted",
  "albumArt.darkMuted",
] as const;

/** I token che ha senso far seguire alla copertina: mai il testo. */
const CANDIDATI_DINAMICI = ["color.accent", "color.hero", "color.ambient.1", "color.ambient.2"];

function Interruttore({
  acceso,
  onCambia,
  etichetta,
}: {
  acceso: boolean;
  onCambia: (v: boolean) => void;
  etichetta: string;
}) {
  return (
    <button
      type="button"
      className="interruttore switch"
      role="switch"
      aria-checked={acceso}
      aria-label={etichetta}
      onClick={() => onCambia(!acceso)}
    >
      <span className="pista switch-track" aria-hidden="true">
        <span className="pallina" />
      </span>
    </button>
  );
}

export function Tavolozza({
  sorgente,
  onSorgente,
  esito,
  registro,
  onVaiA,
  onEsporta,
}: {
  sorgente: string;
  onSorgente: (testo: string) => void;
  esito: Validazione | null;
  registro: Registro | null;
  /** Porta il fuoco su un token o su una parte, dall'elenco di controllo. */
  onVaiA: (percorso: string) => void;
  /** Scrive il `.aeskin`. È lo stesso bottone della testa, qui in fondo. */
  onEsporta: () => void;
}) {
  const documento = leggi(sorgente);
  const capacita = (documento?.capabilities ?? {}) as Record<string, boolean>;
  const tavolozza = (documento?.palette ?? {}) as Record<string, string>;
  const impaginazione = (documento?.layout ?? {}) as Record<string, string>;
  const movimento = (documento?.motion ?? {}) as Record<string, unknown>;
  const tokens = (documento?.tokens ?? {}) as Record<string, unknown>;
  const chiaro = ((documento?.themes ?? {}) as Record<string, unknown>).light as
    | Record<string, unknown>
    | undefined;

  const usi = new Map(esito?.tavolozza ?? []);
  const mancanti = DIECI_DEL_CHIARO.filter((t) => chiaro?.[t] === undefined);
  const conSorgente = CANDIDATI_DINAMICI.filter((t) => {
    const v = tokens[t];
    return v !== null && typeof v === "object" && "$source" in (v as object);
  });

  /** Accende una capacità, e le fa portare quel che promette. */
  const cambiaCapacita = (nome: string, valore: boolean) => {
    onSorgente(scriviIn(sorgente, ["capabilities", nome], valore));
  };

  const cambiaImpaginazione = (campo: string, valore: string) => {
    onSorgente(scriviIn(sorgente, ["layout", campo], valore));
  };

  const cambiaSorgente = (token: string, sorgenteColore: string | null) => {
    onSorgente(
      sorgenteColore === null
        ? togliDa(sorgente, ["tokens", token])
        : scriviIn(sorgente, ["tokens", token], { $source: sorgenteColore }),
    );
  };

  /** Un nome che non è già preso, per il colore nuovo. */
  const nomeLibero = () => {
    for (let n = 1; ; n += 1) {
      const proposto = n === 1 ? "colore" : `colore-${n}`;
      if (tavolozza[proposto] === undefined) return proposto;
    }
  };

  const cambiaColore = (nome: string, valore: string) => {
    onSorgente(scriviIn(sorgente, ["palette", nome], valore));
  };

  const togliColore = (nome: string) => {
    onSorgente(togliDa(sorgente, ["palette", nome]));
  };

  /**
   * Rinomina un colore, e con lui i riferimenti che lo nominano.
   *
   * È il punto in cui una rinomina «solo del nome» romperebbe il documento: ogni
   * `{ "$palette": "<vecchio>" }` sparso fra token e parti diventerebbe un
   * riferimento a un colore che non esiste, e la skin smetterebbe di compilare
   * per una modifica che sembrava cosmetica. Si riscrive l'albero intero: è
   * l'unico modo di non doverne cercare le occorrenze a mano.
   */
  const rinominaColore = (vecchio: string, nuovo: string) => {
    if (documento === null || nuovo === "" || nuovo === vecchio) return;
    if (tavolozza[nuovo] !== undefined) return;

    const rinomina = (cosa: unknown): unknown => {
      if (Array.isArray(cosa)) return cosa.map(rinomina);
      if (cosa === null || typeof cosa !== "object") return cosa;
      const dentro = cosa as Record<string, unknown>;
      if (dentro["$palette"] === vecchio) return { ...dentro, $palette: nuovo };
      return Object.fromEntries(
        Object.entries(dentro).map(([chiave, valore]) => [chiave, rinomina(valore)]),
      );
    };

    // La tavolozza si ricostruisce a parte, e in ordine: la chiave è il nome, e
    // rinominare una chiave con uno `spread` la sposterebbe in fondo — un diff
    // che tocca tutto il blocco per una lettera cambiata.
    const nuovaTavolozza = Object.fromEntries(
      Object.entries(tavolozza).map(([chiave, valore]) => [
        chiave === vecchio ? nuovo : chiave,
        valore,
      ]),
    );
    const riscritto = rinomina({ ...documento, palette: {} }) as Record<string, unknown>;
    riscritto["palette"] = nuovaTavolozza;
    onSorgente(scrivi(riscritto));
  };

  const budget = registro?.budget ?? 10;
  const fuoriBudget = (esito?.avvisi ?? []).filter((a) => a.kind === "costBudget");
  const sottoSoglia = (esito?.contrasti ?? []).filter((c) => !c.passa);
  const senzaErrori = (esito?.errori.length ?? 0) === 0;
  const maiUsati = Object.keys(tavolozza).filter((nome) => (usi.get(nome) ?? 0) === 0);
  const obbligatoriMancanti = (esito?.avvisi ?? []).filter(
    (a) => a.kind === "missingRequiredToken",
  );
  const obbligatori = registro?.tokens.filter((t) => t.required).length ?? 0;

  return (
    <div className="studio-tavolozza">
      <div className="colonna">
        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-skin" dim={16} />
            </span>
            <h3 className="section-heading">Tavolozza locale</h3>
            <span className="nota-testa">
              {Object.keys(tavolozza).length} colori
            </span>
          </header>
          {Object.keys(tavolozza).length === 0 && (
            <p className="niente">
              Nessun colore locale. La tavolozza serve a non ripetere lo stesso
              valore in venti dichiarazioni — nessun componente la legge per nome.
            </p>
          )}
          <div className="colori">
            {Object.entries(tavolozza).map(([nome, valore]) => {
              const quante = usi.get(nome) ?? 0;
              return (
                <div key={nome} className="colore" data-mai={quante === 0 || undefined}>
                  <input
                    className="pastiglia"
                    type="color"
                    aria-label={`Il valore di ${nome}`}
                    value={valore.startsWith("#") ? valore : "#000000"}
                    onChange={(e) => cambiaColore(nome, e.target.value)}
                  />
                  <input
                    className="nome"
                    type="text"
                    aria-label={`Il nome di ${nome}`}
                    defaultValue={nome}
                    spellCheck={false}
                    // Alla conferma e non a ogni tasto: rinominare riscrive
                    // l'albero intero, e farlo a ogni lettera vorrebbe dire un
                    // documento nuovo per ogni carattere digitato.
                    onBlur={(e) => rinominaColore(nome, e.target.value.trim())}
                    onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                  />
                  <code className="valore">{valore}</code>
                  {/* «Usato 14×» è il numero che trasforma una tavolozza in
                      un sistema: un colore usato una volta sola non è un
                      colore della skin, è un letterale con un nome. Non è un
                      avviso, però — è un'informazione. */}
                  <span className="uso">
                    {quante === 0 ? "mai usato" : `usato ${quante}×`}
                  </span>
                  <button
                    type="button"
                    className="via icon-btn"
                    aria-label={`Togli ${nome}`}
                    title={
                      quante > 0
                        ? `${nome} è riferito ${quante} volte: toglierlo rompe quei riferimenti`
                        : undefined
                    }
                    onClick={() => togliColore(nome)}
                  >
                    <Icona nome="i-x" dim={12} />
                  </button>
                </div>
              );
            })}
            <button
              type="button"
              className="aggiungi-colore"
              onClick={() => cambiaColore(nomeLibero(), "#808080")}
            >
              <Icona nome="i-plus" dim={13} />
              Aggiungi colore
            </button>
          </div>

          {maiUsati.length > 0 && (
            /* L'avviso che il crate prevede e che nessun ramo produce ancora:
               `WarningKind::UnusedPattern` è dichiarato in `check_skin` e non
               esce mai. Qui c'è il posto dove serve davvero — è l'unico momento
               in cui qualcuno guarda la tavolozza. */
            <p className="nota gialla">
              <strong>{maiUsati.join(", ")}</strong>{" "}
              {maiUsati.length === 1 ? "è dichiarato e non usato" : "sono dichiarati e non usati"}.
              Un colore in tavolozza si può riferire con un&apos;opacità (
              <code>{'{ "$palette": "ruggine", "alpha": 0.16 }'}</code>): è così
              che le varianti <i>soft</i> e <i>glow</i> restano lo stesso colore.
            </p>
          )}
        </section>

        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-album" dim={16} />
            </span>
            <h3 className="section-heading">Quali token seguono la copertina</h3>
          </header>
          <div className="dinamici">
            {CANDIDATI_DINAMICI.map((token) => {
              const valore = tokens[token];
              const attuale =
                valore !== null && typeof valore === "object" && "$source" in (valore as object)
                  ? String((valore as Record<string, unknown>)["$source"])
                  : "";
              return (
                <div key={token} className="dinamico">
                  <code className="token">{token}</code>
                  <select
                    className="scelta field-input"
                    value={attuale}
                    aria-label={`Sorgente di ${token}`}
                    onChange={(e) =>
                      cambiaSorgente(token, e.target.value === "" ? null : e.target.value)
                    }
                  >
                    <option value="">fisso</option>
                    {SORGENTI.map((s) => (
                      <option key={s} value={s}>
                        {s}
                      </option>
                    ))}
                  </select>
                </div>
              );
            })}
            <div className="dinamico spento">
              <code className="token">color.text.1</code>
              <span className="fisso">fisso — e va bene così</span>
            </div>
          </div>
          <p className="nota">
            Una sorgente <strong>per token</strong>, non un interruttore per tutta
            la skin: è la differenza fra <code>DynamicSource</code> e il vecchio{" "}
            <code>supportsDynamicAccent</code>. Il testo non segue mai la
            copertina — è la regola che tiene il contrasto sotto controllo quando
            la tinta è imprevedibile.
          </p>
        </section>
      </div>

      <div className="colonna">
        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-check" dim={16} />
            </span>
            <h3 className="section-heading">Capacità dichiarate</h3>
          </header>

          <div className="riga-opzione">
            <div className="che-cosa">
              <div className="etichetta">Variante chiara</div>
              <div
                className="spiegazione"
                style={{
                  color:
                    capacita["light"] && mancanti.length > 0 ? "var(--warning)" : undefined,
                }}
              >
                {capacita["light"]
                  ? mancanti.length === 0
                    ? "Dichiarata e definita: tutti e dieci i token ci sono."
                    : `Dichiarata e vuota: ${mancanti.length} token da definire, altrimenti l'interruttore del tema apparirebbe e non farebbe niente.`
                  : "Spenta: l'interruttore del tema non si mostra."}
              </div>
              {capacita["light"] && mancanti.length > 0 && (
                <div className="mancanti">
                  {mancanti.map((t) => (
                    <button key={t} type="button" className="chip" onClick={() => onVaiA(t)}>
                      {t}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <Interruttore
              acceso={Boolean(capacita["light"])}
              etichetta="Variante chiara"
              onCambia={(v) => cambiaCapacita("light", v)}
            />
          </div>

          <div className="riga-opzione">
            <div className="che-cosa">
              <div className="etichetta">Sovrascritture per telefono</div>
              <div className="spiegazione">
                Android non c&apos;è ancora, e prometterla adesso è un avviso in
                più a ogni validazione.
              </div>
            </div>
            <Interruttore
              acceso={Boolean(capacita["mobile"])}
              etichetta="Sovrascritture per telefono"
              onCambia={(v) => cambiaCapacita("mobile", v)}
            />
          </div>

          <div className="riga-opzione">
            <div className="che-cosa">
              <div className="etichetta">Accento dalla copertina</div>
              <div className="spiegazione">
                {conSorgente.length > 0
                  ? `Accesa: ${conSorgente.length} token hanno una sorgente.`
                  : "Senza almeno un token con una sorgente, la capacità è un'altra promessa a vuoto."}
              </div>
            </div>
            <Interruttore
              acceso={capacita["dynamicAccent"] !== false}
              etichetta="Accento dalla copertina"
              onCambia={(v) => cambiaCapacita("dynamicAccent", v)}
            />
          </div>
        </section>

        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-settings" dim={16} />
            </span>
            <h3 className="section-heading">Impalcatura e movimento</h3>
            <span className="nota-testa">l&apos;app li legge davvero</span>
          </header>
          <div className="quattro">
            {(
              [
                ["player", "Lettore", ["floating", "bottom-bar", "compact"]],
                ["sidebar", "Barra laterale", ["rail", "expanded", "hidden"]],
                ["density", "Densità", ["comfortable", "compact", "spacious"]],
              ] as const
            ).map(([campo, etichetta, valori]) => (
              <div key={campo} className="scelta-impaginazione">
                <div className="che">{etichetta}</div>
                <div className="valori">
                  {valori.map((v) => (
                    <button
                      key={v}
                      type="button"
                      className="valore"
                      data-active={
                        (impaginazione[campo] ?? valori[0]) === v || undefined
                      }
                      onClick={() => cambiaImpaginazione(campo, v)}
                    >
                      {v}
                    </button>
                  ))}
                </div>
              </div>
            ))}
            <div className="scelta-impaginazione">
              <div className="che">Movimento</div>
              <div className="valori">
                {(["full", "essential", "none", "maximum"] as const).map((v) => (
                  <button
                    key={v}
                    type="button"
                    className="valore"
                    data-active={(movimento["intensity"] ?? "full") === v || undefined}
                    onClick={() =>
                      onSorgente(scriviIn(sorgente, ["motion", "intensity"], v))
                    }
                  >
                    {v}
                  </button>
                ))}
              </div>
            </div>
          </div>
          <p className="nota">
            Questi quattro fino a poco fa <strong>nessuno li leggeva</strong>:{" "}
            <code>plain.json</code> li dichiarava e l&apos;interfaccia li
            ignorava. Ora li onora — e <code>movimento: none</code> vale come{" "}
            <code>prefers-reduced-motion</code>, non in alternativa: chi ha
            spento le animazioni nel sistema vince sulla skin, sempre.
          </p>
        </section>

        <section className="scheda section-card prima-di-esportare">
          <header>
            <span className="section-icon">
              <Icona nome="i-import" dim={16} />
            </span>
            <h3 className="section-heading">Prima di esportare</h3>
          </header>
          {/* Cinque voci e non tre: le due che mancavano — i token obbligatori e
              la variante chiara vuota — erano già calcolate altrove e non
              comparivano proprio nell'elenco che si guarda prima di esportare. */}
          <ul className="controlli">
            {(
              [
                {
                  esito: senzaErrori ? "bene" : "male",
                  testo: senzaErrori
                    ? `Il documento è valido — 0 errori, formato ${registro?.format ?? 1}`
                    : `${esito?.errori.length ?? 0} errori bloccano l'esportazione`,
                  dove: esito?.errori[0]?.path,
                },
                {
                  esito: obbligatoriMancanti.length === 0 ? "bene" : "attenzione",
                  testo:
                    obbligatoriMancanti.length === 0
                      ? `${obbligatori} token obbligatori dichiarati`
                      : `${obbligatoriMancanti.length} token obbligatori su ${obbligatori} non dichiarati`,
                  dove: obbligatoriMancanti[0]?.path,
                },
                {
                  esito: fuoriBudget.length === 0 ? "bene" : "attenzione",
                  testo:
                    fuoriBudget.length === 0
                      ? `Nessuna superficie fuori budget — costo totale ${esito?.costo ?? 0}`
                      : `${fuoriBudget.length} superfici fuori dal budget di ${budget}`,
                  dove: fuoriBudget[0]?.path,
                },
                {
                  esito: sottoSoglia.length === 0 ? "bene" : "male",
                  testo:
                    sottoSoglia.length === 0
                      ? `Ogni coppia misurata supera ${registro?.contrastoMinimo ?? 4.5}:1`
                      : `${sottoSoglia.length} coppie sotto ${registro?.contrastoMinimo ?? 4.5}:1 — ${sottoSoglia[0]?.davanti} su ${sottoSoglia[0]?.dietro}`,
                  dove: sottoSoglia[0]?.davanti,
                },
                {
                  esito:
                    !capacita["light"] || mancanti.length === 0 ? "bene" : "attenzione",
                  testo: !capacita["light"]
                    ? "Nessuna variante chiara promessa"
                    : mancanti.length === 0
                      ? "La variante chiara è completa"
                      : `La variante chiara è vuota — ${mancanti.length} token`,
                  dove: mancanti[0],
                },
              ] as const
            ).map((riga, i) => (
              <li key={i} data-esito={riga.esito}>
                <Icona
                  nome={riga.esito === "bene" ? "i-check" : "i-alert"}
                  dim={15}
                  titolo={riga.esito === "bene" ? "a posto" : "da guardare"}
                />
                <span>{riga.testo}</span>
                {riga.esito !== "bene" && riga.dove !== undefined && riga.dove !== "" && (
                  <button
                    type="button"
                    className="vai"
                    onClick={() => onVaiA(riga.dove ?? "")}
                  >
                    vai
                  </button>
                )}
              </li>
            ))}
          </ul>
          <div className="piede-esportare">
            <p className="nota">
              Gli avvisi <strong>non bloccano</strong>: una skin con un avviso è
              una skin che funziona. Bloccano gli errori
              {senzaErrori ? ", e questa non ne ha" : ""}.
            </p>
            <button
              type="button"
              className="pillola btn-accent"
              disabled={!senzaErrori}
              title={senzaErrori ? undefined : "Gli errori bloccano l'esportazione"}
              onClick={onEsporta}
            >
              <Icona nome="i-import" dim={15} />
              {senzaErrori && (fuoriBudget.length > 0 || sottoSoglia.length > 0)
                ? "Esporta comunque"
                : "Esporta .aeskin"}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}
