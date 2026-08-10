//! Da un contenuto di Spotify già risolto a righe nel database.
//!
//! # La rete è già finita quando si arriva qui
//!
//! Questa funzione riceve un [`SpotifyContent`], cioè un valore. Non sa da dove
//! arrivi e non può andare a prenderlo: `aether-app` non dipende da
//! `aether-spotify`, e il verso della dipendenza è quel che garantisce che
//! nessuna transazione SQLite resti aperta durante una richiesta HTTP. È la
//! stessa regola che `aether-cloud` si è data per Drive, applicata qui.
//!
//! # Il piano è l'esecuzione, annullata
//!
//! Come per [`crate::import_legacy`]: [`plan`] fa l'importazione **vera** dentro
//! una transazione che viene abbandonata, e restituisce lo stesso rapporto che
//! restituirà [`import`]. Non è una seconda implementazione che prevede cosa
//! farebbe la prima: è la prima. Costa quanto l'importazione — qualche centinaio
//! di righe — e in cambio il piano non può annunciare una cosa e farne un'altra.
//!
//! Qui quel principio vale doppio, perché il piano è l'unico momento in cui
//! l'utente vede **prima** di decidere due cose che non potrebbe più scoprire
//! dopo: quali brani non ha, e se la playlist che sta per importare ne
//! sostituirà una che esiste già.

use std::collections::HashMap;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::{PlaylistKey, TrackKey, TrackKeyInput};
use aether_domain::spotify::{SpotifyContent, SpotifyKind, SpotifyTrack};
use aether_domain::spotify_plan::{Gradino, LibraryTrack, plan_spotify_import};
use rusqlite::{Connection, Transaction};

use crate::library::{db_error, now_ms, rebuild_aggregates};

/// Un brano che su Spotify c'era e in libreria no.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingTrack {
    /// La posizione che aveva nell'elenco di Spotify, da 1.
    pub position: usize,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: Option<String>,
    /// L'album.
    pub album: Option<String>,
}

/// Un elenco arrivato più corto di quanto Spotify dichiari.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Truncation {
    /// Quanti brani sono arrivati.
    pub read: u32,
    /// Quanti ne dichiara Spotify.
    pub expected: u32,
}

/// Cosa l'importazione porterebbe, o ha portato.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyImportReport {
    /// Che cosa si è importato: `brano`, `album`, `playlist`, `artista`, e da
    /// [`crate::import_account`] anche `preferiti`.
    pub kind: String,
    /// Da dove sono arrivati questi brani.
    ///
    /// Da un link è il livello del lettore keyless — `pathfinder`, `embed` o
    /// `oembed`; da un account è la strada — `api` o `archivio`. Una domanda
    /// sola, due vocabolari, e in nessuno dei due è l'identificativo del
    /// contenuto: quello è [`Self::source_id`], e il nome quasi uguale è la
    /// ragione per cui questa riga esiste.
    pub source: String,
    /// L'identificativo del contenuto su Spotify.
    ///
    /// Lo stesso che finisce in `spotify_wanted.source_id`, ed è la chiave con
    /// cui chi mostra la coda ritrova le righe di *questa* importazione.
    pub source_id: String,
    /// Il nome del contenuto su Spotify.
    pub title: String,
    /// Quanti brani sono stati letti da Spotify.
    pub resolved: usize,
    /// Quanti sono stati ritrovati in libreria.
    pub matched: usize,
    /// Quanti no.
    pub missing: usize,
    /// Ritrovati per ISRC: stessa registrazione, senza guardare i nomi.
    ///
    /// Oggi quasi sempre zero, perché Pathfinder l'ISRC non lo manda più (vedi
    /// [`Self::isrc_written`]). Vale per i file già taggati bene e per le
    /// importazioni fatte quando quel campo c'era ancora.
    pub matched_isrc: usize,
    /// Ritrovati con artista, titolo e album coincidenti.
    pub matched_exact: usize,
    /// Ritrovati con artista e titolo, album diverso.
    pub matched_by_title: usize,
    /// Ritrovati dopo aver tolto le decorazioni dai titoli.
    pub matched_stripped: usize,
    /// Quali brani mancano.
    ///
    /// È la voce più importante del rapporto, per la stessa ragione scritta in
    /// [`crate::import_legacy`]: sono l'unica cosa che l'utente non può
    /// ricostruire dopo, e vanno **dette**, non contate e basta.
    pub missing_tracks: Vec<MissingTrack>,
    /// La playlist creata o riempita.
    pub playlist_id: Option<i64>,
    /// Come si chiama.
    pub playlist_name: Option<String>,
    /// È stata creata adesso.
    pub playlist_created: bool,
    /// Esisteva già, e il suo contenuto è stato sostituito.
    pub playlist_replaced: bool,
    /// Quante voci ha adesso.
    pub playlist_entries: usize,
    /// Su quanti brani si è scritto l'identificativo dell'album Spotify.
    pub spotify_album_ids_written: usize,
    /// Su quanti si è scritto l'ISRC.
    ///
    /// Oggi è sempre zero: Pathfinder non espone più quel campo, verificato dal
    /// vivo ad agosto 2026 (vedi la nota in testa a `aether_spotify::pathfinder`).
    /// La colonna e questo conteggio restano perché il campo può tornare, e
    /// perché l'ISRC può arrivare anche dai tag dei file — da lì il percorso di
    /// scrittura è già aperto.
    pub isrc_written: usize,
    /// Quante righe di «desiderati» sono state scritte.
    pub wanted_rows: usize,
    /// Quanti brani scaricati in passato sono tornati nella loro playlist.
    ///
    /// Di solito zero durante un'importazione: i brani già arrivati vengono
    /// ritrovati dall'abbinamento e messi in playlist da lì. Conta quando la
    /// playlist era stata cancellata e ricreata, o quando il brano è entrato in
    /// libreria con dei tag che non combaciano più con la sua chiave.
    pub playlist_restored: usize,
    /// Quante righe di «desiderati» si sono chiuse perché il brano ormai c'è.
    ///
    /// Sono i brani che una vecchia importazione aspettava ancora e che nel
    /// frattempo sono arrivati — scaricati da un'altra playlist, o messi lì a
    /// mano. Senza questa chiusura la coda li riscaricherebbe accanto ai file
    /// che ci sono già.
    pub wanted_closed: usize,
    /// Se l'elenco è arrivato monco.
    pub truncated: Option<Truncation>,
}

