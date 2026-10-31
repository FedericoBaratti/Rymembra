//! Le righe di `tracks`: un file letto diventa una [`TrackRow`], e la riga si inserisce,
//! si riscrive o si toglie — per la scansione o perché l'ha chiesto chi ascolta.

use super::*;

/// Un brano letto dal disco, nella forma in cui il database lo accetta.
///
/// Le conversioni verso `i64` avvengono qui e non al momento di legare i
/// parametri: SQLite ha un tipo intero solo, e farle sparse nelle query
/// significherebbe ripeterle a ogni istruzione che tocca la stessa colonna.
#[derive(Debug, Clone)]
pub struct TrackRow {
    /// Il percorso come sta sul disco.
    pub path: String,
    /// L'identità fra dispositivi, calcolata dai valori qui sotto.
    ///
    /// Segue le correzioni dell'utente: `incerti::applica_correzioni` la
    /// ricalcola dai campi corretti, perché è la chiave con cui gli altri
    /// dispositivi devono ritrovare questo brano.
    pub track_key: String,
    /// L'identità di **contenuto**: vedi [`ContentKey`].
    ///
    /// Si calcola dove il file viene letto e da nessun'altra parte, **dai tag
    /// grezzi e non dai campi qui accanto**: quelli portano già i ripieghi, e
    /// un ripiego viene dal nome del file. Nessuna correzione la tocca, ed è
    /// per questo che `scan_plan::match_moved_tracks` riconosce un file
    /// spostato anche di un brano corretto dieci volte — o rinominato, se i
    /// tag non ce li ha.
    ///
    /// `None` solo per una riga **provvisoria**: un file del cloud entrato in
    /// libreria dall'elenco del servizio, prima che lo si leggesse (vedi
    /// [`riga_provvisoria`]). Va in colonna come `NULL`, e `NULL` è la
    /// sentinella che fa rileggere la riga alla passata dopo
    /// ([`known_tracks`]).
    pub content_key: Option<String>,
    /// Il titolo, o la radice del nome del file se i tag non ne hanno uno.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// L'artista dell'album.
    pub album_artist: Option<String>,
    /// La chiave di raggruppamento: titolo normalizzato più cartella.
    pub album_key: String,
    /// L'anno.
    pub year: Option<i64>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Genere.
    pub genre: Option<String>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// Battiti al minuto.
    pub bpm: Option<f64>,
    /// Tonalità.
    pub musical_key: Option<String>,
    /// Commento.
    pub comment: Option<String>,
    /// Testo.
    pub lyrics: Option<String>,
    /// La copertina salvata nello store, se il file ne aveva una leggibile.
    pub cover: Option<StoredCover>,
    /// L'immagine c'era e non si è potuta salvare, con il perché.
    ///
    /// Non fa fallire il brano — la musica si tiene comunque — ma non va
    /// nemmeno lasciata cadere in silenzio: un disco pieno che si mangia
    /// quattrocento copertine di fila è una cosa che chi scansiona deve poter
    /// leggere, e distinguerla da un file illeggibile conta perché la reazione è
    /// diversa. Da qui finisce in [`ScanReport::cover_failures`].
    pub copertina_persa: Option<AppError>,
    /// Bitrate in kbps.
    pub bitrate: Option<i64>,
    /// Frequenza di campionamento.
    pub sample_rate: Option<i64>,
    /// Canali.
    pub channels: Option<i64>,
    /// Il formato.
    pub codec: Option<String>,
    /// Dimensione del file.
    pub file_size: i64,
    /// Data di modifica del file, già troncata.
    pub date_modified: i64,
    /// ReplayGain della traccia.
    pub replaygain_track_db: Option<f64>,
    /// ReplayGain dell'album.
    pub replaygain_album_db: Option<f64>,
    /// Identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
    /// Identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
    /// Identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// Il file audio vero di un brano d'immagine CD (vedi la migrazione 034);
    /// `None` per un file intero.
    ///
    /// Si scrive **sempre**, anche quando è `None`: un brano d'immagine che
    /// torna file intero — l'immagine tagliata a mano in brani con gli stessi
    /// tag, riconosciuta come spostamento — deve perdere il riferimento, e un
    /// `UPDATE` che saltasse le tre colonne lo lascerebbe a suonare un pezzo di
    /// un file che non c'entra più.
    pub immagine: Option<String>,
    /// Dove comincia il brano dentro l'immagine, in fotogrammi CD (1/75 s).
    pub inizio_cd: Option<i64>,
    /// Dove finisce, escluso; `None` per l'ultimo, che arriva in fondo al file.
    pub fine_cd: Option<i64>,
}

