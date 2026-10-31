/**
 * L'ispettore di tutto quel che non è una parte.
 *
 * Un token, un colore della tavolozza, un motivo, una curva, un'animazione, un
 * file del pacchetto, un carattere, un nodo dello scafale, il documento. Ogni
 * voce ha la stessa forma: una testata che dice che cos'è, i controlli del suo
 * tipo, e in fondo **chi la usa** — con «vai» su ogni riga, perché la domanda
 * che viene dopo «cambio questo colore?» è «dove si vedrà?».
 *
 * Il tema conta anche qui: guardando la variante, un token, un colore o un
 * motivo si scrivono in `variant`, e una voce che la variante non sovrascrive
 * lo dice e offre di farlo.
 */
import { useEffect, useRef, useState } from "react";

import { ConfermaGesto, type Domanda } from "../ConfermaGesto";
import { ipc, type NodoScafale } from "../ipc";
import { numero } from "../formato";
import { t, tSe } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { conNodo, nodoA, type Via } from "./albero";
import { useScriviAlbero } from "./AlberoScafale";
import { idDelFormato } from "./bozza";
import { useStudio } from "./contesto";
import {
  ColoreLetterale,
  ControlloPerTipo,
  Cursore,
  Parole,
  Riga,
  ValoreCurva,
} from "./controlli";
import { avvioProvino, risolutoriDi, versoAnimazione } from "./fotogrammi";
import { scafaleScrivibile } from "./negozio";
import { scrivi as stampa, valoreIn } from "./patch";
import { Manopole } from "./Pila";
import {
  chiUsaAnimazione,
  chiUsaColore,
  chiUsaCurva,
  chiUsaMotivo,
  chiUsaToken,
  rinomina,
  type Genere,
} from "./riferimenti";
import { problemaNomeFamiglia, usiDellaRisorsa } from "./risorse";
import { sentieroColore, sentieroMotivo, sentieroToken } from "./sentieri";
import { caratteriDi, immaginiDi, nomeRisorsa } from "./usaPacchetto";
import { useVai } from "./vai";
import { sorgentiPer } from "./sorgentiColore";
import { domandaTogliRisorsa, MiniCurva, quantoPesa } from "./Variabili";
import { CURSORE_PESO_VARIABILE, type Livello } from "./valori";
import { descrizioneOpzione, descrizionePreset, descrizioneToken, descrizioneWidget, nomeGruppo } from "./vocabolario";

const oggetto = (v: unknown): Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/** La testata di una voce. */
function TestaVoce({
  nome,
  genere,
  descrizione,
  children,
}: {
  nome: string;
  genere: string;
  descrizione?: string | undefined;
  children?: React.ReactNode;
}) {
  return (
    <header className="testa-ispezione">
      <div className="riga-testa">
        <code className="nome-parte">{nome}</code>
        <span className="gruppo-parte">{genere}</span>
      </div>
      {descrizione !== undefined && descrizione !== "" && <p className="descrizione">{descrizione}</p>}
      {children}
    </header>
  );
}

/** Chi usa una voce, con «vai» su ogni riga. */
function UsatoDa({
  percorsi,
  titolo,
  vuoto,
}: {
  percorsi: readonly string[];
  /** Per chi non si usa solo dal documento: un token lo legge anche l'app. */
  titolo?: string;
  vuoto?: string;
}) {
  const vai = useVai();
  return (
    <section className="gruppo-ispezione usato-da">
      <h4 className="titolo-gruppo-ispezione">{titolo ?? t("studio3.usedBy.title", { n: percorsi.length })}</h4>
      {percorsi.length === 0 ? (
        <p className="nota-ispezione">{vuoto ?? t("studio3.usedBy.none")}</p>
      ) : (
        <ul className="elenco-usi">
          {percorsi.slice(0, 40).map((p) => (
            <li key={p}>
              <button type="button" className="voce-uso" onClick={() => vai(p)}>
                <code>{p}</code>
                <Icona nome="i-chev-r" dim={11} />
              </button>
            </li>
          ))}
          {percorsi.length > 40 && <li className="nota-ispezione">{t("studio3.usedBy.more", { n: percorsi.length - 40 })}</li>}
        </ul>
      )}
    </section>
  );
}

/** Rinominare una voce, coi suoi richiami: un campo e un tasto. */
function Rinomina({
  genere,
  nome,
  presi,
  onFatto,
}: {
  genere: Genere;
  nome: string;
  presi: readonly string[];
  onFatto: (nuovo: string) => void;
}) {
  const { documento, scriviTesto } = useStudio();
  const [scritto, setScritto] = useState(nome);
  useEffect(() => setScritto(nome), [nome]);
  const valido = /^[a-z0-9][a-z0-9-]*$/.test(scritto);
  const preso = scritto !== nome && presi.includes(scritto);
  const applica = () => {
    if (!valido || preso || scritto === nome || documento === null) return;
    const nuovo = rinomina(documento, genere, nome, scritto);
    if (nuovo === null) return;
    scriviTesto(stampa(nuovo), t("studio3.step.rename", { da: nome, a: scritto }), `rinomina:${nome}`);
    onFatto(scritto);
  };
  return (
    <Riga che={t("studio3.rename")}>
      <div className="rinomina">
        <input
          className="field-input"
          type="text"
          spellCheck={false}
          aria-label={t("studio3.rename")}
          aria-invalid={!valido || preso || undefined}
          value={scritto}
          onChange={(e) => setScritto(e.target.value.toLowerCase())}
          onBlur={applica}
          onKeyDown={(e) => {
            if (e.key === "Enter") applica();
            if (e.key === "Escape") setScritto(nome);
          }}
        />
        {(preso || !valido) && (
          <span className="errore-codice">{preso ? t("studio3.rename.taken") : t("studio3.rename.alphabet")}</span>
        )}
      </div>
    </Riga>
  );
}

