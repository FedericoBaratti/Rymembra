//! Le domande della finestra alla libreria: elenchi di brani, album e artisti, cronologia,
//! dati tecnici del file e ricerca.

use super::*;

/// Un brano come lo mostra una lista.
///
/// Non tutte le colonne di `tracks`: testo, commento e impronte non servono a
/// disegnare una riga, e trascinarseli dietro vorrebbe dire spedire qualche
/// megabyte di testi di canzoni verso la finestra a ogni scorrimento.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSummary {
    /// L'identificativo della riga.
    pub id: i64,
    /// Il percorso. `None` per un brano di catalogo, che un file non ce l'ha.
    pub path: Option<String>,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// La chiave dell'album, per aprirne la scheda.
    pub album_key: Option<String>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// L'anno.
    pub year: Option<i64>,
    /// L'impronta della copertina.
    pub cover_art_hash: Option<String>,
    /// Quante volte è stato ascoltato.
    pub play_count: i64,
    /// È fra i preferiti.
    pub liked: bool,
    /// Il voto, 0–5.
    pub rating: i64,
    /// Da quale catalogo arriva, col nome stabile di
    /// [`rymembra_domain::esterno::Fonte`]. `None` per un file sul disco.
    ///
    /// È il campo con cui la finestra decide se mostrare la pastiglia «in
    /// streaming» e se nascondere i gesti che vogliono un file — mostrare nella
    /// cartella, riscrivere i tag, spostare. Uno solo, e non un booleano
    /// accanto a un nome: due campi che dicono la stessa cosa sono due campi
    /// che un giorno la diranno diversa.
    pub fonte: Option<String>,
    /// La pagina pubblica del brano, da mettere accanto a chi lo ascolta.
    ///
    /// Non è un ornamento: per certe Creative Commons e per i termini di Audius
    /// il rimando visibile è una condizione d'uso, e questo è il campo che la
    /// soddisfa.
    pub fonte_pagina: Option<String>,
    /// Sotto che licenza sta, col nome stabile di
    /// [`rymembra_domain::esterno::Licenza`].
    pub licenza: Option<String>,
}

/// Le colonne di [`TrackSummary`], nell'ordine in cui le legge `track_from_row`.
pub(crate) const COLONNE_BRANO: &str = "t.id, t.path, t.title, t.artist, t.album, t.album_key,
     t.track_number, t.disc_number, t.duration_ms, t.year, t.cover_art_hash,
     t.play_count, t.liked, t.rating, t.source_service, t.fonte_pagina, t.licenza";

/// Quante sono le colonne di [`COLONNE_BRANO`].
///
/// Esiste perché una query le mette in testa e ne aggiunge altre dopo — la
/// cronologia lo fa — e l'indice della prima colonna aggiunta dipende da
/// questo numero. Scritto una volta sola, aggiungere una colonna al riepilogo
/// resta un cambiamento in un posto solo; scritto a mano in ogni chiamante,
/// sarebbe il genere di disallineamento che si manifesta come «l'ora
/// dell'ascolto è diventata il voto».
pub(crate) const COLONNE_BRANO_QUANTE: usize = 17;

pub(crate) fn track_from_row(row: &Row<'_>) -> rusqlite::Result<TrackSummary> {
    Ok(TrackSummary {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        album_key: row.get(5)?,
        track_number: row.get(6)?,
        disc_number: row.get(7)?,
        duration_ms: row.get(8)?,
        year: row.get(9)?,
        cover_art_hash: row.get(10)?,
        play_count: row.get(11)?,
        liked: row.get::<_, i64>(12)? != 0,
        rating: row.get(13)?,
        fonte: row.get(14)?,
        fonte_pagina: row.get(15)?,
        licenza: row.get(16)?,
    })
}

/// Un album come lo mostra una griglia.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSummary {
    /// La chiave canonica.
    pub album_key: String,
    /// Il titolo.
    pub title: String,
    /// L'artista.
    pub artist: String,
    /// L'anno.
    pub year: Option<i64>,
    /// Il genere dominante.
    pub genre: Option<String>,
    /// Quanti brani.
    pub total_tracks: i64,
    /// L'impronta della copertina.
    pub cover_art_hash: Option<String>,
}

/// Le colonne di [`AlbumSummary`], nell'ordine in cui le legge [`album_from_row`].
///
/// Qualificate con `a.`: una delle query che le usano ha un'altra tabella
/// accanto, e `album_key` da solo diventerebbe ambiguo.
pub(crate) const COLONNE_ALBUM: &str =
    "a.album_key, a.title, a.artist, a.year, a.genre, a.total_tracks, a.cover_art_hash";

/// Un album, da una riga di [`COLONNE_ALBUM`].
///
/// Era scritta tre volte identica — la griglia, gli album di un artista, i
/// dischi entrati per ultimi. Tre copie di sette `row.get` numerati sono tre
/// occasioni di sfasare un indice, e uno sfasamento fra `year` e `genre` non è
/// un errore di compilazione: è una griglia che scrive «2019» dove va il genere.
pub(crate) fn album_from_row(row: &Row<'_>) -> rusqlite::Result<AlbumSummary> {
    Ok(AlbumSummary {
        album_key: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        year: row.get(3)?,
        genre: row.get(4)?,
        total_tracks: row.get(5)?,
        cover_art_hash: row.get(6)?,
    })
}

/// Quanto c'è in libreria.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Brani.
    pub tracks: i64,
    /// Album.
    pub albums: i64,
    /// Artisti.
    pub artists: i64,
    /// Brani fra i preferiti.
    pub liked: i64,
    /// Durata totale in millisecondi.
    pub duration_ms: i64,
}

