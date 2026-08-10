/**
 * L'ispettore: una superficie alla volta, e solo quel che il formato accetta.
 *
 * # La prima regola, resa in controlli
 *
 * «Non si può scrivere quel che il formato non accetta.» Finora l'ispettore
 * aveva quattro campi di testo, e tre di quei quattro accettavano qualunque cosa
 * — compresa una che la validazione avrebbe rifiutato un istante dopo. Qui ogni
 * riga ha la forma del suo tipo: un cursore per una lunghezza, un segmentato per
 * un enum, un elenco chiuso per un riferimento. L'unico posto dove si scrive
 * liberamente resta la scheda «Documento», dove a rispondere è il parser.
 *
 * # Il colore è la riga difficile
 *
 * `ColorValue` ha tre forme — un letterale, `{ "$token": … }`, `{ "$palette": …,
 * "alpha": … }` — e sono tre cose diverse, non tre scritture della stessa. Un
 * campo di testo le accetterebbe tutte e tre e anche una quarta che non esiste;
 * un selettore di colore da solo saprebbe fare solo la prima, e perderebbe
 * proprio quella che tiene insieme una skin: riferirsi al token invece di
 * ricopiarne il valore.
 *
 * Perciò tre modi espliciti, e in due dei tre l'elenco è chiuso.
 */