/** «Ereditato dalla base» e «torna alla base», per le voci che la variante sovrascrive. */
function Variante({
  proprio,
  base,
  percorso,
  etichetta,
}: {
  proprio: unknown;
  base: unknown;
  percorso: string[];
  etichetta: string;
}) {
  const { s, scrivi } = useStudio();
  if (s.tema !== "variante") return null;
  return proprio === undefined ? (
    <div className="dove-scrivi" data-variante>
      <span>{t("studio3.variant.inherited")}</span>
      {base !== undefined && (
        <button
          type="button"
          className="pillola btn-ghost minuto"
          onClick={() => scrivi(percorso, base, t("studio3.step.variantOverride", { cosa: etichetta }))}
        >
          {t("studio3.variant.override")}
        </button>
      )}
    </div>
  ) : (
    <div className="dove-scrivi" data-variante>
      <span>{t("studio3.variant.own")}</span>
      <button
        type="button"
        className="pillola btn-ghost minuto"
        onClick={() => scrivi(percorso, null, t("studio3.step.variantReset", { cosa: etichetta }))}
      >
        {t("studio3.variant.reset")}
      </button>
    </div>
  );
}

/** Un valore che i controlli non sanno leggere: lo si modifica come JSON. */
function CampoJson({ valore, onCambia }: { valore: unknown; onCambia: (v: unknown) => void }) {
  const scritto = JSON.stringify(valore, null, 2) ?? "";
  const [testo, setTesto] = useState(scritto);
  const [rotto, setRotto] = useState(false);
  useEffect(() => {
    setTesto(scritto);
    setRotto(false);
  }, [scritto]);
  return (
    <textarea
      className="field-input codice-frammento corto"
      spellCheck={false}
      aria-label={t("studio3.code.value")}
      aria-invalid={rotto || undefined}
      value={testo}
      onChange={(e) => setTesto(e.target.value)}
      onBlur={() => {
        if (testo === scritto) return;
        try {
          onCambia(JSON.parse(testo));
          setRotto(false);
        } catch {
          setRotto(true);
        }
      }}
    />
  );
}

// ── un token ─────────────────────────────────────────────────────────────────

/** I controlli di `controlli.tsx` sanno leggere questo valore? */
function leggibile(kind: string, valore: unknown): boolean {
  if (valore === undefined || valore === null) return true;
  if (typeof valore === "string" || typeof valore === "number" || Array.isArray(valore)) return true;
  const d = oggetto(valore);
  if (kind === "color") return "$token" in d || "$palette" in d || "$source" in d;
  if (kind === "shadow") return Array.isArray(d["layers"]);
  if (kind === "easing") return typeof d["kind"] === "string";
  return false;
}

export function IspezioneToken({ id }: { id: string }) {
  const { s, registro, documento, scrivi, scriviMolti } = useStudio();
  const def = registro.tokens.find((tk) => tk.id === id);
  if (def === undefined) return <p className="niente-ispettore">{t("studio3.inspector.unknown", { nome: id })}</p>;
  const percorso = sentieroToken(id, s.tema);
  const proprio = documento === null ? undefined : valoreIn(documento, percorso);
  const base = documento === null ? undefined : valoreIn(documento, ["tokens", id]);
  const valore = proprio ?? (s.tema === "variante" ? base : undefined);
  const tavolozza = oggetto(documento?.["palette"]) as Record<string, string>;
  const curve = Object.entries(oggetto(oggetto(documento?.["motion"])["easings"])).map(([nome, curva]) => ({ nome, curva }));
  const pronti = registro.presets.filter((p) => p.group === def.group);
  const contrasti = (s.valido?.contrasti ?? []).filter((c) => c.davanti === id || c.dietro === id);
  const richiami = documento === null ? [] : chiUsaToken(documento, id);
  const scriviValore = (v: unknown) =>
    scrivi(percorso, v, t("studio3.step.token", { id, valore: typeof v === "string" ? v : "…" }));

  return (
    <div className="ispezione-voce">
      <TestaVoce nome={id} genere={nomeGruppo(def.group)} descrizione={descrizioneToken(id, def.description)}>
        <div className="chip-parte">
          <code className="chip-livello">{def.css}</code>
          <span className="chip-livello">{tSe(`studio.kind.${def.kind}`, def.kind)}</span>
          {def.required && <span className="chip-livello">{t("studio3.token.required")}</span>}
        </div>
      </TestaVoce>
      <Variante proprio={proprio} base={base} percorso={percorso} etichetta={id} />
      <section className="gruppo-ispezione">
        {leggibile(def.kind, valore) ? (
          <ControlloPerTipo
            tipo={def.kind}
            valore={valore ?? null}
            token={def}
            tokens={registro.tokens}
            tavolozza={tavolozza}
            // L'accento non segue la copertina con `$source`: vedi `sorgentiPer`.
            sorgenti={def.kind === "color" ? sorgentiPer(id, registro.sorgenti) : []}
            curveSkin={curve}
            onCambia={scriviValore}
          />
        ) : (
          <CampoJson valore={valore} onCambia={scriviValore} />
        )}
        {proprio !== undefined && (
          <button type="button" className="pillola btn-ghost minuto" onClick={() => scriviValore(null)}>
            <Icona nome="i-x" dim={11} />
            {t("studio3.token.undeclare")}
          </button>
        )}
      </section>
      {pronti.length > 0 && (
        <section className="gruppo-ispezione">
          <h4 className="titolo-gruppo-ispezione">{t("studio3.token.presets")}</h4>
          <div className="preset-token">
            {pronti.map((p) => (
              <button
                key={p.id}
                type="button"
                className="pillola btn-ghost"
                onClick={() => {
                  const operazioni = p.valori.flatMap(([token, frammento]) => {
                    try {
                      return [{ togli: false, percorso: sentieroToken(token, s.tema), valore: JSON.parse(frammento) as unknown }];
                    } catch {
                      return [];
                    }
                  });
                  scriviMolti(operazioni, t("studio3.step.preset", { nome: descrizionePreset(p.id, p.nome) }));
                }}
              >
                {descrizionePreset(p.id, p.nome)}
              </button>
            ))}
          </div>
        </section>
      )}
      {contrasti.length > 0 && (
        <section className="gruppo-ispezione">
          <h4 className="titolo-gruppo-ispezione">{t("studio3.token.contrast")}</h4>
          {contrasti.map((c) => (
            <div key={`${c.davanti}/${c.dietro}`} className="riga-contrasto" data-passa={c.passa || undefined}>
              <code>{c.davanti === id ? c.dietro : c.davanti}</code>
              <span>{numero(c.base, 2)}:1</span>
              {c.variante !== null && <span>{numero(c.variante, 2)}:1</span>}
              <Icona nome={c.passa ? "i-check" : "i-alert"} dim={12} />
            </div>
          ))}
        </section>
      )}
      <UsatoDa
        percorsi={richiami}
        titolo={t("studio3.usedBy.tokenTitle", { n: richiami.length })}
        vuoto={t("studio3.usedBy.tokenNone", { css: def.css })}
      />
    </div>
  );
}

