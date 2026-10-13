/**
 * L'ispettore di una parte: com'è fatta, in che stati cambia, come si muove,
 * e il suo JSON.
 *
 * # Il formato 2, per intero
 *
 * L'ispettore dello Studio 2.x aveva otto righe: le otto proprietà che il
 * formato 1 accettava. Il formato 2 ne ha diciotto — l'ombra, l'anello del
 * fuoco, il carattere, la misura e l'interlinea del testo, i filtri
 * sull'elemento, il raggio per angolo, lo stile del bordo, il livello
 * `::before` — e qui ci sono tutte, ognuna col controllo del suo tipo e coi
 * capi che il validatore usa per rifiutarla. Quel che il formato non accetta —
 * misure, margini, posizione — non ha un controllo, e la nota in fondo dice
 * perché.
 *
 * # Dove si scrive
 *
 * Nello stato e nel tema scelti: `parts.card.states.hover.radius` guardando
 * l'hover della base, `variant.parts.card.radius` guardando la variante. Il
 * percorso lo compone `sentieri.ts`, e nessun controllo qui lo sa.
 */
import { useEffect, useState } from "react";

import type { ParteRegistro } from "../ipc";
import { t, tSe } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { nomeStato } from "./Centro";
import { useAzioniStudio, useStudio } from "./contesto";
import {
  Cursore,
  Parole,
  Riga,
  ValoreColore,
  ValoreCurva,
  ValoreDurata,
  ValoreLunghezza,
  ValoreOmbra,
} from "./controlli";
import type { SchedaIspettore } from "./negozio";
import { leggi, scrivi as stampa, valoreIn } from "./patch";
import { EditorCodice, useFonti } from "./EditorCodice";
import { Livelli as PilaSfondi } from "./Pila";
import { radiceParti, sentieroParte } from "./sentieri";
import { immaginiDi } from "./usaPacchetto";
import { coloreCosto, costoDi, type Livello } from "./valori";

const oggetto = (v: unknown): Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/** Il nome di un campo d'aspetto, per chi legge. */
export function nomeCampo(campo: string): string {
  return tSe(`studio3.field.${campo}`, campo);
}

/** Il nome di un innesco del movimento. */
export function nomeInnesco(nome: string): string {
  return tSe(`studio3.trigger.${nome}`, nomeStato(nome));
}

/** Un valore in poche lettere, per l'etichetta di un passo della storia. */
function breve(v: unknown): string {
  if (v === null || v === undefined) return t("studio3.step.removed");
  if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") return String(v);
  const d = oggetto(v);
  if (typeof d["$token"] === "string") return String(d["$token"]);
  if (typeof d["$palette"] === "string") return String(d["$palette"]);
  return "…";
}

