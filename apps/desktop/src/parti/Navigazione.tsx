/**
 * La colonna di sinistra: dove vado.
 *
 * # La regola che l'ha svuotata
 *
 * Qui dentro c'erano cinque blocchi — playlist, cartelle, importazione,
 * aspetto, libreria — di cui uno solo era navigazione. Gli altri quattro erano
 * configurazione, e stavano nella barra laterale per il motivo per cui ci
 * finisce quasi tutto: era il posto dove c'era spazio.
 *
 * Il costo non era estetico. Una barra che contiene sia «dove sono» sia «cosa
 * sto per cambiare» non ha uno stato attivo che significhi qualcosa: il pulsante
 * «Scansiona» era sempre lì accanto ad Album, alla stessa distanza dal dito, e
 * chi scorreva l'elenco delle playlist si trovava sotto una barra di
 * avanzamento. Ora **a sinistra c'è solo navigazione**, e tutto il resto sta in
 * una pagina che si apre come le altre.
 *
 * In fondo, sotto una riga sottile, ne restano due che non sono destinazioni
 * della libreria: Impostazioni, perché quello *è* il posto dove tutti la cercano,
 * e sopra di lei le donazioni. Stanno separate dalle altre proprio per questo —
 * la riga sottile è il confine fra «dove sono» e le due cose che non sono
 * pagine — e le donazioni non hanno mai lo stato attivo perché non aprono
 * niente qui dentro: portano fuori, nel browser di sistema.
 *
 * Nella barra in fondo alla finestra il tasto non c'è. Non è una dimenticanza:
 * là le voci stanno in fila su una riga sola, e una in più toglierebbe spazio
 * alle sei che servono ad andare da qualche parte per chiedere una cosa che si
 * chiede una volta.
 *
 * # La richiusura è un bottone, **e** una soglia
 *
 * La barra si stringe a `--rail-w` e resta usabile: le icone bastano a
 * riconoscere sei destinazioni. Su un monitor grande è l'utente a decidere,
 * non la larghezza della finestra, perché c'è chi vuole comunque tutta la
 * larghezza per le colonne dell'elenco: il bottone possiede la scelta e questa
 * parte della frase resta vera.
 *
 * Quel che non era vero è «non la larghezza della finestra», detto senza
 * condizioni. Lo stato nasceva dal solo flag della skin e non guardava la
 * finestra mai: a `minWidth: 880` — il minimo vero, non un caso di scuola — la
 * barra aperta si teneva duecentoquaranta pixel e al lettore ne restavano 612,
 * sotto i 728 che la sua griglia chiede. Traboccava, e l'unico rimedio era un
 * bottone che chi non lo conosce non cerca. Sotto i mille pixel, quindi, si
 * stringe da sé.
 *
 * Si stringe e non si riallarga, come la terza colonna di `App.tsx`: riaprirla
 * tornando larghi annullerebbe una richiusura decisa a mano, e il verso che
 * conta è solo quello che evita il traboccamento.
 *
 * Si compone con il `@container` del lettore invece di combatterci: là sotto i
 * 760 pixel di **barra** si ritira il cursore del volume, qui sotto i 1000 di
 * **finestra** si stringe la navigazione. La seconda dà al lettore 172 pixel, e
 * quei 172 sono esattamente quel che lo porta di là dalla soglia del volume —
 * cioè stringere la barra laterale è la via per riavere il cursore, che è il
 * gesto che il commento del foglio descrive già.
 */
import { useEffect, useState } from "react";

/**
 * Sotto questa larghezza di finestra la barra laterale si stringe da sé.
 *
 * Mille e non 880: a 880 il lettore trabocca già, e una soglia che coincide col
 * minimo della finestra arriverebbe sempre un pixel dopo il difetto. Cento pixel
 * di margine sopra il minimo sono il posto in cui la barra si ritira **prima**
 * che qualcosa si rompa, e restano ben sotto i 1100 a cui si chiude la terza
 * colonna: le due soglie non si accavallano, e fra l'una e l'altra c'è una
 * finestra in cui si vede l'effetto di ognuna.
 */
const LARGHEZZA_NAV_LARGA = 1000;

import type { Playlist } from "../ipc";
import { Icona, type NomeIcona } from "./Icone";
import { t } from "../lingue";

/** Dove si può andare. */
export type Vista =
  | "home"
  | "album"
  | "artisti"
  | "brani"
  // Dopo «Brani» e non altrove: le prime quattro sono i modi in cui la libreria
  // si guarda **da dentro** — per disco, per chi suona, per traccia — e questa
  // è il modo in cui si guarda da fuori, cioè come sta sul disco. È la stessa
  // posizione che ha in foobar2000, e la si trova lì per abitudine.
  | "cartelle"
  | "preferiti"
  | "impostazioni"
  // Una destinazione senza voce nella barra: per quasi tutto il tempo non c'è
  // niente da vedere, e una voce che non risponde è una voce che si impara a
  // saltare. Ci si arriva dal toast, dalle impostazioni e dalla tastiera.
  | "importazioni";