// ── un colore della tavolozza ───────────────────────────────────────────────

export function IspezioneColore({ nome }: { nome: string }) {
  const { s, manda, documento, scrivi, scriviMolti } = useStudio();
  const percorso = sentieroColore(nome, s.tema);
  const proprio = documento === null ? undefined : valoreIn(documento, percorso);
  const base = documento === null ? undefined : valoreIn(documento, ["palette", nome]);
  const valore = proprio ?? base;
  const usi = documento === null ? [] : chiUsaColore(documento, nome);
  return (
    <div className="ispezione-voce">
      <TestaVoce nome={nome} genere={t("studio3.kind.color")} />
      <Rinomina
        genere="colore"
        nome={nome}
        presi={Object.keys(oggetto(documento?.["palette"]))}
        onFatto={(nuovo) => manda({ tipo: "scegli", selezione: { tipo: "colore", nome: nuovo } })}
      />
      <Variante proprio={proprio} base={base} percorso={percorso} etichetta={nome} />
      <section className="gruppo-ispezione">
        <ColoreLetterale
          valore={valore}
          etichetta={nome}
          onCambia={(v) => scrivi(percorso, v, t("studio3.step.color", { nome, valore: v }))}
        />
      </section>
      <UsatoDa percorsi={usi} />
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        title={usi.length > 0 ? t("studio3.delete.used", { n: usi.length }) : undefined}
        onClick={() => {
          scriviMolti(
            [
              { togli: true, percorso: ["palette", nome], valore: null },
              { togli: true, percorso: ["variant", "palette", nome], valore: null },
            ],
            t("studio3.step.delete", { nome }),
          );
          manda({ tipo: "scegli", selezione: null });
        }}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio3.delete")}
      </button>
    </div>
  );
}

// ── un motivo ────────────────────────────────────────────────────────────────

export function IspezioneMotivo({ nome }: { nome: string }) {
  const { s, manda, registro, documento, scrivi, scriviMolti, file } = useStudio();
  const percorso = sentieroMotivo(nome, s.tema);
  const proprio = documento === null ? undefined : valoreIn(documento, percorso);
  const base = documento === null ? undefined : valoreIn(documento, ["patterns", nome]);
  const effetto = oggetto(proprio ?? base) as Livello;
  const def = registro.effects.find((e) => e.name === effetto["effect"]);
  const tavolozza = oggetto(documento?.["palette"]) as Record<string, string>;
  const usi = documento === null ? [] : chiUsaMotivo(documento, nome);
  return (
    <div className="ispezione-voce">
      <TestaVoce
        nome={nome}
        genere={t("studio3.kind.pattern")}
        descrizione={def === undefined ? undefined : t("studio3.pattern.effect", { effetto: def.name, costo: def.cost })}
      />
      <Rinomina
        genere="motivo"
        nome={nome}
        presi={Object.keys(oggetto(documento?.["patterns"]))}
        onFatto={(nuovo) => manda({ tipo: "scegli", selezione: { tipo: "motivo", nome: nuovo } })}
      />
      <Variante proprio={proprio} base={base} percorso={percorso} etichetta={nome} />
      <section className="gruppo-ispezione">
        {def === undefined ? (
          <CampoJson valore={effetto} onCambia={(v) => scrivi(percorso, v, t("studio3.step.pattern", { nome }))} />
        ) : (
          <Manopole
            livello={effetto}
            params={def.params}
            tokens={registro.tokens}
            tavolozza={tavolozza}
            immagini={immaginiDi(file)}
            onCambia={(v) => scrivi(percorso, v, t("studio3.step.pattern", { nome }))}
          />
        )}
      </section>
      <UsatoDa percorsi={usi} />
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        title={usi.length > 0 ? t("studio3.delete.used", { n: usi.length }) : undefined}
        onClick={() => {
          scriviMolti(
            [
              { togli: true, percorso: ["patterns", nome], valore: null },
              { togli: true, percorso: ["variant", "patterns", nome], valore: null },
            ],
            t("studio3.step.delete", { nome }),
          );
          manda({ tipo: "scegli", selezione: null });
        }}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio3.delete")}
      </button>
    </div>
  );
}

// ── una curva ────────────────────────────────────────────────────────────────

export function IspezioneCurva({ nome }: { nome: string }) {
  const { manda, documento, scrivi } = useStudio();
  const percorso = ["motion", "easings", nome];
  const curva = documento === null ? undefined : valoreIn(documento, percorso);
  const usi = documento === null ? [] : chiUsaCurva(documento, nome);
  return (
    <div className="ispezione-voce">
      <TestaVoce nome={nome} genere={t("studio3.kind.easing")} descrizione={t("studio3.easing.hint")} />
      <Rinomina
        genere="curva"
        nome={nome}
        presi={Object.keys(oggetto(oggetto(documento?.["motion"])["easings"]))}
        onFatto={(nuovo) => manda({ tipo: "scegli", selezione: { tipo: "curva", nome: nuovo } })}
      />
      <section className="gruppo-ispezione curva-grande">
        <MiniCurva curva={curva} />
        <ValoreCurva
          valore={curva ?? null}
          etichetta={nome}
          onCambia={(v) => v !== null && scrivi(percorso, v, t("studio3.step.easing", { nome }))}
        />
      </section>
      <UsatoDa percorsi={usi} />
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        title={usi.length > 0 ? t("studio3.delete.used", { n: usi.length }) : undefined}
        onClick={() => {
          scrivi(percorso, null, t("studio3.step.delete", { nome }));
          manda({ tipo: "scegli", selezione: null });
        }}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio3.delete")}
      </button>
    </div>
  );
}

// ── un'animazione ────────────────────────────────────────────────────────────

/** Le parole dell'origine di un'animazione. */
const ORIGINI = [
  "center",
  "top",
  "bottom",
  "left",
  "right",
  "top left",
  "top right",
  "bottom left",
  "bottom right",
] as const;

