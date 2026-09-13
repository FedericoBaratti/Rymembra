/**
 * L'ancora fra l'orologio della finestra e la posizione che il nucleo racconta.
 *
 * # Il problema, che non è l'interpolazione
 *
 * Fra un colpo del nucleo e il successivo passano duecentocinquanta
 * millisecondi, e la barra deve muoversi comunque: questo si sapeva, e si
 * risolveva da sé — `posizione + (adesso − quando è arrivata)`. Il difetto stava
 * nel secondo addendo. **Ogni** evento riscriveva l'ancora, quindi ogni evento
 * portava dentro il proprio ritardo di consegna: il tempo che il messaggio ha
 * passato nel canale IPC, nella coda del webview, e dietro un fotogramma di
 * JavaScript occupato a disegnare duecento righe.
 *
 * Quel ritardo **non è costante**. Un colpo arriva in due millisecondi, il
 * successivo in quaranta perché nel frattempo si stava scorrendo un elenco. E
 * siccome l'ancora è `arrivo − posizione`, un colpo arrivato tardi dichiara
 * un'origine più avanti nel tempo, cioè **strappa indietro** la posizione
 * stimata. Visto da fuori: un cursore che avanza e ogni tanto scatta indietro di
 * qualche decina di millisecondi, e un testo che su una riga breve si riaccende.
 *
 * # La cura: il minimo, non l'ultimo
 *
 * Il ritardo di consegna può solo **aggiungere**: nessun messaggio arriva prima
 * di essere stato spedito. Quindi, fra tutte le origini che gli eventi recenti
 * propongono, quella più **bassa** è la meno contaminata — è quella dell'evento
 * che ha fatto il viaggio più breve. Si tiene una finestra di campioni e si usa
 * il minimo: il rumore di consegna si accumula verso l'alto e il minimo lo
 * scarta, invece di inseguirlo.
 *
 * La finestra ha due limiti, e servono a due cose diverse:
 *
 * * **quanti** campioni ([`FINESTRA`]) — perché il minimo su una storia infinita
 *   sarebbe il colpo più fortunato di sempre, e un'origine che non si corregge
 *   mai più;
 * * **da quanto** ([`ETA`]) — perché l'orologio del dispositivo audio e quello
 *   della finestra non sono lo stesso orologio. Qualche decina di parti per
 *   milione di scarto fanno meno di un millisecondo su otto secondi, che è il
 *   motivo per cui otto secondi bastano e ottanta no.
 *
 * # Quando si butta via tutto
 *
 * Tre casi, e in tutti e tre la storia di prima non parla più di quel che sta
 * succedendo: un **brano diverso** (l'origine è un'altra), una **pausa che
 * comincia o finisce** (durante la pausa il tempo di parete avanza e la
 * posizione no, quindi ogni campione vecchio dichiara un'origine troppo
 * indietro), e uno **scarto oltre [`SALTO`]** fra quel che si stimava e quel che
 * il nucleo dice. Il terzo copre i salti col cursore senza che nessuno debba
 * dichiararli: trascinare la barra sposta la posizione di molto più di
 * quattrocento millisecondi, e l'orologio se ne accorge da sé.
 *
 * # L'onestà su un caso che rompe la regola
 *
 * «Il ritardo può solo aggiungere» è vero per tutti gli eventi tranne uno:
 * `manda_stato_con_posizione`, nel nucleo, manda la posizione in cui il brano
 * **sarà** invece di quella in cui è — serve a non far rimbalzare il cursore
 * dopo un salto. Quel messaggio propone un'origine troppo **bassa**, cioè
 * esattamente il verso che il minimo non sa difendere.
 *
 * Non serve nessun meccanismo in più, perché quel messaggio arriva sempre in uno
 * dei tre casi che svuotano la finestra: un salto, o un brano nuovo. Resta
 * un'ancora **provvisoria** — da sola, senza storia intorno — e il primo
 * campione onesto che arriva la sostituisce invece di entrarle accanto. Questo sì
 * che è un meccanismo in più, e sono tre righe: senza, l'ancora ottimista
 * vincerebbe il minimo per tutti gli otto secondi dopo ogni salto.
 *
 * # Perché è un modulo puro
 *
 * Niente React, niente `performance.now()` chiamato da qui dentro, niente stato
 * globale: gli istanti entrano come argomenti. Così si prova senza montare una
 * finestra — cioè si può provare l'unica parte di questa catena in cui un errore
 * è invisibile a occhio e si manifesta come «i testi a volte scattano».
 */

/**
 * Quanti campioni d'origine si tengono.
 *
 * Ventiquattro, cioè sei secondi di colpi a quattro al secondo. Abbastanza
 * perché fra loro ce ne sia quasi sempre uno arrivato in fretta, pochi
 * abbastanza perché un'origine non resti appesa a un colpo fortunato di mezzo
 * minuto prima.
 */
export const FINESTRA = 24;

/**
 * Da quanto tempo un campione smette di contare, in millisecondi.
 *
 * Otto secondi. Non è il doppio di [`FINESTRA`] per simmetria: è il tempo oltre
 * il quale lo scarto fra l'orologio del dispositivo audio e quello della finestra
 * smette di essere trascurabile. I due limiti esistono insieme perché i colpi
 * possono anche **non** arrivare — in pausa il nucleo tace — e una finestra
 * contata solo in campioni conserverebbe per sempre gli ultimi ventiquattro di
 * mezz'ora fa.
 */
