/**
 * Lo sfondo di una superficie, a livelli.
 *
 * # Perché è la parte che mancava
 *
 * `parts.<nome>.background` è un array di effetti, e finora lo Studio lo leggeva
 * soltanto per sommarne il costo: per aggiungere una griglia di punti a una
 * scheda bisognava scendere nella vista Documento e scrivere l'oggetto a mano.
 * Il che vuol dire che la vista a controlli non era una seconda tastiera sullo
 * stesso documento — era una tastiera con meno tasti.
 *
 * # Il costo accanto al nome, sempre
 *
 * Ogni livello porta il suo peso, e l'elenco da cui si sceglie ce l'ha già
 * accanto a ciascuna voce: si vede quanto costa **prima** di spenderlo. È la
 * terza regola dello Studio, e senza questo elenco non aveva un posto dove
 * succedere.
 *
 * # Perché si riscrive l'array intero
 *
 * `patch.ts` non indicizza gli array, e non serve che lo faccia: aggiungere,
 * togliere e riordinare producono tutti e tre un array nuovo, e scriverlo in un
 * colpo solo è anche l'unico modo di non lasciare il documento in uno stato
 * intermedio a metà di un riordino.
 */
import { useState } from "react";

import type { EffettoRegistro, ParametroRegistro, TokenRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { Cursore, ValoreColore, ValoreLunghezza } from "./controlli";
import {
  coloreCosto,
  costoDi,
  type Livello,
  type Motivi,
  nomeEffetto,
  ritrattoEffetto,
} from "./valori";
import { t } from "../lingue";
import { usePresaPerRiordino } from "../riordino";
import { descrizioneParametro } from "./vocabolario";

/** Gli angoli che un `chamfer` può tagliare, come li scrive il documento. */
const ANGOLI = ["topLeft", "topRight", "bottomLeft", "bottomRight"] as const;

/**
 * Le manopole di un livello, generate dal registro.
 *
 * Non c'è una tabella di undici effetti scritta qui: c'è una `switch` su sette
 * tipi. È lo stesso principio dell'ispettore dei nodi — «una forma di controllo
 * per tipo» — e la conseguenza è che un effetto nuovo nel nucleo arriva già
 * modificabile, senza che nessuno tocchi questo file.
 */
export function Manopole({
  livello,
  params,
  tokens,
  tavolozza,
  onCambia,
}: {
  livello: Livello;
  params: readonly ParametroRegistro[];
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  onCambia: (livello: Livello) => void;
}) {
  // Le descrizioni dei parametri si chiavano sull'effetto: `color` esiste su
  // sei effetti e ogni volta dice un'altra cosa.
  const effetto = nomeEffetto(livello) ?? "";

  const scrivi = (nome: string, valore: unknown) => {
    const nuovo = { ...livello };
    // Togliere vuol dire togliere la chiave, non scrivere `null`: il parser
    // legge una chiave assente come «usa il valore di serie» e un `null` come
    // un errore di tipo.
    if (valore === null || valore === undefined) delete nuovo[nome];
    else nuovo[nome] = valore;
    onCambia(nuovo);
  };

  return (
    <div className="manopole-livello">
      {params.map((p) => {
        const valore = livello[p.name];
        return (
          <div
            key={p.name}
            className="una-manopola"
            title={descrizioneParametro(effetto, p.name, p.description)}
          >
            <span className="nome-manopola">{p.name}</span>

            {p.kind === "color" && (
              <ValoreColore
                valore={valore ?? null}
                tokens={tokens}
                tavolozza={tavolozza}
                onCambia={(v) => scrivi(p.name, v)}
              />
            )}

            {p.kind === "length" && (
              <ValoreLunghezza
                valore={valore ?? null}
                min={p.min ?? 0}
                max={p.max ?? 64}
                passo={1}
                etichetta={p.name}
                onCambia={(v) => scrivi(p.name, v)}
              />
            )}

            {(p.kind === "angle" || p.kind === "number") && (
              <Cursore
                valore={valore ?? null}
                min={p.min ?? 0}
                max={p.max ?? 100}
                passo={p.kind === "angle" ? 5 : 1}
                etichetta={p.name}
                suffisso={p.kind === "angle" ? "°" : ""}
                onCambia={(v) => scrivi(p.name, v)}
              />
            )}

            {p.kind === "word" && (
              <Segmentato
                etichetta={descrizioneParametro(effetto, p.name, p.description)}
                classe="minuto denso"
                scelta={String(valore ?? "")}
                onScegli={(v) => scrivi(p.name, v === "" ? null : v)}
                voci={[
                  ...(p.optional ? [{ chiave: "", etichetta: "—" }] : []),
                  ...p.allowed.map((a) => ({ chiave: a, etichetta: a })),
                ]}
              />
            )}

            {p.kind === "corners" && (
              <div
                className="angoli"
                role="group"
                aria-label={t("studio.part.corners")}
              >
                {ANGOLI.map((angolo) => {
                  const quali = Array.isArray(valore) ? valore.map(String) : [];
                  const acceso = quali.includes(angolo);
                  return (
                    <button
                      key={angolo}
                      type="button"
                      className="angolo"
                      aria-label={angolo}
                      aria-pressed={acceso}
                      data-active={acceso || undefined}
                      onClick={() => {
                        const nuovi = acceso
                          ? quali.filter((a) => a !== angolo)
                          : [...quali, angolo];
                        // Nessun angolo dichiarato vuol dire **tutti**: una
                        // lista vuota il formato la legge al contrario.
                        scrivi(p.name, nuovi.length === 0 ? null : nuovi);
                      }}
                    />
                  );
                })}
              </div>
            )}

            {p.kind === "stops" && (
              <Fermate
                fermate={Array.isArray(valore) ? valore : []}
                tokens={tokens}
                tavolozza={tavolozza}
                onCambia={(f) => scrivi(p.name, f)}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

/** Una fermata di un gradiente: un colore, e volendo dove si ferma. */
type Fermata = { color?: unknown; at?: string };

/**
 * Le fermate di un gradiente.
 *
 * Un gradiente ne vuole almeno due — il parser lo pretende — quindi il bottone
 * che le toglie si spegne a due invece di lasciar scrivere un documento che poi
 * non si valida.
 */
function Fermate({
  fermate,
  tokens,
  tavolozza,
  onCambia,
}: {
  fermate: readonly Fermata[];
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  onCambia: (fermate: Fermata[]) => void;
}) {
  return (
    <div className="fermate">
      {fermate.map((fermata, indice) => (
        // L'indice è la chiave perché è l'identità: una fermata è il suo posto
        // nel gradiente, e due dello stesso colore non sono la stessa.
        <div key={indice} className="una-fermata">
          <ValoreColore
            valore={fermata.color ?? null}
            tokens={tokens}
            tavolozza={tavolozza}
            onCambia={(v) =>
              onCambia(
                fermate.map((f, i) => (i === indice ? { ...f, color: v } : f)),
              )
            }
          />
          <input
            className="dove field-input"
            type="text"
            inputMode="numeric"
            aria-label={t("studio.layers.stopAt", { n: indice + 1 })}
            placeholder="auto"
            value={fermata.at ?? ""}
            onChange={(e) =>
              onCambia(
                fermate.map((f, i) => {
                  if (i !== indice) return f;
                  const { at: _tolto, ...resto } = f;
                  return e.target.value === ""
                    ? resto
                    : { ...resto, at: e.target.value };
                }),
              )
            }
          />
          <button
            type="button"
            className="via icon-btn"
            aria-label={t("studio.layers.removeStop", { n: indice + 1 })}
            disabled={fermate.length <= 2}
            title={
              fermate.length <= 2 ? t("studio.layers.twoStops") : undefined
            }
            onClick={() => onCambia(fermate.filter((_, i) => i !== indice))}
          >
            <Icona nome="i-x" dim={11} />
          </button>
        </div>
      ))}
      <button
        type="button"
        className="aggiungi-livello"
        onClick={() => onCambia([...fermate, { color: "#000000" }])}
      >
        <Icona nome="i-plus" dim={12} />
        <span>{t("studio.layers.addStop")}</span>
      </button>
    </div>
  );
}

export function Livelli({
  livelli,
  effetti,
  tokens,
  tavolozza,
  budget,
  titolo,
  motivi = {},
  onCambia,
}: {
  livelli: readonly Livello[];
  effetti: readonly EffettoRegistro[];
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  budget: number;
  /** Come si chiama questa pila: lo sfondo, o il livello su `::after`. */
  titolo?: string;
  /**
   * I motivi dichiarati dal documento: nome → effetto per esteso.
   *
   * La mappa e non i soli nomi, perché serve a due cose che devono restare
   * d'accordo — offrirli nell'elenco, e sapere quanto costa e che faccia ha un
   * livello che ne riferisce uno.
   */
  motivi?: Motivi;
  /** L'array nuovo. Vuoto vuol dire «togli la dichiarazione». */
  onCambia: (livelli: Livello[]) => void;
}) {
  const [apertoElenco, setApertoElenco] = useState(false);
  const [preso, setPreso] = useState<number | null>(null);
  /** Dove cadrebbe il livello che si sta trascinando. */
  const [sopra, setSopra] = useState<number | null>(null);
  const [aperto, setAperto] = useState<number | null>(null);

  /** Solo gli effetti che si possono impilare come sfondo. */
  const impilabili = effetti.filter((e) => e.target === "background");

  const aggiungi = (effetto: EffettoRegistro) => {
    try {
      onCambia([...livelli, JSON.parse(effetto.esempio) as Livello]);
      // Aperto subito. L'esemplare del nucleo è un minimo valido — `solid` è un
      // rettangolo nero — e lasciarlo chiuso vuol dire consegnare un livello che
      // non è quel che si voleva e non dire dove si cambia.
      setAperto(livelli.length);
    } catch {
      // L'esemplare viene dal nucleo e non da qui: se non è JSON, il posto dove
      // aggiustarlo è `esempio()` in `studio.rs`, non un ripiego inventato ora.
    }
    setApertoElenco(false);
  };

  /** Un motivo già dichiarato, riusato come livello. */
  const aggiungiMotivo = (nome: string) => {
    onCambia([...livelli, { $pattern: nome } as Livello]);
    setApertoElenco(false);
  };

  const togli = (indice: number) =>
    onCambia(livelli.filter((_, i) => i !== indice));

  /** Sposta un livello, e restituisce l'array nuovo. */
  const sposta = (da: number, a: number) => {
    if (da === a) return;
    const nuovi = [...livelli];
    const [tolto] = nuovi.splice(da, 1);
    if (tolto === undefined) return;
    nuovi.splice(a, 0, tolto);
    onCambia(nuovi);
  };

  // Col puntatore, non col trascinamento HTML5: dentro la finestra di Aether
  // quello non arriva mai al rilascio, e la pila si prendeva ma non si
  // riordinava. Vedi `riordino.ts`, che è lo stesso gesto della coda.
  const presa = usePresaPerRiordino({
    onPresa: (indice) => {
      setPreso(indice);
      if (indice === null) setSopra(null);
    },
    onMira: setSopra,
    onLascia: (a) => {
      if (preso !== null) sposta(preso, a);
      setPreso(null);
      setSopra(null);
    },
  });

  return (
    <div className="livelli">
      <div className="testa-sezione">
        <span className="titolino">{titolo ?? t("studio.layers.title")}</span>
        <span className="da-dove">{t("studio.layers.fromBottom")}</span>
      </div>

      {/* `data-elenco`: ogni livello ha il suo guscio — sotto di lui si aprono
          le manopole — quindi i livelli non sono fratelli, e senza questo il
          riordino non riconoscerebbe due righe della stessa pila. */}
      <div className="pila" data-elenco>
        {livelli.map((livello, indice) => {
          const nome = nomeEffetto(livello, motivi) ?? "?";
          const costo = costoDi(livello, effetti, motivi);
          const definizione = effetti.find((e) => e.name === nome);
          const daMotivo = typeof livello["$pattern"] === "string";
          const apertoQui = aperto === indice;
          return (
            <div key={indice} className="livello-con-manopole">
              <div
                // L'indice è la chiave perché è l'identità: due `solid` nella
                // stessa pila sono due livelli diversi e nient'altro li distingue.
                className="livello"
                data-riordino={indice}
                data-preso={preso === indice || undefined}
                data-sopra={(sopra === indice && preso !== indice) || undefined}
                data-aperto={apertoQui || undefined}
                onPointerDown={(e) => presa(e, indice)}
              >
                <span className="maniglia" aria-hidden="true">
                  <Icona nome="i-grip" dim={12} />
                </span>
                <span
                  className="ritratto"
                  style={{
                    background: ritrattoEffetto(
                      livello,
                      tokens,
                      tavolozza,
                      motivi,
                    ),
                  }}
                  aria-hidden="true"
                />
                {/* Un riferimento a un motivo non ha manopole proprie: si
                    modifica dove il motivo è dichiarato, e cambiarlo qui lo
                    cambierebbe in ogni superficie che lo usa. Dirlo è meglio
                    che offrire dei controlli che poi non compaiono. */}
                {daMotivo ? (
                  <code
                    className="quale da-motivo"
                    title={t("studio.layers.fromPatterns")}
                  >
                    ${String(livello["$pattern"])}
                  </code>
                ) : (
                  <button
                    type="button"
                    className="quale apri"
                    aria-expanded={apertoQui}
                    disabled={definizione === undefined}
                    onClick={() => setAperto(apertoQui ? null : indice)}
                  >
                    <Icona
                      nome={apertoQui ? "i-chev-u" : "i-chev-d"}
                      dim={11}
                    />
                    <code>{nome}</code>
                  </button>
                )}
                <span
                  className="peso"
                  style={{ color: coloreCosto(costo, budget) }}
                >
                  {costo}
                </span>
                <button
                  type="button"
                  className="via icon-btn"
                  aria-label={t("studio.layers.remove", { nome })}
                  onClick={() => {
                    togli(indice);
                    setAperto(null);
                  }}
                >
                  <Icona nome="i-x" dim={12} />
                </button>
              </div>

              {apertoQui && definizione !== undefined && (
                <Manopole
                  livello={livello}
                  params={definizione.params}
                  tokens={tokens}
                  tavolozza={tavolozza}
                  onCambia={(nuovo) =>
                    onCambia(livelli.map((l, i) => (i === indice ? nuovo : l)))
                  }
                />
              )}
            </div>
          );
        })}

        <button
          type="button"
          className="aggiungi-livello"
          aria-expanded={apertoElenco}
          onClick={() => setApertoElenco((prima) => !prima)}
        >
          <Icona nome={apertoElenco ? "i-chev-u" : "i-plus"} dim={13} />
          <span>{t("studio.layers.add")}</span>
          <span className="quanti">
            {t("studio.layers.effects", { n: impilabili.length })}
          </span>
        </button>

        {apertoElenco && (
          <div className="elenco-effetti" role="menu">
            {/* I motivi in testa: sono già dichiarati e già pagati una volta,
                quindi riusarne uno è quasi sempre la risposta migliore a
                «voglio quella texture anche qui». */}
            {Object.keys(motivi).length > 0 && (
              <>
                <span className="titolo-gruppo">
                  {t("studio.layers.patterns")}
                </span>
                {Object.keys(motivi).map((nome) => (
                  <button
                    key={nome}
                    type="button"
                    role="menuitem"
                    className="un-effetto"
                    onClick={() => aggiungiMotivo(nome)}
                  >
                    <code>${nome}</code>
                    <span className="da-dove">
                      {t("studio.layers.fromPattern")}
                    </span>
                  </button>
                ))}
                <span className="titolo-gruppo">
                  {t("studio.layers.effectList")}
                </span>
              </>
            )}
            {impilabili.map((effetto) => (
              <button
                key={effetto.name}
                type="button"
                role="menuitem"
                className="un-effetto"
                onClick={() => aggiungi(effetto)}
              >
                <code>{effetto.name}</code>
                {/* Il peso **prima** di spenderlo: è tutto il punto della terza
                    regola, e un elenco senza questo numero costringerebbe a
                    scoprirlo aggiungendo. */}
                <span className="peso" style={{ color: coloreCosto(effetto.cost, budget) }}>
                  {effetto.cost}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