/// I numeri della libreria, in una query sola.
pub fn counts(connection: &Connection) -> Result<Counts, AppError> {
    connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM tracks),
                    (SELECT COUNT(*) FROM albums),
                    (SELECT COUNT(*) FROM artists),
                    (SELECT COUNT(*) FROM tracks WHERE liked = 1),
                    (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks)",
            [],
            |row| {
                Ok(Counts {
                    tracks: row.get(0)?,
                    albums: row.get(1)?,
                    artists: row.get(2)?,
                    liked: row.get(3)?,
                    duration_ms: row.get(4)?,
                })
            },
        )
        .map_err(|err| db_error("numeri della libreria", &err))
}

/// Come ordinare un elenco di brani.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackOrder {
    /// Artista, poi album, poi disco e traccia: l'ordine di uno scaffale.
    Shelf,
    /// I più recenti in cima.
    RecentlyAdded,
    /// I più ascoltati in cima.
    MostPlayed,
    /// Gli ascoltati più di recente in cima, i mai ascoltati in fondo.
    ///
    /// Diverso da `MostPlayed`, che ordina per **quante volte**: qui conta
    /// **quando**, ed è la domanda a cui risponde la Home. Un brano suonato
    /// una volta stamattina viene prima di uno suonato cento volte l'anno
    /// scorso.
    RecentlyPlayed,
    /// Titolo alfabetico.
    Title,
}

impl TrackOrder {
    /// La clausola `ORDER BY`. Chiusa in un `match`, mai composta da stringhe
    /// che arrivano da fuori: un ordinamento scelto dall'interfaccia non deve
    /// poter diventare un'iniezione.
    pub(super) const fn sql(self) -> &'static str {
        match self {
            Self::Shelf => {
                "t.artist COLLATE NOCASE, t.album COLLATE NOCASE,
                 t.disc_number, t.track_number, t.title COLLATE NOCASE"
            }
            Self::RecentlyAdded => "t.date_added DESC, t.id DESC",
            Self::MostPlayed => "t.play_count DESC, t.last_played_at DESC, t.title COLLATE NOCASE",
            // In SQLite `NULL` è più piccolo di tutto, quindi un `DESC` mette
            // già i mai ascoltati in fondo, per regola documentata. `IS NULL`
            // per primo lo dice esplicito e non cambia l'ordine. Il prezzo è
            // che il primo termine è un'espressione e nessun indice serve
            // questo ordinamento: il ripiano della Home, che vuole solo i primi
            // dodici, passa da [`ascoltati_di_recente`].
            Self::RecentlyPlayed => {
                "t.last_played_at IS NULL, t.last_played_at DESC, t.title COLLATE NOCASE"
            }
            Self::Title => "t.title COLLATE NOCASE, t.artist COLLATE NOCASE",
        }
    }
}

