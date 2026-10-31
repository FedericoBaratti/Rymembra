/**
 * Lo scafale: dove stanno le cose della finestra, come albero.
 *
 * # Un albero vero, anche da tastiera
 *
 * La vista Impagina dello Studio 2.x si guidava solo col puntatore: le righe
 * erano bottoni sparsi, e spostare un widget voleva dire trascinarlo. Qui è un
 * `role="tree"` con un solo punto di tabulazione e le frecce: su e giù fra le
 * righe, destra e sinistra per aprire, chiudere, scendere e risalire; Alt con
 * su e giù sposta il nodo fra i fratelli, Canc lo toglie. Il trascinamento
 * resta — le fessure fra le righe, di `usePresaPerFessure` — ed è un modo in
 * più, non l'unico.
 *
 * # I prefab si modificano davvero
 *
 * Un'istanza di prefab si scrive nel documento come riferimento, e prima una
 * modifica fatta **dentro** un'istanza spariva: il riferimento restava quello
 * di prima. Adesso la modifica va nel corpo del prefab (`prefabCambiati`), e
 * la riga lo dice prima che la si tocchi: cambia qui, cambia ovunque.
 *
 * # Si scrive dall'ultimo albero valido, e solo da quello
 *
 * L'albero viene dal nucleo, completo. Finché il testo ha errori, l'albero che
 * si vede è quello dell'ultima versione buona: modificarlo riscriverebbe lo
 * scafale da una versione vecchia, cancellando quel che nel frattempo si è
 * scritto nel codice. Allora l'albero si guarda e non si tocca, e lo dice.
 *
 * Lo stesso vale, in silenzio, per il breve tratto in cui la validazione del
 * testo di adesso è in volo: l'albero è ancora quello di prima, e un secondo
 * gesto rapido (due Canc, Alt+Giù tenuto premuto) lo riscriveva disfacendo il
 * primo. Il gesto si ignora finché non torna l'esito (`scafaleScrivibile`).
 * Nell'ispettore del nodo la guardia vale per le scelte discrete
 * (`IspezioneNodo`); i suoi cursori no: trascinati, perderebbero l'ultima
 * posizione, e le loro scritture rapide toccano tutte lo stesso campo.
 */
import { useCallback, useState, type KeyboardEvent } from "react";

import type { NodoScafale, WidgetRegistro } from "../ipc";
import { t } from "../lingue";
import { Icona } from "../parti/Icone";
import { usePresaPerFessure } from "../riordino";
import {
  conNodo,
  costoAlbero,
  nodoA,
  nomePrefabLibero,
  prefabCambiati,
  senzaNodo,
  spostato,
  versoDocumento,
  corpoPrefab,
  widgetMontati,
  widgetNuovo,
  zonaNuova,
  type Via,
} from "./albero";
import { useStudio } from "./contesto";
import { scafaleScrivibile } from "./negozio";
import { coloreCosto } from "./valori";
import { descrizioneWidget, nomeGruppo } from "./vocabolario";

/** Cosa si sta trascinando: un nodo che c'è già, o un pezzo nuovo. */
type Preso =
  | { tipo: "nodo"; via: Via }
  | { tipo: "widget"; def: WidgetRegistro }
  | { tipo: "zona"; kind: string };

const chiaveVia = (via: Via) => via.join("/");
const stessaVia = (a: Via | null, b: Via) =>
  a !== null && a.length === b.length && a.every((v, i) => v === b[i]);

function chiaveFessura(dove: Via, indice: number): string {
  return `${dove.join("/")}:${indice}`;
}

function fessuraDa(chiave: string): { dove: Via; indice: number } | null {
  const taglio = chiave.lastIndexOf(":");
  if (taglio < 0) return null;
  const indice = Number(chiave.slice(taglio + 1));
  const via = chiave.slice(0, taglio);
  const numeri = via === "" ? [] : via.split("/").map(Number);
  if (!Number.isInteger(indice) || numeri.some((n) => !Number.isInteger(n))) return null;
  return { dove: numeri, indice };
}

/** Una riga visibile dell'albero, in ordine di lettura. */
type Voce =
  | { tipo: "nodo"; nodo: NodoScafale; via: Via; livello: number; posto: number; fratelli: number }
  | { tipo: "fessura"; dove: Via; indice: number; livello: number };

