/**
 * L'incertezza, disegnata: la fiducia in un elenco e il gradino di un abbinamento.
 *
 * # Perché è un file suo
 *
 * Perché i chiamanti sono quattro — la scheda d'anteprima, la riga del piano, la
 * riga della coda e il rapporto — e una parte del disegno copiata in quattro
 * punti è una parte che diverge al primo ritocco. Sono le uniche due forme di
 * questo flusso che dicono «quanto ci si può fidare», e stanno insieme perché
 * chi ne cambia una deve vedere l'altra.
 *
 * # Perché le tacche e la frase, insieme
 *
 * Le tacche si leggono in un colpo d'occhio su cento righe; la frase di
 * `nomeSorgente()` dice cosa vogliono dire. Nessuna delle due basta da sola: la
 * frase non scala a cento righe, e tre segmenti colorati senza una parola sono
 * un'infografica.
 *
 * # Perché non c'è un punteggio
 *
 * Perché non esiste. Nel nucleo l'abbinamento di un'importazione ha quattro
 * gradini con un nome (`abbinamento::Gradino`) e nessuna confidenza numerica —
 * quella vive in `enrich.rs`, che è un altro sottosistema con un'altra domanda.
 * Calcolarne una qui sarebbe una regola di dominio dentro la finestra, cioè una
 * regola che il prossimo cliente dovrà riscrivere.
 */
import { t } from "../lingue";

/** I tre livelli di fiducia in un elenco letto. */
export type Fiducia = "completo" | "parziale" | "nudo";

/**
 * Quanto fidarsi di un elenco, dal livello che ha risposto.
 *
 * Tre e non cinque: i livelli del lettore sono cinque e si leggono nella frase
 * di `nomeSorgente()`, ma la *decisione* che l'utente prende ne ha tre — vado,
 * vado sapendo, non posso.
 */
export function fiduciaDi(sorgente: string | null): Fiducia {
  switch (sorgente) {
    // Jamendo e Audius paginano, e una pagina che manca è un elenco che manca:
    // arrivano completi quasi sempre, ma il quasi va detto.
    case "jamendo":
    case "audius":
      return "parziale";
    case "oembed":
      return "nudo";
    default:
      // `archivio`, `file-playlist`, `archive.org`, e qualunque livello che il
      // nucleo aggiungerà: chi non è dichiarato monco è completo, perché il caso
      // normale non deve costare una riga di manutenzione a ogni sorgente nuova.
      return "completo";
  }
}

/** Il misuratore: la stessa forma nell'anteprima, nella riga e nel rapporto. */
export function Tacche({
  fiducia,
  etichetta,
}: {
  fiducia: Fiducia;
  /**
   * Cosa sente chi ascolta.
   *
   * Senza, le tacche sono `aria-hidden`: quando la frase accanto dice già la
   * stessa cosa, ripeterla è rumore per chi legge con lo schermo.
   */
  etichetta?: string | undefined;
}) {
  const accese = fiducia === "completo" ? 3 : fiducia === "parziale" ? 2 : 1;
  return (
    <span
      className="tacche"
      data-fiducia={fiducia}
      role={etichetta !== undefined ? "img" : undefined}
      aria-label={etichetta}
      aria-hidden={etichetta === undefined ? true : undefined}
    >
      {[0, 1, 2].map((i) => (
        <span key={i} data-accesa={i < accese || undefined} />
      ))}
    </span>
  );
}

/** Con quale gradino un brano è stato ritrovato in libreria. */
export type Gradino =
  "isrc" | "esatta" | "artistaTitolo" | "ripulito" | "nessuno";

/**
 * Come si chiama ognuno, e cosa vuol dire.
 *
 * I due di mezzo sono confermati dalla durata (`abbinamento::TOLLERANZA_MS`, 10
 * s) e vanno in ambra per questo: non perché siano dubbi, ma perché sono decisi
 * da una **seconda prova** invece che da un identificativo.
 */
function parole(): Readonly<Record<Gradino, readonly [string, string]>> {
  return {
    isrc: [t("grade.isrc"), t("grade.isrc.hint")],
    esatta: [t("grade.exact"), t("grade.exact.hint")],
    artistaTitolo: [t("grade.artistTitle"), t("grade.artistTitle.hint")],
    ripulito: [t("grade.stripped"), t("grade.stripped.hint")],
    // Il rifiuto di indovinare è una proprietà del nucleo, e vale la pena
    // mostrarla: l'abbinamento risponde «niente» anche quando il candidato era
    // uno solo, se le durate c'erano e nessuna combaciava.
    nessuno: [t("grade.none"), t("grade.none.hint")],
  };
}

/** Il gradino, in una parola. */
export function PastigliaGradino({ gradino }: { gradino: Gradino }) {
  const [parola, spiega] = parole()[gradino];
  const debole = gradino === "artistaTitolo" || gradino === "ripulito";
  return (
    <span
      className="pastiglia gradino"
      data-livello={
        gradino === "nessuno" ? "nota" : debole ? "avviso" : "certo"
      }
      title={spiega}
    >
      {parola}
    </span>
  );
}