import type { EffettoRegistro, ParteRegistro, TokenRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { Livelli } from "./Livelli";
import { coloreCosto, costoDi, costoPila, type Livello, tinta } from "./valori";

/** Gli stati di una parte, come li chiama il registro. */
export const STATI = [
  [null, "Base"],
  ["hover", "Passaggio"],
  ["active", "Attivo"],
  ["focus", "Fuoco"],
  ["disabled", "Spento"],
] as const;

/** Gli angoli che un `chamfer` può tagliare. */
const ANGOLI = ["topLeft", "topRight", "bottomLeft", "bottomRight"] as const;

/** In che forma è scritto un colore. */
type Modo = "niente" | "token" | "tavolozza" | "letterale";

function modoDi(valore: unknown): Modo {
  if (typeof valore === "string") return "letterale";
  if (valore !== null && typeof valore === "object") {
    if ("$token" in valore) return "token";
    if ("$palette" in valore) return "tavolozza";
  }
  return "niente";
}

/**
 * Un colore, in una delle tre forme che il documento accetta.
 *
 * Cambiare modo non conserva il valore: passare da un token a un letterale
 * riparte da una scelta esplicita invece di travasare `var(--accent)` dentro un
 * campo che vuole `#c96a2e`. Un travaso silenzioso produrrebbe un documento che
 * non si valida e nessuno saprebbe perché.
 */
function ValoreColore({
  valore,
  tokens,
  tavolozza,
  onCambia,
}: {
  valore: unknown;
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  onCambia: (valore: unknown) => void;
}) {
  const modo = modoDi(valore);
  const dipinto = tinta(valore, tokens, tavolozza);
  const colori = tokens.filter((t) => t.kind === "color");
  const nomiTavolozza = Object.keys(tavolozza);

  const cambiaModo = (prossimo: Modo) => {
    switch (prossimo) {
      case "niente":
        onCambia(null);
        break;
      case "token":
        onCambia({ $token: colori[0]?.id ?? "color.accent" });
        break;
      case "tavolozza":
        // Senza colori in tavolozza il modo non si può scegliere: l'opzione è
        // spenta, e questo ramo non si raggiunge.
        onCambia({ $palette: nomiTavolozza[0] ?? "" });
        break;
      case "letterale":
        onCambia("#000000");
        break;
    }
  };

  return (
    <div className="valore-colore">
      <span
        className="pastiglia"
        style={dipinto === null ? undefined : { background: dipinto }}
        data-vuota={dipinto === null || undefined}
        aria-hidden="true"
      />
      <select
        className="modo field-input"
        aria-label="In che forma"
        value={modo}
        onChange={(e) => cambiaModo(e.target.value as Modo)}
      >
        <option value="niente">non dichiarato</option>
        <option value="token">$token</option>
        <option value="tavolozza" disabled={nomiTavolozza.length === 0}>
          $palette
        </option>
        <option value="letterale">valore</option>
      </select>

      {modo === "token" && (
        <select
          className="quale field-input"
          aria-label="Quale token"
          value={String((valore as Record<string, unknown>)["$token"] ?? "")}
          onChange={(e) => onCambia({ $token: e.target.value })}
        >
          {colori.map((t) => (
            <option key={t.id} value={t.id}>
              {t.id}
            </option>
          ))}
        </select>
      )}

      {modo === "tavolozza" && (
        <select
          className="quale field-input"
          aria-label="Quale colore della tavolozza"
          value={String((valore as Record<string, unknown>)["$palette"] ?? "")}
          onChange={(e) => {
            const alpha = (valore as Record<string, unknown>)["alpha"];
            onCambia(
              typeof alpha === "number"
                ? { $palette: e.target.value, alpha }
                : { $palette: e.target.value },
            );
          }}
        >
          {nomiTavolozza.map((nome) => (
            <option key={nome} value={nome}>
              {nome}
            </option>
          ))}
        </select>
      )}

      {modo === "letterale" && (
        <input
          className="quale field-input"
          type="color"
          aria-label="Il colore"
          value={typeof valore === "string" && valore.startsWith("#") ? valore : "#000000"}
          onChange={(e) => onCambia(e.target.value)}
        />
      )}
    </div>
  );
}

/** Una lunghezza in pixel, col cursore e la lettura. */
function Cursore({
  valore,
  min,
  max,
  passo,
  unita,
  etichetta,
  onCambia,
}: {
  valore: unknown;
  min: number;
  max: number;
  passo: number;
  /** `px` per una lunghezza, `""` per un numero puro. */
  unita: string;
  etichetta: string;
  onCambia: (valore: unknown) => void;
}) {
  const numero =
    typeof valore === "number"
      ? valore
      : typeof valore === "string"
        ? Number.parseFloat(valore)
        : Number.NaN;
  const dichiarato = Number.isFinite(numero);

  return (
    <div className="cursore">
      <input
        type="range"
        aria-label={etichetta}
        min={min}
        max={max}
        step={passo}
        value={dichiarato ? numero : min}
        onChange={(e) => {
          const preso = Number.parseFloat(e.target.value);
          onCambia(unita === "" ? preso : `${preso}${unita}`);
        }}
      />
      <span className="lettura">
        {dichiarato ? `${numero}${unita}` : "—"}
      </span>
      <button
        type="button"
        className="sgancia icon-btn"
        aria-label={`Togli ${etichetta}`}
        disabled={!dichiarato}
        onClick={() => onCambia(null)}
      >
        <Icona nome="i-x" dim={11} />
      </button>
    </div>
  );
}

/** Una riga della tabella delle proprietà. */
function Riga({ che, children }: { che: string; children: React.ReactNode }) {
  return (
    <div className="proprieta-riga">
      <span className="che">{che}</span>
      <div className="come">{children}</div>
    </div>
  );
}

export function Ispettore({
  definizione,
  stato,
  onStato,
  valoreDi,
  scrivi,
  tokens,
  effetti,
  tavolozza,
  budget,
}: {
  definizione: ParteRegistro | null;
  stato: string | null;
  onStato: (stato: string | null) => void;
  valoreDi: (campo: string) => unknown;
  scrivi: (campo: string, valore: unknown) => void;
  tokens: readonly TokenRegistro[];
  effetti: readonly EffettoRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  budget: number;
}) {
  if (definizione === null) {
    return (
      <aside className="ispettore">
        <div className="niente-scelto">
          <p>
            Nessuna parte scelta. Accendi la sonda e passa sopra l&apos;anteprima:
            quel che non è una parte non si illumina.
          </p>
          <p className="sotto">
            È il punto: il problema non è scegliere un colore, è sapere come si
            chiama la cosa che stai guardando.
          </p>
        </div>
      </aside>
    );
  }

  const livelli = (valoreDi("background") ?? []) as Livello[];
  const costo = costoPila(livelli, effetti);
  const ritaglio = valoreDi("clip");
  const angoliTagliati = (() => {
    if (ritaglio === null || typeof ritaglio !== "object") return [];
    const quali = (ritaglio as Record<string, unknown>)["corners"];
    return Array.isArray(quali) ? quali.map(String) : [];
  })();

  return (
    <aside className="ispettore">
      <header className="testa-ispettore">
        <code className="nome-parte">.{definizione.name}</code>
        <span className="gruppo-parte">{definizione.group}</span>
        {definizione.layers && <span className="chip-livello">::after</span>}
        <p className="descrizione">{definizione.description}</p>
      </header>

      <Segmentato
        etichetta="Stato della parte"
        scelta={stato ?? "base"}
        onScegli={(s) => onStato(s === "base" ? null : s)}
        classe="minuto"
        voci={STATI.map(([chiave, etichetta]) => ({
          chiave: chiave ?? "base",
          etichetta,
        }))}
      />

      <div className="corpo-ispettore">
        <Livelli
          livelli={livelli}
          effetti={effetti}
          tokens={tokens}
          tavolozza={tavolozza}
          budget={budget}
          onCambia={(nuovi) => scrivi("background", nuovi)}
        />

        <div className="proprieta">
          <Riga che="Testo">
            <ValoreColore
              valore={valoreDi("textColor")}
              tokens={tokens}
              tavolozza={tavolozza}
              onCambia={(v) => scrivi("textColor", v)}
            />
          </Riga>

          <Riga che="Bordo">
            <div className="bordo">
              <ValoreColore
                valore={valoreDi("borderColor")}
                tokens={tokens}
                tavolozza={tavolozza}
                onCambia={(v) => scrivi("borderColor", v)}
              />
              <input
                className="spessore field-input"
                type="text"
                inputMode="numeric"
                aria-label="Spessore del bordo"
                placeholder="—"
                value={String(valoreDi("borderWidth") ?? "")}
                onChange={(e) =>
                  scrivi("borderWidth", e.target.value === "" ? null : e.target.value)
                }
              />
            </div>
          </Riga>

          <Riga che="Raggio">
            <Cursore
              valore={valoreDi("radius")}
              min={0}
              max={32}
              passo={1}
              unita="px"
              etichetta="Raggio degli angoli"
              onCambia={(v) => scrivi("radius", v)}
            />
          </Riga>

          <Riga che="Ritaglio">
            <div className="ritaglio">
              <input
                className="misura field-input"
                type="text"
                aria-label="Misura del taglio"
                placeholder="non dichiarato"
                value={
                  ritaglio !== null && typeof ritaglio === "object"
                    ? String((ritaglio as Record<string, unknown>)["size"] ?? "")
                    : ""
                }
                onChange={(e) =>
                  scrivi(
                    "clip",
                    e.target.value === ""
                      ? null
                      : { effect: "chamfer", size: e.target.value },
                  )
                }
              />
              {/* Gli angoli si scelgono a griglia perché sono una griglia:
                  quattro caselle disposte come stanno sulla superficie si
                  leggono senza tradurre `bottomRight` in una posizione. */}
              <div className="angoli" role="group" aria-label="Quali angoli">
                {ANGOLI.map((angolo) => {
                  const acceso = angoliTagliati.includes(angolo);
                  return (
                    <button
                      key={angolo}
                      type="button"
                      className="angolo"
                      aria-label={angolo}
                      aria-pressed={acceso}
                      data-active={acceso || undefined}
                      disabled={ritaglio === null || typeof ritaglio !== "object"}
                      onClick={() => {
                        const quali = acceso
                          ? angoliTagliati.filter((a) => a !== angolo)
                          : [...angoliTagliati, angolo];
                        scrivi("clip", {
                          ...(ritaglio as Record<string, unknown>),
                          // Nessun angolo dichiarato vuol dire tutti: togliere
                          // l'ultimo toglie il campo invece di scrivere una
                          // lista vuota, che il formato leggerebbe al contrario.
                          ...(quali.length === 0 ? { corners: undefined } : { corners: quali }),
                        });
                      }}
                    />
                  );
                })}
              </div>
            </div>
          </Riga>

          <Riga che="Opacità">
            <Cursore
              valore={valoreDi("opacity")}
              min={0}
              max={1}
              passo={0.01}
              unita=""
              etichetta="Opacità"
              onCambia={(v) => scrivi("opacity", v)}
            />
          </Riga>

          <Riga che="Lettere">
            <input
              className="field-input"
              type="text"
              aria-label="Spaziatura fra le lettere"
              placeholder="non dichiarata"
              value={String(valoreDi("letterSpacing") ?? "")}
              onChange={(e) =>
                scrivi("letterSpacing", e.target.value === "" ? null : e.target.value)
              }
            />
          </Riga>

          <Riga che="Maiuscole">
            <Segmentato
              etichetta="Trasformazione del testo"
              classe="minuto denso"
              scelta={String(valoreDi("textTransform") ?? "")}
              onScegli={(v) => scrivi("textTransform", v === "" ? null : v)}
              voci={[
                { chiave: "", etichetta: "com'è" },
                { chiave: "uppercase", etichetta: "AB" },
                { chiave: "lowercase", etichetta: "ab" },
                { chiave: "capitalize", etichetta: "Ab" },
              ]}
            />
          </Riga>

          <Riga che="Peso">
            <Cursore
              valore={valoreDi("fontWeight")}
              min={100}
              max={900}
              passo={50}
              unita=""
              etichetta="Peso del carattere"
              onCambia={(v) => scrivi("fontWeight", v)}
            />
          </Riga>
        </div>

        <div className="costo">
          <div className="testa-costo">
            <span className="titolino">Costo della superficie</span>
            <strong style={{ color: coloreCosto(costo, budget) }}>{costo}</strong>
            <span className="su">/ {budget}</span>
          </div>
          {/* Una barra proporzionale e non dieci caselle: un segmento per
              livello, largo quanto pesa. Così il livello che sta mangiando il
              budget si vede senza contare. */}
          <div className="misuratore" data-sopra={costo > budget || undefined}>
            {livelli.map((livello, indice) => {
              const suo = costoDi(livello, effetti);
              return (
                <span
                  key={indice}
                  style={{
                    width: `${Math.min(100, (suo / budget) * 100)}%`,
                    background: coloreCosto(suo, budget),
                  }}
                />
              );
            })}
          </div>
          <p className="nota">
            {livelli.length === 0
              ? "Nessun livello: questa superficie non costa niente."
              : livelli
                  .map((l, i) => `${String(l["effect"] ?? "?")} ${costoDi(livelli[i], effetti)}`)
                  .join(" + ")}
          </p>
        </div>

        <div className="quel-che-non-ce section-card">
          <strong>Quel che non c&apos;è, per scelta</strong>
          <p>
            larghezza, altezza, margine, riempimento, posizione, <code>display</code>.
            Una skin che può spostare le cose può anche nasconderle — e il
            risultato non sembra una skin brutta, sembra un bug dell&apos;app.
          </p>
        </div>
      </div>
    </aside>
  );
}
