/**
 * La scena dello spettro: le stesse bande, in profondità, e mezzo minuto di
 * memoria — dipinta dalla skin, e ferma quando nessuno guarda.
 *
 * # Cosa si vede
 *
 * Una griglia di barre: da otto a 1024 in larghezza — lo sceglie chi guarda — e
 * una cinquantina di file in profondità. La fila davanti è adesso; quelle dietro
 * sono il passato, che arretra verso l'orizzonte e si spegne. Sotto, il riflesso.
 *
 * È il motivo per cui lo sfondo sembra muoversi a tempo: non è un'animazione *a
 * tempo di musica*, è la musica di qualche secondo fa che se ne va all'indietro.
 * E la camera che oscilla lo fa con l'energia delle bande basse, cioè con la
 * grancassa — ferma la musica, si ferma anche lei.
 *
 * # Trenta secondi, e come ci stanno
 *
 * Una fila nasce ogni trentatré millesimi, quindi mezzo minuto sono novecento
 * file. A 1024 barre sarebbero novecentomila scatole per fotogramma, cioè non
 * sono. E disegnarne meno tenendo il passo fisso vorrebbe dire una fila ogni
 * secondo: il fronte della scena si fermerebbe, che è l'opposto di quel che
 * mezzo minuto di memoria serve a mostrare.
 *
 * La scena tiene allora una **cascata di anelli**. Il piano zero ha `perPiano`
 * file, una per evento. La fila che cade dal fondo di un anello non si butta:
 * entra nell'accumulatore del piano di sopra, che ne prende il **massimo a due a
 * due** e ne spinge una nel suo anello. Quindi il piano ℓ tiene file larghe
 * `2^ℓ` eventi e copre le età da `perPiano·(2^ℓ−1)` a `perPiano·(2^(ℓ+1)−1)`:
 * non si sovrappongono, non lasciano buchi, e sette piani da otto file coprono
 * 1016 eventi — trentatré secondi — con cinquantasei file da disegnare.
 *
 * Il vicino è a dettaglio pieno e il lontano è riassunto, che è esattamente il
 * modo in cui si guarda: gli ultimi due decimi di secondo si leggono barra per
 * barra, mezzo minuto fa serve la forma.
 *
 * La cascata gira nel **gestore dell'evento**, non nel ciclo di disegno. È una
 * correzione, non un dettaglio: quando stava di là, «è arrivata una fila» era un
 * booleano invece di una coda, quindi su uno schermo a trenta hertz una fila su
 * tante spariva senza mai entrare nella memoria — e con il ciclo addormentato
 * non ne entrava nessuna. Adesso il passato è esatto e indipendente dal numero
 * di fotogrammi al secondo, che è la sola cosa che «mezzo minuto di memoria»
 * poteva promettere.
 *
 * # Perché la profondità è un logaritmo
 *
 * Perché è ciò che rende uniforme la cascata. Il piano ℓ finisce all'età
 * `perPiano·(2^(ℓ+1)−1)`, dove `1 + età/perPiano` fa esattamente `2^(ℓ+1)`:
 * `log2` vale `ℓ+1`, e **ogni piano si prende la stessa fetta di profondità**
 * mentre il tempo che porta raddoppia. Le file escono spaziate uguali sullo
 * schermo senza che nessuno le abbia sistemate a mano, e il lontano si comprime
 * nel tempo invece che nello spazio — cioè non diventa una sbavatura.
 *
 * Una fila, appena nata, arretra in fretta e poi sempre più piano. È il
 * comportamento giusto per due ragioni: la prospettiva fa già la stessa cosa, e
 * il passato remoto è quel che interessa meno vedere muovere.
 *
 * # Perché WebGL e non una canvas 2D
 *
 * Per il numero. Trentamila scatole con tre facce visibili sarebbero in 2D
 * novantamila percorsi da riempire a ogni fotogramma, cioè il filo principale
 * occupato per sempre. In WebGL sono **due chiamate di disegno** — la scena e il
 * suo riflesso — perché la geometria è una sola scatola disegnata trentamila
 * volte, con `gl_InstanceID` a dire dove. E sono **una sola** quando la skin
 * mette il riflesso a zero, che è un caso vero e non teorico: il preset piatto
 * lo fa.
 *
 * Le altezze non passano da un vettore riscritto ogni volta: stanno in una
 * texture di un byte per banda, e ogni evento ne carica **due righe in media** —
 * quella nuova del piano zero, più i piani che in quel momento chiudono una
 * fila. Le altre restano dov'erano, e la fila che avanza è un cursore che
 * cambia, non un vettore che scorre. È ciò che rende leggera una cosa che sembra
 * costosa.
 *
 * # Quel che la skin dipinge, e quel che no
 *
 * Diciotto token `canvas.viz.*` arrivano qui come variabili CSS: tre colori, la
 * geometria della stanza (larghezza, profondità, altezza delle barre, quanto una
 * barra riempie la sua fetta), l'obiettivo e l'occhio, la foschia, il riflesso,
 * la luce diffusa, quanto gira la camera e quanto la spinge il colpo, e i tre
 * numeri che dicono come una barra sale, come scende e dove ha il fondo scala.
 * Con la 2.2 ne arrivavano tre, e per una skin di serie valevano tutti lo stesso
 * colore: le due `mix()` dello shader erano due no-op e la scena era l'accento
 * moltiplicato per la luce.
 *
 * **Non** sono token, e sono le uniche cose che non lo sono: `MEMORIA_MS`,
 * `PIANI_MAX`, `PIANI_RIFLESSI`, `SCATOLE` e `PASSO_MS`, che sono i budget e le
 * tesi di questo modulo — e sono anche gli unici valori che cambiando
 * richiederebbero di ricostruire le risorse invece di scrivere una uniform;
 * l'ottavo basso `length >> 3`, che è una regola di correttezza sulle frequenze
 * e non un gusto; la direzione della luce, tre numeri per cui non esiste un
 * controllo che calzi e di cui uno sbagliato dà una scena nera; e l'altezza
 * minima `0.004`, che è la garanzia «c'è, e adesso è a zero».
 *
 * # Quel che decide la macchina, e non la skin
 *
 * C'è una terza categoria, e sta fra le due: la densità di pixel, quanto è fine
 * la cascata, e se il riflesso vale il suo disegno. Non sono token — il conto lo
 * paga chi guarda, non chi ha scritto la skin — e non sono nemmeno costanti:
 * sono un'impostazione, `player.spectrum.quality`, che arriva qui come la prop
 * `qualita` e che vale `automatica`, `alta` o `bassa`. `Tetti` è quel che ognuno
 * dei tre livelli decide, e la prosa lì sopra è la linea per intero.
 *
 * L'automatica misura: una finestra di due secondi di ritardi fra fotogrammi, il
 * novantacinquesimo percentile, e un gradino se sta fuori da una banda morta —
 * con cinque secondi di dimora, che raddoppiano a ogni inversione, perché una
 * qualità che oscilla è peggio di una qualità sbagliata. E tocca **solo** la
 * densità e il riflesso: mai quante file ha la cascata, che è l'unica delle tre
 * che ricostruisce delle risorse grafiche. Una scena che si rifà da sola mentre
 * qualcuno la guarda sarebbe peggio del problema che sta risolvendo.
 *
 * # Come si leggono i token, e perché non a ogni fotogramma
 *
 * `getComputedStyle` restituisce una dichiarazione **viva**: rileggerla basta
 * perché una skin nuova arrivi qui senza rimontare niente, ed è quel che rende
 * possibile trascinare un cursore nello Studio e vedere la scena cambiare. Ma
 * `getPropertyValue` su una dichiarazione viva **forza il ricalcolo dello stile**
 * se il documento è sporco, e lo Scrubber lo sporca venti volte al secondo. Con
 * diciotto token era il costo dominante del fotogramma, e il commento che stava
 * qui — «costa niente» — era falso.
 *
 * Quindi si legge **per invalidazione**: un flag che si alza da tre osservatori
 * di mutazioni (la radice, dove `tema.ts` scrive tema, densità e movimento; il
 * più vicino antenato `[data-skin]`, che nello Studio è il riquadro
 * dell'anteprima; e il genitore della tela, l'unico modo di vedere il foglio in
 * prova che React monta **accanto**), da una rete di sicurezza mezzo secondo, e
 * da ogni risveglio. Il confronto è **per token e non su una firma unica**: con
 * una firma, cambiare un cursore vorrebbe dire riconvertire diciotto valori
 * invece di uno.
 *
 * # Quel che non c'è, e perché
 *
 * Nessuna sfocatura, nessun bagliore in post-produzione, nessuna ombra: sono le
 * tre cose che costano una seconda passata su tutti i pixel dello schermo, e su
 * un portatile si pagano in ventola e in batteria per un lettore che sta
 * suonando in sottofondo. La luce è una lambertiana per faccia calcolata nel
 * vertice, e il bagliore è il colore della cresta della skin sulla cima delle
 * barre.
 *
 * Non c'è nemmeno la faccia di dietro delle scatole: la camera sta sempre a
 * `z > 0` e le file a `z ≤ 0`, quindi quella faccia è già buttata dal culling —
 * tenerla vorrebbe dire quattro vertici trasformati per niente su ognuna delle
 * trentamila. E il riflesso si disegna solo per i primi tre piani: a un'alfa che
 * è una frazione di quella della scena, il riflesso di venti secondi fa non si
 * vede.
 *
 * Non si è toccata la **mappatura in frequenza**, che è già giusta: bande a
 * rapporto costante fra 20 Hz e 20 kHz, che su rumore rosa danno un display
 * piatto senza nessuna ponderazione. Non si è tolto `antialias`, che è la scelta
 * giusta su un aliasing puramente geometrico. Non si è tolto `powerPreference:
 * "low-power"`, che è quasi un no-op — i browser ci sono passati di default da
 * anni — e che comunque dice la cosa giusta: svegliare la scheda grossa per lo
 * sfondo di un lettore è precisamente quel che il paragrafo qui sopra rifiuta. E
 * non si è unificato il capovolgimento di `frontFace` fra i due disegni: è una
 * riga per chiamata contro una geometria in doppia copia.
 *
 * # La macchina del riposo
 *
 * Quattro stati. **Vivo** disegna. **Assopito** è il silenzio più profondo della
 * memoria: il passato è uscito dalla scena, non c'è più niente da far scorrere,
 * e ridisegnare sessanta volte al secondo una stanza vuota è il modo più sicuro
 * di far girare la ventola durante una pausa. **Nascosto** è la finestra
 * minimizzata o in secondo piano. **Perso** è il contesto che se ne va.
 *
 * Il riarmo del `requestAnimationFrame` sta **dopo** tutte le uscite anticipate,
 * e non prima come stava: prima il ciclo non si fermava mai — l'uscita per il
 * silenzio c'era, ma il fotogramma successivo era già chiesto, quindi costava
 * una sveglia del compositore a ogni fotogramma per non fare niente. È la stessa
 * correzione, e la stessa disciplina, di `riproduzione.ts:205-227`.
 *
 * Il cinturino contro il difetto peggiore — la scena nera con la musica che
 * suona — è che **ogni** evento con un massimo sopra la soglia del fruscio
 * chiama `sveglia()`, incondizionatamente e prima di qualunque altro controllo.
 * `sveglia()` è idempotente: se il ciclo gira già, esce subito.
 *
 * La pausa **non** ferma la scena, e il paragrafo sui trenta secondi dice
 * perché: il passato deve *uscire* invece di congelarsi a metà strada. A fermare
 * il lavoro è la regola del silenzio, che arriva da sola trentatré secondi dopo.
 *
 * # La presa nel motore la accende questa
 *
 * `ipc.spettro(true)` sta qui, e non altrove: questa tela è l'unico pezzo che
 * guarda le bande, e la presa nel motore deve vivere esattamente quanto lei.
 * Adesso però non vive **solo** quanto lei: la si chiude anche a finestra
 * nascosta, e ad assopita se la riproduzione è ferma. Sono trenta trasformate da
 * 4096 punti al secondo che spariscono, ed è la voce di CPU più grossa che
 * questa schermata sappia togliere.
 *
 * Ad assopita **con la musica che suona** il rubinetto resta aperto, e non è una
 * dimenticanza: il risveglio arriva dagli eventi dello spettro e non c'è nient'
 * altro che lo sostituisca. Chiuderlo lì vorrebbe dire una scena che non torna.
 *
 * Un solo proprietario, [`Spettro3D`]`.rubinetto`, che ricorda lo stato: con due
 * chiamanti si finisce a spegnerlo due volte — o a non spegnerlo affatto, che è
 * peggio, perché il sintomo è una cadenza da 33 ms che resta per tutta la vita
 * della finestra.
 *
 * # Se il contesto se ne va
 *
 * La scheda si riavvia, il portatile passa all'altra, il driver si aggiorna:
 * `webglcontextlost` azzera tutti i manici — cancellarli chiamerebbe una scheda
 * che non c'è più — e `webglcontextrestored` ricostruisce le due risorse.
 *
 * **La memoria sopravvive.** Gli anelli della cascata stanno in memoria normale,
 * non nella texture, quindi mezzo minuto di passato attraversa un riavvio del
 * driver e viene ricaricato riga per riga nella texture nuova. Prima non c'era
 * nessun ripristino: la scena restava nera fino a che qualcuno non chiudeva e
 * riapriva la schermata.
 *
 * # Se WebGL non c'è
 *
 * Non c'è nemmeno la scena, e non succede altro: lo sfondo resta la tinta della
 * copertina, cioè quel che c'era prima. Un ripiego in 2D sarebbe una seconda
 * implementazione da tenere allineata per una macchina che, se non ha WebGL 2,
 * non ha nemmeno il fiato per trentamila rettangoli disegnati a mano.
 *
 * E in quel caso **il rubinetto non si apre**. Prima si apriva lo stesso, perché
 * stava in un effetto che non sapeva niente della tela: il motore faceva trenta
 * trasformate al secondo per una scena che non disegnava niente.
 *
 * # La scena finta, e perché non è una seconda scena
 *
 * Con `sorgente: "finto"` le file non arrivano dal motore: le fabbrica un
 * generatore deterministico, in forma chiusa sul numero dell'evento e senza
 * allocare. La presa non si apre, l'ascolto non si registra, e **tutto il
 * resto** — lo smorzamento a cadenza misurata, la cascata, il caricamento della
 * texture, il seguipulsazioni, la lettura dei token — è la stessa strada, riga
 * per riga.
 *
 * Serve allo Studio, dove si dipinge una skin e non c'è nessun audio da
 * guardare. E la ragione per cui la sorgente si scambia invece di scrivere una
 * seconda tela è che una seconda tela mostrerebbe una skin che non è quella che
 * si sta dipingendo: basterebbe una uniform scritta di là in un altro ordine.
 * Essendo funzione del solo numero dell'evento, due autori su due macchine alla
 * fila 37 vedono la stessa fila — che è ciò che rende una schermata un documento
 * su cui si può discutere.
 *
 * Una sorgente sintetica **non tace mai**, quindi la regola del silenzio non la
 * ferma: al suo posto ci sono tre condizioni di arresto — lo smontaggio, la
 * finestra nascosta, e il riquadro uscito dalla vista — e la seconda non è una
 * gentilezza, è l'unica cosa fra lo Studio lasciato aperto dietro un'altra
 * finestra e una scheda video occupata per sempre.
 */