/// Una pagina di brani.
pub fn list_tracks(
    connection: &Connection,
    order: TrackOrder,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t ORDER BY {} LIMIT ?1 OFFSET ?2",
        order.sql()
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("elenco dei brani", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], track_from_row)
        .map_err(|err| db_error("elenco dei brani", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani", &err))
}

/// I brani ascoltati più di recente, per il ripiano della Home.
///
/// # Perché non `list_tracks` con [`TrackOrder::RecentlyPlayed`]
///
/// Perché quello impagina **tutta** la libreria, mai ascoltati in fondo, e il
/// suo primo termine d'ordine è un'espressione che nessun indice serve: ogni
/// chiamata leggeva e ordinava tutta `tracks`. Il ripiano si rifà a ogni cambio
/// di brano mentre la Home è aperta, e vuole solo i primi dodici ascoltati:
/// con `last_played_at IS NOT NULL` la domanda passa dall'indice parziale
/// `idx_tracks_last_played`, nato apposta per lei, e legge dodici righe.
///
/// Solo gli ascoltati con una data e almeno un ascolto: un brano con dei
/// conteggi arrivati senza data (`play_count > 0`, `last_played_at` NULL) non
/// è «di recente», e non riempie più i posti vuoti in fondo.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn ascoltati_di_recente(
    connection: &Connection,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let mut statement = connection
        .prepare_cached(&sql_dei_recenti())
        .map_err(|err| db_error("ascoltati di recente", &err))?;
    let rows = statement
        .query_map([limit], track_from_row)
        .map_err(|err| db_error("ascoltati di recente", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("ascoltati di recente", &err))
}

/// La domanda di [`ascoltati_di_recente`], a parte perché la prova ne guardi
/// il piano.
///
/// Il più unario su `play_count` toglie a quel termine l'indice dei più
/// ascoltati: senza, il pianificatore può preferirlo e tornare a ordinare
/// tutti gli ascoltati invece di scorrere l'indice delle date per dodici righe.
pub(super) fn sql_dei_recenti() -> String {
    format!(
        "SELECT {COLONNE_BRANO} FROM tracks t
          WHERE t.last_played_at IS NOT NULL AND +t.play_count > 0
          ORDER BY t.last_played_at DESC, t.title COLLATE NOCASE
          LIMIT ?1"
    )
}

/// Una pagina di preferiti.
///
/// # Perché una query e non un filtro sull'elenco dei brani
///
/// La finestra chiedeva duemila brani e teneva quelli col cuore. Due cose
/// storte insieme: legge tutta la libreria a ogni visita, e chi ne ha più di
/// duemila non vede i preferiti che stanno oltre — senza che niente lo dica.
///
/// L'ordine è quello della **decisione**, non del titolo: chi apre i preferiti
/// cerca quasi sempre quello che ha segnato poco fa. `liked_at` è nullo sulle
/// righe segnate prima che quella colonna esistesse, e `IS NULL` in testa
/// all'`ORDER BY` le manda in fondo invece di lasciarle dove capita.
pub fn list_liked(
    connection: &Connection,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t WHERE t.liked = 1
         ORDER BY t.liked_at IS NULL, t.liked_at DESC, t.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("elenco dei preferiti", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], track_from_row)
        .map_err(|err| db_error("elenco dei preferiti", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei preferiti", &err))
}

/// Un ascolto, con il brano che lo ha prodotto.
///
/// Il brano intero e non il solo identificativo: chi guarda la cronologia
/// guarda dei titoli, e restituire duecento `track_id` obbligherebbe la finestra
/// a duecento richieste — o a una `summaries_by_id` che rimescola l'ordine
/// proprio quando l'ordine *è* il contenuto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceCronologia {
    /// La riga di `play_history`, per distinguere due ascolti dello stesso
    /// brano nello stesso secondo.
    pub id: i64,
    /// Il brano, come lo mostra ogni altro elenco.
    pub brano: TrackSummary,
    /// Quando è **cominciato**, in millisecondi dall'epoca: è `played_at`, che
    /// `record_play` scrive all'inizio dell'ascolto.
    pub quando_ms: i64,
    /// Quanto se n'è sentito.
    pub ms_ascoltati: i64,
    /// Da dove viene: `local` se l'ha suonato Rymembra, `spotify` se importato.
    pub sorgente: String,
}

/// Una pagina di cronologia, dal più recente.
///
/// # Perché esiste solo adesso
///
/// `play_history` si scriveva dal primo giorno e non la leggeva nessuno: la
/// linguetta «Cronologia» della terza colonna era spenta con la sua ragione
/// scritta accanto. Era un difetto piccolo finché quella tabella conteneva
/// soltanto gli ascolti fatti dentro Rymembra — su una libreria appena aperta,
/// zero righe. Dopo l'importazione di un account Spotify contiene anni, e una
/// schermata che non li mostra diventa la differenza fra aver importato e non
/// averlo fatto.
///
/// # Un ascolto il cui brano non c'è più non compare
///
/// È una `JOIN`, non una `LEFT JOIN`. `play_history.track_id` ha
/// `ON DELETE CASCADE`, quindi il caso è già impossibile per un brano tolto
/// davvero; la `JOIN` è ciò che rende impossibile anche il resto — una riga
/// senza titolo né interprete in mezzo a un elenco di canzoni.
pub fn list_history(
    connection: &Connection,
    offset: i64,
    limit: i64,
) -> Result<Vec<VoceCronologia>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO}, h.id, h.played_at, h.ms_played, h.source
           FROM play_history AS h
           JOIN tracks AS t ON t.id = h.track_id
          ORDER BY h.played_at DESC, h.id DESC
          LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("cronologia d'ascolto", &err))?;
    // Le colonne del brano vengono per prime: le quattro dell'ascolto cominciano
    // dove finiscono quelle, e il conto lo tiene [`COLONNE_BRANO_QUANTE`]
    // invece di un numero scritto qui. Quando `COLONNE_BRANO` è cresciuta di
    // tre colonne, questi indici si sono spostati da soli — prima di allora
    // erano scritti a mano, e una prova in `library/tests.rs` controlla il campo
    // `quando_ms` e non solo la lunghezza proprio per accorgersene.
    let rows = statement
        .query_map(rusqlite::params![limit, offset], |row| {
            Ok(VoceCronologia {
                brano: track_from_row(row)?,
                id: row.get(COLONNE_BRANO_QUANTE)?,
                quando_ms: row.get(COLONNE_BRANO_QUANTE + 1)?,
                ms_ascoltati: row.get(COLONNE_BRANO_QUANTE + 2)?,
                sorgente: row.get(COLONNE_BRANO_QUANTE + 3)?,
            })
        })
        .map_err(|err| db_error("cronologia d'ascolto", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("cronologia d'ascolto", &err))
}

/// Quanti ascolti ci sono in tutto.
///
/// Serve alla finestra per sapere se c'è una pagina dopo. Una `COUNT(*)` su una
/// tabella di centomila righe con un indice su `played_at` costa poco, e la
/// alternativa — chiedere una riga in più e guardare se arriva — mente sul
/// totale, che qui è l'unica cosa che dice «hai importato dieci anni».
pub fn count_history(connection: &Connection) -> Result<i64, AppError> {
    connection
        .prepare_cached("SELECT COUNT(*) FROM play_history")
        .and_then(|mut statement| statement.query_row([], |row| row.get(0)))
        .map_err(|err| db_error("conteggio della cronologia", &err))
}

/// Un brano solo, per identificativo.
///
/// `None` se la riga non c'è più: capita a un brano tolto dalla libreria mentre
/// la coda lo teneva ancora, e non è un guasto da propagare — è una riga da
/// saltare.
pub fn read_summary(connection: &Connection, id: i64) -> Result<Option<TrackSummary>, AppError> {
    let sql = format!("SELECT {COLONNE_BRANO} FROM tracks t WHERE t.id = ?1");
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("lettura di un brano", &err))?;
    statement
        .query_row([id], track_from_row)
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura di un brano", &altro)),
        })
}

