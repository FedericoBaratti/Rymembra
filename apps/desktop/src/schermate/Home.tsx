/**
 * La Home: la porta d'ingresso che non c'era.
 *
 * # Perché serviva
 *
 * Rymembra apriva su un elenco. Le quattro destinazioni della libreria — album,
 * artisti, brani, preferiti — sono tutte e quattro un elenco alfabetico, e
 * nessuna risponde alla domanda che uno si fa davvero aprendo un lettore
 * musicale: *cosa stavo ascoltando*. Per riprendere il disco di ieri sera
 * bisognava ricordarsi come si chiamava e andarlo a cercare.
 *
 * # I quattro ripiani, e perché sono questi
 *
 * - **Riprendi** — il brano su cui ci si era fermati, con la posizione. È
 *   l'unica ragione per cui la maggior parte delle aperture avviene.
 * - **Ascoltati di recente** — la memoria corta, quella che serve per tornare
 *   su qualcosa di ieri.
 * - **Aggiunti di recente** — quel che è appena entrato, che è anche quel che
 *   si è più curiosi di sentire. È l'unico ripiano fatto di **dischi**: vedi
 *   sotto.
 * - **Trascurati** — dischi in libreria da mesi e mai toccati. È il ripiano
 *   che solo una libreria locale può avere: nessun servizio in streaming sa
 *   cosa possiedi e non ascolti, perché per lui non possiedi niente.
 *
 * # Perché «aggiunti» sono dischi e gli altri sono brani
 *
 * Perché la musica non entra un brano alla volta: entra una cartella alla
 * volta, e una cartella è un disco. Finché il ripiano chiedeva i dodici brani
 * con la data d'ingresso più alta, mostrava dodici tracce dello stesso album —
 * la stessa copertina dodici volte, per giunta in ordine arbitrario, perché
 * quelle dodici righe condividono la data al secondo. Gli altri tre ripiani
 * rispondono a domande che sono davvero sul brano («cosa stavo ascoltando»,
 * «cosa non ascolto da mesi»), e restano brani.
 *
 * # Perché non c'è scorrimento infinito
 *
 * Perché un ripiano si guarda, non si scorre: sono dodici voci a testa, prese
 * una volta sola entrando. `usePagine` resta per le viste che sono elenchi
 * veri, dove l'elenco è il punto. Qui il punto è il contrario — non doverne
 * leggere uno.
 *
 * # Perché tutto arriva da una chiamata sola
 *
 * `ipc.casa()` è un comando aggregato, come `avvio`. Cinque `invoke` separati
 * vorrebbero dire cinque attraversamenti dell'IPC e cinque prese del lucchetto
 * della libreria per disegnare una schermata sola, all'avvio — cioè nel
 * momento in cui l'applicazione ha già tutto il resto da fare.
 */
import { useContext } from "react";

import { Copertina } from "../Copertina";
import { artistaNoto, brani_, durata, nomeArtista, titoloAlbum } from "../formato";
import type { Album, Brano, Casa, Raccolta } from "../ipc";
import { t } from "../lingue";
import { Icona } from "../parti/Icone";
import { conIndice, suonoAlbum, suonoBrano, useQuelCheSuona } from "../segnali";
import { COPERTINA_IN_VOLO, Presenza, Volo } from "../presenza";
import { StatoVuoto } from "./Libreria";

/**
 * L'intestazione della Home: le prop di `Intestazione`, come `testaLibreria`.
 *
 * Passa da `Intestazione` invece di disegnarsi il proprio markup: occhiello,
 * titolo e sottotitolo hanno già le loro classi e le loro regole, e una
 * seconda intestazione scritta a mano sarebbe una seconda cosa da ridipingere
 * ogni volta che una skin cambia idea su come si scrive un titolo di pagina.
 * Le prop e non un componente che la avvolge per la ragione detta in
 * `testaLibreria`: alla prima lettera la testata diventa quella della ricerca,
 * e con un tipo diverso nello stesso posto il campo perdeva il fuoco.
 *
 * La casella va aggiunta da chi la disegna, perché la Home è una
 * **destinazione**, non una pagina di servizio come Impostazioni: è la
 * schermata su cui l'applicazione apre, quindi è il posto da cui si cerca più
 * spesso. Senza `query` e `onQuery` il campo non veniva disegnato
 * (`Intestazione` lo salta quando manca `onQuery`) e premere «/» lasciava il
 * fuoco sul `<body>`.
 */
