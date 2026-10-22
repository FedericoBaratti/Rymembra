/**
 * L'orologio della scena dello spettro: quando una fila entra davanti, e quanto
 * è cresciuta — dal tempo che passa, non dall'istante in cui la fila arriva.
 *
 * # Il difetto che toglie
 *
 * Fino alla 3.0.1 la scena scorreva **ad arrivi**: la fila davanti cresceva per
 * `(adesso − quando)/intervallo`, dove `quando` era l'ultimo evento. Ogni evento
 * arrivato con tre millesimi d'anticipo faceva fare alla scena un saltino in
 * avanti; ogni evento in ritardo la fermava un attimo; una raffica — il filo
 * della finestra che si libera dopo uno stallo e consegna quattro eventi in un
 * fotogramma — la faceva saltare di quattro file. Gli eventi passano dal filo
 * della finestra di Tauri, che fa anche tutto il resto, quindi il loro ritmo non
 * è il ritmo della musica: è il ritmo della musica **più** quel che il filo
 * stava facendo. Misurato sul pannello a 60 Hz con il banco del 22-10-2026:
 * l'avanzamento per fotogramma oscillava fra 0,89 e 1,10 del dovuto anche senza
 * stalli, cioè un micro-scatto continuo che non si sa nominare ma si vede.
 *
 * # Come funziona
 *
 * È il modo standard di disegnare dati che arrivano irregolari: si disegna **un
 * filo nel passato**, con un orologio proprio. Le file arrivate aspettano in una
 * coda (la tiene la scena, questo modulo ne conosce solo la lunghezza), e la
 * scena le prende quando il **suo** tempo lo dice: la frazione avanza di
 * `dt/passo` a ogni fotogramma, e ogni volta che passa l'uno entra una fila.
 *
 * Quanto nel passato lo decide la **riserva**: quante file di musica ci sono
 * ancora davanti alla fila che sta crescendo, misurate nell'istante in cui ne
 * arriva una nuova — cioè nel punto più basso del dente di sega. La si tiene
 * attorno a un **margine** variando di poco la velocità: un po' più svelta se le
 * file si accumulano, un po' più piano se la coda si assottiglia. Il margine
 * stesso si adatta a quanto arrivano in ritardo le file: sale in fretta dopo un
 * ritardo e scende piano, perché aspettare qualche millesimo in più costa niente
 * e restare a secco costa uno scatto.
 *
 * # Quel che costa
 *
 * Il margine, e nient'altro: da un quarto di fila — otto millesimi — in un
 * filo della finestra tranquillo, a una fila e mezza dopo uno stallo. La fila
 * davanti era già una fila indietro prima (cresceva dalla precedente verso
 * l'ultima arrivata), e lo resta.
 *
 * # Quel che non nasconde
 *
 * Uno stallo più lungo del margine. Lì la coda si svuota, e la scena si ferma ad
 * aspettare come prima — ma quando le file arrivano tutte insieme **non salta**:
 * riparte da capo e recupera accelerando, fino al doppio, invece di inghiottire
 * la raffica in un fotogramma. Solo una riserva assurda, oltre [`TETTO`], si
 * scarta di colpo, perché a quel punto la scena starebbe mostrando musica di
 * mezzo secondo fa.
 *
 * Funzioni pure del tempo che gli si passa: nessun `performance.now()` qui
 * dentro, ed è quel che permette di provarlo con un tempo inventato.
 */

/** Il margine più piccolo, in file: un quarto, cioè otto millesimi a 30 Hz. */
const MARGINE_MIN = 0.25;

/** Il margine più grande, in file. Oltre, il ritardo si vedrebbe come tale. */
const MARGINE_MAX = 1.5;

/** Quanto in fretta il margine sale dopo un ritardo, per arrivo. */
const MARGINE_SALE = 0.5;

/**
 * Quanto piano scende, per arrivo.
 *
 * Un duecentesimo: quasi cinque secondi per dimezzare la distanza a 30 Hz. Con
 * un cinquantesimo — la prima scelta — un filo della finestra che inciampa una
 * volta ogni secondo e mezzo trovava il margine già sceso a ogni inciampo, e la
 * scena restava a secco per un fotogramma proprio lì: la prova del tremolio lo
 * coglie.
 */
const MARGINE_SCENDE = 0.005;

/**
 * Quanto un ritardo d'arrivo pesa sul margine che chiede.
 *
 * Uno e mezzo: il ritardo misurato è quello di un evento, e il prossimo può
 * essere un po' peggio.
 */
const MARGINE_PER_RITARDO = 1.5;

/** Quanto la riserva media insegue quella appena misurata, per arrivo. */
const RISERVA_INSEGUE = 0.2;

/**
 * Quanto la velocità risponde a una fila di riserva in più o in meno.
 *
 * Un quinto: con una fila di troppo la scena va al 120 %, e la differenza si
 * chiude in qualche decimo di secondo senza che si veda accelerare. Con cinque
 * file di troppo — una raffica dopo uno stallo — si arriva al doppio.
 */
const GUADAGNO = 0.2;

/** La velocità più bassa: la scena rallenta al più di un settimo. */
const VELOCITA_MIN = 0.85;

/** La più alta: il recupero dopo uno stallo, al doppio. */
const VELOCITA_MAX = 2;

