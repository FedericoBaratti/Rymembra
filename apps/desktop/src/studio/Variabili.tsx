/**
 * Le variabili: quel che la skin dichiara una volta e usa ovunque.
 *
 * Il tema e la sua variante, la tavolozza, i token, i motivi, le curve, le
 * animazioni, i caratteri e le immagini del pacchetto, le istantanee. Ogni voce
 * si sceglie e si modifica nell'ispettore a destra, come una parte: il
 * pannello elenca, l'ispettore scrive. Così una cosa sola — «che cosa ho in
 * mano» — decide che cosa si vede a destra, e non ci sono due editor dello
 * stesso colore in due posti diversi.
 *
 * Tutto quel che si vede qui segue il tema scelto nella barra: guardando la
 * variante, i campioni mostrano i suoi valori, e una voce che la variante non
 * sovrascrive si legge attenuata — è ereditata dalla base.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState, type ReactNode } from "react";

import { ConfermaGesto, type Domanda } from "../ConfermaGesto";
import { ipc, type Istantanea, type TokenRegistro, type VoceSkin } from "../ipc";
import { numero } from "../formato";
import { locale, t } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { haUnPacchetto, scarto } from "./bozza";
import { useStudio } from "./contesto";
import { Generatore } from "./Generatore";
import type { Selezione } from "./negozio";
import { stessaSelezione, testo } from "./negozio";
import { elencoBreve, usiDellaRisorsa } from "./risorse";
import { sentieroColore, sentieroMotivo } from "./sentieri";
import { ritrattoEffetto, tintaNel, type Livello } from "./valori";
import { nomeGruppo } from "./vocabolario";

type Sezione =
  | "tema"
  | "tavolozza"
  | "token"
  | "motivi"
  | "curve"
  | "animazioni"
  | "caratteri"
  | "istantanee";

const oggetto = (v: unknown): Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/** Un nome che non è già preso: `colore`, `colore-2`, … */
export function nomeLibero(base: string, presi: readonly string[]): string {
  if (!presi.includes(base)) return base;
  for (let n = 2; n < 1000; n += 1) {
    if (!presi.includes(`${base}-${n}`)) return `${base}-${n}`;
  }
  return `${base}-${Date.now()}`;
}

/** La sezione in cui sta la voce scelta, se è una variabile. */
function sezioneDi(selezione: Selezione | null): Sezione | null {
  switch (selezione?.tipo) {
    case "token":
      return "token";
    case "colore":
      return "tavolozza";
    case "motivo":
      return "motivi";
    case "curva":
      return "curve";
    case "animazione":
      return "animazioni";
    case "risorsa":
    case "carattere":
      return "caratteri";
    default:
      return null;
  }
}

export function Variabili() {
  const { s } = useStudio();
  const [cerca, setCerca] = useState("");
  const [aperte, setAperte] = useState<ReadonlySet<Sezione>>(
    () => new Set<Sezione>(["tema", "tavolozza", "token"]),
  );
  // Una voce scelta da fuori — «vai» dai Problemi, un'animazione creata
  // dall'ispettore — apre la sua sezione: una scelta dentro una sezione chiusa
  // è una scelta che non si vede.
  const daAprire = sezioneDi(s.selezione);
  useEffect(() => {
    if (daAprire === null) return;
    setAperte((prima) => (prima.has(daAprire) ? prima : new Set([...prima, daAprire])));
  }, [daAprire]);
  const alterna = (quale: Sezione) =>
    setAperte((prima) => {
      const dopo = new Set(prima);
      if (dopo.has(quale)) dopo.delete(quale);
      else dopo.add(quale);
      return dopo;
    });
  const ago = cerca.trim().toLowerCase();
  // Cercando si apre tutto: una voce trovata dentro una sezione chiusa è una
  // voce che non si è trovata.
  const aperta = (quale: Sezione) => ago !== "" || aperte.has(quale);

  return (
    <div className="variabili-studio">
      <label className="cerca-livelli">
        <Icona nome="i-search" dim={14} />
        <input
          type="search"
          className="field-input"
          placeholder={t("studio3.vars.search")}
          aria-label={t("studio3.vars.search")}
          value={cerca}
          onChange={(e) => setCerca(e.target.value)}
        />
      </label>
      <SezioneVariabili quale="tema" titolo={t("studio3.vars.theme")} aperta={aperta("tema")} onAlterna={alterna}>
        <Tema />
      </SezioneVariabili>
      <SezioneVariabili quale="tavolozza" titolo={t("studio3.vars.palette")} aperta={aperta("tavolozza")} onAlterna={alterna}>
        <Tavolozza ago={ago} />
      </SezioneVariabili>
      <SezioneVariabili quale="token" titolo={t("studio3.vars.tokens")} aperta={aperta("token")} onAlterna={alterna}>
        <Token ago={ago} />
      </SezioneVariabili>
      <SezioneVariabili quale="motivi" titolo={t("studio3.vars.patterns")} aperta={aperta("motivi")} onAlterna={alterna}>
        <Motivi ago={ago} />
      </SezioneVariabili>
      <SezioneVariabili quale="curve" titolo={t("studio3.vars.easings")} aperta={aperta("curve")} onAlterna={alterna}>
        <Curve ago={ago} />
      </SezioneVariabili>
      <SezioneVariabili quale="animazioni" titolo={t("studio3.vars.animations")} aperta={aperta("animazioni")} onAlterna={alterna}>
        <Animazioni ago={ago} />
      </SezioneVariabili>
      <SezioneVariabili quale="caratteri" titolo={t("studio3.vars.assets")} aperta={aperta("caratteri")} onAlterna={alterna}>
        <Risorse ago={ago} />
      </SezioneVariabili>
      <SezioneVariabili quale="istantanee" titolo={t("studio3.vars.snapshots")} aperta={aperta("istantanee")} onAlterna={alterna}>
        <Istantanee />
      </SezioneVariabili>
    </div>
  );
}