import { useCallback, useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { ipc, type BandeSpettro, type QualitaSpettro } from "../ipc";
import { useAscolto } from "../pagine";

/** Ogni quanto arriva una fila, in millisecondi. È `PASSO_SPETTRO` del nucleo. */
const PASSO_MS = 33;

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
const PIANI_MAX = 8;

/** Per quanti piani si disegna il riflesso. */
const PIANI_RIFLESSI = 3;

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
interface Tetti {
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
const SCALA_AUTO: readonly { densita: number; riflesso: boolean }[] = [
  { densita: DENSITA_AUTO, riflesso: true },
  { densita: 1.25, riflesso: true },
  { densita: DENSITA_BASSA, riflesso: true },
  { densita: DENSITA_BASSA, riflesso: false },
];

/** I tre livelli, come li legge questa tela. */
const TETTI: Record<QualitaSpettro, Tetti> = {
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
const FINESTRA_MS = 2000;

/**
 * Quanti ritardi ci stanno in una finestra.
 *
 * Cinquecentododici coprono due secondi fino a 256 hertz. Oltre, la finestra si
 * chiude prima per pienezza invece che per tempo, e va bene così: su uno schermo
 * tanto veloce due secondi di campioni sono molti più del necessario, e il
 * percentile di cinquecento numeri è già fermo.
 */
const RITARDI_MAX = 512;

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
 */
const SOFFRE_MS = 24;

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
const AGIO_MS = 20;

/**
 * Quanto un gradino deve durare prima che se ne possa fare un altro.
 *
 * Cinque secondi. È la seconda metà dell'isteresi, e serve a un caso che la sola
 * banda morta non prende: un carico che va e viene con un periodo di pochi
 * secondi — un'altra finestra che si apre, una scansione che parte — porterebbe
 * il percentile sopra e sotto le due soglie a ogni finestra, e la scena
 * cambierebbe densità due volte al secondo sotto gli occhi di qualcuno.
 */
const DIMORA_MS = 5000;

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
const DIMORA_MAX = 60_000;

/**
 * Quante finestre buone di fila servono per **risalire**.
 *
 * Due, contro una sola per scendere, e l'asimmetria è voluta: scendere è urgente
 * — c'è qualcuno che sta guardando una scena a scatti adesso — mentre risalire è
 * un lusso, e prenderselo un attimo dopo non costa niente a nessuno. Con la
 * dimora, una discesa e la risalita successiva distano almeno nove secondi.
 */
const RISALITE = 2;

/**
 * Quanto si butta via dopo una discontinuità, in millisecondi.
 *
 * Un secondo. Il primo fotogramma dopo un risveglio, dopo un ridimensionamento o
 * dopo un contesto ritrovato non dice quanto costa la scena: dice che c'era una
 * texture da caricare, un buffer da riallocare, una cache di shader da
 * riscaldare. Misurarli vorrebbe dire scendere di un gradino ogni volta che si
 * tira il bordo della finestra.
 */
const ASSESTAMENTO_MS = 1000;

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
function percentile(
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
 * Il fruscio della quantizzazione, sul byte che arriva dal filo.
 *
 * Quattro su 255 non è un suono. È la soglia che decide se un evento sveglia la
 * scena, e si misura sul byte **grezzo** e non sul livello rimappato: altrimenti
 * alzare `canvas.viz.floor` addormenterebbe la scena su un brano piano, che è
 * l'esatto contrario di quel che quel token promette.
 */
const FRUSCIO = 4;

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
function struttura(
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

/**
 * I numeri della skin che non finiscono in una uniform.
 *
 * Sono quelli che servono a **questo** codice: la camera, il filtro delle barre,
 * il fondo scala, la dissolvenza d'apertura, e i due che decidono se si disegna
 * il riflesso e se ci si muove affatto. Tutti gli altri token vanno dritti in
 * una uniform e non si rileggono più, quindi non hanno bisogno di stare qui.
 *
 * Un record a forma fissa e non un vettore indicizzato: i nomi li controlla il
 * compilatore, e un token aggiunto nel posto sbagliato non diventa
 * silenziosamente un altro.
 */
interface Manopole {
  /** L'apertura dell'obiettivo, in gradi. */
  lente: number;
  /** L'altezza dell'occhio, in unità di mondo. */
  occhio: number;
  /** Quanto gira la camera. */
  oscillazione: number;
  /** Quanto il colpo la spinge. */
  pulsazione: number;
  /** La costante di tempo della salita di una barra, in millisecondi. */
  salita: number;
  /** Quella della discesa. */
  discesa: number;
  /** Il fondo scala, in decibel. Non sotto −70: là non c'è informazione. */
  fondo: number;
  /** La forza del riflesso: sotto una soglia, il suo disegno si salta. */
  riflesso: number;
  /** Quanto dura la dissolvenza d'apertura, in millisecondi. */
  dissolvenza: number;
  /** La scala del movimento della skin: a zero, la stanza sta ferma. */
  moto: number;
}

/**
 * I valori di serie, che sono anche il ripiego.
 *
 * Servono a due cose: a partire prima di aver letto qualunque cosa, e a
 * rispondere al gestore dell'evento nell'istante in cui il contesto è perso e le
 * risorse non ci sono. Sono gli stessi numeri del registro delle skin, e
 * ripeterli qui è il prezzo di non dover leggere il foglio di stile per sapere
 * come smorzare una barra.
 */
const SERIE: Manopole = {
  lente: 52,
  occhio: 0.72,
  oscillazione: 1,
  pulsazione: 1,
  salita: 41,
  discesa: 258,
  fondo: -70,
  riflesso: 0.22,
  dissolvenza: 450,
  moto: 1,
};

/** Una copia fresca dei valori di serie. */
function nuoveManopole(): Manopole {
  return { ...SERIE };
}

/** Una voce del registro dei token, come la legge questo file. */
type Voce =
  | {
      /** Il nome della variabile CSS. */
      css: string;
      colore: true;
      /** Che colore vale se la skin non l'ha dichiarata. */
      ripiego: string;
    }
  | {
      css: string;
      colore: false;
      /** I limiti del registro, ripetuti qui come rete. */
      min: number;
      max: number;
      /** Quanto vale se non c'è, o se c'è e non è un numero. */
      base: number;
    };

/**
 * I token che dipingono la scena, e i due prestiti dal resto del registro.
 *
 * L'ordine di questo elenco è l'ordine dei due vettori paralleli che tengono il
 * valore grezzo di ciascuno: è per questo che il confronto costa una stringa per
 * token invece di una firma sola, ed è il motivo per cui trascinare un cursore
 * nello Studio costa **una** pipetta e non diciotto.
 *
 * I limiti sono ripetuti qui e non importati: il registro li valida quando una
 * skin si carica, ma queste variabili sono raggiungibili anche da un foglio
 * scritto a mano che il validatore non ha mai visto, e una profondità negativa
 * dividerebbe per zero dentro lo shader.
 *
 * `--dur-3` e `--motion-scale` non sono token del blocco `canvas.viz`: sono la
 * durata media delle transizioni e la scala del movimento, cioè due cose che
 * l'app già rispetta ovunque tranne che qui. `transizione.ts:56-63` considera la
 * seconda vincolante; una skin `motion: none` fermava tutto e non fermava
 * l'oscillazione di questa stanza.
 */
const TOKEN: readonly Voce[] = [
  // I tre colori. Il ripiego è il valore della skin di serie: quando manca la
  // variabile manca il foglio, non la skin, e dipingere di nero sarebbe la
  // stessa scena nera che tutto questo file cerca di non avere.
  { css: "--viz-primary", colore: true, ripiego: "#8b7cf6" },
  { css: "--viz-secondary", colore: true, ripiego: "#1d1b3a" },
  { css: "--viz-tip", colore: true, ripiego: "#e8e2ff" },
  { css: "--viz-glow", colore: false, min: 0, max: 100, base: 20 },
  { css: "--viz-width", colore: false, min: 1, max: 12, base: 4.6 },
  { css: "--viz-depth", colore: false, min: 1, max: 16, base: 5.2 },
  { css: "--viz-height", colore: false, min: 0, max: 3, base: 0.62 },
  { css: "--viz-fill", colore: false, min: 0.1, max: 1, base: 0.7 },
  { css: "--viz-lens", colore: false, min: 24, max: 90, base: 52 },
  { css: "--viz-eye", colore: false, min: 0, max: 2, base: 0.72 },
  { css: "--viz-haze", colore: false, min: 0, max: 0.9, base: 0.55 },
  { css: "--viz-reflection", colore: false, min: 0, max: 1, base: 0.22 },
  { css: "--viz-ambient", colore: false, min: 0, max: 1, base: 0.42 },
  { css: "--viz-sway", colore: false, min: 0, max: 2, base: 1 },
  { css: "--viz-beat", colore: false, min: 0, max: 2, base: 1 },
  { css: "--viz-attack", colore: false, min: 0, max: 400, base: 41 },
  { css: "--viz-release", colore: false, min: 20, max: 2000, base: 258 },
  { css: "--viz-floor", colore: false, min: -70, max: -24, base: -70 },
  { css: "--dur-3", colore: false, min: 0, max: 4000, base: 450 },
  { css: "--motion-scale", colore: false, min: 0, max: 4, base: 1 },
];

/**
 * Il valore grezzo che nessuna dichiarazione può avere.
 *
 * Serve a distinguere «non l'ho ancora letto» da «l'ho letto e non c'era», che
 * sono due cose diverse: la stringa vuota è il secondo, ed è un valore legittimo
 * — `--motion-scale` non è dichiarata nel foglio di serie. Partendo da vuoto, un
 * token assente non sarebbe mai stato applicato, la sua uniform sarebbe rimasta
 * a zero, e una profondità zero è una divisione per zero dentro lo shader, cioè
 * una scena nera.
 *
 * Uno spazio, perché quel che si confronta è già passato da `trim()`: uno spazio
 * è l'unica stringa che di là non può uscire.
 */
const MAI = " ";

/**
 * Un colore CSS qualunque, in tre numeri fra zero e uno.
 *
 * Passa da una canvas di un pixel e non da un'espressione regolare: un token può
 * essere `#abc`, `rgb(… / .35)`, `oklch(…)` o il nome di un colore, e l'unica
 * cosa che sa leggerli tutti è il parser del browser. Costa un `getImageData` su
 * un pixel, e si paga solo quando quel token è cambiato davvero.
 *
 * Scrive in un vettore prestato invece di restituirne uno: tre numeri allocati
 * per ogni cambio di skin non sono niente, tre numeri allocati per ogni
 * fotogramma erano il difetto di prima.
 */
function tinta(
  pennello: CanvasRenderingContext2D,
  colore: string,
  fuori: Float32Array,
): void {
  pennello.clearRect(0, 0, 1, 1);
  // Due assegnazioni: `fillStyle` ignora in silenzio un colore che non sa
  // leggere, e senza il nero davanti si finirebbe a dipingere con quel che
  // c'era prima invece di accorgersene.
  pennello.fillStyle = "#000";
  pennello.fillStyle = colore;
  pennello.fillRect(0, 0, 1, 1);
  const dati = pennello.getImageData(0, 0, 1, 1).data;
  fuori[0] = (dati[0] ?? 0) / 255;
  fuori[1] = (dati[1] ?? 0) / 255;
  fuori[2] = (dati[2] ?? 0) / 255;
}

const VERTICE = `#version 300 es
precision highp float;

in vec3 posizione;
in vec3 normale;

uniform mat4 camera;
uniform sampler2D altezze;
uniform int colonne;
uniform int perPiano;
uniform int piani;
uniform int cursori[8];
uniform int fase;
uniform float frazione;
uniform float prof;
uniform float larghezza;
uniform float spessore;
uniform float alta;
uniform float specchio;
uniform float foschia;
uniform float ambiente;
uniform float riflesso;

out float vLivello;
out float vSu;
out float vLuce;
out float vAlfa;

// Dove sta un'età, in profondità. L'età è in eventi e non in secondi, perché è
// in eventi che la cascata conta.
//
// Il logaritmo non è una curva scelta a occhio: il piano ℓ finisce all'età
// perPiano·(2^(ℓ+1)−1), dove 1 + età/perPiano fa esattamente 2^(ℓ+1) — quindi
// log2 vale ℓ+1, e ogni piano si prende la stessa fetta di profondità mentre il
// tempo che porta raddoppia. Le file escono spaziate uguali senza che nessuno le
// abbia sistemate, e l'ultima arriva esattamente a -prof.
float zDi(float eta) {
  return -prof * log2(1.0 + eta / float(perPiano)) / float(piani);
}

// L'età del fronte di una fila, in eventi.
//
// Il progresso per piano è ciò che tiene lo scorrimento continuo: quando un
// piano chiude una fila il contenuto arretra di un posto **e** il progresso
// torna a zero, e le due cose si annullano.
//
// Sta in una funzione, e non dentro main, perché la serve anche la fila dietro:
// è da lì che ognuna prende il proprio fondo. Risponde anche per la fila che
// non esiste, quella dopo l'ultima — è il piano dopo l'ultimo, e la sua età cade
// appena oltre -prof, cioè esattamente dove la scena deve finire. Niente
// apostrofi inversi in questo commento, né in nessun altro qui dentro: lo
// shader è una stringa a modello, e uno solo la chiuderebbe a metà.
float etaDi(int fila) {
  int piano = fila / perPiano;
  int dentro = fila - piano * perPiano;
  int passo = 1 << piano;
  return float(perPiano * (passo - 1) + dentro * passo + fase % passo) + frazione;
}

void main() {
  int colonna = gl_InstanceID % colonne;
  int fila = gl_InstanceID / colonne;

  int piano = fila / perPiano;
  int dentro = fila - piano * perPiano;
  // Nella texture le righe non scorrono: scorre il cursore del piano, e qui si
  // torna indietro di «dentro» partendo da lui.
  int riga = piano * perPiano + (cursori[piano] - dentro + perPiano) % perPiano;
  float livello = texelFetch(altezze, ivec2(colonna, riga), 0).r;

  // La fila davanti non compare: cresce da quella dietro. Nasce con l'altezza
  // che aveva la precedente — cioè indistinguibile da un allungamento del
  // terreno — e nei trentatré millesimi prima della prossima diventa la sua.
  // Prima appariva già alta, trenta volte al secondo: quello era lo scatto.
  if (fila == 0) {
    int dietro = (cursori[0] - 1 + perPiano) % perPiano;
    float precedente = texelFetch(altezze, ivec2(colonna, dietro), 0).r;
    livello = mix(precedente, livello, smoothstep(0.0, 1.0, frazione));
  }

  // Il fondo di una fila è il fronte di quella dietro, e non la fine della
  // propria fetta di tempo. Le due cose non coincidono, ed era il difetto: alla
  // giunzione fra un piano e il successivo la fetta subito dopo l'ultima fila
  // sta ancora nell'accumulatore di quello di sopra, quindi si apriva un buco —
  // largo fino a sette decimi di fila — che si chiudeva e si riapriva a ogni
  // evento. Sei giunzioni, ognuna al suo ritmo, la più vicina a quindici volte
  // al secondo: era quello a far sembrare la scena guasta.
  //
  // Prendendo il fondo da chi sta dietro, le file **piastrellano** per
  // costruzione: nessun buco e nessuna compenetrazione, a ogni fase. Quel che
  // resta della differenza se lo prende l'ultima fila di ogni piano, che si
  // allunga e si accorcia — un elastico dove la scena è già un riassunto,
  // invece di un lampo dove si guarda.
  //
  // È la stessa espressione da tutte e due le parti, quindi i due bordi sono lo
  // stesso numero fino all'ultimo bit: si toccano, e non c'è la fessura che una
  // giunzione calcolata due volte in due modi lascerebbe.
  float zFronte = zDi(etaDi(fila));
  float zFondo = zDi(etaDi(fila + 1));

  // Un fondo visibile anche a zero: una griglia che sparisce del tutto lascia un
  // rettangolo vuoto che sembra un guasto, e la riga di base dice «c'è, e adesso
  // è a zero». Non è un token: è una garanzia, e una garanzia con una manopola
  // sopra è una garanzia che qualcuno può togliere.
  float altezza = max(livello * alta, 0.004);

  float passoX = larghezza / float(colonne);
  float x = (float(colonna) + 0.5) * passoX - larghezza * 0.5;

  vec3 locale = posizione;
  locale.x *= passoX * spessore;
  // Esatta, senza il filo di sovrapposizione che c'era prima. Quel quattro per
  // cento serviva a nascondere il buco alla giunzione — ne copriva un ventesimo,
  // e in cambio faceva compenetrare **tutte** le file vicine. Due file alla
  // stessa altezza hanno la cima sullo stesso piano, e nella fascia in cui si
  // compenetravano erano due quadrati complanari a contendersi gli stessi pixel:
  // il buffer di profondità non ha un vincitore da dichiarare, e ne usciva un
  // pulviscolo che si rimescolava a ogni fotogramma. Succedeva ovunque il suono
  // fosse fermo, cioè su tutto il fondo a riposo, dove i livelli sono lo stesso
  // byte fila dopo fila.
  locale.z *= zFronte - zFondo;
  locale.y *= altezza;
  vec3 mondo = vec3(
    x + locale.x,
    locale.y * specchio,
    (zFronte + zFondo) * 0.5 + locale.z
  );

  // La luce: una direzionale da sopra-davanti più un ambiente. Nel vertice e non
  // nel frammento perché le facce sono piatte — la normale è la stessa su tutta
  // la faccia, e interpolarla darebbe lo stesso numero pixel per pixel.
  //
  // Il complemento e non una seconda costante: così la faccia perfettamente
  // illuminata resta esattamente 1 qualunque sia l'ambiente, e alzare la luce
  // diffusa schiarisce le ombre invece di bruciare le luci.
  vec3 luce = normalize(vec3(-0.35, 0.85, 0.4));
  vec3 n = vec3(normale.x, normale.y * specchio, normale.z);
  vLuce = ambiente + (1.0 - ambiente) * max(dot(n, luce), 0.0);

  // Quanto è lontana la fila, da zero a uno — e si misura in profondità, non in
  // file: le file sono spaziate uguali per costruzione, e quel che deve
  // spegnersi è il fondo della stanza. È il modo in cui la scena si fonde con lo
  // sfondo invece di finire con un muro all'orizzonte.
  //
  // Comincia poco oltre metà stanza, e non a otto decimi come prima. Le file
  // sono spaziate uguali nel mondo, non sullo schermo: da lontananza 0,5 in poi
  // la prospettiva le schiaccia sotto i tre pixel, e oltre 0,8 sotto il pixel e
  // mezzo. Erano venti file alte un pixel, opache, che arretravano — un reticolo
  // che striscia, ed è la seconda metà di quel che stancava gli occhi.
  //
  // Non si perde la memoria: si perde la pretesa di leggerla riga per riga. A
  // 0,7 di lontananza siamo già a sette secondi fa e l'alfa è ancora due terzi, a
  // 0,9 sono venti secondi e resta una foschia. È la prospettiva aerea, cioè il
  // modo in cui il lontano si vede davvero — e dove comincia lo decide la skin
  // con canvas.viz.haze, che è la manopola da girare se la stanza sembra troppo
  // corta o troppo densa.
  float lontananza = -mondo.z / prof;
  vAlfa = 1.0 - smoothstep(foschia, 0.98, lontananza);
  // Il riflesso è un accenno, non una copia: sopra il pavimento c'è la scena,
  // sotto la sua idea. E non è un'alfa piatta: sfuma salendo, cioè allontanandosi
  // dal pavimento, che è come si comporta un riflesso vero. Con l'alfa piatta
  // sembrava una seconda scena appesa sotto la prima; così sembra un pavimento
  // bagnato. Costa una moltiplicazione nel vertice.
  //
  // Il profilo 0,30 → 0,05 è l'alfa **finale**, quella che si vede con il
  // riflesso di serie: non è un fattore da moltiplicare una seconda volta per il
  // token. Per questo si divide per 0,22, che è il valore di serie di
  // canvas.viz.reflection — lo stesso numero che sta in SERIE.riflesso qui sopra
  // e nel registro delle skin, e l'unico contro cui questo profilo abbia senso
  // di essere normalizzato. Così a 0,22 il conto torna 0,30 al pavimento e 0,05
  // in cima, il token scala tutto linearmente com'è il suo mestiere, e a zero il
  // riflesso sparisce del tutto esattamente come prima.
  //
  // Senza quella divisione i due si moltiplicavano: di serie l'alfa usciva 0,066
  // al pavimento e 0,011 in cima, contro il 0,22 piatto della 2.2.x — tre volte
  // e mezzo più pallida, cioè un riflesso che a occhio non c'è più. Erano due
  // decisioni giuste prese separatamente, «il token sostituisce la costante» e
  // «l'alfa sfuma», che composte a caso davano una terza cosa che nessuno aveva
  // scelto.
  //
  // Il taglio a uno non è una cintura di sicurezza: 0,30/0,22 fa 1,36, quindi
  // con il token al suo massimo il fattore passa l'unità per davvero. E vAlfa
  // non è un'alfa qualunque — il frammento la moltiplica per il velo e scrive il
  // colore **premoltiplicato**, dove un'alfa sopra uno vuol dire un colore che
  // vale più di se stesso, cioè una banda bruciata nella parte bassa del
  // riflesso. Si taglia qui e non di là perché qui è l'unico posto in cui vAlfa
  // può salire sopra uno: la foschia la lascia dentro [0,1] e il velo può solo
  // abbassarla, e un min nel vertice si paga una volta per vertice invece che
  // una per pixel.
  if (specchio < 0.0) {
    vAlfa = min(vAlfa * riflesso * mix(0.30, 0.05, posizione.y) / 0.22, 1.0);
  }

  vLivello = livello;
  vSu = posizione.y;
  gl_Position = camera * vec4(mondo, 1.0);
}`;

const FRAMMENTO = `#version 300 es
precision mediump float;

in float vLivello;
in float vSu;
in float vLuce;
in float vAlfa;

uniform vec3 primario;
uniform vec3 secondario;
uniform vec3 alone;
uniform float velo;
uniform float cresta;

out vec4 fuori;

void main() {
  // Il colore dice l'altezza: in basso il secondario, in cima il primario. Sono
  // due token della skin, e nient'altro nell'app li legge — l'equalizzatore ha i
  // suoi, che è quel che permette a questa scena di avere una tavolozza sua
  // senza tirarsi dietro le dieci barre di un'altra parte.
  vec3 colore = mix(secondario, primario, clamp(vLivello * 1.3, 0.0, 1.0));
  // La cima si accende con il colore della cresta: è il bagliore che una
  // sfocatura avrebbe dato, pagato zero.
  colore = mix(colore, alone, smoothstep(0.72, 1.0, vSu) * cresta);
  colore *= vLuce;

  float alfa = vAlfa * velo;
  // Niente discard. La riga che stava qui era: if (alfa <= 0.004) discard;
  //
  // Con la miscelazione spenta e il colore premoltiplicato, un frammento sotto
  // quell'alfa scrive un vec4 quasi nullo, cioè esattamente quel che il
  // compositore avrebbe messo lì senza di lui: il risultato composito è lo
  // stesso. Quel che cambia è che adesso scrive anche la profondità, e che la
  // scheda può tornare a scartare i frammenti coperti prima di eseguire questo
  // shader.
  //
  // Un discard, ovunque sia scritto, disabilita quel test per l'intera chiamata
  // di disegno: la scheda non può più sapere in anticipo se un frammento
  // sopravvivrà. Su quarantacinquemila istanze con molta sovrapposizione — la
  // scena è una griglia vista di taglio, quindi la sovrapposizione è quasi
  // tutto — è il guadagno più grosso che questo shader avesse da dare.
  //
  // È un cambio di comportamento vero e non una pulizia, ed è per questo che è
  // arrivato come un passo separato: quei frammenti adesso occupano il buffer
  // di profondità, quindi nella fascia più lontana — dove l'alfa sta sotto lo
  // zero virgola quattro per cento — il primo nasconde quelli dietro invece di
  // lasciarli trasparire. Sono tutti sotto l'un per cento di opacità, cioè
  // sotto un livello del canale a otto bit.
  // Premoltiplicato: la tela è trasparente e il compositore del browser la posa
  // sopra la tinta della copertina. Scrivere il colore pieno con un'alfa bassa
  // darebbe un alone chiaro attorno a ogni barra.
  fuori = vec4(colore * alfa, alfa);
}`;

/** Compila uno shader, o restituisce `null` dicendo alla console perché no. */
function compila(gl: WebGL2RenderingContext, tipo: number, sorgente: string) {
  const shader = gl.createShader(tipo);
  if (!shader) return null;
  gl.shaderSource(shader, sorgente);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    console.error("spettro 3D:", gl.getShaderInfoLog(shader));
    gl.deleteShader(shader);
    return null;
  }
  return shader;
}

/**
 * La scatola unitaria: x e z in `[-0.5, 0.5]`, y in `[0, 1]`.
 *
 * Quattro facce e non sei. Quella di sotto poggia sul pavimento e non si vede né
 * dalla scena né dal riflesso. Quella di dietro nemmeno: la camera sta sempre a
 * `z > 0` e le file a `z ≤ 0`, quindi è già buttata dal culling — e trentamila
 * quadrati che il culling butta restano trentamila quadrati da trasformare.
 */
function scatola(): { vertici: Float32Array; indici: Uint16Array } {
  // Ogni faccia: la sua normale e i quattro angoli, in senso antiorario visti da
  // fuori — è il verso che dice alla scheda quale lato buttare via.
  const facce: [number[], number[]][] = [
    [
      [0, 0, 1],
      [-0.5, 0, 0.5, 0.5, 0, 0.5, 0.5, 1, 0.5, -0.5, 1, 0.5],
    ],
    [
      [1, 0, 0],
      [0.5, 0, 0.5, 0.5, 0, -0.5, 0.5, 1, -0.5, 0.5, 1, 0.5],
    ],
    [
      [-1, 0, 0],
      [-0.5, 0, -0.5, -0.5, 0, 0.5, -0.5, 1, 0.5, -0.5, 1, -0.5],
    ],
    [
      [0, 1, 0],
      [-0.5, 1, 0.5, 0.5, 1, 0.5, 0.5, 1, -0.5, -0.5, 1, -0.5],
    ],
  ];
  const vertici: number[] = [];
  const indici: number[] = [];
  facce.forEach(([normale, punti], faccia) => {
    for (let i = 0; i < 4; i += 1) {
      vertici.push(
        punti[i * 3] ?? 0,
        punti[i * 3 + 1] ?? 0,
        punti[i * 3 + 2] ?? 0,
        normale[0] ?? 0,
        normale[1] ?? 0,
        normale[2] ?? 0,
      );
    }
    const base = faccia * 4;
    indici.push(base, base + 1, base + 2, base, base + 2, base + 3);
  });
  return {
    vertici: new Float32Array(vertici),
    indici: new Uint16Array(indici),
  };
}

/**
 * Proiezione per vista, già moltiplicate, dentro una matrice prestata.
 *
 * Una matrice sola e non due: è l'unica cosa che lo shader deve sapere della
 * camera, e farne il prodotto qui costa sedici moltiplicazioni al fotogramma
 * invece di trentamila.
 *
 * Prestata e non restituita: allocare una `Float32Array(16)` sessanta volte al
 * secondo sono sessanta occasioni al secondo perché il raccoglitore fermi il filo
 * che disegna. Per la stessa ragione l'occhio e la mira arrivano come sei
 * scalari invece che come due vettori, e il verso della vista non è più un array
 * di tre elementi ma tre variabili.
 *
 * `lente` è l'apertura in gradi, cioè `canvas.viz.lens`: era 52 scritto dentro
 * la formula.
 */
function camera(
  fuori: Float32Array,
  aspetto: number,
  lente: number,
  ox: number,
  oy: number,
  oz: number,
  mx: number,
  my: number,
  mz: number,
): void {
  const vicino = 0.1;
  const lontano = 24;
  const f = 1 / Math.tan((lente * Math.PI) / 180 / 2);

  // La vista, cioè una base ortonormale attorno alla direzione dello sguardo.
  const vx = ox - mx;
  const vy = oy - my;
  const vz = oz - mz;
  const lunghezza = Math.hypot(vx, vy, vz) || 1;
  const zx = vx / lunghezza;
  const zy = vy / lunghezza;
  const zz = vz / lunghezza;
  // L'asse x è l'alto del mondo per lo z della vista, cioè `(0,1,0) × z`. Sta
  // scritto per esteso perché due delle tre componenti sono zero e un prodotto
  // vettoriale generico le moltiplicherebbe lo stesso.
  const orizzontale = Math.hypot(zz, zx) || 1;
  const xx = zz / orizzontale;
  const xy = 0;
  const xz = -zx / orizzontale;
  const yx = zy * xz - zz * xy;
  const yy = zz * xx - zx * xz;
  const yz = zx * xy - zy * xx;

  const tx = -(xx * ox + xy * oy + xz * oz);
  const ty = -(yx * ox + yy * oy + yz * oz);
  const tz = -(zx * ox + zy * oy + zz * oz);

  // Il prodotto proiezione × vista, scritto per colonne come lo vuole WebGL. La
  // proiezione ha solo quattro valori diversi da zero, e una moltiplicazione
  // generica fra matrici farebbe sessantaquattro prodotti per ottenere questi
  // sedici numeri.
  const a = f / aspetto;
  const c = (lontano + vicino) / (vicino - lontano);
  const d = (2 * lontano * vicino) / (vicino - lontano);
  fuori[0] = a * xx;
  fuori[1] = f * yx;
  fuori[2] = c * zx;
  fuori[3] = -zx;
  fuori[4] = a * xy;
  fuori[5] = f * yy;
  fuori[6] = c * zy;
  fuori[7] = -zy;
  fuori[8] = a * xz;
  fuori[9] = f * yz;
  fuori[10] = c * zz;
  fuori[11] = -zz;
  fuori[12] = a * tx;
  fuori[13] = f * ty;
  fuori[14] = c * tz + d;
  fuori[15] = -tz;
}

/** Dove sta ogni uniform, cercata una volta sola per programma. */
interface Uniformi {
  camera: WebGLUniformLocation | null;
  cursori: WebGLUniformLocation | null;
  fase: WebGLUniformLocation | null;
  frazione: WebGLUniformLocation | null;
  specchio: WebGLUniformLocation | null;
  colonne: WebGLUniformLocation | null;
  perPiano: WebGLUniformLocation | null;
  piani: WebGLUniformLocation | null;
  prof: WebGLUniformLocation | null;
  larghezza: WebGLUniformLocation | null;
  spessore: WebGLUniformLocation | null;
  alta: WebGLUniformLocation | null;
  foschia: WebGLUniformLocation | null;
  ambiente: WebGLUniformLocation | null;
  riflesso: WebGLUniformLocation | null;
  primario: WebGLUniformLocation | null;
  secondario: WebGLUniformLocation | null;
  alone: WebGLUniformLocation | null;
  velo: WebGLUniformLocation | null;
  cresta: WebGLUniformLocation | null;
}

/**
 * Quel che non dipende da quante barre ci sono.
 *
 * Il contesto, il programma con i suoi due shader, la geometria della scatola, e
 * la macchina che legge i token. Nasce al montaggio, muore allo smontaggio, e
 * l'unica altra cosa che la rifà è un contesto perso e ritrovato.
 *
 * Sta separata dal resto perché prima non lo era: un cambio di risoluzione
 * rifaceva tutto, cioè ricompilava due shader e rilinkava un programma per
 * cambiare la larghezza di una texture. E soprattutto perché senza questa
 * separazione non esisteva un modo di ricostruire il contesto senza rimontare il
 * componente.
 */
interface RisorseStabili {
  gl: WebGL2RenderingContext;
  programma: WebGLProgram;
  vertice: WebGLShader;
  frammento: WebGLShader;
  vao: WebGLVertexArrayObject;
  buffer: WebGLBuffer;
  elementi: WebGLBuffer;
  /** Quanti indici ha la scatola: è il conto della `drawElementsInstanced`. */
  indici: number;
  uniformi: Uniformi;
  /** Il pennello di un pixel con cui si leggono i colori: vedi `tinta`. */
  pennello: CanvasRenderingContext2D | null;
  /**
   * Lo stile della tela, **vivo**.
   *
   * `getComputedStyle` torna una dichiarazione legata all'elemento, non una
   * fotografia: rileggerla basta perché una skin nuova arrivi qui senza
   * rimontare niente. È quel che rende possibile lo Studio.
   */
  stile: CSSStyleDeclaration;
  /** La matrice della camera, riscritta e mai riallocata. */
  matrice: Float32Array;
  /** Tre numeri per la pipetta, riscritti e mai riallocati. */
  colore: Float32Array;
  /** L'ultimo valore grezzo di ogni token, nell'ordine di `TOKEN`. */
  grezzi: string[];
  manopole: Manopole;
  /** C'è da rileggere i token. */
  sporco: boolean;
}

/**
 * Quel che dipende da quante barre ci sono.
 *
 * La texture, gli anelli della cascata, i cursori e la fase. La texture si rifà
 * perché `texStorage2D` è **storage immutabile**: la sua larghezza è decisa alla
 * nascita e non si cambia. Tutto il resto — programma, shader, VAO, i due
 * buffer — sopravvive, perché di `barre` non sa niente.
 *
 * Gli anelli stanno in memoria normale e non nella texture, e non è una copia
 * per comodità: è l'unico modo di sapere **cosa cade dal fondo** di un piano
 * senza rileggere la texture dalla scheda. Il fatto che sopravvivano a un
 * contesto perso è un regalo di quella scelta.
 *
 * L'altra cosa che la fa rifare è il passaggio da o verso la qualità «bassa»,
 * che cambia quante file ha ogni piano — ed è l'unica cosa che un livello di
 * qualità ricostruisce, contro la densità — che è un ridimensionamento della
 * tela — e il riflesso, che è un `if` nel ciclo. Per questo `perPiano` sta qui
 * dentro: senza, un cambio di tetto a
 * numero di barre invariato riuserebbe gli anelli della forma sbagliata.
 */
interface RisorseRisoluzione {
  barre: number;
  perPiano: number;
  piani: number;
  file: number;
  /** Quanti eventi di silenzio prima che la scena sia vuota davvero. */
  copertura: number;
  texture: WebGLTexture | null;
  anelli: Uint8Array[];
  parziali: Uint8Array[];
  cursori: Int32Array;
  conta: Int32Array;
  /** Gli eventi da quando i piani erano tutti allineati. */
  fase: number;
  istanzeRiflesso: number;
}

/**
 * Quanti eventi impiega la gobba a passare su tutte le bande.
 *
 * Novantasei, cioè tre secondi e due decimi: abbastanza lenta perché si legga
 * come una spazzata e non come uno sfarfallio, abbastanza svelta perché chi
 * trascina un cursore nello Studio veda tutte le bande arrivare a fondo scala
 * senza aspettare.
 */
const SPAZZATA = 96;

/** Quanto è larga la gobba, in frazione di spettro. */
const LARGO = 0.22;

/**
 * Ogni quanti eventi batte la grancassa.
 *
 * Ventiquattro, cioè quattro colpi per giro di spazzata e 792 millesimi l'uno:
 * settantasei battiti al minuto. Che sia un divisore di `SPAZZATA` non è
 * civetteria — significa che l'immagine intera si ripete ogni 96 eventi, e non
 * ogni minimo comune multiplo di due periodi primi fra loro.
 */
const GRANCASSA = 24;

/** Con che costante di tempo si spegne un colpo, in eventi. */
const TAU_COLPO = 3.5;

/**
 * Quanto si aspetta, con il movimento ridotto, prima di posare la scena.
 *
 * Un secondo e mezzo. Con `prefers-reduced-motion` la sorgente sintetica spinge
 * **una** fila e si ferma, e il ciclo di disegno da solo non lo saprebbe mai: il
 * silenzio si conta in eventi, e da una sorgente che ha smesso di parlare non ne
 * arriva più nessuno a contarlo. Senza questo, lo Studio ridisegnerebbe sessanta
 * volte al secondo, per sempre, un'immagine che non cambia — cioè proprio il
 * consumo che le tre condizioni di arresto esistono per non pagare, addosso
 * all'unica persona che ha chiesto al sistema di non far muovere le cose.
 *
 * Un secondo e mezzo, e non meno, perché la scena entra con una dissolvenza
 * lunga `--dur-3` — 450 millesimi di serie — e fermarla prima lascerebbe un
 * quadro fermo a metà opacità. Una skin che portasse quella durata oltre il
 * secondo e mezzo avrebbe esattamente quel difetto: è il prezzo di un numero
 * scritto qui invece che letto da una manopola che a questo punto della vita
 * della tela non è ancora stata letta.
 */
const FERMO_MS = 1500;

/** Quanto alza le bande basse. */
const ALTEZZA_COLPO = 0.35;

/**
 * Un intero mescolato, fra zero e uno.
 *
 * Il mescolatore a due giri di `imul` che si trova ovunque sotto il nome di
 * hash32shift: nessuno stato, nessuna allocazione, e — la sola cosa che qui
 * conta davvero — **nessun `Math.random`**. La grana della scena finta dev'essere
 * la stessa su due macchine, o una schermata smette di essere un documento su
 * cui si può discutere.
 */
function mescola(x: number): number {
  let h = x | 0;
  h = Math.imul(h ^ (h >>> 16), 0x45d9f3b);
  h = Math.imul(h ^ (h >>> 16), 0x45d9f3b);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/**
 * Una fila di bande finte, in forma chiusa sul numero dell'evento.
 *
 * # A cosa serve
 *
 * A far vedere la scena dove non c'è audio — nello Studio, mentre si dipinge la
 * skin. Non è una simulazione di musica: è il **banco di prova dei diciotto
 * token**, e ogni suo pezzo esiste perché senza quel pezzo uno dei diciotto non
 * si vedrebbe muovere.
 *
 * # I quattro pezzi
 *
 * La **pendenza** `(1−f)^1.6` è quel che una musica vera fa allo spettro: molta
 * energia in basso, poca in alto. Serve a `canvas.viz.primary`, `secondary` e
 * `tip`, che dipingono in funzione dell'altezza: con una fila piatta le tre
 * tinte si vedrebbero tutte allo stesso livello e nessuno saprebbe dire qual è
 * la base e qual è la cresta.
 *
 * La **spazzata** è una gobba di larghezza `LARGO` il cui centro va e viene su
 * tutto lo spettro in `SPAZZATA` eventi, con una `smoothstep` a smussarne i
 * bordi. Porta **ogni** banda a fondo scala almeno una volta per giro, che è la
 * condizione perché `canvas.viz.height` e `canvas.viz.floor` si vedano davvero:
 * un fondo scala si giudica su una barra che ci arriva.
 *
 * La **grancassa** vive nello stesso ottavo basso che la camera legge
 * (`length >> 3`), e non altrove: è l'unico modo perché `canvas.viz.beat` e
 * `canvas.viz.eye` facciano qualcosa di visibile. Scende con un esponenziale in
 * `TAU_COLPO` eventi, quindi il colpo arriva e se ne va come farebbe.
 *
 * La **grana** è un hash intero di banda e tempo lento, che moltiplica per un
 * fattore fra 0,80 e 1. Senza, le file vicine avrebbero la stessa identica
 * altezza e `canvas.viz.fill` si leggerebbe come un nastro invece che come
 * barre — cioè la manopola che decide quanto una barra riempie la sua fetta
 * sarebbe l'unica delle diciotto a non mostrarsi.
 *
 * # Perché in forma chiusa su `n`, e senza allocare
 *
 * Perché è funzione del solo numero dell'evento: due autori su due macchine, a
 * qualunque velocità arrivi il loro `setInterval`, alla fila numero 37 vedono la
 * stessa fila. È ciò che rende una schermata un documento di revisione invece di
 * un aneddoto. E scrive in un vettore prestato — trenta `Uint8Array` al secondo
 * sarebbero trenta occasioni al secondo perché il raccoglitore fermi il filo che
 * disegna, che è precisamente il difetto che il resto di questo file ha tolto.
 *
 * L'uscita è nello stesso formato del filo: un byte per banda, `0..255`, dove il
 * byte è la potenza fra `FONDO_DB` e il fondo scala. Va quindi nello stesso
 * gestore delle bande vere, e da lì nello stesso smorzamento, nella stessa
 * cascata, nella stessa texture, nello stesso seguipulsazioni.
 */
function filaFinta(fuori: Uint8Array, n: number): void {
  const larghezza = fuori.length;
  if (larghezza === 0) return;
  // Il centro della gobba: un coseno rialzato, che si ferma un attimo agli
  // estremi invece di rimbalzarci contro. Le bande di bordo restano a fondo
  // scala qualche evento in più, ed è giusto — sono quelle che si guardano per
  // ultime.
  const giro = (n % SPAZZATA) / SPAZZATA;
  const cresta = 0.5 - 0.5 * Math.cos(giro * 2 * Math.PI);
  const colpo = ALTEZZA_COLPO * Math.exp(-(n % GRANCASSA) / TAU_COLPO);
  for (let i = 0; i < larghezza; i += 1) {
    // Il centro della banda e non il suo bordo: con `i / larghezza` la prima
    // banda starebbe esattamente a zero e l'ultima non arriverebbe mai a uno.
    const f = (i + 0.5) / larghezza;
    const d = Math.abs(f - cresta);
    const s = d < LARGO ? 1 - d / LARGO : 0;
    const gobba = s * s * (3 - 2 * s);
    const fondo = 0.1 + 0.55 * Math.pow(1 - f, 1.6);
    const grancassa = f < 0.125 ? colpo * (1 - f * 8) : 0;
    // Il tempo della grana scorre a un ottavo della cadenza, cioè cambia ogni
    // 264 millesimi: più veloce sarebbe pulviscolo, più lento sarebbe un pettine
    // dipinto. I due moltiplicatori sono dispari e diversi fra loro — il primo è
    // il moltiplicatore di Knuth, 2^32 diviso la sezione aurea — così banda e
    // tempo lento non si ripiegano uno sull'altro prima che il mescolatore
    // faccia il suo lavoro.
    const grana =
      0.8 + 0.2 * mescola(Math.imul(i, 2654435761) + Math.imul(n >> 3, 40503));
    // Il ,25 di troppo sulla gobba è deliberato: la grana al minimo toglie un
    // quinto, e senza quel margine una banda con la grana bassa resterebbe a tre
    // quarti di scala anche al passaggio della gobba — cioè il token che il
    // fondo scala governa non si vedrebbe mai lavorare. Con il margine, ogni
    // banda arriva almeno a 234 su 255 nel giro di una spazzata, e la maggior
    // parte tocca il tetto.
    const v = (fondo + (1 - fondo) * gobba * 1.25 + grancassa) * grana;
    fuori[i] = Math.round(Math.min(1, Math.max(0, v)) * 255);
  }
}

/** Apre il contesto e costruisce tutto quel che non dipende dalle barre. */
function creaStabili(canvas: HTMLCanvasElement): RisorseStabili | null {
  const gl = canvas.getContext("webgl2", {
    alpha: true,
    antialias: true,
    depth: true,
    // Su un portatile con due schede, questa scena non è il motivo per
    // svegliare quella grossa.
    powerPreference: "low-power",
    premultipliedAlpha: true,
  });
  if (!gl) return null;

  const programma = gl.createProgram();
  const vertice = compila(gl, gl.VERTEX_SHADER, VERTICE);
  const frammento = compila(gl, gl.FRAGMENT_SHADER, FRAMMENTO);
  if (!programma || !vertice || !frammento) return null;
  gl.attachShader(programma, vertice);
  gl.attachShader(programma, frammento);
  gl.linkProgram(programma);
  if (!gl.getProgramParameter(programma, gl.LINK_STATUS)) {
    console.error("spettro 3D:", gl.getProgramInfoLog(programma));
    return null;
  }
  gl.useProgram(programma);

  const { vertici, indici } = scatola();
  const vao = gl.createVertexArray();
  const buffer = gl.createBuffer();
  const elementi = gl.createBuffer();
  if (!vao || !buffer || !elementi) return null;
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, vertici, gl.STATIC_DRAW);
  const posizione = gl.getAttribLocation(programma, "posizione");
  const normale = gl.getAttribLocation(programma, "normale");
  gl.enableVertexAttribArray(posizione);
  gl.vertexAttribPointer(posizione, 3, gl.FLOAT, false, 24, 0);
  gl.enableVertexAttribArray(normale);
  gl.vertexAttribPointer(normale, 3, gl.FLOAT, false, 24, 12);
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, elementi);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indici, gl.STATIC_DRAW);

  const dove = (nome: string) => gl.getUniformLocation(programma, nome);
  const uniformi: Uniformi = {
    camera: dove("camera"),
    cursori: dove("cursori"),
    fase: dove("fase"),
    frazione: dove("frazione"),
    specchio: dove("specchio"),
    colonne: dove("colonne"),
    perPiano: dove("perPiano"),
    piani: dove("piani"),
    prof: dove("prof"),
    larghezza: dove("larghezza"),
    spessore: dove("spessore"),
    alta: dove("alta"),
    foschia: dove("foschia"),
    ambiente: dove("ambiente"),
    riflesso: dove("riflesso"),
    primario: dove("primario"),
    secondario: dove("secondario"),
    alone: dove("alone"),
    velo: dove("velo"),
    cresta: dove("cresta"),
  };
  // L'unica uniform davvero una-tantum rimasta: la texture sta nell'unità zero,
  // e non c'è una seconda unità. Tutte le altre o dipendono dalla risoluzione,
  // o sono un token, e scriverle una volta sola era il difetto per cui una skin
  // non poteva cambiare la geometria della stanza.
  gl.uniform1i(dove("altezze"), 0);

  // Nessuna miscelazione, e non è una dimenticanza: la profondità basta a dire
  // chi sta davanti, e le barre non sono trasparenti fra loro. L'alfa che esce
  // dal frammento serve al **compositore del browser**, che posa la tela sopra
  // la tinta della copertina. Accendere la miscelazione avrebbe voluto dire
  // ordinare trentamila scatole dal fondo a ogni fotogramma, che è precisamente
  // il lavoro che una scheda video fa da sola con il buffer di profondità.
  gl.enable(gl.DEPTH_TEST);
  gl.enable(gl.CULL_FACE);
  gl.frontFace(gl.CCW);
  gl.clearColor(0, 0, 0, 0);

  const pipetta = document.createElement("canvas");
  pipetta.width = 1;
  pipetta.height = 1;

  return {
    gl,
    programma,
    vertice,
    frammento,
    vao,
    buffer,
    elementi,
    indici: indici.length,
    uniformi,
    pennello: pipetta.getContext("2d", { willReadFrequently: true }),
    stile: getComputedStyle(canvas),
    matrice: new Float32Array(16),
    colore: new Float32Array(3),
    grezzi: TOKEN.map(() => MAI),
    manopole: nuoveManopole(),
    sporco: true,
  };
}

/** Butta via il contesto e tutto quel che ci sta dentro. */
function distruggiStabili(s: RisorseStabili): void {
  const { gl } = s;
  gl.deleteBuffer(s.buffer);
  gl.deleteBuffer(s.elementi);
  gl.deleteVertexArray(s.vao);
  gl.deleteProgram(s.programma);
  gl.deleteShader(s.vertice);
  gl.deleteShader(s.frammento);
}

/**
 * La texture e la cascata, per un dato numero di barre.
 *
 * Con `vecchia` alla stessa risoluzione la memoria si **riprende**: gli anelli,
 * i cursori, i contatori e la fase restano quelli, e la texture nuova si
 * ricarica da lì. È la strada del contesto ritrovato, ed è il motivo per cui un
 * riavvio del driver non cancella mezzo minuto di musica.
 */
function creaRisoluzione(
  s: RisorseStabili,
  barre: number,
  tetto: number,
  vecchia: RisorseRisoluzione | null,
): RisorseRisoluzione | null {
  const { gl, uniformi: u } = s;
  const { perPiano, piani, file, copertura } = struttura(barre, tetto);

  // La forma degli anelli è `barre · perPiano` per piano: bastasse il numero di
  // barre, un passaggio a «bassa» riuserebbe anelli lunghi il doppio del giusto
  // e la cascata leggerebbe file che non esistono.
  const riusa =
    vecchia !== null && vecchia.barre === barre && vecchia.perPiano === perPiano;
  const anelli: Uint8Array[] = riusa && vecchia ? vecchia.anelli : [];
  const parziali: Uint8Array[] = riusa && vecchia ? vecchia.parziali : [];
  if (!riusa) {
    // Per piano: una copia dell'anello, un accumulatore e un contatore. Sono
    // `barre · perPiano · piani` byte, cinquanta chilobyte nel caso peggiore.
    for (let p = 0; p < piani; p += 1) {
      anelli.push(new Uint8Array(barre * perPiano));
      parziali.push(new Uint8Array(barre));
    }
  }

  // Un byte per banda, com'è arrivato dal filo: fra l'evento e la texture non
  // c'è nessuna conversione. Il piano ℓ occupa le righe da ℓ·perPiano, quindi
  // la cascata intera sta nella stessa texture che servirebbe a una fila per
  // evento — la memoria non cresce con i secondi, cresce con le file disegnate.
  const texture = gl.createTexture();
  if (!texture) return null;
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R8, barre, file);
  // Un caricamento per piano, dall'anello che gli corrisponde: la riga di un
  // piano nella texture è `piano · perPiano + cursore`, e nell'anello è
  // `cursore · barre`, quindi i due sono lo stesso vettore nello stesso ordine.
  // Su una texture appena nata gli anelli sono zeri, e questo è l'azzeramento;
  // su una ritrovata sono mezzo minuto di musica, e questo è il ripristino.
  for (let p = 0; p < piani; p += 1) {
    const anello = anelli[p];
    if (!anello) continue;
    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      0,
      p * perPiano,
      barre,
      perPiano,
      gl.RED,
      gl.UNSIGNED_BYTE,
      anello,
    );
  }
  // `NEAREST` perché il vertice legge con `texelFetch`: nessun filtro entra in
  // gioco, e chiederne uno lineare vorrebbe dire un'estensione in più da
  // sperare che ci sia.
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

  gl.uniform1i(u.colonne, barre);
  gl.uniform1i(u.perPiano, perPiano);
  gl.uniform1i(u.piani, piani);

  return {
    barre,
    perPiano,
    piani,
    file,
    copertura,
    texture,
    anelli,
    parziali,
    cursori: riusa && vecchia ? vecchia.cursori : new Int32Array(PIANI_MAX),
    conta: riusa && vecchia ? vecchia.conta : new Int32Array(PIANI_MAX),
    fase: riusa && vecchia ? vecchia.fase : 0,
    // Il riflesso solo per i primi piani: più in fondo, a un'alfa che è una
    // frazione di quella della scena, non si vede. Basta un `instanceCount` più
    // piccolo, perché `fila = id / colonne` mette già le file vicine per prime.
    istanzeRiflesso: barre * Math.min(file, perPiano * PIANI_RIFLESSI),
  };
}