/// Legge un file e ne costruisce la riga, salvando la copertina nello store.
///
/// # I ripieghi sui campi obbligatori
///
/// `title`, `artist` e `album` non possono essere nulli nello schema, e i file
/// senza tag esistono. Il titolo ripiega sulla radice del nome del file — che è
/// l'unica cosa che si sa di quel brano, e per come la gente nomina i file è
/// spesso anche giusta; gli altri due sui ripieghi del dominio.
///
/// I ripieghi entrano **anche nel calcolo di [`TrackKey`]**, e non è
/// indifferente: derivandola dai tag grezzi, ogni file senza tag avrebbe chiave
/// `||`, e la sincronizzazione li tratterebbe tutti come lo stesso brano.
///
/// In [`ContentKey`] invece **non entrano**, e la differenza è tutto il punto
/// della colonna. Un ripiego dipende dal nome del file: se lo mettessimo dentro
/// l'identità di contenuto, un file senza tag avrebbe chiave `radice del
/// nome|artista sconosciuto|album sconosciuto`, e rinominarlo — la sola cosa
/// che si può fare per distinguere un file che i tag non ce li ha, e quindi la
/// prima che si fa — lo trasformerebbe in un brano nuovo, con la riga vecchia
/// cancellata e via ascolti, voto e playlist. La chiave di contenuto di quel
/// file è quindi `durata|dimensione`, che un rinomino non tocca.
///
/// Una copertina che non si decodifica non fa fallire il brano: si perde
/// l'immagine, si tiene la musica.
pub fn read_track(
    files: &dyn MusicFiles,
    covers: &CoverStore,
    file: &DiscoveredFile,
) -> Result<TrackRow, AppError> {
    let tags = read_tags(files, &file.path)?;

    let file_size = i64::try_from(file.size_bytes).unwrap_or(i64::MAX);
    // Zero dal file vuol dire «non la so»: un file letto senza le proprietà
    // perché il lettore del cloud ha toccato il suo tetto (vedi
    // `metadata::read_tags`). Allora vale quella che il servizio ha detto
    // nell'elenco, se l'ha detta; sul disco non ce n'è mai una, e resta zero
    // come prima.
    let durata_letta = if tags.duration_ms > 0 {
        tags.duration_ms
    } else {
        files
            .tag_noti(&file.path)
            .and_then(|noti| noti.durata_ms)
            .unwrap_or(0)
    };
    let duration_ms = i64::try_from(durata_letta).unwrap_or(0);

    // L'identità di contenuto si calcola **prima dei ripieghi**, ed è per
    // questo che sta sopra e non sotto: le riceve come `Option`, cioè come il
    // file le ha dette. Passarle dopo — quando il titolo è già diventato la
    // radice del nome — legherebbe l'identità di un file senza tag al suo
    // **nome**, e rinominarlo lo duplicherebbe: che è esattamente quel che
    // capita a un file senza tag, perché il nome è l'unica cosa che lo
    // distingue e quindi l'unica che si ha voglia di sistemare.
    let content_key = ContentKey::compute(ContentKeyInput {
        artist: tags.artist.as_deref(),
        title: tags.title.as_deref(),
        album: tags.album.as_deref(),
        duration_ms: Some(duration_ms),
        file_size: Some(file_size),
    })
    .into_string();

    let title = tags
        .title
        .clone()
        .unwrap_or_else(|| radice_del_nome(&file.path));
    let artist = tags
        .artist
        .clone()
        .unwrap_or_else(|| rymembra_domain::album::UNKNOWN_ARTIST.to_owned());
    let album = tags
        .album
        .clone()
        .unwrap_or_else(|| rymembra_domain::album::UNKNOWN_ALBUM.to_owned());

    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artist),
        title: Some(&title),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, &file.path);

    // L'esito si spacca in due invece di finire in un `.ok()`: quel `.ok()`
    // buttava via l'unica spiegazione che ci fosse di una copertina mancante, e
    // chi guardava l'esito della scansione vedeva un brano senza immagine senza
    // nessun modo di sapere se il file non ne avesse una o se il disco fosse
    // pieno.
    let (cover, copertina_persa) = match tags.cover.as_ref() {
        None => (None, None),
        Some(incorporata) => match covers.store(&incorporata.data, CoverSource::Tag) {
            Ok(salvata) => (Some(salvata), None),
            Err(err) => (None, Some(err)),
        },
    };

    Ok(TrackRow {
        path: file.path.clone(),
        track_key,
        content_key: Some(content_key),
        title,
        artist,
        album,
        album_artist: tags.album_artist,
        album_key,
        year: tags.year.map(i64::from),
        track_number: tags.track_number.map(i64::from),
        disc_number: tags.disc_number.map(i64::from),
        genre: tags.genre,
        duration_ms,
        bpm: tags.bpm,
        musical_key: tags.musical_key,
        comment: tags.comment,
        lyrics: tags.lyrics,
        cover,
        copertina_persa,
        bitrate: tags.bitrate.map(i64::from),
        sample_rate: tags.sample_rate.map(i64::from),
        channels: tags.channels.map(i64::from),
        codec: tags.codec,
        file_size,
        date_modified: file.modified_ms,
        replaygain_track_db: tags.replaygain_track_db.map(f64::from),
        replaygain_album_db: tags.replaygain_album_db.map(f64::from),
        mb_recording_id: tags.mb_recording_id,
        mb_release_group_id: tags.mb_release_group_id,
        mb_release_id: tags.mb_release_id,
        immagine: None,
        inizio_cd: None,
        fine_cd: None,
    })
}

/// La radice del nome di un file, per il titolo di ripiego.
///
/// Quella del percorso, com'è: per un file del disco è il nome che l'utente
/// vede in Esplora. Per un file del cloud il nome nel percorso è un segmento
/// **codificato** (`Live %7C 1977`), e il titolo di ripiego è il nome com'era
/// sul servizio (`Live | 1977`): vedi [`rymembra_domain::nuvola`].
fn radice_del_nome(path: &str) -> String {
    let radice = file_stem(path);
    if rymembra_domain::nuvola::e_nuvola(path) {
        rymembra_domain::nuvola::decodifica_segmento(radice)
    } else {
        radice.to_owned()
    }
}

