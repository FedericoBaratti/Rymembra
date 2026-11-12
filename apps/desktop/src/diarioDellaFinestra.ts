/**
 * Le righe che la finestra scrive nel diario per i guasti che il recinto non
 * prende: un errore fuori dal disegno, una promessa rifiutata senza `catch`.
 *
 * Funzioni pure, fuori da `main.tsx`, perché si possano provare: `main.tsx`
 * disegna l'applicazione appena lo si importa. Le registra `main.tsx`, a
 * livello di modulo; il perché sta lì.
 */

/**
 * Le prime righe della pila, legate con « ← »: il nucleo taglia a una riga
 * sola, e così ci stanno. Vuoto se la pila non c'è o non ha righe «at …».
 */
function pilaBreve(errore: unknown): string {
  const pila = errore instanceof Error ? errore.stack : undefined;
  const righe = pila
    ?.split("\n")
    .map((riga) => riga.trim())
    .filter((riga) => riga.startsWith("at "))
    .slice(0, 4);
  return righe && righe.length > 0 ? righe.join(" ← ") : "";
}

/** Quel che serve di un `ErrorEvent`. */
export type ErroreDellaFinestra = Pick<
  ErrorEvent,
  "message" | "error" | "filename" | "lineno" | "colno"
>;

/**
 * Il messaggio, e dove è successo: le prime righe della pila, o almeno file,
 * riga e colonna. Il messaggio da solo — «Cannot read properties of undefined
 * (reading 'length')», arrivato così da un diario vero — non dice quale dei
 * cento `.length` dell'interfaccia fosse, e senza il punto un guasto che non
 * si ripete non si trova.
 */
export function conDove(evento: ErroreDellaFinestra): string {
  const pila = pilaBreve(evento.error);
  if (pila !== "") return `${evento.message} ${pila}`;
  return evento.filename
    ? `${evento.message} (${evento.filename}:${evento.lineno}:${evento.colno})`
    : evento.message;
}

/**
 * La forma di un errore del nucleo, letta qui senza `eErroreIpc`: la regola è
 * quella di `ipc/errori.ts`, ma quel modulo si porta dietro le lingue, e le
 * prove di questo file girano senza.
 */
function delNucleo(
  motivo: unknown,
): motivo is { code: string; message?: unknown; cause?: unknown } {
  return (
    typeof motivo === "object" &&
    motivo !== null &&
    "code" in motivo &&
    typeof motivo.code === "string" &&
    "domain" in motivo &&
    typeof motivo.domain === "string" &&
    "severity" in motivo &&
    typeof motivo.severity === "string"
  );
}

/** `String(...)`, che su un oggetto senza prototipo cade. */
function comeTesto(motivo: unknown): string {
  try {
    return String(motivo);
  } catch {
    return Object.prototype.toString.call(motivo);
  }
}

/**
 * La riga di una promessa rifiutata.
 *
 * `reason` è quel che è stato passato a `reject`, e può essere qualunque cosa.
 * Il caso normale è un errore del nucleo, perché `invoke` rifiuta con
 * l'oggetto serializzato: da lui il **codice**, che è quel che si cerca nel
 * diario, poi il messaggio e la causa — mai la frase tradotta, che è per chi
 * guarda la finestra e cambia con la lingua. `String(...)` ne faceva
 * «[object Object]», e la riga non diceva niente.
 *
 * Un `Error` dice tipo, messaggio e dove, come gli errori della finestra; un
 * altro oggetto si scrive in JSON; tutto il resto con `String(...)`. Non cade
 * mai: un gestore di guasti che cade non lascia nessuna traccia.
 */
export function rigaDelRifiuto(motivo: unknown): string {
  if (delNucleo(motivo)) {
    const parti = [motivo.message, motivo.cause]
      .filter((parte): parte is string => typeof parte === "string" && parte.trim() !== "")
      .filter((parte, i, tutte) => tutte.indexOf(parte) === i);
    return parti.length > 0 ? `${motivo.code}: ${parti.join(" · ")}` : motivo.code;
  }
  if (motivo instanceof Error) {
    const pila = pilaBreve(motivo);
    const testo = comeTesto(motivo);
    return pila !== "" ? `${testo} ${pila}` : testo;
  }
  if (typeof motivo === "object" && motivo !== null) {
    try {
      const json = JSON.stringify(motivo);
      if (typeof json === "string") return json;
    } catch {
      // Circolare, o con un `toJSON` che cade: la lettura di sempre.
    }
  }
  return comeTesto(motivo);
}