/** Butta la texture. Gli anelli no: quelli sono la memoria. */
function distruggiRisoluzione(
  s: RisorseStabili | null,
  r: RisorseRisoluzione | null,
): void {
  if (!s || !r || !r.texture) return;
  s.gl.deleteTexture(r.texture);
  r.texture = null;
}

/**
 * Mette una fila in cima a un piano, e manda su quella che cade dal fondo.
 *
 * Della texture si scrive **una riga sola**: quella nuova. Le altre restano
 * dov'erano, e la fila che avanza è il cursore che cambia.
 *
 * Funzioni di modulo e non chiusure: da quando il contesto arriva come
 * parametro, non c'è più niente da restringere e la ricorsione mutua con
 * `accumula` si scrive nel verso naturale. Con `s` a `null` — contesto perso — la
 * cascata continua lo stesso e salta solo il caricamento: è così che mezzo minuto
 * di passato attraversa un riavvio del driver.
 */
function spingi(
  s: RisorseStabili | null,
  r: RisorseRisoluzione,
  piano: number,
  fila: Uint8Array,
): void {
  const anello = r.anelli[piano];
  if (!anello) return;
  const cursore = ((r.cursori[piano] ?? 0) + 1) % r.perPiano;
  const base = cursore * r.barre;
  // Prima di sovrascriverlo: quel che sta in questo posto è la fila che esce
  // dalla finestra del piano, e il piano di sopra la sta aspettando. Dopo
  // l'ultimo piano invece si perde, ed è giusto — è passato più vecchio
  // della memoria.
  if (piano + 1 < r.piani) {
    accumula(s, r, piano + 1, anello.subarray(base, base + r.barre));
  }
  anello.set(fila, base);
  r.cursori[piano] = cursore;
  if (!s || !r.texture) return;
  s.gl.texSubImage2D(
    s.gl.TEXTURE_2D,
    0,
    0,
    piano * r.perPiano + cursore,
    r.barre,
    1,
    s.gl.RED,
    s.gl.UNSIGNED_BYTE,
    fila,
  );
}