/// La riga **provvisoria** di un file che entra in libreria prima di essere
/// letto: la prima delle due passate di una radice del cloud.
///
/// # Da dove viene ogni campo
///
/// - **Dal servizio**, quando l'ha detto nell'elenco ([`MusicFiles::tag_noti`]:
///   la faccetta `audio` di OneDrive): titolo, interpreti, album, genere, anno,
///   numeri, durata.
/// - **Dai ripieghi di sempre** dove il servizio tace: gli stessi di
///   [`read_track`] per un file senza tag — la radice del nome come titolo,
///   «Artista sconosciuto», «Album sconosciuto». Un brano che dopo la lettura
///   vera resta senza tag ha così la stessa faccia di prima, e non cambia nome
///   sotto gli occhi di nessuno.
/// - **Dalla camminata**: dimensione e data, esattamente quelle che il piano
///   confronterà la volta dopo.
///
/// La chiave di contenuto **non c'è** (`None`, cioè `NULL`): non la si
/// inventa dai dati dell'elenco, perché è l'identità del file e la decidono i
/// suoi tag. Ed è proprio il `NULL` a far rileggere la riga: [`known_tracks`]
/// dà a una riga senza chiave la data `-1`, il piano la vede cambiata, e la
/// seconda passata la completa come un file ritaggato — stessa riga, stessi
/// ascolti, stesse playlist.
pub(super) fn riga_provvisoria(file: &DiscoveredFile, noti: Option<TagNoti>) -> TrackRow {
    use rymembra_domain::album::{UNKNOWN_ALBUM, UNKNOWN_ARTIST};
    let noti = noti.unwrap_or_default();
    let pulito = |valore: Option<String>| crate::metadata::clean(valore.as_deref());
    let title = pulito(noti.titolo).unwrap_or_else(|| radice_del_nome(&file.path));
    let artist = pulito(noti.artista).unwrap_or_else(|| UNKNOWN_ARTIST.to_owned());
    let album = pulito(noti.album).unwrap_or_else(|| UNKNOWN_ALBUM.to_owned());
    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artist),
        title: Some(&title),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, &file.path);
    TrackRow {
        path: file.path.clone(),
        track_key,
        content_key: None,
        title,
        artist,
        album,
        album_artist: pulito(noti.artista_album),
        album_key,
        year: noti.anno.map(i64::from),
        track_number: noti.traccia.map(i64::from),
        disc_number: noti.disco.map(i64::from),
        genre: pulito(noti.genere),
        duration_ms: noti
            .durata_ms
            .and_then(|durata| i64::try_from(durata).ok())
            .unwrap_or(0),
        bpm: None,
        musical_key: None,
        comment: None,
        lyrics: None,
        cover: None,
        copertina_persa: None,
        bitrate: None,
        sample_rate: None,
        channels: None,
        codec: None,
        file_size: i64::try_from(file.size_bytes).unwrap_or(i64::MAX),
        date_modified: file.modified_ms,
        replaygain_track_db: None,
        replaygain_album_db: None,
        mb_recording_id: None,
        mb_release_group_id: None,
        mb_release_id: None,
        immagine: None,
        inizio_cd: None,
        fine_cd: None,
    }
}

/// Le righe provvisorie della libreria: quelle del cloud ancora senza chiave
/// di contenuto, cioè mai lette (vedi [`riga_provvisoria`]).
///
/// Il cloud nella domanda non è un dettaglio: anche la migrazione 019 ha
/// lasciato righe senza chiave, ma sono righe del disco con anni di ascolti, e
/// nessuna regola delle righe provvisorie deve mai valere per loro.
pub(super) fn righe_provvisorie(
    connection: &Connection,
) -> Result<std::collections::HashSet<i64>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id FROM tracks
              WHERE content_key IS NULL AND path >= 'nuvola://' AND path < 'nuvola:/0'",
        )
        .map_err(|err| db_error("elenco delle righe provvisorie", &err))?;
    let righe = statement
        .query_map([], |riga| riga.get::<_, i64>(0))
        .map_err(|err| db_error("elenco delle righe provvisorie", &err))?;
    righe
        .collect::<Result<_, _>>()
        .map_err(|err| db_error("elenco delle righe provvisorie", &err))
}

/// Toglie le righe provvisorie il cui file il lettore ha rifiutato per com'è
/// fatto: come un file che non era mai entrato.
///
/// # Solo se nessuno ci ha messo le mani
///
/// Una riga provvisoria si può suonare (la durata la scrive il primo ascolto),
/// votare, mettere in playlist. Toglierla allora porterebbe via in cascata
/// quel che l'utente ci ha messo sopra, per un file che magari si legge con la
/// prossima versione del lettore. Quindi si toglie solo una riga ancora com'era
/// entrata: senza chiave di contenuto (nessuna lettura l'ha completata nel
/// frattempo), senza ascolti, voto, preferito, correzioni né playlist. Le
/// altre restano — il rifiuto si ricorda lo stesso, e la riga non si rilegge
/// finché il file non cambia.
///
/// Identificativo **e** percorso, come [`remove_tracks`]: fra la lettura e la
/// scrittura il lucchetto era libero.
pub(super) fn togli_provvisorie(
    tx: &Transaction<'_>,
    righe: &[(i64, String)],
) -> Result<usize, AppError> {
    let mut statement = tx
        .prepare_cached(
            "DELETE FROM tracks
              WHERE id = ?1 AND path = ?2 AND content_key IS NULL
                AND play_count = 0 AND rating = 0 AND liked = 0
                AND NOT EXISTS (SELECT 1 FROM play_history WHERE track_id = ?1)
                AND NOT EXISTS (SELECT 1 FROM playlist_tracks WHERE track_id = ?1)
                AND NOT EXISTS (SELECT 1 FROM track_overrides WHERE track_id = ?1)",
        )
        .map_err(|err| db_error("rimozione di una riga provvisoria", &err))?;
    let mut tolte = 0;
    for (id, path) in righe {
        tolte += statement
            .execute(rusqlite::params![id, path])
            .map_err(|err| db_error("rimozione di una riga provvisoria", &err))?;
    }
    Ok(tolte)
}