function SezioneVariabili({
  quale,
  titolo,
  aperta,
  onAlterna,
  children,
}: {
  quale: Sezione;
  titolo: string;
  aperta: boolean;
  onAlterna: (quale: Sezione) => void;
  children: ReactNode;
}) {
  return (
    <section className="sezione-variabili" data-aperta={aperta || undefined}>
      <button
        type="button"
        className="testa-variabili"
        aria-expanded={aperta}
        onClick={() => onAlterna(quale)}
      >
        <Icona nome={aperta ? "i-chev-d" : "i-chev-r"} dim={11} />
        <span>{titolo}</span>
      </button>
      {aperta && <div className="corpo-variabili">{children}</div>}
    </section>
  );
}

/** Una voce scelta: la riga si accende, e un clic la apre nell'ispettore. */
function useScelta() {
  const { s, manda } = useStudio();
  return {
    scelta: (sel: Selezione) => stessaSelezione(s.selezione, sel),
    scegli: (sel: Selezione) => manda({ tipo: "scegli", selezione: sel }),
  };
}

// ── il tema ──────────────────────────────────────────────────────────────────

function Tema() {
  const { s, manda, documento, scrivi } = useStudio();
  const scura = documento?.["scheme"] !== "light";
  const variante = documento?.["variant"] === undefined ? null : oggetto(documento["variant"]);
  const conta = (chiave: string) => Object.keys(oggetto(variante?.[chiave])).length;
  const dinamico = oggetto(documento?.["capabilities"])["dynamicAccent"] === true;
  const opposto = scura ? t("studio3.bar.light") : t("studio3.bar.dark");

  return (
    <div className="tema-variabili">
      <div className="riga-variabili">
        <span className="che">{t("studio3.vars.theme.base")}</span>
        <Segmentato<"dark" | "light">
          etichetta={t("studio3.vars.theme.base")}
          classe="minuto"
          scelta={scura ? "dark" : "light"}
          onScegli={(v) =>
            scrivi(["scheme"], v, t("studio3.step.scheme", { tema: v === "dark" ? t("studio3.bar.dark") : t("studio3.bar.light") }))
          }
          voci={[
            { chiave: "dark", etichetta: t("studio3.bar.dark") },
            { chiave: "light", etichetta: t("studio3.bar.light") },
          ]}
        />
      </div>
      {variante === null ? (
        <p className="nota-variabili">{t("studio3.vars.variant.none", { tema: opposto })}</p>
      ) : (
        <p className="nota-variabili">
          {t("studio3.vars.variant.some", {
            tema: opposto,
            token: conta("tokens"),
            colori: conta("palette"),
            motivi: conta("patterns"),
            parti: conta("parts"),
          })}
        </p>
      )}
      <div className="azioni-variabili">
        {s.tema === "base" ? (
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={() => manda({ tipo: "imposta", campi: { tema: "variante" } })}
          >
            <Icona nome={scura ? "i-sun" : "i-moon"} dim={13} />
            {variante === null
              ? t("studio3.vars.variant.start", { tema: opposto })
              : t("studio3.vars.variant.look", { tema: opposto })}
          </button>
        ) : (
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={() => manda({ tipo: "imposta", campi: { tema: "base" } })}
          >
            <Icona nome={scura ? "i-moon" : "i-sun"} dim={13} />
            {t("studio3.vars.variant.back")}
          </button>
        )}
        {variante !== null && (
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={() => {
              scrivi(["variant"], null, t("studio3.step.variantRemove"));
              manda({ tipo: "imposta", campi: { tema: "base" } });
            }}
          >
            <Icona nome="i-x" dim={13} />
            {t("studio3.vars.variant.remove")}
          </button>
        )}
      </div>
      <label className="spunta riga-variabili">
        <input
          type="checkbox"
          checked={dinamico}
          onChange={(e) =>
            scrivi(
              ["capabilities", "dynamicAccent"],
              e.target.checked,
              e.target.checked ? t("studio3.step.dynamicOn") : t("studio3.step.dynamicOff"),
            )
          }
        />
        <span>{t("studio3.vars.dynamic")}</span>
      </label>
      {s.tema === "variante" && <p className="nota-variabili tema-acceso">{t("studio3.vars.variant.editing", { tema: opposto })}</p>}
      <Generatore />
    </div>
  );
}