/**
 * Il massimo di due file del piano di sotto diventa una fila di questo.
 *
 * Il massimo e non la media: due file riassunte devono dire «qui c'è stato un
 * colpo», e la media di un colpo e di un silenzio è un mezzo colpo che non è mai
 * suonato.
 */
function accumula(
  s: RisorseStabili | null,
  r: RisorseRisoluzione,
  piano: number,
  fila: Uint8Array,
): void {
  const parziale = r.parziali[piano];
  if (!parziale) return;
  for (let i = 0; i < r.barre; i += 1) {
    const valore = fila[i] ?? 0;
    if (valore > (parziale[i] ?? 0)) parziale[i] = valore;
  }
  r.conta[piano] = (r.conta[piano] ?? 0) + 1;
  if ((r.conta[piano] ?? 0) >= 2) {
    spingi(s, r, piano, parziale);
    parziale.fill(0);
    r.conta[piano] = 0;
  }
}

/** Manda un token appena cambiato dove deve andare. */
function applica(
  s: RisorseStabili,
  voce: Voce,
  grezzo: string,
  barre: number,
): void {
  const { gl, uniformi: u, manopole: m } = s;
  if (voce.colore) {
    if (!s.pennello) return;
    tinta(s.pennello, grezzo || voce.ripiego, s.colore);
    if (voce.css === "--viz-primary") gl.uniform3fv(u.primario, s.colore);
    else if (voce.css === "--viz-secondary") gl.uniform3fv(u.secondario, s.colore);
    else gl.uniform3fv(u.alone, s.colore);
    return;
  }
  // `parseFloat` e non `Number`: `--dur-3` è «450ms», e un'unità è quel che una
  // durata CSS ha il diritto di avere. Un valore illeggibile vale la base, che è
  // la stessa regola con cui `read_json` tratta una preferenza malformata.
  const letto = Number.parseFloat(grezzo);
  const v = Number.isFinite(letto)
    ? Math.min(voce.max, Math.max(voce.min, letto))
    : voce.base;
  switch (voce.css) {
    case "--viz-width":
      gl.uniform1f(u.larghezza, v);
      break;
    case "--viz-depth":
      gl.uniform1f(u.prof, v);
      break;
    case "--viz-height":
      gl.uniform1f(u.alta, v);
      break;
    case "--viz-fill":
      // Più barre, più piene: sotto le 256 la fessura fra una e l'altra è quel
      // che le rende barre, sopra sarebbe più larga della barra stessa. Il
      // token dice quanto una barra riempie la sua fetta, e questa correzione
      // resta perché è una regola di leggibilità, non un gusto — ma adesso è
      // scritta come una correzione dichiarata invece che come due numeri in un
      // ternario.
      gl.uniform1f(u.spessore, barre > 256 ? Math.min(1, v + 0.18) : v);
      break;
    case "--viz-haze":
      gl.uniform1f(u.foschia, v);
      break;
    case "--viz-ambient":
      gl.uniform1f(u.ambiente, v);
      break;
    case "--viz-glow":
      // Il token è una percentuale: quanta cresta si vede sulla cima. Non è
      // confrontabile con lo 0,55 che stava qui prima, perché quello mescolava
      // l'accento con se stesso — era il 55 % di niente. Adesso la cresta è un
      // colore suo, e un quinto di un colore diverso si vede più di metà dello
      // stesso.
      gl.uniform1f(u.cresta, v / 100);
      break;
    case "--viz-reflection":
      gl.uniform1f(u.riflesso, v);
      m.riflesso = v;
      break;
    case "--viz-lens":
      m.lente = v;
      break;
    case "--viz-eye":
      m.occhio = v;
      break;
    case "--viz-sway":
      m.oscillazione = v;
      break;
    case "--viz-beat":
      m.pulsazione = v;
      break;
    case "--viz-attack":
      m.salita = v;
      break;
    case "--viz-release":
      m.discesa = v;
      break;
    case "--viz-floor":
      m.fondo = v;
      break;
    case "--dur-3":
      m.dissolvenza = v;
      break;
    case "--motion-scale":
      m.moto = v;
      break;
    default:
      break;
  }
}