/**
 * Le destinazioni della libreria, nell'ordine in cui si guardano.
 *
 * La prima non è un elenco e non ha un conteggio: è la porta d'ingresso, e
 * risponde alla domanda che uno si fa aprendo un lettore — «cosa stavo
 * ascoltando» — che nessuno dei quattro elenchi sa fare.
 *
 * Una funzione perché porta testo: una costante di modulo si fisserebbe sulla
 * lingua che c'era al primo `import`, e cambiare lingua dalle impostazioni
 * lascerebbe la barra in quella di prima fino al riavvio.
 */
function destinazioni(): readonly (readonly [Vista, string, NomeIcona])[] {
  return [
    ["home", t("nav.home"), "i-home"],
    ["album", t("nav.albums"), "i-album"],
    ["artisti", t("nav.artists"), "i-artist"],
    ["brani", t("nav.tracks"), "i-track"],
    // Il conteggio qui è il **numero di radici**, non quello dei brani: è
    // l'unica cosa che si sa senza aprire il pannello — l'albero si costruisce
    // alla prima domanda — e per giunta è quella giusta, perché dice quante
    // cartelle sorvegliate ci sono da guardare.
    ["cartelle", t("nav.folders"), "i-folder"],
    ["preferiti", t("nav.favorites"), "i-heart"],
  ];
}