/// I dati tecnici del file di un brano: cos'è il file, non cosa esce dalle casse.
///
/// La distinzione è una decisione, non una semplificazione. Quel che esce dalla
/// scheda audio può essere ricampionato dal mixer di sistema, e questa struttura
/// non lo sa e non lo dice: descrive il file che sta sul disco. Raccontare
/// l'uscita reale vorrebbe dire una riga che cambia quando si cambia cuffia, per
/// una domanda — «com'è fatta questa edizione» — a cui l'uscita non risponde.
///
/// # Perché non sta in [`TrackSummary`]
///
/// Perché `TrackSummary` è la riga di un elenco, e di righe ce ne sono qualche
/// migliaio per volta, più tutta la coda. Quattro campi in più su ognuna
/// sarebbero quattro campi spediti mille volte per disegnarne **uno**: quello
/// del brano che sta suonando. La parsimonia di quella struttura è dichiarata
/// nella sua carta, e questo tipo esiste per non smentirla.
///
/// # Perché ogni campo è un `Option`
///
/// Perché le quattro colonne sono nullable, e lo sono per forza: `lofty` non
/// ricava le proprietà di tutti i contenitori allo stesso modo, e una libreria
/// scansionata quando quelle colonne non si scrivevano ancora ha le righe vuote.
/// Chi disegna unisce i pezzi che ci sono e tace sugli altri.
///
/// # Perché i numeri sono `i64` e non `u32`
///
/// Perché qui si **legge**, e leggere non deve poter fallire per un dato storto.
/// Le colonne sono `INTEGER` senza `CHECK`: nessuno vi scrive un numero negativo
/// — arrivano dai `u32` e `u8` di `lofty` — ma un `u32` chiesto a una riga
/// negativa è un errore di rusqlite, e quell'errore butterebbe via tutti e
/// quattro i campi per colpa di uno. Con un `i64` il valore assurdo arriva a chi
/// disegna, che lo scarta perché non è positivo: lo stesso posto in cui si
/// scarta un `NULL`, invece di un secondo cammino per il guasto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatoFile {
    /// Il formato come si scrive a chi guarda: «FLAC», «MP3», «M4A».
    ///
    /// Già passato per [`etichetta_codec`]: in tabella c'è il `Debug` di
    /// `lofty::FileType`, e quel che la finestra deve mostrare non è «Mpeg».
    pub codec: Option<String>,
    /// La frequenza di campionamento, in hertz: 44100, non 44,1.
    ///
    /// In hertz perché è così che sta in tabella e così che `lofty` la legge. I
    /// kilohertz con la virgola sono una questione di lingua, e si fanno dove la
    /// lingua si conosce — nella finestra.
    pub sample_rate: Option<i64>,
    /// Quanti canali: 1, 2, 6, 8.
    ///
    /// Il numero e non il nome. «Stereo» e «5.1» sono parole, e le parole si
    /// traducono: mandarle da qui vorrebbe dire mandare italiano a chi ha
    /// l'interfaccia in inglese.
    pub channels: Option<i64>,
    /// Il bitrate **medio**, in kbit/s, come l'ha calcolato `lofty`.
    ///
    /// Medio e non dichiarato, ed è la ragione per cui si mostra anche sui
    /// formati senza perdita: su un VBR dice quel che il file pesa davvero
    /// invece di quel che l'intestazione promette, e su un FLAC dice quanto è
    /// densa l'edizione — che è l'unica differenza fra due FLAC dello stesso
    /// brano.
    pub bitrate: Option<i64>,
    /// I bit per campione, solo per i formati senza perdita.
    ///
    /// Non è una colonna: lo dice il decodificatore quando il brano suona. Vedi
    /// [`FormatoFile::completato_da`].
    pub bits_per_sample: Option<i64>,
    /// La frequenza a cui il brano esce dalla scheda audio, in hertz.
    ///
    /// Diversa da [`Self::sample_rate`] vuol dire ricampionato. Come
    /// [`Self::bits_per_sample`], lo sa solo chi suona.
    pub output_rate: Option<i64>,
}

impl FormatoFile {
    /// La scheda del database corretta con quel che il decodificatore ha in
    /// mano.
    ///
    /// # Chi vince, e perché
    ///
    /// Il **decodificatore** su codec, frequenza e canali: sono i numeri di
    /// quel che si sente. La colonna può dire FLAC di un brano dell'Internet
    /// Archive che si ascolta dal derivato MP3, M4A di un file che dentro è
    /// ALAC, e niente del tutto per un flusso.
    ///
    /// Il **database** sul bitrate dei file locali: è la misura di `lofty`, che
    /// toglie tag e copertina, mentre il conto del motore è byte diviso durata.
    /// Per un flusso è il contrario — le colonne descrivono il file del catalogo,
    /// e i byte che arrivano sono quelli del derivato — e vince il motore.
    #[must_use]
    pub fn completato_da(self, suonato: &rymembra_play::FormatoSuonato) -> Self {
        let kbps = suonato.kbps.map(i64::from);
        Self {
            codec: suonato.codec.map(str::to_owned).or(self.codec),
            sample_rate: Some(i64::from(suonato.frequenza)),
            channels: Some(i64::from(suonato.canali)),
            bitrate: if suonato.remoto {
                kbps.or(self.bitrate)
            } else {
                self.bitrate.or(kbps)
            },
            bits_per_sample: suonato.bit.map(i64::from),
            output_rate: Some(i64::from(suonato.frequenza_uscita)),
        }
    }
}

