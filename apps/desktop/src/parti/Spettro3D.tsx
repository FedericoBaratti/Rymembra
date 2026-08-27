/**
 * La scena dello spettro: le stesse bande, in profondità, e mezzo minuto di
 * memoria.
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
 * volte, con `gl_InstanceID` a dire dove.
 *
 * Le altezze non passano da un vettore riscritto ogni volta: stanno in una
 * texture di un byte per banda, e ogni evento ne carica **due righe in media** —
 * quella nuova del piano zero, più i piani che in quel momento chiudono una
 * fila. Le altre restano dov'erano, e la fila che avanza è un cursore che
 * cambia, non un vettore che scorre. È ciò che rende leggera una cosa che sembra
 * costosa.
 *
 * # Quel che non c'è, e perché
 *
 * Nessuna sfocatura, nessun bagliore in post-produzione, nessuna ombra: sono le
 * tre cose che costano una seconda passata su tutti i pixel dello schermo, e su
 * un portatile si pagano in ventola e in batteria per un lettore che sta
 * suonando in sottofondo. La luce è una lambertiana per faccia calcolata nel
 * vertice, e il bagliore è il colore d'alone della skin sulla cima delle barre.
 *
 * Non c'è nemmeno la faccia di dietro delle scatole: la camera sta sempre a
 * `z > 0` e le file a `z ≤ 0`, quindi quella faccia è già buttata dal culling —
 * tenerla vorrebbe dire quattro vertici trasformati per niente su ognuna delle
 * trentamila. E il riflesso si disegna solo per i primi tre piani: a 0,22 di
 * alfa, il riflesso di venti secondi fa non si vede.
 *
 * Nel silenzio, alla fine, non c'è nemmeno il fotogramma — ma il ciclo si ferma
 * quando **tutta** la scena è silenzio, non dopo un secondo e mezzo: con mezzo
 * minuto di memoria, spegnersi subito congelerebbe la musica di prima a metà
 * strada invece di lasciarla arretrare e svuotare la stanza. Si paga mezzo
 * minuto di fotogrammi dopo ogni pausa, ed è il prezzo di un passato che se ne
 * va davvero.
 *
 * # La presa nel motore la accende questa
 *
 * `ipc.spettro(true)` sta qui, e non altrove: questa tela è l'unico pezzo che
 * guarda le bande, e la presa nel motore deve vivere esattamente quanto lei.
 * Prima la accendeva la striscia a dieci barre sotto la copertina, che non c'è
 * più — la scena si è presa lo schermo.
 *
 * # Se WebGL non c'è
 *
 * Non c'è nemmeno la scena, e non succede altro: lo sfondo resta la tinta della
 * copertina, cioè quel che c'era prima. Un ripiego in 2D sarebbe una seconda
 * implementazione da tenere allineata per una macchina che, se non ha WebGL 2,
 * non ha nemmeno il fiato per trentamila rettangoli disegnati a mano.
 */
import { useEffect, useRef } from "react";

import { ipc, type BandeSpettro } from "../ipc";
import { listen } from "@tauri-apps/api/event";

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

/** Quanto è profonda la stanza, in unità di mondo. */
const PROFONDITA = 5.2;

/**
 * Com'è fatta la cascata, per quante barre ci sono in larghezza.
 *
 * Il costo è il prodotto delle due, e il numero di secondi non è negoziabile:
 * quel che si stringe quando le barre crescono è **quante file per piano**, cioè
 * quanto è fine il dettaglio davanti. A 1024 barre sono quattro file per piano —
 * 132 millesimi a dettaglio pieno — e otto piani per arrivare comunque in fondo
 * ai trenta secondi.
 */
