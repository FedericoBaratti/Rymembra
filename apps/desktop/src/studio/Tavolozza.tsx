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
import { Manopole } from "./Livelli";
import { leggi, scrivi, scriviIn, togliDa } from "./patch";
import {
  coloreCosto,
  costoDi,
  type Livello,
  nomeEffetto,
  ritrattoEffetto,
} from "./valori";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";

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
const CANDIDATI_DINAMICI = [
  "color.accent",
  "color.hero",
  "color.ambient.1",
  "color.ambient.2",
];

/**
 * I campi di `meta` che si scrivono liberamente.
 *
 * `preview` non c'è: sono tre colori e hanno il loro controllo. `version` sì,
 * perché è testo — il formato vuole un semver e a dirlo è il validatore, non un
 * campo che indovina.
 */
function campiMeta() {
  return [
    ["name", t("studio.meta.name"), t("studio.meta.name.hint")],
    ["author", t("studio.meta.author"), t("studio.meta.author.hint")],
    ["version", t("studio.meta.version"), "1.0.0"],
    [
      "description",
      t("studio.meta.description"),
      t("studio.meta.description.hint"),
    ],
    ["license", t("studio.meta.license"), t("studio.meta.license.hint")],
  ] as const;
}

/**
 * Le manopole di un motivo, più la scelta di **quale** effetto è.
 *
 * Un livello di una pila si toglie e se ne aggiunge un altro; un motivo no — ha
 * un nome che il resto del documento riferisce, e cambiarne il tipo deve
 * lasciare in piedi il nome. Perciò qui c'è un elenco chiuso di effetti dove
 * nella pila c'è un bottone «togli».
 */
