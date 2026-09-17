/**
 * La vista «Impagina»: l'albero, la tavolozza, l'ispettore del nodo.
 *
 * # I bersagli sono le fessure, non le righe
 *
 * È l'unico punto in cui il trascinamento di `Livelli.tsx` va migliorato invece
 * che copiato. Là si rilascia su un indice di un array piatto, e ha una lettura
 * sola: «prima di questo». Qui i nodi sono annidati, e rilasciare su una *zona*
 * ne ha due — dentro, o prima di essa? Le fessure di sei pixel fra i fratelli e
 * in testa e in coda a ogni zona tolgono l'ambiguità senza una libreria: una
 * fessura è un posto, e un posto è un indice dentro una zona precisa.
 *
 * # Col puntatore, non col trascinamento del browser
 *
 * Per la ragione di `riordino.ts`: dentro la finestra di Aether `dragover` e
 * `drop` non arrivano mai, e le fessure si accendevano senza che rilasciare
 * facesse niente. Il gesto è [`usePresaPerFessure`], che di questo file sa solo
 * che ogni fessura porta una chiave in `data-fessura`.
 *
 * # I rilasci illegali non si illuminano
 *
 * Un widget che non ci sta in una zona, o un singleton già montato, non accende
 * la fessura: l'interfaccia non offre l'errore, come non offre un campo di testo
 * libero dove il formato vuole un valore chiuso. È la prima regola dello Studio,
 * applicata alla struttura invece che ai valori.
 */
import { useState } from "react";

