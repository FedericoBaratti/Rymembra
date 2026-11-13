/**
 * «Ricollega» con un altro account (R24-5, D-92).
 *
 * Il nucleo rifiuta un consenso dato con un account diverso da quello che si
 * stava ricollegando (`cloud.otherAccount`), e fra i `values` dell'errore mette
 * il nome dell'account con cui si è entrati (`ottenuto`): un dato, non una
 * frase (`ErrorCode::valori_della_frase`, nel nucleo). Il nome di quello
 * atteso lo sa la finestrella, che lo sta ricollegando: qui si mettono insieme
 * i due, così la frase li nomina tutti e due nella lingua di chi legge.
 *
 * Senza dipendenze dalle lingue, perché si prova da sola: chi la usa passa i
 * valori a `t("folders.reconnect.otherAccount", …)`.
 */

/** I due account da nominare, o `null` se il guasto è un altro. */
export function contiDaNominare(
  guasto: unknown,
  daRicollegare: { nome: string } | null,
): { ottenuto: string; atteso: string } | null {
  if (daRicollegare === null || typeof guasto !== "object" || guasto === null) return null;
  const codice = "code" in guasto ? guasto.code : null;
  const valori = "values" in guasto ? guasto.values : null;
  if (codice !== "cloud.otherAccount" || typeof valori !== "object" || valori === null) return null;
  const ottenuto = "ottenuto" in valori && typeof valori.ottenuto === "string" ? valori.ottenuto.trim() : "";
  if (ottenuto === "") return null;
  return { ottenuto, atteso: daRicollegare.nome };
}