/// Cosa succederebbe importando, senza scrivere niente.
///
/// Richiede `&mut` perché apre una transazione per davvero, e quel `&mut` è
/// l'unica traccia onesta di cosa sta facendo.
///
/// # Errori
///
/// `library.playlistNameInvalid` se il nome non identifica niente,
/// `library.playlistIsSmart` se esiste già una playlist automatica con quel
/// nome, `spotify.tracklistTruncated` se un elenco monco sostituirebbe una
/// playlist che esiste già, `db.queryFailed` per il resto.
pub fn plan(
    connection: &mut Connection,
    contenuto: &SpotifyContent,
    crea_playlist: bool,
) -> Result<SpotifyImportReport, AppError> {
    run(connection, contenuto, crea_playlist, false)
}

/// Importa per davvero.
///
/// # Errori
///
/// Gli stessi di [`plan`].
pub fn import(
    connection: &mut Connection,
    contenuto: &SpotifyContent,
    crea_playlist: bool,
) -> Result<SpotifyImportReport, AppError> {
    run(connection, contenuto, crea_playlist, true)
}

/// L'importazione. `commit` decide se resta.
fn run(
    connection: &mut Connection,
    contenuto: &SpotifyContent,
    crea_playlist: bool,
    commit: bool,
) -> Result<SpotifyImportReport, AppError> {
    let libreria = leggi_libreria(connection)?;
    let piano = plan_spotify_import(&contenuto.tracks, &libreria);

    let mut rapporto = SpotifyImportReport {
        kind: contenuto.kind.nome().to_owned(),
        source: contenuto.source.nome().to_owned(),
        source_id: contenuto.id.clone(),
        title: contenuto.title.clone(),
        resolved: contenuto.tracks.len(),
        matched: piano.abbinati.len(),
        missing: piano.mancanti.len(),
        matched_isrc: piano.per_gradino(Gradino::Isrc),
        matched_exact: piano.per_gradino(Gradino::ChiaveEsatta),
        matched_by_title: piano.per_gradino(Gradino::ArtistaTitolo),
        matched_stripped: piano.per_gradino(Gradino::Ripulito),
        truncated: contenuto
            .truncation()
            .map(|(read, expected)| Truncation { read, expected }),
        ..SpotifyImportReport::default()
    };

    for indice in &piano.mancanti {
        if let Some(brano) = contenuto.tracks.get(*indice) {
            rapporto.missing_tracks.push(MissingTrack {
                position: indice.saturating_add(1),
                title: brano.title.clone(),
                artist: brano.artist.clone(),
                album: brano.album.clone(),
            });
        }
    }

    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione", &err))?;

    if crea_playlist {
        // L'identificativo si passa **solo** quando è davvero quello di una
        // playlist. `contenuto.id` è l'identificativo di quel che si sta
        // importando: per un album è un album, e scriverlo in
        // `playlists.spotify_playlist_id` farebbe riconoscere come «la stessa
        // playlist» due cose che non lo sono.
        let identificativo =
            matches!(contenuto.kind, SpotifyKind::Playlist).then_some(contenuto.id.as_str());
        let (id, creata, sostituita) = prepara_playlist(
            &tx,
            &contenuto.title,
            identificativo,
            contenuto.truncation(),
        )?;
        rapporto.playlist_id = Some(id);
        rapporto.playlist_name = Some(contenuto.title.trim().to_owned());
        rapporto.playlist_created = creata;
        rapporto.playlist_replaced = sostituita;
        rapporto.playlist_entries = riempi_playlist(
            &tx,
            id,
            &piano
                .abbinati
                .iter()
                .map(|a| (a.indice, a.track_id))
                .collect::<Vec<_>>(),
        )?;
    }

    // Gli identificativi esterni sui brani ritrovati. È qui che l'innesto di
    // `aether_domain::album` — che sa già fondere le edizioni di un album per
    // `spotify_album_id`, con tanto di vettori dorati — riceve finalmente
    // qualcuno che ci scriva dentro.
    let mut scritti_album = 0_usize;
    let mut scritti_isrc = 0_usize;
    for abbinato in &piano.abbinati {
        let Some(brano) = contenuto.tracks.get(abbinato.indice) else {
            continue;
        };
        let (album, isrc) = scrivi_identificativi(&tx, abbinato.track_id, brano)?;
        scritti_album = scritti_album.saturating_add(usize::from(album));
        scritti_isrc = scritti_isrc.saturating_add(usize::from(isrc));
    }
    rapporto.spotify_album_ids_written = scritti_album;
    rapporto.isrc_written = scritti_isrc;

    rapporto.wanted_rows = scrivi_desiderati(
        &tx,
        &contenuto.tracks,
        Sorgente {
            kind: contenuto.kind.nome(),
            id: &contenuto.id,
            titolo: &contenuto.title,
        },
        &piano.mancanti,
        rapporto.playlist_id,
    )?;

    // Il viaggio di ritorno, ristretto a **questa** importazione: quel che è
    // successo alle altre playlist non è affar suo e non deve comparire nel suo
    // rapporto. Qui serve soprattutto la seconda metà — chiudere le righe di una
    // importazione precedente il cui brano nel frattempo è arrivato — perché
    // altrimenti resterebbero in attesa per sempre e la coda le riscaricherebbe
    // accanto ai file che ci sono già.
    let ritorno = crate::desiderati::riconcilia(&tx, Some(&contenuto.id))?;
    rapporto.playlist_restored = ritorno.voci_rimesse;
    rapporto.wanted_closed = ritorno.righe_chiuse;

    // Solo se qualcosa è cambiato: ricostruire gli aggregati su una libreria
    // grande non è gratis, e un'importazione che non ha scritto nemmeno un
    // identificativo non ha spostato nessun album.
    if scritti_album > 0 {
        rebuild_aggregates(&tx)?;
    }

    if commit {
        tx.commit()
            .map_err(|err| db_error("chiusura dell'importazione", &err))?;
    } else {
        // Abbandonata di proposito: vedi la nota in testa al modulo.
        drop(tx);
    }
    Ok(rapporto)
}

