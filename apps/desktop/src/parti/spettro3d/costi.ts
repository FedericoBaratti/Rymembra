/**
 * Spettro3D, i costi: i budget, i tre livelli di qualità, il controllore
 * dell'automatica e la forma della cascata. Spostati invariati da `Spettro3D.tsx`,
 * la cui testata li chiama ancora «i budget e le tesi di questo modulo».
 */

import type { QualitaSpettro } from "../../ipc";

/** Ogni quanto arriva una fila, in millisecondi. È `PASSO_SPETTRO` del nucleo. */
export const PASSO_MS = 33;

/** Quanto passato tiene la scena. */
const MEMORIA_MS = 30_000;

/** Lo stesso, in eventi: è l'unità in cui la cascata conta. */
const MEMORIA_EVENTI = Math.round(MEMORIA_MS / PASSO_MS);

/**
 * Il tetto di scatole per fotogramma.
 *
 * Trentaduemila, che a quattro facce e sedici vertici l'una fanno mezzo milione
 * di vertici — tre volte quel che disegnava la scena da un secondo di memoria, su
 * uno shader che fa un `texelFetch` e qualche moltiplicazione. È il numero che
 * decide quante file per piano ci stanno, **non** quanti secondi si vedono: i
 * secondi sono trenta comunque, ed è la cascata a pagarne il conto.
 */
const SCATOLE = 32_768;

/** Quanti piani al massimo. È la misura di `cursori[]` nello shader. */
export const PIANI_MAX = 8;

/** Per quanti piani si disegna il riflesso. */
export const PIANI_RIFLESSI = 3;

/**
 * Quel che un livello di qualità decide, e quel che non decide.
 *
 * # La linea, e perché è esattamente qui
 *
 * Una skin dice **come la scena appare**; la qualità dice **quanto questa
 * macchina è disposta a spenderci**. Le manopole qui sotto sono, una per una,
 * quelle che il registro dei token si è rifiutato di esporre: la densità di
 * pixel, quanto è fine la cascata, e se il riflesso vale il suo disegno. Sono
 * costi, non gusti — il conto lo paga chi guarda, non chi ha scritto la skin — e
 * per questo nessuna di loro tocca una variabile `canvas.viz.*`. Un livello di
 * qualità che cambiasse un colore sarebbe una seconda skin nascosta in
 * Impostazioni.
 *
 * # Perché `perPiano` e non `piani`
 *
 * Perché la cascata deve continuare a coprire mezzo minuto. Togliere piani costa
 * memoria in modo brutale — la copertura è `perPiano·(2^piani−1)`, quindi
 * scendere da sette piani a quattro porta trentatré secondi a quattro — mentre
 * dimezzare le file per piano toglie **metà delle scatole** e la copertura resta
 * dov'era, perché `struttura()` si riprende il piano che avanza. A 64 barre sono
 * 56 file che diventano 32, con 33,7 secondi al posto di 33,5: il dettaglio
 * davanti passa da 264 a 132 millesimi, che è esattamente il baratto che la
 * scena fa già da sola sopra le 256 barre.
 *
 * # Perché è l'unica cosa qui che ricostruisce qualcosa
 *
 * `texStorage2D` è storage immutabile: cambiare quante file ha la cascata vuol
 * dire una texture nuova. È il motivo per cui **solo** un passaggio esplicito da
 * o verso «bassa» la rifà, e per cui il controllore automatico non ha `perPiano`
 * fra le cose che tocca — una qualità che si regola da sé ricostruendo risorse
 * grafiche mentre qualcuno guarda è peggio del problema che risolve.
 */
