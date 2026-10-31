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
 * E la camera che oscilla lo fa con l'energia delle bande più basse — l'ottavo
 * basso, da 20 a circa 47 Hz: il sub-basso, il fondo della grancassa e del
 * basso — ferma la musica, si ferma anche lei.
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
 * Chi spinge le file nella cascata è l'**orologio**, nel ciclo di disegno — ma
 * da una **coda**, non da un booleano, ed è la differenza con l'errore di una
 * volta. Quando la cascata stava nel ciclo con un booleano, «è arrivata una
 * fila» si perdeva se ne arrivavano due fra un fotogramma e l'altro: su uno
 * schermo a trenta hertz una fila su tante spariva senza mai entrare nella
 * memoria, e con il ciclo addormentato non ne entrava nessuna. La correzione di
 * allora fu spostarla nel gestore dell'evento; quella di adesso è la coda. Ogni
 * fila arrivata ci entra, l'orologio le prende tutte e in ordine, e quando il
 * ciclo si ferma — sonno, finestra nascosta, contesto perso — la coda si versa
 * nella cascata e le file tornano a entrare dirette dal gestore. Il passato
 * resta esatto e indipendente dal numero di fotogrammi al secondo.
 *
 * # L'orologio, e perché le file non entrano quando arrivano
 *
 * Perché il ritmo d'arrivo non è il ritmo della musica: gli eventi passano dal
 * filo della finestra di Tauri, e arrivano quando quel filo è libero. Fino alla
 * 3.0.1 la scena scorreva ad arrivi, e ogni evento in anticipo o in ritardo di
 * qualche millesimo diventava un saltino o una sosta — il banco del 22-10-2026
 * lo misurava fra 0,89 e 1,10 del dovuto, fotogramma per fotogramma — e uno
 * stallo del filo diventava una raffica, cioè un salto di più file. Adesso la
 * scena disegna un filo nel passato con un orologio suo: `orologioSpettro.ts`
 * spiega come, e che cosa costa (qualche millesimo).
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
 * controllo che calzi e di cui uno sbagliato dà una scena nera; l'altezza
 * minima `0.004`, che è la garanzia «c'è, e adesso è a zero»; e le soglie **in
 * pixel** con cui lo shader chiude le fessure sotto il pixel e spegne il
 * contrasto delle barre più strette di quattro (vedi `fessuraPx` e
 * `vDettaglio`), che sono regole su quel che uno schermo sa mostrare, non gusti
 * — una skin che le abbassasse riporterebbe la grana, non uno stile. Lo stesso
 * vale per la sfumatura verticale che tiene leggibile il testo, che era una
 * maschera nel foglio e adesso sta nel frammento con gli stessi tre stop.
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
 * Quindi si legge **per invalidazione**: un flag che si alza da due osservatori
 * di mutazioni sul documento della tela (la radice, dove `tema.ts` scrive tema,
 * densità e movimento; e la testa, dove stanno il foglio della skin indossata
 * o, sulla tela dello Studio, quello della bozza), da una rete di sicurezza
 * mezzo secondo, e da ogni risveglio. Il confronto è **per token e non su una firma unica**: con
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
 * Le decisioni stanno in `riposoSpettro.ts`, puro e provato: questa tela esegue
 * i passi che le restituisce. Lì anche le due cose che mancavano: la finestra
 * nascosta tenuta a parte dallo stato — un contesto perso a finestra giù la
 * cancellava, e al ritorno la presa restava chiusa con la musica che suonava —
 * e il contesto che non torna, che dopo `ATTESA_RITORNO_MS` chiude la presa e
 * lo dice nel recinto della scena invece di lasciare un vuoto muto.
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
 * È pensata per lo Studio, dove si dipinge una skin e non c'è nessun audio da
 * guardare — ma **oggi nessuno la monta**. L'unico che monta questa tela è
 * `InRiproduzione`, senza `sorgente`, cioè con il motore; e sulla tela dello
 * Studio, che pure disegna `InRiproduzione`, il mondo finto risponde
 * `spettroVisibile: false` (`studio/mondo.ts`), quindi la scena lì non compare
 * e i token `canvas.viz.*` si ritoccano senza vederne l'effetto. Il ramo
 * sintetico qui sotto — `filaFinta`, il battito, l'`IntersectionObserver`,
 * `FERMO_MS` — è pronto e non esercitato: per accenderlo, il mondo dello
 * Studio deve rispondere `spettroVisibile` vero sulla pagina dello schermo e
 * `InRiproduzione` deve passare `sorgente="finto"` solo lì, o la tela dello
 * Studio aprirebbe la presa vera del motore. Quel che segue descrive come
 * funziona quando lo si monta.
 *
 * La ragione per cui la sorgente si scambia invece di scrivere una
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
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { ipc, type BandeSpettro, type QualitaSpettro } from "../ipc";
import { t } from "../lingue";
import { useAscolto } from "../pagine";
import { passoTempo, posaCamera } from "./cameraSpettro";
import { OrologioSpettro } from "./orologioSpettro";
import * as macchina from "./riposoSpettro";
import { AGIO_MS, ASSESTAMENTO_MS, DIMORA_MAX, DIMORA_MS, FINESTRA_MS, PASSO_MS, percentile, RISALITE, RITARDI_MAX, SCALA_AUTO, SOFFRE_MS, TETTI, type Tetti } from "./spettro3d/costi";
import { CAPIENZA, creaRisoluzione, creaStabili, distruggiRisoluzione, distruggiStabili, esce, nuovaCoda, rileggi, spingi, type Coda, type RisorseRisoluzione, type RisorseStabili } from "./spettro3d/risorse";
import { camera, MAI, SERIE } from "./spettro3d/shader";

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