/** L'albero steso in righe, saltando i figli delle zone chiuse. */
function stendi(radice: NodoScafale, chiusi: ReadonlySet<string>): Voce[] {
  const fuori: Voce[] = [];
  const visita = (nodo: NodoScafale, via: Via, livello: number, posto: number, fratelli: number) => {
    fuori.push({ tipo: "nodo", nodo, via, livello, posto, fratelli });
    if (nodo.kind !== "zone" || chiusi.has(chiaveVia(via))) return;
    fuori.push({ tipo: "fessura", dove: via, indice: 0, livello: livello + 1 });
    nodo.children.forEach((figlio, i) => {
      visita(figlio, [...via, i], livello + 1, i + 1, nodo.children.length);
      fuori.push({ tipo: "fessura", dove: via, indice: i + 1, livello: livello + 1 });
    });
  };
  visita(radice, [], 1, 1, 1);
  return fuori;
}

/**
 * Scrive un albero nel documento, coi prefab che ha cambiato: un passo solo.
 *
 * È anche quel che usa l'ispettore del nodo: due strade per scrivere lo stesso
 * albero sarebbero due modi di dimenticare i prefab.
 */
export function useScriviAlbero() {
  const { s, scriviMolti } = useStudio();
  const vecchio = s.valido?.layout?.shell ?? null;
  return useCallback(
    (nuovo: NodoScafale, etichetta: string) => {
      const operazioni = [
        { togli: false, percorso: ["layout", "shell"], valore: versoDocumento(nuovo) },
        ...[...(vecchio === null ? [] : prefabCambiati(vecchio, nuovo))].map(([nome, corpo]) => ({
          togli: false,
          percorso: ["layout", "prefabs", nome],
          valore: corpo,
        })),
      ];
      scriviMolti(operazioni, etichetta, "layout.shell");
    },
    [vecchio, scriviMolti],
  );
}

