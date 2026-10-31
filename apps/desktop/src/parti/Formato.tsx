/**
 * I dati tecnici di quel che suona: «FLAC · 44,1 kHz · 24 bit · Stereo · 1058 kbps».
 *
 * # Cosa dice, e cosa non dice
 *
 * L'**edizione**, non l'uscita. Se la scheda audio sta ricampionando a 48 kHz,
 * questa riga continua a dire 44,1: la domanda a cui risponde è «com'è fatta
 * questa edizione», e l'uscita reale non ci risponde — cambierebbe cambiando
 * cuffia, mentre il file resta quello. È una decisione dell'autore, non una
 * semplificazione. Il ricampionamento lo dice il suggerimento, a chi ci si
 * ferma sopra per saperlo.
 *
 * I numeri sono quelli del decodificatore quando il brano suona (il codec vero
 * dentro un M4A, l'MP3 che si ascolta di un FLAC dell'Internet Archive), e
 * quelli del database nell'istante prima: vedi `FormatoFile` in
 * `ipc/libreria.ts`.
 *
 * # Perché è un componente puro
 *
 * Perché non ha niente da chiedere: il dato arriva dentro `StatoRiproduzione`,
 * e il nucleo lo manda **solo** quando si deve mostrare. Qui non si legge
 * nessuna preferenza, non si monta nessun effetto e non si tiene nessuno stato:
 * la riga c'è se e solo se il dato c'è. Il perché sta nella carta di
 * `StatoRiproduzione.formato`, e in breve è che una copia della preferenza in
 * ogni schermata è una copia che resta indietro quando l'interruttore cambia
 * mentre si guarda un'altra pagina.
 *
 * # Il degrado, che è il caso normale
 *
 * Le quattro colonne sono nullable e una libreria vecchia le ha vuote: si
 * uniscono i pezzi **presenti**, e se non ne resta nessuno non si disegna
 * niente. Mai un « · · », e mai un contenitore vuoto che si prende
 * un'interlinea per non dire niente.
 */
import { numero } from "../formato";
import { t } from "../lingue";
import type { FormatoFile } from "../ipc";

/**
 * Come si chiama una configurazione di canali.
 *
 * I quattro casi che esistono davvero in una libreria di musica hanno un nome,
 * e il nome è più corto e più chiaro del numero: nessuno legge «2 canali» e
 * pensa «stereo» più in fretta di quanto legga «stereo». Gli altri — un 4.0
 * quadrifonico, un 12 di una registrazione ambisonica — restano il numero, che
 * è vero per tutti e non richiede un elenco di parole che non finisce mai.
 */
function canali(quanti: number): string {
  switch (quanti) {
    case 1:
      return t("format.channels.mono");
    case 2:
      return t("format.channels.stereo");
    case 6:
      return t("format.channels.51");
    case 8:
      return t("format.channels.71");
    default:
      return t("format.channels.n", { n: quanti });
  }
}

export function Formato({
  formato,
  classe,
}: {
  /** Quel che è arrivato dal nucleo. `null` quando non si deve disegnare. */
  formato: FormatoFile | null;
  /** Le classi del posto in cui sta: il chiamante sa dov'è, questo no. */
  classe: string;
}) {
  if (!formato) return null;

  const pezzi: string[] = [];
  if (formato.codec) pezzi.push(formato.codec);
  // Positivo e non solo «non nullo»: la colonna è un `INTEGER` senza `CHECK`,
  // il nucleo la legge come `i64` apposta per non fallire su un dato storto, e
  // lo scarto del valore assurdo è qui — nello stesso posto in cui si scarta un
  // `null`, invece di un secondo cammino per il guasto.
  if (formato.sampleRate && formato.sampleRate > 0) {
    pezzi.push(kilohertz(formato.sampleRate));
  }
  // Solo per i formati senza perdita, e lo decide il nucleo: su un MP3 il
  // numero non esiste, e il decodificatore ne riporterebbe uno inventato.
  if (formato.bitsPerSample && formato.bitsPerSample > 0) {
    pezzi.push(t("format.bits", { n: formato.bitsPerSample }));
  }
  if (formato.channels && formato.channels > 0) {
    pezzi.push(canali(formato.channels));
  }
  // Anche sui formati senza perdita, ed è voluto: è la media vera calcolata da
  // `lofty`, non il numero dichiarato nell'intestazione. Su un FLAC dice quanto
  // è densa l'edizione, che è l'unica differenza visibile fra due FLAC dello
  // stesso brano.
  if (formato.bitrate && formato.bitrate > 0) {
    pezzi.push(t("format.kbps", { n: numero(formato.bitrate) }));
  }

  if (pezzi.length === 0) return null;

  const riga = pezzi.join(" · ");
  // Il ricampionamento sta nel suggerimento e non nella riga: vedi in testa.
  const ricampionato =
    formato.sampleRate &&
    formato.sampleRate > 0 &&
    formato.outputRate &&
    formato.outputRate > 0 &&
    formato.outputRate !== formato.sampleRate
      ? t("format.resampled", {
          da: kilohertz(formato.sampleRate),
          a: kilohertz(formato.outputRate),
        })
      : null;
  // Il `title` è la riga intera, come per `.np-meta` accanto: nella colonna
  // larga 348 px fissi il foglio tronca con i puntini, e un formato dal nome
  // lungo — «Ogg Vorbis», «Musepack» — perde il bitrate in coda. Col
  // suggerimento resta leggibile fermandoci sopra.
  return (
    <div
      className={classe}
      title={ricampionato === null ? riga : `${riga}\n${ricampionato}`}
    >
      {riga}
    </div>
  );
}

/**
 * Hertz in «44,1 kHz».
 *
 * Il decimale **solo quando c'è**: «44,1 kHz» ma «48 kHz», non «48,0 kHz».
 * Un decimale fisso scriverebbe uno zero che non dice niente sui tre quarti
 * delle librerie — 48, 96 e 192 sono interi — e la riga è già stretta.
 *
 * La virgola la sceglie la lingua: `numero` è l'unico posto in cui questa
 * applicazione chiama `toLocaleString`, e passarci da qui è ciò che dà «44,1»
 * in italiano e «44.1» in inglese senza scriverlo qui.
 */
function kilohertz(hz: number): string {
  const khz = hz / 1000;
  return t("format.khz", { n: numero(khz, Number.isInteger(khz) ? 0 : 1) });
}