// ── la tavolozza ─────────────────────────────────────────────────────────────

function Tavolozza({ ago }: { ago: string }) {
  const { s, registro, documento, scrivi } = useStudio();
  const { scelta, scegli } = useScelta();
  const variante = s.tema === "variante";
  const base = oggetto(documento?.["palette"]);
  const altra = oggetto(oggetto(documento?.["variant"])["palette"]);
  const nomi = [...new Set([...Object.keys(base), ...Object.keys(altra)])].filter(
    (n) => ago === "" || n.includes(ago),
  );
  const usi = new Map(s.valido?.tavolozza ?? []);

  return (
    <div className="tavolozza-variabili">
      {nomi.length === 0 && <p className="nota-variabili">{t("studio3.vars.palette.none")}</p>}
      <div className="griglia-campioni">
        {nomi.map((nome) => {
          const valore = variante ? (altra[nome] ?? base[nome]) : base[nome];
          const ereditato = variante && altra[nome] === undefined;
          return (
            <button
              key={nome}
              type="button"
              className="campione-tavolozza"
              data-active={scelta({ tipo: "colore", nome }) || undefined}
              data-ereditato={ereditato || undefined}
              title={t("studio3.vars.palette.uses", { nome, n: usi.get(nome) ?? 0 })}
              onClick={() => scegli({ tipo: "colore", nome })}
            >
              <span
                className="campione"
                style={{ background: tintaNel(valore, documento, variante, registro.tokens) ?? undefined }}
              />
              <code>{nome}</code>
              <span className="usi">{usi.get(nome) ?? 0}</span>
            </button>
          );
        })}
      </div>
      <button
        type="button"
        className="aggiungi-livello"
        onClick={() => {
          const nome = nomeLibero("colore", Object.keys(base));
          scrivi(sentieroColore(nome, "base"), "#8b7cf6", t("studio3.step.colorAdd", { nome }));
          scegli({ tipo: "colore", nome });
        }}
      >
        <Icona nome="i-plus" dim={13} />
        <span>{t("studio3.vars.palette.add")}</span>
      </button>
    </div>
  );
}

// ── i token ──────────────────────────────────────────────────────────────────

/** Come si legge un valore di token in poche lettere, accanto al nome. */
function riassunto(valore: unknown): string {
  if (valore === undefined) return "";
  if (typeof valore === "string" || typeof valore === "number") return String(valore);
  if (Array.isArray(valore)) return valore.map(String).join(", ");
  const dentro = oggetto(valore);
  if (typeof dentro["$token"] === "string") return `→ ${dentro["$token"]}`;
  if (typeof dentro["$palette"] === "string") return `◆ ${dentro["$palette"]}`;
  if (typeof dentro["$source"] === "string") return `✦ ${dentro["$source"]}`;
  if (Array.isArray(dentro["layers"])) return t("studio3.vars.tokens.layers", { n: dentro["layers"].length });
  if (dentro["kind"] === "keyword") return String(dentro["keyword"]);
  if (dentro["kind"] === "cubicBezier") return "cubic-bezier";
  if (dentro["preferred"] !== undefined) return `clamp(${String(dentro["preferred"])})`;
  return "{…}";
}