/// Le righe già in libreria, nella forma che serve a decidere.
///
/// Ordinate per identificativo: l'ordine di arrivo detta l'ordine del piano, e
/// un piano da mostrare all'utente non deve cambiare fra due aperture.
///
/// # Le righe senza `content_key` si rileggono
///
/// La migrazione 019 ha lasciato `content_key` a NULL sulle righe che avevano
/// già una correzione, in attesa della «prima riscansione che le rilegge». Ma una
/// riscansione rilegge solo i file cambiati, e un file corretto a mano di solito
/// non lo tocca più nessuno: la chiave restava NULL per sempre. E il trigger
/// della 023, che mette da parte le correzioni di un file che sparisce, le salta
/// proprio quando la chiave manca — cioè perdeva le correzioni più vecchie, che
/// sono quelle che contano di più. Per quelle righe la data qui vale `-1`, che
/// nessun file ha: il piano le vede cambiate e le rilegge una volta, e da lì in
/// poi la data è quella vera.
///
/// La stessa sentinella la mette [`rileggi_tutti_i_tag`], su ogni riga.
///
/// La dimensione viaggia accanto alla data: vedi
/// [`rymembra_domain::scan_plan::KnownTrack::size_bytes`]. Una dimensione negativa
/// in colonna non è una dimensione, e vale «non si sa».
pub fn known_tracks(connection: &Connection) -> Result<Vec<KnownTrack>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, path, CASE WHEN content_key IS NULL THEN -1 ELSE date_modified END,
                    file_size
               FROM tracks WHERE path IS NOT NULL ORDER BY id",
        )
        .map_err(|err| db_error("elenco dei brani noti", &err))?;
    let rows = statement
        .query_map([], |row| {
            Ok(KnownTrack {
                id: row.get(0)?,
                path: row.get(1)?,
                modified_ms: row.get(2)?,
                size_bytes: row
                    .get::<_, Option<i64>>(3)?
                    .and_then(|size| u64::try_from(size).ok()),
            })
        })
        .map_err(|err| db_error("elenco dei brani noti", &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("elenco dei brani noti", &err))
}

/// Le chiavi di **contenuto** delle righe date, nello stesso ordine.
///
/// Servono a una cosa sola: chiedere a `match_moved_tracks` se una di queste
/// righe è il file che è appena comparso altrove. La domanda va fatta sulla
/// chiave che non cambia quando l'utente corregge un brano — vedi
/// [`ContentKey`] — altrimenti proprio i brani a cui qualcuno ha dedicato
/// attenzione sono quelli che, spostandosi, si duplicano.
///
/// `COALESCE` perché la migrazione 019 ha lasciato `content_key` a NULL sulle
/// righe che avevano già una sovrascrittura: di quelle i tag grezzi non si
/// conoscono finché non le si rilegge, e fino ad allora vale `track_key`, che è
/// esattamente come si comportavano prima.
///
/// Una riga sparita nel frattempo si salta invece di far fallire tutto: fra il
/// calcolo del piano e questo momento un'altra scrittura può averla tolta, e
/// non è un motivo per rinunciare alla scansione.
pub(super) fn content_keys_of(
    connection: &Connection,
    ids: &[i64],
) -> Result<Vec<(i64, String)>, AppError> {
    let mut statement = connection
        .prepare("SELECT COALESCE(content_key, track_key) FROM tracks WHERE id = ?1")
        .map_err(|err| db_error("chiave di un brano", &err))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        match statement.query_row([id], |row| row.get::<_, String>(0)) {
            Ok(key) => out.push((*id, key)),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(err) => return Err(db_error("chiave di un brano", &err)),
        }
    }
    Ok(out)
}

/// Registra la copertina di una riga, se ce n'è una.
///
/// `INSERT OR IGNORE`: la stessa impronta arriva da dodici brani dello stesso
/// album, e la seconda volta non c'è niente da registrare. Va fatto **prima**
/// della riga di `tracks` che la riferisce, altrimenti la chiave esterna cade.
fn insert_cover(tx: &Transaction<'_>, row: &TrackRow, now: i64) -> Result<(), AppError> {
    let Some(cover) = row.cover.as_ref() else {
        return Ok(());
    };
    let mut statement = tx
        .prepare_cached(
            "INSERT OR IGNORE INTO cover_art
               (hash, mime_type, width, height, byte_size, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'tag', ?6)",
        )
        .map_err(|err| db_error("registrazione di una copertina", &err))?;
    statement
        .execute(rusqlite::params![
            cover.hash,
            cover.mime_type,
            cover.width,
            cover.height,
            i64::try_from(cover.byte_size).unwrap_or(0),
            now,
        ])
        .map(|_| ())
        .map_err(|err| db_error("registrazione di una copertina", &err))
}

/// I campi che un brano prende dal file, nell'ordine dei segnaposto: i ventotto
/// di sempre, la chiave di contenuto (`?29`), quel che l'istruzione aggiunge in
/// coda (`?30`) e le tre colonne dei brani d'immagine (`?31`–`?33`).
///
/// Un elenco solo per inserimento e aggiornamento. Scritto due volte, prima o
/// poi le due copie divergono su una colonna, e il risultato è un campo che si
/// popola inserendo e resta al valore vecchio aggiornando — cioè un brano
/// ritaggato che in libreria mostra il tag di prima.
///
/// La forma a quattro argomenti prende la copertina da fuori invece che dalla
/// riga: serve a [`update_track`] quando rimette quella dell'arricchimento, che
/// sta in `cover_art` ma non nel file, e quindi non ha uno `StoredCover`.
macro_rules! esegui_coi_campi {
    ($statement:expr, $row:expr, $coda:expr) => {{
        esegui_coi_campi!(
            $statement,
            $row,
            $row.cover.as_ref().map(|c| c.hash.as_str()),
            $coda
        )
    }};
    ($statement:expr, $row:expr, $copertina:expr, $coda:expr) => {{
        let cover_art_hash: Option<&str> = $copertina;
        $statement.execute(rusqlite::params![
            $row.path,
            $row.track_key,
            $row.title,
            $row.artist,
            $row.album,
            $row.album_artist,
            $row.album_key,
            $row.year,
            $row.track_number,
            $row.disc_number,
            $row.genre,
            $row.duration_ms,
            $row.bpm,
            $row.musical_key,
            $row.comment,
            $row.lyrics,
            cover_art_hash,
            $row.bitrate,
            $row.sample_rate,
            $row.channels,
            $row.codec,
            $row.file_size,
            $row.date_modified,
            $row.replaygain_track_db,
            $row.replaygain_album_db,
            $row.mb_recording_id,
            $row.mb_release_group_id,
            $row.mb_release_id,
            // In coda e non accanto a `track_key`, dove starebbe bene: mettercela
            // lì avrebbe rinumerato a mano tutti i segnaposti che seguono, in
            // due query, ed è il genere di modifica che ne sbaglia uno solo e
            // lo sbaglia in silenzio.
            $row.content_key,
            $coda,
            // Dopo la coda per la stessa ragione di `content_key`: aggiunte
            // in fondo, nessun segnaposto di prima cambia numero.
            $row.immagine,
            $row.inizio_cd,
            $row.fine_cd,
        ])
    }};
}

