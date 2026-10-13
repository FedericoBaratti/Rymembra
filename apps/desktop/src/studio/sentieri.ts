/**
 * Dove si scrive, e dove porta un percorso.
 *
 * # Due temi, un gesto
 *
 * Una skin del formato 2 ha un tema di base e, se vuole, la sua variante: il
 * tema opposto, che sovrascrive token, tavolozza, motivi e parti. Nello Studio
 * si passa dall'uno all'altro con un interruttore solo, e da lì in poi **ogni**
 * controllo scrive nel tema che si sta guardando. Il percorso lo decide questa
 * funzione e nessun'altra: se ogni pannello se lo componesse da sé, prima o poi
 * uno scriverebbe nella base mentre la tela mostra la variante — e la modifica
 * sparirebbe sotto gli occhi di chi l'ha fatta.
 *
 * # Da un percorso a una selezione
 *
 * Il nucleo dice dove sta un problema con un percorso puntato:
 * `parts.card.states.hover.textColor`, `tokens.color.surface.0`,
 * `motion.animations.sale.frames.1.at`. «Vai» deve portare alla cosa, non alla
 * riga: la parte scelta con lo stato giusto, il token nel suo editor,
 * l'animazione nella timeline. I token hanno i punti nel nome, quindi il loro
 * id si riconosce dal registro — il più lungo che combacia — e non tagliando
 * al primo punto.
 */
import type { Selezione, SchedaIspettore, Tema } from "./negozio";

/** La radice delle parti nel tema scelto. */
export function radiceParti(tema: Tema): string[] {
  return tema === "variante" ? ["variant", "parts"] : ["parts"];
}

/** Il percorso di un campo d'aspetto di una parte, nello stato e nel tema scelti. */
export function sentieroParte(
  parte: string,
  stato: string | null,
  campo: string,
  tema: Tema,
): string[] {
  const radice = [...radiceParti(tema), parte];
  return stato === null ? [...radice, campo] : [...radice, "states", stato, campo];
}

/** Il percorso di un token nel tema scelto. */
export function sentieroToken(id: string, tema: Tema): string[] {
  return tema === "variante" ? ["variant", "tokens", id] : ["tokens", id];
}

/** Il percorso di un colore della tavolozza nel tema scelto. */
export function sentieroColore(nome: string, tema: Tema): string[] {
  return tema === "variante" ? ["variant", "palette", nome] : ["palette", nome];
}

/** Il percorso di un motivo nel tema scelto. */
export function sentieroMotivo(nome: string, tema: Tema): string[] {
  return tema === "variante" ? ["variant", "patterns", nome] : ["patterns", nome];
}

/** Dove porta un percorso del nucleo: cosa scegliere, e come guardarlo. */
export interface Destinazione {
  selezione: Selezione;
  /** Lo stato della parte, se il percorso ne nomina uno. */
  stato?: string | null;
  tema?: Tema;
  ispettore?: SchedaIspettore;
}

/** Il token più lungo che combacia con l'inizio di `resto`. */
function tokenIn(resto: readonly string[], idToken: readonly string[]): string | null {
  for (let fino = resto.length; fino > 0; fino -= 1) {
    const candidato = resto.slice(0, fino).join(".");
    if (idToken.includes(candidato)) return candidato;
  }
  return null;
}

/**
 * La selezione a cui porta un percorso, o `null` se non porta da nessuna parte
 * che lo Studio sappia mostrare.
 */
export function destinazioneDi(
  percorso: string,
  idToken: readonly string[],
): Destinazione | null {
  const pezzi = percorso.split(".").filter((p) => p !== "");
  const primo = pezzi[0];
  if (primo === undefined) return null;

  if (primo === "variant") {
    const dentro = destinazioneDi(pezzi.slice(1).join("."), idToken);
    return dentro === null ? null : { ...dentro, tema: "variante" };
  }

  switch (primo) {
    case "parts": {
      const nome = pezzi[1];
      if (nome === undefined) return null;
      const selezione: Selezione = { tipo: "parte", nome };
      if (pezzi[2] === "states" && pezzi[3] !== undefined) {
        return { selezione, stato: pezzi[3], ispettore: "stile" };
      }
      if (pezzi[2] === "motion") return { selezione, ispettore: "movimento" };
      return { selezione, stato: null, ispettore: "stile" };
    }
    case "tokens": {
      const id = tokenIn(pezzi.slice(1), idToken);
      return id === null ? null : { selezione: { tipo: "token", id } };
    }
    case "palette":
      return pezzi[1] === undefined ? null : { selezione: { tipo: "colore", nome: pezzi[1] } };
    case "patterns":
      return pezzi[1] === undefined ? null : { selezione: { tipo: "motivo", nome: pezzi[1] } };
    case "fonts":
      return pezzi[1] === undefined ? null : { selezione: { tipo: "carattere", nome: pezzi[1] } };
    case "motion": {
      const nome = pezzi[2];
      if (pezzi[1] === "animations" && nome !== undefined) {
        return { selezione: { tipo: "animazione", nome } };
      }
      if (pezzi[1] === "easings" && nome !== undefined) {
        return { selezione: { tipo: "curva", nome } };
      }
      return { selezione: { tipo: "documento" } };
    }
    case "layout": {
      if (pezzi[1] !== "shell") return { selezione: { tipo: "documento" } };
      // `layout.shell.children.2.children.0` → [2, 0]: la via dell'albero.
      const via: number[] = [];
      for (let i = 2; i + 1 < pezzi.length; i += 2) {
        if (pezzi[i] !== "children") break;
        const n = Number(pezzi[i + 1]);
        if (!Number.isInteger(n)) break;
        via.push(n);
      }
      return { selezione: { tipo: "nodo", via } };
    }
    default:
      return { selezione: { tipo: "documento" } };
  }
}
