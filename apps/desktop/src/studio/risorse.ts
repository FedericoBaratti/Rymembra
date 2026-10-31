/**
 * Un file del pacchetto e chi lo usa: per dirlo prima di toglierlo, e per
 * togliere con lui le dichiarazioni che resterebbero senza file.
 *
 * # Togliere un file non si annulla
 *
 * `studio_risorsa_togli` cancella il file dalla bozza, e nessun Ctrl+Z lo
 * rimette. Fino alla 3.2 il gesto partiva al primo clic e le dichiarazioni dei
 * caratteri tolte con lui entravano nella storia come un passo, «via
 * fraunces.woff2»: la barra offriva di annullarlo, e annullando tornava la
 * dichiarazione e non il file. Adesso si chiede prima, con l'elenco degli usi
 * (`ConfermaGesto`), e la pulizia non è un passo: si applica a **tutta** la
 * storia (`negozio.ts`, azione `risorsaTolta`). Applicarla solo al testo di
 * adesso non bastava: il passo di prima ha ancora la dichiarazione, e il primo
 * Ctrl+Z l'avrebbe rimessa sotto l'etichetta di un'altra modifica.
 *
 * Pure, per `node --test` (`test/risorse.test.ts`).
 */
import type { OperazioneIa } from "../ipc";
import { applicaOperazioni, leggi } from "./patch";

const oggetto = (v: unknown): Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/**
 * Dove la skin usa un file: le dichiarazioni dei caratteri (`fonts.*`) e i
 * livelli `image` delle parti, dei motivi e della variante.
 */
export function usiDellaRisorsa(documento: Record<string, unknown> | null, nome: string): string[] {
  if (documento === null) return [];
  const fuori: string[] = [];
  for (const [f, d] of Object.entries(oggetto(documento["fonts"]))) {
    if (oggetto(d)["src"] === nome) fuori.push(`fonts.${f}`);
  }
  const cerca = (v: unknown, via: string[]) => {
    if (Array.isArray(v)) v.forEach((x, i) => cerca(x, [...via, String(i)]));
    else if (v !== null && typeof v === "object") {
      const d = v as Record<string, unknown>;
      if (d["effect"] === "image" && d["src"] === nome) fuori.push(via.join("."));
      else for (const [k, x] of Object.entries(d)) cerca(x, [...via, k]);
    }
  };
  cerca(documento["parts"], ["parts"]);
  cerca(documento["patterns"], ["patterns"]);
  cerca(documento["variant"], ["variant"]);
  return fuori;
}

/** Le dichiarazioni di carattere che, senza `nome`, non avrebbero più un file. */
export function dichiarazioniDi(documento: Record<string, unknown> | null, nome: string): OperazioneIa[] {
  return Object.entries(oggetto(documento?.["fonts"]))
    .filter(([, d]) => oggetto(d)["src"] === nome)
    .map(([f]) => ({ togli: true, percorso: ["fonts", f], valore: null }));
}

/**
 * Il testo senza le dichiarazioni di carattere che puntano a `nome`.
 *
 * Lo stesso testo, intatto, se non ce ne sono o se non è JSON: un passo vecchio
 * della storia può essere a metà, e lì non c'è niente da togliere con sicurezza.
 */
export function senzaCaratteriDi(testo: string, nome: string): string {
  const operazioni = dichiarazioniDi(leggi(testo), nome);
  return operazioni.length === 0 ? testo : applicaOperazioni(testo, operazioni).testo;
}

/** Gli usi in una riga, per la domanda: i primi `quanti`, poi quanti altri. */
export function elencoBreve(usi: readonly string[], quanti = 8): string {
  const visti = usi.slice(0, quanti).join(", ");
  return usi.length > quanti ? `${visti}, … (+${usi.length - quanti})` : visti;
}

/**
 * Perché un nome di famiglia non si può usare, o `null` se si può.
 *
 * Le famiglie hanno un alfabeto più largo delle altre voci — maiuscole e
 * spazi, «Inter Display» — e per questo hanno il loro campo invece di
 * `Rinomina` (`IspezioneVoci.tsx`). Fino alla 3.2 quel campo, con un nome non
 * ammesso o già preso, non faceva niente e non lo diceva: restava col testo
 * scritto, e sembrava rinominato.
 */
export function problemaNomeFamiglia(
  scritto: string,
  nome: string,
  presi: readonly string[],
): "alfabeto" | "preso" | null {
  const nuovo = scritto.trim();
  if (nuovo === nome) return null;
  if (!/^[A-Za-z0-9 _-]+$/.test(nuovo)) return "alfabeto";
  return presi.includes(nuovo) ? "preso" : null;
}