export interface Tetti {
  /**
   * Il tetto della densità di pixel.
   *
   * Su uno schermo a densità tripla, disegnare a densità piena vorrebbe dire
   * nove volte i pixel di una tela logica per uno sfondo. Uno e mezzo è dove la
   * scalettatura sparisce e la ventola non parte, ed è da dove parte
   * l'automatica; «alta» arriva a due, perché su uno schermo denso la differenza
   * si vede sui bordi obliqui delle scatole; «bassa» sta a uno, che è il punto
   * in cui i pixel da riempire tornano a essere quelli della tela.
   */
  densita: number;
  /** Quante file tiene al massimo il piano zero: vedi la prosa qui sopra. */
  perPiano: number;
  /** Il riflesso si può disegnare — se poi si disegni lo dice la skin. */
  riflesso: boolean;
  /** Il controllore automatico gira, e riscrive i due campi qui sopra. */
  automatico: boolean;
}

/** La densità di «alta»: il doppio della tela logica, e non oltre. */
const DENSITA_ALTA = 2;

/** Quella da cui parte l'automatica, ed è il comportamento di sempre. */
const DENSITA_AUTO = 1.5;

/** Quella di «bassa»: un pixel disegnato per un pixel di tela. */
const DENSITA_BASSA = 1;

/**
 * La scala su cui si muove l'automatica, dal più caro al più economico.
 *
 * Quattro gradini e non un controllore continuo: la densità della tela non è una
 * quantità che si regola di un filo — ogni cambio ridimensiona il buffer di
 * disegno, e farlo per un decimo di millesimo di guadagno vorrebbe dire una
 * riallocazione sulla scheda al secondo. Tre densità e poi il riflesso, che è
 * l'ultima cosa da spegnere perché è la sola che si vede sparire.
 *
 * L'ordine non è arbitrario: la densità si taglia per prima perché è quadratica
 * nei pixel — da 1,5 a 1 sono il 56 % dei frammenti in meno e la scena resta la
 * stessa scena — mentre il riflesso è metà delle chiamate di disegno ma anche
 * l'unica riga che qualcuno noterebbe mancare.
 */
export const SCALA_AUTO: readonly { densita: number; riflesso: boolean }[] = [
  { densita: DENSITA_AUTO, riflesso: true },
  { densita: 1.25, riflesso: true },
  { densita: DENSITA_BASSA, riflesso: true },
  { densita: DENSITA_BASSA, riflesso: false },
];

/** I tre livelli, come li legge questa tela. */
export const TETTI: Record<QualitaSpettro, Tetti> = {
  // Densità e riflesso di «auto» sono il primo gradino di `SCALA_AUTO`, e
  // servono soltanto finché il controllore non ha misurato niente.
  auto: { densita: DENSITA_AUTO, perPiano: 8, riflesso: true, automatico: true },
  alta: { densita: DENSITA_ALTA, perPiano: 8, riflesso: true, automatico: false },
  bassa: {
    densita: DENSITA_BASSA,
    perPiano: 4,
    riflesso: false,
    automatico: false,
  },
};

/**
 * Quanto dura una finestra di misura dell'automatica, in millisecondi.
 *
 * Due secondi sono centoventi fotogrammi a sessanta hertz: abbastanza perché un
 * novantacinquesimo percentile voglia dire qualcosa, e abbastanza pochi perché
 * chi apre una finestra pesante accanto non aspetti mezzo minuto prima di vedere
 * la scena farsi da parte.
 */
export const FINESTRA_MS = 2000;

/**
 * Quanti ritardi ci stanno in una finestra.
 *
 * Cinquecentododici coprono due secondi fino a 256 hertz. Oltre, la finestra si
 * chiude prima per pienezza invece che per tempo, e va bene così: su uno schermo
 * tanto veloce due secondi di campioni sono molti più del necessario, e il
 * percentile di cinquecento numeri è già fermo.
 */
export const RITARDI_MAX = 512;