function struttura(barre: number): {
  perPiano: number;
  piani: number;
  file: number;
  copertura: number;
} {
  const perPiano = Math.max(
    4,
    Math.min(8, Math.floor(SCATOLE / (barre * PIANI_MAX))),
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

/** Il movimento del sistema è ridotto. */
function motoRidotto(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Un colore CSS qualunque, in tre numeri fra zero e uno.
 *
 * Passa da una canvas di un pixel e non da un'espressione regolare: un token può
 * essere `#abc`, `rgb(… / .35)`, `oklch(…)` o il nome di un colore, e l'unica
 * cosa che sa leggerli tutti è il parser del browser. Costa un `getImageData` su
 * un pixel, e si paga solo quando la skin cambia davvero.
 */
function tinta(
  pennello: CanvasRenderingContext2D,
  colore: string,
): [number, number, number] {
  pennello.clearRect(0, 0, 1, 1);
  // Due assegnazioni: `fillStyle` ignora in silenzio un colore che non sa
  // leggere, e senza il nero davanti si finirebbe a dipingere con quel che
  // c'era prima invece di accorgersene.
  pennello.fillStyle = "#000";
  pennello.fillStyle = colore;
  pennello.fillRect(0, 0, 1, 1);
  const dati = pennello.getImageData(0, 0, 1, 1).data;
  return [(dati[0] ?? 0) / 255, (dati[1] ?? 0) / 255, (dati[2] ?? 0) / 255];
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
  // è a zero».
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
  vec3 luce = normalize(vec3(-0.35, 0.85, 0.4));
  vec3 n = vec3(normale.x, normale.y * specchio, normale.z);
  vLuce = 0.42 + 0.58 * max(dot(n, luce), 0.0);

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
  // modo in cui il lontano si vede davvero — e i due numeri qui sotto sono
  // l'unica manopola se la stanza dovesse sembrare troppo corta o troppo densa.
  float lontananza = -mondo.z / prof;
  vAlfa = 1.0 - smoothstep(0.55, 0.98, lontananza);
  // Il riflesso è un accenno, non una copia: sopra il pavimento c'è la scena,
  // sotto la sua idea.
  if (specchio < 0.0) vAlfa *= 0.22;

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

out vec4 fuori;

void main() {
  // Il colore dice l'altezza: in basso il secondario, in cima il primario. Sono
  // gli stessi due token che l'equalizzatore usa per le sue bande, e una skin
  // che li cambia cambia tutte e due le cose insieme.
  vec3 colore = mix(secondario, primario, clamp(vLivello * 1.3, 0.0, 1.0));
  // La cima si accende con il colore d'alone: è il bagliore che una sfocatura
  // avrebbe dato, pagato zero.
  colore = mix(colore, alone, smoothstep(0.72, 1.0, vSu) * 0.55);
  colore *= vLuce;

  float alfa = vAlfa * velo;
  if (alfa <= 0.004) discard;
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
 * Proiezione per vista, già moltiplicate.
 *
 * Una matrice sola e non due: è l'unica cosa che lo shader deve sapere della
 * camera, e farne il prodotto qui costa sedici moltiplicazioni al fotogramma
 * invece di trentamila.
 */
function camera(
  aspetto: number,
  occhio: [number, number, number],
  mira: [number, number, number],
): Float32Array {
  const vicino = 0.1;
  const lontano = 24;
  const f = 1 / Math.tan((52 * Math.PI) / 180 / 2);

  // La vista, cioè una base ortonormale attorno alla direzione dello sguardo.
  const [ox, oy, oz] = occhio;
  const verso = [ox - mira[0], oy - mira[1], oz - mira[2]];
  const lunghezza =
    Math.hypot(verso[0] ?? 0, verso[1] ?? 0, verso[2] ?? 0) || 1;
  const zx = (verso[0] ?? 0) / lunghezza;
  const zy = (verso[1] ?? 0) / lunghezza;
  const zz = (verso[2] ?? 0) / lunghezza;
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
  return new Float32Array([
    a * xx,
    f * yx,
    c * zx,
    -zx,
    a * xy,
    f * yy,
    c * zy,
    -zy,
    a * xz,
    f * yz,
    c * zz,
    -zz,
    a * tx,
    f * ty,
    c * tz + d,
    -tz,
  ]);
}

export function Spettro3D({
  barre,
  onErrore,
}: {
  /** Quante barre in larghezza, cioè quante bande fini chiede il nucleo. */
  barre: number;
  onErrore: (e: unknown) => void;
}) {
  const tela = useRef<HTMLCanvasElement>(null);
  /** L'ultima fila arrivata, fuori da React: la legge solo il disegno. */
  const arrivo = useRef<Uint8Array>(new Uint8Array(0));
  /** Quando è arrivata, per far scorrere le file fra un evento e l'altro. */
  const quando = useRef(0);
  /**
   * Quanto passa fra un evento e l'altro, misurato.
   *
   * Non `PASSO_MS`, che è quanto il filo di là **dorme**: il periodo vero è quel
   * sonno più il lavoro, quindi dividere per la costante fa saturare la frazione
   * un pelo prima di ogni fila e poi ripartire da zero — un micro-tremolio a
   * trenta hertz che si vede e non si sa dire.
   */
  const intervallo = useRef(PASSO_MS);
  /** C'è una fila nuova da caricare. */
  const fresca = useRef(false);
  /** Da quanti eventi di fila non arriva niente che non sia silenzio. */
  const silenzio = useRef(0);

  // La presa nel motore la accende questa, che è l'unico pezzo che guarda le
  // bande: vive esattamente quanto la tela. Senza lo spegnimento la callback
  // audio continuerebbe a riempire un anello che nessuno svuota, e il filo di là
  // a mandare trenta eventi al secondo a una schermata chiusa.
  useEffect(() => {
    ipc.spettro(true).catch(onErrore);
    const promessa = listen<BandeSpettro>("riproduzione:spettro", (evento) => {
      const fini = evento.payload.fini;
      // Un vettore riusato e non uno nuovo: trenta allocazioni al secondo da
      // mille byte sono trenta occasioni al secondo perché il raccoglitore
      // fermi il filo che disegna.
      if (arrivo.current.length !== fini.length) {
        arrivo.current = new Uint8Array(fini.length);
      }
      let massimo = 0;
      for (let i = 0; i < fini.length; i += 1) {
        const livello = fini[i] ?? 0;
        arrivo.current[i] = livello;
        if (livello > massimo) massimo = livello;
      }
      // Quattro su 255 è il fruscio della quantizzazione, non un suono.
      silenzio.current = massimo > 4 ? 0 : silenzio.current + 1;
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
      fresca.current = true;
    });
    return () => {
      void promessa.then((stop) => stop());
      ipc.spettro(false).catch(onErrore);
    };
  }, [onErrore]);

  useEffect(() => {
    const canvas = tela.current;
    if (!canvas) return;
    const gl = canvas.getContext("webgl2", {
      alpha: true,
      antialias: true,
      depth: true,
      // Su un portatile con due schede, questa scena non è il motivo per
      // svegliare quella grossa.
      powerPreference: "low-power",
      premultipliedAlpha: true,
    });
    if (!gl) return;

    const programma = gl.createProgram();
    const vertice = compila(gl, gl.VERTEX_SHADER, VERTICE);
    const frammento = compila(gl, gl.FRAGMENT_SHADER, FRAMMENTO);
    if (!programma || !vertice || !frammento) return;
    gl.attachShader(programma, vertice);
    gl.attachShader(programma, frammento);
    gl.linkProgram(programma);
    if (!gl.getProgramParameter(programma, gl.LINK_STATUS)) {
      console.error("spettro 3D:", gl.getProgramInfoLog(programma));
      return;
    }
    gl.useProgram(programma);

    const { vertici, indici } = scatola();
    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, vertici, gl.STATIC_DRAW);
    const posizione = gl.getAttribLocation(programma, "posizione");
    const normale = gl.getAttribLocation(programma, "normale");
    gl.enableVertexAttribArray(posizione);
    gl.vertexAttribPointer(posizione, 3, gl.FLOAT, false, 24, 0);
    gl.enableVertexAttribArray(normale);
    gl.vertexAttribPointer(normale, 3, gl.FLOAT, false, 24, 12);
    const elementi = gl.createBuffer();
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, elementi);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indici, gl.STATIC_DRAW);

    const { perPiano, piani, file, copertura } = struttura(barre);
    // Un byte per banda, com'è arrivato dal filo: fra l'evento e la texture non
    // c'è nessuna conversione. Il piano ℓ occupa le righe da ℓ·perPiano, quindi
    // la cascata intera sta nella stessa texture che servirebbe a una fila per
    // evento — la memoria non cresce con i secondi, cresce con le file disegnate.
    const texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.R8, barre, file);
    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      0,
      0,
      barre,
      file,
      gl.RED,
      gl.UNSIGNED_BYTE,
      new Uint8Array(barre * file),
    );
    // `NEAREST` perché il vertice legge con `texelFetch`: nessun filtro entra in
    // gioco, e chiederne uno lineare vorrebbe dire un'estensione in più da
    // sperare che ci sia.
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    const dove = (nome: string) => gl.getUniformLocation(programma, nome);
    const uCamera = dove("camera");
    const uCursori = dove("cursori");
    const uFase = dove("fase");
    const uFrazione = dove("frazione");
    const uSpecchio = dove("specchio");
    const uPrimario = dove("primario");
    const uSecondario = dove("secondario");
    const uAlone = dove("alone");
    const uVelo = dove("velo");
    gl.uniform1i(dove("altezze"), 0);
    gl.uniform1i(dove("colonne"), barre);
    gl.uniform1i(dove("perPiano"), perPiano);
    gl.uniform1i(dove("piani"), piani);
    gl.uniform1f(dove("prof"), PROFONDITA);
    gl.uniform1f(dove("larghezza"), 4.6);
    // Più barre, più piene: sotto le 256 la fessura fra una e l'altra è quel che
    // le rende barre, sopra sarebbe più larga della barra stessa.
    gl.uniform1f(dove("spessore"), barre > 256 ? 0.88 : 0.7);
    gl.uniform1f(dove("alta"), 0.62);

    // Nessuna miscelazione, e non è una dimenticanza: la profondità basta a dire
    // chi sta davanti, e le barre non sono trasparenti fra loro. L'alfa che esce
    // dal frammento serve al **compositore del browser**, che posa la tela sopra
    // la tinta della copertina. Accendere la miscelazione avrebbe voluto dire
    // ordinare trentamila scatole dal fondo a ogni fotogramma, che è precisamente
    // il lavoro che una scheda video fa da sola con il buffer di profondità.
    gl.enable(gl.DEPTH_TEST);
    gl.enable(gl.CULL_FACE);
    gl.clearColor(0, 0, 0, 0);

    // ── La cascata, dal lato della CPU ──────────────────────────────────────
    //
    // Per piano: una copia dell'anello — serve per sapere **cosa cade dal
    // fondo**, che è l'unica cosa che la texture non saprebbe dire senza
    // rileggerla dalla scheda — un accumulatore e un contatore. Sono
    // `barre · perPiano · piani` byte, cinquanta chilobyte nel caso peggiore.
    const anelli: Uint8Array[] = [];
    const parziali: Uint8Array[] = [];
    const cursori = new Int32Array(PIANI_MAX);
    const conta = new Int32Array(PIANI_MAX);
    for (let p = 0; p < piani; p += 1) {
      anelli.push(new Uint8Array(barre * perPiano));
      parziali.push(new Uint8Array(barre));
    }
    /** La fase: gli eventi da quando i piani erano tutti allineati. */
    let fase = 0;

    /**
     * Il massimo di due file del piano di sotto diventa una fila di questo.
     *
     * Il massimo e non la media: due file riassunte devono dire «qui c'è stato
     * un colpo», e la media di un colpo e di un silenzio è un mezzo colpo che
     * non è mai suonato.
     */
    const accumula = (piano: number, fila: Uint8Array) => {
      const parziale = parziali[piano];
      if (!parziale) return;
      for (let i = 0; i < barre; i += 1) {
        const valore = fila[i] ?? 0;
        if (valore > (parziale[i] ?? 0)) parziale[i] = valore;
      }
      conta[piano] = (conta[piano] ?? 0) + 1;
      if ((conta[piano] ?? 0) >= 2) {
        spingi(piano, parziale);
        parziale.fill(0);
        conta[piano] = 0;
      }
    };

    /**
     * Mette una fila in cima a un piano, e manda su quella che cade dal fondo.
     *
     * Della texture si scrive **una riga sola**: quella nuova. Le altre restano
     * dov'erano, e la fila che avanza è il cursore che cambia.
     *
     * Chiuse in due `const` e non dichiarate come funzioni, queste due, e non è
     * uno stile: una dichiarazione risale in cima allo scope, e TypeScript non
     * può sapere che nessuno la chiama prima del controllo su `gl` — dentro
     * vedrebbe un contesto che può essere nullo.
     */
    const spingi = (piano: number, fila: Uint8Array) => {
      const anello = anelli[piano];
      if (!anello) return;
      const cursore = ((cursori[piano] ?? 0) + 1) % perPiano;
      const base = cursore * barre;
      // Prima di sovrascriverlo: quel che sta in questo posto è la fila che esce
      // dalla finestra del piano, e il piano di sopra la sta aspettando. Dopo
      // l'ultimo piano invece si perde, ed è giusto — è passato più vecchio
      // della memoria.
      if (piano + 1 < piani) {
        accumula(piano + 1, anello.subarray(base, base + barre));
      }
      anello.set(fila, base);
      cursori[piano] = cursore;
      gl.texSubImage2D(
        gl.TEXTURE_2D,
        0,
        0,
        piano * perPiano + cursore,
        barre,
        1,
        gl.RED,
        gl.UNSIGNED_BYTE,
        fila,
      );
    };

    // Il pennello di un pixel per leggere i token: vedi `tinta`.
    const pipetta = document.createElement("canvas");
    pipetta.width = 1;
    pipetta.height = 1;
    const pennello = pipetta.getContext("2d", { willReadFrequently: true });
    // Viva e non una fotografia: `getComputedStyle` torna una dichiarazione
    // legata all'elemento, quindi rileggerla dentro il ciclo basta perché una
    // skin nuova arrivi qui senza rimontare niente.
    const stile = getComputedStyle(canvas);
    let ultimiColori = "";

    let fotogramma = 0;
    let vivo = true;
    // Il colpo: quanta energia c'è nelle bande basse, con la sua inerzia. È
    // l'unica cosa che muove la camera, ed è la ragione per cui lo sfondo sembra
    // andare a tempo — perché ci va davvero.
    let colpo = 0;
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
    /** Quando è stato disegnato il fotogramma di prima, per l'inseguitore. */
    let precedente = 0;
    let nato = performance.now();
    // Il riflesso solo per i primi piani: più in fondo, a 0,22 di alfa, non si
    // vede. Basta un `instanceCount` più piccolo, perché `fila = id / colonne`
    // mette già le file vicine per prime.
    const istanzeRiflesso = barre * Math.min(file, perPiano * PIANI_RIFLESSI);

    const disegna = (adesso: number) => {
      if (!vivo) return;
      fotogramma = requestAnimationFrame(disegna);

      // Silenzio da quanto è profonda la memoria: il passato è uscito dalla
      // scena, non c'è più niente da far scorrere, e ridisegnare sessanta volte
      // al secondo una stanza vuota è il modo più sicuro di far girare la
      // ventola durante una pausa. Quel che c'è resta finché non torna il suono.
      if (silenzio.current > copertura) {
        fresca.current = false;
        return;
      }

      const scala = Math.min(window.devicePixelRatio || 1, 1.5);
      const larghezzaPx = Math.round(canvas.clientWidth * scala);
      const altezzaPx = Math.round(canvas.clientHeight * scala);
      if (larghezzaPx < 2 || altezzaPx < 2) return;
      if (canvas.width !== larghezzaPx || canvas.height !== altezzaPx) {
        canvas.width = larghezzaPx;
        canvas.height = altezzaPx;
        gl.viewport(0, 0, larghezzaPx, altezzaPx);
      }

      // I colori si rileggono a ogni fotogramma **come stringhe**, che costa
      // niente, e si riconvertono solo quando sono cambiate davvero: è così che
      // una skin nuova arriva qui senza pagare un `getImageData` sessanta volte
      // al secondo.
      const primario = stile.getPropertyValue("--viz-primary").trim();
      const secondario =
        stile.getPropertyValue("--viz-secondary").trim() || primario;
      const alone = stile.getPropertyValue("--accent-glow").trim() || primario;
      const firma = `${primario}|${secondario}|${alone}`;
      if (pennello && firma !== ultimiColori) {
        ultimiColori = firma;
        gl.uniform3fv(uPrimario, tinta(pennello, primario));
        gl.uniform3fv(uSecondario, tinta(pennello, secondario));
        gl.uniform3fv(uAlone, tinta(pennello, alone));
      }

      if (fresca.current) {
        fresca.current = false;
        const dati = arrivo.current;
        // Fra il cambio di risoluzione e la prima fila della misura nuova
        // passano trenta millesimi: in mezzo arriva una fila della lunghezza di
        // prima, e scriverla in una texture larga un'altra cosa vorrebbe dire
        // disegnare una riga di rumore.
        if (dati.length === barre) {
          gl.bindTexture(gl.TEXTURE_2D, texture);
          spingi(0, dati);
          // La fase resta dentro un giro completo della cascata: `2^piani` è il
          // minimo comune multiplo di tutti i passi, quindi il resto per uno
          // qualunque di essi non cambia, e il numero non cresce per sempre.
          fase = (fase + 1) % (1 << piani);
          // Le prime bande sono i bassi, e i bassi sono il tempo. Un ottavo delle
          // barre, qualunque sia il loro numero: con 64 sono le otto sotto i 100
          // Hz, con 1024 sono le prime centoventotto — la stessa fascia di
          // frequenze, non lo stesso numero di barre.
          const quante = Math.max(1, dati.length >> 3);
          let bassi = 0;
          for (let i = 0; i < quante; i += 1) bassi += (dati[i] ?? 0) / 255;
          bassi /= quante;
          // Sale di scatto e scende piano, come le barre: un colpo di grancassa
          // deve spingere la camera, non farla vibrare.
          colpo = bassi > colpo ? bassi : colpo + (bassi - colpo) * 0.08;
        }
      }

      const ridotto = motoRidotto();
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
      // prima, e un salto di scheda lascia un buco lungo quanto vuole: da lì il
      // minimo e il tetto.
      const dt = precedente > 0 ? Math.min(adesso - precedente, 100) : 16.7;
      precedente = adesso;
      // L'attacco più svelto del rilascio, come per le barre: il colpo deve
      // arrivare, e poi andarsene con calma.
      const inerzia = colpo > seguito ? 120 : 450;
      seguito += (colpo - seguito) * (1 - Math.exp(-dt / inerzia));

      const tempo = (adesso - nato) / 1000;
      // L'oscillazione: lentissima, e la sua ampiezza cresce col colpo. Meno di
      // prima — un quinto invece di mezzo radiante — perché quel che deve
      // portare il movimento è il girare lento, e il colpo lo accompagna: con
      // l'ampiezza vecchia il termine del colpo era tre volte quello di base, e
      // la scena non oscillava, sbandava.
      const oscilla = ridotto
        ? 0
        : Math.sin(tempo * 0.21) * (0.16 + seguito * 0.22);
      const avvicina = ridotto ? 0 : seguito * 0.06;
      // Un po' più in alto di quando la memoria era un secondo: con mezzo minuto
      // di terreno davanti, uno sguardo raso terra lo nasconderebbe tutto dietro
      // la prima cresta. Non tanto da diventare una mappa vista dall'alto — la
      // scena resta una stanza in cui si sta, non un grafico da leggere.
      const occhio: [number, number, number] = [
        Math.sin(oscilla) * 1.5,
        0.72 + seguito * 0.03,
        1.45 - avvicina,
      ];
      const mira: [number, number, number] = [
        Math.sin(oscilla) * 0.4,
        0.02,
        -2.2,
      ];
      gl.uniformMatrix4fv(
        uCamera,
        false,
        camera(larghezzaPx / altezzaPx, occhio, mira),
      );
      // Mezzo secondo di dissolvenza all'apertura: la scena arriva dietro una
      // copertina che sta già ferma sullo schermo, e comparire di colpo la
      // farebbe sembrare un guasto invece di una cosa che si accende.
      gl.uniform1f(uVelo, Math.min(1, (adesso - nato) / 500));

      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      gl.bindVertexArray(vao);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.uniform1iv(uCursori, cursori);
      gl.uniform1i(uFase, fase);
      gl.uniform1f(uFrazione, frazione);

      // Il riflesso per primo: sta sotto il pavimento, quindi non copre niente.
      // Le facce si rovesciano perché lo specchio inverte il verso, e senza
      // questa riga la scheda butterebbe via proprio quelle che si vedono.
      gl.frontFace(gl.CW);
      gl.uniform1f(uSpecchio, -1);
      gl.drawElementsInstanced(
        gl.TRIANGLES,
        indici.length,
        gl.UNSIGNED_SHORT,
        0,
        istanzeRiflesso,
      );
      gl.frontFace(gl.CCW);
      gl.uniform1f(uSpecchio, 1);
      gl.drawElementsInstanced(
        gl.TRIANGLES,
        indici.length,
        gl.UNSIGNED_SHORT,
        0,
        barre * file,
      );
    };

    // Il contesto può andarsene — la scheda si riavvia, il portatile passa
    // all'altra — e senza questo il ciclo continuerebbe a chiamare una scheda
    // che non risponde più. `preventDefault` è ciò che tiene aperta la strada
    // del ripristino; rimetterla in piedi la fa React alla riapertura della
    // schermata, che è il momento in cui qualcuno la sta guardando davvero.
    const perso = (e: Event) => {
      e.preventDefault();
      vivo = false;
      cancelAnimationFrame(fotogramma);
    };
    canvas.addEventListener("webglcontextlost", perso);

    nato = performance.now();
    fotogramma = requestAnimationFrame(disegna);
    return () => {
      vivo = false;
      cancelAnimationFrame(fotogramma);
      canvas.removeEventListener("webglcontextlost", perso);
      gl.deleteTexture(texture);
      gl.deleteBuffer(buffer);
      gl.deleteBuffer(elementi);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(programma);
      gl.deleteShader(vertice);
      gl.deleteShader(frammento);
    };
  }, [barre]);

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