/** In che stato è la scena. */
type Stato = macchina.StatoScena;

/**
 * Quanto si aspetta il ritorno di un contesto perso, a finestra visibile, prima
 * di darlo per perduto: vedi «Il contesto che non torna» in `riposoSpettro.ts`.
 *
 * Il ripristino arriva di norma in mezzo secondo; cinque secondi sono un
 * margine largo per un driver che si riavvia, e abbastanza corti perché una
 * scena che non torna non tenga aperta la presa nel motore per niente.
 */
const ATTESA_RITORNO_MS = 5000;

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
   * scelto. Servirebbe a chi la monta per guardare la skin e non per
   * ascoltare — lo Studio, che oggi non la monta: vedi «La scena finta» in
   * testa — dove il livello di qualità di **questo** computer non c'entra
   * niente con quel che si sta dipingendo. Oggi nessuno la passa.
   */
  qualita?: QualitaSpettro | undefined;
  /**
   * Da dove arrivano le file.
   *
   * `"motore"` sono le bande vere, e la presa nel motore si apre. `"finto"` è un
   * generatore deterministico che scrive nello stesso posto: la presa **non** si
   * apre, l'ascolto dell'evento non si registra, e tutto quel che viene dopo —
   * smorzamento, cascata, texture, seguipulsazioni, lettura dei token — è la
   * stessa identica strada. È pensata per lo Studio, per mostrare una scena
   * viva dove non c'è nessun audio senza che esista una seconda scena da tenere
   * allineata a questa; ma oggi nessuno passa `"finto"`: vedi «La scena finta»
   * in testa.
   */
  sorgente?: "motore" | "finto" | undefined;
  onErrore: (e: unknown) => void;
}) {
  const tela = useRef<HTMLCanvasElement>(null);
  const stabili = useRef<RisorseStabili | null>(null);
  const risoluzione = useRef<RisorseRisoluzione | null>(null);
  const stato = useRef<Stato>("vivo");
  /**
   * La finestra non si vede, qualunque sia lo stato: vedi «La finestra
   * nascosta è un fatto a parte dallo stato» in `riposoSpettro.ts`.
   */
  const nascosta = useRef(false);
  /**
   * Lancia un guasto dentro il disegno, perché lo prenda il recinto che sta
   * intorno alla scena (`RecintoErrori` in `InRiproduzione`) e metta «Riprova»
   * al posto della tela. Serve al contesto che non torna.
   */
  const [, lanciaGuasto] = useState<null>(null);
  /** Il risveglio, che lo monta l'effetto di montaggio: vedi `sveglia`. */
  const sveglia = useRef<() => void>(() => {});
  /** L'ultima fila, in byte, quando entra nella cascata senza passare dalla coda. */
  const arrivo = useRef<Uint8Array>(new Uint8Array(0));
  /** Gli stessi livelli in virgola mobile: è qui che vive lo smorzamento. */
  const livelli = useRef<Float32Array>(new Float32Array(0));
  /** Le file arrivate che aspettano il loro momento: vedi `Coda`. */
  const coda = useRef<Coda>(nuovaCoda(0));
  /**
   * L'orologio della scena: quando una fila entra, e quanto è cresciuta quella
   * davanti. Vedi `orologioSpettro.ts`, che è dove sta il perché.
   */
  const orologio = useRef<OrologioSpettro | null>(null);
  orologio.current ??= new OrologioSpettro(PASSO_MS);
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
   * Una fila entra nella cascata, davanti: la texture, la fase, il colpo.
   *
   * Due chiamanti e una strada sola: l'orologio, che prende le file dalla coda
   * quando il **suo** tempo lo dice, e `arrivata`, quando il ciclo non gira e
   * non c'è un orologio a svuotarla.
   *
   * Stabile per sempre, come `arrivata`: tutto quel che tocca sta dietro un
   * riferimento.
   */
  const entra = useCallback((fila: Uint8Array, bassi: number) => {
    const r = risoluzione.current;
    if (!r || r.barre !== fila.length) return;
    const s = stabili.current;
    if (s && r.texture) s.gl.bindTexture(s.gl.TEXTURE_2D, r.texture);
    spingi(s, r, 0, fila);
    // La fase resta dentro un giro completo della cascata: `2^piani` è il
    // minimo comune multiplo di tutti i passi, quindi il resto per uno
    // qualunque di essi non cambia, e il numero non cresce per sempre.
    r.fase = (r.fase + 1) % (1 << r.piani);
    // Sale di scatto e scende piano, come le barre: un colpo di grancassa deve
    // spingere la camera, non farla vibrare.
    colpo.current =
      bassi > colpo.current
        ? bassi
        : colpo.current + (bassi - colpo.current) * 0.08;
  }, []);

  /**
   * Versa la coda nella cascata, in ordine, e ferma l'orologio.
   *
   * La chiama chiunque fermi il ciclo — il sonno, la finestra nascosta, il
   * contesto perso, il moto ridotto — **prima** di fermarlo: da lì in poi le
   * file entrano dirette, e una fila rimasta in coda arriverebbe dopo di loro.
   */
  const versa = useCallback(() => {
    const c = coda.current;
    while (c.lunghezza > 0) esce(c, entra);
    c.attiva = false;
    orologio.current?.azzera();
  }, [entra]);

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
  const arrivata = useCallback(
    (fini: ArrayLike<number>) => {
      // Con il contesto perso le manopole non ci sono: si smorza con i valori di
      // serie per il mezzo secondo che ci mette a tornare. È l'unico posto in cui
      // il ripiego serve davvero.
      const m = stabili.current?.manopole ?? SERIE;

      // Due vettori riusati e non due nuovi: trenta allocazioni al secondo da
      // mille byte sono trenta occasioni al secondo perché il raccoglitore fermi
      // il filo che disegna. Cambiando risoluzione si rifanno, ed è anche il modo
      // in cui la memoria dello smorzamento si azzera insieme alle barre — e la
      // coda, le cui file sono larghe quanto le barre di prima e non entrerebbero
      // più da nessuna parte.
      if (livelli.current.length !== fini.length) {
        livelli.current = new Float32Array(fini.length);
        arrivo.current = new Uint8Array(fini.length);
        const attiva = coda.current.attiva;
        coda.current = nuovaCoda(fini.length);
        coda.current.attiva = attiva;
        orologio.current?.azzera();
      }
      const smorzati = livelli.current;
      const byte = arrivo.current;

      // I due coefficienti dello smorzamento, sul tempo **della musica**: una fila
      // è `PASSO_MS` di suono, qualunque sia l'istante in cui è arrivata. Fino
      // alla 3.0.1 il passo era l'intervallo d'arrivo misurato, che è il ritmo
      // del filo della finestra e non quello della canzone; adesso che a
      // decidere quando una fila si vede è l'orologio, l'arrivo non deve più
      // decidere niente. Un filtro con `α = 1 − exp(−Δt/τ)` resta indipendente
      // dalla cadenza per costruzione. A τ zero il filtro è istantaneo: è quel
      // che un autore si aspetta scrivendo zero, e senza questa guardia sarebbe
      // una divisione per zero.
      const salita = m.salita > 0 ? 1 - Math.exp(-PASSO_MS / m.salita) : 1;
      const discesa = m.discesa > 0 ? 1 - Math.exp(-PASSO_MS / m.discesa) : 1;
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

      // Fra il cambio di risoluzione e la prima fila della misura nuova passano
      // trenta millesimi: in mezzo arriva una fila della lunghezza di prima, e
      // scriverla in una texture larga un'altra cosa vorrebbe dire disegnare una
      // riga di rumore.
      const r = risoluzione.current;
      if (!r || r.barre !== fini.length) return;

      // Le prime bande sono i bassi, e i bassi sono il tempo. Un ottavo delle
      // barre, qualunque sia il loro numero: con 64 sono le prime otto, con 1024
      // le prime centoventotto — la stessa fascia di frequenze, non lo stesso
      // numero di barre. Quale fascia: le bande del nucleo sono logaritmiche
      // uguali fra 20 Hz e 20 kHz (`rymembra_play::spettro`, `bordi_di`), quindi
      // l'ottavo basso finisce a 20·1000^(1/8), circa 47 Hz. È il sub-basso, non
      // «sotto i 100 Hz» come stava scritto: una cassa che sta fra 60 e 90 Hz,
      // con poco sotto, spinge la camera poco. Si calcola qui, dove i livelli ci
      // sono, ma spinge la camera solo quando la fila **entra**: vedi `entra`.
      const quante = Math.max(1, fini.length >> 3);
      let bassi = 0;
      for (let i = 0; i < quante; i += 1) bassi += smorzati[i] ?? 0;
      bassi /= quante;

      // A ciclo fermo non c'è un orologio che svuoti la coda: la fila entra
      // subito, com'è sempre entrata. È la strada che tiene esatta la memoria
      // mentre la scena dorme, e che il paragrafo sulla cascata in testa al file
      // racconta.
      const c = coda.current;
      if (!c.attiva) {
        entra(byte, bassi);
        return;
      }
      // Una coda piena è un ciclo che non gira da un pezzo — una finestra
      // coperta che il browser non ridisegna più, per esempio. La più vecchia
      // entra adesso, per far posto: l'ordine resta quello d'arrivo.
      if (c.lunghezza >= CAPIENZA) esce(c, entra);
      orologio.current?.arrivo(performance.now(), c.lunghezza);
      const posto = (c.testa + c.lunghezza) % CAPIENZA;
      c.file[posto]?.set(byte);
      c.bassi[posto] = bassi;
      c.lunghezza += 1;
    },
    [entra],
  );

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
    nascosta.current = false;

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
    /**
     * Il tempo della scena, in millisecondi: la somma dei passi dei fotogrammi
     * **disegnati**. Muove l'oscillazione della camera, che contata da `nato`
     * correva anche mentre la scena dormiva e al risveglio saltava. `nato`
     * resta alla sola dissolvenza. Vedi `posaCamera`.
     */
    let tempoScena = 0;
    /** Il ritorno del contesto che si aspetta: vedi `ATTESA_RITORNO_MS`. */
    let attesaRitorno = 0;
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
     * riapre. Sono due coppie — `addormenta` in pausa contro la ripresa della
     * riproduzione, `nascondi` contro `mostra` — più il contesto ritrovato,
     * che la riapre a finestra visibile, e quello che non torna, che la chiude
     * (`riposoSpettro.ts`); e chiuderle qui dentro
     * vorrebbe dire aprirlo a ogni ingresso, cioè due giri di IPC e due
     * lucchetti sul lettore per ogni tacca di un bordo trascinato mentre la
     * musica è ferma.
     */
    const risveglia = () => {
      if (stato.current === "perso" || stato.current === "nascosto") return;
      stato.current = "vivo";
      if (fotogramma !== 0) return;
      precedente = 0;
      // **Dopo** l'uscita anticipata, e non prima: `risveglia` la chiama ogni
      // evento con del suono, cioè trenta volte al secondo, e armare di là la
      // quarantena vorrebbe dire un controllore che con la musica accesa non
      // misura mai niente. Quel che è una discontinuità è il ciclo che
      // **riparte**, non il ciclo che gira già.
      assesta();
      // Ogni risveglio rilegge i token: la scena può aver dormito attraverso un
      // cambio di skin, e nessuno degli osservatori è tenuto a ricordarselo.
      //
      // Anche questa dopo l'uscita, per la stessa ragione della quarantena, ed è
      // stata a lungo prima. Con la musica accesa voleva dire venti letture
      // dello stile vivo a ogni evento — trenta volte al secondo, ognuna un
      // ricalcolo forzato su un documento che lo Scrubber sporca venti volte al
      // secondo — cioè esattamente il costo che «Come si leggono i token»
      // dichiara tolto. Il ciclo che gira già ha i suoi osservatori e la sua
      // rete; è il ciclo che riparte a non sapere cosa si è perso.
      const attuali = stabili.current;
      if (attuali) attuali.sporco = true;
      fotogramma = requestAnimationFrame(disegna);
    };
    sveglia.current = risveglia;

    /** Il silenzio è più profondo della memoria: non c'è più niente da muovere. */
    const addormenta = () => {
      versa();
      stato.current = "assopito";
      // Solo se è in pausa. Se sta suonando, il risveglio arriva dagli eventi
      // dello spettro e non c'è nient'altro che lo sostituisca.
      if (fermo.current) rubinetto(false);
    };

    /**
     * Esegue un passo della macchina del riposo (`riposoSpettro.ts`): lo stato,
     * il ciclo da fermare, la presa, il risveglio, il guasto. Nell'ordine di
     * sempre: prima lo stato, poi il ciclo, poi la presa, poi il risveglio.
     */
    const esegui = (passo: macchina.Passo) => {
      stato.current = passo.stato;
      nascosta.current = passo.nascosta;
      if (passo.ferma) {
        versa();
        cancelAnimationFrame(fotogramma);
        fotogramma = 0;
      }
      if (passo.attesa !== null) {
        // L'attesa del ritorno di un contesto perso, a finestra visibile:
        // vedi `ATTESA_RITORNO_MS`. Riarmarla la fa ripartire da capo.
        window.clearTimeout(attesaRitorno);
        attesaRitorno = 0;
        if (passo.attesa === "arma") {
          attesaRitorno = window.setTimeout(() => {
            attesaRitorno = 0;
            if (stato.current === "perso") esegui(macchina.scaduto(riposoOra()));
          }, ATTESA_RITORNO_MS);
        }
      }
      if (passo.rubinetto !== null) rubinetto(passo.rubinetto);
      if (passo.risveglia) risveglia();
      if (passo.guasto) {
        const guasto = new Error(t("spectrum.lost"));
        lanciaGuasto(() => {
          throw guasto;
        });
      }
    };
    const riposoOra = (): macchina.Riposo => ({
      stato: stato.current,
      nascosta: nascosta.current,
    });

    const nascondi = () => esegui(macchina.nascondi(riposoOra()));

    const mostra = () => esegui(macchina.mostra(riposoOra()));
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
      // Il silenzio non salta più il disegno: addormentandosi la scena disegna
      // l'ultimo fotogramma e non riarma. Vedi `passoCiclo`.
      const giro = macchina.passoCiclo({
        silenzio: silenzio.current,
        copertura: r.copertura,
        misuraValida: larghezzaPx >= 2 && altezzaPx >= 2,
      });
      if (!giro.disegna) {
        if (giro.addormenta) addormenta();
        return;
      }
      if (giro.riarma) fotogramma = requestAnimationFrame(disegna);

      const { gl, uniformi: u, manopole: m } = risorse;
      if (risorse.sporco) {
        risorse.sporco = false;
        rileggi(risorse, r.barre);
      }

      // Il movimento è ridotto se lo dice il sistema **o** se lo dice la skin.
      // La seconda metà mancava, e non era una sfumatura: una skin `motion:
      // none` fermava tutte le transizioni dell'app e lasciava girare la stanza.
      const ridotto = pocoMoto || m.moto === 0;
      // Quanto si è avanti fra una fila e la prossima, e quali file entrano in
      // questo fotogramma. Lo decide l'orologio, sul tempo che passa e non
      // sull'istante in cui le file arrivano: vedi `orologioSpettro.ts`. Le
      // file si prendono **prima** di caricare cursori, fase e frazione qui
      // sotto, o la fila davanti crescerebbe verso una fila che non c'è ancora.
      //
      // Con il moto ridotto niente orologio: la frazione resta a uno, la fila
      // davanti sta alla sua altezza vera invece di crescere da quella dietro, e
      // la scena avanza a scatti — perché una preferenza di accessibilità non si
      // aggira accendendo proprio la cosa che vieta. Le file entrano dirette,
      // come quando la scena dorme.
      let frazione = 1;
      const c = coda.current;
      const scorre = orologio.current;
      if (ridotto || !scorre) {
        if (c.attiva) versa();
      } else {
        if (!c.attiva) {
          c.attiva = true;
          scorre.azzera();
        }
        const avanzata = scorre.avanza(adesso, c.lunghezza);
        for (let k = 0; k < avanzata.prendi; k += 1) esce(c, entra);
        frazione = avanzata.frazione;
      }

      // L'inseguitore, per tempo trascorso e non per fotogramma: a novanta hertz
      // il passo dev'essere più piccolo che a sessanta, o la stanza respirerebbe
      // a velocità diverse su due schermi diversi. Il primo fotogramma non ha un
      // prima, e un risveglio lascia un buco lungo quanto vuole: da lì il
      // minimo e il tetto.
      const dt = passoTempo(adesso, precedente);
      precedente = adesso;
      tempoScena += dt;
      // L'attacco più svelto del rilascio, come per le barre: il colpo deve
      // arrivare, e poi andarsene con calma.
      const inerzia = colpo.current > seguito ? 120 : 450;
      seguito += (colpo.current - seguito) * (1 - Math.exp(-dt / inerzia));

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
      //
      // Sul tempo **della scena** (`tempoScena`), e con il moto ridotto ferma
      // del tutto, occhio compreso: vedi `posaCamera`.
      const posa = posaCamera(ridotto, seguito, tempoScena / 1000, m);
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
        posa.occhioX,
        posa.occhioY,
        posa.occhioZ,
        posa.bersaglioX,
        posa.bersaglioY,
        posa.bersaglioZ,
      );
      gl.uniformMatrix4fv(u.camera, false, risorse.matrice);
      gl.uniform1f(u.mezzaTela, larghezzaPx * 0.5);
      gl.uniform1f(u.altezzaTela, altezzaPx);
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
      // Dopo l'ultimo fotogramma, e non al posto suo: vedi `passoCiclo`.
      if (giro.addormenta) addormenta();
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
      // La densità si rilegge **qui**, e non solo all'apertura dell'effetto.
      // Lo zoom della WebView (`Ctrl++`, `Ctrl+-`, `Ctrl+0`) cambia il rapporto
      // fra pixel CSS e pixel del dispositivo, cioè `devicePixelRatio`, e la
      // tela disegna nei secondi mentre `contentBoxSize` la misura nei primi:
      // senza questa riga, dopo un `Ctrl++` a 1,5 il riquadro cresce ma il
      // buffer resta calcolato sulla densità di prima, e la scena continua a
      // disegnarsi a risoluzione più bassa finché il componente non si rimonta.
      // Il caso opposto — la densità che cambia senza che la misura in pixel
      // CSS si muova — lo prende l'ascolto della risoluzione, più sotto.
      //
      // Il posto è questo perché lo zoom cambia sempre anche la misura in pixel
      // CSS della tela — è la definizione dello zoom — quindi l'osservatore
      // scatta comunque, e leggere il numero un attimo prima di `applicaMisura`
      // vuol dire che larghezza, altezza e densità entrano insieme nello stesso
      // conto invece che a due tempi.
      densitaSistema = window.devicePixelRatio || 1;
      scala = Math.min(densitaSistema, tettoDensita);
      applicaMisura(false);
      sveglia.current();
    });
    osserva.observe(canvas);

    // La densità che cambia **senza** che la tela cambi misura in pixel CSS:
    // la finestra che passa su un monitor con un'altra scala, la scala di
    // Windows cambiata. Lì l'osservatore di sopra di norma non scatta — il
    // rettangolo che Windows suggerisce conserva la misura logica.
    //
    // Si ascolta `devicePixelRatio` stesso, e non `onScaleChanged` di Tauri,
    // che stava qui: quello porta il fattore del **monitor**, senza lo zoom
    // della finestra, e scritto nella densità dava con lo zoom al 125% su un
    // monitor al 100% una tela all'80% dei pixel, ingrandita dal compositore.
    // `matchMedia` sulla risoluzione di adesso scatta quando il rapporto è già
    // aggiornato, e si riarma ogni volta sul valore nuovo: è l'idioma per
    // ascoltare `devicePixelRatio`, che un evento suo non ce l'ha.
    let mediaDensita: MediaQueryList | null = null;
    const cambiaDensita = () => {
      densitaSistema = window.devicePixelRatio || 1;
      scala = Math.min(densitaSistema, tettoDensita);
      applicaMisura(false);
      sveglia.current();
      ascoltaDensita();
    };
    const ascoltaDensita = () => {
      mediaDensita?.removeEventListener("change", cambiaDensita);
      mediaDensita = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
      mediaDensita.addEventListener("change", cambiaDensita);
    };
    ascoltaDensita();

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
    // La radice del **documento della tela**, dove `tema.ts` scrive tema,
    // densità e movimento. Non `document`: sulla tela dello Studio lo spettro
    // vive dentro un `<iframe>`, col suo documento e la sua radice, e gli
    // attributi che contano sono quelli — la skin in prova, il tema scelto
    // sulla tela — non quelli dello Studio che la contiene.
    const casa = canvas.ownerDocument;
    const radice = new MutationObserver(sporca);
    radice.observe(casa.documentElement, {
      attributes: true,
      attributeFilter: ATTRIBUTI,
    });
    // I fogli nella testa dello stesso documento: il foglio della skin
    // indossata nell'app (`#skin-attiva`, `aspetto.ts`) e quello della bozza
    // sulla tela (`#bozza`, `studio/Tela.tsx`). Un foglio che compare o il suo
    // testo che cambia è il segnale che i token sono da rileggere. Nella testa
    // non succede quasi nient'altro, quindi guardarla tutta costa niente — al
    // contrario di un osservatore sull'intero documento, che vedrebbe ogni
    // riga di ogni lista che si ridisegna.
    const eUnFoglio = (nodo: Node | null) =>
      nodo !== null &&
      (nodo.nodeName === "STYLE" || nodo.parentNode?.nodeName === "STYLE");
    const foglio = new MutationObserver((mutazioni) => {
      for (const m of mutazioni) {
        if (
          eUnFoglio(m.target) ||
          Array.from(m.addedNodes).some(eUnFoglio) ||
          Array.from(m.removedNodes).some(eUnFoglio)
        ) {
          sporca();
          return;
        }
      }
    });
    foglio.observe(casa.head, {
      childList: true,
      subtree: true,
      characterData: true,
    });
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
    // conclusione. I permessi sono questi due e nient'altro, in
    // `capabilities/default.json`: `core:event:allow-listen` (con `unlisten`)
    // per l'ascoltatore, `core:window:allow-is-minimized` per la domanda.
    // Una chiamata nuova a `finestra.*` vuole la sua voce là, o viene rifiutata.
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
    // Il monitor che cambia densità non si ascolta più da qui: vedi
    // `ascoltaDensita`, accanto all'osservatore della misura.

    // ── Perso e ritrovato ───────────────────────────────────────────────────
    //
    // `preventDefault` è ciò che tiene aperta la strada del ripristino: senza,
    // il browser non manda mai `webglcontextrestored`. I manici si azzerano
    // invece di cancellarsi, perché cancellare un manico di un contesto che non
    // c'è più vuol dire chiamare una scheda che non risponde.
    const perduto = (e: Event) => {
      e.preventDefault();
      // Se non torna in tempo si chiude la presa e si dice: vedi
      // `ATTESA_RITORNO_MS`. L'attesa la arma la macchina, e solo a finestra
      // visibile; un'altra perdita prima del ritorno la fa ripartire.
      esegui(macchina.perso(riposoOra()));
      cancelAnimationFrame(fotogramma);
      fotogramma = 0;
      stabili.current = null;
      const r = risoluzione.current;
      if (r) r.texture = null;
      // Dopo aver tolto i manici: la coda entra negli anelli e non nella
      // texture, che non c'è più — e il ripristino la ricaricherà da lì.
      versa();
    };
    // Prima non c'era: la scena restava nera fino a che qualcuno non chiudeva e
    // riapriva la schermata. Adesso si ricostruisce da sola, e la texture si
    // ricarica dagli anelli — che sono sopravvissuti perché stanno in memoria
    // normale. Mezzo minuto di musica attraversa un riavvio del driver.
    const ritrovato = () => {
      // L'attesa la toglie il passo della macchina, qui sotto; ma se le risorse
      // non si ricreano si passa da `scaduto`, e l'attesa va tolta anche lì.
      window.clearTimeout(attesaRitorno);
      attesaRitorno = 0;
      const vecchia = risoluzione.current;
      const nuove = creaStabili(canvas);
      if (!nuove) {
        // Tornato e non ricreato: come se non fosse tornato. La presa si
        // chiude e il recinto lo dice, invece di un vuoto muto.
        console.error("spettro 3D: il contesto non è tornato");
        esegui(macchina.scaduto(riposoOra()));
        return;
      }
      stabili.current = nuove;
      nato = performance.now();
      // La fase dell'oscillazione riparte con la dissolvenza, come prima
      // ripartiva con `nato`: dietro il velo il salto non si vede.
      tempoScena = 0;
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
      // Dove tornare lo decide la macchina: «nascosto» se la finestra è giù,
      // altrimenti «assopito», la presa riaperta e il risveglio.
      esegui(macchina.ritrovato(riposoOra()));
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
      window.clearTimeout(attesaRitorno);
      mediaDensita?.removeEventListener("change", cambiaDensita);
      media.removeEventListener("change", cambiaMoto);
      osserva.disconnect();
      radice.disconnect();
      foglio.disconnect();
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
    // Dipendenze che non sono dipendenze: `rubinetto`, `entra` e `versa` sono
    // stabili per costruzione — `onErrore` sta dietro un riferimento apposta
    // perché `rubinetto` lo sia, e le altre due toccano solo riferimenti.
    // `barre` non è qui e non deve esserci: rimontare questo effetto vorrebbe
    // dire ricompilare due shader per cambiare la larghezza di una texture, che
    // è precisamente il difetto che la separazione in due record ha tolto.
  }, [rubinetto, entra, versa]);

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
    // Le file in coda sono della cascata di prima: larghe quanto le barre di
    // prima, o fatte per anelli di un'altra forma. Si buttano, e l'orologio
    // riparte da fermo con la prima fila della cascata nuova.
    coda.current.lunghezza = 0;
    orologio.current?.azzera();
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
    if (inPausa || !disponibile.current || nascosta.current) return;
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