import type { NodoScafale, Registro, WidgetRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { coloreCosto } from "./valori";
import {
  conNodo,
  costoAlbero,
  nodoA,
  nomePrefabLibero,
  senzaNodo,
  spostato,
  widgetMontati,
  widgetNuovo,
  zonaNuova,
  type Via,
} from "./albero";
import { t } from "../lingue";
import { usePresaPerFessure } from "../riordino";
import { descrizioneOpzione, descrizioneWidget } from "./vocabolario";

/** Cosa si sta trascinando: un nodo che c'è già, o un widget dalla tavolozza. */
type Preso =
  | { tipo: "nodo"; via: Via }
  | { tipo: "widget"; def: WidgetRegistro }
  | { tipo: "zona"; kind: string };

/**
 * Il nome di una fessura: la via della zona, e il posto dentro di lei.
 *
 * Una stringa perché è quel che sta in un attributo del DOM, ed è da lì che il
 * gesto la ripesca. I due pezzi non si confondono: la via è fatta di numeri
 * separati da `/`, il posto viene dopo l'unico `:`.
 */
function chiaveFessura(dove: Via, indice: number): string {
  return `${dove.join("/")}:${indice}`;
}

/** La fessura che quella chiave nomina, o `null` se non la si legge. */
function fessuraDa(chiave: string): { dove: Via; indice: number } | null {
  const taglio = chiave.lastIndexOf(":");
  if (taglio < 0) return null;
  const indice = Number(chiave.slice(taglio + 1));
  const via = chiave.slice(0, taglio);
  const numeri = via === "" ? [] : via.split("/").map(Number);
  if (!Number.isInteger(indice) || numeri.some((n) => !Number.isInteger(n))) {
    return null;
  }
  return { dove: numeri, indice };
}

export function Scafale({
  albero,
  registro,
  scelto,
  prefabs,
  onScegli,
  onAlbero,
  onPrefab,
}: {
  albero: NodoScafale | null;
  registro: Registro | null;
  scelto: Via | null;
  /** I prefab già dichiarati, per non proporre un nome preso. */
  prefabs: readonly string[];
  onScegli: (via: Via | null) => void;
  /** L'albero nuovo. Chi lo riceve riscrive `layout.shell` intero. */
  onAlbero: (albero: NodoScafale) => void;
  /** Solleva un sottoalbero in `layout.prefabs` e lascia un riferimento. */
  onPrefab: (via: Via) => void;
}) {
  const [preso, setPreso] = useState<Preso | null>(null);
  /** La fessura sotto il puntatore, per accendere quella e non tutte. */
  const [mirata, setMirata] = useState<string | null>(null);
  const widgets = registro?.widgets ?? [];

  if (albero === null) {
    return (
      <aside className="scafale-albero">
        <p className="niente">{t("studio.shelf.later")}</p>
      </aside>
    );
  }

  const montati = widgetMontati(albero);

  /** Un rilascio è legale? La risposta la dà il registro, non questo file. */
  const accetta = (dentro: NodoScafale, cosa: Preso): boolean => {
    if (dentro.kind !== "zone") return false;
    if (cosa.tipo === "zona") return true;
    if (cosa.tipo === "nodo") {
      const nodo = nodoA(albero, cosa.via);
      if (nodo === null) return false;
      if (nodo.kind === "zone") return true;
      const def = widgets.find((w) => w.name === nodo.name);
      return def?.fits.includes(dentro.name) ?? false;
    }
    if (!cosa.def.fits.includes(dentro.name)) return false;
    return !(cosa.def.singleton && montati.includes(cosa.def.name));
  };

  const rilascia = (dove: Via, indice: number, cosa: Preso) => {
    const dentro = nodoA(albero, dove);
    if (dentro === null || !accetta(dentro, cosa)) return;
    if (cosa.tipo === "nodo") {
      onAlbero(spostato(albero, cosa.via, dove, indice));
    } else {
      const nuovo =
        cosa.tipo === "widget" ? widgetNuovo(cosa.def) : zonaNuova(cosa.kind);
      const children = [...dentro.children];
      children.splice(indice, 0, nuovo);
      onAlbero(conNodo(albero, dove, { ...dentro, children }));
      onScegli([...dove, indice]);
    }
  };

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

  /** Una fessura fra due fratelli, o in testa e in coda a una zona. */
  const Fessura = ({ dove, indice }: { dove: Via; indice: number }) => {
    const dentro = nodoA(albero, dove);
    const legale = preso !== null && dentro !== null && accetta(dentro, preso);
    const chiave = chiaveFessura(dove, indice);
    return (
      <div
        className="fessura"
        // Solo le fessure legali si fanno trovare: il gesto cerca
        // `data-fessura`, e una che non c'è non si mira e non si illumina.
        data-fessura={legale ? chiave : undefined}
        data-attiva={legale || undefined}
        data-mirata={(legale && mirata === chiave) || undefined}
      />
    );
  };

  const Riga = ({ nodo, via }: { nodo: NodoScafale; via: Via }) => {
    const def = widgets.find((w) => w.name === nodo.name);
    const costo = def?.cost ?? 0;
    const qui = scelto !== null && scelto.length === via.length && scelto.every((v, i) => v === via[i]);
    return (
      <div style={{ paddingLeft: `${via.length * 12}px` }}>
        <button
          type="button"
          className="nodo-riga voce-registro"
          data-active={qui || undefined}
          data-zona={nodo.kind === "zone" || undefined}
          title={
            def?.description ?? t("studio.shelf.zone", { nome: nodo.name })
          }
          // La radice no: non ha un posto dove andare, e non si può portare
          // dentro sé stessa.
          onPointerDown={
            via.length > 0
              ? (e) => presa(e, { tipo: "nodo", via })
              : undefined
          }
          onClick={() => onScegli(via)}
        >
          <Icona nome={nodo.kind === "zone" ? "i-list" : "i-grip"} dim={12} />
          <code className="nome-voce">{nodo.name}</code>
          {/* Da un prefab: modificarlo qui muove **tutti** gli usi, e dirlo
              prima vale più che scoprirlo dopo. */}
          {nodo.fromPrefab !== null && (
            <span
              className="da-prefab"
              title={t("studio.shelf.fromPrefab", { nome: nodo.fromPrefab })}
            >
              {nodo.fromPrefab}
            </span>
          )}
          {/* Il pallino dell'essenziale: senza, togliere il widget sbagliato si
              scopre dal validatore invece che dall'albero. */}
          {def && def.essential !== "no" && (
            <span
              className="essenziale"
              title={t("studio.shelf.essential", { quale: def.essential })}
            >
              •
            </span>
          )}
          <span className="misura">{nodo.size}</span>
          {costo > 0 && (
            <span
              className="peso"
              style={{ color: coloreCosto(costo, registro?.shellBudget ?? 24) }}
            >
              {costo}
            </span>
          )}
        </button>

        {nodo.kind === "zone" && (
          <div className="dentro-zona">
            <Fessura dove={via} indice={0} />
            {nodo.children.map((figlio, i) => (
              <div key={`${figlio.kind}-${figlio.name}-${i}`}>
                <Riga nodo={figlio} via={[...via, i]} />
                <Fessura dove={via} indice={i + 1} />
              </div>
            ))}
          </div>
        )}
      </div>
    );
  };

  const perGruppo = new Map<string, WidgetRegistro[]>();
  for (const w of widgets) {
    const dentro = perGruppo.get(w.group) ?? [];
    dentro.push(w);
    perGruppo.set(w.group, dentro);
  }

  return (
    <aside className="scafale-albero">
      <div className="testa-sezione">
        <span className="titolino">{t("studio.shelf.structure")}</span>
        <span className="da-dove">
          {costoAlbero(albero, widgets)} / {registro?.shellBudget ?? 24}
        </span>
      </div>

      <div>
        <Riga nodo={albero} via={[]} />
      </div>

      {scelto !== null && scelto.length > 0 && (
        <div className="azioni-nodo">
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={() => {
              onAlbero(senzaNodo(albero, scelto));
              onScegli(null);
            }}
          >
            <Icona nome="i-x" dim={13} />
            {t("studio.shelf.removeNode")}
          </button>
          {/* Solo le zone: un prefab è un **sottoalbero** nominato, e nominare
              un widget solo sarebbe dargli un secondo nome per niente. Non uno
              che viene già da un prefab: sarebbe un prefab dentro un prefab. */}
          {nodoA(albero, scelto)?.kind === "zone" &&
            nodoA(albero, scelto)?.fromPrefab === null && (
              <button
                type="button"
                className="pillola btn-ghost"
                title={t("studio.shelf.makePrefab.why", {
                  nome: nomePrefabLibero(
                    nodoA(albero, scelto) ?? zonaNuova("row"),
                    prefabs,
                  ),
                })}
                onClick={() => onPrefab(scelto)}
              >
                <Icona nome="i-list" dim={13} />
                {t("studio.shelf.makePrefab")}
              </button>
            )}
        </div>
      )}

      <div className="testa-sezione">
        <span className="titolino">{t("studio.shelf.palette")}</span>
        <span className="da-dove">{t("studio.shelf.dragHint")}</span>
      </div>

      <div className="tavolozza-widget">
        {/* Le zone prima dei widget: sono il contenitore, e chi costruisce un
            layout ne mette una prima di riempirla. */}
        <div className="gruppo-widget">
          <span className="titolo-gruppo">{t("studio.shelf.zones")}</span>
          {(registro?.vocabolario.zones ?? []).map((kind) => (
            <div
              key={kind}
              className="un-widget"
              onPointerDown={(e) => presa(e, { tipo: "zona", kind })}
            >
              <Icona nome="i-list" dim={12} />
              <code>{kind}</code>
            </div>
          ))}
        </div>

        {[...perGruppo.entries()].map(([gruppo, dentro]) => (
          <div key={gruppo} className="gruppo-widget">
            <span className="titolo-gruppo">{gruppo}</span>
            {dentro.map((w) => {
              // Un singleton già montato è disabilitato **con la ragione nel
              // suggerimento**: spento e muto insegnerebbe solo che l'editor a
              // volte non risponde.
              const gia = w.singleton && montati.includes(w.name);
              return (
                <div
                  key={w.name}
                  className="un-widget"
                  data-spento={gia || undefined}
                  title={
                    gia
                      ? t("studio.shelf.singleton", { nome: w.name })
                      : t("studio.shelf.fits", {
                          descrizione: descrizioneWidget(w.name, w.description),
                          dove: w.fits.join(t("studio.shelf.or")),
                        })
                  }
                  onPointerDown={
                    gia ? undefined : (e) => presa(e, { tipo: "widget", def: w })
                  }
                >
                  <Icona nome="i-grip" dim={12} />
                  <code>{w.name}</code>
                  {w.essential !== "no" && <span className="essenziale">•</span>}
                  {/* Il costo **prima** di spenderlo: la terza regola dello
                      Studio, applicata allo scafale. */}
                  <span
                    className="peso"
                    style={{ color: coloreCosto(w.cost, registro?.shellBudget ?? 24) }}
                  >
                    {w.cost}
                  </span>
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </aside>
  );
}

/**
 * L'ispettore di un nodo.
 *
 * Riusa i controlli dell'ispettore delle parti alla lettera: un segmentato per
 * ogni enum, una casella per ogni bandiera, un elenco chiuso per la parte. È il
 * dividendo della disciplina «una forma di controllo per tipo del crate» — la
 * vista nuova non ha avuto bisogno di inventarne uno.
 */
export function IspettoreNodo({
  albero,
  via,
  registro,
  onAlbero,
}: {
  albero: NodoScafale | null;
  via: Via | null;
  registro: Registro | null;
  onAlbero: (albero: NodoScafale) => void;
}) {
  const nodo = albero !== null && via !== null ? nodoA(albero, via) : null;

  if (albero === null || via === null || nodo === null) {
    return (
      <aside className="ispettore">
        <p className="niente">{t("studio.shelf.pickNode")}</p>
      </aside>
    );
  }

  const cambia = (campo: string, valore: unknown) =>
    onAlbero(conNodo(albero, via, { ...nodo, [campo]: valore }));

  const def = registro?.widgets.find((w) => w.name === nodo.name);
  const parti = registro?.parts ?? [];

  return (
    <aside className="ispettore">
      <header className="testa-ispettore">
        <code className="nome-parte">{nodo.name}</code>
        <span className="gruppo-parte">
          {nodo.kind === "zone"
            ? t("studio.shelf.zoneWord")
            : (def?.group ?? "widget")}
        </span>
        {/* Dentro la testata e non dopo: `.testa-ispettore .descrizione` è
            un discendente, e un paragrafo fuori restava senza regola. */}
        {def && (
          <p className="descrizione">
            {descrizioneWidget(def.name, def.description)}
          </p>
        )}
      </header>

      <div className="campo-ispettore">
        <span className="titolino">{t("studio.shelf.size")}</span>
        <Segmentato
          etichetta={t("studio.shelf.size.how")}
          scelta={
            nodo.size === "hug" || nodo.size === "fill" ? nodo.size : "fissa"
          }
          onScegli={(scelta) =>
            cambia("size", scelta === "fissa" ? "240px" : scelta)
          }
          classe="minuto"
          voci={[
            { chiave: "hug", etichetta: "Hug" },
            { chiave: "fill", etichetta: "Fill" },
            { chiave: "fissa", etichetta: t("studio.shelf.size.fixed") },
          ]}
        />
        {nodo.size !== "hug" && nodo.size !== "fill" && (
          <label className="cursore-campo">
            <input
              type="range"
              className="scorrimento range-accent"
              min={24}
              max={480}
              step={2}
              value={Number.parseFloat(nodo.size) || 240}
              onChange={(e) => cambia("size", `${e.target.value}px`)}
            />
            <code>{nodo.size}</code>
          </label>
        )}
      </div>

      {nodo.kind === "zone" && (
        <>
          <div className="campo-ispettore">
            <span className="titolino">{t("studio.shelf.direction")}</span>
            <Segmentato
              etichetta={t("studio.shelf.direction.how")}
              scelta={nodo.name}
              onScegli={(scelta) => cambia("name", scelta)}
              classe="minuto"
              voci={(registro?.vocabolario.zones ?? []).map((z) => ({
                chiave: z,
                etichetta: z,
              }))}
            />
          </div>

          <div className="campo-ispettore">
            <span className="titolino">{t("studio.shelf.gap")}</span>
            <Segmentato
              etichetta={t("studio.shelf.gap.how")}
              scelta={nodo.gap ?? "none"}
              onScegli={(scelta) => cambia("gap", scelta)}
              classe="minuto"
              voci={(registro?.vocabolario.gaps ?? []).map((g) => ({
                chiave: g,
                etichetta: g,
              }))}
            />
          </div>

          <div className="campo-ispettore">
            <span className="titolino">{t("studio.shelf.align")}</span>
            <Segmentato
              etichetta={t("studio.shelf.align.how")}
              scelta={nodo.align ?? "stretch"}
              onScegli={(scelta) => cambia("align", scelta)}
              classe="minuto"
              voci={(registro?.vocabolario.aligns ?? []).map((a) => ({
                chiave: a,
                etichetta: a,
              }))}
            />
          </div>

          <div className="campo-ispettore">
            <span className="titolino">{t("studio.shelf.spread")}</span>
            <Segmentato
              etichetta={t("studio.shelf.spread.how")}
              scelta={nodo.spread ?? "start"}
              onScegli={(scelta) => cambia("spread", scelta)}
              classe="minuto"
              voci={(registro?.vocabolario.spreads ?? []).map((s) => ({
                chiave: s,
                etichetta: s,
              }))}
            />
          </div>

          <div className="campo-ispettore">
            <span className="titolino">{t("studio.shelf.part")}</span>
            {/* Un elenco chiuso e non un campo di testo: il nome deve venire dal
                registro, e un refuso qui sarebbe un errore di validazione al
                posto di una scelta che non si poteva sbagliare. */}
            <select
              className="scelta field-input"
              value={nodo.part ?? ""}
              onChange={(e) =>
                cambia("part", e.target.value === "" ? null : e.target.value)
              }
            >
              <option value="">{t("studio.shelf.part.none")}</option>
              {parti.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
        </>
      )}

      {def && def.options.length > 0 && (
        <div className="campo-ispettore">
          <span className="titolino">{t("studio.shelf.knobs")}</span>
          {def.options.map((o) => {
            const valore = nodo.options[o.name] ?? o.default;
            const scriviOpzione = (v: boolean | string | number) =>
              cambia("options", { ...nodo.options, [o.name]: v });

            if (o.kind === "flag") {
              return (
                <label
                  key={o.name}
                  className="interruttore"
                  title={descrizioneOpzione(nodo.name, o.name, o.description)}
                >
                  <input
                    type="checkbox"
                    checked={valore === true}
                    onChange={(e) => scriviOpzione(e.target.checked)}
                  />
                  <span>{o.name}</span>
                </label>
              );
            }
            if (o.kind === "word") {
              return (
                <div
                  key={o.name}
                  className="manopola"
                  title={descrizioneOpzione(nodo.name, o.name, o.description)}
                >
                  <span className="nome-manopola">{o.name}</span>
                  <Segmentato
                    etichetta={descrizioneOpzione(nodo.name, o.name, o.description)}
                    scelta={String(valore)}
                    onScegli={scriviOpzione}
                    classe="minuto"
                    voci={o.allowed.map((a) => ({ chiave: a, etichetta: a }))}
                  />
                </div>
              );
            }
            return (
              <label
                key={o.name}
                className="manopola"
                title={descrizioneOpzione(nodo.name, o.name, o.description)}
              >
                <span className="nome-manopola">{o.name}</span>
                <input
                  type="range"
                  className="scorrimento range-accent"
                  min={o.min ?? 0}
                  max={o.max ?? 10}
                  step={1}
                  value={Number(valore)}
                  onChange={(e) => scriviOpzione(Number(e.target.value))}
                />
                <code>{String(valore)}</code>
              </label>
            );
          })}
        </div>
      )}
    </aside>
  );
}