function Token({ ago }: { ago: string }) {
  const { s, registro, documento } = useStudio();
  const { scelta, scegli } = useScelta();
  const [chiusi, setChiusi] = useState<ReadonlySet<string>>(new Set());
  const [soloDichiarati, setSoloDichiarati] = useState(false);
  const variante = s.tema === "variante";
  const base = oggetto(documento?.["tokens"]);
  const altra = oggetto(oggetto(documento?.["variant"])["tokens"]);

  const gruppi = useMemo(() => {
    const fuori = new Map<string, TokenRegistro[]>();
    for (const tk of registro.tokens) {
      const dichiarato = variante ? altra[tk.id] !== undefined : base[tk.id] !== undefined;
      if (soloDichiarati && !dichiarato) continue;
      if (ago !== "" && !tk.id.includes(ago) && !nomeGruppo(tk.group).toLowerCase().includes(ago)) continue;
      fuori.set(tk.group, [...(fuori.get(tk.group) ?? []), tk]);
    }
    return [...fuori.entries()];
  }, [registro.tokens, base, altra, variante, soloDichiarati, ago]);

  return (
    <div className="token-variabili">
      <label className="spunta">
        <input
          type="checkbox"
          checked={soloDichiarati}
          onChange={(e) => setSoloDichiarati(e.target.checked)}
        />
        <span>{variante ? t("studio3.vars.tokens.onlyVariant") : t("studio3.vars.tokens.onlyDeclared")}</span>
      </label>
      {gruppi.map(([gruppo, tokens]) => {
        const aperto = ago !== "" || !chiusi.has(gruppo);
        return (
          <div key={gruppo} className="gruppo-token">
            <button
              type="button"
              className="testa-gruppo-token"
              aria-expanded={aperto}
              onClick={() =>
                setChiusi((prima) => {
                  const dopo = new Set(prima);
                  if (dopo.has(gruppo)) dopo.delete(gruppo);
                  else dopo.add(gruppo);
                  return dopo;
                })
              }
            >
              <Icona nome={aperto ? "i-chev-d" : "i-chev-r"} dim={10} />
              <span>{nomeGruppo(gruppo)}</span>
              <span className="quante">{tokens.length}</span>
            </button>
            {aperto &&
              tokens.map((tk) => {
                const proprio = variante ? altra[tk.id] : base[tk.id];
                const valore = proprio ?? (variante ? base[tk.id] : undefined);
                return (
                  <button
                    key={tk.id}
                    type="button"
                    className="voce-token"
                    data-active={scelta({ tipo: "token", id: tk.id }) || undefined}
                    data-dichiarato={proprio !== undefined || undefined}
                    data-ereditato={(variante && proprio === undefined && valore !== undefined) || undefined}
                    title={tk.description}
                    onClick={() => scegli({ tipo: "token", id: tk.id })}
                  >
                    {tk.kind === "color" ? (
                      <span
                        className="campione piccolo"
                        style={{
                          background:
                            tintaNel(valore, documento, variante, registro.tokens) ?? `var(${tk.css})`,
                        }}
                      />
                    ) : (
                      <span className="tipo-token">{tk.kind.slice(0, 3)}</span>
                    )}
                    <code className="nome-voce">{tk.id}</code>
                    <span className="valore-token">{riassunto(valore)}</span>
                  </button>
                );
              })}
          </div>
        );
      })}
    </div>
  );
}

// ── i motivi ─────────────────────────────────────────────────────────────────

function Motivi({ ago }: { ago: string }) {
  const { s, registro, documento, scrivi } = useStudio();
  const { scelta, scegli } = useScelta();
  const [scegliendo, setScegliendo] = useState(false);
  const motivi = oggetto(documento?.["patterns"]);
  const altri = oggetto(oggetto(documento?.["variant"])["patterns"]);
  const variante = s.tema === "variante";
  const tavolozza = oggetto(documento?.["palette"]) as Record<string, string>;
  const nomi = [...new Set([...Object.keys(motivi), ...Object.keys(altri)])].filter(
    (n) => ago === "" || n.includes(ago),
  );
  const impilabili = registro.effects.filter((e) => e.target === "background");

  return (
    <div className="motivi-variabili">
      {nomi.length === 0 && <p className="nota-variabili">{t("studio3.vars.patterns.none")}</p>}
      {nomi.map((nome) => {
        const effetto = (variante ? (altri[nome] ?? motivi[nome]) : motivi[nome]) as Livello | undefined;
        return (
          <button
            key={nome}
            type="button"
            className="voce-motivo"
            data-active={scelta({ tipo: "motivo", nome }) || undefined}
            data-ereditato={(variante && altri[nome] === undefined) || undefined}
            onClick={() => scegli({ tipo: "motivo", nome })}
          >
            <span
              className="campione largo"
              style={{
                background:
                  effetto === undefined ? undefined : ritrattoEffetto(effetto, registro.tokens, tavolozza, motivi),
              }}
            />
            <code className="nome-voce">{nome}</code>
            <span className="valore-token">{String(oggetto(effetto)["effect"] ?? "")}</span>
          </button>
        );
      })}
      {scegliendo ? (
        <div className="elenco-effetti" role="group" aria-label={t("studio3.vars.patterns.add")}>
          {impilabili.map((e) => (
            <button
              key={e.name}
              type="button"
              className="un-widget"
              onClick={() => {
                const nome = nomeLibero(e.name.toLowerCase(), Object.keys(motivi));
                try {
                  scrivi(sentieroMotivo(nome, "base"), JSON.parse(e.esempio), t("studio3.step.patternAdd", { nome }));
                  scegli({ tipo: "motivo", nome });
                } finally {
                  setScegliendo(false);
                }
              }}
            >
              <code>{e.name}</code>
              <span className="peso">{e.cost}</span>
            </button>
          ))}
          <button type="button" className="pillola btn-ghost" onClick={() => setScegliendo(false)}>
            {t("common.cancel")}
          </button>
        </div>
      ) : (
        <button type="button" className="aggiungi-livello" onClick={() => setScegliendo(true)}>
          <Icona nome="i-plus" dim={13} />
          <span>{t("studio3.vars.patterns.add")}</span>
        </button>
      )}
    </div>
  );
}