/// Il nome del formato come si scrive, dal `Debug` di `lofty::FileType`.
///
/// # Perché serve una mappa, e perché lavora in lettura
///
/// Perché la scansione scrive `format!("{:?}", tagged.file_type())` (vedi
/// [`crate::metadata::read_tags`]), cioè i nomi delle varianti di
/// un'enumerazione Rust: «Mpeg», «Vorbis», «Mp4». Sono nomi di codice, non nomi
/// di formati, e nessuno dei tre è quel che chi ascolta si aspetta di leggere.
///
/// La correzione sta qui e non nella scansione perché **le librerie già
/// scansionate hanno quel valore in tabella**. Tradurre in scrittura vorrebbe
/// dire una migrazione per riscrivere una colonna che fino a oggi nessuno aveva
/// mai letto, e anche dopo la migrazione le due forme convivrebbero comunque:
/// basta un disco esterno ricollegato, o un profilo ripristinato da un backup
/// più vecchio. Tradurre in lettura funziona su tutto quel che c'è già, e non
/// costa niente.
///
/// # Cosa dice, e cosa non dice
///
/// Il **contenitore**, che è l'unica cosa che la colonna sa. `Mp4 → M4A` e non
/// «AAC»: dentro un M4A ci può stare AAC o ALAC, la colonna non distingue, e
/// scrivere «AAC» su un ALAC vorrebbe dire chiamare «con perdita» un file che
/// non ne ha. Per la stessa ragione `Vorbis → Ogg Vorbis`, che è il nome con cui
/// quel formato si conosce.
///
/// L'unica imprecisione che la mappa accetta è `Mpeg → MP3`. `lofty` tiene
/// MPEG-1 Layer I, II e III in una variante sola, quindi un `.mp2` — che in una
/// libreria di musica non c'è — si leggerebbe «MP3». L'alternativa sarebbe
/// scrivere «MPEG», che è giusto e non dice niente a nessuno.
///
/// # Un nome sconosciuto resta com'è
///
/// `FileType` è `#[non_exhaustive]` e ha una variante `Custom`. Un valore fuori
/// elenco si mostra **così come sta scritto**: è sempre meglio di niente, e non
/// mente mai. Il giorno in cui `lofty` aggiunge un contenitore, Rymembra ne scrive
/// il nome della variante invece di lasciare un buco — brutto e vero, che è
/// l'ordine giusto fra le due cose.
#[must_use]
pub fn etichetta_codec(grezzo: &str) -> String {
    match grezzo {
        "Aac" => "AAC",
        "Aiff" => "AIFF",
        "Ape" => "APE",
        "Flac" => "FLAC",
        "Mpeg" => "MP3",
        "Mp4" => "M4A",
        "Mpc" => "Musepack",
        "Opus" => "Opus",
        "Vorbis" => "Ogg Vorbis",
        "Speex" => "Speex",
        "Wav" => "WAV",
        "WavPack" => "WavPack",
        altro => return altro.to_owned(),
    }
    .to_owned()
}

/// I dati tecnici del file di un brano, per identificativo.
///
/// `None` se la riga non c'è più, per la stessa ragione di [`read_summary`]: un
/// brano può uscire dalla libreria mentre la coda lo tiene ancora, e non è un
/// guasto da propagare. `Some` con i campi vuoti è un'altra cosa, e vuol dire
/// che la riga c'è ma quei dati non si erano letti.
///
/// Una `SELECT` sulle quattro colonne e non [`COLONNE_BRANO`]: le quattro non
/// stanno in quell'elenco, e metterle là vorrebbe dire spedirle con ogni riga di
/// ogni elenco per disegnarle in un posto solo.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn read_formato(
    connection: &Connection,
    track_id: i64,
) -> Result<Option<FormatoFile>, AppError> {
    let mut statement = connection
        .prepare_cached("SELECT codec, sample_rate, channels, bitrate FROM tracks WHERE id = ?1")
        .map_err(|err| db_error("lettura del formato di un brano", &err))?;
    statement
        .query_row([track_id], |row| {
            Ok(FormatoFile {
                // Una stringa vuota vale come assente: «» non è un formato, e
                // lasciarla passare vorrebbe dire un separatore senza niente
                // davanti nella riga che chi disegna compone.
                codec: row
                    .get::<_, Option<String>>(0)?
                    .filter(|grezzo| !grezzo.trim().is_empty())
                    .map(|grezzo| etichetta_codec(grezzo.trim())),
                sample_rate: row.get(1)?,
                channels: row.get(2)?,
                bitrate: row.get(3)?,
                bits_per_sample: None,
                output_rate: None,
            })
        })
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura del formato di un brano", &altro)),
        })
}

/// Le righe di un elenco di identificativi, **nell'ordine chiesto**.
///
/// L'ordine è il punto. Una `IN (…)` restituisce le righe nell'ordine che fa
/// comodo a SQLite, e chi chiama qui — il pannello della coda — ha un ordine
/// suo che è l'unica cosa che gli interessa: la coda non suona per
/// identificativo crescente. Riordinare a valle costa una mappa e toglie una
/// classe di difetti in cui l'elenco mostrato non è quello che si sentirà.
///
/// Gli identificativi che non esistono più semplicemente non compaiono.
pub fn summaries_by_id(
    connection: &Connection,
    ids: &[i64],
) -> Result<Vec<TrackSummary>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    // I segnaposto si contano, non si compongono con i valori: un elenco di
    // identificativi resta un elenco di parametri anche quando è lungo. A pezzi
    // di [`PEZZO`] perché «Riproduci tutto» su una libreria grande fa una coda
    // di decine di migliaia di brani, e SQLite oltre i 32 766 parametri non
    // prepara la query: il pannello della coda restava vuoto con un errore.
    // L'ordine non ne soffre, lo rimette la mappa qui sotto.
    let mut trovati = Vec::with_capacity(ids.len());
    for pezzo in ids.chunks(PEZZO) {
        let sql = format!(
            "SELECT {COLONNE_BRANO} FROM tracks t WHERE t.id IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(|err| db_error("brani per identificativo", &err))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(pezzo), track_from_row)
            .map_err(|err| db_error("brani per identificativo", &err))?;
        for riga in rows {
            trovati.push(riga.map_err(|err| db_error("brani per identificativo", &err))?);
        }
    }

    // Si clona invece di consumare la mappa: la stessa canzone può stare due
    // volte nella stessa coda — accodarla due volte è legittimo — e toglierla
    // dalla mappa alla prima occorrenza la farebbe sparire dalla seconda.
    let per_id: std::collections::HashMap<i64, TrackSummary> =
        trovati.into_iter().map(|b| (b.id, b)).collect();
    Ok(ids
        .iter()
        .filter_map(|id| per_id.get(id).cloned())
        .collect())
}