/// Inserisce un brano nuovo.
pub(super) fn insert_track(tx: &Transaction<'_>, row: &TrackRow, now: i64) -> Result<(), AppError> {
    insert_cover(tx, row, now)?;
    let mut statement = tx
        .prepare_cached(
            "INSERT INTO tracks (
               path, track_key, title, artist, album, album_artist, album_key,
               year, track_number, disc_number, genre, duration_ms, bpm,
               musical_key, comment, lyrics, cover_art_hash, bitrate,
               sample_rate, channels, codec, file_size, date_modified,
               replaygain_track_db, replaygain_album_db, mb_recording_id,
               mb_release_group_id, mb_release_id, content_key, date_added,
               source, immagine, inizio_cd, fine_cd
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
               ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
               ?27, ?28, ?29, ?30, 'scan', ?31, ?32, ?33
             )",
        )
        .map_err(|err| db_error("inserimento di un brano", &err))?;
    esegui_coi_campi!(statement, row, now)
        .map(|_| ())
        .map_err(|err| db_error("inserimento di un brano", &err))
}

/// Riscrive una riga esistente con quel che il file dice adesso.
///
/// **Non tocca** `play_count`, `last_played_at`, `rating`, `liked`, `liked_at`,
/// `stats_updated_at` né `date_added`: sono ciò che l'utente ha costruito
/// ascoltando, e non stanno nei file. Un `INSERT OR REPLACE` — che sarebbe la
/// scrittura più corta — le azzererebbe tutte, e in più cambierebbe
/// l'identificativo della riga, togliendo il brano dalle playlist in cui sta.
///
/// Il percorso si riscrive sempre, anche quando l'aggiornamento nasce solo da
/// una data di modifica cambiata: se sul disco il file ora si chiama `A.MP3` e
/// nel database sta come `a.mp3`, questa è l'occasione in cui la deriva si
/// chiude invece di accumularsi.
///
/// **Riscrive titolo, interprete e album dai tag del file**, e con essi le tre
/// chiavi. Quel che l'utente ha corretto a mano sta in `track_overrides` e va
/// rimesso sopra subito dopo, con [`crate::incerti::riapplica`]: non lo fa
/// questa funzione perché la scansione è solo uno dei modi in cui una riga viene
/// riscritta dai tag, e la regola è la stessa per tutti — chi riscrive rimette.
///
/// Di quelle tre, `content_key` è l'unica che `riapplica` **non** rimetterà a
/// posto, e non deve: è l'identità del file, non della riga, e questo è il solo
/// punto del programma in cui si scrive. Vedi il `//!` di
/// [`crate::incerti`].
///
/// # Lo strato dell'arricchimento, invece, lo rimette lei
///
/// Prima di scrivere: [`crate::incerti::arricchimento_sopra_i_tag`] guarda,
/// campo per campo, se il file sotto ai campi arricchiti è ancora quello di
/// allora, e restituisce la riga da scrivere con sopra quel che vale ancora.
/// Dentro e non dopo, a differenza della correzione a mano, perché così la riga
/// passa dai tag al risultato con un `UPDATE` solo e `track_key` cambia al più
/// una volta — vedi là il perché.
///
/// Restituisce la `track_key` scritta, che è quella da passare a
/// [`crate::incerti::riapplica_sopra_i_tag`]: la chiave dei tag **più**
/// l'arricchimento rimesso. Passarle quella dei soli tag, com'era prima,
/// riporterebbe un brano arricchito all'identità del suo file un istante dopo
/// averla corretta.
pub(super) fn update_track(
    tx: &Transaction<'_>,
    track_id: i64,
    row: &TrackRow,
    now: i64,
) -> Result<String, AppError> {
    insert_cover(tx, row, now)?;
    let sopra = crate::incerti::arricchimento_sopra_i_tag(tx, track_id, row)?;
    let (finale, copertina_arricchita) = match sopra.as_ref() {
        Some(sopra) => (&sopra.riga, sopra.copertina.as_deref()),
        None => (row, None),
    };
    let copertina = copertina_arricchita.or_else(|| finale.cover.as_ref().map(|c| c.hash.as_str()));
    let mut statement = tx
        .prepare_cached(
            "UPDATE tracks SET
               path = ?1, title = ?3, artist = ?4, album = ?5,
               -- Non su un brano corretto: vedi `incerti::riapplica_sopra_i_tag`.
               track_key = CASE WHEN EXISTS
                             (SELECT 1 FROM track_overrides WHERE track_id = ?30)
                           THEN track_key ELSE ?2 END,
               album_artist = ?6, album_key = ?7, year = ?8, track_number = ?9,
               disc_number = ?10, genre = ?11, duration_ms = ?12, bpm = ?13,
               musical_key = ?14, comment = ?15, lyrics = ?16,
               cover_art_hash = ?17, bitrate = ?18, sample_rate = ?19,
               channels = ?20, codec = ?21, file_size = ?22, date_modified = ?23,
               replaygain_track_db = ?24, replaygain_album_db = ?25,
               mb_recording_id = ?26, mb_release_group_id = ?27,
               mb_release_id = ?28, content_key = ?29,
               immagine = ?31, inizio_cd = ?32, fine_cd = ?33
             WHERE id = ?30",
        )
        .map_err(|err| db_error("aggiornamento di un brano", &err))?;
    esegui_coi_campi!(statement, finale, copertina, track_id)
        .map_err(|err| db_error("aggiornamento di un brano", &err))?;
    Ok(finale.track_key.clone())
}