function ManopoleMotivo({
  effetto,
  registro,
  tavolozza,
  onCambia,
}: {
  effetto: unknown;
  registro: Registro | null;
  tavolozza: Readonly<Record<string, string>>;
  onCambia: (effetto: unknown) => void;
}) {
  const quale = nomeEffetto(effetto);
  const definizione = registro?.effects.find((e) => e.name === quale);
  // I motivi finiscono in `background`, in `clip-path` o in `backdrop-filter`
  // a seconda dell'effetto, e il validatore controlla che il posto in cui si
  // usano combaci. Qui si offrono tutti: a dire dove ci sta è il registro.
  const disponibili = registro?.effects ?? [];

  return (
    <div className="manopole-motivo">
      <div className="una-manopola">
        <span className="nome-manopola">effect</span>
        <select
          className="field-input"
          aria-label={t("studio.pattern.which")}
          value={quale ?? ""}
          onChange={(e) => {
            const scelto = disponibili.find((d) => d.name === e.target.value);
            if (scelto === undefined) return;
            try {
              // Si riparte dall'esemplare del nucleo invece di travasare i
              // campi vecchi: `dotGrid` e `scanlines` non hanno gli stessi
              // parametri, e un travaso lascerebbe chiavi che il parser
              // rifiuta.
              onCambia(JSON.parse(scelto.esempio));
            } catch {
              // L'esemplare viene dal nucleo: se non è JSON si aggiusta là.
            }
          }}
        >
          {disponibili.map((d) => (
            <option key={d.name} value={d.name}>
              {d.name} · {d.cost}
            </option>
          ))}
        </select>
      </div>
      {definizione !== undefined && (
        <Manopole
          livello={(effetto ?? {}) as Livello}
          params={definizione.params}
          tokens={registro?.tokens ?? []}
          tavolozza={tavolozza}
          onCambia={onCambia}
        />
      )}
    </div>
  );
}

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
  onApriToken,
  onEsporta,
}: {
  sorgente: string;
  onSorgente: (testo: string) => void;
  esito: Validazione | null;
  registro: Registro | null;
  /** Porta il fuoco su un token o su una parte, dall'elenco di controllo. */
  onVaiA: (percorso: string) => void;
  /** Apre un token nel suo editor, nella vista Ispeziona. */
  onApriToken: (id: string) => void;
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
    const riscritto = rinomina({ ...documento, palette: {} }) as Record<
      string,
      unknown
    >;
    riscritto["palette"] = nuovaTavolozza;
    onSorgente(scrivi(riscritto));
  };

  const meta = (documento?.meta ?? {}) as Record<string, unknown>;
  const anteprimaMeta = (meta["preview"] ?? {}) as Record<string, unknown>;
  const idDichiarato = String(documento?.["id"] ?? "");
  const versione = String(meta["version"] ?? "—");
  const motivi = (documento?.patterns ?? {}) as Record<string, unknown>;
  /** La skin scrive il suo albero: da lì in poi `player` e `sidebar` non contano. */
  const conScafale = impaginazione["shell"] !== undefined;

  const cambiaMeta = (campo: string, valore: string) => {
    onSorgente(
      valore === ""
        ? togliDa(sorgente, ["meta", campo])
        : scriviIn(sorgente, ["meta", campo], valore),
    );
  };

  /** Un nome di motivo che non è già preso. */
  const nomeMotivoLibero = () => {
    for (let n = 1; ; n += 1) {
      const proposto = n === 1 ? "motivo" : `motivo-${n}`;
      if (motivi[proposto] === undefined) return proposto;
    }
  };

  /**
   * Rinomina un motivo, e con lui i riferimenti che lo nominano.
   *
   * Identica a `rinominaColore` nella sostanza e per la stessa ragione: un
   * `{ "$pattern": "<vecchio>" }` rimasto indietro è un riferimento a un motivo
   * che non esiste, e la skin smette di compilare per una modifica che sembrava
   * cosmetica.
   */
  const rinominaMotivo = (vecchio: string, nuovo: string) => {
    if (documento === null || nuovo === "" || nuovo === vecchio) return;
    if (motivi[nuovo] !== undefined) return;

    const rinomina = (cosa: unknown): unknown => {
      if (Array.isArray(cosa)) return cosa.map(rinomina);
      if (cosa === null || typeof cosa !== "object") return cosa;
      const dentro = cosa as Record<string, unknown>;
      if (dentro["$pattern"] === vecchio) return { ...dentro, $pattern: nuovo };
      return Object.fromEntries(
        Object.entries(dentro).map(([chiave, valore]) => [
          chiave,
          rinomina(valore),
        ]),
      );
    };

    const nuoviMotivi = Object.fromEntries(
      Object.entries(motivi).map(([chiave, valore]) => [
        chiave === vecchio ? nuovo : chiave,
        valore,
      ]),
    );
    const riscritto = rinomina({ ...documento, patterns: {} }) as Record<
      string,
      unknown
    >;
    riscritto["patterns"] = nuoviMotivi;
    onSorgente(scrivi(riscritto));
  };

  const budget = registro?.budget ?? 10;
  const fuoriBudget = (esito?.avvisi ?? []).filter(
    (a) => a.kind === "costBudget",
  );
  const sottoSoglia = (esito?.contrasti ?? []).filter((c) => !c.passa);
  const senzaErrori = (esito?.errori.length ?? 0) === 0;
  const maiUsati = Object.keys(tavolozza).filter(
    (nome) => (usi.get(nome) ?? 0) === 0,
  );
  const obbligatoriMancanti = (esito?.avvisi ?? []).filter(
    (a) => a.kind === "missingRequiredToken",
  );
  const obbligatori = registro?.tokens.filter((t) => t.required).length ?? 0;

  return (
    <div className="studio-tavolozza">
      <div className="colonna">
        {/*
          L'identità, prima di tutto il resto.
          Nome, autore e versione si scrivevano alla creazione e poi non si
          toccavano più se non nel JSON — e sono proprio i campi che si
          correggono dopo, quando la skin ha preso una direzione diversa da
          quella che aveva il giorno in cui è nata.
        */}
        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-mark" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.palette.identity")}</h3>
            <span className="nota-testa">{versione}</span>
          </header>
          <div className="identita">
            <label className="campo-identita">
              <span className="titolino">id</span>
              <input
                className="field-input mono"
                type="text"
                value={idDichiarato}
                spellCheck={false}
                placeholder={t("studio.palette.id.hint")}
                onChange={(e) =>
                  onSorgente(scriviIn(sorgente, ["id"], e.target.value))
                }
              />
            </label>
            {campiMeta().map(([campo, etichetta, suggerimento]) => (
              <label key={campo} className="campo-identita">
                <span className="titolino">{etichetta}</span>
                <input
                  className="field-input"
                  type="text"
                  value={String(meta[campo] ?? "")}
                  placeholder={suggerimento}
                  onChange={(e) => cambiaMeta(campo, e.target.value)}
                />
              </label>
            ))}
          </div>
          {/* I tre colori del biglietto da visita: è quel che si vede
              nell'elenco delle skin prima di installarne una, cioè l'unica cosa
              che qualcuno guarda prima di decidere. */}
          <div className="tre-colori">
            <span className="titolino">{t("studio.palette.card")}</span>
            {(["bg", "fg", "accent"] as const).map((quale) => (
              <input
                key={quale}
                type="color"
                className="pastiglia"
                aria-label={t("studio.palette.cardColor", { quale })}
                value={String(anteprimaMeta[quale] ?? "#000000")}
                onChange={(e) =>
                  onSorgente(
                    scriviIn(
                      sorgente,
                      ["meta", "preview", quale],
                      e.target.value,
                    ),
                  )
                }
              />
            ))}
          </div>
        </section>

        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-skin" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.palette.title")}</h3>
            <span className="nota-testa">
              {t("studio.palette.count", { n: Object.keys(tavolozza).length })}
            </span>
          </header>
          {Object.keys(tavolozza).length === 0 && (
            <p className="niente">{t("studio.palette.empty")}</p>
          )}
          <div className="colori">
            {Object.entries(tavolozza).map(([nome, valore]) => {
              const quante = usi.get(nome) ?? 0;
              return (
                <div
                  key={nome}
                  className="colore"
                  data-mai={quante === 0 || undefined}
                >
                  <input
                    className="pastiglia"
                    type="color"
                    aria-label={t("studio.palette.valueOf", { nome })}
                    value={valore.startsWith("#") ? valore : "#000000"}
                    onChange={(e) => cambiaColore(nome, e.target.value)}
                  />
                  <input
                    className="nome"
                    type="text"
                    aria-label={t("studio.palette.nameOf", { nome })}
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
                    {quante === 0
                      ? t("studio.palette.never")
                      : t("studio.palette.used", { n: quante })}
                  </span>
                  <button
                    type="button"
                    className="via icon-btn"
                    aria-label={t("studio.palette.remove", { nome })}
                    title={
                      quante > 0
                        ? t("studio.palette.removeWarn", { nome, n: quante })
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
              {t("studio.palette.add")}
            </button>
          </div>

          {maiUsati.length > 0 && (
            /* L'avviso che il crate prevede e che nessun ramo produce ancora:
               `WarningKind::UnusedPattern` è dichiarato in `check_skin` e non
               esce mai. Qui c'è il posto dove serve davvero — è l'unico momento
               in cui qualcuno guarda la tavolozza. */
            <p className="nota gialla">
              <Trans
                k="studio.palette.unused"
                n={{ n: maiUsati.length }}
                v={{
                  quali: <strong>{maiUsati.join(", ")}</strong>,
                  esempio: (
                    <code>{'{ "$palette": "ruggine", "alpha": 0.16 }'}</code>
                  ),
                  soft: <i>soft</i>,
                  glow: <i>glow</i>,
                }}
              />
            </p>
          )}
        </section>

        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-album" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.dynamic.title")}</h3>
          </header>
          <div className="dinamici">
            {CANDIDATI_DINAMICI.map((token) => {
              const valore = tokens[token];
              const attuale =
                valore !== null &&
                typeof valore === "object" &&
                "$source" in (valore as object)
                  ? String((valore as Record<string, unknown>)["$source"])
                  : "";
              return (
                <div key={token} className="dinamico">
                  <code className="token">{token}</code>
                  <select
                    className="scelta field-input"
                    value={attuale}
                    aria-label={t("studio.dynamic.sourceOf", { token })}
                    onChange={(e) =>
                      cambiaSorgente(
                        token,
                        e.target.value === "" ? null : e.target.value,
                      )
                    }
                  >
                    <option value="">{t("studio.dynamic.fixed")}</option>
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
              <span className="fisso">{t("studio.dynamic.fixedOk")}</span>
            </div>
          </div>
          <p className="nota">
            <Trans
              k="studio.dynamic.note"
              v={{
                perToken: <strong>{t("studio.dynamic.note.perToken")}</strong>,
                nuovo: <code>DynamicSource</code>,
                vecchio: <code>supportsDynamicAccent</code>,
              }}
            />
          </p>
        </section>

        {/*
          I motivi: un effetto dichiarato una volta e riusato per nome.
          Non avevano nessun editor — né qui né nell'ispettore — quindi l'unico
          modo di scriverne uno era il JSON, e l'unico modo di sapere che
          esistessero era leggere `sala.json`.
        */}
        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-list" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.patterns.title")}</h3>
            <span className="nota-testa">{Object.keys(motivi).length}</span>
          </header>
          {Object.keys(motivi).length === 0 && (
            <p className="niente">{t("studio.patterns.empty")}</p>
          )}
          <div className="motivi">
            {Object.entries(motivi).map(([nome, effetto]) => (
              <div key={nome} className="un-motivo">
                <div className="testa-motivo">
                  <span
                    className="ritratto"
                    style={{
                      background: ritrattoEffetto(
                        effetto as Livello,
                        registro?.tokens ?? [],
                        tavolozza,
                      ),
                    }}
                    aria-hidden="true"
                  />
                  <input
                    className="nome field-input"
                    type="text"
                    aria-label={t("studio.palette.nameOf", { nome })}
                    defaultValue={nome}
                    spellCheck={false}
                    // Come per i colori: alla conferma, perché rinominare
                    // riscrive ogni `{ "$pattern": … }` sparso nel documento.
                    onBlur={(e) => rinominaMotivo(nome, e.target.value.trim())}
                    onKeyDown={(e) =>
                      e.key === "Enter" && e.currentTarget.blur()
                    }
                  />
                  <span
                    className="peso"
                    style={{
                      color: coloreCosto(
                        costoDi(effetto, registro?.effects ?? []),
                        budget,
                      ),
                    }}
                  >
                    {costoDi(effetto, registro?.effects ?? [])}
                  </span>
                  <button
                    type="button"
                    className="via icon-btn"
                    aria-label={t("studio.palette.remove", { nome })}
                    onClick={() =>
                      onSorgente(togliDa(sorgente, ["patterns", nome]))
                    }
                  >
                    <Icona nome="i-x" dim={12} />
                  </button>
                </div>
                {/* Un motivo è **un** effetto, non una pila: `Livelli` con un
                    array da uno solo darebbe un bottone «aggiungi» che scrive
                    un documento che il parser rifiuta. Le manopole sì, quelle
                    sono le stesse. */}
                <ManopoleMotivo
                  effetto={effetto}
                  registro={registro}
                  tavolozza={tavolozza}
                  onCambia={(nuovo) =>
                    onSorgente(scriviIn(sorgente, ["patterns", nome], nuovo))
                  }
                />
              </div>
            ))}
          </div>
          <button
            type="button"
            className="aggiungi-livello"
            onClick={() => {
              const primo = (registro?.effects ?? []).find(
                (e) => e.target === "background",
              );
              if (primo === undefined) return;
              try {
                onSorgente(
                  scriviIn(
                    sorgente,
                    ["patterns", nomeMotivoLibero()],
                    JSON.parse(primo.esempio),
                  ),
                );
              } catch {
                // L'esemplare viene dal nucleo: se non è JSON si aggiusta là.
              }
            }}
          >
            <Icona nome="i-plus" dim={13} />
            <span>{t("studio.patterns.add")}</span>
          </button>
        </section>
      </div>

      <div className="colonna">
        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-check" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.caps.title")}</h3>
          </header>

          <div className="riga-opzione">
            <div className="che-cosa">
              <div className="etichetta">{t("studio.caps.light")}</div>
              <div
                className="spiegazione"
                style={{
                  color:
                    capacita["light"] && mancanti.length > 0 ? "var(--warning)" : undefined,
                }}
              >
                {capacita["light"]
                  ? mancanti.length === 0
                    ? t("studio.caps.light.full")
                    : t("studio.caps.light.empty", { n: mancanti.length })
                  : t("studio.caps.light.off")}
              </div>
              {capacita["light"] && mancanti.length > 0 && (
                <div className="mancanti">
                  {/* Il chip apre il token, non il JSON. Prima era un elenco di
                      rimproveri che mandava a cercare la riga giusta in un file
                      di duecento righe: sapeva già dire cosa manca, e non
                      sapeva darti dove scriverlo. */}
                  {mancanti.map((token) => (
                    <button
                      key={token}
                      type="button"
                      className="chip"
                      title={t("studio.caps.light.open", { token })}
                      onClick={() => onApriToken(token)}
                    >
                      {token}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <Interruttore
              acceso={Boolean(capacita["light"])}
              etichetta={t("studio.caps.light")}
              onCambia={(v) => cambiaCapacita("light", v)}
            />
          </div>

          <div className="riga-opzione">
            <div className="che-cosa">
              <div className="etichetta">{t("studio.caps.mobile")}</div>
              <div className="spiegazione">{t("studio.caps.mobile.why")}</div>
            </div>
            <Interruttore
              acceso={Boolean(capacita["mobile"])}
              etichetta={t("studio.caps.mobile")}
              onCambia={(v) => cambiaCapacita("mobile", v)}
            />
          </div>

          <div className="riga-opzione">
            <div className="che-cosa">
              <div className="etichetta">{t("studio.caps.accent")}</div>
              <div className="spiegazione">
                {conSorgente.length > 0
                  ? t("studio.caps.accent.on", { n: conSorgente.length })
                  : t("studio.caps.accent.off")}
              </div>
            </div>
            <Interruttore
              acceso={capacita["dynamicAccent"] !== false}
              etichetta={t("studio.caps.accent")}
              onCambia={(v) => cambiaCapacita("dynamicAccent", v)}
            />
          </div>
        </section>

        <section className="scheda section-card">
          <header>
            <span className="section-icon">
              <Icona nome="i-settings" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.layout.title")}</h3>
            <span className="nota-testa">
              {conScafale
                ? t("studio.layout.twoOfFour")
                : t("studio.layout.allRead")}
            </span>
          </header>
          <div className="quattro">
            {(
              [
                [
                  "player",
                  t("studio.layout.player"),
                  ["floating", "bottom-bar", "compact"],
                ],
                [
                  "sidebar",
                  t("studio.layout.sidebar"),
                  ["rail", "expanded", "hidden"],
                ],
                [
                  "density",
                  t("studio.layout.density"),
                  ["comfortable", "compact", "spacious"],
                ],
              ] as const
            ).map(([campo, etichetta, valori]) => {
              // `player` e `sidebar` scelgono una variante dell'albero **di
              // serie**, e contano solo finché `layout.shell` non c'è — sta
              // scritto sulla loro definizione in `layout.rs`. Scritto lo
              // scafale, girarle non fa più niente: erano due manopole vive
              // sotto un titolo che prometteva che l'app le leggesse.
              const spenta = conScafale && campo !== "density";
              return (
                <div
                  key={campo}
                  className="scelta-impaginazione"
                  data-spenta={spenta || undefined}
                >
                  <div className="che">
                    {etichetta}
                    {spenta && (
                      <span className="perche">
                        {t("studio.layout.shellSays")}
                      </span>
                    )}
                  </div>
                  <div className="valori">
                    {valori.map((v) => (
                      <button
                        key={v}
                        type="button"
                        className="valore"
                        disabled={spenta}
                        title={
                          spenta ? t("studio.layout.shellWins") : undefined
                        }
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
              );
            })}
            <div className="scelta-impaginazione">
              <div className="che">{t("studio.layout.motion")}</div>
              <div className="valori">
                {(["full", "essential", "none", "maximum"] as const).map(
                  (v) => (
                    <button
                      key={v}
                      type="button"
                      className="valore"
                      data-active={
                        (movimento["intensity"] ?? "full") === v || undefined
                      }
                      onClick={() =>
                        onSorgente(
                          scriviIn(sorgente, ["motion", "intensity"], v),
                        )
                      }
                    >
                      {v}
                    </button>
                  ),
                )}
              </div>
            </div>
          </div>
          <p className="nota">
            {conScafale ? (
              <Trans
                k="studio.layout.note.shell"
                v={{
                  lettore: <strong>{t("studio.layout.player")}</strong>,
                  barra: <strong>{t("studio.layout.sidebar")}</strong>,
                  diSerie: <em>{t("studio.layout.note.shell.stock")}</em>,
                  impagina: <strong>{t("studio.view.impagina")}</strong>,
                }}
              />
            ) : (
              <Trans
                k="studio.layout.note.plain"
                v={{
                  nessuno: (
                    <strong>{t("studio.layout.note.plain.nobody")}</strong>
                  ),
                  plain: <code>plain.json</code>,
                  none: <code>motion: none</code>,
                  ridotto: <code>prefers-reduced-motion</code>,
                }}
              />
            )}
          </p>
        </section>

        <section className="scheda section-card prima-di-esportare">
          <header>
            <span className="section-icon">
              <Icona nome="i-import" dim={16} />
            </span>
            <h3 className="section-heading">{t("studio.export.title")}</h3>
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
                    ? t("studio.export.valid", {
                        formato: registro?.format ?? 1,
                      })
                    : t("studio.export.errors", {
                        n: esito?.errori.length ?? 0,
                      }),
                  dove: esito?.errori[0]?.path,
                },
                {
                  esito:
                    obbligatoriMancanti.length === 0 ? "bene" : "attenzione",
                  testo:
                    obbligatoriMancanti.length === 0
                      ? t("studio.export.required", { n: obbligatori })
                      : t("studio.export.requiredMissing", {
                          n: obbligatoriMancanti.length,
                          quanti: obbligatori,
                        }),
                  dove: obbligatoriMancanti[0]?.path,
                },
                {
                  esito: fuoriBudget.length === 0 ? "bene" : "attenzione",
                  testo:
                    fuoriBudget.length === 0
                      ? t("studio.export.budgetOk", {
                          costo: esito?.costo ?? 0,
                        })
                      : t("studio.export.budgetOver", {
                          n: fuoriBudget.length,
                          budget,
                        }),
                  dove: fuoriBudget[0]?.path,
                },
                {
                  esito: sottoSoglia.length === 0 ? "bene" : "male",
                  testo:
                    sottoSoglia.length === 0
                      ? t("studio.export.contrastOk", {
                          soglia: registro?.contrastoMinimo ?? 4.5,
                        })
                      : t("studio.export.contrastBad", {
                          n: sottoSoglia.length,
                          soglia: registro?.contrastoMinimo ?? 4.5,
                          davanti: sottoSoglia[0]?.davanti ?? "",
                          dietro: sottoSoglia[0]?.dietro ?? "",
                        }),
                  dove: sottoSoglia[0]?.davanti,
                },
                {
                  esito:
                    !capacita["light"] || mancanti.length === 0
                      ? "bene"
                      : "attenzione",
                  testo: !capacita["light"]
                    ? t("studio.export.noLight")
                    : mancanti.length === 0
                      ? t("studio.export.lightFull")
                      : t("studio.export.lightEmpty", { n: mancanti.length }),
                  dove: mancanti[0],
                },
              ] as const
            ).map((riga, i) => (
              <li key={i} data-esito={riga.esito}>
                <Icona
                  nome={riga.esito === "bene" ? "i-check" : "i-alert"}
                  dim={15}
                  titolo={
                    riga.esito === "bene"
                      ? t("studio.export.ok")
                      : t("studio.export.look")
                  }
                />
                <span>{riga.testo}</span>
                {riga.esito !== "bene" &&
                  riga.dove !== undefined &&
                  riga.dove !== "" && (
                    <button
                      type="button"
                      className="vai"
                      onClick={() => onVaiA(riga.dove ?? "")}
                    >
                      {t("studio.export.go")}
                    </button>
                  )}
              </li>
            ))}
          </ul>
          <div className="piede-esportare">
            <p className="nota">
              <Trans
                k={
                  senzaErrori
                    ? "studio.export.note.clean"
                    : "studio.export.note"
                }
                v={{
                  nonBloccano: <strong>{t("studio.export.note.dont")}</strong>,
                }}
              />
            </p>
            <button
              type="button"
              className="pillola btn-accent"
              disabled={!senzaErrori}
              title={senzaErrori ? undefined : t("studio.export.blocked")}
              onClick={onEsporta}
            >
              <Icona nome="i-import" dim={15} />
              {senzaErrori && (fuoriBudget.length > 0 || sottoSoglia.length > 0)
                ? t("studio.export.anyway")
                : t("studio.export.do")}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}