export function testaHome() {
  return { occhiello: t("home.eyebrow"), titolo: t("home.title") };
}

/** Un brano, come scheda dentro un ripiano. */
function Scheda({
  brano,
  indice,
  onSuona,
  onMenu,
}: {
  brano: Brano;
  /** La posizione nel ripiano, per la cascata. */
  indice: number;
  onSuona: () => void;
  onMenu: (e: React.MouseEvent, brano: Brano) => void;
}) {
  return (
    <button
      type="button"
      className="scheda-brano track-card"
      {...suonoBrano(useQuelCheSuona(), brano.id)}
      style={conIndice(indice)}
      onClick={onSuona}
      onContextMenu={(e) => onMenu(e, brano)}
      title={`${brano.title} — ${nomeArtista(brano.artist)}`}
    >
      <Copertina
        hash={brano.coverArtHash}
        titolo={titoloAlbum(brano.album)}
        classe="copertina card-cover"
        accanto
      />
      <span className="titolo card-title" dir="auto">
        {brano.title}
      </span>
      <span className="artista card-subtitle" dir="auto">
        {nomeArtista(brano.artist)}
      </span>
    </button>
  );
}

/**
 * Un ripiano di brani, se ha qualcosa dentro.
 *
 * Un ripiano vuoto non si disegna: su una libreria appena scansionata
 * «ascoltati di recente» non ha niente da dire, e una fila di riquadri vuoti
 * sotto un titolo direbbe che manca qualcosa invece che «non ancora».
 *
 * `onSuona` riceve **il ripiano intero** e l'indice, non il brano solo. È la
 * regola scritta nel doc di `suonaDa`: la coda è la lista che si sta guardando,
 * e chi preme la terza copertina si aspetta che poi parta la quarta. Mandando
 * un brano solo, ogni clic sulla Home diventava una fine coda tre minuti dopo.
 */
function Ripiano({
  titolo,
  brani,
  onSuona,
  onMenu,
}: {
  titolo: string;
  brani: Brano[];
  onSuona: (brani: Brano[], indice: number) => void;
  onMenu: (e: React.MouseEvent, brano: Brano) => void;
}) {
  if (brani.length === 0) return null;
  return (
    <section className="casa-ripiano home-shelf">
      <h2 className="casa-titolo home-shelf-title">{titolo}</h2>
      <div className="casa-schede track-grid">
        {brani.map((b, i) => (
          <Scheda
            key={b.id}
            brano={b}
            indice={i}
            onSuona={() => onSuona(brani, i)}
            onMenu={onMenu}
          />
        ))}
      </div>
    </section>
  );
}

/**
 * Un disco, come scheda dentro un ripiano.
 *
 * Lo stesso markup della griglia degli album — `scheda`, `titolo`, `sotto` —
 * invece di uno suo: due schermate che mostrano la stessa cosa devono mostrarla
 * uguale, e una scheda album disegnata a mano qui sarebbe la seconda da
 * ridipingere ogni volta che una skin cambia idea.
 *
 * Un clic **apre** il disco invece di farlo partire: è la differenza fra un
 * brano, che si ascolta, e un album, che prima si guarda. Chi lo vuole in coda
 * ha il menù col destro, che è lo stesso della griglia.
 */
function SchedaAlbum({
  album,
  indice,
  onApri,
  onMenu,
}: {
  album: Album;
  /** La posizione nel ripiano, per la cascata. */
  indice: number;
  onApri: (album: Album) => void;
  onMenu: (e: React.MouseEvent, album: Album) => void;
}) {
  const titolo = titoloAlbum(album.title);
  const inVolo = useContext(Volo);
  return (
    <button
      type="button"
      className="scheda card"
      {...suonoAlbum(useQuelCheSuona(), album.albumKey)}
      style={conIndice(indice)}
      onClick={() => onApri(album)}
      onContextMenu={(e) => onMenu(e, album)}
      title={`${titolo} — ${nomeArtista(album.artist)}`}
    >
      <Presenza
        parte="card-cover"
        nome={album.albumKey === inVolo ? COPERTINA_IN_VOLO : undefined}
        compagna="hero-art"
      >
        <Copertina
          hash={album.coverArtHash}
          titolo={titolo}
          classe="copertina card-cover"
          accanto
        />
      </Presenza>
      <div className="titolo card-title" dir="auto">
        {titolo}
      </div>
      <div className="sotto card-subtitle">
        {album.year ?? ""}
        {album.year && album.totalTracks > 1 ? " · " : ""}
        {album.totalTracks > 1 ? brani_(album.totalTracks) : ""}
      </div>
    </button>
  );
}