/**
 * Sopra questo, la scena non tiene il passo.
 *
 * Ventiquattro millesimi, cioè quarantadue fotogrammi al secondo. Il numero non
 * è scelto sul frame rate ma sul **fotogramma saltato**: il ritardo fra due
 * `requestAnimationFrame` non è il tempo che la scena ci mette, è quello che ci
 * mette lo schermo, arrotondato in su al prossimo aggiornamento. Su un pannello
 * a sessanta hertz un fotogramma saltato vale 33,3 millesimi e uno tenuto 16,7,
 * e ventiquattro sta in mezzo: sopra, il cinque per cento peggiore ne ha saltato
 * almeno uno.
 *
 * È un **percentile** e non una media, perché quel che si vede come scatto è la
 * coda: una media a 18 con un fotogramma su venti a 40 descrive una scena fluida
 * che non lo è.
 *
 * # Il limite: uno schermo a trenta hertz
 *
 * Le soglie sono millesimi tarati sui sessanta hertz, e il periodo di uno
 * schermo a trenta è 33,3: ogni ritardo sta sopra ventiquattro anche quando non
 * si è perso un solo fotogramma. Lì l'automatica scende un gradino a finestra
 * fino al più basso, e ci resta, con una scena che avrebbe tenuto il passo.
 *
 * Resta così di proposito. Per tararle sul pannello bisognerebbe conoscerne la
 * frequenza vera, e quella la sa Win32 (`EnumDisplaySettings`, o DXGI) dalla
 * parte del nucleo, non la finestra. Una misura fatta solo da qui — il periodo
 * più corto fra due `requestAnimationFrame` — non distingue uno schermo a
 * trenta hertz da uno a sessanta che sotto un carico costante salta un
 * fotogramma su due: alzare le soglie per il primo vorrebbe dire non vedere
 * più il secondo, che è proprio il caso per cui l'automatica esiste. Chi ha uno
 * schermo a trenta hertz può fissare la qualità a mano.
 */
export const SOFFRE_MS = 24;

/**
 * Sotto questo, ne avanza.
 *
 * Venti millesimi, e si misura sul **novantanovesimo** percentile e non sul
 * novantacinquesimo. La differenza fra i due numeri non è una sfumatura: è
 * l'unico posto in cui questo controllore ha un'isteresi vera.
 *
 * Il ritardo fra due `requestAnimationFrame` non è una quantità continua — è un
 * multiplo del periodo dello schermo. A sessanta hertz vale 16,7 o 33,3 e niente
 * in mezzo, quindi due soglie in millesimi non lasciano nessuna banda morta:
 * qualunque coppia di numeri fra 16,7 e 33,3 divide le finestre in due, e il
 * controllore si muove a ogni finestra. Cambiando **percentile** invece che
 * soglia la banda morta ricompare, e si misura in fotogrammi saltati: si scende
 * quando ne salta più del cinque per cento, si risale quando ne salta meno
 * dell'uno, e fra l'uno e il cinque non si fa niente.
 *
 * Non dieci millesimi, che sarebbe stato il numero ovvio: a sessanta hertz una
 * scena perfettamente fluida misura 16,7 e **non un decimo di meno**, quindi una
 * soglia di risalita a dieci sarebbe irraggiungibile su metà dei computer che
 * esistono — l'automatica scenderebbe una volta e non tornerebbe mai su, che è
 * il contrario esatto di quel che la scheda in Impostazioni promette.
 */
export const AGIO_MS = 20;

/**
 * Quanto un gradino deve durare prima che se ne possa fare un altro.
 *
 * Cinque secondi. È la seconda metà dell'isteresi, e serve a un caso che la sola
 * banda morta non prende: un carico che va e viene con un periodo di pochi
 * secondi — un'altra finestra che si apre, una scansione che parte — porterebbe
 * il percentile sopra e sotto le due soglie a ogni finestra, e la scena
 * cambierebbe densità due volte al secondo sotto gli occhi di qualcuno.
 */
export const DIMORA_MS = 5000;

/**
 * E quanto può arrivare a durare, raddoppiando a ogni inversione.
 *
 * Un minuto. La banda morta e i cinque secondi non prendono un caso solo: un
 * carico che sta esattamente a cavallo delle due soglie, dove ogni gradino
 * sposta il percentile dall'altra parte e il successivo lo riporta indietro. Lì
 * l'unica cosa che ferma il pendolo è farlo rallentare: dieci secondi, poi
 * venti, poi quaranta, e in un minuto il controllore si è posato. Oltre non
 * serve andare — un minuto di attesa è già «non si muove più».
 */