/** Una durata lunga, fino al tetto del registro: un'animazione può durare secondi. */
function DurataLunga({
  valore,
  max,
  etichetta,
  facoltativa,
  onCambia,
}: {
  valore: unknown;
  max: number;
  etichetta: string;
  facoltativa: boolean;
  onCambia: (v: unknown) => void;
}) {
  const n = typeof valore === "string" ? Number.parseFloat(valore) * (/ms\s*$/.test(valore) ? 1 : 1000) : Number.NaN;
  const dichiarata = Number.isFinite(n);
  return (
    <div className="cursore-studio">
      <input
        type="range"
        min={0}
        max={max}
        step={10}
        aria-label={etichetta}
        value={dichiarata ? n : 0}
        onChange={(e) => onCambia(`${e.target.value}ms`)}
      />
      <span className="lettura">{dichiarata ? `${Math.round(n)}ms` : "—"}</span>
      {facoltativa && (
        <button
          type="button"
          className="sgancia icon-btn"
          aria-label={t("studio.ctl.remove", { cosa: etichetta })}
          disabled={!dichiarata}
          onClick={() => onCambia(null)}
        >
          <Icona nome="i-x" dim={11} />
        </button>
      )}
    </div>
  );
}

/** Un'anteprima dell'animazione su un riquadro, con la Web Animations API. */
export function Provino({ nome, animazione }: { nome: string; animazione: Record<string, unknown> }) {
  const { s, registro, documento } = useStudio();
  const riquadro = useRef<HTMLDivElement>(null);
  const [giro, setGiro] = useState(0);
  /** Il giro già suonato: una partenza con lo stesso giro non l'ha chiesta ▶. */
  const giroVisto = useRef(0);
  const firma = JSON.stringify(animazione);
  useEffect(() => {
    const nodo = riquadro.current;
    if (nodo === null) return;
    const { fotogrammi, opzioni } = versoAnimazione(
      animazione,
      risolutoriDi(documento, s.tema === "variante", registro.tokens),
    );
    // «Riduci movimento», di Rymembra o del sistema: vedi `avvioProvino`.
    const ridotto =
      document.documentElement.dataset.motionUtente === "ridotto" ||
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const avvio = avvioProvino(ridotto, giro !== giroVisto.current);
    giroVisto.current = giro;
    let corsa: Animation | null = null;
    try {
      corsa = nodo.animate(
        fotogrammi,
        avvio === "suona" ? opzioni : { ...opzioni, iterations: 1, fill: "forwards" },
      );
      if (avvio === "fermo") corsa.finish();
    } catch {
      // Un fotogramma che il motore non legge — un colore a metà mentre lo si
      // scrive — non ferma l'ispettore: il provino resta fermo.
    }
    return () => corsa?.cancel();
    // `firma` riassume l'animazione: un oggetto nuovo con gli stessi valori
    // non deve far ripartire il provino a ogni disegno.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firma, giro, documento, s.tema, registro.tokens]);
  return (
    <div className="provino-animazione">
      <div className="palco-provino">
        <div ref={riquadro} className="riquadro-provino" aria-hidden="true">
          <span />
        </div>
      </div>
      <button
        type="button"
        className="tasto-icona icon-btn"
        aria-label={t("studio3.animation.play", { nome })}
        title={t("studio3.animation.play", { nome })}
        onClick={() => setGiro((g) => g + 1)}
      >
        <Icona nome="i-play" dim={13} />
      </button>
    </div>
  );
}

export function IspezioneAnimazione({ nome }: { nome: string }) {
  const { manda, registro, documento, scrivi } = useStudio();
  const percorso = ["motion", "animations", nome];
  const animazione = oggetto(documento === null ? undefined : valoreIn(documento, percorso));
  const curve = Object.entries(oggetto(oggetto(documento?.["motion"])["easings"])).map(([n, curva]) => ({ nome: n, curva }));
  const usi = documento === null ? [] : chiUsaAnimazione(documento, nome);
  const campo = (chiave: string, v: unknown) => {
    const nuovo: Record<string, unknown> = { ...animazione, [chiave]: v ?? undefined };
    scrivi(
      percorso,
      Object.fromEntries(Object.entries(nuovo).filter(([, x]) => x !== undefined)),
      t("studio3.step.animationField", { nome, campo: tSe(`studio3.anim.${chiave}`, chiave) }),
    );
  };
  const infinita = animazione["iterations"] === "infinite";
  const fotogrammi = Array.isArray(animazione["frames"]) ? animazione["frames"].length : 0;

  return (
    <div className="ispezione-voce">
      <TestaVoce nome={nome} genere={t("studio3.kind.animation")} />
      <Provino nome={nome} animazione={animazione} />
      <Rinomina
        genere="animazione"
        nome={nome}
        presi={Object.keys(oggetto(oggetto(documento?.["motion"])["animations"]))}
        onFatto={(nuovo) => manda({ tipo: "scegli", selezione: { tipo: "animazione", nome: nuovo } })}
      />
      <section className="gruppo-ispezione">
        <Riga che={t("studio3.anim.duration")}>
          <DurataLunga
            valore={animazione["duration"]}
            max={registro.maxDurataMs}
            etichetta={t("studio3.anim.duration")}
            facoltativa={false}
            onCambia={(v) => campo("duration", v)}
          />
        </Riga>
        <Riga che={t("studio3.anim.delay")}>
          <DurataLunga
            valore={animazione["delay"]}
            max={registro.maxRitardoMs}
            etichetta={t("studio3.anim.delay")}
            facoltativa
            onCambia={(v) => campo("delay", v)}
          />
        </Riga>
        <Riga che={t("studio3.anim.easing")}>
          <ValoreCurva
            valore={animazione["easing"] ?? null}
            etichetta={t("studio3.anim.easing")}
            curveSkin={curve}
            perNome
            onCambia={(v) => campo("easing", v)}
          />
        </Riga>
        <Riga che={t("studio3.anim.iterations")}>
          <div className="iterazioni">
            <label className="spunta">
              <input
                type="checkbox"
                checked={infinita}
                onChange={(e) => campo("iterations", e.target.checked ? "infinite" : null)}
              />
              <span>∞</span>
            </label>
            {!infinita && (
              <Cursore
                valore={animazione["iterations"] ?? 1}
                min={1}
                max={registro.maxIterazioni}
                passo={1}
                etichetta={t("studio3.anim.iterations")}
                onCambia={(v) => campo("iterations", v === 1 ? null : v)}
              />
            )}
          </div>
        </Riga>
        {infinita && <p className="nota-ispezione">{t("studio3.anim.infiniteHint")}</p>}
        <Riga che={t("studio3.anim.direction")}>
          <select
            className="field-input"
            aria-label={t("studio3.anim.direction")}
            value={typeof animazione["direction"] === "string" ? animazione["direction"] : ""}
            onChange={(e) => campo("direction", e.target.value === "" ? null : e.target.value)}
          >
            <option value="">{t("studio.ctl.undeclared")}</option>
            {registro.versi.map((v) => (
              <option key={v} value={v}>
                {tSe(`studio3.anim.direction.${v}`, v)}
              </option>
            ))}
          </select>
        </Riga>
        <Riga che={t("studio3.anim.fill")}>
          <Parole
            valore={animazione["fill"]}
            etichetta={t("studio3.anim.fill")}
            onCambia={(v) => campo("fill", v)}
            voci={registro.riempimenti.map((v) => ({ chiave: v, etichetta: v }))}
          />
        </Riga>
        <Riga che={t("studio3.anim.origin")}>
          <select
            className="field-input"
            aria-label={t("studio3.anim.origin")}
            value={typeof animazione["origin"] === "string" ? animazione["origin"] : ""}
            onChange={(e) => campo("origin", e.target.value === "" ? null : e.target.value)}
          >
            <option value="">{t("studio.ctl.undeclared.f")}</option>
            {ORIGINI.map((o) => (
              <option key={o} value={o}>
                {o}
              </option>
            ))}
          </select>
        </Riga>
      </section>
      <section className="gruppo-ispezione">
        <h4 className="titolo-gruppo-ispezione">{t("studio3.anim.frames", { n: fotogrammi })}</h4>
        <button
          type="button"
          className="pillola btn-ghost"
          onClick={() => manda({ tipo: "imposta", campi: { dock: "timeline" } })}
        >
          <Icona nome="i-timeline" dim={13} />
          {t("studio3.anim.openTimeline")}
        </button>
      </section>
      <UsatoDa percorsi={usi} />
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        title={usi.length > 0 ? t("studio3.delete.used", { n: usi.length }) : undefined}
        onClick={() => {
          scrivi(percorso, null, t("studio3.step.delete", { nome }));
          manda({ tipo: "scegli", selezione: null });
        }}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio3.delete")}
      </button>
    </div>
  );
}

// ── un file del pacchetto ────────────────────────────────────────────────────

/** L'indirizzo di una risorsa della bozza, come lo scrive il compilatore. */
export function indirizzoBozza(id: string, nome: string): string {
  return `http://rymembra-skin.localhost/bozza/${encodeURIComponent(id)}/${encodeURIComponent(nome)}`;
}

/** Un carattere della bozza caricato per un campione; il nome della famiglia, o `null`. */
function useCarattere(id: string, nome: string | null): string | null {
  const [pronto, setPronto] = useState<string | null>(null);
  useEffect(() => {
    if (nome === null) return;
    let vivo = true;
    const famiglia = `studio-campione-${nome.replace(/[^a-z0-9]/gi, "-")}`;
    const faccia = new FontFace(famiglia, `url(${indirizzoBozza(id, nome)})`);
    faccia
      .load()
      .then((caricata) => {
        document.fonts.add(caricata);
        if (vivo) setPronto(famiglia);
      })
      .catch(() => {
        if (vivo) setPronto(null);
      });
    return () => {
      vivo = false;
      document.fonts.delete(faccia);
    };
  }, [id, nome]);
  return pronto;
}

export function IspezioneRisorsa({ nome }: { nome: string }) {
  const { s, manda, documento, file, rileggiFile, onErrore } = useStudio();
  const voce = file.find((v) => nomeRisorsa(v) === nome);
  const carattere = nome.endsWith(".woff2");
  const famiglia = useCarattere(s.id, carattere ? nome : null);
  const usi = usiDellaRisorsa(documento, nome);
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  // Il file sparisce dal disco e non torna: si chiede prima (vedi
  // `domandaTogliRisorsa`), e le dichiarazioni dei caratteri si tolgono da
  // tutta la storia, senza un passo da annullare (`risorse.ts`).
  const togli = () => {
    ipc
      .studioRisorsaTogli(s.id, nome)
      .then(() => {
        rileggiFile();
        manda({ tipo: "risorsaTolta", nome });
        manda({ tipo: "scegli", selezione: null });
      })
      .catch(onErrore);
  };

  return (
    <div className="ispezione-voce">
      <TestaVoce nome={nome} genere={carattere ? t("studio3.kind.fontFile") : t("studio3.kind.image")}>
        {voce !== undefined && <span className="chip-livello">{quantoPesa(voce.byte)}</span>}
      </TestaVoce>
      <section className="gruppo-ispezione anteprima-risorsa">
        {carattere ? (
          <p className="campione-carattere" style={famiglia === null ? undefined : { fontFamily: `'${famiglia}'` }}>
            {t("studio3.font.sample")}
          </p>
        ) : (
          <img src={indirizzoBozza(s.id, nome)} alt={nome} />
        )}
      </section>
      <UsatoDa percorsi={usi} />
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        onClick={() => setDomanda(domandaTogliRisorsa(nome, usi, togli))}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio3.vars.files.remove", { nome })}
      </button>
      {domanda !== null && <ConfermaGesto domanda={domanda} onChiudi={() => setDomanda(null)} />}
    </div>
  );
}

// ── un carattere ─────────────────────────────────────────────────────────────

/** I token delle famiglie, e che cosa vuol dire usarne una lì. */
const PILE = [
  ["font.sans", "studio3.font.useSans"],
  ["font.display", "studio3.font.useDisplay"],
  ["font.mono", "studio3.font.useMono"],
] as const;

export function IspezioneCarattere({ nome }: { nome: string }) {
  const { s, manda, documento, file, scrivi, scriviMolti, scriviTesto } = useStudio();
  const percorso = ["fonts", nome];
  const dichiarato = oggetto(documento === null ? undefined : valoreIn(documento, percorso));
  const src = typeof dichiarato["src"] === "string" ? dichiarato["src"] : null;
  const famiglia = useCarattere(s.id, src);
  const peso = dichiarato["weight"];
  const variabile = Array.isArray(peso);
  const [scritto, setScritto] = useState(nome);
  useEffect(() => setScritto(nome), [nome]);
  const tokens = oggetto(documento?.["tokens"]);
  const campo = (chiave: string, v: unknown) => {
    const nuovo: Record<string, unknown> = { ...dichiarato, [chiave]: v ?? undefined };
    scrivi(
      percorso,
      Object.fromEntries(Object.entries(nuovo).filter(([, x]) => x !== undefined)),
      t("studio3.step.font", { nome }),
    );
  };

  // Un nome che non si può usare si dice accanto al campo, come in `Rinomina`.
  const problema = problemaNomeFamiglia(scritto, nome, Object.keys(oggetto(documento?.["fonts"])));
  const rinomina = () => {
    const nuovo = scritto.trim();
    if (documento === null || nuovo === nome || problema !== null) return;
    // La famiglia è richiamata per nome dalle pile di caratteri: si rinomina
    // con loro, nella base e nella variante.
    const copia = JSON.parse(JSON.stringify(documento)) as Record<string, unknown>;
    copia["fonts"] = Object.fromEntries(
      Object.entries(oggetto(copia["fonts"])).map(([k, v]) => [k === nome ? nuovo : k, v]),
    );
    for (const sezione of [copia["tokens"], oggetto(copia["variant"])["tokens"]]) {
      if (sezione === null || typeof sezione !== "object") continue;
      for (const [k, v] of Object.entries(sezione as Record<string, unknown>)) {
        if (Array.isArray(v)) (sezione as Record<string, unknown>)[k] = v.map((f) => (f === nome ? nuovo : f));
      }
    }
    scriviTesto(stampa(copia), t("studio3.step.rename", { da: nome, a: nuovo }), `rinomina:${nome}`);
    manda({ tipo: "scegli", selezione: { tipo: "carattere", nome: nuovo } });
  };

  return (
    <div className="ispezione-voce">
      <TestaVoce nome={nome} genere={t("studio3.kind.font")} />
      <section className="gruppo-ispezione anteprima-risorsa">
        <p className="campione-carattere" style={famiglia === null ? undefined : { fontFamily: `'${famiglia}'` }}>
          {t("studio3.font.sample")}
        </p>
      </section>
      <Riga che={t("studio3.rename")}>
        <div className="rinomina">
          <input
            className="field-input"
            type="text"
            spellCheck={false}
            aria-label={t("studio3.rename")}
            aria-invalid={problema !== null || undefined}
            value={scritto}
            onChange={(e) => setScritto(e.target.value)}
            onBlur={rinomina}
            onKeyDown={(e) => {
              if (e.key === "Enter") rinomina();
              if (e.key === "Escape") setScritto(nome);
            }}
          />
          {problema !== null && (
            <span className="errore-codice">
              {problema === "preso" ? t("studio3.rename.taken") : t("studio3.font.rename.alphabet")}
            </span>
          )}
        </div>
      </Riga>
      <section className="gruppo-ispezione">
        <Riga che={t("studio3.font.file")}>
          <select
            className="field-input"
            aria-label={t("studio3.font.file")}
            value={src ?? ""}
            onChange={(e) => campo("src", e.target.value)}
          >
            {src === null && <option value="">—</option>}
            {caratteriDi(file).map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
        </Riga>
        <Riga che={t("studio3.font.weight")}>
          <div className="peso-carattere">
            <Segmentato<"fisso" | "variabile">
              etichetta={t("studio3.font.weight")}
              classe="minuto denso"
              scelta={variabile ? "variabile" : "fisso"}
              onScegli={(forma) =>
                campo("weight", forma === "variabile" ? [100, 900] : typeof peso === "number" ? peso : 400)
              }
              voci={[
                { chiave: "fisso", etichetta: t("studio3.font.weight.fixed") },
                { chiave: "variabile", etichetta: t("studio3.font.weight.range") },
              ]}
            />
            {variabile ? (
              <>
                <Cursore
                  valore={(peso as unknown[])[0]}
                  min={CURSORE_PESO_VARIABILE.min}
                  max={CURSORE_PESO_VARIABILE.max}
                  passo={CURSORE_PESO_VARIABILE.passo}
                  etichetta={t("studio3.font.weight.min")}
                  onCambia={(v) => v !== null && campo("weight", [v, (peso as unknown[])[1]])}
                />
                <Cursore
                  valore={(peso as unknown[])[1]}
                  min={CURSORE_PESO_VARIABILE.min}
                  max={CURSORE_PESO_VARIABILE.max}
                  passo={CURSORE_PESO_VARIABILE.passo}
                  etichetta={t("studio3.font.weight.max")}
                  onCambia={(v) => v !== null && campo("weight", [(peso as unknown[])[0], v])}
                />
              </>
            ) : (
              <Cursore
                valore={peso ?? 400}
                min={100}
                max={900}
                passo={50}
                etichetta={t("studio3.font.weight")}
                onCambia={(v) => campo("weight", v)}
              />
            )}
          </div>
        </Riga>
        <Riga che={t("studio3.font.style")}>
          <Parole
            valore={dichiarato["style"]}
            etichetta={t("studio3.font.style")}
            onCambia={(v) => campo("style", v)}
            voci={[
              { chiave: "normal", etichetta: "Aa" },
              { chiave: "italic", etichetta: "Aa ⁄" },
            ]}
          />
        </Riga>
      </section>
      <section className="gruppo-ispezione">
        <h4 className="titolo-gruppo-ispezione">{t("studio3.font.useFor")}</h4>
        <div className="preset-token">
          {PILE.map(([token, chiave]) => {
            const pila = Array.isArray(tokens[token]) ? (tokens[token] as unknown[]).map(String) : [];
            const primo = pila[0] === nome;
            return (
              <button
                key={token}
                type="button"
                className="pillola btn-ghost"
                aria-pressed={primo}
                onClick={() =>
                  scriviMolti(
                    [
                      {
                        togli: false,
                        percorso: ["tokens", token],
                        valore: primo ? pila.filter((f) => f !== nome) : [nome, ...pila.filter((f) => f !== nome)],
                      },
                    ],
                    t("studio3.step.fontUse", { nome, token }),
                  )
                }
              >
                {t(chiave)}
              </button>
            );
          })}
        </div>
      </section>
      <UsatoDa
        percorsi={Object.entries(tokens)
          .filter(([, v]) => Array.isArray(v) && v.includes(nome))
          .map(([k]) => `tokens.${k}`)}
      />
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        onClick={() => {
          scrivi(percorso, null, t("studio3.step.delete", { nome }));
          manda({ tipo: "scegli", selezione: null });
        }}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio3.delete")}
      </button>
    </div>
  );
}

// ── un nodo dello scafale ───────────────────────────────────────────────────

export function IspezioneNodo({ via }: { via: Via }) {
  const { s, registro } = useStudio();
  const scriviAlbero = useScriviAlbero();
  const albero = s.valido?.layout?.shell ?? null;
  const nodo = albero === null ? null : nodoA(albero, via);
  const bloccato = s.esito !== null && s.esito.errori.length > 0;
  if (albero === null || nodo === null) {
    return <p className="niente-ispettore">{t("studio3.shell.pickNode")}</p>;
  }
  const def = registro.widgets.find((w) => w.name === nodo.name);
  const cambia = (campo: keyof NodoScafale, valore: unknown) =>
    scriviAlbero(
      conNodo(albero, via, { ...nodo, [campo]: valore }),
      t("studio3.step.node", { nome: nodo.name, campo: String(campo) }),
    );
  // Le scelte discrete — segmentati, elenchi, caselle — hanno la guardia muta
  // dello scafale (`scafaleScrivibile`): due scelte rapide su campi diversi
  // partirebbero tutte e due dall'albero di prima della validazione, e la
  // seconda disferebbe la prima. I cursori no: trascinati scrivono di continuo
  // lo stesso campo, e con la guardia perderebbero l'ultima posizione.
  const scegli = (campo: keyof NodoScafale, valore: unknown) => {
    if (scafaleScrivibile(s)) cambia(campo, valore);
  };
  const zona = nodo.kind === "zone";
  const parole = (scelta: string, voci: readonly string[], campo: keyof NodoScafale, etichetta: string) => (
    <Riga che={etichetta}>
      <Segmentato
        etichetta={etichetta}
        classe="minuto denso"
        scelta={scelta}
        onScegli={(v) => scegli(campo, v)}
        voci={voci.map((v) => ({ chiave: v, etichetta: v }))}
      />
    </Riga>
  );

  return (
    <div className="ispezione-voce" aria-disabled={bloccato || undefined}>
      <TestaVoce
        nome={nodo.name}
        genere={zona ? t("studio3.shell.zone") : nomeGruppo(def?.group ?? "widget")}
        descrizione={def === undefined ? undefined : descrizioneWidget(def.name, def.description)}
      >
        {nodo.fromPrefab !== null && (
          <p className="nota-ispezione">{t("studio3.shell.fromPrefab", { nome: nodo.fromPrefab })}</p>
        )}
      </TestaVoce>
      {bloccato && <p className="nota-ispezione">{t("studio3.shell.locked")}</p>}
      <fieldset className="gruppo-ispezione campi-nodo" disabled={bloccato}>
        <Riga che={t("studio3.shell.size")}>
          <div className="misura-nodo">
            <Segmentato<"hug" | "fill" | "fissa">
              etichetta={t("studio3.shell.size")}
              classe="minuto denso"
              scelta={nodo.size === "hug" || nodo.size === "fill" ? nodo.size : "fissa"}
              onScegli={(v) => scegli("size", v === "fissa" ? "240px" : v)}
              voci={[
                { chiave: "hug", etichetta: "Hug" },
                { chiave: "fill", etichetta: "Fill" },
                { chiave: "fissa", etichetta: t("studio3.shell.size.fixed") },
              ]}
            />
            {nodo.size !== "hug" && nodo.size !== "fill" && (
              <Cursore
                valore={Number.parseFloat(nodo.size) || 240}
                min={24}
                max={480}
                passo={2}
                suffisso="px"
                etichetta={t("studio3.shell.size")}
                onCambia={(v) => v !== null && cambia("size", `${String(v)}px`)}
              />
            )}
          </div>
        </Riga>
        {zona && (
          <>
            {parole(nodo.name, registro.vocabolario.zones, "name", t("studio3.shell.direction"))}
            {parole(nodo.gap ?? "none", registro.vocabolario.gaps, "gap", t("studio3.shell.gap"))}
            {parole(nodo.align ?? "stretch", registro.vocabolario.aligns, "align", t("studio3.shell.align"))}
            {parole(nodo.spread ?? "start", registro.vocabolario.spreads, "spread", t("studio3.shell.spread"))}
            <Riga che={t("studio3.shell.part")}>
              <select
                className="field-input"
                aria-label={t("studio3.shell.part")}
                value={nodo.part ?? ""}
                onChange={(e) => scegli("part", e.target.value === "" ? null : e.target.value)}
              >
                <option value="">{t("studio3.shell.part.none")}</option>
                {registro.parts.map((p) => (
                  <option key={p.name} value={p.name}>
                    {p.name}
                  </option>
                ))}
              </select>
            </Riga>
          </>
        )}
        {def !== undefined &&
          def.options.map((o) => {
            const valore = nodo.options[o.name] ?? o.default;
            const scriviOpzione = (v: boolean | string | number) => cambia("options", { ...nodo.options, [o.name]: v });
            const scegliOpzione = (v: boolean | string | number) => scegli("options", { ...nodo.options, [o.name]: v });
            const descrizione = descrizioneOpzione(nodo.name, o.name, o.description);
            if (o.kind === "flag") {
              return (
                <label key={o.name} className="spunta" title={descrizione}>
                  <input type="checkbox" checked={valore === true} onChange={(e) => scegliOpzione(e.target.checked)} />
                  <span>{o.name}</span>
                </label>
              );
            }
            if (o.kind === "word") {
              return (
                <Riga key={o.name} che={o.name}>
                  <Segmentato
                    etichetta={descrizione}
                    classe="minuto denso"
                    scelta={String(valore)}
                    onScegli={scegliOpzione}
                    voci={o.allowed.map((a) => ({ chiave: a, etichetta: a }))}
                  />
                </Riga>
              );
            }
            return (
              <Riga key={o.name} che={o.name}>
                <Cursore
                  valore={Number(valore)}
                  min={o.min ?? 0}
                  max={o.max ?? 10}
                  passo={1}
                  etichetta={descrizione}
                  onCambia={(v) => v !== null && scriviOpzione(Number(v))}
                />
              </Riga>
            );
          })}
      </fieldset>
    </div>
  );
}

// ── il documento ─────────────────────────────────────────────────────────────

const CAMPI_META = ["name", "author", "version", "description", "license"] as const;

export function IspezioneDocumento() {
  const { s, documento, scrivi } = useStudio();
  const meta = oggetto(documento?.["meta"]);
  const anteprima = Array.isArray(meta["preview"]) ? (meta["preview"] as unknown[]).map(String) : [];
  const [id, setId] = useState(String(documento?.["id"] ?? s.id));
  useEffect(() => setId(String(documento?.["id"] ?? s.id)), [documento, s.id]);
  // La regola del formato, non quella più larga delle cartelle: un id che il
  // formato rifiuta non arriva nel documento, e quindi non sposta la bozza.
  // Vedi `bozza.ts`.
  const idValido = idDelFormato(id);

  return (
    <div className="ispezione-voce">
      <TestaVoce nome={String(meta["name"] ?? s.id)} genere={t("studio3.kind.document")} descrizione={t("studio3.doc.hint")} />
      <section className="gruppo-ispezione">
        {CAMPI_META.map((campo) => (
          <CampoTesto
            key={campo}
            etichetta={t(`studio3.doc.${campo}`)}
            valore={typeof meta[campo] === "string" ? meta[campo] : ""}
            lungo={campo === "description"}
            onCambia={(v) =>
              scrivi(["meta", campo], v === "" && campo !== "name" ? null : v, t("studio3.step.meta", { campo: t(`studio3.doc.${campo}`) }))
            }
          />
        ))}
        <Riga che={t("studio3.doc.id")}>
          <div className="rinomina">
            <input
              className="field-input"
              type="text"
              spellCheck={false}
              aria-label={t("studio3.doc.id")}
              aria-invalid={!idValido || undefined}
              value={id}
              onChange={(e) => setId(e.target.value)}
              onBlur={() => {
                if (idValido && id !== documento?.["id"]) scrivi(["id"], id, t("studio3.step.meta", { campo: t("studio3.doc.id") }));
              }}
            />
            {/* Il perché, accanto al campo rosso: prima lo diceva la
                validazione, quando l'id sbagliato era già nel documento. */}
            {!idValido && <span className="errore-codice">{t("studio3.doc.idRule")}</span>}
            <span className="nota-ispezione">{t("studio3.doc.idHint")}</span>
          </div>
        </Riga>
      </section>
      <section className="gruppo-ispezione">
        <h4 className="titolo-gruppo-ispezione">{t("studio3.doc.preview")}</h4>
        <div className="anteprima-meta">
          {[0, 1, 2].map((i) => (
            <label key={i} className="colore-meta">
              <input
                type="color"
                aria-label={t("studio3.doc.previewColor", { n: i + 1 })}
                value={/^#[0-9a-f]{6}$/i.test(anteprima[i] ?? "") ? anteprima[i] : "#808080"}
                onChange={(e) => {
                  const nuove = [0, 1, 2].map((j) => (j === i ? e.target.value : (anteprima[j] ?? "#808080")));
                  scrivi(["meta", "preview"], nuove, t("studio3.step.meta", { campo: t("studio3.doc.preview") }));
                }}
              />
            </label>
          ))}
        </div>
      </section>
      <p className="nota-ispezione">{t("studio3.doc.format", { n: Number(documento?.["format"] ?? 2) })}</p>
    </div>
  );
}

/** Un campo di testo che scrive quando si esce, non a ogni lettera. */
function CampoTesto({
  etichetta,
  valore,
  lungo = false,
  onCambia,
}: {
  etichetta: string;
  valore: string;
  lungo?: boolean;
  onCambia: (v: string) => void;
}) {
  const [scritto, setScritto] = useState(valore);
  useEffect(() => setScritto(valore), [valore]);
  const applica = () => {
    if (scritto !== valore) onCambia(scritto);
  };
  return (
    <Riga che={etichetta}>
      {lungo ? (
        <textarea
          className="field-input"
          aria-label={etichetta}
          rows={3}
          value={scritto}
          onChange={(e) => setScritto(e.target.value)}
          onBlur={applica}
        />
      ) : (
        <input
          className="field-input"
          type="text"
          aria-label={etichetta}
          value={scritto}
          onChange={(e) => setScritto(e.target.value)}
          onBlur={applica}
          onKeyDown={(e) => {
            if (e.key === "Enter") applica();
          }}
        />
      )}
    </Riga>
  );
}

// ── niente di scelto ─────────────────────────────────────────────────────────

export function IspezioneVuota() {
  const { s, registro } = useStudio();
  const valido = s.valido;
  return (
    <div className="ispezione-voce ispezione-vuota">
      <Icona nome="i-probe" dim={28} />
      <h3>{t("studio3.inspector.empty")}</h3>
      <p className="nota-ispezione">{t("studio3.inspector.empty.hint")}</p>
      <ul className="scorciatoie-studio">
        <li>
          <kbd>I</kbd> {t("studio3.keys.probe")}
        </li>
        <li>
          <kbd>Ctrl K</kbd> {t("studio3.keys.search")}
        </li>
        <li>
          <kbd>1–4</kbd> {t("studio3.keys.tabs")}
        </li>
        <li>
          <kbd>F</kbd> {t("studio3.keys.replay")}
        </li>
        <li>
          <kbd>Ctrl Z</kbd> {t("studio3.keys.undo")}
        </li>
        <li>
          <kbd>Ctrl S</kbd> {t("studio3.keys.snapshot")}
        </li>
      </ul>
      {valido !== null && (
        <section className="gruppo-ispezione">
          <h4 className="titolo-gruppo-ispezione">{t("studio3.inspector.summary")}</h4>
          <p className="nota-ispezione">
            {t("studio3.inspector.summary.body", {
              parti: valido.parti,
              scafale: valido.costoScafale,
              budgetScafale: registro.shellBudget,
              finestra: valido.costoFinestra,
              budgetFinestra: registro.windowMotionBudget,
            })}
          </p>
        </section>
      )}
    </div>
  );
}
