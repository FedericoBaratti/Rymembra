/**
 * I controlli, uno per tipo del crate.
 *
 * # Perché stanno qui e non nell'ispettore
 *
 * Erano dentro `Ispettore.tsx`, dove sono nati, e finché l'unica cosa che si
 * modificava a controlli era una parte quella era casa loro. Non lo è più: un
 * token, un parametro di un livello e una proprietà d'aspetto sono **valori
 * dello stesso vocabolario** — un colore è un colore che stia in
 * `tokens.color.accent` o in `parts.x.textColor` — e tenere i controlli
 * nell'ispettore avrebbe voluto dire riscriverli una seconda volta con le stesse
 * tre forme del colore e lo stesso cursore. Due copie dello stesso controllo
 * sono due controlli che divergono.
 *
 * È la regola che `Scafale.tsx` aveva già dimostrato: «una forma di controllo
 * per tipo». Qui quel principio smette di essere una disciplina e diventa un
 * file.
 *
 * # Il cursore non impone più i pixel
 *
 * `Cursore` scriveva `${numero}${unita}` con l'unità decisa da chi lo montava.
 * Su un valore già scritto come `0.5rem` ne leggeva `0.5`, ci appiccicava `px` e
 * lo salvava come `0.5px`: una modifica che l'autore non aveva chiesto, fatta al
 * primo tocco del cursore e su un campo che sembrava solo da guardare. Ora
 * l'unità si legge dal valore e si conserva; `Unita` la cambia quando lo si
 * vuole davvero.
 */
import type { TipoToken, TokenRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { tinta } from "./valori";
import { t } from "../lingue";

/** Le unità che il formato accetta per una lunghezza. Vengono da `LengthUnit`. */
export const UNITA = [
  "px",
  "rem",
  "em",
  "%",
  "vh",
  "vw",
  "cqw",
  "cqh",
  "ch",
] as const;

/** Le curve con un nome, come le riconosce `EasingKeyword`. */
export const CURVE = [
  "linear",
  "ease",
  "easeIn",
  "easeOut",
  "easeInOut",
  "stepStart",
  "stepEnd",
] as const;

/** In che forma è scritto un colore. */
export type Modo = "niente" | "token" | "tavolozza" | "letterale";

export function modoDi(valore: unknown): Modo {
  if (typeof valore === "string") return "letterale";
  if (valore !== null && typeof valore === "object") {
    if ("$token" in valore) return "token";
    if ("$palette" in valore) return "tavolozza";
  }
  return "niente";
}

/** Il numero dentro un valore, comunque sia scritto. `NaN` se non ce n'è uno. */
export function numeroDi(valore: unknown): number {
  if (typeof valore === "number") return valore;
  if (typeof valore === "string") return Number.parseFloat(valore);
  return Number.NaN;
}

/**
 * L'unità di una lunghezza già scritta, o `null` se è un numero puro.
 *
 * Si legge dalla coda e non con una regex sull'intero: `-0.5rem` comincia con un
 * segno e contiene un punto, e ogni tentativo di riconoscere «la parte numerica»
 * in avanti finisce per rifare `parse_length` in TypeScript. Il suffisso invece
 * è chiuso — sono le nove unità di `LengthUnit` — quindi si cerca quello.
 */
export function unitaDi(valore: unknown): string | null {
  if (typeof valore !== "string") return null;
  const testo = valore.trim();
  // Dalla più lunga: `cqw` prima di `w`… e soprattutto `rem` prima di `em`, che
  // ne è un suffisso e ruberebbe ogni `rem` se venisse prima.
  const ordinate = [...UNITA].sort((a, b) => b.length - a.length);
  return ordinate.find((u) => testo.endsWith(u)) ?? null;
}

/**
 * Un colore, in una delle tre forme che il documento accetta.
 *
 * Cambiare modo non conserva il valore: passare da un token a un letterale
 * riparte da una scelta esplicita invece di travasare `var(--accent)` dentro un
 * campo che vuole `#c96a2e`. Un travaso silenzioso produrrebbe un documento che
 * non si valida e nessuno saprebbe perché.
 */
export function ValoreColore({
  valore,
  tokens,
  tavolozza,
  escluso,
  onCambia,
}: {
  valore: unknown;
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  /**
   * Un token che non può riferire sé stesso.
   *
   * `color.accent: { "$token": "color.accent" }` è una variabile CSS che si
   * definisce con sé stessa: il browser la butta e il colore sparisce, senza un
   * errore da nessuna parte. Si toglie dall'elenco invece di spiegarlo dopo.
   */
  escluso?: string | undefined;
  onCambia: (valore: unknown) => void;
}) {
  const modo = modoDi(valore);
  const dipinto = tinta(valore, tokens, tavolozza);
  const colori = tokens.filter((t) => t.kind === "color" && t.id !== escluso);
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
        aria-label={t("studio.ctl.shape")}
        value={modo}
        onChange={(e) => cambiaModo(e.target.value as Modo)}
      >
        <option value="niente">{t("studio.ctl.undeclared")}</option>
        <option value="token">$token</option>
        <option value="tavolozza" disabled={nomiTavolozza.length === 0}>
          $palette
        </option>
        <option value="letterale">{t("studio.ctl.literal")}</option>
      </select>

      {modo === "token" && (
        <select
          className="quale field-input"
          aria-label={t("studio.ctl.whichToken")}
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
          aria-label={t("studio.ctl.whichColor")}
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
          aria-label={t("studio.ctl.theColor")}
          value={
            typeof valore === "string" && valore.startsWith("#")
              ? valore
              : "#000000"
          }
          onChange={(e) => onCambia(e.target.value)}
        />
      )}
    </div>
  );
}