export const DIMORA_MAX = 60_000;

/**
 * Quante finestre buone di fila servono per **risalire**.
 *
 * Due, contro una sola per scendere, e l'asimmetria è voluta: scendere è urgente
 * — c'è qualcuno che sta guardando una scena a scatti adesso — mentre risalire è
 * un lusso, e prenderselo un attimo dopo non costa niente a nessuno. Con la
 * dimora, una discesa e la risalita successiva distano almeno nove secondi.
 */
export const RISALITE = 2;

/**
 * Quanto si butta via dopo una discontinuità, in millisecondi.
 *
 * Un secondo. Il primo fotogramma dopo un risveglio, dopo un ridimensionamento o
 * dopo un contesto ritrovato non dice quanto costa la scena: dice che c'era una
 * texture da caricare, un buffer da riallocare, una cache di shader da
 * riscaldare. Misurarli vorrebbe dire scendere di un gradino ogni volta che si
 * tira il bordo della finestra.
 */
export const ASSESTAMENTO_MS = 1000;

/**
 * Il percentile di una finestra di ritardi, ordinandola sul posto.
 *
 * Metodo del rango più vicino: il p-esimo percentile di `n` numeri è il
 * `ceil(p·n)`-esimo più piccolo. Nessuna interpolazione, perché quel che si
 * chiede a questo numero è «quanto è lungo un fotogramma lento», e la media fra
 * due campioni vicini non è un fotogramma che sia mai esistito.
 *
 * Ordina **dentro** il vettore che riceve e non in una copia: il chiamante
 * azzera il conteggio subito dopo, quindi l'ordine d'arrivo non serve più a
 * nessuno, e una `Float32Array` nuova ogni due secondi sarebbe un'allocazione
 * ogni due secondi in un file che di allocazioni per fotogramma ne ha zero.
 */
export function percentile(
  ritardi: Float32Array,
  quanti: number,
  quota: number,
): number {
  if (quanti <= 0) return 0;
  // `sort` senza comparatore su una `Float32Array` ordina per valore e non per
  // stringa: è la differenza fra questa e un `Array`, ed è il motivo per cui non
  // serve passargli una funzione che allocherebbe una chiusura.
  const fetta = ritardi.subarray(0, quanti).sort();
  const posto = Math.min(quanti - 1, Math.max(0, Math.ceil(quota * quanti) - 1));
  return fetta[posto] ?? 0;
}

/**
 * Com'è fatta la cascata, per quante barre ci sono in larghezza.
 *
 * Il costo è il prodotto delle due, e il numero di secondi non è negoziabile:
 * quel che si stringe quando le barre crescono è **quante file per piano**, cioè
 * quanto è fine il dettaglio davanti. A 1024 barre sono quattro file per piano —
 * 132 millesimi a dettaglio pieno — e otto piani per arrivare comunque in fondo
 * ai trenta secondi.
 *
 * `tetto` è la stessa leva, tirata dall'altra parte: è quel che «bassa» abbassa
 * a quattro per stringere la cascata su una macchina che non ce la fa, senza
 * toccare i trenta secondi. Il numero di piani non è un parametro e non lo
 * diventerà — è **calcolato** perché la copertura resti quella, e da qui si
 * vede: con `perPiano` dimezzato il logaritmo restituisce un piano in più, e la
 * copertura passa da 1016 eventi a 1020. Metà delle scatole, la stessa memoria.
 */
export function struttura(
  barre: number,
  tetto: number,
): {
  perPiano: number;
  piani: number;
  file: number;
  copertura: number;
} {
  const perPiano = Math.max(
    4,
    Math.min(tetto, Math.floor(SCATOLE / (barre * PIANI_MAX))),
  );
  const piani = Math.max(
    1,
    Math.min(PIANI_MAX, Math.round(Math.log2(MEMORIA_EVENTI / perPiano + 1))),
  );
  return {
    perPiano,
    piani,
    file: perPiano * piani,
    copertura: perPiano * ((1 << piani) - 1),
  };
}