/**
 * La riserva oltre la quale si scarta invece di recuperare, in file oltre il
 * margine.
 *
 * Dodici file sono quattro decimi di secondo: una scena così indietro sta
 * mostrando una musica che si è già finita di sentire, e recuperarla al doppio
 * della velocità vorrebbe dire quasi mezzo secondo di avanti veloce.
 */
const TETTO = 12;

/** Il tetto di un fotogramma, in millisecondi: un risveglio non è tempo da recuperare. */
const DT_MAX = 100;

/** Quel che un fotogramma deve fare: quante file prendere, e dove sta la fila davanti. */
export interface PassoOrologio {
  /** Quante file togliere dalla coda e spingere nella cascata, in ordine. */
  prendi: number;
  /** Quanto è cresciuta la fila davanti, in `[0, 1]`: la uniform `frazione`. */
  frazione: number;
}

export class OrologioSpettro {
  /** Quanto dura una fila, in millisecondi. */
  private readonly passo: number;
  private frazione = 1;
  /** Ferma ad aspettare una fila: la coda si è svuotata, o non è ancora partita. */
  private fermo = true;
  /** L'ultimo fotogramma, o −1 se non ce n'è stato ancora uno. */
  private ultimo = -1;
  /** L'ultimo arrivo, o −1. */
  private arrivoPrima = -1;
  private velocita = 1;
  private margine = MARGINE_MIN;
  private riservaMedia = MARGINE_MIN;

  constructor(passoMs: number) {
    this.passo = passoMs;
  }

  /**
   * Ricomincia da capo: la scena è ferma, e la prima fila che arriva entra
   * piccola e cresce.
   *
   * La chiama la scena ogni volta che la coda smette di avere un senso: il
   * ciclo che si ferma (e la versa nella cascata), un cambio di risoluzione, un
   * contesto ritrovato. Il margine **non** si azzera: è una cosa che si è
   * imparata sul filo della finestra, non sulla scena.
   */
  azzera(): void {
    this.frazione = 1;
    this.fermo = true;
    this.ultimo = -1;
    this.arrivoPrima = -1;
    this.velocita = 1;
    this.riservaMedia = this.margine;
  }

  /**
   * È arrivata una fila. `inCoda` è quante ce n'erano **prima** di lei.
   *
   * Qui si misurano le due cose che regolano l'orologio: la riserva, che è
   * minima esattamente adesso — un istante prima che la fila nuova la alzi di
   * uno — e il ritardo di questo arrivo rispetto al passo, che dice quanto
   * margine serve.
   */
  arrivo(adesso: number, inCoda: number): void {
    if (this.arrivoPrima >= 0) {
      const ritardo = Math.max(0, adesso - this.arrivoPrima - this.passo) / this.passo;
      const voluto = Math.min(MARGINE_MAX, Math.max(MARGINE_MIN, ritardo * MARGINE_PER_RITARDO));
      const quanto = voluto > this.margine ? MARGINE_SALE : MARGINE_SCENDE;
      this.margine += (voluto - this.margine) * quanto;
    }
    this.arrivoPrima = adesso;

    // La fila davanti, adesso: quella del fotogramma scorso, più il tempo
    // passato da allora alla velocità di allora. Ferma, è cresciuta del tutto.
    const trascorso = this.ultimo < 0 ? 0 : Math.max(0, adesso - this.ultimo);
    const frazione = this.fermo
      ? 1
      : Math.min(1, this.frazione + (trascorso / this.passo) * this.velocita);
    const riserva = inCoda + (1 - frazione);
    this.riservaMedia += (riserva - this.riservaMedia) * RISERVA_INSEGUE;
  }

  /**
   * Un fotogramma: quante file prendere dalla coda, e la frazione da disegnare.
   *
   * `inCoda` è quante file aspettano adesso. Chi chiama deve prenderne
   * esattamente `prendi`, prima di caricare cursori, fase e frazione — è
   * l'ordine che tiene la fila davanti attaccata a quella appena entrata.
   */
  avanza(adesso: number, inCoda: number): PassoOrologio {
    const dt = this.ultimo < 0 ? 0 : Math.min(DT_MAX, Math.max(0, adesso - this.ultimo));
    this.ultimo = adesso;
    this.velocita = Math.min(
      VELOCITA_MAX,
      Math.max(VELOCITA_MIN, 1 + GUADAGNO * (this.riservaMedia - this.margine)),
    );

    let prendi = 0;
    let frazione = this.frazione + (dt / this.passo) * this.velocita;
    if (this.fermo) {
      // Ripartire da capo, e non da dove l'orologio sarebbe arrivato: il tempo
      // passato ad aspettare non è musica da saltare. Il recupero lo farà la
      // velocità, con calma.
      if (inCoda > 0) {
        prendi = 1;
        frazione = 0;
        this.fermo = false;
      } else {
        frazione = 1;
      }
    }
    while (!this.fermo && frazione >= 1) {
      if (inCoda - prendi > 0) {
        prendi += 1;
        frazione -= 1;
      } else {
        frazione = 1;
        this.fermo = true;
      }
    }

    // Una riserva assurda si scarta, lasciandone il margine più una fila.
    const riserva = inCoda - prendi + (1 - frazione);
    if (riserva > this.margine + TETTO) {
      const via = Math.min(inCoda - prendi, Math.floor(riserva - this.margine - 1));
      prendi += Math.max(0, via);
      this.riservaMedia = inCoda - prendi + (1 - frazione);
    }

    this.frazione = frazione;
    return { prendi, frazione };
  }
}