/** Lettura e scrittura dei campi di una parte, nello stato e nel tema scelti. */
function useParte(parte: string) {
  const { s, documento, scrivi, parti } = useStudio();
  const def = parti.get(parte) ?? null;
  const dove = [
    parte,
    s.stato === null ? null : nomeStato(s.stato),
    s.tema === "variante" ? t("studio3.theme.variant") : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return {
    def,
    valore: (campo: string) =>
      documento === null ? undefined : valoreIn(documento, sentieroParte(parte, s.stato, campo, s.tema)),
    base: (campo: string) =>
      documento === null ? undefined : valoreIn(documento, [...radiceParti(s.tema), parte, campo]),
    scrivi: (campo: string, v: unknown) =>
      scrivi(
        sentieroParte(parte, s.stato, campo, s.tema),
        v,
        t("studio3.step.partField", { campo: nomeCampo(campo), dove, valore: breve(v) }),
      ),
    scriviBase: (campo: string, v: unknown) =>
      scrivi(
        [...radiceParti(s.tema), parte, campo],
        v,
        t("studio3.step.partField", { campo: nomeCampo(campo), dove, valore: breve(v) }),
      ),
  };
}

export function IspezioneParte({ parte }: { parte: string }) {
  const { s, manda } = useStudio();
  const { def } = useParte(parte);
  if (def === null) {
    return <p className="niente-ispettore">{t("studio3.inspector.unknownPart", { parte })}</p>;
  }
  const schede: readonly (readonly [SchedaIspettore, string])[] = [
    ["stile", t("studio3.inspector.tab.style")],
    ["stati", t("studio3.inspector.tab.states")],
    ["movimento", t("studio3.inspector.tab.motion")],
    ["codice", t("studio3.inspector.tab.code")],
  ];
  return (
    <div className="ispezione-parte">
      <TestaParte def={def} />
      <div className="schede-studio" role="tablist" aria-label={t("studio3.inspector.tabs")}>
        {schede.map(([chiave, etichetta], i) => (
          <button
            key={chiave}
            type="button"
            role="tab"
            className="scheda-studio"
            aria-selected={s.ispettore === chiave}
            title={`${etichetta} (${i + 1})`}
            onClick={() => manda({ tipo: "imposta", campi: { ispettore: chiave } })}
          >
            {etichetta}
          </button>
        ))}
      </div>
      <div className="corpo-ispezione" role="tabpanel">
        {s.ispettore === "stile" && <Stile def={def} />}
        {s.ispettore === "stati" && <Stati def={def} />}
        {s.ispettore === "movimento" && <Movimento def={def} />}
        {s.ispettore === "codice" && <CodiceParte parte={def.name} />}
      </div>
    </div>
  );
}

function TestaParte({ def }: { def: ParteRegistro }) {
  const { s, manda, registro } = useStudio();
  const costo = s.valido?.costi.find((c) => c.parte === def.name);
  return (
    <header className="testa-ispezione">
      <div className="riga-testa">
        <code className="nome-parte">.{def.name}</code>
        <button
          type="button"
          className="pillola btn-ghost minuto"
          title={t("studio3.inspector.show.hint")}
          onClick={() => manda({ tipo: "mostra", parte: def.name })}
        >
          <Icona nome="i-eye" dim={12} />
          {t("studio3.inspector.show")}
        </button>
      </div>
      <p className="descrizione">{tSe(`skin.part.${def.name}`, def.description)}</p>
      <div className="chip-parte">
        {def.before && <span className="chip-livello">::before</span>}
        {def.after && <span className="chip-livello">::after</span>}
        {def.presenza && <span className="chip-livello">{t("studio3.inspector.presence")}</span>}
        {def.condivisa && <span className="chip-livello">{t("studio3.inspector.shared")}</span>}
      </div>
      {costo !== undefined && (
        <div className="costi-parte">
          <Misuratore
            etichetta={t("studio3.inspector.cost.surface")}
            valore={costo.superficie}
            budget={registro.budget}
          />
          <Misuratore
            etichetta={t("studio3.inspector.cost.motion")}
            valore={costo.movimento}
            budget={registro.motionBudget}
          />
        </div>
      )}
    </header>
  );
}

export function Misuratore({
  etichetta,
  valore,
  budget,
}: {
  etichetta: string;
  valore: number;
  budget: number;
}) {
  return (
    <div className="misuratore-studio" title={`${etichetta}: ${valore} / ${budget}`}>
      <span className="che">{etichetta}</span>
      <span className="barra" data-sopra={valore > budget || undefined}>
        <span
          style={{
            width: `${Math.min(100, (valore / Math.max(1, budget)) * 100)}%`,
            background: coloreCosto(valore, budget),
          }}
        />
      </span>
      <span className="quanto">
        {valore}/{budget}
      </span>
    </div>
  );
}

/** Dove si sta scrivendo: lo stato e il tema, detti in una riga. */
function DoveScrivi() {
  const { s, manda } = useStudio();
  return (
    <div className="dove-scrivi" data-variante={s.tema === "variante" || undefined}>
      <span>
        {s.stato === null ? t("studio3.state.base") : nomeStato(s.stato)}
        {" · "}
        {s.tema === "variante" ? t("studio3.theme.variant") : t("studio3.theme.base")}
      </span>
      {s.stato !== null && (
        <button type="button" className="pillola btn-ghost minuto" onClick={() => manda({ tipo: "stato", stato: null })}>
          {t("studio3.inspector.backToBase")}
        </button>
      )}
    </div>
  );
}

/** Un gruppo di righe con un titolo: il puntino dice se dentro c'è qualcosa. */
function Gruppo({
  titolo,
  pieno,
  children,
}: {
  titolo: string;
  pieno: boolean;
  children: React.ReactNode;
}) {
  return (
    <section className="gruppo-ispezione">
      <h4 className="titolo-gruppo-ispezione" data-pieno={pieno || undefined}>
        {titolo}
      </h4>
      {children}
    </section>
  );
}

// ── Stile ────────────────────────────────────────────────────────────────────

function Stile({ def }: { def: ParteRegistro }) {
  const { s, registro, documento, file } = useStudio();
  const immagini = immaginiDi(file);
  const p = useParte(def.name);
  const tokens = registro.tokens;
  const tavolozza = oggetto(documento?.["palette"]) as Record<string, string>;
  const motivi = oggetto(documento?.["patterns"]);
  const c = (campo: string) => p.valore(campo);
  const dichiarato = (...campi: string[]) => campi.some((x) => c(x) !== undefined);
  const inStato = s.stato !== null;

  return (
    <div className="stile-parte">
      <DoveScrivi />

      <Gruppo titolo={t("studio3.group.fill")} pieno={dichiarato("background", "opacity")}>
        <PilaSfondi
          livelli={(Array.isArray(c("background")) ? c("background") : []) as Livello[]}
          effetti={registro.effects}
          tokens={tokens}
          tavolozza={tavolozza}
          motivi={motivi}
          immagini={immagini}
          budget={registro.budget}
          onCambia={(nuovi) => p.scrivi("background", nuovi.length === 0 ? null : nuovi)}
        />
        <Riga che={nomeCampo("opacity")}>
          <Cursore
            valore={c("opacity")}
            min={0}
            max={1}
            passo={0.01}
            etichetta={nomeCampo("opacity")}
            onCambia={(v) => p.scrivi("opacity", v)}
          />
        </Riga>
      </Gruppo>

      <Gruppo
        titolo={t("studio3.group.text")}
        pieno={dichiarato("textColor", "font", "fontSize", "lineHeight", "letterSpacing", "textTransform", "fontWeight")}
      >
        <Riga che={nomeCampo("textColor")}>
          <ValoreColore valore={c("textColor")} tokens={tokens} tavolozza={tavolozza} onCambia={(v) => p.scrivi("textColor", v)} />
        </Riga>
        <Riga che={nomeCampo("font")}>
          <Parole
            valore={c("font")}
            etichetta={nomeCampo("font")}
            onCambia={(v) => p.scrivi("font", v)}
            voci={[
              { chiave: "sans", etichetta: "Sans" },
              { chiave: "display", etichetta: "Display" },
              { chiave: "mono", etichetta: "Mono" },
            ]}
          />
        </Riga>
        <Riga che={nomeCampo("fontSize")}>
          <ValoreLunghezza
            valore={c("fontSize")}
            min={9}
            max={72}
            passo={1}
            soloAssolute
            etichetta={nomeCampo("fontSize")}
            onCambia={(v) => p.scrivi("fontSize", v)}
          />
        </Riga>
        <Riga che={nomeCampo("lineHeight")}>
          <Cursore
            valore={c("lineHeight")}
            min={0.8}
            max={3}
            passo={0.05}
            etichetta={nomeCampo("lineHeight")}
            onCambia={(v) => p.scrivi("lineHeight", v)}
          />
        </Riga>
        <Riga che={nomeCampo("letterSpacing")}>
          <ValoreLunghezza
            valore={c("letterSpacing")}
            min={-0.1}
            max={0.5}
            passo={0.005}
            unitaPredefinita="em"
            etichetta={nomeCampo("letterSpacing")}
            onCambia={(v) => p.scrivi("letterSpacing", v)}
          />
        </Riga>
        <Riga che={nomeCampo("textTransform")}>
          <Parole
            valore={c("textTransform")}
            etichetta={nomeCampo("textTransform")}
            onCambia={(v) => p.scrivi("textTransform", v)}
            voci={[
              { chiave: "none", etichetta: "Ab" },
              { chiave: "uppercase", etichetta: "AB" },
              { chiave: "lowercase", etichetta: "ab" },
              { chiave: "capitalize", etichetta: "Ab Cd" },
            ]}
          />
        </Riga>
        <Riga che={nomeCampo("fontWeight")}>
          <Cursore
            valore={c("fontWeight")}
            min={100}
            max={900}
            passo={50}
            etichetta={nomeCampo("fontWeight")}
            onCambia={(v) => p.scrivi("fontWeight", v)}
          />
        </Riga>
      </Gruppo>

      <Gruppo
        titolo={t("studio3.group.shape")}
        pieno={dichiarato("borderColor", "borderWidth", "borderStyle", "radius", "clip")}
      >
        <Riga che={nomeCampo("borderColor")}>
          <ValoreColore valore={c("borderColor")} tokens={tokens} tavolozza={tavolozza} onCambia={(v) => p.scrivi("borderColor", v)} />
        </Riga>
        <Riga che={nomeCampo("borderWidth")}>
          <ValoreLunghezza
            valore={c("borderWidth")}
            min={0}
            max={8}
            passo={0.5}
            etichetta={nomeCampo("borderWidth")}
            onCambia={(v) => p.scrivi("borderWidth", v)}
          />
        </Riga>
        <Riga che={nomeCampo("borderStyle")}>
          <Parole
            valore={c("borderStyle")}
            etichetta={nomeCampo("borderStyle")}
            onCambia={(v) => p.scrivi("borderStyle", v)}
            voci={[
              { chiave: "solid", etichetta: "—" },
              { chiave: "dashed", etichetta: "- -" },
              { chiave: "dotted", etichetta: "···" },
              { chiave: "double", etichetta: "=" },
              { chiave: "none", etichetta: "∅" },
            ]}
          />
        </Riga>
        <Riga che={nomeCampo("radius")}>
          <Raggio valore={c("radius")} onCambia={(v) => p.scrivi("radius", v)} />
        </Riga>
        <Riga che={nomeCampo("clip")}>
          <Smusso valore={c("clip")} onCambia={(v) => p.scrivi("clip", v)} />
        </Riga>
      </Gruppo>

      <Gruppo titolo={t("studio3.group.depth")} pieno={dichiarato("shadow", "focusRing")}>
        <Riga che={nomeCampo("shadow")}>
          <ValoreOmbra valore={c("shadow")} tokens={tokens} tavolozza={tavolozza} conToken onCambia={(v) => p.scrivi("shadow", v)} />
        </Riga>
        <Riga che={nomeCampo("focusRing")}>
          <AnelloFuoco valore={c("focusRing")} tavolozza={tavolozza} onCambia={(v) => p.scrivi("focusRing", v)} />
        </Riga>
      </Gruppo>

      <Gruppo titolo={t("studio3.group.filters")} pieno={dichiarato("filter", "backdrop")}>
        <Riga che={nomeCampo("filter")}>
          <PilaFiltri valore={c("filter")} tavolozza={tavolozza} onCambia={(v) => p.scrivi("filter", v)} />
        </Riga>
        <Riga che={nomeCampo("backdrop")}>
          <Vetro valore={c("backdrop")} budget={registro.budget} onCambia={(v) => p.scrivi("backdrop", v)} />
        </Riga>
      </Gruppo>

      {(def.before || def.after) && (
        <Gruppo titolo={t("studio3.group.layers")} pieno={p.base("before") !== undefined || p.base("after") !== undefined}>
          {inStato ? (
            <p className="nota-ispezione">{t("studio3.inspector.layersBaseOnly")}</p>
          ) : (
            (["before", "after"] as const)
              .filter((quale) => def[quale])
              .map((quale) => {
                const livello = oggetto(p.base(quale));
                const sfondi = (Array.isArray(livello["background"]) ? livello["background"] : []) as Livello[];
                return (
                  <div key={quale} className="livello-pseudo">
                    <PilaSfondi
                      livelli={sfondi}
                      effetti={registro.effects}
                      tokens={tokens}
                      tavolozza={tavolozza}
                      motivi={motivi}
                      immagini={immagini}
                      budget={registro.budget}
                      titolo={`::${quale}`}
                      onCambia={(nuovi) =>
                        p.scriviBase(
                          quale,
                          nuovi.length === 0
                            ? null
                            : livello["opacity"] === undefined
                              ? { background: nuovi }
                              : { background: nuovi, opacity: livello["opacity"] },
                        )
                      }
                    />
                    {sfondi.length > 0 && (
                      <Riga che={nomeCampo("opacity")}>
                        <Cursore
                          valore={livello["opacity"]}
                          min={0}
                          max={1}
                          passo={0.01}
                          etichetta={`::${quale} · ${nomeCampo("opacity")}`}
                          onCambia={(v) =>
                            p.scriviBase(quale, v === null ? { background: sfondi } : { background: sfondi, opacity: v })
                          }
                        />
                      </Riga>
                    )}
                  </div>
                );
              })
          )}
        </Gruppo>
      )}

      <p className="nota-ispezione">{t("studio3.inspector.absent")}</p>
    </div>
  );
}

/** Il raggio: uguale per tutti gli angoli, o uno per angolo. */
const ANGOLI = ["topLeft", "topRight", "bottomRight", "bottomLeft"] as const;

function Raggio({ valore, onCambia }: { valore: unknown; onCambia: (v: unknown) => void }) {
  const perAngolo = valore !== null && typeof valore === "object";
  const angoli = oggetto(valore);
  return (
    <div className="valore-raggio">
      <Segmentato<"uno" | "quattro">
        etichetta={nomeCampo("radius")}
        classe="minuto denso"
        scelta={perAngolo ? "quattro" : "uno"}
        onScegli={(forma) => {
          if (forma === "quattro" && !perAngolo) {
            const uno = typeof valore === "string" ? valore : "8px";
            onCambia(Object.fromEntries(ANGOLI.map((a) => [a, uno])));
          } else if (forma === "uno" && perAngolo) {
            onCambia(typeof angoli["topLeft"] === "string" ? angoli["topLeft"] : null);
          }
        }}
        voci={[
          { chiave: "uno", etichetta: t("studio3.radius.same") },
          { chiave: "quattro", etichetta: t("studio3.radius.each") },
        ]}
      />
      {perAngolo ? (
        <div className="quattro-angoli">
          {ANGOLI.map((angolo) => (
            <ValoreLunghezza
              key={angolo}
              valore={angoli[angolo]}
              min={0}
              max={64}
              passo={1}
              etichetta={t(`studio3.radius.${angolo}`)}
              onCambia={(v) => {
                const nuovi = { ...angoli, [angolo]: v ?? undefined };
                const restano = Object.fromEntries(Object.entries(nuovi).filter(([, x]) => x !== undefined));
                onCambia(Object.keys(restano).length === 0 ? null : restano);
              }}
            />
          ))}
        </div>
      ) : (
        <ValoreLunghezza
          valore={valore}
          min={0}
          max={64}
          passo={1}
          etichetta={nomeCampo("radius")}
          onCambia={onCambia}
        />
      )}
    </div>
  );
}

/** Lo smusso degli angoli: un `chamfer` con la sua misura e gli angoli tagliati. */
function Smusso({ valore, onCambia }: { valore: unknown; onCambia: (v: unknown) => void }) {
  const dentro = oggetto(valore);
  const tagliati = Array.isArray(dentro["corners"]) ? dentro["corners"].map(String) : [];
  const acceso = dentro["effect"] === "chamfer";
  return (
    <div className="ritaglio">
      <ValoreLunghezza
        valore={acceso ? dentro["size"] : null}
        min={0}
        max={48}
        passo={1}
        etichetta={t("studio3.clip.size")}
        onCambia={(v) => onCambia(v === null ? null : { ...dentro, effect: "chamfer", size: v })}
      />
      <div className="angoli" role="group" aria-label={t("studio3.clip.corners")}>
        {(["topLeft", "topRight", "bottomLeft", "bottomRight"] as const).map((angolo) => {
          const su = tagliati.includes(angolo);
          return (
            <button
              key={angolo}
              type="button"
              className="angolo"
              aria-label={t(`studio3.radius.${angolo}`)}
              aria-pressed={su}
              data-active={su || undefined}
              disabled={!acceso}
              onClick={() => {
                const quali = su ? tagliati.filter((a) => a !== angolo) : [...tagliati, angolo];
                // Nessun angolo dichiarato vuol dire tutti: togliere l'ultimo
                // toglie il campo, invece di scrivere una lista vuota.
                const { corners: _via, ...resto } = dentro;
                onCambia(quali.length === 0 ? resto : { ...resto, corners: quali });
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

/** L'anello del fuoco: colore, spessore, distanza. */
function AnelloFuoco({
  valore,
  tavolozza,
  onCambia,
}: {
  valore: unknown;
  tavolozza: Record<string, string>;
  onCambia: (v: unknown) => void;
}) {
  const { registro } = useAzioniStudio();
  if (valore === undefined || valore === null) {
    return (
      <button
        type="button"
        className="aggiungi-livello"
        onClick={() => onCambia({ color: { $token: "color.accent" }, width: "2px", offset: "2px" })}
      >
        <Icona nome="i-plus" dim={13} />
        <span>{t("studio3.focus.add")}</span>
      </button>
    );
  }
  const anello = oggetto(valore);
  const cambia = (campo: string, v: unknown) => {
    if (v === null && campo !== "offset") return;
    const nuovo = { ...anello, [campo]: v ?? undefined };
    onCambia(Object.fromEntries(Object.entries(nuovo).filter(([, x]) => x !== undefined)));
  };
  return (
    <div className="anello-fuoco">
      <ValoreColore valore={anello["color"]} tokens={registro.tokens} tavolozza={tavolozza} onCambia={(v) => cambia("color", v)} />
      <ValoreLunghezza valore={anello["width"]} min={0} max={8} passo={0.5} soloAssolute etichetta={t("studio3.focus.width")} onCambia={(v) => cambia("width", v)} />
      <ValoreLunghezza valore={anello["offset"]} min={-12} max={12} passo={0.5} soloAssolute etichetta={t("studio3.focus.offset")} onCambia={(v) => cambia("offset", v)} />
      <button type="button" className="pillola btn-ghost minuto" onClick={() => onCambia(null)}>
        <Icona nome="i-x" dim={11} />
        {t("studio3.focus.remove")}
      </button>
    </div>
  );
}

/** I filtri sull'elemento, in ordine: l'ordine cambia il risultato. */
const FILTRI = ["blur", "brightness", "contrast", "saturate", "grayscale", "hueRotate", "dropShadow"] as const;
type Filtro = (typeof FILTRI)[number];

const FILTRO_NUOVO: Readonly<Record<Filtro, unknown>> = {
  blur: "2px",
  brightness: 1.1,
  contrast: 1.1,
  saturate: 1.3,
  grayscale: 1,
  hueRotate: 30,
  dropShadow: { x: "0px", y: "2px", blur: "6px", color: "#00000055" },
};

function PilaFiltri({
  valore,
  tavolozza,
  onCambia,
}: {
  valore: unknown;
  tavolozza: Record<string, string>;
  onCambia: (v: unknown) => void;
}) {
  const { registro } = useAzioniStudio();
  const [aggiungendo, setAggiungendo] = useState(false);
  const filtri = (Array.isArray(valore) ? valore : []) as Record<string, unknown>[];
  const scrivi = (nuovi: Record<string, unknown>[]) => onCambia(nuovi.length === 0 ? null : nuovi);
  const cambia = (i: number, nome: string, v: unknown) =>
    scrivi(filtri.map((f, j) => (j === i ? { [nome]: v } : f)));

  return (
    <div className="pila-filtri">
      {filtri.map((filtro, i) => {
        const nome = Object.keys(filtro)[0] ?? "";
        const v = filtro[nome];
        return (
          <div key={i} className="un-filtro">
            <div className="testa-filtro">
              <code>{nome}</code>
              <button
                type="button"
                className="tasto-icona icon-btn"
                disabled={i === 0}
                aria-label={t("studio3.filters.up")}
                onClick={() => {
                  const nuovi = [...filtri];
                  const [preso] = nuovi.splice(i, 1);
                  if (preso !== undefined) nuovi.splice(i - 1, 0, preso);
                  scrivi(nuovi);
                }}
              >
                <Icona nome="i-chev-u" dim={11} />
              </button>
              <button
                type="button"
                className="tasto-icona icon-btn"
                aria-label={t("studio3.filters.remove", { nome })}
                onClick={() => scrivi(filtri.filter((_, j) => j !== i))}
              >
                <Icona nome="i-x" dim={11} />
              </button>
            </div>
            {nome === "blur" && (
              <ValoreLunghezza valore={v} min={0} max={40} passo={0.5} soloAssolute etichetta="blur" onCambia={(x) => x !== null && cambia(i, nome, x)} />
            )}
            {(nome === "brightness" || nome === "contrast" || nome === "saturate") && (
              <Cursore valore={v} min={0} max={3} passo={0.05} etichetta={nome} onCambia={(x) => x !== null && cambia(i, nome, x)} />
            )}
            {nome === "grayscale" && (
              <Cursore valore={v} min={0} max={1} passo={0.05} etichetta={nome} onCambia={(x) => x !== null && cambia(i, nome, x)} />
            )}
            {nome === "hueRotate" && (
              <Cursore valore={v} min={-360} max={360} passo={1} suffisso="°" etichetta={nome} onCambia={(x) => x !== null && cambia(i, nome, x)} />
            )}
            {nome === "dropShadow" && (
              <div className="ombra-filtro">
                {(["x", "y", "blur"] as const).map((campo) => (
                  <ValoreLunghezza
                    key={campo}
                    valore={oggetto(v)[campo]}
                    min={campo === "blur" ? 0 : -24}
                    max={24}
                    passo={0.5}
                    etichetta={`dropShadow ${campo}`}
                    onCambia={(x) => x !== null && cambia(i, nome, { ...oggetto(v), [campo]: x })}
                  />
                ))}
                <ValoreColore
                  valore={oggetto(v)["color"]}
                  tokens={registro.tokens}
                  tavolozza={tavolozza}
                  onCambia={(x) => x !== null && cambia(i, nome, { ...oggetto(v), color: x })}
                />
              </div>
            )}
          </div>
        );
      })}
      {aggiungendo ? (
        <div className="elenco-effetti">
          {FILTRI.map((nome) => (
            <button
              key={nome}
              type="button"
              className="un-widget"
              onClick={() => {
                scrivi([...filtri, { [nome]: FILTRO_NUOVO[nome] }]);
                setAggiungendo(false);
              }}
            >
              <code>{nome}</code>
            </button>
          ))}
        </div>
      ) : (
        <button
          type="button"
          className="aggiungi-livello"
          disabled={filtri.length >= 6}
          onClick={() => setAggiungendo(true)}
        >
          <Icona nome="i-plus" dim={13} />
          <span>{t("studio3.filters.add")}</span>
        </button>
      )}
    </div>
  );
}

/** La sfocatura di quel che sta dietro: il vetro. Costa, e lo dice. */
function Vetro({
  valore,
  budget,
  onCambia,
}: {
  valore: unknown;
  budget: number;
  onCambia: (v: unknown) => void;
}) {
  const { registro } = useAzioniStudio();
  const dentro = oggetto(valore);
  const acceso = dentro["effect"] === "blurBehind";
  const costo = acceso ? costoDi(dentro, registro.effects) : 0;
  return (
    <div className="sfoca-dietro">
      <ValoreLunghezza
        valore={acceso ? dentro["radius"] : null}
        min={0}
        max={64}
        passo={1}
        etichetta={t("studio3.backdrop.radius")}
        onCambia={(v) => onCambia(v === null ? null : { ...dentro, effect: "blurBehind", radius: v })}
      />
      {acceso && (
        <Cursore
          valore={dentro["saturate"]}
          min={0}
          max={400}
          passo={5}
          suffisso="%"
          etichetta={t("studio3.backdrop.saturate")}
          onCambia={(v) => {
            const { saturate: _via, ...resto } = dentro;
            onCambia(v === null ? resto : { ...resto, saturate: v });
          }}
        />
      )}
      <span className="peso" style={{ color: coloreCosto(costo, budget) }} title={t("studio3.backdrop.cost")}>
        {costo}
      </span>
    </div>
  );
}

// ── Stati ────────────────────────────────────────────────────────────────────

function Stati({ def }: { def: ParteRegistro }) {
  const { s, manda, registro, documento, scrivi } = useStudio();
  const radice = oggetto(documento === null ? {} : valoreIn(documento, [...radiceParti(s.tema), def.name]));
  const stati = oggetto(radice["states"]);
  const movimento = oggetto(oggetto(oggetto(documento?.["parts"])[def.name])["motion"]);
  return (
    <div className="stati-parte">
      <p className="nota-ispezione">{t("studio3.states.intro")}</p>
      {registro.stati.map((st) => {
        const dichiarato = oggetto(stati[st.name]);
        const quanti = Object.keys(dichiarato).length;
        const animato = typeof movimento[st.name] === "string";
        return (
          <div key={st.name} className="riga-stato" data-active={s.stato === st.name || undefined}>
            <button
              type="button"
              className="nome-stato"
              onClick={() => {
                manda({ tipo: "stato", stato: st.name });
                manda({ tipo: "imposta", campi: { ispettore: "stile", forza: true } });
              }}
            >
              <strong>{nomeStato(st.name)}</strong>
              <code className="selettore-stato">{st.selector}</code>
            </button>
            <span className="segni-stato">
              {quanti > 0 && <span className="chip-livello">{t("studio3.states.fields", { n: quanti })}</span>}
              {animato && (
                <span className="segno" title={t("studio3.layers.mark.animated")}>
                  <Icona nome="i-timeline" dim={11} />
                </span>
              )}
            </span>
            {quanti > 0 && (
              <button
                type="button"
                className="tasto-icona icon-btn"
                aria-label={t("studio3.states.clear", { stato: nomeStato(st.name) })}
                title={t("studio3.states.clear", { stato: nomeStato(st.name) })}
                onClick={() =>
                  scrivi(
                    [...radiceParti(s.tema), def.name, "states", st.name],
                    null,
                    t("studio3.step.stateClear", { stato: nomeStato(st.name), parte: def.name }),
                  )
                }
              >
                <Icona nome="i-x" dim={11} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ── Movimento ────────────────────────────────────────────────────────────────

function Movimento({ def }: { def: ParteRegistro }) {
  const { s, manda, registro, documento, scrivi } = useStudio();
  const radice = ["parts", def.name, "motion"];
  const movimento = oggetto(documento === null ? {} : valoreIn(documento, radice));
  const animazioni = oggetto(oggetto(documento?.["motion"])["animations"]);
  const curve = Object.entries(oggetto(oggetto(documento?.["motion"])["easings"])).map(([nome, curva]) => ({
    nome,
    curva,
  }));
  const inneschi = registro.inneschi.filter(
    (i) => (i.name !== "exit" || def.presenza) && (i.name !== "share" || def.condivisa),
  );
  const gruppi: readonly (readonly [string, typeof inneschi])[] = [
    [t("studio3.motion.lifecycle"), inneschi.filter((i) => !i.stato && ["enter", "exit", "share"].includes(i.name))],
    [t("studio3.motion.scroll"), inneschi.filter((i) => !i.stato && !["enter", "exit", "share"].includes(i.name))],
    [t("studio3.motion.states"), inneschi.filter((i) => i.stato)],
  ];

  const assegna = (innesco: string, nome: string | null) =>
    scrivi(
      [...radice, innesco],
      nome,
      t("studio3.step.motionAssign", { parte: def.name, innesco: nomeInnesco(innesco), nome: nome ?? "—" }),
    );

  /** Un'animazione nuova per un innesco: la pronta che gli si addice, o una dissolvenza. */
  const crea = (innesco: string, ciclo: boolean) => {
    const pronta =
      registro.animazioniPronte.find((p) => p.innesco === innesco) ??
      registro.animazioniPronte.find((p) => (ciclo ? p.innesco === "playing" : p.innesco === "enter"));
    let valore: unknown = {
      duration: "240ms",
      easing: "ease-out",
      frames: [
        { at: 0, opacity: 0 },
        { at: 100, opacity: 1 },
      ],
    };
    try {
      if (pronta !== undefined) valore = JSON.parse(pronta.valore);
    } catch {
      // L'esemplare viene dal nucleo; se non si legge, resta la dissolvenza.
    }
    const presi = Object.keys(animazioni);
    const base = `${def.name}-${innesco}`.toLowerCase().replace(/[^a-z0-9-]/g, "-");
    let nome = base;
    for (let n = 2; presi.includes(nome); n += 1) nome = `${base}-${n}`;
    scrivi(["motion", "animations", nome], valore, t("studio3.step.animationAdd", { nome }));
    assegna(innesco, nome);
    manda({ tipo: "scegli", selezione: { tipo: "animazione", nome } });
    manda({ tipo: "imposta", campi: { dock: "timeline" } });
  };

  const transizione = oggetto(movimento["transition"]);

  return (
    <div className="movimento-parte">
      {s.tema === "variante" && <p className="nota-ispezione">{t("studio3.motion.sameInBoth")}</p>}
      {gruppi.map(([titolo, elenco]) =>
        elenco.length === 0 ? null : (
          <Gruppo key={titolo} titolo={titolo} pieno={elenco.some((i) => typeof movimento[i.name] === "string")}>
            {elenco.map((innesco) => {
              const scelta = typeof movimento[innesco.name] === "string" ? String(movimento[innesco.name]) : "";
              return (
                <div key={innesco.name} className="riga-innesco">
                  <span className="che" title={tSe(`studio3.trigger.${innesco.name}.hint`, "")}>
                    {nomeInnesco(innesco.name)}
                  </span>
                  <select
                    className="field-input"
                    aria-label={t("studio3.motion.animationFor", { innesco: nomeInnesco(innesco.name) })}
                    value={scelta}
                    onChange={(e) => assegna(innesco.name, e.target.value === "" ? null : e.target.value)}
                  >
                    <option value="">{t("studio3.motion.none")}</option>
                    {Object.entries(animazioni).map(([nome, a]) => {
                      const infinita = oggetto(a)["iterations"] === "infinite";
                      return (
                        <option key={nome} value={nome} disabled={infinita && !innesco.ciclo}>
                          {infinita ? `${nome} · ∞` : nome}
                        </option>
                      );
                    })}
                  </select>
                  {scelta === "" ? (
                    <button
                      type="button"
                      className="tasto-icona icon-btn"
                      aria-label={t("studio3.motion.create")}
                      title={t("studio3.motion.create")}
                      onClick={() => crea(innesco.name, innesco.ciclo)}
                    >
                      <Icona nome="i-plus" dim={13} />
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="tasto-icona icon-btn"
                      aria-label={t("studio3.motion.openTimeline")}
                      title={t("studio3.motion.openTimeline")}
                      onClick={() => {
                        manda({ tipo: "scegli", selezione: { tipo: "animazione", nome: scelta } });
                        manda({ tipo: "imposta", campi: { dock: "timeline" } });
                      }}
                    >
                      <Icona nome="i-timeline" dim={13} />
                    </button>
                  )}
                </div>
              );
            })}
          </Gruppo>
        ),
      )}

      <Gruppo titolo={t("studio3.motion.more")} pieno={movimento["stagger"] !== undefined || movimento["transition"] !== undefined}>
        <Riga che={t("studio3.motion.stagger")}>
          <ValoreDurata
            valore={movimento["stagger"]}
            etichetta={t("studio3.motion.stagger")}
            onCambia={(v) =>
              scrivi([...radice, "stagger"], v, t("studio3.step.motionField", { campo: t("studio3.motion.stagger"), parte: def.name }))
            }
          />
        </Riga>
        <Riga che={t("studio3.motion.transition")}>
          <div className="transizione-parte">
            <ValoreDurata
              valore={transizione["duration"]}
              etichetta={t("studio3.motion.transition")}
              onCambia={(v) =>
                scrivi(
                  [...radice, "transition"],
                  v === null ? null : { ...transizione, duration: v },
                  t("studio3.step.motionField", { campo: t("studio3.motion.transition"), parte: def.name }),
                )
              }
            />
            {transizione["duration"] !== undefined && (
              <ValoreCurva
                valore={transizione["easing"]}
                etichetta={t("studio3.motion.easing")}
                curveSkin={curve}
                perNome
                onCambia={(v) => {
                  const { easing: _via, ...resto } = transizione;
                  scrivi(
                    [...radice, "transition"],
                    v === null ? resto : { ...resto, easing: v },
                    t("studio3.step.motionField", { campo: t("studio3.motion.easing"), parte: def.name }),
                  );
                }}
              />
            )}
          </div>
        </Riga>
      </Gruppo>
      <div className="azioni-variabili">
        <button type="button" className="pillola btn-ghost" onClick={() => manda({ tipo: "rigioca" })}>
          <Icona nome="i-repeat" dim={13} />
          {t("studio3.scene.replay")}
        </button>
      </div>
    </div>
  );
}

// ── Codice ───────────────────────────────────────────────────────────────────

/**
 * Il JSON di una sola parte, da modificare a mano.
 *
 * Il documento intero sta nel cassetto in basso; qui c'è il pezzo che si ha
 * in mano, perché è quello che si vuole ritoccare quando un controllo non
 * basta. Il completamento parte da `parts.<nome>`, quindi propone le chiavi
 * d'aspetto, gli stati, gli inneschi. Si applica da sé mezzo secondo dopo
 * l'ultima battuta, e solo se è JSON: un testo a metà non tocca il documento.
 */
function CodiceParte({ parte }: { parte: string }) {
  const { s, manda, documento, scrivi: scriviDoc } = useStudio();
  const percorso = [...radiceParti(s.tema), parte];
  const valore = documento === null ? undefined : valoreIn(documento, percorso);
  const scritto = valore === undefined ? "{}\n" : stampa(valore as Record<string, unknown>);
  const [testo, setTesto] = useState(scritto);
  const [errore, setErrore] = useState(false);
  const fonti = useFonti(percorso);
  useEffect(() => {
    setTesto(scritto);
    setErrore(false);
  }, [scritto]);

  useEffect(() => {
    if (testo === scritto) return;
    const attesa = window.setTimeout(() => {
      const letto = leggi(testo.trim() === "" ? "{}" : testo);
      if (letto === null) {
        setErrore(true);
        return;
      }
      setErrore(false);
      scriviDoc(percorso, Object.keys(letto).length === 0 ? null : letto, t("studio3.step.partCode", { parte }));
    }, 500);
    return () => window.clearTimeout(attesa);
    // `percorso` si ricompone a ogni disegno: conta la sua chiave.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [testo, scritto, parte, s.tema]);

  return (
    <div className="codice-parte">
      <EditorCodice
        testo={testo}
        onCambia={setTesto}
        onAnnulla={() => manda({ tipo: "annulla" })}
        onRipeti={() => manda({ tipo: "ripeti" })}
        etichetta={t("studio3.code.partAria", { parte })}
        fonti={fonti}
        classe="codice-frammento"
      />
      {errore && <p className="errore-codice">{t("studio3.code.notJson")}</p>}
      <p className="nota-ispezione">{t("studio3.code.partHint")}</p>
    </div>
  );
}