/** Un ripiano di dischi, se ha qualcosa dentro. */
function RipianoAlbum({
  titolo,
  album,
  onApri,
  onMenu,
}: {
  titolo: string;
  album: Album[];
  onApri: (album: Album) => void;
  onMenu: (e: React.MouseEvent, album: Album) => void;
}) {
  if (album.length === 0) return null;
  return (
    <section className="casa-ripiano home-shelf">
      <h2 className="casa-titolo home-shelf-title">{titolo}</h2>
      <div className="casa-schede track-grid">
        {album.map((a, i) => (
          <SchedaAlbum
            key={a.albumKey}
            album={a}
            indice={i}
            onApri={onApri}
            onMenu={onMenu}
          />
        ))}
      </div>
    </section>
  );
}

/**
 * Come si chiama una raccolta del lunedì.
 *
 * Il nucleo manda il **materiale** — un genere, un artista, o niente — e la
 * frase si compone qui. Un titolo già scritto in italiano dentro il database
 * sarebbe un titolo italiano anche per chi ha l'interfaccia in inglese, e
 * resterebbe tale per sempre: le raccolte non si rigenerano quando si cambia
 * lingua.
 */
function nomeRaccolta(raccolta: Raccolta): string {
  if (raccolta.genere === "ripescati") return t("home.week.rescued");
  if (raccolta.etichetta !== null && raccolta.etichettaTipo === "genere") {
    return t("home.week.genre", { nome: raccolta.etichetta });
  }
  if (raccolta.etichetta !== null && raccolta.etichettaTipo === "artista") {
    return t("home.week.artist", { nome: raccolta.etichetta });
  }
  // Nessuna maggioranza dentro il gruppo: non c'è un genere o un artista che
  // la descriva da solo. Il numero era la sola cosa che la distingueva dalle
  // altre — «Ancora, 1», «Ancora, 2» — e non diceva niente di cosa ci fosse
  // dentro. I primi due interpreti che si incontrano, in ordine, sono veri e
  // bastano a riconoscerla; il numero resta per una raccolta senza brani, o
  // fatta di soli brani senza il tag dell'artista — che non è un nome e non
  // dice niente, per questo `artistaNoto` lo butta prima di contare.
  const interpreti = [
    ...new Set(raccolta.brani.map((b) => b.artist).filter(artistaNoto)),
  ];
  const [primo, secondo] = interpreti;
  if (primo !== undefined && secondo !== undefined)
    return t("home.week.mixFrom", { primo, secondo });
  if (primo !== undefined) return t("home.week.artist", { nome: primo });
  return t("home.week.mix", { numero: String(raccolta.ordine + 1) });
}

/**
 * Il ripiano del lunedì: la sola cosa nella Home che cambia da sola.
 *
 * # Perché sta in cima
 *
 * Gli altri quattro ripiani rispondono a domande che uno si fa già — «cosa
 * stavo ascoltando», «cosa è appena entrato». Questo risponde a una che non si
 * era fatto, ed è l'unica ragione ricorrente per riaprire il programma: se non
 * è la prima cosa che si vede non esiste.
 *
 * # Cosa sparisce quando le raccolte sono state aperte
 *
 * **L'annuncio**, non il ripiano. Il progetto diceva che spariva tutto, e non
 * regge: le raccolte restano l'unico modo di arrivare a quei brani per i sette
 * giorni successivi, e farle sparire perché le hai guardate una volta vorrebbe
 * dire che aprirle è il modo di perderle. Quindi resta il ripiano — che è
 * contenuto — e se ne va la riga «Nuove questa settimana» col suo pallino, che
 * è l'annuncio. Aperte tutte e quattro, il lunedì diventa un ripiano come gli
 * altri fino al lunedì dopo.
 *
 * # Perché un clic fa partire la raccolta e non la apre
 *
 * Perché una raccolta è una playlist, non un disco: non c'è niente da guardare
 * dentro che non sia già scritto sulla scheda. È la stessa distinzione fra
 * `Scheda` e `SchedaAlbum` qui sopra, dalla parte del brano.
 */