export function Navigazione({
  vista,
  playlistAperta,
  inLibreria,
  conteggi,
  playlist,
  larga = true,
  inFondo = false,
  onVista,
  onPlaylist,
  onMenuPlaylist,
  onNuovaPlaylist,
  onNuovaSmart,
  onImportaFile,
  onDona,
}: {
  vista: Vista;
  playlistAperta: number | null;
  /** Falso mentre si cerca o si guarda un album: nessuna voce è «quella lì». */
  inLibreria: boolean;
  conteggi: Partial<Record<Vista, number>>;
  playlist: Playlist[];
  /**
   * Comincia larga, con le etichette accanto alle icone.
   *
   * È solo lo stato **iniziale**: il bottone di richiusura continua a
   * possederlo, e la skin dice da dove si parte invece di decidere per sempre.
   */
  larga?: boolean;
  /** In fondo alla finestra invece che di lato: niente marchio, niente playlist. */
  inFondo?: boolean;
  onVista: (vista: Vista) => void;
  onPlaylist: (p: Playlist) => void;
  onMenuPlaylist: (e: React.MouseEvent, p: Playlist) => void;
  onNuovaPlaylist: () => void;
  /** Apre l'editor delle regole per una playlist che si aggiorna da sé. */
  onNuovaSmart: () => void;
  /** Sceglie un file M3U, PLS o XSPF da portare dentro. */
  onImportaFile: () => void;
  /** Apre la pagina delle donazioni nel browser di sistema. */
  onDona: () => void;
}) {
  // La finestra conta già all'avvio: aprirsi larga su una finestra da 880 e
  // stringersi al primo `resize` sarebbe un traboccamento che si vede e poi si
  // corregge da solo, cioè la stessa cosa di prima con un fotogramma in più.
  const [stretta, setStretta] = useState(
    () => !larga || window.innerWidth < LARGHEZZA_NAV_LARGA,
  );

  // Vedi la nota del modulo: si stringe da sé, non si riallarga da sé.
  useEffect(() => {
    const guarda = () => {
      if (window.innerWidth < LARGHEZZA_NAV_LARGA) setStretta(true);
    };
    window.addEventListener("resize", guarda);
    return () => window.removeEventListener("resize", guarda);
  }, []);

  return (
    // Niente `app-shell`: quella classe dice «il contenitore di tutta la
    // finestra», e la porta la zona radice dello scafale. Averla qui e là
    // voleva dire che una skin che la ridipingeva colorava due cose.
    <nav
      className={inFondo ? "navigazione bottom-nav" : "navigazione"}
      /* L'ancora del giro guidato. Sta sul `nav` e non sulle voci: quel che il
         primo passo racconta è «da qui si va da qualche parte», e illuminare
         una pastiglia sola direbbe un'altra cosa. La porta anche la barra in
         fondo, che è la stessa navigazione in un'altra forma: delle due ne
         esiste una alla volta, e `Giro` prende comunque quella disegnata. */
      data-giro="navigazione"
      data-stretta={(!inFondo && stretta) || undefined}
      data-fondo={inFondo || undefined}
      aria-label={t("nav.aria")}
    >
      {/* In fondo il marchio non ci sta e non serve: la finestra è già aperta,
          e chi la guarda sa in quale programma si trova. */}
      <div className="marchio" hidden={inFondo}>
        <span className="anello" aria-hidden="true">
          <Icona nome="i-mark" dim={20} />
        </span>
        <span className="nome">Aether</span>
        <button
          type="button"
          className="tasto icon-btn richiudi"
          aria-label={stretta ? t("nav.expand") : t("nav.collapse")}
          aria-expanded={!stretta}
          title={stretta ? t("nav.expand.short") : t("nav.collapse.short")}
          onClick={() => setStretta((prima) => !prima)}
        >
          <Icona nome={stretta ? "i-chev-r" : "i-chev-l"} dim={15} />
        </button>
      </div>

      <div className="destinazioni">
        {destinazioni().map(([chiave, etichetta, icona]) => {
          const qui = inLibreria && vista === chiave && playlistAperta === null;
          return (
            <button
              key={chiave}
              type="button"
              className="voce nav-pill"
              aria-current={qui ? "page" : undefined}
              data-active={qui || undefined}
              title={stretta ? etichetta : undefined}
              onClick={() => onVista(chiave)}
            >
              <Icona nome={icona} dim={19} />
              <span className="etichetta">{etichetta}</span>
              <span className="conteggio">{conteggi[chiave] ?? "—"}</span>
            </button>
          );
        })}
      </div>

      {/* `hidden` in fondo, quindi senza rettangolo: il passo delle playlist
          si salta da sé sulla barra bassa, dove le playlist non ci sono. */}
      <div className="gruppo" hidden={inFondo} data-giro="playlist">
        <div className="titolo-gruppo">
          <span>{t("playlist.group")}</span>
          {/* Tre tasti e non un menù: sono tre cose che si fanno di rado ma
              che, quando si fanno, si sanno già — e un menù a tendina per tre
              voci è un clic in più per ognuna delle tre. L'ordine è quello
              della frequenza. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("playlist.importFile")}
            title={t("playlist.importFile.title")}
            onClick={onImportaFile}
          >
            <Icona nome="i-import" dim={14} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("playlist.newSmart")}
            title={t("playlist.newSmart.title")}
            onClick={onNuovaSmart}
          >
            <Icona nome="i-settings" dim={14} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("playlist.new")}
            title={t("playlist.new")}
            onClick={onNuovaPlaylist}
          >
            <Icona nome="i-plus" dim={14} />
          </button>
        </div>
        {playlist.length === 0 ? (
          <p className="niente">{t("playlist.none")}</p>
        ) : (
          playlist.map((p) => {
            const qui = playlistAperta === p.id && inLibreria;
            return (
              <button
                key={p.id}
                type="button"
                className="voce nav-pill playlist"
                aria-current={qui ? "page" : undefined}
                data-active={qui || undefined}
                title={
                  p.isSmart
                    ? t("playlist.smart.title", { nome: p.name })
                    : p.name
                }
                onClick={() => onPlaylist(p)}
                onContextMenu={(e) => onMenuPlaylist(e, p)}
              >
                {/* L'ingranaggio era un glifo che su alcuni caratteri di sistema
                    non esisteva e si vedeva come un rettangolo vuoto. */}
                <Icona nome={p.isSmart ? "i-settings" : "i-list"} dim={16} />
                <span className="etichetta">{p.name}</span>
                <span className="conteggio">{p.tracks}</span>
              </button>
            );
          })
        )}
      </div>

      {/* Lo spaziatore spinge Impostazioni in fondo. Non è una svista di
          allineamento: è la posizione in cui la si cerca. */}
      <div className="spinta" hidden={inFondo} />

      <div className="fondo">
        {/* Sopra Impostazioni e non sotto: è l'ordine in cui si guardano — chi
            scende fin qui sta cercando l'ingranaggio, e una voce *dopo* quella
            che si stava cercando è una voce che non si legge mai. Niente
            `aria-current`: non è una pagina, e un lettore di schermo che la
            annunciasse come «pagina corrente» direbbe una cosa falsa. */}
        <button
          type="button"
          className="voce nav-pill dona"
          hidden={inFondo}
          title={stretta ? t("nav.donate") : t("nav.donate.title")}
          onClick={onDona}
        >
          <Icona nome="i-donate" dim={19} />
          <span className="etichetta">{t("nav.donate")}</span>
        </button>
        <button
          type="button"
          className="voce nav-pill"
          data-giro="impostazioni"
          aria-current={vista === "impostazioni" ? "page" : undefined}
          data-active={vista === "impostazioni" || undefined}
          title={stretta ? t("nav.settings") : undefined}
          onClick={() => onVista("impostazioni")}
        >
          <Icona nome="i-settings" dim={19} />
          <span className="etichetta">{t("nav.settings")}</span>
        </button>
      </div>
    </nav>
  );
}