/**
 * Rilegge i token che sono cambiati, e solo quelli.
 *
 * Venti `getPropertyValue` quando il flag è alto, zero quando non lo è; e di
 * quei venti, solo quelli il cui testo è cambiato pagano una conversione — una
 * pipetta per un colore, un `parseFloat` e una uniform per un numero.
 */
function rileggi(s: RisorseStabili, barre: number): void {
  for (let i = 0; i < TOKEN.length; i += 1) {
    const voce = TOKEN[i];
    if (!voce) continue;
    const grezzo = s.stile.getPropertyValue(voce.css).trim();
    if (grezzo === s.grezzi[i]) continue;
    s.grezzi[i] = grezzo;
    applica(s, voce, grezzo, barre);
  }
}

/** In che stato è la scena. */
type Stato = "vivo" | "assopito" | "nascosto" | "perso";

export function Spettro3D({
  barre,
  inPausa,
  qualita = "auto",
  sorgente = "motore",
  onErrore,
}: {
  /** Quante barre in larghezza, cioè quante bande fini chiede il nucleo. */
  barre: number;
  /**
   * La riproduzione è ferma.
   *
   * Serve a una cosa sola: decidere se, quando la scena si assopisce, si può
   * chiudere anche il rubinetto. Con la musica che suona non si può — il
   * risveglio arriva dagli eventi dello spettro, e chiudendoli non tornerebbe
   * più nessuno a svegliarla.
   */
  inPausa: boolean;
  /**
   * Quanto questa macchina è disposta a spendere per la scena.
   *
   * Di serie `"auto"`, che è anche il valore di serie della preferenza: chi
   * monta questa tela senza dire niente ottiene quel che ottiene chi non ha mai
   * scelto. Serve a chi la monta per guardare la skin e non per ascoltare — lo
   * Studio — dove il livello di qualità di **questo** computer non c'entra
   * niente con quel che si sta dipingendo.
   */
  qualita?: QualitaSpettro | undefined;
  /**
   * Da dove arrivano le file.
   *
   * `"motore"` sono le bande vere, e la presa nel motore si apre. `"finto"` è un
   * generatore deterministico che scrive nello stesso posto: la presa **non** si
   * apre, l'ascolto dell'evento non si registra, e tutto quel che viene dopo —
   * smorzamento, cascata, texture, seguipulsazioni, lettura dei token — è la
   * stessa identica strada. È il modo in cui lo Studio mostra una scena viva
   * dove non c'è nessun audio, senza che esista una seconda scena da tenere
   * allineata a questa.
   */
  sorgente?: "motore" | "finto" | undefined;
  onErrore: (e: unknown) => void;
}) {
  const tela = useRef<HTMLCanvasElement>(null);
  const stabili = useRef<RisorseStabili | null>(null);
  const risoluzione = useRef<RisorseRisoluzione | null>(null);
  const stato = useRef<Stato>("vivo");
  /** Il risveglio, che lo monta l'effetto di montaggio: vedi `sveglia`. */
  const sveglia = useRef<() => void>(() => {});
  /** L'ultima fila, in byte, pronta per la texture. */
  const arrivo = useRef<Uint8Array>(new Uint8Array(0));
  /** Gli stessi livelli in virgola mobile: è qui che vive lo smorzamento. */
  const livelli = useRef<Float32Array>(new Float32Array(0));
  /** Quando è arrivata l'ultima fila, per far scorrere le file fra un evento e l'altro. */
  const quando = useRef(0);
  /**
   * Quanto passa fra un evento e l'altro, misurato.
   *
   * Non `PASSO_MS`, che è quanto il filo di là **dorme**: il periodo vero è quel
   * sonno più il lavoro, quindi dividere per la costante fa saturare la frazione
   * un pelo prima di ogni fila e poi ripartire da zero — un micro-tremolio a
   * trenta hertz che si vede e non si sa dire.
   *
   * Adesso serve a una seconda cosa, ed è quella che rende corretto lo
   * smorzamento delle barre: il coefficiente si ricava da questo intervallo, non
   * da una costante. Un filtro con `α = 1 − exp(−Δt/τ)` è indipendente dalla
   * cadenza **per costruzione** — se un giorno il nucleo mandasse venti eventi
   * al secondo invece di trenta, le barre scenderebbero nello stesso tempo.
   */
  const intervallo = useRef(PASSO_MS);
  /** Da quanti eventi di fila non arriva niente che non sia silenzio. */
  const silenzio = useRef(0);
  /**
   * Il colpo: quanta energia c'è nelle bande basse.
   *
   * È l'unica cosa che muove la camera, ed è la ragione per cui lo sfondo sembra
   * andare a tempo — perché ci va davvero. Si calcola **per evento** e non per
   * fotogramma: è un dato del suono, e un dato del suono non deve dipendere da
   * quanti fotogrammi al secondo fa lo schermo di chi guarda.
   */
  const colpo = useRef(0);
  /** La riproduzione è ferma, dove lo legge la macchina del riposo. */
  const fermo = useRef(inPausa);
  /** Il rubinetto è aperto. Un solo proprietario: `rubinetto`. */
  const aperto = useRef(false);
  /**
   * WebGL 2 c'è, e la scena esiste.
   *
   * È la condizione di ogni apertura del rubinetto, e non basta guardare
   * `stabili`: quello va a `null` mentre il contesto è perso, e in quel mezzo
   * secondo la presa deve restare aperta — sono gli eventi a tenere piena la
   * memoria che il ripristino ricaricherà.
   */
  const disponibile = useRef(false);
  /**
   * Quante barre si vogliono, dove le legge chi non ha le dipendenze.
   *
   * L'effetto del montaggio non ha `barre` fra le dipendenze — averle vorrebbe
   * dire ricompilare due shader a ogni cambio di risoluzione — ma la strada del
   * contesto ritrovato ne ha bisogno, e la vuole **aggiornata**: se qualcuno ha
   * cambiato risoluzione mentre il contesto era via, la scena deve tornare con
   * quella nuova, non con quella di prima.
   */
  const barreVolute = useRef(barre);
  /**
   * Dove mandare gli errori, dietro un riferimento.
   *
   * Perché così `rubinetto` è stabile per sempre, e la presa nel motore non si
   * chiude e riapre — due giri di IPC e due lucchetti sul lettore — solo perché
   * chi ci sta sopra ha ricostruito una callback fra due disegni.
   */
  const errore = useRef(onErrore);
  useEffect(() => {
    errore.current = onErrore;
  });
  /**
   * I tetti in vigore, dove li legge chi non ha `qualita` fra le dipendenze.
   *
   * Il ciclo di disegno e il controllore automatico stanno nell'effetto di
   * montaggio, che ha una dipendenza sola e non deve acquisirne altre: rimontarlo
   * per cambiare un tetto vorrebbe dire ricompilare due shader per scrivere un
   * numero, cioè il difetto che la separazione in due record ha appena tolto.
   */
  const tetti = useRef<Tetti>(TETTI[qualita]);
  /**
   * La sorgente è sintetica.
   *
   * Sta in un riferimento e non solo nella prop perché lo legge `rubinetto`, che
   * è stabile per costruzione e non può dipendere da niente. L'effetto che lo
   * tiene aggiornato è dichiarato **prima** di quello di montaggio, quindi al
   * primo giro il valore è già giusto quando la presa potrebbe aprirsi — e il
   * valore iniziale del riferimento è già quello della prima prop, così la
   * garanzia non dipende dall'ordine degli effetti.
   */
  const sintetica = useRef(sorgente === "finto");
  useEffect(() => {
    sintetica.current = sorgente === "finto";
  }, [sorgente]);
  /**
   * Manda la scena a riposo, o la riprende: lo monta l'effetto di montaggio.
   *
   * Serve alla sorgente sintetica, che vive in un effetto suo e deve poter
   * fermare **anche il disegno** quando smette di produrre file. Sono le stesse
   * due funzioni che usa `visibilitychange`, e sono idempotenti: chiamarle da due
   * parti non fa succedere niente due volte.
   */
  const riposo = useRef<(giu: boolean) => void>(() => {});
  /** Riapplica i tetti: lo monta l'effetto di montaggio, per la stessa ragione. */
  const cambiaTetti = useRef<() => void>(() => {});

  /**
   * L'unico posto che accende e spegne la presa nel motore.
   *
   * Ricorda lo stato: con due chiamanti si finisce a spegnerla due volte — o,
   * peggio, a non spegnerla affatto, e il sintomo è una cadenza da 33 ms che
   * resta viva per tutta la vita della finestra mentre nessuno guarda.
   */
  const rubinetto = useCallback((acceso: boolean) => {
    // Con la sorgente sintetica la presa non si apre **mai**, e la guardia sta
    // qui e non nei quattro punti che la chiamano: il proprietario unico è
    // questo, e una regola su chi può aprire il rubinetto scritta in quattro
    // posti è una regola che prima o poi ne dimentica uno. Chiedere di aprirla
    // diventa un'uscita alla prima riga, e non una chiamata IPC che il nucleo
    // esaudirebbe volentieri per una tela che le sue bande non le guarda.
    const voluto = acceso && !sintetica.current;
    if (voluto === aperto.current) return;
    aperto.current = voluto;
    ipc.spettro(voluto).catch((e: unknown) => errore.current(e));
  }, []);

  /**
   * Cosa succede quando arriva una fila, qualunque cosa l'abbia mandata.
   *
   * Sta in una funzione stabile e non dentro l'ascolto perché **ha due
   * chiamanti**: l'evento del nucleo e, con la sorgente sintetica, un
   * `setInterval`. Averne due strade sarebbe stato averne due comportamenti, e
   * il punto della scena finta è precisamente che non ce ne sia un secondo — che
   * quel che si vede nello Studio passi per lo stesso smorzamento, la stessa
   * cascata, la stessa texture e lo stesso seguipulsazioni di quel che si vede
   * nell'app.
   *
   * `ArrayLike<number>` e non `number[]`: dal filo arriva un array JSON, dal
   * generatore una `Uint8Array` prestata, e l'unica cosa che questa funzione
   * chiede a tutte e due è una lunghezza e un indice.
   *
   * Stabile per sempre — le dipendenze sono vuote — perché tutto quel che tocca
   * sta dietro un riferimento. Se non lo fosse, l'effetto della sorgente
   * sintetica si rifarebbe a ogni disegno di chi sta sopra.
   */
  const arrivata = useCallback((fini: ArrayLike<number>) => {
    // Con il contesto perso le manopole non ci sono: si smorza con i valori di
    // serie per il mezzo secondo che ci mette a tornare. È l'unico posto in cui
    // il ripiego serve davvero.
    const m = stabili.current?.manopole ?? SERIE;

    // Due vettori riusati e non due nuovi: trenta allocazioni al secondo da
    // mille byte sono trenta occasioni al secondo perché il raccoglitore fermi
    // il filo che disegna. Cambiando risoluzione si rifanno, ed è anche il modo
    // in cui la memoria dello smorzamento si azzera insieme alle barre.
    if (livelli.current.length !== fini.length) {
      livelli.current = new Float32Array(fini.length);
      arrivo.current = new Uint8Array(fini.length);
    }
    const smorzati = livelli.current;
    const byte = arrivo.current;

    const adesso = performance.now();
    const passato = adesso - quando.current;
    // La media si muove solo su ritardi credibili: il primo evento e quelli
    // dopo una pausa arrivano dopo un'eternità, e la porterebbero a un numero
    // che non descrive niente.
    if (
      quando.current > 0 &&
      passato > PASSO_MS * 0.4 &&
      passato < PASSO_MS * 4
    ) {
      intervallo.current += (passato - intervallo.current) * 0.1;
    }
    quando.current = adesso;

    // I due coefficienti dello smorzamento, ricavati dal tempo vero. A τ zero il
    // filtro è istantaneo: è quel che un autore si aspetta scrivendo zero, e
    // senza questa guardia sarebbe una divisione per zero.
    const dt = intervallo.current;
    const salita = m.salita > 0 ? 1 - Math.exp(-dt / m.salita) : 1;
    const discesa = m.discesa > 0 ? 1 - Math.exp(-dt / m.discesa) : 1;
    // Il fondo scala rimappato. Il byte dice sempre la stessa cosa — è la
    // potenza fra −70 dB e il fondo scala, e quello è il formato del filo — e
    // qui si riporta su un fondo più alto: da `v` si ricava `dB = 70·(v−1)`, e
    // `v' = (dB − F) / (−F)`. Con `F = −70` i due numeri qui sotto valgono uno e
    // zero, cioè l'identità esatta: alzare il fondo non è una correzione al
    // valore di serie, è una scelta che parte da dove il valore di serie
    // finisce.
    const scala = 70 / -m.fondo;
    const scostamento = (-70 - m.fondo) / -m.fondo;

    let massimo = 0;
    for (let i = 0; i < fini.length; i += 1) {
      const grezzo = fini[i] ?? 0;
      if (grezzo > massimo) massimo = grezzo;
      const voluto = Math.min(
        1,
        Math.max(0, (grezzo / 255) * scala + scostamento),
      );
      const prima = smorzati[i] ?? 0;
      const dopo = prima + (voluto - prima) * (voluto > prima ? salita : discesa);
      smorzati[i] = dopo;
      byte[i] = Math.round(dopo * 255);
    }

    silenzio.current = massimo > FRUSCIO ? 0 : silenzio.current + 1;
    // Il cinturino. Prima di ogni altro controllo, e senza condizioni oltre a
    // «c'è del suono»: è quel che impedisce il difetto peggiore possibile qui
    // dentro, una scena nera con la musica che suona perché una macchina a
    // stati si è incastrata da qualche parte.
    if (massimo > FRUSCIO) sveglia.current();

    // La cascata. Fra il cambio di risoluzione e la prima fila della misura
    // nuova passano trenta millesimi: in mezzo arriva una fila della lunghezza
    // di prima, e scriverla in una texture larga un'altra cosa vorrebbe dire
    // disegnare una riga di rumore.
    const r = risoluzione.current;
    if (!r || r.barre !== fini.length) return;
    const s = stabili.current;
    if (s && r.texture) s.gl.bindTexture(s.gl.TEXTURE_2D, r.texture);
    spingi(s, r, 0, byte);
    // La fase resta dentro un giro completo della cascata: `2^piani` è il
    // minimo comune multiplo di tutti i passi, quindi il resto per uno
    // qualunque di essi non cambia, e il numero non cresce per sempre.
    r.fase = (r.fase + 1) % (1 << r.piani);

    // Le prime bande sono i bassi, e i bassi sono il tempo. Un ottavo delle
    // barre, qualunque sia il loro numero: con 64 sono le otto sotto i 100 Hz,
    // con 1024 sono le prime centoventotto — la stessa fascia di frequenze, non
    // lo stesso numero di barre.
    const quante = Math.max(1, fini.length >> 3);
    let bassi = 0;
    for (let i = 0; i < quante; i += 1) bassi += smorzati[i] ?? 0;
    bassi /= quante;
    // Sale di scatto e scende piano, come le barre: un colpo di grancassa deve
    // spingere la camera, non farla vibrare.
    colpo.current =
      bassi > colpo.current
        ? bassi
        : colpo.current + (bassi - colpo.current) * 0.08;
  }, []);

  // Chi guarda le bande. Non tocca lo stato di React: scrive nei riferimenti
  // che legge il disegno, fa scorrere la cascata, e vive quanto la tela.
  //
  // Con la sorgente sintetica l'ascolto **non si apre**: le file se le fabbrica
  // la tela, e restare in ascolto vorrebbe dire ricevere anche quelle vere se
  // qualcun altro nella finestra ha aperto il rubinetto — due sorgenti nella
  // stessa cascata, che è un'immagine che non descrive niente. L'hook si chiama
  // comunque, sempre, incondizionatamente: a spegnersi è l'effetto che ci sta
  // dentro, o l'ordine degli hook cambierebbe fra un disegno e l'altro.
  useAscolto<BandeSpettro>(
    "riproduzione:spettro",
    (carico) => arrivata(carico.fini),
    sorgente !== "finto",
  );

  // ── Il montaggio: le risorse stabili, gli ascoltatori, il ciclo ──────────
  useEffect(() => {
    const canvas = tela.current;
    if (!canvas) return;
    const s = creaStabili(canvas);
    if (!s) return;
    stabili.current = s;
    disponibile.current = true;
    // Lo stato riparte da vivo: il riferimento sopravvive a questo effetto — è
    // del componente — e un montaggio ripetuto lo troverebbe fermo su dove era
    // arrivato il precedente.
    stato.current = "vivo";

    // La misura vive qui e non nelle risorse: è una proprietà del posto in cui
    // la tela sta, non del contesto, e deve sopravvivere a un contesto perso.
    let larghezzaCss = 0;
    let altezzaCss = 0;
    /** La densità che dichiara il sistema, prima di qualunque tetto. */
    let densitaSistema = window.devicePixelRatio || 1;
    // I due tetti in vigore adesso. Partono dal livello scelto — che per
    // «automatica» è il primo gradino della scala — e da lì il controllore li
    // riscrive. Sono variabili di questa chiusura e non campi delle risorse
    // perché non sono proprietà del contesto: sopravvivono a un contesto perso
    // esattamente come la misura, e per la stessa ragione.
    let tettoDensita = tetti.current.densita;
    let riflessoAmmesso = tetti.current.riflesso;
    let scala = Math.min(densitaSistema, tettoDensita);
    let larghezzaPx = 0;
    let altezzaPx = 0;

    // ── Il controllore dell'automatica ──────────────────────────────────────
    //
    // Su quale gradino di `SCALA_AUTO` sta, i ritardi della finestra aperta, e
    // la memoria che gli impedisce di oscillare. Vive qui e non nelle risorse
    // per la stessa ragione dei due tetti: misura il posto, non il contesto.
    let gradino = 0;
    const ritardi = new Float32Array(RITARDI_MAX);
    let campioni = 0;
    let apertura = 0;
    /** Prima di questo istante i fotogrammi non si misurano: vedi `assesta`. */
    let quarantena = 0;
    let ultimoPasso = 0;
    let dimora = DIMORA_MS;
    /** Da che parte è andato l'ultimo gradino: +1 giù, −1 su, 0 mai. */
    let versoUltimo = 0;
    /** Quante finestre buone di fila, per la risalita. */
    let risalite = 0;

    /**
     * Butta la finestra aperta e ne apre una nuova fra un secondo.
     *
     * Lo chiamano tutte le discontinuità: un risveglio, un ridimensionamento,
     * un gradino appena fatto, un contesto ritrovato. Il fotogramma dopo una di
     * queste non dice quanto costa la scena — dice che c'era una texture da
     * caricare o un buffer da riallocare — e misurarlo vorrebbe dire far
     * scendere di un gradino chiunque tiri il bordo della finestra.
     */
    const assesta = () => {
      quarantena = performance.now() + ASSESTAMENTO_MS;
      campioni = 0;
    };

    let fotogramma = 0;
    let nato = performance.now();
    /** Quando è stato disegnato il fotogramma di prima, per l'inseguitore. */
    let precedente = 0;
    /**
     * Il colpo come lo vede la camera.
     *
     * `colpo` cambia trenta volte al secondo e sale tutto in una volta, ed è
     * giusto così per una barra: quel gradino è il colpo, e mostrarlo è il suo
     * mestiere. Per la camera invece era un teletrasporto — mezza unità di mondo
     * in un fotogramma solo, un decimo della scena che si sposta di scatto a
     * ogni grancassa, due volte al secondo. Questo insegue quello nel tempo
     * vero, fotogramma per fotogramma: la stanza respira sul tempo invece di
     * sobbalzarci sopra.
     */
    let seguito = 0;
    let minimizzato = false;
    let vivo = true;
    const scioglie: Array<() => void> = [];

    /**
     * Il moto ridotto del sistema, letto una volta e poi ascoltato.
     *
     * Prima si costruiva una `MediaQueryList` **per fotogramma**. È l'idioma di
     * `tema.ts:107-118`: l'oggetto si tiene, il listener si registra sempre, e
     * `matchMedia` non costa niente finché nessuno cambia impostazione.
     */
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    let pocoMoto = media.matches;
    const cambiaMoto = () => {
      pocoMoto = media.matches;
      sveglia.current();
    };
    media.addEventListener("change", cambiaMoto);

    // `forza` serve alla sola strada del contesto ritrovato: là la misura non è
    // cambiata, ma la tela ha un buffer nuovo e va ridimensionata lo stesso. E
    // il contesto si prende da `stabili.current` e non dalla chiusura, perché
    // dopo un ripristino quello della chiusura è morto.
    const applicaMisura = (forza: boolean) => {
      const l = Math.round(larghezzaCss * scala);
      const a = Math.round(altezzaCss * scala);
      if (!forza && l === larghezzaPx && a === altezzaPx) return;
      larghezzaPx = l;
      altezzaPx = a;
      // Il buffer di disegno cambia misura, quindi i prossimi fotogrammi pagano
      // una riallocazione che non è il costo della scena.
      assesta();
      if (l < 2 || a < 2) return;
      canvas.width = l;
      canvas.height = a;
      stabili.current?.gl.viewport(0, 0, l, a);
    };

    /**
     * Il risveglio, idempotente.
     *
     * Lo chiamano tutti gli ingressi: un evento con del suono, una misura nuova,
     * un cambio di preferenza, la finestra che torna, la riproduzione che
     * riparte. Se il ciclo gira già, esce alla seconda riga.
     *
     * **Non** tocca il rubinetto, e la simmetria è voluta: chi lo chiude lo
     * riapre. Sono due sole coppie — `addormenta` in pausa contro la ripresa
     * della riproduzione, `nascondi` contro `mostra` — e chiuderle qui dentro
     * vorrebbe dire aprirlo a ogni ingresso, cioè due giri di IPC e due
     * lucchetti sul lettore per ogni tacca di un bordo trascinato mentre la
     * musica è ferma.
     */
    const risveglia = () => {
      if (stato.current === "perso" || stato.current === "nascosto") return;
      stato.current = "vivo";
      // Ogni risveglio rilegge i token: la scena può aver dormito attraverso un
      // cambio di skin, e nessuno degli osservatori è tenuto a ricordarselo.
      const attuali = stabili.current;
      if (attuali) attuali.sporco = true;
      if (fotogramma !== 0) return;
      precedente = 0;
      // **Dopo** l'uscita anticipata, e non prima: `risveglia` la chiama ogni
      // evento con del suono, cioè trenta volte al secondo, e armare di là la
      // quarantena vorrebbe dire un controllore che con la musica accesa non
      // misura mai niente. Quel che è una discontinuità è il ciclo che
      // **riparte**, non il ciclo che gira già.
      assesta();
      fotogramma = requestAnimationFrame(disegna);
    };
    sveglia.current = risveglia;

    /** Il silenzio è più profondo della memoria: non c'è più niente da muovere. */
    const addormenta = () => {
      stato.current = "assopito";
      // Solo se è in pausa. Se sta suonando, il risveglio arriva dagli eventi
      // dello spettro e non c'è nient'altro che lo sostituisca.
      if (fermo.current) rubinetto(false);
    };

    const nascondi = () => {
      if (stato.current === "perso" || stato.current === "nascosto") return;
      stato.current = "nascosto";
      cancelAnimationFrame(fotogramma);
      fotogramma = 0;
      rubinetto(false);
    };

    const mostra = () => {
      if (stato.current !== "nascosto") return;
      stato.current = "assopito";
      rubinetto(true);
      risveglia();
    };
    // Le stesse due, per la sorgente sintetica: quando smette di produrre file
    // deve poter fermare anche il disegno, o resterebbe un ciclo che ridisegna
    // sessanta volte al secondo un'immagine che non cambia più. Sono
    // idempotenti, quindi il fatto che le chiamino anche `visibilitychange` e la
    // minimizzazione non è un problema da coordinare.
    riposo.current = (giu: boolean) => {
      if (giu) nascondi();
      else mostra();
    };

    /**
     * Porta in vigore il gradino corrente, o il livello scelto se non è auto.
     *
     * Le due sole cose che cambia sono la densità della tela e il permesso al
     * riflesso: la prima passa per la strada del ridimensionamento, che è la
     * sola che sappia toccare `canvas.width` e il viewport insieme, e la seconda
     * è un `if` nel ciclo. **Nessuna delle due ricostruisce niente**: la texture,
     * gli anelli e i cursori non sanno che è successo qualcosa, ed è per questo
     * che una qualità che si regola da sé non fa mai sparire mezzo minuto di
     * memoria sotto gli occhi di chi guarda.
     */
    const applicaGradino = () => {
      const t = tetti.current;
      const g = t.automatico ? SCALA_AUTO[gradino] : null;
      tettoDensita = g ? g.densita : t.densita;
      riflessoAmmesso = g ? g.riflesso : t.riflesso;
      scala = Math.min(densitaSistema, tettoDensita);
      applicaMisura(false);
      assesta();
      risveglia();
    };

    /** Un gradino, con la dimora che si allunga a ogni inversione. */
    const passo = (adesso: number, verso: number) => {
      gradino += verso;
      // Se questo gradino disfa il precedente, la prossima decisione aspetta il
      // doppio. È la rete contro il caso che né la banda morta né i cinque
      // secondi prendono: un carico che sta esattamente a cavallo delle due
      // soglie, dove ogni gradino sposta il percentile dall'altra parte. La
      // prima inversione costa dieci secondi di attesa, la seconda venti, e in
      // un minuto il controllore si è fermato da solo sul gradino giusto.
      dimora =
        versoUltimo !== 0 && verso !== versoUltimo
          ? Math.min(dimora * 2, DIMORA_MAX)
          : DIMORA_MS;
      versoUltimo = verso;
      ultimoPasso = adesso;
      applicaGradino();
    };

    /**
     * Guarda quanto ci mette questo computer a disegnare un fotogramma.
     *
     * Una finestra di due secondi di ritardi, il novantacinquesimo percentile, e
     * un gradino se sta fuori dalla banda morta. Il percentile e non la media
     * perché quel che si vede come scatto è la coda: una media a 18 con un
     * fotogramma su venti a 40 descrive una scena fluida che non lo è.
     *
     * # Due percentili e non uno, e perché
     *
     * Perché il ritardo fra due `requestAnimationFrame` non è il tempo che la
     * scena ci mette: è il tempo che ci mette **lo schermo**, arrotondato in su
     * al prossimo aggiornamento. Su un pannello a sessanta hertz vale 16,7 o
     * 33,3 e niente in mezzo, quindi due soglie in millesimi non lascerebbero
     * nessuna banda morta — qualunque coppia di numeri fra i due divide le
     * finestre in due, e il controllore si muoverebbe a ogni finestra.
     *
     * La banda morta si ritrova cambiando percentile: **si scende** quando il
     * novantacinquesimo passa 24, cioè quando più di un fotogramma su venti ha
     * saltato un aggiornamento; **si risale** quando il novantanovesimo sta
     * sotto 20, cioè quando in due secondi ne è saltato al più uno. Fra l'uno e
     * il cinque per cento non succede niente, ed è lì che si ferma un computer
     * che sta esattamente al limite.
     */
    const regola = (adesso: number, dt: number) => {
      if (adesso < quarantena) {
        campioni = 0;
        return;
      }
      if (campioni === 0) apertura = adesso;
      if (campioni < RITARDI_MAX) {
        ritardi[campioni] = dt;
        campioni += 1;
      }
      // La finestra si chiude per tempo o per pienezza: su uno schermo molto
      // veloce arriva prima la seconda, ed è giusto che sia così.
      if (campioni < RITARDI_MAX && adesso - apertura < FINESTRA_MS) return;
      // Il novantanovesimo prima: `percentile` ordina sul posto, quindi il
      // secondo lavora su un vettore già ordinato e non costa più niente.
      const p99 = percentile(ritardi, campioni, 0.99);
      const p95 = percentile(ritardi, campioni, 0.95);
      campioni = 0;
      if (adesso - ultimoPasso < dimora) return;
      if (p95 > SOFFRE_MS && gradino < SCALA_AUTO.length - 1) {
        // Scendere è urgente: c'è qualcuno che sta guardando una scena a scatti
        // adesso, e una finestra sola basta.
        risalite = 0;
        passo(adesso, 1);
      } else if (p99 < AGIO_MS && gradino > 0) {
        risalite += 1;
        if (risalite >= RISALITE) {
          risalite = 0;
          passo(adesso, -1);
        }
      } else {
        // Dentro la banda morta, o già in fondo alla scala: il conteggio delle
        // finestre buone riparte, o due finestre buone separate da mezzo minuto
        // varrebbero quanto due di fila.
        risalite = 0;
      }
    };

    // Il livello di qualità è cambiato: si riparte dal primo gradino, perché la
    // memoria del controllore descriveva un tetto che non è più quello.
    cambiaTetti.current = () => {
      gradino = 0;
      risalite = 0;
      dimora = DIMORA_MS;
      versoUltimo = 0;
      ultimoPasso = performance.now();
      applicaGradino();
    };

    const disegna = (adesso: number) => {
      // Il riarmo sta **dopo** tutte le uscite anticipate, e non prima come
      // stava: prima il ciclo non si fermava mai, perché il fotogramma
      // successivo era già chiesto quando si scopriva che non c'era niente da
      // disegnare. Ogni uscita qui sotto ha la sua strada di ritorno — gli
      // eventi per il silenzio, la macchina degli stati per il resto, il
      // ridimensionamento per la tela senza misura — e nessuna lascia la scena
      // spenta senza qualcuno che sappia riaccenderla.
      fotogramma = 0;
      const risorse = stabili.current;
      const r = risoluzione.current;
      if (!risorse || !r || !r.texture) return;
      if (stato.current !== "vivo") return;
      if (silenzio.current > r.copertura) {
        addormenta();
        return;
      }
      if (larghezzaPx < 2 || altezzaPx < 2) return;
      fotogramma = requestAnimationFrame(disegna);

      const { gl, uniformi: u, manopole: m } = risorse;
      if (risorse.sporco) {
        risorse.sporco = false;
        rileggi(risorse, r.barre);
      }

      // Il movimento è ridotto se lo dice il sistema **o** se lo dice la skin.
      // La seconda metà mancava, e non era una sfumatura: una skin `motion:
      // none` fermava tutte le transizioni dell'app e lasciava girare la stanza.
      const ridotto = pocoMoto || m.moto === 0;
      // Quanto si è avanti fra una fila e la prossima. Con il moto ridotto resta
      // a uno: la fila davanti sta alla sua altezza vera invece di crescere da
      // quella dietro, e la scena avanza a scatti — perché una preferenza di
      // accessibilità non si aggira accendendo proprio la cosa che vieta.
      const frazione = ridotto
        ? 1
        : Math.min(1, (adesso - quando.current) / intervallo.current);

      // L'inseguitore, per tempo trascorso e non per fotogramma: a novanta hertz
      // il passo dev'essere più piccolo che a sessanta, o la stanza respirerebbe
      // a velocità diverse su due schermi diversi. Il primo fotogramma non ha un
      // prima, e un risveglio lascia un buco lungo quanto vuole: da lì il
      // minimo e il tetto.
      const dt = precedente > 0 ? Math.min(adesso - precedente, 100) : 16.7;
      precedente = adesso;
      // L'attacco più svelto del rilascio, come per le barre: il colpo deve
      // arrivare, e poi andarsene con calma.
      const inerzia = colpo.current > seguito ? 120 : 450;
      seguito += (colpo.current - seguito) * (1 - Math.exp(-dt / inerzia));

      const tempo = (adesso - nato) / 1000;
      // L'oscillazione: lentissima, e la sua ampiezza cresce col colpo. Meno di
      // quando la memoria era un secondo — un quinto invece di mezzo radiante —
      // perché quel che deve portare il movimento è il girare lento, e il colpo
      // lo accompagna: con l'ampiezza vecchia il termine del colpo era tre volte
      // quello di base, e la scena non oscillava, sbandava.
      //
      // I due termini hanno adesso due manopole distinte, e la separazione è il
      // punto: `sway` è quanto la stanza gira da sola, `beat` è quanto la spinge
      // la musica. Una skin può volere una stanza ferma che pulsa, o una che
      // gira ignorando la grancassa. A uno e uno la formula è identica a prima,
      // numero per numero.
      const oscilla = ridotto
        ? 0
        : Math.sin(tempo * 0.21) *
          (0.16 * m.oscillazione + seguito * 0.22 * m.pulsazione);
      const avvicina = ridotto ? 0 : seguito * 0.06 * m.pulsazione;
      // L'occhio un po' più in alto di quando la memoria era un secondo: con
      // mezzo minuto di terreno davanti, uno sguardo raso terra lo nasconderebbe
      // tutto dietro la prima cresta. Non tanto da diventare una mappa vista
      // dall'alto — la scena resta una stanza in cui si sta, non un grafico da
      // leggere. Adesso lo decide la skin, e le due estremità di
      // `canvas.viz.eye` sono esattamente quelle due scene.
      camera(
        risorse.matrice,
        larghezzaPx / altezzaPx,
        m.lente,
        Math.sin(oscilla) * 1.5,
        m.occhio + seguito * 0.03 * m.pulsazione,
        1.45 - avvicina,
        Math.sin(oscilla) * 0.4,
        0.02,
        -2.2,
      );
      gl.uniformMatrix4fv(u.camera, false, risorse.matrice);
      // La dissolvenza all'apertura: la scena arriva dietro una copertina che
      // sta già ferma sullo schermo, e comparire di colpo la farebbe sembrare un
      // guasto invece di una cosa che si accende. Quanto duri lo dice `--dur-3`,
      // cioè la stessa durata media che l'app usa per le sue transizioni lunghe:
      // era mezzo secondo scritto qui dentro, e una skin che rallenta tutto
      // lasciava indietro questa sola.
      gl.uniform1f(
        u.velo,
        m.dissolvenza > 0 ? Math.min(1, (adesso - nato) / m.dissolvenza) : 1,
      );

      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      gl.bindVertexArray(risorse.vao);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, r.texture);
      gl.uniform1iv(u.cursori, r.cursori);
      gl.uniform1i(u.fase, r.fase);
      gl.uniform1f(u.frazione, frazione);

      // Il riflesso per primo: sta sotto il pavimento, quindi non copre niente.
      // Le facce si rovesciano perché lo specchio inverte il verso, e senza
      // questa riga la scheda butterebbe via proprio quelle che si vedono.
      //
      // E si salta del tutto quando la skin lo spegne: sotto quella soglia ogni
      // suo frammento uscirebbe comunque sotto l'alfa minima. È metà delle
      // chiamate di disegno e metà delle istanze, gratis, e il preset piatto lo
      // fa davvero.
      //
      // L'altra metà della condizione è il tetto di qualità, ed è l'unico posto
      // in cui una qualità e una skin decidono la stessa cosa. La regola è che
      // **spegnere vince**: la skin dice se il riflesso ci sta in questa scena,
      // la qualità se questa macchina se lo può permettere, e nessuna delle due
      // può accenderlo contro l'altra. È anche l'ultimo gradino a cui
      // l'automatica arriva, e non il primo, perché è la sola cosa in questa
      // scala che si vede sparire.
      if (riflessoAmmesso && m.riflesso > 0.004) {
        gl.frontFace(gl.CW);
        gl.uniform1f(u.specchio, -1);
        gl.drawElementsInstanced(
          gl.TRIANGLES,
          risorse.indici,
          gl.UNSIGNED_SHORT,
          0,
          r.istanzeRiflesso,
        );
        gl.frontFace(gl.CCW);
      }
      gl.uniform1f(u.specchio, 1);
      gl.drawElementsInstanced(
        gl.TRIANGLES,
        risorse.indici,
        gl.UNSIGNED_SHORT,
        0,
        r.barre * r.file,
      );

      // In **coda** al fotogramma, e non in mezzo: un gradino ridimensiona la
      // tela, e ridimensionarla fra il calcolo della camera e le due chiamate di
      // disegno vorrebbe dire un fotogramma con l'aspetto di prima dentro un
      // buffer della misura nuova. Qui il cambio si vede dal prossimo, che è
      // quando la scheda ha davvero il buffer nuovo.
      if (tetti.current.automatico) regola(adesso, dt);
    };

    // ── La misura, fuori dal ciclo ──────────────────────────────────────────
    //
    // Prima si leggevano `clientWidth` e `clientHeight` **dentro** il disegno,
    // cioè si chiedeva al motore un layout sessanta volte al secondo per un
    // numero che cambia quando qualcuno tira il bordo della finestra.
    // `ResizeObserver` lo consegna, e da `contentBoxSize` — che è il numero già
    // calcolato — senza leggere niente. Nessun anello: la tela è
    // `position: absolute; inset: 0`, quindi la sua misura non dipende da quel
    // che ci si disegna dentro.
    const osserva = new ResizeObserver(([voce]) => {
      const riquadro = voce?.contentBoxSize?.[0];
      if (!riquadro) return;
      larghezzaCss = riquadro.inlineSize;
      altezzaCss = riquadro.blockSize;
      applicaMisura(false);
      sveglia.current();
    });
    osserva.observe(canvas);

    // ── I token, per invalidazione ──────────────────────────────────────────
    const sporca = () => {
      const attuali = stabili.current;
      if (attuali) attuali.sporco = true;
    };
    const ATTRIBUTI = [
      "data-theme",
      "data-skin",
      "data-density",
      "data-motion",
      "class",
      "style",
    ];
    // La radice, dove `tema.ts:85-91` scrive tema, densità e movimento.
    const radice = new MutationObserver(sporca);
    radice.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ATTRIBUTI,
    });
    // Il più vicino antenato che porta una skin: nell'app è la radice, nello
    // Studio è il riquadro dell'anteprima (`Anteprima.tsx:355-358`), che scrive
    // lì gli stessi quattro attributi.
    const guscio = canvas.closest("[data-skin]");
    const antenato =
      guscio && guscio !== document.documentElement
        ? new MutationObserver(sporca)
        : null;
    if (antenato && guscio) {
      antenato.observe(guscio, {
        attributes: true,
        attributeFilter: ATTRIBUTI,
      });
    }
    // Il genitore della tela, filtrato sui soli `<style>`: è il segnale che
    // dice «la skin in prova è stata ricompilata». Il foglio vero non sta qui —
    // `Anteprima.tsx:324` lo inietta in `.anteprima-guscio`, cioè quattro
    // livelli sopra il genitore della tela, dove questo osservatore non
    // arriverebbe mai. È lo Studio a **far sì** che accanto alla tela ci sia
    // qualcosa di datato: `scene.tsx` monta dentro `.pannello-finto`, fratello
    // della tela, un `<style>` minuscolo che non dipinge niente e porta un
    // contatore di revisione, e quel contatore cresce esattamente quando il
    // foglio vero cambia. Qui si guarda solo quello. Le due alternative erano
    // peggiori: un osservatore su `document` con `subtree: true` vedrebbe la
    // stessa cosa e in più ogni riga di ogni lista che si ridisegna — cioè
    // sarebbe il costo che questo lavoro sta togliendo; e senza nessun segnale
    // vicino, trascinare un cursore si vedrebbe solo alla passata successiva
    // della rete di sicurezza, mezzo secondo dopo.
    const eUnFoglio = (nodo: Node | null) =>
      nodo !== null &&
      (nodo.nodeName === "STYLE" || nodo.parentNode?.nodeName === "STYLE");
    const genitore = canvas.parentElement;
    const foglio = genitore
      ? new MutationObserver((mutazioni) => {
          for (const m of mutazioni) {
            // Un `<style>` che compare o sparisce, o il testo dentro uno che
            // cambia: sono i due modi in cui l'anteprima dello Studio rifà il
            // foglio della skin in prova. Tutto il resto che succede lì sotto —
            // e succede — non ci riguarda.
            if (
              eUnFoglio(m.target) ||
              Array.from(m.addedNodes).some(eUnFoglio) ||
              Array.from(m.removedNodes).some(eUnFoglio)
            ) {
              sporca();
              return;
            }
          }
        })
      : null;
    if (foglio && genitore) {
      foglio.observe(genitore, {
        childList: true,
        subtree: true,
        characterData: true,
      });
    }
    // La rete di sicurezza. Gli osservatori coprono i modi che si conoscono; il
    // mezzo secondo copre quelli che non si conoscono, e costa venti letture
    // due volte al secondo quando la scena disegna — cioè zero quando dorme.
    const rete = window.setInterval(sporca, 500);

    // ── Nascosto: la finestra non si vede ───────────────────────────────────
    const visibilita = () => {
      if (document.hidden) nascondi();
      else mostra();
    };
    document.addEventListener("visibilitychange", visibilita);

    const finestra = getCurrentWindow();
    // La promessa può risolversi dopo lo smontaggio: l'ascoltatore esiste già ed
    // è orfano, e si scioglie lì — che è l'unico posto in cui lo si ha in mano.
    // È l'idioma di `useAscolto` (`pagine.ts:216-241`), per lo stesso motivo.
    const registra = (promessa: Promise<() => void>) => {
      void promessa
        .then((stop) => {
          if (vivo) scioglie.push(stop);
          else stop();
        })
        .catch((e: unknown) => console.error("spettro 3D:", e));
    };
    // Su Tauri `document.visibilityState` non è affidabile — resta «visible» con
    // la finestra minimizzata, e su alcune piattaforme non cambia affatto —
    // quindi è un segnale **in più**, mai l'unico. L'occlusione Tauri v2 non la
    // espone; la minimizzazione su Windows si riconosce da un ridimensionamento
    // ad area nulla, che `isMinimized` conferma prima che se ne tragga una
    // conclusione. Nessun permesso nuovo: `core:window:default` e
    // `core:event:allow-listen` sono già concessi.
    registra(
      finestra.onResized(({ payload }) => {
        if (payload.width === 0 || payload.height === 0) {
          finestra
            .isMinimized()
            .then((giu) => {
              if (giu && vivo) {
                minimizzato = true;
                nascondi();
              }
            })
            .catch((e: unknown) => console.error("spettro 3D:", e));
        } else if (minimizzato) {
          minimizzato = false;
          if (!document.hidden) mostra();
        }
      }),
    );
    // Il monitor cambia densità, o la finestra passa su un altro monitor: il
    // tetto si riapplica e la tela si rimisura senza toccare il layout.
    registra(
      finestra.onScaleChanged(({ payload }) => {
        densitaSistema = payload.scaleFactor || 1;
        scala = Math.min(densitaSistema, tettoDensita);
        applicaMisura(false);
        sveglia.current();
      }),
    );

    // ── Perso e ritrovato ───────────────────────────────────────────────────
    //
    // `preventDefault` è ciò che tiene aperta la strada del ripristino: senza,
    // il browser non manda mai `webglcontextrestored`. I manici si azzerano
    // invece di cancellarsi, perché cancellare un manico di un contesto che non
    // c'è più vuol dire chiamare una scheda che non risponde.
    const perduto = (e: Event) => {
      e.preventDefault();
      stato.current = "perso";
      cancelAnimationFrame(fotogramma);
      fotogramma = 0;
      stabili.current = null;
      const r = risoluzione.current;
      if (r) r.texture = null;
    };
    // Prima non c'era: la scena restava nera fino a che qualcuno non chiudeva e
    // riapriva la schermata. Adesso si ricostruisce da sola, e la texture si
    // ricarica dagli anelli — che sono sopravvissuti perché stanno in memoria
    // normale. Mezzo minuto di musica attraversa un riavvio del driver.
    const ritrovato = () => {
      const vecchia = risoluzione.current;
      const nuove = creaStabili(canvas);
      if (!nuove) {
        console.error("spettro 3D: il contesto non è tornato");
        return;
      }
      stabili.current = nuove;
      stato.current = "assopito";
      nato = performance.now();
      precedente = 0;
      // Con `vecchia` alla stessa risoluzione la memoria si riprende; se nel
      // frattempo qualcuno ha cambiato il numero di barre, gli anelli si rifanno
      // e mezzo minuto si perde — che è quel che succede a cambiare risoluzione
      // anche a contesto sano, e per la stessa ragione.
      risoluzione.current = creaRisoluzione(
        nuove,
        barreVolute.current,
        tetti.current.perPiano,
        vecchia,
      );
      applicaMisura(true);
      risveglia();
    };
    canvas.addEventListener("webglcontextlost", perduto);
    canvas.addEventListener("webglcontextrestored", ritrovato);

    fotogramma = requestAnimationFrame(disegna);
    return () => {
      vivo = false;
      sveglia.current = () => {};
      riposo.current = () => {};
      cambiaTetti.current = () => {};
      cancelAnimationFrame(fotogramma);
      window.clearInterval(rete);
      media.removeEventListener("change", cambiaMoto);
      osserva.disconnect();
      radice.disconnect();
      antenato?.disconnect();
      foglio?.disconnect();
      document.removeEventListener("visibilitychange", visibilita);
      canvas.removeEventListener("webglcontextlost", perduto);
      canvas.removeEventListener("webglcontextrestored", ritrovato);
      for (const stop of scioglie) stop();
      distruggiRisoluzione(stabili.current, risoluzione.current);
      risoluzione.current = null;
      const ultime = stabili.current;
      if (ultime) distruggiStabili(ultime);
      stabili.current = null;
      disponibile.current = false;
    };
    // Una dipendenza sola, e non è una dipendenza: `rubinetto` è stabile per
    // costruzione — `onErrore` sta dietro un riferimento apposta perché lo sia.
    // `barre` non è qui e non deve esserci: rimontare questo effetto vorrebbe
    // dire ricompilare due shader per cambiare la larghezza di una texture, che
    // è precisamente il difetto che la separazione in due record ha tolto.
  }, [rubinetto]);

  // ── La risoluzione: solo la texture ─────────────────────────────────────
  //
  // Prima questo effetto era l'unico, e cambiare il numero di barre voleva dire
  // ricompilare due shader, rilinkare un programma e ricostruire un VAO per
  // cambiare la larghezza di una texture.
  //
  // La seconda dipendenza **non** è `qualita`, ed è la differenza fra un
  // pacchetto corretto e uno che ricostruisce la scena per niente: è il solo
  // numero che quel livello porta fin qui, e vale 8 per due livelli su tre.
  // Passare da «automatica» ad «alta» non lo cambia, quindi non rifà niente;
  // solo un passaggio da o verso «bassa» lo muove, ed è l'unico caso in cui la
  // texture deve davvero rinascere.
  const tettoPerPiano = TETTI[qualita].perPiano;
  useEffect(() => {
    barreVolute.current = barre;
    const s = stabili.current;
    // Senza WebGL 2 non c'è niente da dimensionare; con il contesto perso ci
    // penserà il ripristino, che il numero di barre lo rilegge dal riferimento
    // appena scritto.
    if (!s) return;
    risoluzione.current = creaRisoluzione(
      s,
      barre,
      tettoPerPiano,
      risoluzione.current,
    );
    // La larghezza di una barra dipende da quante ce ne sono, quindi il token
    // che la governa va riletto — e il confronto per token, da solo, non se ne
    // accorgerebbe: la stringa nel foglio non è cambiata, è cambiato il senso
    // che ha. Si azzerano tutti e non solo quello: sono tre pipette e succede
    // quando un dito preme una linguetta, non trenta volte al secondo.
    s.grezzi.fill(MAI);
    s.sporco = true;
    sveglia.current();
    return () => {
      distruggiRisoluzione(stabili.current, risoluzione.current);
    };
  }, [barre, tettoPerPiano]);

  // ── La qualità: due numeri e un permesso, nessuna ricostruzione ─────────
  //
  // Dopo l'effetto di montaggio, che è quello che monta `cambiaTetti`. Quel che
  // arriva di qui è la densità della tela e il permesso al riflesso: la prima
  // passa per la strada del ridimensionamento, la seconda è un `if` nel ciclo, e
  // nessuna delle due tocca la texture, gli anelli o i cursori. Il terzo numero
  // — quante file ha ogni piano — è passato dall'effetto qui sopra, che è
  // l'unico autorizzato a ricostruire qualcosa.
  useEffect(() => {
    tetti.current = TETTI[qualita];
    cambiaTetti.current();
  }, [qualita]);

  // ── Il rubinetto: nasce e muore con la tela ─────────────────────────────
  //
  // Dopo l'effetto di montaggio, e non prima: è quello a dire se WebGL 2 c'è, e
  // senza contesto la presa **non** si apre. Prima si apriva lo stesso, e il
  // motore faceva trenta trasformate da 4096 punti al secondo per una tela che
  // non disegnava niente.
  //
  // E con la sorgente sintetica non si apre affatto: `rubinetto` lo saprebbe da
  // solo — la guardia sta là dentro, dov'è il proprietario — ma un effetto che
  // chiede una cosa sapendo che non succederà è una riga che il prossimo lettore
  // deve verificare. Meglio non chiederla.
  useEffect(() => {
    if (!disponibile.current || sorgente === "finto") return;
    rubinetto(true);
    return () => rubinetto(false);
  }, [rubinetto, sorgente]);

  // ── La pausa ────────────────────────────────────────────────────────────
  //
  // Non ferma la scena: il passato deve **uscire** invece di congelarsi a metà
  // strada, e a fermare il lavoro ci pensa la regola del silenzio trentatré
  // secondi dopo. Serve a due cose sole: dire alla macchina se può chiudere il
  // rubinetto quando si assopisce, e riaprirlo quando si riparte — perché se lo
  // aveva chiuso, non arriverebbe più nessun evento a svegliarla.
  useEffect(() => {
    fermo.current = inPausa;
    // A finestra nascosta no: là comanda `mostra`, e riaprire la presa per una
    // scena che nessuno sta guardando è esattamente il costo che la macchina del
    // riposo esiste per non pagare.
    if (inPausa || !disponibile.current || stato.current === "nascosto") return;
    rubinetto(true);
    sveglia.current();
  }, [inPausa, rubinetto]);

  // ── La sorgente sintetica ───────────────────────────────────────────────
  //
  // Un `setInterval` alla cadenza della scena che chiama `arrivata` — la
  // **stessa** funzione dell'evento vero — con una fila fabbricata in forma
  // chiusa sul numero dell'evento. Da lì in poi non esiste un secondo percorso:
  // lo smorzamento, la cascata, il caricamento della texture, il
  // seguipulsazioni e la lettura dei token sono quelli, riga per riga. È il
  // punto di tutto il pacchetto — una scena finta che passasse da un'altra
  // parte mostrerebbe una skin che non è quella che si sta dipingendo.
  //
  // # Le tre condizioni di arresto, e perché la seconda è obbligatoria
  //
  // Lo smontaggio, la finestra nascosta, e il riquadro uscito dalla vista.
  //
  // La seconda non è una gentilezza. La scena vera si addormenta da sola sul
  // silenzio, che è il modo in cui questo file smette di lavorare quando nessuno
  // guarda; un segnale sintetico **non tace mai**, quindi quella strada non
  // esiste. Senza `visibilitychange`, lo Studio lasciato aperto su Pannelli
  // dietro un'altra finestra terrebbe una scheda video occupata per sempre.
  //
  // La terza è la stessa cosa a un livello più fine: nello Studio l'anteprima è
  // un riquadro dentro una colonna che scorre, e un riquadro scrollato via è
  // nascosto quanto una finestra dietro un'altra. `IntersectionObserver` lo dice
  // senza leggere il layout, che è l'unico modo di chiederlo che non costi.
  //
  // Le tre fermano **anche il disegno** e non solo la fabbrica delle file:
  // `riposo` è la coppia `nascondi`/`mostra` della macchina del riposo, cioè le
  // stesse due funzioni che usa `visibilitychange` là dentro. Fermare solo la
  // sorgente lascerebbe un ciclo che ridisegna un'immagine ferma.
  useEffect(() => {
    if (sorgente !== "finto") return;
    const canvas = tela.current;
    if (!canvas) return;
    // Senza WebGL 2 non c'è niente da alimentare, ed è la stessa regola del
    // rubinetto: la fabbrica delle file non deve girare per una tela che non
    // disegna. Un contesto **perso** invece non la ferma, e non è
    // un'incoerenza — gli anelli stanno in memoria normale, quindi le file che
    // arrivano mentre il driver si riavvia sono esattamente quelle che il
    // ripristino ricaricherà nella texture nuova.
    if (!disponibile.current) return;

    // Un vettore solo per tutta la vita dell'effetto: trenta `Uint8Array` al
    // secondo sarebbero trenta occasioni al secondo perché il raccoglitore fermi
    // il filo che disegna.
    const fila = new Uint8Array(barre);
    let evento = 0;
    let battito = 0;
    // Fuori dalla vista finché l'osservatore non dice il contrario, e non il
    // contrario: partire acceso vorrebbe dire accendere il battito e la scena
    // per il fotogramma o due che l'osservatore ci mette a consegnare la prima
    // osservazione, anche per un riquadro che è montato già fuori dallo schermo.
    // La prima osservazione arriva sempre, e porta lo stato di adesso.
    let inVista = false;

    const spingiUna = () => {
      filaFinta(fila, evento);
      evento += 1;
      arrivata(fila);
    };

    // Con il movimento ridotto: **una** fila e basta. Non è una scena rallentata
    // — sarebbe pur sempre una scena che si muove — è un quadro fermo dei colori
    // che la skin dà alla base, al corpo e alla cresta di una barra, che è quel
    // che serve davvero a chi sta scegliendo tre tinte. Il resto del terreno
    // resta alla sua altezza minima, cioè la riga di base che dice «c'è, e
    // adesso è a zero».
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      spingiUna();
      // E poi si applica a mano la regola del silenzio, che a una sorgente che
      // ha smesso di parlare non arriverebbe mai da sola: vedi `FERMO_MS`.
      const posa = window.setTimeout(() => {
        silenzio.current = Number.MAX_SAFE_INTEGER;
      }, FERMO_MS);
      return () => window.clearTimeout(posa);
    }

    // Un solo posto che accende e spegne il battito, e ricorda lo stato: è la
    // stessa disciplina del rubinetto, per lo stesso motivo — con due chiamanti
    // si finisce a lasciarne acceso uno.
    const regolaBattito = () => {
      const vuole = !document.hidden && inVista;
      if (vuole === (battito !== 0)) return;
      if (vuole) {
        battito = window.setInterval(spingiUna, PASSO_MS);
        riposo.current(false);
      } else {
        window.clearInterval(battito);
        battito = 0;
        riposo.current(true);
      }
    };

    document.addEventListener("visibilitychange", regolaBattito);
    // È l'osservatore ad accendere il battito la prima volta, con la sua prima
    // osservazione: nessuna chiamata a mano qui sotto, o si accenderebbe una
    // scena che magari non si vede.
    const occhio = new IntersectionObserver((voci) => {
      // L'ultima e non la prima: una raffica di osservazioni descrive una
      // storia, e quel che conta è dove si è arrivati.
      const ultima = voci[voci.length - 1];
      if (!ultima) return;
      inVista = ultima.isIntersecting;
      regolaBattito();
    });
    occhio.observe(canvas);

    return () => {
      document.removeEventListener("visibilitychange", regolaBattito);
      occhio.disconnect();
      if (battito !== 0) window.clearInterval(battito);
    };
    // `arrivata` è stabile per costruzione — dipendenze vuote, tutto dietro
    // riferimenti — quindi questo effetto si rifà solo cambiando sorgente o
    // numero di barre, che sono le due cose che cambiano davvero la fila.
  }, [sorgente, barre, arrivata]);

  return (
    <canvas
      ref={tela}
      className="scena-spettro viz-screen"
      /* Decorazione che segue il suono: quel che dice — «sta suonando» — è già
         nel trasporto, e non c'è un testo che la sostituisca senza inventarlo. */
      aria-hidden="true"
    />
  );
}