function RipianoSettimana({
  raccolte,
  onApri,
}: {
  raccolte: Raccolta[];
  onApri: (raccolta: Raccolta) => void;
}) {
  if (raccolte.length === 0) return null;
  const nuove = raccolte.filter((r) => !r.aperta);
  const ripescati = raccolte.find((r) => r.genere === "ripescati");

  return (
    <section className="casa-ripiano settimana home-shelf">
      <h2 className="casa-titolo home-shelf-title">
        {t("home.week.title")}
        {nuove.length > 0 && (
          <span className="annuncio">
            {ripescati && !ripescati.aperta
              ? t("home.week.rescuedHint", {
                  quanti: brani_(ripescati.brani.length),
                })
              : t("home.week.fresh")}
          </span>
        )}
      </h2>
      <div className="casa-raccolte track-grid">
        {raccolte.map((raccolta, i) => (
          <button
            type="button"
            key={raccolta.id}
            className="scheda-raccolta collection-card"
            style={conIndice(i)}
            onClick={() => onApri(raccolta)}
            title={nomeRaccolta(raccolta)}
          >
            <Copertina
              hash={raccolta.brani[0]?.coverArtHash ?? null}
              titolo={nomeRaccolta(raccolta)}
              classe="copertina card-cover"
              accanto
            />
            <span className="titolo card-title" dir="auto">
              {nomeRaccolta(raccolta)}
            </span>
            <span className="sotto card-subtitle">
              {/* «1 brano», «12 brani», con i separatori della lingua: era
                  `String()` di un numero, cioè «1 brani». */}
              {brani_(raccolta.brani.length)}
            </span>
            {!raccolta.aperta && (
              <span
                className="pallino"
                /* `role="img"`, perché un `aria-label` su uno `span` nudo
                   non arriva a nessuno: senza ruolo il nodo resta generico e
                   l'etichetta si perde. Come `.lyric-breath` in `Testo.tsx`. */
                role="img"
                aria-label={t("home.week.new")}
              />
            )}
          </button>
        ))}
      </div>
    </section>
  );
}

/**
 * Quanti riquadri finti stanno in un ripiano in attesa.
 *
 * Dodici, cioè quanti ne ha un ripiano vero (`RIPIANO` in `comandi.rs`): un
 * segnaposto che occupa meno spazio del contenuto fa saltare la pagina proprio
 * nel momento in cui i dati arrivano, che è il difetto che i segnaposto
 * esistono per togliere.
 */
const RIQUADRI_FINTI = 12;

/**
 * La Home mentre arriva.
 *
 * Prima qui non c'era niente: `casa === null` disegnava il vuoto, con l'idea
 * che una chiamata di pochi millisecondi non valesse un segnaposto. Ma i
 * millisecondi sono pochi su una libreria di prova — quella query raggruppa
 * `tracks` e ne ordina due viste — e soprattutto l'applicazione ha già i suoi
 * segnaposto ovunque (`GrigliaFinta`, `ElencoFinto` in `App.tsx`): la Home era
 * l'unica schermata che, mentre caricava, diceva «non c'è niente» invece di
 * «sto arrivando». Sulla schermata d'apertura, che è il posto peggiore.
 *
 * Si vede **solo** al primo caricamento. Tornando alla Home i ripiani di prima
 * restano sullo schermo finché non arrivano i nuovi — `casa` non si azzera
 * prima della richiesta — quindi di qui non ci si passa mai due volte.
 *
 * `role="status"` sul contenitore e `aria-hidden` sui riquadri: chi ascolta lo
 * schermo sente «sto caricando» una volta, non venticinque rettangoli.
 */