/**
 * Un numero puro col cursore e la lettura.
 *
 * Per le lunghezze c'è [`ValoreLunghezza`], che è questo più l'unità: qui
 * l'unità non esiste proprio, così non c'è nessun posto in cui possa comparirne
 * una per sbaglio.
 */
export function Cursore({
  valore,
  min,
  max,
  passo,
  etichetta,
  suffisso,
  onCambia,
}: {
  valore: unknown;
  min: number;
  max: number;
  passo: number;
  etichetta: string;
  /** Quel che si legge accanto al numero. Solo da guardare: non finisce nel documento. */
  suffisso?: string;
  onCambia: (valore: unknown) => void;
}) {
  const numero = numeroDi(valore);
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
        onChange={(e) => onCambia(Number.parseFloat(e.target.value))}
      />
      <span className="lettura">
        {dichiarato ? `${numero}${suffisso ?? ""}` : "—"}
      </span>
      <button
        type="button"
        className="sgancia icon-btn"
        aria-label={t("studio.ctl.remove", { cosa: etichetta })}
        disabled={!dichiarato}
        onClick={() => onCambia(null)}
      >
        <Icona nome="i-x" dim={11} />
      </button>
    </div>
  );
}

/**
 * Una lunghezza: il numero col cursore, l'unità con un elenco chiuso.
 *
 * L'unità di un valore già scritto si conserva. È la correzione di un cursore
 * che imponeva `px` a ogni tocco e trasformava un `0.5rem` in `0.5px` — una
 * modifica mai chiesta, su una riga che sembrava solo da leggere.
 */
