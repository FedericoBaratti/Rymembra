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

/** Cosa si sta trascinando: un nodo che c'è già, o un widget dalla tavolozza. */
type Preso =
  | { tipo: "nodo"; via: Via }
  | { tipo: "widget"; def: WidgetRegistro }
  | { tipo: "zona"; kind: string };

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
  const widgets = registro?.widgets ?? [];

  if (albero === null) {
    return (
      <aside className="scafale-albero">
        <p className="niente">Lo scafale arriva col primo esito valido.</p>
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

  const rilascia = (dove: Via, indice: number) => {
    if (preso === null) return;
    const dentro = nodoA(albero, dove);
    if (dentro === null || !accetta(dentro, preso)) return;
    if (preso.tipo === "nodo") {
      onAlbero(spostato(albero, preso.via, dove, indice));
    } else {
      const nuovo =
        preso.tipo === "widget" ? widgetNuovo(preso.def) : zonaNuova(preso.kind);
      const children = [...dentro.children];
      children.splice(indice, 0, nuovo);
      onAlbero(conNodo(albero, dove, { ...dentro, children }));
      onScegli([...dove, indice]);
    }
    setPreso(null);
  };

  /** Una fessura fra due fratelli, o in testa e in coda a una zona. */
  const Fessura = ({ dove, indice }: { dove: Via; indice: number }) => {
    const dentro = nodoA(albero, dove);
    const legale = preso !== null && dentro !== null && accetta(dentro, preso);
    return (
      <div
        className="fessura"
        data-attiva={legale || undefined}
        onDragOver={(e) => {
          if (!legale) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = preso?.tipo === "nodo" ? "move" : "copy";
        }}
        onDrop={(e) => {
          e.preventDefault();
          rilascia(dove, indice);
        }}
      />
    );
  };

  const Riga = ({ nodo, via }: { nodo: NodoScafale; via: Via }) => {
    const def = widgets.find((w) => w.name === nodo.name);
    const costo = def?.cost ?? 0;
    const qui = scelto !== null && scelto.length === via.length && scelto.every((v, i) => v === via[i]);
    return (
      <div className="ramo" style={{ paddingLeft: `${via.length * 12}px` }}>
        <button
          type="button"
          className="nodo-riga voce-registro"
          data-active={qui || undefined}
          data-zona={nodo.kind === "zone" || undefined}
          draggable={via.length > 0}
          title={def?.description ?? `Zona ${nodo.name}`}
          onDragStart={(e) => {
            setPreso({ tipo: "nodo", via });
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragEnd={() => setPreso(null)}
          onClick={() => onScegli(via)}
        >
          <Icona nome={nodo.kind === "zone" ? "i-list" : "i-grip"} dim={12} />
          <code className="nome-voce">{nodo.name}</code>
          {/* Da un prefab: modificarlo qui muove **tutti** gli usi, e dirlo
              prima vale più che scoprirlo dopo. */}
          {nodo.fromPrefab !== null && (
            <span className="da-prefab" title={`dal prefab «${nodo.fromPrefab}»`}>
              {nodo.fromPrefab}
            </span>
          )}
          {/* Il pallino dell'essenziale: senza, togliere il widget sbagliato si
              scopre dal validatore invece che dall'albero. */}
          {def && def.essential !== "no" && (
            <span className="essenziale" title={`essenziale · ${def.essential}`}>
              •
            </span>
          )}
          <span className="misura">{nodo.size}</span>
          {costo > 0 && (
            <span className="peso" style={{ color: coloreCosto(costo, registro?.shellBudget ?? 24) }}>
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
        <span className="titolino">Struttura</span>
        <span className="da-dove">
          {costoAlbero(albero, widgets)} / {registro?.shellBudget ?? 24}
        </span>
      </div>

      <div className="albero-scafale">
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
            Togli il nodo
          </button>
          {/* Solo le zone: un prefab è un **sottoalbero** nominato, e nominare
              un widget solo sarebbe dargli un secondo nome per niente. Non uno
              che viene già da un prefab: sarebbe un prefab dentro un prefab. */}
          {nodoA(albero, scelto)?.kind === "zone" &&
            nodoA(albero, scelto)?.fromPrefab === null && (
              <button
                type="button"
                className="pillola btn-ghost"
                title={`Diventa «${nomePrefabLibero(
                  nodoA(albero, scelto) ?? zonaNuova("row"),
                  prefabs,
                )}» in layout.prefabs, e qui resta un riferimento`}
                onClick={() => onPrefab(scelto)}
              >
                <Icona nome="i-list" dim={13} />
                Fanne un prefab
              </button>
            )}
        </div>
      )}

      <div className="testa-sezione">
        <span className="titolino">Tavolozza</span>
        <span className="da-dove">trascina in una fessura</span>
      </div>

      <div className="tavolozza-widget">
        {/* Le zone prima dei widget: sono il contenitore, e chi costruisce un
            layout ne mette una prima di riempirla. */}
        <div className="gruppo-widget">
          <span className="titolo-gruppo">Zone</span>
          {(registro?.vocabolario.zones ?? []).map((kind) => (
            <div
              key={kind}
              className="un-widget"
              draggable
              onDragStart={(e) => {
                setPreso({ tipo: "zona", kind });
                e.dataTransfer.effectAllowed = "copy";
              }}
              onDragEnd={() => setPreso(null)}
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
                  draggable={!gia}
                  title={
                    gia
                      ? `«${w.name}» può stare in un posto solo, ed è già montato`
                      : `${w.description} · ci sta in ${w.fits.join(" o ")}`
                  }
                  onDragStart={(e) => {
                    setPreso({ tipo: "widget", def: w });
                    e.dataTransfer.effectAllowed = "copy";
                  }}
                  onDragEnd={() => setPreso(null)}
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
        <p className="niente">
          Scegli un nodo nell&apos;albero, o trascina un widget in una fessura.
        </p>
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
        <code className="quale">{nodo.name}</code>
        <span className="gruppo">{nodo.kind === "zone" ? "zona" : def?.group ?? "widget"}</span>
      </header>

      {def && <p className="a-cosa-serve">{def.description}</p>}

      <div className="campo-ispettore">
        <span className="titolino">Misura</span>
        <Segmentato
          etichetta="Quanto spazio prende"
          scelta={nodo.size === "hug" || nodo.size === "fill" ? nodo.size : "fissa"}
          onScegli={(scelta) =>
            cambia("size", scelta === "fissa" ? "240px" : scelta)
          }
          classe="minuto"
          voci={[
            { chiave: "hug", etichetta: "Hug" },
            { chiave: "fill", etichetta: "Fill" },
            { chiave: "fissa", etichetta: "Fissa" },
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
            <span className="titolino">Verso</span>
            <Segmentato
              etichetta="In che verso dispone"
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
            <span className="titolino">Aria</span>
            <Segmentato
              etichetta="Quanta aria fra i figli"
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
            <span className="titolino">Allineamento</span>
            <Segmentato
              etichetta="Come allinea sull'asse trasverso"
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
            <span className="titolino">Distribuzione</span>
            <Segmentato
              etichetta="Come distribuisce sull'asse principale"
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
            <span className="titolino">Parte ridipingibile</span>
            {/* Un elenco chiuso e non un campo di testo: il nome deve venire dal
                registro, e un refuso qui sarebbe un errore di validazione al
                posto di una scelta che non si poteva sbagliare. */}
            <select
              className="scelta field-input"
              value={nodo.part ?? ""}
              onChange={(e) => cambia("part", e.target.value === "" ? null : e.target.value)}
            >
              <option value="">nessuna</option>
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
          <span className="titolino">Manopole</span>
          {def.options.map((o) => {
            const valore = nodo.options[o.name] ?? o.default;
            const scriviOpzione = (v: boolean | string | number) =>
              cambia("options", { ...nodo.options, [o.name]: v });

            if (o.kind === "flag") {
              return (
                <label key={o.name} className="interruttore" title={o.description}>
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
                <div key={o.name} className="manopola" title={o.description}>
                  <span className="nome-manopola">{o.name}</span>
                  <Segmentato
                    etichetta={o.description}
                    scelta={String(valore)}
                    onScegli={scriviOpzione}
                    classe="minuto"
                    voci={o.allowed.map((a) => ({ chiave: a, etichetta: a }))}
                  />
                </div>
              );
            }
            return (
              <label key={o.name} className="manopola" title={o.description}>
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