function CasaFinta() {
  return (
    <div className="casa" role="status" aria-label={t("list.loading")}>
      <section className="casa-ripiano home-shelf">
        <div className="casa-titolo finta skeleton" aria-hidden="true" />
        <div className="casa-riprendi finta" aria-hidden="true">
          <div className="copertina skeleton" />
          <span className="che-cosa">
            <div className="riga-finta skeleton" />
            <div className="riga-finta corta skeleton" />
          </span>
        </div>
      </section>
      {[0, 1].map((ripiano) => (
        <section className="casa-ripiano" key={ripiano}>
          <div className="casa-titolo finta skeleton" aria-hidden="true" />
          <div className="casa-schede track-grid">
            {Array.from({ length: RIQUADRI_FINTI }, (_, i) => (
              <div className="scheda-brano finta" key={i} aria-hidden="true">
                <div className="copertina skeleton" />
                <div className="riga-finta skeleton" />
                <div className="riga-finta corta skeleton" />
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

export function Home({
  casa,
  settimana,
  onSuona,
  onApriRaccolta,
  onRiprendi,
  onMenu,
  onApriAlbum,
  onMenuAlbum,
  onAggiungiCartella,
}: {
  casa: Casa | null;
  /** Le raccolte di questa settimana. Vuoto finché non arrivano. */
  settimana: Raccolta[];
  /** Fa partire un ripiano a partire da uno dei suoi brani. */
  onSuona: (brani: Brano[], indice: number) => void;
  /** Fa partire una raccolta del lunedì e la segna aperta. */
  onApriRaccolta: (raccolta: Raccolta) => void;
  /**
   * Riprende la coda conservata. Senza il punto: lo sceglie il nucleo, che sa
   * se il brano è già aperto — vedi `riprendi` in `riproduzione/mod.rs`.
   */
  onRiprendi: () => void;
  onMenu: (e: React.MouseEvent, brano: Brano) => void;
  onApriAlbum: (album: Album) => void;
  onMenuAlbum: (e: React.MouseEvent, album: Album) => void;
  /** Apre «Aggiungi cartella», per lo stato vuoto. */
  onAggiungiCartella: () => void;
}) {
  const ora = useQuelCheSuona();
  if (casa === null) return <CasaFinta />;

  const vuota =
    casa.riprendi === null &&
    casa.recenti.length === 0 &&
    casa.aggiunti.length === 0 &&
    casa.trascurati.length === 0 &&
    settimana.length === 0;

  // Una rete di sicurezza più che una schermata: chi non ha ancora cartelle o
  // ha la libreria vuota incontra prima le due schermate d'ingresso, che sanno
  // dire cosa fare. Qui ci si arriva solo con una libreria che esiste e non ha
  // niente da mettere su nessuno dei quattro ripiani.
  if (vuota) {
    return (
      <StatoVuoto
        icona="i-home"
        titolo={t("home.empty.title")}
        testo={t("home.empty.hint")}
        azione={{ etichetta: t("settings.roots.add"), fai: onAggiungiCartella }}
      />
    );
  }

  return (
    <div className="casa">
      <RipianoSettimana raccolte={settimana} onApri={onApriRaccolta} />

      {casa.riprendi !== null && (
        <section className="casa-ripiano home-shelf">
          <h2 className="casa-titolo home-shelf-title">{t("home.resume")}</h2>
          <button
            type="button"
            className="casa-riprendi home-resume"
            {...suonoBrano(ora, casa.riprendi.id)}
            onClick={onRiprendi}
            onContextMenu={(e) => {
              if (casa.riprendi) onMenu(e, casa.riprendi);
            }}
          >
            <Copertina
              hash={casa.riprendi.coverArtHash}
              titolo={titoloAlbum(casa.riprendi.album)}
              accanto
            />
            <span className="che-cosa">
              <span className="titolo" dir="auto">
                {casa.riprendi.title}
              </span>
              <span className="artista" dir="auto">
                {nomeArtista(casa.riprendi.artist)}
              </span>
            </span>
            {/* Il segno si mostra solo se vale la pena riprenderlo: sotto i
                dieci secondi «riprendi» e «dall'inizio» sono la stessa cosa, e
                dirlo sarebbe una precisione senza contenuto. */}
            {casa.riprendiMs > 10_000 && (
              <span className="lettura">
                {t("home.resumeAt", { tempo: durata(casa.riprendiMs) })}
              </span>
            )}
            <span className="segno" aria-hidden="true">
              <Icona nome="i-play" dim={18} />
            </span>
          </button>
        </section>
      )}

      <Ripiano
        titolo={t("home.recent")}
        brani={casa.recenti}
        onSuona={onSuona}
        onMenu={onMenu}
      />
      <RipianoAlbum
        titolo={t("home.added")}
        album={casa.aggiunti}
        onApri={onApriAlbum}
        onMenu={onMenuAlbum}
      />
      <Ripiano
        titolo={t("home.neglected")}
        brani={casa.trascurati}
        onSuona={onSuona}
        onMenu={onMenu}
      />
    </div>
  );
}
