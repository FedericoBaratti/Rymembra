/**
 * Spettro3D, quel che la skin dipinge e come la scheda lo disegna: le manopole e i
 * token `canvas.viz.*`, i due shader, la scatola unitaria e la matrice della camera.
 * Spostati invariati da `Spettro3D.tsx`: «questo file», nei commenti, è la scena intera.
 */

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
export interface Manopole {
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
export const SERIE: Manopole = {
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
export function nuoveManopole(): Manopole {
  return { ...SERIE };
}

/** Una voce del registro dei token, come la legge questo file. */
export type Voce =
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
export const TOKEN: readonly Voce[] = [
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
export const MAI = " ";

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
export function tinta(
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

export const VERTICE = `#version 300 es
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
uniform float mezzaTela;
uniform float alta;
uniform float specchio;
uniform float foschia;
uniform float ambiente;
uniform float riflesso;

out float vLivello;
out float vSu;
out float vLuce;
out float vAlfa;
out float vDettaglio;

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

  // La fessura fra due barre, in pixel, e quanto se ne lascia vedere.
  //
  // Una fessura più stretta di un pixel non si può disegnare: la scheda la
  // campiona a caso, una volta sì e una no, e con la camera che oscilla il
  // caso cambia a ogni fotogramma. A 512 barre su uno schermo da 1920 erano
  // fessure da 0,85 pixel davanti e da 0,2 in fondo — un codice a barre che
  // sfarfallava su tutta la scena, ed era la grana che si vedeva. Quindi la
  // fessura resta com'è dove si vede (sopra un pixel e mezzo), si chiude dove
  // non si vede (sotto mezzo pixel), e sfuma in mezzo: le barre restano barre
  // dove sono abbastanza larghe da esserlo, e diventano un terreno continuo
  // dove non lo sono.
  //
  // Il conto si fa sul **centro** della scatola, con valori che sono gli stessi
  // per tutti i suoi vertici: calcolato vertice per vertice, la faccia davanti
  // e quella dietro avrebbero due larghezze diverse, cioè una scatola storta e
  // una crepa fra una fila e l'altra. Quanti pixel fa un'unità di mondo è la
  // derivata della proiezione lungo x — camera[0][0] — divisa per la
  // profondità del centro e riportata sulla mezza tela.
  float wCentro = (camera * vec4(x, 0.0, (zFronte + zFondo) * 0.5, 1.0)).w;
  float passoPx = passoX * camera[0][0] * mezzaTela / max(wCentro, 0.001);
  float fessuraPx = passoPx * (1.0 - spessore);
  float pieno = mix(1.0, spessore, smoothstep(0.75, 1.5, fessuraPx));
  // E quanto la barra è ancora leggibile come una scatola, per la luce qui
  // sotto: piena sopra i quattro pixel, al minimo sotto il pixel e mezzo.
  float dettaglio = smoothstep(1.5, 4.0, passoPx);
  vDettaglio = dettaglio;

  vec3 locale = posizione;
  locale.x *= passoX * pieno;
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
  // La luce di una barra troppo stretta per essere una scatola.
  //
  // Una barra larga due pixel è un pixel di cima, chiaro, e un pixel di
  // fianco, scuro; cento barre di altezze diverse una accanto all'altra sono
  // una scacchiera di puntini che con la camera che oscilla si rimescola a ogni
  // fotogramma. Era la grana del fondo della scena a 512 barre, e non era
  // aliasing di bordo — più campioni non l'avrebbero tolta — ma contrasto
  // alla scala del pixel. Dove una barra è più stretta di quattro pixel, la
  // luce delle sue facce si avvicina alla media di quelle che si vedono (cima
  // e fronte, sei decimi della diretta più l'ambiente), fino a tenerne meno di
  // metà del contrasto sotto il pixel e mezzo. È la prospettiva aerea che la
  // foschia già applica all'alfa, applicata al contrasto: il rilievo resta
  // nell'altezza e nel colore che sale col livello, sparisce il puntinato.
  // Davanti, e con poche barre, vDettaglio vale uno e la luce è quella di
  // sempre.
  float lucemedia = ambiente + (1.0 - ambiente) * 0.6;
  vLuce = mix(lucemedia, vLuce, mix(0.45, 1.0, dettaglio));

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

export const FRAMMENTO = `#version 300 es
precision mediump float;

in float vLivello;
in float vSu;
in float vLuce;
in float vAlfa;
in float vDettaglio;

uniform vec3 primario;
uniform vec3 secondario;
uniform vec3 alone;
uniform float velo;
uniform float cresta;
uniform float altezzaTela;

out vec4 fuori;

void main() {
  // Il colore dice l'altezza: in basso il secondario, in cima il primario. Sono
  // due token della skin, e nient'altro nell'app li legge — l'equalizzatore ha i
  // suoi, che è quel che permette a questa scena di avere una tavolozza sua
  // senza tirarsi dietro le dieci barre di un'altra parte.
  vec3 colore = mix(secondario, primario, clamp(vLivello * 1.3, 0.0, 1.0));
  // La cima si accende con il colore della cresta: è il bagliore che una
  // sfocatura avrebbe dato, pagato zero.
  // La cresta si spegne insieme al dettaglio: sulla cima di una barra larga
  // due pixel è un puntino chiaro in più nella scacchiera (vedi vDettaglio
  // nel vertice).
  colore = mix(colore, alone, smoothstep(0.72, 1.0, vSu) * cresta * vDettaglio);
  colore *= vLuce;

  // La sfumatura verticale che tiene leggibile il testo sopra la scena: zero
  // in cima, un terzo al sedici per cento dell'altezza, piena dal quarantadue
  // in giù. Era una mask-image sulla tela, e costava al compositore una
  // passata in più su tutto lo schermo a ogni fotogramma — misurata, cinque
  // punti di GPU su quarantotto — per moltiplicare un'alfa che qui si
  // moltiplica gratis. Gli stop sono gli stessi, ed è la stessa retta a tratti
  // che il CSS interpolava. gl_FragCoord parte dal basso, la sfumatura
  // dall'alto: da qui il complemento.
  float dallAlto = 1.0 - gl_FragCoord.y / altezzaTela;
  float sfumatura = dallAlto < 0.16
    ? 0.35 * dallAlto / 0.16
    : min(1.0, 0.35 + 0.65 * (dallAlto - 0.16) / 0.26);
  float alfa = vAlfa * velo * clamp(sfumatura, 0.0, 1.0);
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
export function compila(gl: WebGL2RenderingContext, tipo: number, sorgente: string) {
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
export function scatola(): { vertici: Float32Array; indici: Uint16Array } {
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
export function camera(
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