/// Gli identificativi dei brani a questi percorsi, nello stesso ordine.
///
/// `None` per un percorso che non è in libreria: serve a chi ha dei file in
/// mano — trascinati sulla finestra — e deve sapere quali può mettere in coda,
/// perché la coda è fatta di identificativi e un file che la scansione non ha
/// mai visto non ne ha uno.
///
/// # Due confronti, e perché non `path_key`
///
/// Prima quello esatto, che passa dall'indice `UNIQUE` di `path` ed è il caso
/// di quasi tutti: Esplora risorse consegna il percorso con le stesse maiuscole
/// che la camminata ha scritto. Poi, solo dove il filesystem ignora le
/// maiuscole, lo stesso confronto con `COLLATE NOCASE`, per i file che il primo
/// non ha trovato.
///
/// Fino alla 2.4 quel secondo confronto non aveva un indice, e ogni file
/// lasciato cadere con una grafia diversa leggeva la tabella intera: cento file
/// trascinati da una cartella rinominata nelle maiuscole erano cento letture di
/// cinquantamila righe, sul filo che aspettava di metterli in coda. La 030 gli
/// ha dato `idx_tracks_percorso_piegato`, della stessa forma, e una prova in
/// `library/prove_per_chiavi.rs` guarda il piano di SQLite e fallisce se torna
/// la lettura intera.
///
/// [`rymembra_domain::paths::path_key`] unificherebbe anche i separatori, e
/// servirebbe calcolarla su ogni riga della tabella: una lettura intera della
/// libreria per ogni file lasciato cadere, per riconciliare una barra che i
/// percorsi di Windows non hanno mai girata dall'altra parte.
pub fn ids_by_path(
    connection: &Connection,
    paths: &[String],
    rules: PathRules,
) -> Result<Vec<Option<i64>>, AppError> {
    use rusqlite::OptionalExtension as _;

    let mut esatto = connection
        .prepare_cached("SELECT id FROM tracks WHERE path = ?1")
        .map_err(|err| db_error("brani per percorso", &err))?;
    let mut piegato = connection
        .prepare_cached("SELECT id FROM tracks WHERE path = ?1 COLLATE NOCASE LIMIT 1")
        .map_err(|err| db_error("brani per percorso", &err))?;
    paths
        .iter()
        .map(|path| {
            let trovato = esatto
                .query_row([path], |row| row.get(0))
                .optional()
                .map_err(|err| db_error("brani per percorso", &err))?;
            if trovato.is_some() || !rules.case_insensitive {
                return Ok(trovato);
            }
            piegato
                .query_row([path], |row| row.get(0))
                .optional()
                .map_err(|err| db_error("brani per percorso", &err))
        })
        .collect()
}

/// I brani di un album, nell'ordine del disco.
pub fn album_tracks(
    connection: &Connection,
    album_key: &str,
) -> Result<Vec<TrackSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_BRANO} FROM tracks t WHERE t.album_key = ?1
         ORDER BY t.disc_number, t.track_number, t.title COLLATE NOCASE"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("brani di un album", &err))?;
    let rows = statement
        .query_map([album_key], track_from_row)
        .map_err(|err| db_error("brani di un album", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("brani di un album", &err))
}

