/**
 * Spettro3D, le risorse e la memoria: il contesto WebGL, la texture, la cascata di
 * anelli, la lettura dei token e la coda delle file in arrivo. Spostati invariati da
 * `Spettro3D.tsx`: «questo file», nei commenti, è la scena intera.
 */

import { PIANI_MAX, PIANI_RIFLESSI, struttura } from "./costi";
import { compila, FRAMMENTO, MAI, nuoveManopole, scatola, tinta, TOKEN, VERTICE, type Manopole, type Voce } from "./shader";

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
  /** Mezza larghezza della tela, in pixel: serve alle fessure. */
  mezzaTela: WebGLUniformLocation | null;
  alta: WebGLUniformLocation | null;
  foschia: WebGLUniformLocation | null;
  ambiente: WebGLUniformLocation | null;
  riflesso: WebGLUniformLocation | null;
  primario: WebGLUniformLocation | null;
  secondario: WebGLUniformLocation | null;
  alone: WebGLUniformLocation | null;
  velo: WebGLUniformLocation | null;
  /** L'altezza della tela, in pixel: serve alla sfumatura verticale. */
  altezzaTela: WebGLUniformLocation | null;
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
export interface RisorseStabili {
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
export interface RisorseRisoluzione {
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

/** Apre il contesto e costruisce tutto quel che non dipende dalle barre. */
export function creaStabili(canvas: HTMLCanvasElement): RisorseStabili | null {
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
    mezzaTela: dove("mezzaTela"),
    alta: dove("alta"),
    foschia: dove("foschia"),
    ambiente: dove("ambiente"),
    riflesso: dove("riflesso"),
    primario: dove("primario"),
    secondario: dove("secondario"),
    alone: dove("alone"),
    velo: dove("velo"),
    altezzaTela: dove("altezzaTela"),
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
export function distruggiStabili(s: RisorseStabili): void {
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
export function creaRisoluzione(
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
export function distruggiRisoluzione(
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
export function spingi(
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
export function rileggi(s: RisorseStabili, barre: number): void {
  for (let i = 0; i < TOKEN.length; i += 1) {
    const voce = TOKEN[i];
    if (!voce) continue;
    const grezzo = s.stile.getPropertyValue(voce.css).trim();
    if (grezzo === s.grezzi[i]) continue;
    s.grezzi[i] = grezzo;
    applica(s, voce, grezzo, barre);
  }
}

/**
 * Quante file la coda tiene al più.
 *
 * Sedici: l'orologio ne scarta oltre il suo tetto — dodici più il margine — e
 * quel che resta è la rete per un ciclo che non gira affatto mentre gli eventi
 * arrivano, cioè una finestra coperta che il browser smette di ridisegnare.
 */
export const CAPIENZA = 16;

/**
 * Le file arrivate che aspettano il loro momento.
 *
 * Un anello di file preallocate, larghe quanto le barre: nessuna allocazione
 * per evento, per la ragione che questo file ripete ovunque. Accanto a ogni
 * fila, il suo colpo — l'energia dei bassi — perché la camera deve spingersi
 * quando la fila **entra**, non quando arriva: altrimenti precederebbe la fila
 * davanti di tutta la profondità della coda.
 */
export interface Coda {
  file: Uint8Array[];
  bassi: Float32Array;
  /** Il posto della più vecchia. */
  testa: number;
  lunghezza: number;
  /**
   * L'orologio la sta svuotando: il ciclo gira, e il moto non è ridotto.
   *
   * Spenta, le file non ci entrano: vanno dritte nella cascata. È lo stato di
   * quando la scena dorme, è nascosta o ha perso il contesto — e in tutti e tre
   * i casi chi la spegne prima la **versa**, così niente resta indietro e
   * niente arriva fuori ordine.
   */
  attiva: boolean;
}

/** Una coda vuota e spenta, per file larghe `barre`. */
export function nuovaCoda(barre: number): Coda {
  return {
    file: Array.from({ length: CAPIENZA }, () => new Uint8Array(barre)),
    bassi: new Float32Array(CAPIENZA),
    testa: 0,
    lunghezza: 0,
    attiva: false,
  };
}

/** Toglie la fila più vecchia e la dà a `entra`. */
export function esce(c: Coda, entra: (fila: Uint8Array, bassi: number) => void): void {
  if (c.lunghezza <= 0) return;
  const posto = c.testa;
  const fila = c.file[posto];
  c.testa = (posto + 1) % CAPIENZA;
  c.lunghezza -= 1;
  if (fila) entra(fila, c.bassi[posto] ?? 0);
}