export const ETA = 8000;

/**
 * Oltre quanto scarto non è più rumore di consegna, in millisecondi.
 *
 * Quattrocento. Sopra il ritardo IPC peggiore che si è misurato, e ben sotto il
 * salto più piccolo che un gesto produce: le frecce spostano di secondi, e
 * trascinare la barra di molto più. In mezzo non c'è niente che debba essere
 * preso per rumore.
 */
export const SALTO = 400;

/** Come si regola l'orologio. Vedi le tre costanti qui sopra. */
export interface OpzioniOrologio {
  /** Quanti campioni d'origine tenere. */
  finestra: number;
  /** Da quanto tempo un campione smette di contare, in millisecondi. */
  eta: number;
  /** Oltre quanto scarto si riancora secco, in millisecondi. */
  salto: number;
}

/** Un'origine proposta da un evento, e quando l'ha proposta. */
interface Campione {
  /** L'istante di parete in cui il brano era a zero, secondo questo evento. */
  origine: number;
  /** Quando l'evento è arrivato, per dimenticarlo quando invecchia. */
  quando: number;
}

/** L'orologio: si nutre di eventi, risponde a «dove siamo adesso». */
export interface Orologio {
  /**
   * Un evento del nucleo: il brano era a `posizioneMs` e la notizia è arrivata a
   * `arrivoMs`.
   *
   * `discontinuo` dichiara che la storia di prima non parla più di quel che sta
   * succedendo: un brano diverso, oppure una pausa che comincia o finisce. Un
   * salto col cursore **non** va dichiarato — lo scarto lo tradisce da sé, vedi
   * [`SALTO`].
   */
  ancora(posizioneMs: number, arrivoMs: number, discontinuo: boolean): void;
  /**
   * Dove siamo a questo istante di parete, in millisecondi dall'inizio del brano.
   *
   * Zero finché nessun evento è arrivato: prima del primo ancoraggio non c'è
   * niente da stimare, e zero è dove la barra sta comunque.
   */
  stima(oraMs: number): number;
}

/** Costruisce un orologio. Nessuno stato condiviso: uno per riproduzione. */
export function creaOrologio({ finestra, eta, salto }: OpzioniOrologio): Orologio {
  const campioni: Campione[] = [];
  /** L'origine in uso: il minimo dei campioni, o l'ancora provvisoria. */
  let origine = 0;
  /** Almeno un evento è arrivato. */
  let iniziato = false;
  /**
   * L'origine viene da un riancoraggio secco e nessun campione onesto l'ha
   * ancora sostituita. Vedi «L'onestà su un caso che rompe la regola».
   */
  let provvisoria = false;

  const riancora = (posizioneMs: number, arrivoMs: number): void => {
    campioni.length = 0;
    origine = arrivoMs - posizioneMs;
    iniziato = true;
    provvisoria = true;
  };

  /** La più bassa delle origini in finestra, che è quella meno in ritardo. */
  const piuBassa = (): number => {
    let minima = Number.POSITIVE_INFINITY;
    for (const campione of campioni) {
      if (campione.origine < minima) minima = campione.origine;
    }
    // Una finestra vuota non si verifica — chi chiama ha appena spinto — e il
    // ripiego è l'origine di prima invece di un infinito che finirebbe in una
    // sottrazione.
    return Number.isFinite(minima) ? minima : origine;
  };

  return {
    ancora(posizioneMs: number, arrivoMs: number, discontinuo: boolean): void {
      if (discontinuo || !iniziato) {
        riancora(posizioneMs, arrivoMs);
        return;
      }
      // Lo scarto fra quel che si stimava e quel che il nucleo dice. Sopra la
      // soglia non è rumore di consegna: è un salto, e la storia di prima
      // parlava di un altro punto del brano.
      if (Math.abs(arrivoMs - origine - posizioneMs) > salto) {
        riancora(posizioneMs, arrivoMs);
        return;
      }
      // Il primo campione onesto **sostituisce** l'ancora provvisoria invece di
      // affiancarla: quella poteva venire da un messaggio che diceva dove il
      // brano sarebbe stato, e il minimo la preferirebbe a qualunque verità.
      if (provvisoria) {
        campioni.length = 0;
        provvisoria = false;
      }
      campioni.push({ origine: arrivoMs - posizioneMs, quando: arrivoMs });
      while (campioni.length > finestra) campioni.shift();
      const limite = arrivoMs - eta;
      // Mai sotto un campione: senza nessuno, l'origine non avrebbe più da dove
      // venire. Un campione vecchio è meglio di nessun campione.
      while (campioni.length > 1 && (campioni[0]?.quando ?? arrivoMs) < limite) {
        campioni.shift();
      }
      origine = piuBassa();
    },

    stima(oraMs: number): number {
      if (!iniziato) return 0;
      // Mai negativa: un'origine appena dopo l'istante chiesto è possibile per
      // un millisecondo di arrotondamento, e una posizione negativa accenderebbe
      // la prima riga del testo al contrario.
      return Math.max(0, oraMs - origine);
    },
  };
}