/// La libreria, ridotta a quel che serve per riconoscere un brano.
///
/// Si legge **una volta per importazione**, non una per elenco: un account con
/// duecento playlist la rileggerebbe duecento volte, ed è la ragione per cui
/// `plan_account_import` prende la libreria come argomento invece di andarsela a
/// prendere da sé.
pub(crate) fn leggi_libreria(connection: &Connection) -> Result<Vec<LibraryTrack>, AppError> {
    let mut statement = connection
        .prepare("SELECT id, track_key, artist, title, duration_ms, isrc FROM tracks ORDER BY id")
        .map_err(|err| db_error("lettura della libreria", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok(LibraryTrack {
                id: row.get(0)?,
                track_key: row.get(1)?,
                artist: row.get(2)?,
                title: row.get(3)?,
                duration_ms: row.get(4)?,
                isrc: row.get(5)?,
            })
        })
        .map_err(|err| db_error("lettura della libreria", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura della libreria", &err))
}

/// Trova o crea la playlist, svuotandola se esisteva.
///
/// Restituisce `(id, creata, sostituita)`.
///
/// # I due modi di riconoscere una playlist già importata
///
/// `spotify_id` prima, `playlist_key` poi. L'ordine è tutto: la chiave nasce dal
/// **nome** (vedi [`PlaylistKey`]), quindi chi rinomina «Corsa» in «Corsa 2026»
/// su Spotify, senza l'identificativo, alla reimportazione si ritroverebbe due
/// playlist — la vecchia col nome vecchio e i suoi brani, e una nuova identica
/// accanto. È il difetto per cui `005_account.sql` ha aggiunto quella colonna, e
/// riconoscere per identificativo è il solo modo di ripagarla.
///
/// Quando l'identificativo ritrova una playlist che nel frattempo ha cambiato
/// nome, il nome **si aggiorna**: è quel che l'utente ha deciso su Spotify. Non
/// si aggiorna in un solo caso — se quel nome è già di un'altra playlist — e lì
/// si tiene il vecchio: rifiutare l'importazione intera per un nome occupato
/// sarebbe una punizione sproporzionata rispetto ai brani che sta portando.
///
/// # Perché sostituire e non accodare
///
/// Perché reimportare lo stesso link deve dare lo stesso stato. Accodando, la
/// seconda importazione raddoppierebbe ogni brano; creando un nome nuovo,
/// lascerebbe una scia di «Playlist (2)». Sostituire è l'unica delle tre che si
/// può fare due volte senza pentirsene — e siccome cancella, l'utente lo vede
/// scritto nel piano prima di confermare.
///
/// # Tranne quando l'elenco è arrivato monco
///
/// «Sostituire si può fare due volte senza pentirsene» vale finché le due volte
/// leggono la stessa cosa. Una playlist di trecento brani letta a duecento —
/// perché una pagina di Pathfinder ha preso un `429`, o perché la rete ha
/// singhiozzato a metà — cancellerebbe le trecento voci buone e ne rimetterebbe
/// duecento. L'utente non se ne accorge: la playlist c'è, ha un nome giusto, ed
/// è più corta di cento brani che nessuno gli dirà mai.
///
/// Quindi in quel caso non si sostituisce: si rifiuta con
/// `spotify.tracklistTruncated`, che porta i due numeri perché il messaggio
/// possa dire «duecento su trecento». Il piano lo scopre esattamente come
/// l'importazione, quindi il tasto «Importa» lo dice **prima**.
///
/// **Solo** la sostituzione, però. Un elenco monco che crea una playlist nuova
/// non cancella niente: duecento brani sono meglio di nessuno, e reimportare
/// quando la rete è tornata a posto li completa.
pub(crate) fn prepara_playlist(
    tx: &Transaction<'_>,
    titolo: &str,
    spotify_id: Option<&str>,
    troncatura: Option<(u32, u32)>,
) -> Result<(i64, bool, bool), AppError> {
    let chiave = PlaylistKey::compute(Some(titolo.trim()));
    if chiave.as_str().is_empty() {
        return Err(AppError::new(ErrorCode::LibraryPlaylistNameInvalid {
            name: titolo.to_owned(),
        }));
    }

    let per_identificativo = match spotify_id {
        Some(id) => tx
            .query_row(
                "SELECT id, is_smart FROM playlists WHERE spotify_playlist_id = ?1",
                [id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .ok(),
        None => None,
    };
    let esistente: Option<(i64, bool)> = per_identificativo.or_else(|| {
        tx.query_row(
            "SELECT id, is_smart FROM playlists WHERE playlist_key = ?1",
            [chiave.as_str()],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .ok()
    });

    match esistente {
        Some((_, true)) => Err(AppError::new(ErrorCode::LibraryPlaylistIsSmart {
            playlist_id: esistente.map(|(id, _)| id),
        })),
        Some((id, false)) => {
            if let Some((letti, attesi)) = troncatura {
                return Err(AppError::new(ErrorCode::SpotifyTracklistTruncated {
                    letti,
                    attesi,
                }));
            }
            tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [id])
                .map_err(|err| db_error("svuotamento della playlist", &err))?;
            rinomina_se_libero(tx, id, titolo.trim(), &chiave)?;
            segna_identificativo(tx, id, spotify_id)?;
            Ok((id, false, true))
        }
        None => {
            let id = crate::playlists::crea_riga(tx, titolo.trim())?;
            segna_identificativo(tx, id, spotify_id)?;
            Ok((id, true, false))
        }
    }
}

/// Allinea il nome a quello che la playlist ha adesso su Spotify.
///
/// `WHERE NOT EXISTS` invece di un `UPDATE` nudo: `playlist_key` è unica, e una
/// collisione qui alzerebbe un errore che farebbe fallire l'importazione intera
/// per un nome occupato. Il `WHERE` la trasforma in «zero righe cambiate», che è
/// la decisione giusta — la playlist resta col nome vecchio e i suoi brani sono
/// comunque arrivati.
fn rinomina_se_libero(
    tx: &Transaction<'_>,
    id: i64,
    nome: &str,
    chiave: &PlaylistKey,
) -> Result<(), AppError> {
    tx.execute(
        "UPDATE playlists SET name = ?2, playlist_key = ?3
          WHERE id = ?1 AND playlist_key <> ?3
            AND NOT EXISTS (SELECT 1 FROM playlists WHERE playlist_key = ?3)",
        rusqlite::params![id, nome, chiave.as_str()],
    )
    .map_err(|err| db_error("aggiornamento del nome della playlist", &err))?;
    Ok(())
}

/// Scrive `spotify_playlist_id`, se ce n'è uno da scrivere.
fn segna_identificativo(
    tx: &Transaction<'_>,
    id: i64,
    spotify_id: Option<&str>,
) -> Result<(), AppError> {
    let Some(spotify_id) = spotify_id else {
        return Ok(());
    };
    tx.execute(
        "UPDATE playlists SET spotify_playlist_id = ?2 WHERE id = ?1",
        rusqlite::params![id, spotify_id],
    )
    .map_err(|err| db_error("scrittura dell'identificativo della playlist", &err))?;
    Ok(())
}

/// Mette i brani ritrovati in playlist, al posto che hanno su Spotify.
///
/// Riceve `(indice su Spotify, brano)` e scrive **quell'indice** come posizione,
/// lasciando un buco dove il brano manca.
///
/// # Perché i buchi, e perché prima non c'erano
///
/// Perché i buchi sono i posti che aspettano i brani da scaricare. Un elenco
/// compattato `0..n` è coerente e definitivo: quando poi
/// [`crate::desiderati::riconcilia`] riporta indietro il brano scaricato, non
/// c'è più nessun posto che sia il suo, e l'unica cosa che resta da fare è
/// metterlo in fondo. Con i buchi la playlist si ricompone da sola nell'ordine
/// che ha su Spotify, un brano alla volta, man mano che scendono.
///
/// La compattazione era motivata così: «le posizioni con salti farebbero puntare
/// al brano sbagliato gli indici che la finestra rimanda indietro per uno
/// spostamento». Non è vero, ed è per questo che si può cambiare:
/// [`crate::playlists::remove_at`] e `reorder` rileggono l'ordine in una lista
/// densa e la riscrivono (`ordine_attuale` → `riscrivi_ordine`), quindi trattano
/// quel numero come indice di **visualizzazione** e non come `position` grezza.
/// I buchi non li vede nessuno, e la prima modifica a mano li richiude da sé.
pub(crate) fn riempi_playlist(
    tx: &Transaction<'_>,
    playlist_id: i64,
    voci: &[(usize, i64)],
) -> Result<usize, AppError> {
    {
        let mut inserisci = tx
            .prepare(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
            )
            .map_err(|err| db_error("riempimento della playlist", &err))?;
        for (posizione, track_id) in voci {
            inserisci
                .execute(rusqlite::params![
                    playlist_id,
                    track_id,
                    i64::try_from(*posizione).unwrap_or(i64::MAX)
                ])
                .map_err(|err| db_error("riempimento della playlist", &err))?;
        }
    }
    tx.execute(
        "UPDATE playlists SET updated_at = ?2 WHERE id = ?1",
        rusqlite::params![playlist_id, now_ms()],
    )
    .map_err(|err| db_error("riempimento della playlist", &err))?;
    Ok(voci.len())
}

/// Scrive `spotify_album_id` e `isrc` su un brano, senza sovrascrivere.
///
/// `WHERE ... IS NULL` e non `COALESCE`: un identificativo già presente arriva
/// da MusicBrainz o da un'importazione precedente, ed è almeno altrettanto
/// autorevole. La condizione serve anche a contare le scritture vere, che è
/// l'unico modo di sapere se vale la pena ricostruire gli aggregati.
pub(crate) fn scrivi_identificativi(
    tx: &Transaction<'_>,
    track_id: i64,
    brano: &SpotifyTrack,
) -> Result<(bool, bool), AppError> {
    let mut album = false;
    if let Some(id_album) = &brano.spotify_album_id {
        let quante = tx
            .execute(
                "UPDATE tracks SET spotify_album_id = ?2
                 WHERE id = ?1 AND (spotify_album_id IS NULL OR spotify_album_id = '')",
                rusqlite::params![track_id, id_album],
            )
            .map_err(|err| db_error("scrittura dell'identificativo Spotify", &err))?;
        album = quante > 0;
    }

    let mut isrc = false;
    if let Some(codice) = &brano.isrc {
        let quante = tx
            .execute(
                "UPDATE tracks SET isrc = ?2 WHERE id = ?1 AND (isrc IS NULL OR isrc = '')",
                rusqlite::params![track_id, codice],
            )
            .map_err(|err| db_error("scrittura dell'ISRC", &err))?;
        isrc = quante > 0;
    }
    Ok((album, isrc))
}

/// Da dove veniva un brano desiderato.
///
/// Le tre colonne `source_*` di `spotify_wanted`, raccolte perché sono un
/// argomento solo: separate erano tre `&str` di fila, cioè tre occasioni di
/// passarle in ordine sbagliato senza che il compilatore dica niente.
///
/// [`Self::id`] è anche metà dell'indice unico `(track_key, source_id)`, quindi
/// **due sorgenti diverse devono avere identificativi diversi**: se due playlist
/// lo condividessero, il brano che manca a entrambe diventerebbe una riga sola e
/// la seconda playlist perderebbe il proprio posto. Da un link e dalla Web API
/// l'identificativo è quello di Spotify; dall'archivio, che non lo contiene, chi
/// chiama ne fabbrica uno stabile (vedi `import_account`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sorgente<'a> {
    /// `brano`, `album`, `playlist`, `artista`, `preferiti`.
    pub kind: &'a str,
    /// L'identificativo del contenitore.
    pub id: &'a str,
    /// Come si chiama, per poter dire «mancano da questa playlist».
    pub titolo: &'a str,
}

/// Registra i brani che in libreria non ci sono.
pub(crate) fn scrivi_desiderati(
    tx: &Transaction<'_>,
    brani: &[SpotifyTrack],
    sorgente: Sorgente<'_>,
    mancanti: &[usize],
    playlist_id: Option<i64>,
) -> Result<usize, AppError> {
    if mancanti.is_empty() {
        return Ok(0);
    }
    let adesso = now_ms();
    // `ON CONFLICT DO UPDATE` e non più `INSERT OR REPLACE`: quest'ultimo
    // **cancella e reinserisce** la riga, e dalla migrazione 3 in poi quella riga
    // porta anche lo stato dello scaricamento. Reimportare la stessa playlist
    // azzererebbe i tentativi, rimetterebbe in coda ciò che è già stato preso e
    // dimenticherebbe quali brani su YouTube non esistono — cioè rifarebbe da
    // capo tutto il lavoro ogni volta che l'utente ricontrolla una playlist.
    //
    // I metadati si aggiornano (su Spotify un titolo può cambiare); le colonne
    // `download_*` non si nominano, quindi restano.
    let mut inserisci = tx
        .prepare(
            "INSERT INTO spotify_wanted (
                 track_key, title, artist, album, album_artist, duration_ms, isrc,
                 spotify_track_id, spotify_album_id, cover_url,
                 track_number, disc_number, year,
                 source_kind, source_id, source_title, playlist_id, position, added_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                       ?14, ?15, ?16, ?17, ?18, ?19)
             ON CONFLICT(track_key, source_id) DO UPDATE SET
                 title = excluded.title,
                 artist = excluded.artist,
                 album = excluded.album,
                 album_artist = excluded.album_artist,
                 duration_ms = excluded.duration_ms,
                 isrc = excluded.isrc,
                 spotify_track_id = excluded.spotify_track_id,
                 spotify_album_id = excluded.spotify_album_id,
                 cover_url = excluded.cover_url,
                 track_number = excluded.track_number,
                 disc_number = excluded.disc_number,
                 year = excluded.year,
                 source_title = excluded.source_title,
                 playlist_id = excluded.playlist_id,
                 position = excluded.position",
        )
        .map_err(|err| db_error("registrazione dei brani desiderati", &err))?;

    // Due brani della stessa playlist possono avere la stessa chiave — una
    // playlist può ripetere una canzone. `INSERT OR REPLACE` sull'indice unico
    // ne terrebbe una sola: si contano le righe davvero distinte, così il
    // rapporto non promette più righe di quante ne scrive.
    let mut viste: HashMap<String, ()> = HashMap::new();
    let mut quante = 0_usize;
    for indice in mancanti {
        let Some(brano) = brani.get(*indice) else {
            continue;
        };
        let chiave = TrackKey::compute(TrackKeyInput {
            artist: brano.artist.as_deref(),
            title: Some(&brano.title),
            album: brano.album.as_deref(),
        });
        inserisci
            .execute(rusqlite::params![
                chiave.as_str(),
                brano.title,
                brano.artist,
                brano.album,
                brano.album_artist,
                brano.duration_ms.and_then(|d| i64::try_from(d).ok()),
                brano.isrc,
                brano.spotify_track_id,
                brano.spotify_album_id,
                brano.cover_url,
                brano.track_number,
                brano.disc_number,
                brano.year,
                sorgente.kind,
                sorgente.id,
                sorgente.titolo,
                playlist_id,
                i64::try_from(*indice).unwrap_or(i64::MAX),
                adesso,
            ])
            .map_err(|err| db_error("registrazione dei brani desiderati", &err))?;
        if viste.insert(chiave.into_string(), ()).is_none() {
            quante = quante.saturating_add(1);
        }
    }
    Ok(quante)
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::spotify::{SpotifyKind, SpotifySource};

    fn database() -> Connection {
        let Ok(aperto) = crate::db::open_in_memory() else {
            panic!("il database in memoria si deve aprire");
        };
        aperto.connection
    }

    fn aggiungi(connection: &Connection, artista: &str, titolo: &str, album: &str, durata: i64) {
        let chiave = TrackKey::compute(TrackKeyInput {
            artist: Some(artista),
            title: Some(titolo),
            album: Some(album),
        });
        let esito = connection.execute(
            "INSERT INTO tracks (path, track_key, title, artist, album, duration_ms,
                                 file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0)",
            rusqlite::params![
                format!("C:/musica/{artista} - {titolo}.mp3"),
                chiave.as_str(),
                titolo,
                artista,
                album,
                durata
            ],
        );
        assert!(esito.is_ok(), "l'inserimento di prova deve riuscire");
    }

    fn brano(artista: &str, titolo: &str, album: &str, durata: u64) -> SpotifyTrack {
        // Un ISRC diverso per titolo. Darlo uguale a tutti sarebbe comodo e
        // falso: due registrazioni diverse con lo stesso codice non esistono, e
        // qui accenderebbero il gradino dell'ISRC su abbinamenti che nella
        // realtà non avverrebbero mai — nascondendo quel che il test vuole
        // provare, cioè la scala sotto.
        let somma: u32 = titolo.bytes().map(u32::from).sum();
        SpotifyTrack {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            album: Some(album.to_owned()),
            duration_ms: Some(durata),
            spotify_album_id: Some("6dVIqQ8qmQ5GBnJ9shOYGE".to_owned()),
            isrc: Some(format!("GBAYE{somma:07}")),
            ..SpotifyTrack::default()
        }
    }

    fn contenuto(brani: Vec<SpotifyTrack>) -> SpotifyContent {
        let quanti = u32::try_from(brani.len()).unwrap_or(0);
        SpotifyContent {
            kind: SpotifyKind::Playlist,
            id: "37i9dQZF1DXcBWIGoYBM5M".to_owned(),
            title: "La mia playlist".to_owned(),
            author: Some("Tizio".to_owned()),
            cover_url: None,
            tracks: brani,
            declared_total: Some(quanti),
            source: SpotifySource::Pathfinder,
        }
    }

    fn conta(connection: &Connection, sql: &str) -> i64 {
        connection.query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
    }

    #[test]
    fn il_piano_non_scrive_niente() {
        // Il principio del modulo: costa quanto l'importazione e non lascia
        // traccia. Se lasciasse traccia, «vedere prima di decidere» sarebbe
        // «decidere e basta».
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![
            brano("Radiohead", "Karma Police", "OK Computer", 264_066),
            brano("Caio", "Sconosciuta", "Altro", 200_000),
        ]);

        let Ok(rapporto) = plan(&mut db, &c, true) else {
            panic!("il piano deve riuscire");
        };
        assert_eq!(rapporto.matched, 1);
        assert_eq!(rapporto.missing, 1);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 0);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlist_tracks"), 0);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM spotify_wanted"), 0);
    }

    #[test]
    fn il_piano_dice_esattamente_quel_che_farà_limportazione() {
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![
            brano("Radiohead", "Karma Police", "OK Computer", 264_066),
            brano("Caio", "Sconosciuta", "Altro", 200_000),
        ]);

        let (Ok(previsto), Ok(fatto)) = (plan(&mut db, &c, true), import(&mut db, &c, true)) else {
            panic!("entrambi devono riuscire");
        };
        // L'identificativo della playlist è l'unico campo che può differire: nel
        // piano la riga viene creata e annullata, e SQLite non riusa quel numero.
        let previsto = SpotifyImportReport {
            playlist_id: fatto.playlist_id,
            ..previsto
        };
        assert_eq!(previsto, fatto);
    }

    #[test]
    fn limportazione_crea_la_playlist_e_ci_mette_i_brani_ritrovati() {
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        aggiungi(&db, "Blur", "Song 2", "Blur", 122_000);
        let c = contenuto(vec![
            brano("Radiohead", "Karma Police", "OK Computer", 264_066),
            brano("Caio", "Sconosciuta", "Altro", 200_000),
            brano("Blur", "Song 2", "Blur", 122_000),
        ]);

        let Ok(rapporto) = import(&mut db, &c, true) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.matched, 2);
        assert_eq!(rapporto.playlist_entries, 2);
        assert!(rapporto.playlist_created);
        assert!(!rapporto.playlist_replaced);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlist_tracks"), 2);

        // Le posizioni sono quelle di Spotify: il mancante in mezzo lascia il
        // suo posto libero, ed è il posto in cui `desiderati::riconcilia` lo
        // rimetterà quando sarà sceso da YouTube.
        let posizioni: Vec<i64> = {
            let Ok(mut s) = db.prepare("SELECT position FROM playlist_tracks ORDER BY position")
            else {
                panic!("query valida");
            };
            let Ok(righe) = s.query_map([], |r| r.get(0)) else {
                panic!("query valida");
            };
            righe.filter_map(Result::ok).collect()
        };
        assert_eq!(posizioni, vec![0, 2], "il posto 1 aspetta «Sconosciuta»");
    }

    #[test]
    fn i_mancanti_finiscono_fra_i_desiderati_con_la_loro_posizione_vera() {
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![
            brano("Caio", "Prima", "Altro", 200_000),
            brano("Radiohead", "Karma Police", "OK Computer", 264_066),
            brano("Sempronio", "Terza", "Altro", 210_000),
        ]);

        let Ok(rapporto) = import(&mut db, &c, true) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.wanted_rows, 2);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM spotify_wanted"), 2);
        assert_eq!(
            rapporto.missing_tracks.first().map(|m| m.position),
            Some(1),
            "le posizioni mostrate all'utente contano da 1"
        );
        assert_eq!(
            conta(
                &db,
                "SELECT position FROM spotify_wanted WHERE title = 'Terza'"
            ),
            2,
            "in `spotify_wanted` resta l'indice vero dentro l'elenco di Spotify"
        );
        assert_eq!(
            conta(
                &db,
                "SELECT COUNT(*) FROM spotify_wanted WHERE source_id = '37i9dQZF1DXcBWIGoYBM5M'"
            ),
            2,
            "e la provenienza, per poter dire «mancano da questa playlist»"
        );
    }

    #[test]
    fn gli_identificativi_esterni_arrivano_sui_brani_ritrovati() {
        // È l'innesto che in `aether_domain::album` era già scritto e testato
        // senza nessuno che ci scrivesse dentro.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);

        let Ok(rapporto) = import(&mut db, &c, false) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.spotify_album_ids_written, 1);
        assert_eq!(rapporto.isrc_written, 1);
        let letto: Option<String> = db
            .query_row("SELECT spotify_album_id FROM tracks", [], |r| r.get(0))
            .unwrap_or(None);
        assert_eq!(letto.as_deref(), Some("6dVIqQ8qmQ5GBnJ9shOYGE"));
        let isrc: Option<String> = db
            .query_row("SELECT isrc FROM tracks", [], |r| r.get(0))
            .unwrap_or(None);
        // Ricavato dal costruttore invece che scritto a mano: due copie dello
        // stesso codice divergono, e questa divergerebbe in silenzio.
        assert_eq!(
            isrc,
            brano("Radiohead", "Karma Police", "OK Computer", 0).isrc
        );
    }

    #[test]
    fn un_identificativo_gia_presente_non_si_sovrascrive() {
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let _ = db.execute("UPDATE tracks SET isrc = 'GIAPRESENTE1'", []);
        let c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);

        let Ok(rapporto) = import(&mut db, &c, false) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.isrc_written, 0);
        let isrc: Option<String> = db
            .query_row("SELECT isrc FROM tracks", [], |r| r.get(0))
            .unwrap_or(None);
        assert_eq!(isrc.as_deref(), Some("GIAPRESENTE1"));
    }

    #[test]
    fn reimportare_due_volte_da_lo_stesso_stato() {
        // La proprietà che rende l'importazione ripetibile senza pentirsene, e
        // la ragione per cui una playlist esistente si sostituisce invece di
        // accodarsi.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![
            brano("Radiohead", "Karma Police", "OK Computer", 264_066),
            brano("Caio", "Sconosciuta", "Altro", 200_000),
        ]);

        let Ok(primo) = import(&mut db, &c, true) else {
            panic!("la prima importazione deve riuscire");
        };
        let Ok(secondo) = import(&mut db, &c, true) else {
            panic!("la seconda importazione deve riuscire");
        };

        assert!(primo.playlist_created && !primo.playlist_replaced);
        assert!(!secondo.playlist_created && secondo.playlist_replaced);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 1);
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM playlist_tracks"),
            1,
            "non si accoda: si sostituisce"
        );
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM spotify_wanted"),
            1,
            "e i desiderati non si accumulano"
        );
    }

    #[test]
    fn la_playlist_si_ricorda_da_quale_playlist_di_spotify_viene() {
        // E ci si riconosce anche dopo un rinominare, che è tutto il motivo per
        // cui `005_account.sql` ha aggiunto quella colonna: `PlaylistKey` nasce
        // dal nome, e senza l'identificativo la seconda importazione ne
        // creerebbe una accanto alla prima.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let mut c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);

        let Ok(_) = import(&mut db, &c, true) else {
            panic!("la prima importazione deve riuscire");
        };
        let scritto: Option<String> = db
            .query_row("SELECT spotify_playlist_id FROM playlists", [], |r| {
                r.get(0)
            })
            .unwrap_or(None);
        assert_eq!(scritto.as_deref(), Some("37i9dQZF1DXcBWIGoYBM5M"));

        c.title = "Un altro nome".to_owned();
        let Ok(secondo) = import(&mut db, &c, true) else {
            panic!("la seconda importazione deve riuscire");
        };
        assert!(secondo.playlist_replaced, "è la stessa playlist");
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 1);
        let nome: String = db
            .query_row("SELECT name FROM playlists", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(nome, "Un altro nome");
    }

    #[test]
    fn un_album_non_si_spaccia_per_una_playlist() {
        // `contenuto.id` è l'identificativo di quel che si sta importando: per
        // un album è un album, e scriverlo in `playlists.spotify_playlist_id`
        // farebbe riconoscere come «la stessa playlist» due cose che non lo
        // sono — bastano un album e una playlist con lo stesso identificativo
        // dentro due spazi di nomi diversi.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let mut c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);
        c.kind = SpotifyKind::Album;

        let Ok(_) = import(&mut db, &c, true) else {
            panic!("l'importazione di un album deve riuscire");
        };
        let scritto: Option<String> = db
            .query_row("SELECT spotify_playlist_id FROM playlists", [], |r| {
                r.get(0)
            })
            .unwrap_or(None);
        assert_eq!(scritto, None);
    }

    #[test]
    fn un_nome_gia_di_un_altra_playlist_non_fa_fallire_l_importazione() {
        // Il rinominare si ferma qui e basta: `playlist_key` è unica, e far
        // fallire l'importazione per un nome occupato sarebbe una punizione
        // sproporzionata rispetto ai brani che sta portando.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let mut c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);
        let Ok(_) = import(&mut db, &c, true) else {
            panic!("la prima importazione deve riuscire");
        };
        let Ok(_) = db.execute(
            "INSERT INTO playlists (playlist_key, name, created_at, updated_at, is_smart)
             VALUES ('occupato', 'Occupato', 0, 0, 0)",
            [],
        ) else {
            panic!("la playlist di prova si deve inserire");
        };

        c.title = "Occupato".to_owned();
        let Ok(rapporto) = import(&mut db, &c, true) else {
            panic!("l'importazione deve riuscire lo stesso");
        };
        assert!(rapporto.playlist_replaced);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 2);
        assert_eq!(
            conta(
                &db,
                "SELECT COUNT(*) FROM playlists WHERE name = 'La mia playlist'"
            ),
            1,
            "tiene il nome vecchio invece di rifiutare i brani"
        );
    }

    #[test]
    fn una_playlist_automatica_non_si_tocca() {
        let mut db = database();
        let _ = db.execute(
            "INSERT INTO playlists (playlist_key, name, created_at, updated_at, is_smart, rules)
             VALUES ('la mia playlist', 'La mia playlist', 0, 0, 1, '{}')",
            [],
        );
        let c = contenuto(vec![brano("Tizio", "Canzone", "Album", 200_000)]);
        let esito = import(&mut db, &c, true);
        assert!(esito.is_err(), "una playlist automatica ricalcola da sé");
    }

    #[test]
    fn un_elenco_monco_non_sostituisce_una_playlist_che_esiste() {
        // Il guasto che questa guardia impedisce: trecento voci buone cancellate
        // e rimpiazzate da duecento, senza che niente lo dica. La playlist c'è,
        // ha il nome giusto, ed è più corta di cento brani.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);

        let Ok(_) = import(&mut db, &c, true) else {
            panic!("la prima importazione, completa, deve riuscire");
        };
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlist_tracks"), 1);

        let mut monco = c.clone();
        monco.declared_total = Some(300);
        let esito = import(&mut db, &monco, true);
        assert!(esito.is_err(), "un elenco monco non deve poter sostituire");
        assert_eq!(
            esito.err().map(|e| e.code().kind().code().to_owned()),
            Some("spotify.tracklistTruncated".to_owned())
        );
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM playlist_tracks"),
            1,
            "e soprattutto non deve aver cancellato niente"
        );
    }

    #[test]
    fn un_elenco_monco_puo_creare_una_playlist_nuova() {
        // La guardia è sulla **sostituzione**, non sul troncamento: qui non c'è
        // niente da perdere, e un brano è meglio di nessuno.
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let mut c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);
        c.declared_total = Some(300);

        let Ok(rapporto) = import(&mut db, &c, true) else {
            panic!("creare una playlist nuova deve riuscire anche da un elenco monco");
        };
        assert!(rapporto.playlist_created);
        assert_eq!(rapporto.playlist_entries, 1);
    }

    #[test]
    fn il_brano_scaricato_torna_al_suo_posto() {
        // La catena intera, dall'importazione al ritorno: è il buco lasciato da
        // `riempi_playlist` che rende possibile rimettere il brano dov'era
        // invece che in fondo.
        let mut db = database();
        aggiungi(&db, "Bravo", "Bravo", "Album", 200_000);
        let c = contenuto(vec![
            brano("Alpha", "Alpha", "Album", 200_000),
            brano("Bravo", "Bravo", "Album", 200_000),
            brano("Charlie", "Charlie", "Album", 200_000),
        ]);

        let Ok(rapporto) = import(&mut db, &c, true) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.playlist_entries, 1);
        assert_eq!(rapporto.wanted_rows, 2);

        // Arrivano da YouTube, e la scansione li porta in libreria.
        aggiungi(&db, "Alpha", "Alpha", "Album", 200_000);
        aggiungi(&db, "Charlie", "Charlie", "Album", 200_000);
        let Ok(ritorno) = crate::desiderati::riconcilia(&db, None) else {
            panic!("la riconciliazione deve riuscire");
        };
        assert_eq!(ritorno.voci_rimesse, 2);

        let ordine: Vec<String> = {
            let Ok(mut s) = db.prepare(
                "SELECT t.title FROM playlist_tracks pt
                 JOIN tracks t ON t.id = pt.track_id ORDER BY pt.position",
            ) else {
                panic!("query valida");
            };
            let Ok(righe) = s.query_map([], |r| r.get(0)) else {
                panic!("query valida");
            };
            righe.filter_map(Result::ok).collect()
        };
        assert_eq!(
            ordine,
            vec!["Alpha", "Bravo", "Charlie"],
            "la playlist si ricompone nell'ordine che ha su Spotify"
        );
    }

    #[test]
    fn una_riga_vecchia_il_cui_brano_e_arrivato_si_chiude() {
        // Reimportare dopo aver messo i file a mano: senza questa chiusura le
        // righe restano in attesa e la coda le riscarica accanto a quelle che
        // ci sono già.
        let mut db = database();
        let c = contenuto(vec![brano("Alpha", "Alpha", "Album", 200_000)]);
        let Ok(primo) = import(&mut db, &c, false) else {
            panic!("la prima importazione deve riuscire");
        };
        assert_eq!(primo.wanted_rows, 1);

        aggiungi(&db, "Alpha", "Alpha", "Album", 200_000);
        let Ok(secondo) = import(&mut db, &c, false) else {
            panic!("la seconda importazione deve riuscire");
        };
        assert_eq!(secondo.matched, 1, "adesso il brano c'è");
        assert_eq!(
            conta(
                &db,
                "SELECT COUNT(*) FROM spotify_wanted WHERE download_state = 'attesa'"
            ),
            0,
            "e la riga che lo aspettava si è chiusa"
        );
    }

    #[test]
    fn lisrc_ritrova_quel_che_i_titoli_non_ritroverebbero() {
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let Ok(_) = db.execute("UPDATE tracks SET isrc = 'GBAYE9700426'", []) else {
            panic!("l'ISRC di prova si deve scrivere");
        };
        let c = contenuto(vec![SpotifyTrack {
            title: "Karma Police - 2017 Remaster".to_owned(),
            artist: Some("Radiohead & Friends".to_owned()),
            album: Some("OKNOTOK".to_owned()),
            duration_ms: Some(400_000),
            isrc: Some("GBAYE9700426".to_owned()),
            ..SpotifyTrack::default()
        }]);

        let Ok(rapporto) = plan(&mut db, &c, false) else {
            panic!("il piano deve riuscire");
        };
        assert_eq!(rapporto.matched_isrc, 1);
        assert_eq!(rapporto.missing, 0);
    }

    #[test]
    fn un_elenco_monco_finisce_nel_rapporto() {
        let mut db = database();
        let mut c = contenuto(vec![brano("Tizio", "Canzone", "Album", 200_000)]);
        c.declared_total = Some(300);
        let Ok(rapporto) = plan(&mut db, &c, false) else {
            panic!("il piano deve riuscire");
        };
        assert_eq!(
            rapporto.truncated,
            Some(Truncation {
                read: 1,
                expected: 300
            })
        );
    }

    #[test]
    fn senza_playlist_si_importano_solo_gli_identificativi() {
        let mut db = database();
        aggiungi(&db, "Radiohead", "Karma Police", "OK Computer", 264_000);
        let c = contenuto(vec![brano(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_066,
        )]);
        let Ok(rapporto) = import(&mut db, &c, false) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.playlist_id, None);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 0);
        assert_eq!(rapporto.spotify_album_ids_written, 1);
    }
}