export function AlberoScafale() {
  const { s, manda, registro, documento, scriviMolti } = useStudio();
  const albero = s.valido?.layout?.shell ?? null;
  const bloccato = s.esito !== null && s.esito.errori.length > 0;
  // La guardia muta delle scritture: anche mentre la validazione è in volo
  // l'albero che si vede è vecchio, e due gesti rapidi partivano tutti e due
  // da lui — il secondo disfaceva il primo. Non tocca la nota né i tasti, che
  // dicono solo degli errori: vedi `scafaleScrivibile`.
  const scrivibile = scafaleScrivibile(s);
  const scelto = s.selezione?.tipo === "nodo" ? s.selezione.via : null;
  const [chiusi, setChiusi] = useState<ReadonlySet<string>>(new Set());
  const [preso, setPreso] = useState<Preso | null>(null);
  const [mirata, setMirata] = useState<string | null>(null);
  const [fuoco, setFuoco] = useState<string>("");
  const scriviAlbero = useScriviAlbero();
  const widgets = registro.widgets;
  const prefabs = Object.keys(
    (((documento?.["layout"] ?? {}) as Record<string, unknown>)["prefabs"] ?? {}) as object,
  );

  const scegli = (via: Via | null) =>
    manda({ tipo: "scegli", selezione: via === null ? null : { tipo: "nodo", via } });

  const presa = usePresaPerFessure<Preso>({
    onPresa: (cosa) => {
      setPreso(cosa);
      if (cosa === null) setMirata(null);
    },
    onMira: setMirata,
    onLascia: (chiave, cosa) => {
      const fessura = fessuraDa(chiave);
      if (fessura !== null) rilascia(fessura.dove, fessura.indice, cosa);
      setPreso(null);
      setMirata(null);
    },
  });

  if (albero === null) {
    return <p className="niente-scafale">{t("studio3.shell.waiting")}</p>;
  }

  const montati = widgetMontati(albero);

  /** Un pezzo ci sta in quella zona? Lo dice il registro, non questo file. */
  const accetta = (dentro: NodoScafale, cosa: Preso): boolean => {
    if (dentro.kind !== "zone") return false;
    if (cosa.tipo === "zona") return true;
    if (cosa.tipo === "nodo") {
      const nodo = nodoA(albero, cosa.via);
      if (nodo === null) return false;
      if (nodo.kind === "zone") return true;
      return widgets.find((w) => w.name === nodo.name)?.fits.includes(dentro.name) ?? false;
    }
    if (!cosa.def.fits.includes(dentro.name)) return false;
    return !(cosa.def.singleton && montati.includes(cosa.def.name));
  };

  function rilascia(dove: Via, indice: number, cosa: Preso) {
    if (albero === null || !scrivibile) return;
    const dentro = nodoA(albero, dove);
    if (dentro === null || !accetta(dentro, cosa)) return;
    if (cosa.tipo === "nodo") {
      const nodo = nodoA(albero, cosa.via);
      scriviAlbero(
        spostato(albero, cosa.via, dove, indice),
        t("studio3.step.shellMove", { cosa: nodo?.name ?? "" }),
      );
      return;
    }
    const nuovo = cosa.tipo === "widget" ? widgetNuovo(cosa.def) : zonaNuova(cosa.kind);
    const children = [...dentro.children];
    children.splice(indice, 0, nuovo);
    scriviAlbero(
      conNodo(albero, dove, { ...dentro, children }),
      t("studio3.step.shellAdd", { cosa: nuovo.name }),
    );
    scegli([...dove, indice]);
  }

  /** La zona in cui un pezzo aggiunto col clic finisce: quella scelta, o la radice. */
  const zonaDiArrivo = (): Via => {
    if (scelto === null) return [];
    const nodo = nodoA(albero, scelto);
    return nodo?.kind === "zone" ? scelto : scelto.slice(0, -1);
  };

  const aggiungi = (cosa: Preso) => {
    const dove = zonaDiArrivo();
    const dentro = nodoA(albero, dove);
    if (dentro === null) return;
    rilascia(dove, dentro.children.length, cosa);
  };

  const togli = (via: Via) => {
    const nodo = nodoA(albero, via);
    if (!scrivibile || via.length === 0 || nodo === null) return;
    scriviAlbero(senzaNodo(albero, via), t("studio3.step.shellRemove", { cosa: nodo.name }));
    scegli(via.length > 1 ? via.slice(0, -1) : null);
  };

  /** Sposta di un posto fra i fratelli; restituisce dove è arrivato, o `null`. */
  const sposta = (via: Via, verso: -1 | 1): Via | null => {
    const indice = via[via.length - 1];
    const padre = via.slice(0, -1);
    const zona = nodoA(albero, padre);
    if (!scrivibile || indice === undefined || zona === null) return null;
    const arrivo = indice + verso;
    if (arrivo < 0 || arrivo >= zona.children.length) return null;
    const nodo = nodoA(albero, via);
    // `spostato` inserisce **prima** dell'indice dato, a nodo già tolto: per
    // scendere di uno si punta due posti più in là.
    scriviAlbero(
      spostato(albero, via, padre, verso === 1 ? arrivo + 1 : arrivo),
      t("studio3.step.shellMove", { cosa: nodo?.name ?? "" }),
    );
    const dove = [...padre, arrivo];
    scegli(dove);
    return dove;
  };

  const faiPrefab = (via: Via) => {
    const nodo = nodoA(albero, via);
    if (!scrivibile || nodo === null || nodo.kind !== "zone" || nodo.fromPrefab !== null) return;
    const nome = nomePrefabLibero(nodo, prefabs);
    // Il corpo prima del riferimento, in un passo solo: al contrario, per un
    // istante il documento richiamerebbe un prefab che non c'è.
    scriviMolti(
      [
        { togli: false, percorso: ["layout", "prefabs", nome], valore: corpoPrefab(nodo) },
        {
          togli: false,
          percorso: ["layout", "shell"],
          valore: versoDocumento(conNodo(albero, via, { ...nodo, fromPrefab: nome })),
        },
      ],
      t("studio3.step.shellPrefab", { nome }),
      "layout.shell",
    );
  };

  const alterna = (via: Via) =>
    setChiusi((prima) => {
      const dopo = new Set(prima);
      const chiave = chiaveVia(via);
      if (dopo.has(chiave)) dopo.delete(chiave);
      else dopo.add(chiave);
      return dopo;
    });

  const voci = stendi(albero, chiusi);
  const nodi = voci.filter((v): v is Extract<Voce, { tipo: "nodo" }> => v.tipo === "nodo");
  const attiva = fuoco !== "" && nodi.some((v) => chiaveVia(v.via) === fuoco) ? fuoco : chiaveVia(scelto ?? []);

  const vaiA = (chiave: string) => {
    setFuoco(chiave);
    document.querySelector<HTMLElement>(`.albero-scafale [data-via="${chiave}"]`)?.focus();
  };

  const daTastiera = (e: KeyboardEvent<HTMLDivElement>) => {
    const qui = nodi.findIndex((v) => chiaveVia(v.via) === attiva);
    const voce = nodi[qui];
    if (voce === undefined) return;
    const zona = voce.nodo.kind === "zone";
    const aperta = zona && !chiusi.has(chiaveVia(voce.via));
    const altra = (indice: number) => {
      const presa = nodi[Math.max(0, Math.min(nodi.length - 1, indice))];
      if (presa !== undefined) vaiA(chiaveVia(presa.via));
    };
    let fatto = true;
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      // Il fuoco segue il nodo. Restando sulla via di partenza, dopo la
      // validazione quella riga mostrava il fratello salito al suo posto, e
      // il secondo Alt+Giù spostava lui: i due tornavano com'erano. La riga
      // d'arrivo c'è già nell'albero di adesso, ed è lo stesso elemento che
      // dopo la validazione mostra il nodo spostato.
      const arrivo = !bloccato && voce.via.length > 0 ? sposta(voce.via, e.key === "ArrowUp" ? -1 : 1) : null;
      if (arrivo !== null) vaiA(chiaveVia(arrivo));
    } else if (e.key === "ArrowDown") altra(qui + 1);
    else if (e.key === "ArrowUp") altra(qui - 1);
    else if (e.key === "Home") altra(0);
    else if (e.key === "End") altra(nodi.length - 1);
    else if (e.key === "ArrowRight") {
      if (zona && !aperta) alterna(voce.via);
      else if (aperta && voce.nodo.children.length > 0) vaiA(chiaveVia([...voce.via, 0]));
    } else if (e.key === "ArrowLeft") {
      if (aperta) alterna(voce.via);
      else if (voce.via.length > 0) vaiA(chiaveVia(voce.via.slice(0, -1)));
    } else if (e.key === "Enter" || e.key === " ") scegli(voce.via);
    else if (e.key === "Delete" || e.key === "Backspace") {
      if (!bloccato) togli(voce.via);
    } else fatto = false;
    if (fatto) e.preventDefault();
  };

  const nodoScelto = scelto === null ? null : nodoA(albero, scelto);
  const perGruppo = new Map<string, WidgetRegistro[]>();
  for (const w of widgets) perGruppo.set(w.group, [...(perGruppo.get(w.group) ?? []), w]);
  const arrivo = nodoA(albero, zonaDiArrivo());

  return (
    <div className="scafale-studio">
      <div className="testa-sezione">
        <span className="titolino">{t("studio3.shell.title")}</span>
        <span
          className="peso"
          style={{ color: coloreCosto(costoAlbero(albero, widgets), registro.shellBudget) }}
          title={t("studio3.shell.cost.hint")}
        >
          {costoAlbero(albero, widgets)} / {registro.shellBudget}
        </span>
      </div>
      {bloccato && <p className="nota-scafale">{t("studio3.shell.locked")}</p>}

      <div
        className="albero-scafale"
        role="tree"
        aria-label={t("studio3.shell.aria")}
        onKeyDown={daTastiera}
        data-preso={preso !== null || undefined}
      >
        {voci.map((voce) =>
          voce.tipo === "fessura" ? (
            <FessuraScafale
              key={`f:${chiaveFessura(voce.dove, voce.indice)}`}
              chiave={chiaveFessura(voce.dove, voce.indice)}
              livello={voce.livello}
              legale={
                preso !== null &&
                (() => {
                  const dentro = nodoA(albero, voce.dove);
                  return dentro !== null && accetta(dentro, preso);
                })()
              }
              mirata={mirata === chiaveFessura(voce.dove, voce.indice)}
            />
          ) : (
            <RigaScafale
              key={`n:${chiaveVia(voce.via)}`}
              voce={voce}
              def={widgets.find((w) => w.name === voce.nodo.name) ?? null}
              scelta={stessaVia(scelto, voce.via)}
              raggiungibile={chiaveVia(voce.via) === attiva}
              aperta={!chiusi.has(chiaveVia(voce.via))}
              budget={registro.shellBudget}
              onScegli={() => {
                setFuoco(chiaveVia(voce.via));
                scegli(voce.via);
              }}
              onAlterna={() => alterna(voce.via)}
              onPresa={
                voce.via.length > 0 && !bloccato
                  ? (e) => presa(e, { tipo: "nodo", via: voce.via })
                  : undefined
              }
            />
          ),
        )}
      </div>

      {scelto !== null && scelto.length > 0 && nodoScelto !== null && !bloccato && (
        <div className="azioni-nodo">
          <button type="button" className="pillola btn-ghost" onClick={() => sposta(scelto, -1)}>
            <Icona nome="i-chev-u" dim={13} />
            {t("studio3.shell.up")}
          </button>
          <button type="button" className="pillola btn-ghost" onClick={() => sposta(scelto, 1)}>
            <Icona nome="i-chev-d" dim={13} />
            {t("studio3.shell.down")}
          </button>
          <button type="button" className="pillola btn-ghost" onClick={() => togli(scelto)}>
            <Icona nome="i-x" dim={13} />
            {t("studio3.shell.remove")}
          </button>
          {nodoScelto.kind === "zone" && nodoScelto.fromPrefab === null && (
            <button
              type="button"
              className="pillola btn-ghost"
              title={t("studio3.shell.prefab.hint", {
                nome: nomePrefabLibero(nodoScelto, prefabs),
              })}
              onClick={() => faiPrefab(scelto)}
            >
              <Icona nome="i-layers" dim={13} />
              {t("studio3.shell.prefab")}
            </button>
          )}
        </div>
      )}

      <div className="testa-sezione">
        <span className="titolino">{t("studio3.shell.palette")}</span>
        <span className="da-dove">
          {t("studio3.shell.paletteHint", { dove: arrivo?.name ?? "" })}
        </span>
      </div>
      <div className="tavolozza-widget">
        <div className="gruppo-widget">
          <span className="titolo-gruppo">{t("studio3.shell.zones")}</span>
          {registro.vocabolario.zones.map((kind) => (
            <PezzoTavolozza
              key={kind}
              nome={kind}
              costo={0}
              budget={registro.shellBudget}
              spento={bloccato ? t("studio3.shell.locked") : null}
              titolo={t("studio3.shell.zoneHint", { nome: kind })}
              onPresa={(e) => presa(e, { tipo: "zona", kind })}
              onAggiungi={() => aggiungi({ tipo: "zona", kind })}
            />
          ))}
        </div>
        {[...perGruppo.entries()].map(([gruppo, dentro]) => (
          <div key={gruppo} className="gruppo-widget">
            <span className="titolo-gruppo">{nomeGruppo(gruppo)}</span>
            {dentro.map((w) => {
              const gia = w.singleton && montati.includes(w.name);
              const nonCi = arrivo !== null && !w.fits.includes(arrivo.name);
              return (
                <PezzoTavolozza
                  key={w.name}
                  nome={w.name}
                  costo={w.cost}
                  budget={registro.shellBudget}
                  essenziale={w.essential !== "no"}
                  spento={
                    bloccato
                      ? t("studio3.shell.locked")
                      : gia
                        ? t("studio3.shell.singleton", { nome: w.name })
                        : null
                  }
                  titolo={
                    nonCi
                      ? t("studio3.shell.fitsElsewhere", { dove: w.fits.join(", ") })
                      : descrizioneWidget(w.name, w.description)
                  }
                  nonQui={nonCi}
                  onPresa={(e) => presa(e, { tipo: "widget", def: w })}
                  onAggiungi={() => aggiungi({ tipo: "widget", def: w })}
                />
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

function FessuraScafale({
  chiave,
  livello,
  legale,
  mirata,
}: {
  chiave: string;
  livello: number;
  legale: boolean;
  mirata: boolean;
}) {
  return (
    <div
      className="fessura"
      role="none"
      aria-hidden="true"
      style={{ marginLeft: `${(livello - 1) * 12}px` }}
      // Solo le fessure legali si fanno trovare: il gesto cerca
      // `data-fessura`, e una che non c'è non si mira e non si illumina.
      data-fessura={legale ? chiave : undefined}
      data-attiva={legale || undefined}
      data-mirata={(legale && mirata) || undefined}
    />
  );
}

function RigaScafale({
  voce,
  def,
  scelta,
  raggiungibile,
  aperta,
  budget,
  onScegli,
  onAlterna,
  onPresa,
}: {
  voce: Extract<Voce, { tipo: "nodo" }>;
  def: WidgetRegistro | null;
  scelta: boolean;
  raggiungibile: boolean;
  aperta: boolean;
  budget: number;
  onScegli: () => void;
  onAlterna: () => void;
  onPresa: ((e: React.PointerEvent<HTMLElement>) => void) | undefined;
}) {
  const { nodo, via, livello, posto, fratelli } = voce;
  const zona = nodo.kind === "zone";
  return (
    <div
      role="treeitem"
      className="riga-scafale"
      data-via={chiaveVia(via)}
      aria-level={livello}
      aria-posinset={posto}
      aria-setsize={fratelli}
      aria-selected={scelta}
      aria-expanded={zona ? aperta : undefined}
      tabIndex={raggiungibile ? 0 : -1}
      data-zona={zona || undefined}
      style={{ paddingLeft: `${(livello - 1) * 12 + 4}px` }}
      title={def === null ? t("studio3.shell.zoneHint", { nome: nodo.name }) : def.description}
      onPointerDown={onPresa}
      onClick={onScegli}
    >
      {zona ? (
        <button
          type="button"
          className="apri-zona"
          tabIndex={-1}
          aria-hidden="true"
          onClick={(e) => {
            e.stopPropagation();
            onAlterna();
          }}
        >
          <Icona nome={aperta ? "i-chev-d" : "i-chev-r"} dim={11} />
        </button>
      ) : (
        <Icona nome="i-grip" dim={11} />
      )}
      <code className="nome-voce">{nodo.name}</code>
      {nodo.part !== null && zona && <span className="parte-nodo">.{nodo.part}</span>}
      {nodo.fromPrefab !== null && (
        <span className="da-prefab" title={t("studio3.shell.fromPrefab", { nome: nodo.fromPrefab })}>
          {nodo.fromPrefab}
        </span>
      )}
      {def !== null && def.essential !== "no" && (
        <span className="essenziale" title={t("studio3.shell.essential", { quale: def.essential })}>
          •
        </span>
      )}
      <span className="misura">{nodo.size}</span>
      {def !== null && def.cost > 0 && (
        <span className="peso" style={{ color: coloreCosto(def.cost, budget) }}>
          {def.cost}
        </span>
      )}
    </div>
  );
}

function PezzoTavolozza({
  nome,
  costo,
  budget,
  essenziale = false,
  spento,
  titolo,
  nonQui = false,
  onPresa,
  onAggiungi,
}: {
  nome: string;
  costo: number;
  budget: number;
  essenziale?: boolean;
  /** Perché non si può prendere, o `null`. */
  spento: string | null;
  titolo: string;
  /** Non ci sta nella zona scelta: si trascina altrove, ma il clic non aggiunge. */
  nonQui?: boolean;
  onPresa: (e: React.PointerEvent<HTMLElement>) => void;
  onAggiungi: () => void;
}) {
  return (
    <button
      type="button"
      className="un-widget"
      data-spento={spento !== null || undefined}
      aria-disabled={spento !== null || nonQui || undefined}
      title={spento ?? titolo}
      onPointerDown={spento === null ? onPresa : undefined}
      onClick={() => {
        if (spento === null && !nonQui) onAggiungi();
      }}
    >
      <Icona nome={costo === 0 ? "i-list" : "i-grip"} dim={12} />
      <code>{nome}</code>
      {essenziale && <span className="essenziale">•</span>}
      {costo > 0 && (
        <span className="peso" style={{ color: coloreCosto(costo, budget) }}>
          {costo}
        </span>
      )}
    </button>
  );
}
