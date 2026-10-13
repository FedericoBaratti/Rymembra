/**
 * I livelli: tutto quel che una skin può ridipingere, e lo scafale.
 *
 * Le parti del registro, per gruppo, ognuna coi suoi segni: ridipinta (la skin
 * la dichiara), animata (ha un movimento), oltre il budget. Sceglierne una la
 * apre nell'ispettore **e** porta la tela dove si vede — `explore-row` sta in
 * Esplora, `menu-item` sotto un menù aperto — così non ci si chiede mai se la
 * skin la stia ridipingendo o se semplicemente non è sulla pagina.
 *
 * Sotto, lo scafale: l'albero di dove stanno le cose (`AlberoScafale.tsx`).
 *
 * L'elenco è un albero ARIA con un solo punto di tabulazione: le frecce
 * scorrono, destra e sinistra aprono e chiudono un gruppo, Invio sceglie.
 * Ctrl+K porta nel campo di ricerca da qualunque punto dello Studio.
 */
import { useMemo, useState, type KeyboardEvent } from "react";

import type { ParteRegistro } from "../ipc";
import { t } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { AlberoScafale } from "./AlberoScafale";
import { useStudio } from "./contesto";
import { parteScelta } from "./negozio";
import { descrizioneParte, nomeGruppo } from "./vocabolario";

type Filtro = "tutte" | "ridipinte" | "animate";

/** Che cosa la skin fa di una parte. */
interface Segni {
  ridipinta: boolean;
  animata: boolean;
  /** Nella variante: il tema opposto la ridipinge a modo suo. */
  variata: boolean;
  oltre: boolean;
}