// ── le curve ─────────────────────────────────────────────────────────────────

/** Una curva disegnata in piccolo, o niente se non è una bezier. */
export function MiniCurva({ curva }: { curva: unknown }) {
  const punti = oggetto(curva)["points"];
  if (!Array.isArray(punti) || punti.length !== 4) return <span className="mini-curva" />;
  const [x1, y1, x2, y2] = punti.map(Number) as [number, number, number, number];
  return (
    <svg className="mini-curva" viewBox="0 0 20 20" aria-hidden="true">
      <path d="M0 20 L20 0" className="diagonale" />
      <path d={`M0 20 C ${x1 * 20} ${20 - y1 * 20}, ${x2 * 20} ${20 - y2 * 20}, 20 0`} className="curva" />
    </svg>
  );
}

function Curve({ ago }: { ago: string }) {
  const { documento, scrivi } = useStudio();
  const { scelta, scegli } = useScelta();
  const curve = oggetto(oggetto(documento?.["motion"])["easings"]);
  const nomi = Object.keys(curve).filter((n) => ago === "" || n.includes(ago));
  return (
    <div className="curve-variabili">
      {nomi.length === 0 && <p className="nota-variabili">{t("studio3.vars.easings.none")}</p>}
      {nomi.map((nome) => (
        <button
          key={nome}
          type="button"
          className="voce-motivo"
          data-active={scelta({ tipo: "curva", nome }) || undefined}
          onClick={() => scegli({ tipo: "curva", nome })}
        >
          <MiniCurva curva={curve[nome]} />
          <code className="nome-voce">{nome}</code>
        </button>
      ))}
      <button
        type="button"
        className="aggiungi-livello"
        onClick={() => {
          const nome = nomeLibero("curva", Object.keys(curve));
          scrivi(
            ["motion", "easings", nome],
            { kind: "cubicBezier", points: [0.2, 0.8, 0.2, 1] },
            t("studio3.step.easingAdd", { nome }),
          );
          scegli({ tipo: "curva", nome });
        }}
      >
        <Icona nome="i-plus" dim={13} />
        <span>{t("studio3.vars.easings.add")}</span>
      </button>
    </div>
  );
}

// ── le animazioni ────────────────────────────────────────────────────────────

/** Quante volte ogni animazione è richiamata dalle parti. */
export function richiamiAnimazioni(documento: Record<string, unknown> | null): Map<string, string[]> {
  const fuori = new Map<string, string[]>();
  for (const [parte, stile] of Object.entries(oggetto(documento?.["parts"]))) {
    for (const [innesco, nome] of Object.entries(oggetto(oggetto(stile)["motion"]))) {
      if (typeof nome !== "string" || innesco === "stagger") continue;
      fuori.set(nome, [...(fuori.get(nome) ?? []), `${parte} · ${innesco}`]);
    }
  }
  return fuori;
}