export function ValoreLunghezza({
  valore,
  min,
  max,
  passo,
  etichetta,
  unitaPredefinita = "px",
  soloAssolute = false,
  onCambia,
}: {
  valore: unknown;
  min: number;
  max: number;
  passo: number;
  etichetta: string;
  unitaPredefinita?: string;
  /**
   * Solo `px` e `rem`.
   *
   * Dove il registro dichiara degli estremi, il validatore rifiuta le unità
   * relative alla finestra: `30vh` starebbe dentro qualunque numero e sarebbe
   * comunque un terzo dello schermo. L'elenco lo dice prima, invece di lasciarlo
   * scoprire dall'errore.
   */
  soloAssolute?: boolean;
  onCambia: (valore: unknown) => void;
}) {
  const numero = numeroDi(valore);
  const dichiarato = Number.isFinite(numero);
  const unita = unitaDi(valore) ?? unitaPredefinita;
  const ammesse = soloAssolute ? (["px", "rem"] as const) : UNITA;

  const scrivi = (quanto: number, quale: string) =>
    onCambia(`${quanto}${quale}`);

  return (
    <div className="cursore con-unita">
      <input
        type="range"
        aria-label={etichetta}
        min={min}
        max={max}
        step={passo}
        value={dichiarato ? numero : min}
        onChange={(e) => scrivi(Number.parseFloat(e.target.value), unita)}
      />
      <span className="lettura">{dichiarato ? String(numero) : "—"}</span>
      <select
        className="unita field-input"
        aria-label={t("studio.ctl.unitOf", { cosa: etichetta })}
        value={unita}
        disabled={!dichiarato}
        onChange={(e) => scrivi(numero, e.target.value)}
      >
        {ammesse.map((u) => (
          <option key={u} value={u}>
            {u}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="sgancia icon-btn"
        aria-label={t("studio.ctl.remove", { cosa: etichetta })}
        disabled={!dichiarato}
        onClick={() => onCambia(null)}
      >
        <Icona nome="i-x" dim={11} />
      </button>
    </div>
  );
}

/**
 * Una durata in millisecondi.
 *
 * Il tetto è quello del crate — `MAX_DURATION_MS` è diecimila — ma il cursore si
 * ferma a milleduecento: oltre quella soglia non si sta più scegliendo una
 * transizione, e i mille millisecondi utili starebbero schiacciati nel primo
 * decimo della corsa. Chi vuole davvero un'animazione da otto secondi la scrive
 * nel documento, dove a rispondere è il parser.
 */
export function ValoreDurata({
  valore,
  etichetta,
  onCambia,
}: {
  valore: unknown;
  etichetta: string;
  onCambia: (valore: unknown) => void;
}) {
  const numero = numeroDi(valore);
  const dichiarato = Number.isFinite(numero);
  // `250ms` e `0.25s` sono la stessa durata: si legge quel che c'è e si riscrive
  // nella stessa unità, perché cambiarla sotto le mani sarebbe una modifica che
  // nessuno ha chiesto.
  const inSecondi =
    typeof valore === "string" &&
    /s\s*$/.test(valore) &&
    !/ms\s*$/.test(valore);
  const ms = dichiarato && inSecondi ? numero * 1000 : numero;

  return (
    <div className="cursore">
      <input
        type="range"
        aria-label={etichetta}
        min={0}
        max={1200}
        step={10}
        value={dichiarato ? ms : 0}
        onChange={(e) => {
          const preso = Number.parseFloat(e.target.value);
          onCambia(inSecondi ? `${preso / 1000}s` : `${preso}ms`);
        }}
      />
      <span className="lettura">{dichiarato ? `${ms}ms` : "—"}</span>
      <button
        type="button"
        className="sgancia icon-btn"
        aria-label={t("studio.ctl.remove", { cosa: etichetta })}
        disabled={!dichiarato}
        onClick={() => onCambia(null)}
      >
        <Icona nome="i-x" dim={11} />
      </button>
    </div>
  );
}

/**
 * Una curva: una parola dell'elenco, o quattro numeri.
 *
 * L'anteprima disegnata conta più dei numeri. `cubic-bezier(0.16, 1, 0.3, 1)`
 * non dice niente a nessuno; la stessa curva tracciata su venti pixel si legge a
 * colpo d'occhio, e due curve diverse si distinguono senza confrontare otto
 * cifre.
 */
export function ValoreCurva({
  valore,
  etichetta,
  onCambia,
}: {
  valore: unknown;
  etichetta: string;
  onCambia: (valore: unknown) => void;
}) {
  const punti =
    Array.isArray(valore) && valore.length === 4 ? valore.map(Number) : null;
  const parola = typeof valore === "string" ? valore : null;
  const modo =
    punti !== null ? "bezier" : parola !== null ? "parola" : "niente";

  return (
    <div className="valore-curva">
      {punti !== null && (
        <svg className="traccia" viewBox="0 0 40 40" aria-hidden="true">
          {/* La diagonale è il riferimento: una curva che ci sta sopra
              accelera, una che ci sta sotto frena. Senza, i quattro numeri
              tornano a essere quattro numeri. */}
          <path d="M0 40 L40 0" className="diagonale" />
          <path
            d={`M0 40 C ${(punti[0] ?? 0) * 40} ${40 - (punti[1] ?? 0) * 40}, ${
              (punti[2] ?? 0) * 40
            } ${40 - (punti[3] ?? 0) * 40}, 40 0`}
            className="curva"
          />
        </svg>
      )}
      <select
        className="modo field-input"
        aria-label={etichetta}
        value={modo === "parola" ? String(parola) : modo}
        onChange={(e) => {
          const scelto = e.target.value;
          if (scelto === "niente") onCambia(null);
          else if (scelto === "bezier") onCambia([0.16, 1, 0.3, 1]);
          else onCambia(scelto);
        }}
      >
        <option value="niente">{t("studio.ctl.undeclared.f")}</option>
        {CURVE.map((c) => (
          <option key={c} value={c}>
            {c}
          </option>
        ))}
        <option value="bezier">cubic-bezier…</option>
      </select>

      {punti !== null && (
        <div className="quattro">
          {punti.map((p, i) => (
            <input
              // Quattro caselle senza identità propria: la posizione **è** il
              // significato — x1, y1, x2, y2 — e non cambia mai.
              key={i}
              className="field-input"
              type="number"
              aria-label={t("studio.ctl.curvePoint", {
                punto: ["x1", "y1", "x2", "y2"][i] ?? "",
              })}
              step={0.01}
              // I due x stanno fra 0 e 1 per definizione; le y possono uscirne,
              // ed è così che si ottiene un rimbalzo.
              min={i % 2 === 0 ? 0 : -2}
              max={i % 2 === 0 ? 1 : 2}
              value={p}
              onChange={(e) => {
                const nuovi = [...punti];
                nuovi[i] = Number.parseFloat(e.target.value);
                onCambia(nuovi);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Una pila di caratteri: le famiglie in ordine, la prima che c'è vince.
 *
 * Il compilatore aggiunge da sé il ripiego di sistema in coda — `sans-serif`,
 * `monospace` o quel che corrisponde al genere del token — quindi qui si
 * scrivono solo i nomi veri, e nessuno deve ricordarsi di chiudere la pila.
 */
export function PilaCaratteri({
  valore,
  onCambia,
}: {
  valore: unknown;
  onCambia: (valore: unknown) => void;
}) {
  const famiglie = Array.isArray(valore)
    ? valore.map(String)
    : typeof valore === "string"
      ? [valore]
      : [];

  const scrivi = (nuove: string[]) =>
    onCambia(nuove.length === 0 ? null : nuove);

  return (
    <div className="pila-caratteri">
      {famiglie.map((famiglia, indice) => (
        <div key={indice} className="una-famiglia">
          <span className="posto">{indice + 1}</span>
          <input
            className="field-input"
            type="text"
            aria-label={t("studio.ctl.family", { n: indice + 1 })}
            value={famiglia}
            style={{ fontFamily: `'${famiglia}', sans-serif` }}
            onChange={(e) => {
              const nuove = [...famiglie];
              nuove[indice] = e.target.value;
              scrivi(nuove);
            }}
          />
          <button
            type="button"
            className="via icon-btn"
            aria-label={t("studio.ctl.remove", { cosa: famiglia })}
            onClick={() => scrivi(famiglie.filter((_, i) => i !== indice))}
          >
            <Icona nome="i-x" dim={11} />
          </button>
        </div>
      ))}
      <button
        type="button"
        className="aggiungi-livello"
        onClick={() => scrivi([...famiglie, ""])}
      >
        <Icona nome="i-plus" dim={13} />
        <span>{t("studio.ctl.addFamily")}</span>
      </button>
    </div>
  );
}

/** Un livello d'ombra, come lo scrive il documento. */
type LivelloOmbra = {
  inset?: boolean;
  x?: string;
  y?: string;
  blur?: string;
  spread?: string;
  color?: unknown;
};

/**
 * Un'ombra: una lista di livelli, non una stringa.
 *
 * È il controllo che il registro aveva già chiesto. `ShadowValue` è una lista
 * apposta — «con la struttura, l'editor può mostrare un controllo per livello
 * invece di un campo di testo in cui si può scrivere qualunque cosa», dice
 * `tokens.rs` — e finora quel controllo non esisteva, quindi l'unico modo di
 * ritoccare un'ombra era il JSON.
 *
 * Zero livelli è legittimo e vuol dire `none`: è così che una skin piatta
 * spegne un'ombra senza doverne inventare una trasparente.
 */
export function ValoreOmbra({
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
  const livelli: LivelloOmbra[] = Array.isArray(valore)
    ? (valore as LivelloOmbra[])
    : [];

  const cambia = (indice: number, campo: keyof LivelloOmbra, dato: unknown) => {
    const nuovi = livelli.map((l, i) =>
      i === indice ? { ...l, [campo]: dato ?? undefined } : l,
    );
    onCambia(nuovi);
  };

  return (
    <div className="valore-ombra">
      {livelli.map((livello, indice) => (
        <div key={indice} className="un-livello-ombra">
          <div className="testa-livello-ombra">
            <label className="interruttore" title={t("studio.ctl.inward")}>
              <input
                type="checkbox"
                checked={livello.inset === true}
                onChange={(e) =>
                  cambia(indice, "inset", e.target.checked || undefined)
                }
              />
              <span>inset</span>
            </label>
            <button
              type="button"
              className="via icon-btn"
              aria-label={t("studio.ctl.removeLayer", { n: indice + 1 })}
              onClick={() => onCambia(livelli.filter((_, i) => i !== indice))}
            >
              <Icona nome="i-x" dim={11} />
            </button>
          </div>
          <div className="misure-ombra">
            {(["x", "y", "blur", "spread"] as const).map((campo) => (
              <label key={campo} className="misura-ombra">
                <span>{campo}</span>
                <input
                  className="field-input"
                  type="text"
                  inputMode="numeric"
                  placeholder="—"
                  value={livello[campo] ?? ""}
                  onChange={(e) =>
                    cambia(
                      indice,
                      campo,
                      e.target.value === "" ? undefined : e.target.value,
                    )
                  }
                />
              </label>
            ))}
          </div>
          <ValoreColore
            valore={livello.color ?? null}
            tokens={tokens}
            tavolozza={tavolozza}
            onCambia={(v) => cambia(indice, "color", v)}
          />
        </div>
      ))}
      <button
        type="button"
        className="aggiungi-livello"
        onClick={() =>
          onCambia([
            ...livelli,
            { x: "0px", y: "1px", blur: "2px", color: "#00000040" },
          ])
        }
      >
        <Icona nome="i-plus" dim={13} />
        <span>{t("studio.ctl.addLayer")}</span>
        {livelli.length === 0 && (
          <span className="quanti">{t("studio.ctl.nowNone")}</span>
        )}
      </button>
    </div>
  );
}

/** Una riga della tabella delle proprietà. */
export function Riga({
  che,
  children,
}: {
  che: string;
  children: React.ReactNode;
}) {
  return (
    <div className="proprieta-riga">
      <span className="che">{che}</span>
      <div className="come">{children}</div>
    </div>
  );
}

/**
 * Il controllo che corrisponde a un tipo del registro.
 *
 * È il cuore della faccenda: l'editor dei token non ha una tabella di
 * cinquantotto voci con scritto quale controllo montare per ciascuna, perché una
 * tabella del genere è una seconda copia del registro — e le due copie divergono
 * al primo token aggiunto. Ha invece questa funzione, che guarda `kind` e
 * `min`/`max`, cioè quel che il registro dichiara già. Un token nuovo nel crate
 * compare qui col controllo giusto senza che nessuno tocchi questo file.
 */
export function ControlloPerTipo({
  tipo,
  valore,
  token,
  tokens,
  tavolozza,
  onCambia,
}: {
  tipo: TipoToken;
  valore: unknown;
  /** Il token che si sta modificando, quando è un token: dà estremi e identità. */
  token?: TokenRegistro | undefined;
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  onCambia: (valore: unknown) => void;
}) {
  const limitato = token?.min != null && token.max != null;

  switch (tipo) {
    case "color":
      return (
        <ValoreColore
          valore={valore}
          tokens={tokens}
          tavolozza={tavolozza}
          escluso={token?.id}
          onCambia={onCambia}
        />
      );
    case "length":
      return (
        <ValoreLunghezza
          valore={valore}
          min={token?.min ?? 0}
          max={token?.max ?? 96}
          // Un ritmo si sceglie al pixel; una misura senza estremi ha una corsa
          // più lunga e mezzo pixel non serve a nessuno.
          passo={limitato ? 1 : 2}
          etichetta={token?.id ?? t("studio.ctl.length")}
          soloAssolute={limitato}
          onCambia={onCambia}
        />
      );
    case "duration":
      return (
        <ValoreDurata
          valore={valore}
          etichetta={token?.id ?? t("studio.ctl.duration")}
          onCambia={onCambia}
        />
      );
    case "easing":
      return (
        <ValoreCurva
          valore={valore}
          etichetta={token?.id ?? t("studio.ctl.curve")}
          onCambia={onCambia}
        />
      );
    case "number":
      return (
        <Cursore
          valore={valore}
          min={token?.min ?? 0}
          max={token?.max ?? 1}
          // Con estremi stretti — `radius.inner` sta fra 0 e 1 — un passo fisso
          // darebbe due o tre posizioni in tutta la corsa. Cento gradini sono
          // abbastanza fini ovunque e restano numeri tondi.
          passo={Number(
            (((token?.max ?? 1) - (token?.min ?? 0)) / 100).toFixed(4),
          )}
          etichetta={token?.id ?? t("studio.ctl.number")}
          onCambia={onCambia}
        />
      );
    case "fontStack":
      return <PilaCaratteri valore={valore} onCambia={onCambia} />;
    case "shadow":
      return (
        <ValoreOmbra
          valore={valore}
          tokens={tokens}
          tavolozza={tavolozza}
          onCambia={onCambia}
        />
      );
  }
}

/** Un segmentato per un enum di parole, con «non dichiarato» in testa. */
export function Parole({
  valore,
  voci,
  etichetta,
  onCambia,
}: {
  valore: unknown;
  voci: readonly { chiave: string; etichetta: string }[];
  etichetta: string;
  onCambia: (valore: unknown) => void;
}) {
  return (
    <Segmentato
      etichetta={etichetta}
      classe="minuto denso"
      scelta={String(valore ?? "")}
      onScegli={(v) => onCambia(v === "" ? null : v)}
      voci={[{ chiave: "", etichetta: "—" }, ...voci]}
    />
  );
}