/// Una pagina di album, per artista e anno.
pub fn list_albums(
    connection: &Connection,
    offset: i64,
    limit: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         ORDER BY a.artist COLLATE NOCASE, a.year, a.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("elenco degli album", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit, offset], album_from_row)
        .map_err(|err| db_error("elenco degli album", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco degli album", &err))
}

/// I dischi entrati in libreria per ultimi.
///
/// # Perché dischi e non brani
///
/// Il ripiano «aggiunti di recente» della Home chiedeva i dodici brani con la
/// `date_added` più alta. Ma la musica non entra un brano alla volta: entra una
/// cartella alla volta, e una cartella è un disco. Le dodici righe erano dodici
/// tracce **dello stesso album** — per giunta in ordine arbitrario, perché
/// condividono la `date_added` al secondo e il criterio di spareggio era l'id.
/// Un ripiano che mostra dodici volte la stessa copertina non dice «cos'è
/// entrato di nuovo»: dice «l'ultima cartella», e la dice dodici volte.
///
/// # L'ordine di spareggio, e perché non è il titolo
///
/// Uno spareggio ci vuole: due dischi entrati nello stesso istante devono
/// presentarsi nello stesso ordine a ogni apertura, o il ripiano si riscrive da
/// solo fra una visita e l'altra senza che sia successo niente. Il candidato
/// ovvio era il titolo, e sarebbe stato sbagliato — su una libreria arrivata
/// tutta in una volta (una cartella sola scansionata una volta sola: il caso
/// normale al primo avvio, dove `date_added` ha *un* valore per migliaia di
/// brani) il titolo diventa l'unico criterio, e «aggiunti di recente» mostra i
/// dodici dischi che cominciano per A. Corretto e illeggibile.
///
/// Quindi `MAX(t.id)`: l'ordine in cui le righe sono state scritte, che è più
/// fine della data al millisecondo e vuol dire la stessa cosa — chi è entrato
/// per ultimo sta davanti. È stabile quanto il titolo, perché un id non cambia
/// più, e non ha bisogno di essere letto dalla tabella: in SQLite l'id **è** il
/// rowid, e sta dentro ogni voce dell'indice che serve il raggruppamento.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn recently_added_albums(
    connection: &Connection,
    limit: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    // Il raggruppamento in una sottoquery e non sull'esterna: così gira su
    // `idx_tracks_album_added` e ne esce una riga per album, invece di tutte le
    // tracce della libreria da scartare dopo.
    let sql = format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         JOIN (SELECT album_key, MAX(date_added) AS entrato, MAX(id) AS ultima
               FROM tracks WHERE album_key IS NOT NULL
               GROUP BY album_key) u ON u.album_key = a.album_key
         ORDER BY u.entrato DESC, u.ultima DESC
         LIMIT ?1"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("dischi aggiunti di recente", &err))?;
    let rows = statement
        .query_map(rusqlite::params![limit], album_from_row)
        .map_err(|err| db_error("dischi aggiunti di recente", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("dischi aggiunti di recente", &err))
}

/// Una pagina degli album di un artista.
///
/// # La stessa chiave di [`list_artists`]
///
/// Gli album in cui **suona** un brano dell'artista, con l'artista di ogni
/// brano calcolato come lo calcola [`list_artists`] ([`ARTISTA_EFFETTIVO`]:
/// `album_artist`, o l'interprete se manca), che conta `COUNT(DISTINCT
/// album_key)` su quegli stessi brani. Così l'intestazione della pagina e la
/// griglia dicono lo stesso numero. Prima si filtrava su `albums.artist`, che
/// è l'artista **canonico** del disco — il più frequente fra i suoi brani —:
/// chi compariva solo in una raccolta senza `album_artist`, o come «X feat. Y»
/// in un disco di altri, aveva un titolo che diceva «1 album» e una griglia
/// vuota.
///
/// Effetto voluto: una raccolta compare nella pagina di ciascun interprete che
/// vi suona, e il suo `artist` resta quello canonico.
///
/// Il confronto resta esatto (il risultato di `TRIM` non porta collazione,
/// quindi `BINARY`): la normalizzazione serve a **ordinare** gli artisti, non a
/// fonderli, e due grafie diverse restano due artisti anche nella griglia.
///
/// Esiste perché la pagina di un artista filtrava nella finestra la pagina di
/// album già scaricata: chi stava oltre l'ultimo album chiesto vedeva una
/// pagina vuota sotto un titolo che diceva «tre album».
pub fn albums_by_artist(
    connection: &Connection,
    artist: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    let sql = sql_degli_album_di_un_artista();
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("album di un artista", &err))?;
    let rows = statement
        .query_map(rusqlite::params![artist, limit, offset], album_from_row)
        .map_err(|err| db_error("album di un artista", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("album di un artista", &err))
}

/// La domanda di [`albums_by_artist`], a parte perché la prova ne guardi il
/// piano.
///
/// La sottoquery è scritta con [`ARTISTA_EFFETTIVO`] su `tracks` senza alias,
/// parola per parola: è l'espressione di `idx_tracks_artista_effettivo`, e
/// SQLite usa un indice su espressione solo se la riconosce identica. Scritta
/// a mano, la pagina di un artista leggerebbe tutta la libreria.
pub(super) fn sql_degli_album_di_un_artista() -> String {
    format!(
        "SELECT {COLONNE_ALBUM} FROM albums a
         WHERE a.album_key IN (SELECT album_key FROM tracks WHERE {ARTISTA_EFFETTIVO} = ?1)
         ORDER BY a.year, a.title COLLATE NOCASE
         LIMIT ?2 OFFSET ?3"
    )
}

/// Un artista come lo mostra la griglia.
///
/// # Perché le copertine sono quattro e vengono dagli album
///
/// Nessuna immagine d'artista esisterà mai: non c'è rete, e un lettore locale
/// che va a prendersi i ritratti su internet è un lettore locale che chiama
/// casa. Il ritratto è un mosaico due per due delle copertine dei suoi dischi —
/// dati che ci sono già, e che per di più dicono qualcosa di vero: si riconosce
/// un artista dai suoi album prima che dalla sua faccia.
///
/// Quattro perché il mosaico ne mostra quattro. Chi ne ha uno solo ottiene una
/// copertina sola a tutto riquadro, che è il caso giusto e non un ripiego.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistSummary {
    /// Il nome come sta nei tag.
    pub name: String,
    /// Il nome sotto cui ordinarlo, senza articolo iniziale.
    pub sort_name: String,
    /// Quanti album.
    pub albums: i64,
    /// Quanti brani.
    pub tracks: i64,
    /// Fino a quattro copertine, dalle più vecchie alle più recenti.
    pub covers: Vec<String>,
}