/// Fa rileggere i tag di ogni file alla prossima scansione.
///
/// # Perché serve, ora che il piano guarda anche la dimensione
///
/// Perché nemmeno data e dimensione insieme vedono tutto. Un taggatore che ferma
/// la data e riscrive dentro lo spazio di riempimento di un ID3v2 lascia il file
/// identico a sé per entrambe, e la scansione non ha modo di accorgersene; e ci
/// sono le righe scritte da una versione che leggeva meno — la normalizzazione
/// NFC dei tag è arrivata con la 3.0, e un brano taggato su un Mac resta
/// introvabile dalla ricerca finché non lo si rilegge. Per questi casi c'è un
/// gesto, e lo fa l'utente dalle Impostazioni.
///
/// # Come
///
/// Con la stessa sentinella che [`known_tracks`] usa per le righe senza
/// `content_key`: `date_modified = -1`, una data che nessun file ha. Il piano
/// vede ogni riga cambiata e la rilegge, e la rilettura riscrive la data vera.
/// Niente bandiere a parte e niente da ricordarsi di spegnere: una scansione
/// interrotta a metà lascia la sentinella solo sulle righe che non ha fatto in
/// tempo a rileggere, che sono esattamente quelle da rileggere la volta dopo.
///
/// La rilettura è quella di sempre, con tutto quel che si porta dietro: le
/// correzioni a mano si rimettono, e l'arricchimento pure, dove il file non
/// l'ha smentito — vedi [`update_track`].
///
/// Restituisce quante righe aspettano la rilettura. I brani di catalogo non
/// hanno un file, e non si contano.
///
/// # E le due memorie della scansione si svuotano
///
/// La memoria dei rifiuti (`file_illeggibili`, migrazione 032) e la cache dei
/// fogli `.cue` (`fogli_cue`, migrazione 034): sono le due cose per cui una
/// scansione **non** riapre un file, e chi chiede di rileggere tutto chiede
/// proprio di riaprirli. Un file rifiutato da un lettore di allora, o un foglio
/// letto male da un parser di allora, tornano a essere guardati.
///
/// # E gli album si rifanno da capo
///
/// Chi chiede di rileggere tutto sta dicendo che la libreria in tabella non gli
/// torna, e la risposta giusta comprende gli aggregati: la prossima scansione li
/// ricostruisce per intero invece che per le sole chiavi toccate — vedi
/// [`chiedi_ricostruzione_intera`]. Toccherebbe comunque quasi tutto, ma
/// «quasi» non basta a chi vuole vedere sparire un album fantasma.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn rileggi_tutti_i_tag(connection: &Connection) -> Result<usize, AppError> {
    connection
        .execute(
            "UPDATE tracks SET date_modified = -1
              WHERE path IS NOT NULL AND date_modified <> -1",
            [],
        )
        .map_err(|err| db_error("richiesta di rilettura dei tag", &err))?;
    connection
        .execute_batch("DELETE FROM file_illeggibili; DELETE FROM fogli_cue;")
        .map_err(|err| db_error("richiesta di rilettura dei tag", &err))?;
    chiedi_ricostruzione_intera(connection)?;
    connection
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE path IS NOT NULL AND date_modified = -1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|quante| usize::try_from(quante).unwrap_or(0))
        .map_err(|err| db_error("richiesta di rilettura dei tag", &err))
}

/// Toglie le righe indicate.
///
/// # Niente lapidi di sincronizzazione, qui
///
/// `sync_tombstones` serve a far viaggiare una **cancellazione decisa
/// dall'utente**: senza, l'altro dispositivo vedrebbe solo «a me manca un brano»
/// e lo rimanderebbe indietro. Una rimozione da scansione non è quella cosa: è
/// «questo file non è su questo disco», che è un fatto locale.
///
/// Scriverne una qui vorrebbe dire che staccare un disco esterno e riscansionare
/// propaga al telefono la cancellazione di trecento brani che nessuno ha chiesto
/// di cancellare — e le lapidi, per come funzionano, impedirebbero pure di
/// riaverli riattaccando il disco.
///
/// # Perché anche il percorso, e non il solo identificativo
///
/// Perché fra il momento in cui il piano ha deciso «questa riga va tolta» e il
/// momento in cui la si toglie, la libreria è **stata di qualcun altro**: la
/// scansione lascia il deposito fra una fase e l'altra apposta, e in quel mezzo
/// una sincronizzazione o un'importazione possono aver cancellato quella riga e
/// inserito qualcos'altro. SQLite riusa gli identificativi liberi: la riga 412
/// che il piano voleva togliere può non essere più la riga 412 che c'è adesso.
///
/// Il percorso rende la cancellazione **una domanda sulla riga che si è vista**,
/// non sul numero che aveva. Se non combacia più non si toglie niente, il
/// conteggio lo dice, e la passata dopo — che guarda di nuovo il disco — decide
/// da capo con i fatti aggiornati.
///
/// Le coppie arrivano da `ScanPlan::to_remove`, che porta già entrambi.
pub(super) fn remove_tracks(
    tx: &Transaction<'_>,
    righe: &[(i64, &str)],
) -> Result<usize, AppError> {
    // `COALESCE(path, fonte_url)`: per un file l'identità è il percorso, per un
    // brano di catalogo è l'indirizzo — in tutti e due i casi la stringa che
    // quella riga non condivide con nessun'altra. Un confronto sul solo `path`
    // qui non toglierebbe **mai** un brano di catalogo, perché in SQL `NULL = ?`
    // non è vero nemmeno quando il parametro è nullo.
    let mut statement = tx
        .prepare_cached("DELETE FROM tracks WHERE id = ?1 AND COALESCE(path, fonte_url) = ?2")
        .map_err(|err| db_error("rimozione di un brano", &err))?;
    let mut removed = 0;
    for (id, path) in righe {
        removed += statement
            .execute(rusqlite::params![id, path])
            .map_err(|err| db_error("rimozione di un brano", &err))?;
    }
    Ok(removed)
}

