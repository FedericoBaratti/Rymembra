/**
 * L'equalizzatore, le regole che non sono disegno: come si riconosce una curva
 * nella tendina, come si chiama una curva di serie, come si scrivono i decibel.
 *
 * Un modulo a parte perché è puro e si prova con `node --test`: il componente
 * porta con sé `ipc.ts` e le lingue, che fuori da Vite non si caricano. Da
 * `./ipc` arriva solo il **tipo**.
 */
import type { VocePreset } from "./ipc";

/**
 * La chiave di una curva nella tendina: `s:Rock` per quella di serie,
 * `u:Rock` per quella dell'utente.
 *
 * Non il nome, che stava lì: il nucleo accetta di salvare una curva col nome
 * di una di serie («Rock» ritoccata e salvata come «Rock»), e due `<option>`
 * con lo stesso valore sono due voci di cui si sceglie sempre la prima — la
 * curva salvata non si applicava mai, si poteva solo cancellare.
 */
export function chiavePreset(p: Pick<VocePreset, "diSerie" | "nome">): string {
  return `${p.diSerie ? "s" : "u"}:${p.nome}`;
}

/** La curva che ha questa chiave, o `undefined`. Vedi `chiavePreset`. */
export function trovaPreset<P extends Pick<VocePreset, "diSerie" | "nome">>(
  elenco: readonly P[],
  chiave: string,
): P | undefined {
  return elenco.find((p) => chiavePreset(p) === chiave);
}

/**
 * La chiave di lingua del nome di una curva di serie: `eq.preset.Rock`.
 *
 * Il nome arriva dal nucleo così com'è scritto in
 * `rymembra_play::equalizzatore::PRESET_DI_SERIE` — «Piatto», «Voce»,
 * «Bassi» — e resta il valore stabile; si traduce solo quel che si mostra.
 */
export function chiaveNomePreset(nome: string): string {
  return `eq.preset.${nome}`;
}

/**
 * I decibel di una banda, come si leggono accanto al cursore: «+1,5»,
 * «−2,5», «0».
 *
 * Con i separatori della lingua — il passo è mezzo decibel, quindi metà dei
 * valori ha un decimale, e «+1.5» in un'interfaccia italiana sembra un errore
 * di calcolo — e col segno meno vero (U+2212) invece del trattino, come il
 * «+» che gli sta di fronte. Il formattatore arriva da fuori (`numero` di
 * `formato.ts`) perché questo modulo resti senza lingue.
 */
export function decibel(db: number, numero: (n: number, decimali: number) => string): string {
  if (db === 0) return "0";
  const scritto = numero(Math.abs(db), Number.isInteger(db) ? 0 : 1);
  return db > 0 ? `+${scritto}` : `−${scritto}`;
}

/** Come si legge il centro di una banda: «31 Hz», «1 kHz». */
export function etichettaBanda(hz: number): string {
  return hz >= 1000 ? `${hz / 1000} kHz` : `${Math.round(hz)} Hz`;
}

/**
 * La stessa, corta: «31», «1k». Per il telefono, dove dieci etichette lunghe
 * in trecento pixel si accavallano; l'unità la sa chi guarda un equalizzatore.
 */
export function etichettaCortaBanda(hz: number): string {
  return hz >= 1000 ? `${hz / 1000}k` : String(Math.round(hz));
}

/** Il tempo, per il freno: quello della finestra, o uno finto nelle prove. */
export interface OrologioFreno {
  ora: () => number;
  dopo: (ms: number, fai: () => void) => number;
  annulla: (id: number) => void;
}

/** Vedi [`creaFreno`]. */
export interface Freno<T> {
  /** Un valore nuovo: parte subito se è passato il passo, se no aspetta. */
  metti: (valore: T) => void;
  /** Manda subito quel che aspetta, se c'è, e ferma l'attesa. */
  svuota: () => void;
}

/**
 * Manda al più un valore ogni `passo` millisecondi, e mai l'ultimo in ritardo
 * oltre il passo.
 *
 * Il primo parte **subito**: chi tocca un cursore deve sentire il
 * cambiamento al primo pixel. Quelli che arrivano prima che il passo sia
 * finito si tengono da parte, e allo scadere parte solo l'ultimo.
 *
 * # Perché `svuota`
 *
 * Chiudere il pannello dell'equalizzatore entro il passo dall'ultimo
 * movimento smontava il componente con la curva più recente ancora in
 * sospeso: la finestra l'aveva disegnata, il nucleo aveva quella di prima, e
 * riaprendo il cursore tornava indietro. Allo smontaggio quel che aspetta
 * parte subito — come fa `LivelloDelMembro` in «Ascolta insieme».
 */
export function creaFreno<T>(
  passo: number,
  spedisci: (valore: T) => void,
  orologio: OrologioFreno,
): Freno<T> {
  let ultimoInvio = Number.NEGATIVE_INFINITY;
  let inSospeso: { valore: T } | null = null;
  let attesa: number | null = null;

  const manda = (valore: T) => {
    ultimoInvio = orologio.ora();
    spedisci(valore);
  };

  return {
    metti(valore) {
      const trascorso = orologio.ora() - ultimoInvio;
      if (trascorso >= passo && attesa === null) {
        manda(valore);
        return;
      }
      inSospeso = { valore };
      if (attesa !== null) return;
      attesa = orologio.dopo(Math.max(0, passo - trascorso), () => {
        attesa = null;
        const ultimo = inSospeso;
        inSospeso = null;
        if (ultimo) manda(ultimo.valore);
      });
    },
    svuota() {
      if (attesa !== null) {
        orologio.annulla(attesa);
        attesa = null;
      }
      const ultimo = inSospeso;
      inSospeso = null;
      if (ultimo) manda(ultimo.valore);
    },
  };
}