/// Gli artisti della libreria, in ordine alfabetico del nome normalizzato.
///
/// # Perché niente pagina, e perché l'ordine si fa qui
///
/// Gli artisti sono pochi — un ordine di grandezza meno dei brani — e la vista
/// li mostra tutti con un indice alfabetico laterale invece che a pagine: si
/// salta alla lettera, non alla pagina sette. Chiederne una fetta alla volta
/// vorrebbe dire non poter dire quante lettere esistono.
///
/// L'ordinamento **non** è in SQL. `sort_name` è una regola del dominio — «The
/// Cure» sta sotto C — e SQLite non può chiamarla senza che gliela si registri
/// come funzione, cioè senza duplicarla. Ordinare qui la tiene in un posto solo,
/// e quel posto è lo stesso che userà Android.
pub fn list_artists(connection: &Connection) -> Result<Vec<ArtistSummary>, AppError> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {ARTISTA_EFFETTIVO} AS nome,
                    COUNT(DISTINCT album_key) AS album,
                    COUNT(*)                  AS brani
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} <> ''
             GROUP BY nome"
        ))
        .map_err(|err| db_error("elenco degli artisti", &err))?;
    let mut artisti: Vec<ArtistSummary> = statement
        .query_map([], |row| {
            let name: String = row.get(0)?;
            Ok(ArtistSummary {
                sort_name: rymembra_domain::keys::sort_name(&name),
                name,
                albums: row.get(1)?,
                tracks: row.get(2)?,
                covers: Vec::new(),
            })
        })
        .map_err(|err| db_error("elenco degli artisti", &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco degli artisti", &err))?;

    // Le copertine in una seconda passata e non in una sottoquery correlata: una
    // riga per album invece di una per brano, e il raggruppamento a quattro si
    // fa dove è leggibile. Su una libreria vera sono qualche migliaio di righe.
    let mut copertine = connection
        .prepare_cached(&format!(
            "SELECT {ARTISTA_EFFETTIVO} AS nome, MIN(cover_art_hash) AS hash, MIN(year) AS anno
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} <> '' AND cover_art_hash IS NOT NULL
             GROUP BY nome, album_key
             ORDER BY nome, anno"
        ))
        .map_err(|err| db_error("copertine degli artisti", &err))?;
    let mut per_artista: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let righe = copertine
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| db_error("copertine degli artisti", &err))?;
    for riga in righe {
        let (nome, hash) = riga.map_err(|err| db_error("copertine degli artisti", &err))?;
        let elenco = per_artista.entry(nome).or_default();
        // Quattro, e le stesse quattro a ogni chiamata: l'ordine della query è
        // deterministico, quindi il mosaico di un artista non cambia disegno da
        // un'apertura all'altra.
        if elenco.len() < 4 && !elenco.contains(&hash) {
            elenco.push(hash);
        }
    }
    for artista in &mut artisti {
        if let Some(trovate) = per_artista.remove(&artista.name) {
            artista.covers = trovate;
        }
    }

    artisti.sort_by(|a, b| {
        a.sort_name
            .cmp(&b.sort_name)
            // A parità di chiave d'ordinamento — due artisti che differiscono
            // solo per accenti o punteggiatura — decide il nome vero, altrimenti
            // l'ordine dipenderebbe da come `HashMap` ha girato.
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(artisti)
}

/// Traduce quel che l'utente ha scritto in un'espressione FTS5.
///
/// Ogni parola diventa una stringa fra virgolette con un asterisco in coda.
/// Virgolettata perché altrimenti `AND`, `OR`, `NOT`, `NEAR`, l'asterisco, il
/// trattino e le parentesi verrebbero letti come operatori: chi cerca `AC/DC` o
/// `Where Are You` otterrebbe un errore di sintassi invece dei suoi dischi.
/// L'asterisco perché in una libreria musicale si cerca mentre si digita, e
/// `bjor` deve già trovare Björk.
///
/// Le virgolette dentro una parola si raddoppiano, che è come FTS5 le protegge.
#[must_use]
pub fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cerca nella libreria.
///
/// L'ordine è quello di `bm25`, la rilevanza di FTS5: una parola rara pesa più
/// di una comune, e un riscontro in un campo corto pesa più dello stesso
/// riscontro perso dentro uno lungo.
pub fn search(
    connection: &Connection,
    query: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<TrackSummary>, AppError> {
    let expression = fts_query(query);
    if expression.is_empty() {
        return Ok(Vec::new());
    }
    // Il nome della tabella e non un alias: in FTS5 il lato sinistro di `MATCH`
    // e l'argomento di `bm25` devono nominare la tabella virtuale per esteso, e
    // con un alias SQLite risponde che quella colonna non esiste.
    let sql = format!(
        "SELECT {COLONNE_BRANO}
         FROM tracks_fts
         JOIN tracks t ON t.id = tracks_fts.rowid
         WHERE tracks_fts MATCH ?1
         ORDER BY bm25(tracks_fts) LIMIT ?2 OFFSET ?3"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("ricerca", &err))?;
    let rows = statement
        .query_map(rusqlite::params![expression, limit, offset], track_from_row)
        .map_err(|err| db_error("ricerca", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("ricerca", &err))
}

/// Quanti brani risponderebbero a questa ricerca.
///
/// Serve a poter scrivere «312 risultati» invece di «60 risultati», che è quel
/// che l'intestazione diceva quando il numero era la lunghezza della prima
/// pagina: un conteggio che si ferma al limite non è un conteggio, è il limite
/// scritto in lettere.
///
/// Passa dalla **stessa** [`fts_query`] della ricerca: due traduzioni diverse
/// della stessa stringa sono due occasioni di contare righe che poi non
/// arrivano.
pub fn search_count(connection: &Connection, query: &str) -> Result<i64, AppError> {
    let expression = fts_query(query);
    if expression.is_empty() {
        return Ok(0);
    }
    connection
        .prepare_cached("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?1")
        .and_then(|mut statement| statement.query_row([&expression], |row| row.get(0)))
        .map_err(|err| db_error("conteggio della ricerca", &err))
}