export function Livelli() {
  const { s, manda, registro, documento } = useStudio();
  const [cerca, setCerca] = useState("");
  const [filtro, setFiltro] = useState<Filtro>("tutte");
  const [chiusi, setChiusi] = useState<ReadonlySet<string>>(new Set());
  const [fuoco, setFuoco] = useState<string | null>(null);
  const [scafaleAperto, setScafaleAperto] = useState(true);
  const scelta = parteScelta(s);

  const segni = useMemo(() => {
    const parti = (documento?.["parts"] ?? {}) as Record<string, Record<string, unknown>>;
    const varianti = (((documento?.["variant"] ?? {}) as Record<string, unknown>)["parts"] ??
      {}) as Record<string, unknown>;
    const costi = new Map((s.valido?.costi ?? []).map((c) => [c.parte, c]));
    const fuori = new Map<string, Segni>();
    for (const p of registro.parts) {
      const dichiarata = parti[p.name];
      const costo = costi.get(p.name);
      fuori.set(p.name, {
        ridipinta: dichiarata !== undefined,
        animata: dichiarata?.["motion"] !== undefined,
        variata: varianti[p.name] !== undefined,
        oltre:
          costo !== undefined &&
          (costo.superficie > registro.budget || costo.movimento > registro.motionBudget),
      });
    }
    return fuori;
  }, [documento, registro, s.valido]);

  const quante = {
    tutte: registro.parts.length,
    ridipinte: [...segni.values()].filter((g) => g.ridipinta || g.variata).length,
    animate: [...segni.values()].filter((g) => g.animata).length,
  };

  const gruppi = useMemo(() => {
    const ago = cerca.trim().toLowerCase();
    const fuori = new Map<string, ParteRegistro[]>();
    for (const p of registro.parts) {
      const g = segni.get(p.name);
      if (filtro === "ridipinte" && !(g?.ridipinta || g?.variata)) continue;
      if (filtro === "animate" && !g?.animata) continue;
      if (
        ago !== "" &&
        !p.name.includes(ago) &&
        !descrizioneParte(p.name, p.description).toLowerCase().includes(ago) &&
        !nomeGruppo(p.group).toLowerCase().includes(ago)
      )
        continue;
      fuori.set(p.group, [...(fuori.get(p.group) ?? []), p]);
    }
    return [...fuori.entries()];
  }, [registro, segni, filtro, cerca]);

  // Le righe visibili, in ordine: i gruppi e le parti dei gruppi aperti. Una
  // ricerca apre tutto — chi cerca vuole vedere quel che ha trovato.
  const cercando = cerca.trim() !== "";
  const righe: { chiave: string; gruppo: string; parte: string | null }[] = [];
  for (const [gruppo, parti] of gruppi) {
    righe.push({ chiave: `g:${gruppo}`, gruppo, parte: null });
    if (cercando || !chiusi.has(gruppo)) {
      for (const p of parti) righe.push({ chiave: `p:${p.name}`, gruppo, parte: p.name });
    }
  }
  const attiva =
    fuoco !== null && righe.some((r) => r.chiave === fuoco)
      ? fuoco
      : scelta !== null && righe.some((r) => r.parte === scelta)
        ? `p:${scelta}`
        : (righe[0]?.chiave ?? "");

  const scegli = (parte: string) => {
    manda({ tipo: "scegli", selezione: { tipo: "parte", nome: parte } });
    manda({ tipo: "mostra", parte });
  };

  const alterna = (gruppo: string) =>
    setChiusi((prima) => {
      const dopo = new Set(prima);
      if (dopo.has(gruppo)) dopo.delete(gruppo);
      else dopo.add(gruppo);
      return dopo;
    });

  const vaiA = (chiave: string) => {
    setFuoco(chiave);
    document
      .querySelector<HTMLElement>(`.albero-parti [data-riga="${CSS.escape(chiave)}"]`)
      ?.focus();
  };

  const daTastiera = (e: KeyboardEvent<HTMLDivElement>) => {
    const qui = righe.findIndex((r) => r.chiave === attiva);
    const riga = righe[qui];
    if (riga === undefined) return;
    const altra = (i: number) => {
      const presa = righe[Math.max(0, Math.min(righe.length - 1, i))];
      if (presa !== undefined) vaiA(presa.chiave);
    };
    let fatto = true;
    if (e.key === "ArrowDown") altra(qui + 1);
    else if (e.key === "ArrowUp") altra(qui - 1);
    else if (e.key === "Home") altra(0);
    else if (e.key === "End") altra(righe.length - 1);
    else if (e.key === "ArrowRight" && riga.parte === null) {
      if (chiusi.has(riga.gruppo)) alterna(riga.gruppo);
      else altra(qui + 1);
    } else if (e.key === "ArrowLeft") {
      if (riga.parte === null) {
        if (!chiusi.has(riga.gruppo)) alterna(riga.gruppo);
      } else vaiA(`g:${riga.gruppo}`);
    } else if (e.key === "Enter" || e.key === " ") {
      if (riga.parte === null) alterna(riga.gruppo);
      else scegli(riga.parte);
    } else fatto = false;
    if (fatto) e.preventDefault();
  };

  return (
    <div className="livelli-studio">
      <label className="cerca-livelli">
        <Icona nome="i-search" dim={14} />
        <input
          type="search"
          className="field-input"
          placeholder={t("studio3.layers.search")}
          aria-label={t("studio3.layers.search")}
          value={cerca}
          onChange={(e) => setCerca(e.target.value)}
          onKeyDown={(e) => {
            // Dalla ricerca si scende nell'elenco con la freccia, come in un
            // campo di suggerimenti: senza, trovata la parte bisognava
            // attraversare il filtro col tabulatore per arrivarci.
            if (e.key === "ArrowDown") {
              e.preventDefault();
              const prima = righe.find((r) => r.parte !== null);
              if (prima !== undefined) vaiA(prima.chiave);
            }
            if (e.key === "Escape" && cerca !== "") {
              e.stopPropagation();
              setCerca("");
            }
          }}
        />
        <kbd className="tasto-cerca">Ctrl K</kbd>
      </label>
      <Segmentato<Filtro>
        etichetta={t("studio3.layers.filter")}
        classe="minuto denso"
        scelta={filtro}
        onScegli={setFiltro}
        voci={[
          { chiave: "tutte", etichetta: t("studio3.layers.all"), conteggio: quante.tutte },
          { chiave: "ridipinte", etichetta: t("studio3.layers.painted"), conteggio: quante.ridipinte },
          { chiave: "animate", etichetta: t("studio3.layers.animated"), conteggio: quante.animate },
        ]}
      />

      <div
        className="albero-parti"
        role="tree"
        aria-label={t("studio3.layers.aria")}
        onKeyDown={daTastiera}
      >
        {gruppi.length === 0 && <p className="niente-livelli">{t("studio3.layers.nothing")}</p>}
        {gruppi.map(([gruppo, parti]) => {
          const aperto = cercando || !chiusi.has(gruppo);
          return (
            <div key={gruppo} role="none">
              <div
                role="treeitem"
                className="gruppo-livelli"
                data-riga={`g:${gruppo}`}
                aria-level={1}
                aria-expanded={aperto}
                aria-selected={false}
                tabIndex={attiva === `g:${gruppo}` ? 0 : -1}
                onClick={() => {
                  setFuoco(`g:${gruppo}`);
                  alterna(gruppo);
                }}
              >
                <Icona nome={aperto ? "i-chev-d" : "i-chev-r"} dim={11} />
                <span className="nome-gruppo">{nomeGruppo(gruppo)}</span>
                <span className="quante">{parti.length}</span>
              </div>
              {aperto && (
                <div role="group">
                  {parti.map((p, i) => (
                    <RigaParte
                      key={p.name}
                      parte={p}
                      posto={i + 1}
                      fratelli={parti.length}
                      segni={segni.get(p.name)}
                      scelta={scelta === p.name}
                      raggiungibile={attiva === `p:${p.name}`}
                      onScegli={() => {
                        setFuoco(`p:${p.name}`);
                        scegli(p.name);
                      }}
                    />
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>

      <section className="sezione-scafale" data-aperta={scafaleAperto || undefined}>
        <button
          type="button"
          className="testa-scafale"
          aria-expanded={scafaleAperto}
          onClick={() => setScafaleAperto((a) => !a)}
        >
          <Icona nome={scafaleAperto ? "i-chev-d" : "i-chev-r"} dim={11} />
          <span>{t("studio3.shell.section")}</span>
        </button>
        {scafaleAperto && <AlberoScafale />}
      </section>
    </div>
  );
}

function RigaParte({
  parte,
  posto,
  fratelli,
  segni,
  scelta,
  raggiungibile,
  onScegli,
}: {
  parte: ParteRegistro;
  posto: number;
  fratelli: number;
  segni: Segni | undefined;
  scelta: boolean;
  raggiungibile: boolean;
  onScegli: () => void;
}) {
  return (
    <div
      role="treeitem"
      className="voce-livello"
      data-riga={`p:${parte.name}`}
      aria-level={2}
      aria-posinset={posto}
      aria-setsize={fratelli}
      aria-selected={scelta}
      tabIndex={raggiungibile ? 0 : -1}
      title={descrizioneParte(parte.name, parte.description)}
      data-ridipinta={segni?.ridipinta || undefined}
      onClick={onScegli}
    >
      <span className="punto-livello" aria-hidden="true" />
      <code className="nome-voce">{parte.name}</code>
      <span className="segni-livello">
        {segni?.variata && (
          <span className="segno" title={t("studio3.layers.mark.variant")}>
            <Icona nome="i-moon" dim={11} />
          </span>
        )}
        {segni?.animata && (
          <span className="segno" title={t("studio3.layers.mark.animated")}>
            <Icona nome="i-timeline" dim={11} />
          </span>
        )}
        {segni?.oltre && (
          <span className="segno oltre" title={t("studio3.layers.mark.over")}>
            <Icona nome="i-alert" dim={11} />
          </span>
        )}
      </span>
      {/* Segnati anche per chi non vede i segni: una parte ridipinta si legge
          «ridipinta», non solo più chiara. */}
      <span className="solo-lettori">
        {[
          segni?.ridipinta ? t("studio3.layers.mark.painted") : null,
          segni?.variata ? t("studio3.layers.mark.variant") : null,
          segni?.animata ? t("studio3.layers.mark.animated") : null,
          segni?.oltre ? t("studio3.layers.mark.over") : null,
        ]
          .filter(Boolean)
          .join(", ")}
      </span>
    </div>
  );
}