/**
 * Come si chiama l'ufficialità di un canale davanti a chi guarda.
 *
 * `null` per `ignoto`, e non la parola: una pastiglia che dice «ignoto» occupa
 * lo spazio di un'informazione senza esserlo.
 */
export function nomeAffidabilita(affidabilita: string): string | null {
  switch (affidabilita) {
    case "nomeAutore":
      return t("reliability.authorName");
    case "verificata":
      return t("reliability.verified");
    default:
      // `ignota`: la pastiglia **non compare**. Una che dicesse «ignota» su
      // metà delle righe sarebbe una che si impara a non leggere, e lo spazio
      // vuoto dice la stessa cosa senza occuparlo.
      return null;
  }
}

/**
 * Che registrazione è stata presa: studio, dal vivo, o un'altra cosa.
 *
 * # Perché esiste, ed è la pastiglia più importante di questo file
 *
 * Perché i cataloghi liberi non hanno le versioni in studio del catalogo
 * commerciale: hanno concerti, riedizioni, riletture. Il Live Music Archive è
 * fatto **soltanto** di concerti.
 *
 * Prendere un live e metterlo in libreria col nome della versione in studio,
 * senza dirlo, sarebbe scrivere una cosa per un'altra — e sarebbe invisibile
 * finché qualcuno non lo ascolta. Questa pastiglia è il posto in cui non lo si
 * fa.
 *
 * `studio` non produce niente, per la stessa ragione per cui `ignota` non
 * produce niente sopra: è il caso normale, e marcarlo insegnerebbe a ignorare
 * la marcatura.
 */
export function nomeNatura(natura: string): string | null {
  switch (natura) {
    case "dalVivo":
      return t("nature.live");
    case "alternativa":
      return t("nature.alternative");
    default:
      return null;
  }
}

/**
 * Come si chiama una licenza davanti a chi ascolta.
 *
 * # Perché si mostra
 *
 * Perché è la differenza fra un'applicazione che prende musica dove le pare e
 * una che sa cosa sta prendendo. Chi ascolta ha il diritto di saperlo quanto
 * chi pubblica, e un file che entra in libreria senza che nessuno dica sotto
 * che condizioni ci è entrato è un file che fra un anno nessuno saprà se può
 * condividere.
 *
 * Le sigle Creative Commons restano sigle — `cc-by-nc-sa` non si traduce in
 * «attribuzione, non commerciale, condividi allo stesso modo» — perché la sigla
 * è il nome vero, ed è quella che si cerca quando si vuole sapere cosa
 * comporta.
 */
export function nomeLicenza(licenza: string): string {
  switch (licenza) {
    case "pubblicoDominio":
      return t("license.publicDomain");
    case "openMusicLicense":
      return t("license.openMusic");
    case "liberaNonCommerciale":
      return t("license.freeNonCommercial");
    case "tutteRiservate":
      return t("license.allRights");
    case "sconosciuta":
      return t("license.unknown");
    default:
      // Le Creative Commons arrivano come sigla e restano sigla.
      return licenza.toUpperCase();
  }
}

/** Sopra questo scarto la durata va in ambra: `scelta::BANDA_STRETTA_MS`. */
const BANDA_STRETTA_MS = 30_000;

/**
 * Lo scarto di durata fra il file trovato e quel che la fonte dichiara, firmato.
 *
 * Il segno non è pedanteria: `+8 s` è un'introduzione parlata, un applauso o un
 * finale che sfuma; `−8 s` è una versione tagliata. Il valore assoluto direbbe
 * la metà della cosa.
 *
 * Oltre `BANDA_LARGA_MS` (60 s) il candidato non viene scelto affatto, quindi
 * qui non ci arriva: i valori possibili stanno tutti dentro il minuto.
 */
export function Scarto({ scartoMs }: { scartoMs: number }) {
  const secondi = Math.round(scartoMs / 1000);
  return (
    <span
      className="scarto"
      data-livello={Math.abs(scartoMs) > BANDA_STRETTA_MS ? "avviso" : "nota"}
      title={t("drift.title")}
    >
      {secondi > 0 ? `+${secondi}` : secondi}&nbsp;s
    </span>
  );
}

/**
 * Come si chiama un catalogo davanti a chi ascolta.
 *
 * L'etichetta non viaggia più dall'IPC: `Fonte::etichetta()` esiste ancora nel
 * nucleo, ma serve alle cause degli errori e all'attribuzione che finisce nei
 * tag di un file — due posti dove la lingua dell'interfaccia non c'entra. Quel
 * che attraversa il confine è il nome stabile, e il nome di un catalogo è quasi
 * sempre un nome proprio: «Internet Archive» si scrive così in ogni lingua. Le
 * due voci che non lo sono — l'archivio che Spotify consegna e un file di
 * playlist — sono descrizioni, e quelle stanno nel catalogo delle lingue.
 */
export function nomeFonte(fonte: string): string {
  switch (fonte) {
    case "archivio-spotify":
      return t("source.spotifyArchive");
    case "file-playlist":
      return t("source.playlistFile");
    case "internet-archive":
      return "Internet Archive";
    case "jamendo":
      return "Jamendo";
    case "audius":
      return "Audius";
    default:
      return fonte;
  }
}