function Animazioni({ ago }: { ago: string }) {
  const { registro, documento, scrivi } = useStudio();
  const { scelta, scegli } = useScelta();
  const [pronte, setPronte] = useState(false);
  const animazioni = oggetto(oggetto(documento?.["motion"])["animations"]);
  const nomi = Object.keys(animazioni).filter((n) => ago === "" || n.includes(ago));
  const richiami = richiamiAnimazioni(documento);
  const piene = Object.keys(animazioni).length >= registro.maxAnimazioni;

  const aggiungi = (base: string, valore: unknown) => {
    const nome = nomeLibero(base, Object.keys(animazioni));
    scrivi(["motion", "animations", nome], valore, t("studio3.step.animationAdd", { nome }));
    scegli({ tipo: "animazione", nome });
  };

  return (
    <div className="animazioni-variabili">
      {nomi.length === 0 && <p className="nota-variabili">{t("studio3.vars.animations.none")}</p>}
      {nomi.map((nome) => {
        const a = oggetto(animazioni[nome]);
        const usata = richiami.get(nome) ?? [];
        return (
          <button
            key={nome}
            type="button"
            className="voce-motivo"
            data-active={scelta({ tipo: "animazione", nome }) || undefined}
            data-mai={usata.length === 0 || undefined}
            title={usata.length === 0 ? t("studio3.vars.animations.unused") : usata.join("\n")}
            onClick={() => scegli({ tipo: "animazione", nome })}
          >
            <Icona nome="i-timeline" dim={12} />
            <code className="nome-voce">{nome}</code>
            <span className="valore-token">
              {String(a["duration"] ?? "")}
              {a["iterations"] === "infinite" ? " · ∞" : ""}
            </span>
            <span className="usi">{usata.length}</span>
          </button>
        );
      })}
      {pronte ? (
        <div className="elenco-effetti" role="group" aria-label={t("studio3.vars.animations.ready")}>
          {registro.animazioniPronte.map((p) => (
            <button
              key={p.id}
              type="button"
              className="un-widget"
              title={t("studio3.vars.animations.readyFor", { innesco: p.innesco })}
              onClick={() => {
                try {
                  aggiungi(p.id, JSON.parse(p.valore));
                } finally {
                  setPronte(false);
                }
              }}
            >
              <code>{p.nome}</code>
              <span className="valore-token">{p.innesco}</span>
            </button>
          ))}
          <button type="button" className="pillola btn-ghost" onClick={() => setPronte(false)}>
            {t("common.cancel")}
          </button>
        </div>
      ) : (
        <div className="azioni-variabili">
          <button
            type="button"
            className="aggiungi-livello"
            disabled={piene}
            title={piene ? t("studio3.vars.animations.full", { n: registro.maxAnimazioni }) : undefined}
            onClick={() =>
              aggiungi("animazione", {
                duration: "300ms",
                easing: "ease-out",
                frames: [
                  { at: 0, opacity: 0 },
                  { at: 100, opacity: 1 },
                ],
              })
            }
          >
            <Icona nome="i-plus" dim={13} />
            <span>{t("studio3.vars.animations.add")}</span>
          </button>
          <button type="button" className="aggiungi-livello" disabled={piene} onClick={() => setPronte(true)}>
            <Icona nome="i-wand" dim={13} />
            <span>{t("studio3.vars.animations.ready")}</span>
          </button>
        </div>
      )}
    </div>
  );
}

// ── caratteri e immagini ─────────────────────────────────────────────────────

/** «fraunces-variable.woff2» → «Fraunces Variable». */
export function famigliaDa(file: string): string {
  const radice = file.replace(/\.[a-z0-9]+$/i, "").replace(/[-_]+/g, " ").trim();
  return (
    radice
      .split(" ")
      .filter((p) => p !== "")
      .map((p) => p[0]!.toUpperCase() + p.slice(1))
      .join(" ") || "Carattere"
  );
}

/**
 * La domanda prima di togliere un file del pacchetto: cosa lo usa, e che non
 * si torna indietro. Vedi `risorse.ts`. Una sola, per le due strade che
 * tolgono — il × qui e il tasto dell'ispettore del file.
 */
export function domandaTogliRisorsa(nome: string, usi: readonly string[], fai: () => void): Domanda {
  const chi =
    usi.length === 0
      ? t("studio3.vars.files.remove.unused")
      : t("studio3.vars.files.remove.used", { usi: elencoBreve(usi) });
  return {
    titolo: t("studio3.vars.files.remove.title", { nome }),
    descrizione: `${t("studio3.vars.files.remove.what")} ${chi}`,
    conferma: t("studio3.vars.files.remove", { nome }),
    fai,
  };
}