/// Quel che una cancellazione decisa dall'utente ha portato via.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cancellazione {
    /// Quante righe sono sparite davvero.
    pub tolti: usize,
    /// Quanti di quei file stanno ancora sotto una cartella sorvegliata.
    ///
    /// Cioè: quanti torneranno alla prossima scansione. Non è una diagnostica,
    /// è la cosa da dire a chi ha appena premuto «Togli dalla libreria» e si
    /// aspetta che resti tolto.
    pub torneranno: usize,
}

/// Toglie dalla libreria i brani indicati. **Non tocca i file.**
///
/// # Perché una funzione a parte, e non [`remove_tracks`] reso pubblico
///
/// Perché le due rimozioni non sono la stessa cosa detta due volte. Quella della
/// scansione è una constatazione — «questo file non c'è più su questo disco» —
/// e arriva già dentro la transazione di una passata, in mezzo ad altre sei
/// fasi; questa è una decisione, arriva da sola, e deve portarsi dietro tutto
/// quel che una rimozione isolata richiede: la transazione, gli aggregati, e il
/// conto di quel che tornerà. `remove_tracks` resta la primitiva di tutte e due.
///
/// # Quel che fanno lo schema e i trigger, e non si scrive qui
///
/// Le chiavi esterne sono accese su ogni connessione (vedi `db::configure`), e
/// con la riga se ne vanno in cascata `playlist_tracks`, `play_history`,
/// `enrich_undo`, `track_overrides`, `track_impronta`, `track_intensita`,
/// `brano_vicino` — nei due versi — `brano_vicino_stato`, `settimana_brano` e
/// `track_meta_arricchita`. Il
/// trigger `tracks_fts_delete` toglie il brano dall'indice di ricerca, e
/// `correzioni_restano_orfane` mette da parte le correzioni a mano sotto la
/// `content_key`, dove scadono da sole dopo novanta giorni: se il file viene
/// cancellato davvero non le rivedrà nessuno, e se invece torna con una
/// scansione è giusto che tornino con lui.
///
/// Le tabelle della sincronia non si toccano nemmeno: `sincronia::allinea` le
/// rimette in pari con la libreria all'inizio di ogni passata, ed è pigra
/// apposta perché chi scrive non debba ricordarsene.
///
/// # Niente lapidi di sincronizzazione
///
/// Per la stessa ragione per cui non ne scrive la scansione, detta dall'altro
/// lato: `lapidi.brani` viaggia e si fonde, ma **nessuno la consulta** per
/// impedire il ritorno di un brano — vedi `sincronia::togli_playlist` e
/// `backup::dal_salvataggio`. Una riga di brano non la crea mai una sincronia,
/// la crea la scansione leggendo un file che c'è; se l'altro dispositivo quel
/// file ce l'ha, deve tenerselo. Scriverne una qui sarebbe stato morto.
///
/// # Perché si legge dentro la transazione
///
/// Perché così i percorsi con cui si cancella sono quelli che ci sono **adesso**:
/// la guardia `AND path = ?2` di [`remove_tracks`] esiste per il caso opposto —
/// un piano deciso prima, eseguito dopo, con in mezzo altre scritture — e qui
/// non c'è nessun «prima». Il conto dei tolti coincide quindi con quello delle
/// righe lette, ed è ciò che rende affidabile `torneranno`: si guarda solo quel
/// che è stato tolto davvero.
///
/// # Errori
///
/// `db.queryFailed` se la lettura o la scrittura falliscono.
pub fn cancella_brani(
    connection: &mut Connection,
    ids: &[i64],
    sorvegliate: &[String],
    rules: PathRules,
) -> Result<Cancellazione, AppError> {
    if ids.is_empty() {
        return Ok(Cancellazione::default());
    }
    // `Immediate` perché qui si legge e poi si scrive: vedi
    // [`transazione_di_scrittura`].
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("cancellazione di brani", &err))?;
    // `Transaction` si deferenzia a `Connection`: la lettura vede quel che la
    // transazione vede, cioè il database un istante prima della cancellazione.
    // Due colonne e non il riepilogo intero: qui serve l'identità della riga,
    // che per un file è il percorso e per un brano di catalogo l'indirizzo.
    //
    // A pezzi di [`PEZZO`], e non con un'unica `IN (…)`: «seleziona tutto» su
    // una libreria di quarantamila brani sono quarantamila parametri, e SQLite
    // ne accetta 32 766 — la cancellazione falliva con un `db.queryFailed` che
    // non diceva niente a nessuno.
    let mut percorsi: Vec<(i64, String, bool)> = Vec::new();
    for pezzo in ids.chunks(PEZZO) {
        let sql = format!(
            "SELECT id, COALESCE(path, fonte_url), path IS NOT NULL
               FROM tracks WHERE id IN ({})",
            segnaposti(pezzo.len())
        );
        let mut statement = tx
            .prepare_cached(&sql)
            .map_err(|err| db_error("cancellazione di brani", &err))?;
        let righe = statement
            .query_map(rusqlite::params_from_iter(pezzo), |riga| {
                Ok((riga.get(0)?, riga.get(1)?, riga.get::<_, i64>(2)? != 0))
            })
            .map_err(|err| db_error("cancellazione di brani", &err))?;
        for riga in righe {
            percorsi.push(riga.map_err(|err| db_error("cancellazione di brani", &err))?);
        }
    }
    if percorsi.is_empty() {
        // Niente da togliere non è un guasto: è il clic su un elenco che è
        // cambiato sotto. Si esce senza rifare gli aggregati, che su una
        // libreria grande costano una lettura intera per niente.
        drop(tx);
        return Ok(Cancellazione::default());
    }
    let coppie: Vec<(i64, &str)> = percorsi
        .iter()
        .map(|(id, identita, _)| (*id, identita.as_str()))
        .collect();
    // Le chiavi **prima** di togliere: dopo, le righe non ci sono più a dire
    // di quale album e di quale artista erano.
    let mut toccati = Toccati::nessuno();
    let quali: Vec<i64> = coppie.iter().map(|(id, _)| *id).collect();
    toccati.segna_brani(&tx, &quali)?;
    let tolti = remove_tracks(&tx, &coppie)?;
    // Gli aggregati nella stessa transazione: un album senza più brani e un
    // artista senza più album devono sparire insieme alle righe, non alla
    // prossima scansione. È il passo 7 di `Scan::run_su`, fatto qui perché qui
    // non c'è una scansione che lo faccia dopo — e fatto solo sugli album e
    // sugli artisti di questi brani.
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("cancellazione di brani", &err))?;

    // Solo i file: un brano di catalogo non sta sotto nessuna cartella e
    // nessuna scansione lo riporterà indietro. Contarlo qui vorrebbe dire
    // avvisare «tornerà alla prossima scansione» di qualcosa che non torna.
    let torneranno = percorsi
        .iter()
        .filter(|(_, percorso, e_un_file)| {
            *e_un_file
                && sorvegliate
                    .iter()
                    .any(|radice| is_under(percorso, radice, rules))
        })
        .count();
    Ok(Cancellazione { tolti, torneranno })
}