function Risorse({ ago }: { ago: string }) {
  const { s, manda, documento, scrivi, onErrore, file: voci, rileggiFile: rileggi } = useStudio();
  const { scelta, scegli } = useScelta();
  const [aggiungendo, setAggiungendo] = useState(false);
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  const caratteri = oggetto(documento?.["fonts"]);
  const file = voci
    .filter((v) => v.genere === "risorsa")
    .map((v) => ({ ...v, nome: v.nome.replace(/^assets\//, "") }))
    .filter((v) => ago === "" || v.nome.includes(ago));
  const famiglie = Object.keys(caratteri).filter((f) => ago === "" || f.toLowerCase().includes(ago));

  const aggiungi = async () => {
    const scelto = await open({
      multiple: false,
      directory: false,
      filters: [{ name: t("studio3.vars.assets.filter"), extensions: ["woff2", "png", "jpg", "jpeg", "webp"] }],
    });
    if (typeof scelto !== "string") return;
    setAggiungendo(true);
    try {
      const voce = await ipc.studioRisorsaAggiungi(s.id, scelto);
      const nome = voce.nome.replace(/^assets\//, "");
      rileggi();
      // Un carattere senza dichiarazione non serve a niente: lo si dichiara
      // subito, col nome del file, e lo si apre per ritoccarlo.
      if (nome.endsWith(".woff2")) {
        const famiglia = nomeLibero(famigliaDa(nome), Object.keys(caratteri));
        scrivi(["fonts", famiglia], { src: nome }, t("studio3.step.fontAdd", { nome: famiglia }));
        scegli({ tipo: "carattere", nome: famiglia });
      } else {
        scegli({ tipo: "risorsa", nome });
      }
    } catch (e) {
      onErrore(e);
    } finally {
      setAggiungendo(false);
    }
  };

  const togli = async (nome: string) => {
    try {
      await ipc.studioRisorsaTogli(s.id, nome);
      rileggi();
      // Chi lo dichiarava resterebbe a puntare un file che non c'è, e il
      // validatore non lo direbbe: guarda il documento, non il disco. Una
      // dichiarazione di carattere senza file non ha niente da correggere, e
      // si toglie con lui — da tutta la storia, senza un passo da annullare:
      // il file non torna. Vedi `risorse.ts`.
      manda({ tipo: "risorsaTolta", nome });
    } catch (e) {
      onErrore(e);
    }
  };

  return (
    <div className="risorse-variabili">
      <span className="titolino">{t("studio3.vars.fonts")}</span>
      {famiglie.length === 0 && <p className="nota-variabili">{t("studio3.vars.fonts.none")}</p>}
      {famiglie.map((famiglia) => (
        <button
          key={famiglia}
          type="button"
          className="voce-motivo"
          data-active={scelta({ tipo: "carattere", nome: famiglia }) || undefined}
          onClick={() => scegli({ tipo: "carattere", nome: famiglia })}
        >
          <span className="aa-carattere">Aa</span>
          <code className="nome-voce">{famiglia}</code>
          <span className="valore-token">{String(oggetto(caratteri[famiglia])["src"] ?? "")}</span>
        </button>
      ))}
      <span className="titolino">{t("studio3.vars.files")}</span>
      {file.length === 0 && <p className="nota-variabili">{t("studio3.vars.files.none")}</p>}
      {file.map((v) => (
        <div key={v.nome} className="riga-file">
          <button
            type="button"
            className="voce-motivo"
            data-active={scelta({ tipo: "risorsa", nome: v.nome }) || undefined}
            onClick={() => scegli({ tipo: "risorsa", nome: v.nome })}
          >
            <Icona nome={v.nome.endsWith(".woff2") ? "i-text" : "i-album"} dim={12} />
            <code className="nome-voce">{v.nome}</code>
            <span className="valore-token">{quantoPesa(v.byte)}</span>
          </button>
          <button
            type="button"
            className="tasto-icona icon-btn"
            aria-label={t("studio3.vars.files.remove", { nome: v.nome })}
            title={t("studio3.vars.files.remove", { nome: v.nome })}
            onClick={() =>
              setDomanda(
                domandaTogliRisorsa(v.nome, usiDellaRisorsa(documento, v.nome), () => void togli(v.nome)),
              )
            }
          >
            <Icona nome="i-x" dim={11} />
          </button>
        </div>
      ))}
      {domanda !== null && <ConfermaGesto domanda={domanda} onChiudi={() => setDomanda(null)} />}
      <button type="button" className="aggiungi-livello" disabled={aggiungendo} onClick={() => void aggiungi()}>
        <Icona nome="i-plus" dim={13} />
        <span>{aggiungendo ? t("common.loading") : t("studio3.vars.files.add")}</span>
      </button>
    </div>
  );
}

/** Quanto pesa un file, per chi legge. */
export function quantoPesa(byte: number): string {
  if (byte < 1024) return `${numero(byte)} B`;
  if (byte < 1024 * 1024) return `${numero(byte / 1024, byte < 10 * 1024 ? 1 : 0)} KB`;
  return `${numero(byte / 1024 / 1024, 1)} MB`;
}

// ── le istantanee ────────────────────────────────────────────────────────────

/** «3 minuti fa», nella lingua dell'interfaccia. */
function quandoFa(quando: number, adesso: number): string {
  const secondi = Math.round((quando - adesso) / 1000);
  const forma = new Intl.RelativeTimeFormat(locale(), { numeric: "auto" });
  const passi: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  for (const [unita, quanti] of passi) {
    if (Math.abs(secondi) >= quanti) return forma.format(Math.round(secondi / quanti), unita);
  }
  return forma.format(secondi, "second");
}

function Istantanee() {
  const { s, manda, scriviTesto, onErrore } = useStudio();
  const [elenco, setElenco] = useState<Istantanea[]>([]);
  const [giro, setGiro] = useState(0);
  const derivata = testo(s) !== s.originale;
  /**
   * Le skin di serie e installate, per sapere se «Butta la bozza» ha un
   * pacchetto a cui tornare: `null` finché non arrivano. Vedi `bozza.ts`.
   *
   * Una bozza che ha cambiato id non ce l'ha — Studio aperto su Plain, id
   * «mia» — e il nucleo la rifiuta senza toccare il disco; qui il tasto si
   * spegne prima e dice perché. Si rilegge quando cambia l'id e quando si
   * prende un'istantanea fuori da qui, cioè anche dopo «Salva e usa», che può
   * aver dato all'id un pacchetto suo (quell'istantanea viene prima
   * dell'installazione, e l'elenco può arrivare vecchio: il tasto resta spento
   * fino alla prossima, che è il verso che non perde niente). Un elenco che
   * non si legge non è un errore da mostrare: il tasto segue il solo testo,
   * come prima.
   */
  const [elencoSkin, setElencoSkin] = useState<VoceSkin[] | null>(null);
  useEffect(() => {
    let vivo = true;
    ipc
      .skinElenco()
      .then((v) => {
        if (vivo) setElencoSkin(v);
      })
      .catch(() => {
        if (vivo) setElencoSkin(null);
      });
    return () => {
      vivo = false;
    };
  }, [s.id, s.istantanee]);
  const stato = scarto(derivata, haUnPacchetto(s.id, elencoSkin));

  useEffect(() => {
    let vivo = true;
    ipc
      .studioIstantanee(s.id)
      .then((v) => {
        if (vivo) setElenco(v);
      })
      .catch(onErrore);
    return () => {
      vivo = false;
    };
    // `s.istantanee`: le istantanee prese fuori da qui — Ctrl+S, «Salva e
    // usa», «Esporta», la chat in agent — devono comparire anche loro.
  }, [s.id, giro, s.istantanee, onErrore]);

  const prendi = async () => {
    try {
      await ipc.studioIstantanea(s.id, testo(s), "manuale");
      setGiro((g) => g + 1);
    } catch (e) {
      onErrore(e);
    }
  };

  const ripristina = async (quando: number) => {
    try {
      const letto = await ipc.studioRipristina(s.id, quando);
      scriviTesto(letto, t("studio3.step.restore"), "istantanea");
    } catch (e) {
      onErrore(e);
    }
  };

  const scarta = async () => {
    try {
      // Un'istantanea prima: è l'unica azione che butta via qualcosa, e la
      // rete sotto è quella che c'è già.
      await ipc.studioIstantanea(s.id, testo(s), "manuale");
      const dalPacchetto = await ipc.studioScarta(s.id);
      scriviTesto(dalPacchetto, t("studio3.step.discard"), "istantanea");
      manda({ tipo: "salvato", testo: dalPacchetto });
      setGiro((g) => g + 1);
    } catch (e) {
      onErrore(e);
    }
  };

  const adesso = Date.now();
  return (
    <div className="istantanee-variabili">
      <button type="button" className="aggiungi-livello" onClick={() => void prendi()}>
        <Icona nome="i-plus" dim={13} />
        <span>{t("studio3.vars.snapshots.take")}</span>
      </button>
      {elenco.length === 0 && <p className="nota-variabili">{t("studio3.vars.snapshots.none")}</p>}
      {elenco.map((presa) => (
        <button
          key={presa.quando}
          type="button"
          className="istantanea"
          title={t("studio3.vars.snapshots.restore")}
          onClick={() => void ripristina(presa.quando)}
        >
          <span className="quale">{t(`studio.pkg.snap.${presa.causa}`)}</span>
          <span className="quanto">
            {t("studio.pkg.countsWhen", { parti: presa.parti, token: presa.token, quando: quandoFa(presa.quando, adesso) })}
          </span>
        </button>
      ))}
      <button
        type="button"
        className="pillola btn-ghost scarta-bozza"
        disabled={stato !== "si"}
        title={
          stato === "si"
            ? t("studio.pkg.drop.can")
            : stato === "niente"
              ? t("studio.pkg.drop.none")
              : t("studio.pkg.drop.noPackage")
        }
        onClick={() => void scarta()}
      >
        <Icona nome="i-x" dim={13} />
        {t("studio.pkg.drop")}
      </button>
    </div>
  );
}