/// Toglie le rimozioni che una scansione aveva trattenuto, e che qualcuno ha
/// confermato. **Non cammina il disco.**
///
/// # Esattamente quelle
///
/// La conferma arriva su un elenco che la persona ha letto — quante, da quali
/// cartelle — e deve togliere quello: né una riga di più né una di meno.
/// Ricamminare il disco per «rifare la scansione con il permesso» vorrebbe dire
/// togliere un elenco diverso da quello confermato, e magari un elenco che nel
/// frattempo è cresciuto perché la share è caduta del tutto.
///
/// Ogni riga si toglie per identificativo **e** percorso, come in ogni
/// rimozione della scansione: se fra la domanda e la risposta qualcuno ha
/// cambiato quella riga, non la si riconosce e non la si tocca. Il numero
/// restituito dice quante se ne sono tolte davvero.
///
/// # Quel che resta
///
/// Quel che la migrazione 028 mette da parte: ascolti, voto, preferito e posto
/// nelle playlist di ogni riga tolta restano per novanta giorni sotto la
/// `content_key`, e tornano se il file ricompare a una scansione. Una conferma
/// data per sbaglio — «ah, il disco era solo staccato» — costa quindi una
/// riscansione col disco attaccato, non la storia dei brani.
///
/// # Errori
///
/// `db.*` se la scrittura fallisce; in quel caso non si è tolto niente.
pub fn applica_rimozioni(
    connection: &mut Connection,
    elenco: &[RimozioneInAttesa],
) -> Result<usize, AppError> {
    if elenco.is_empty() {
        return Ok(0);
    }
    let tx = transazione_di_scrittura(connection)
        .map_err(|err| db_error("rimozioni confermate", &err))?;
    let coppie: Vec<(i64, &str)> = elenco
        .iter()
        .map(|rimozione| (rimozione.id, rimozione.path.as_str()))
        .collect();
    let mut toccati = Toccati::nessuno();
    let quali: Vec<i64> = coppie.iter().map(|(id, _)| *id).collect();
    toccati.segna_brani(&tx, &quali)?;
    let tolte = remove_tracks(&tx, &coppie)?;
    // Gli aggregati nella stessa transazione, come in `cancella_brani`: qui non
    // c'è una scansione che li rifaccia dopo.
    ricostruisci_aggregati(&tx, &toccati)?;
    tx.commit()
        .map_err(|err| db_error("rimozioni confermate", &err))?;
    Ok(tolte)
}

/// Le rimozioni in attesa, raggruppate per cartella sorvegliata.
///
/// Una voce per radice, nell'ordine delle radici, con il conto e i primi
/// `esempi` percorsi. Una riga che non sta sotto nessuna radice — non dovrebbe
/// succedere: il piano toglie solo quel che sta sotto una radice guardata —
/// finisce in una voce a sé con la radice vuota, invece di sparire dal conto
/// che la persona sta per confermare.
#[must_use]
pub fn riassumi_rimozioni(
    elenco: &[RimozioneInAttesa],
    roots: &[String],
    rules: PathRules,
    esempi: usize,
) -> Vec<RimozioniSottoUnaRadice> {
    let mut voci: Vec<RimozioniSottoUnaRadice> = Vec::new();
    for rimozione in elenco {
        let radice = roots
            .iter()
            .find(|root| is_under(&rimozione.path, root, rules))
            .map_or("", String::as_str);
        let voce = if let Some(posto) = voci.iter().position(|v| v.radice == radice) {
            voci.get_mut(posto)
        } else {
            voci.push(RimozioniSottoUnaRadice {
                radice: radice.to_owned(),
                quante: 0,
                esempi: Vec::new(),
            });
            voci.last_mut()
        };
        if let Some(voce) = voce {
            voce.quante = voce.quante.saturating_add(1);
            if voce.esempi.len() < esempi {
                voce.esempi.push(rimozione.path.clone());
            }
        }
    }
    // Nell'ordine delle radici, e la voce senza radice in fondo: è l'ordine in
    // cui la persona le ha scelte, ed è quello in cui le riconosce.
    voci.sort_by_key(|voce| {
        roots
            .iter()
            .position(|root| *root == voce.radice)
            .unwrap_or(usize::MAX)
    });
    voci
}
