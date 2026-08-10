//! Un account Spotify intero, scritto in libreria.
//!
//! # Un solo scrittore per due strade
//!
//! Qui arriva un [`AccountSnapshot`], e chi l'ha riempito non si sa: la Web API
//! dopo un consenso OAuth, o l'archivio che Spotify manda per posta. È la
//! decisione presa in [`aether_domain::spotify_account`], e questo modulo è il
//! posto in cui si ripaga — una correzione all'abbinamento dei preferiti vale
//! per tutte e due le strade perché di codice ce n'è uno.
//!
//! Vale anche il verso della dipendenza di sempre: qui non si va a prendere
//! niente. `aether-app` non conosce né `aether-spotify` né `aether-archivio`, e
//! quel silenzio è ciò che rende impossibile tenere una transazione SQLite
//! aperta mentre si scompatta un archivio da trecento megabyte.
//!
//! # Il piano è l'esecuzione, annullata
//!
//! Come per [`crate::import_legacy`] e [`crate::import_spotify`]: [`plan`] fa
//! l'importazione **vera** dentro una transazione abbandonata. Qui il principio
//! pesa più che altrove, perché quel che sta per succedere è la scrittura meno
//! reversibile dell'applicazione: decine di migliaia di righe di cronologia in
//! mezzo a quelle vere. L'utente deve poter vedere i numeri *prima*.
//!
//! # E rifarla non raddoppia niente
//!
//! Le tre scritture ripetibili hanno tre guardie diverse, e nessuna delle tre è
//! un caso:
//!
//! - le playlist si **sostituiscono** (`prepara_playlist`), quindi la seconda
//!   passata non accoda;
//! - la cronologia ha `WHERE NOT EXISTS (track_id, played_at)`, la stessa
//!   guardia di `import_legacy`, servita dall'indice che `005_account.sql` ha
//!   messo lì apposta;
//! - i conteggi passano da [`merge_stats`], che prende il massimo e non la
//!   somma — è tutta la ragione per cui quella funzione esiste.
//!
//! # Quel che questa importazione non fa
//!
//! **Non toglie.** Un brano che è preferito qui e non è più fra i «Brani che ti
//! piacciono» lassù resta preferito; una playlist cancellata su Spotify resta.
//! Importare è portare dentro: una sincronia che cancella è un'altra funzione,
//! con un'altra conferma, e confonderle vorrebbe dire che chi importa il proprio
//! account per curiosità si ritrova la libreria potata.

use std::collections::HashSet;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::PlaylistKey;
use aether_domain::merge::{TrackStats, merge_stats};
use aether_domain::spotify::SpotifyTrack;
use aether_domain::spotify_account::{
    AccountSnapshot, AlbumSpotify, PlaylistSpotify, Scelte, plan_account_import,
};
use aether_domain::spotify_plan::{Gradino, SpotifyPlan};
use rusqlite::{Connection, Transaction};

use crate::import_spotify::{
    MissingTrack, Sorgente, SpotifyImportReport, Truncation, leggi_libreria, prepara_playlist,
    riempi_playlist, scrivi_desiderati, scrivi_identificativi,
};
use crate::library::{db_error, now_ms, rebuild_aggregates};

/// Una playlist che si è rifiutato di importare, e perché.
///
/// Un account ne porta duecento: far fallire l'importazione intera perché una
/// sola ha un nome vuoto, o è arrivata monca, o si scontra con una playlist
/// automatica che esiste già, vorrebbe dire buttare via le altre
/// centonovantanove. Si salta quella, si dice quale, e si va avanti.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedPlaylist {
    /// Come si chiamava su Spotify.
    pub name: String,
    /// Il codice d'errore stabile, lo stesso che passa per l'IPC.
    pub code: String,
}

/// Quel che della cronologia non è diventato un ascolto.
///
/// Il gemello serializzabile di
/// [`aether_domain::spotify_account::ScartiCronologia`]: il dominio non ha serde
/// fra le dipendenze e non deve averlo, perché il formato con cui una cosa si
/// scrive è una decisione di chi la scrive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySkipped {
    /// Righe uguali a un'altra: l'archivio ripete lo stesso ascolto fra un file
    /// e l'altro.
    pub duplicates: usize,
    /// Ascoltate troppo poco per contare.
    pub too_short: usize,
    /// Brani che in libreria non ci sono — e che **non** finiscono fra i
    /// desiderati: vedi `ScartiCronologia::non_in_libreria`.
    pub not_in_library: usize,
}

impl HistorySkipped {
    /// Quante righe in tutto sono rimaste fuori.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.duplicates
            .saturating_add(self.too_short)
            .saturating_add(self.not_in_library)
    }
}

/// Cosa l'importazione di un account porterebbe, o ha portato.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountImportReport {
    /// Da quale delle due strade: `api` o `archivio`.
    pub source: String,
    /// Il nome visualizzato dell'account, se si è potuto leggere.
    pub profile: Option<String>,
    /// L'identificativo dell'utente su Spotify.
    pub spotify_user_id: Option<String>,
    /// La cronologia che è arrivata è tutta quella che esiste?
    ///
    /// Falso per l'API, che ne dà cinquanta righe. Serve a non far sembrare un
    /// guasto il limite di un endpoint.
    pub full_history: bool,
    /// Un rapporto per playlist, nella forma che la finestra già conosce.
    pub playlists: Vec<SpotifyImportReport>,
    /// Le playlist saltate, con il perché.
    pub rejected_playlists: Vec<RejectedPlaylist>,
    /// I «Brani che ti piacciono».
    pub liked: SpotifyImportReport,
    /// Quanti brani sono stati segnati come preferiti **adesso**.
    ///
    /// Meno di `liked.matched` alla seconda passata, e zero alla terza: chi era
    /// già segnato non si conta due volte.
    pub liked_marked: usize,
    /// Un rapporto per album salvato che portava le proprie tracce.
    pub albums: Vec<SpotifyImportReport>,
    /// Quanti album salvati si sono visti.
    pub albums_seen: usize,
    /// Su quanti brani si è scritto `spotify_album_id`.
    pub album_ids_written: usize,
    /// Quanti artisti seguiti si sono visti.
    pub artists_seen: usize,
    /// Su quanti si è scritto `artists.spotify_id`.
    pub artists_linked: usize,
    /// Quante righe di cronologia sono entrate in `play_history`.
    pub history_rows: usize,
    /// E quante no, divise per motivo.
    pub history_skipped: HistorySkipped,
    /// Su quanti brani `play_count` o `last_played_at` sono cambiati.
    pub stats_updated: usize,
    /// Quanti brani scaricati in passato sono tornati nella loro playlist.
    pub playlist_restored: usize,
    /// Quante righe di «desiderati» si sono chiuse perché il brano ormai c'è.
    pub wanted_closed: usize,
}

impl AccountImportReport {
    /// Quanti brani in tutto sono stati ritrovati in libreria.
    #[must_use]
    pub fn matched(&self) -> usize {
        self.elenchi().map(|r| r.matched).sum()
    }

    /// Quanti no.
    ///
    /// È la voce che conta più di tutte, per la ragione scritta in
    /// [`crate::import_legacy`]: sono l'unica cosa che l'utente non può
    /// ricostruire dopo.
    #[must_use]
    pub fn missing(&self) -> usize {
        self.elenchi().map(|r| r.missing).sum()
    }

    /// Quante righe di «desiderati» sono state scritte.
    #[must_use]
    pub fn wanted_rows(&self) -> usize {
        self.elenchi().map(|r| r.wanted_rows).sum()
    }

    /// Tutti i rapporti per elenco: playlist, album e preferiti.
    fn elenchi(&self) -> impl Iterator<Item = &SpotifyImportReport> {
        self.playlists
            .iter()
            .chain(self.albums.iter())
            .chain(std::iter::once(&self.liked))
    }
}

/// Cosa succederebbe importando l'account, senza scrivere niente.
///
/// Richiede `&mut` perché apre una transazione per davvero, e quel `&mut` è
/// l'unica traccia onesta di cosa sta facendo.
///
/// # Errori
///
/// `db.queryFailed` quando il database non risponde. Le playlist che non si
/// possono importare **non** sono un errore: finiscono in
/// [`AccountImportReport::rejected_playlists`].
pub fn plan(
    connection: &mut Connection,
    snapshot: &AccountSnapshot,
    scelte: &Scelte,
) -> Result<AccountImportReport, AppError> {
    run(connection, snapshot, scelte, false)
}

/// Importa per davvero.
///
/// # Errori
///
/// Gli stessi di [`plan`].
pub fn import(
    connection: &mut Connection,
    snapshot: &AccountSnapshot,
    scelte: &Scelte,
) -> Result<AccountImportReport, AppError> {
    run(connection, snapshot, scelte, true)
}

/// L'importazione. `commit` decide se resta.
fn run(
    connection: &mut Connection,
    snapshot: &AccountSnapshot,
    scelte: &Scelte,
    commit: bool,
) -> Result<AccountImportReport, AppError> {
    // Una volta sola, e non una per elenco: un account da duecento playlist la
    // rileggerebbe duecento volte, e su una libreria di cinquantamila righe è la
    // differenza fra un'attesa e nessuna. È il motivo per cui
    // `plan_account_import` prende la libreria come argomento.
    let libreria = leggi_libreria(connection)?;
    let piano = plan_account_import(snapshot, &libreria, scelte);
    drop(libreria);

    let mut rapporto = AccountImportReport {
        source: snapshot.provenienza.nome().to_owned(),
        profile: snapshot.profilo.clone(),
        spotify_user_id: snapshot.spotify_user_id.clone(),
        full_history: snapshot.provenienza.ha_cronologia_completa(),
        albums_seen: snapshot.album.len(),
        artists_seen: piano.artisti,
        history_skipped: HistorySkipped {
            duplicates: piano.scarti.doppioni,
            too_short: piano.scarti.troppo_brevi,
            not_in_library: piano.scarti.non_in_libreria,
        },
        ..AccountImportReport::default()
    };

    let adesso = now_ms();
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione", &err))?;

    let mut scritti_album = 0_usize;

    for voce in &piano.playlist {
        let Some(playlist) = snapshot.playlist.get(voce.indice) else {
            continue;
        };
        match scrivi_playlist(&tx, playlist, &voce.piano, snapshot.provenienza.nome()) {
            Ok(fatto) => {
                scritti_album = scritti_album.saturating_add(fatto.spotify_album_ids_written);
                rapporto.playlists.push(fatto);
            }
            Err(err) if e_rifiuto(&err) => rapporto.rejected_playlists.push(RejectedPlaylist {
                name: playlist.nome.clone(),
                code: err.code().kind().code().to_owned(),
            }),
            Err(err) => return Err(err),
        }
    }

    if scelte.preferiti {
        rapporto.liked = scrivi_elenco(
            &tx,
            &snapshot.preferiti,
            &piano.preferiti,
            Sorgente {
                kind: "preferiti",
                id: SORGENTE_PREFERITI,
                titolo: TITOLO_PREFERITI,
            },
            snapshot.provenienza.nome(),
            None,
        )?;
        scritti_album = scritti_album.saturating_add(rapporto.liked.spotify_album_ids_written);
        rapporto.liked_marked = segna_preferiti(&tx, &piano.preferiti, adesso)?;
    }

    for voce in &piano.album {
        let Some(album) = snapshot.album.get(voce.indice) else {
            continue;
        };
        if album.brani.is_empty() {
            scritti_album = scritti_album.saturating_add(lega_album(&tx, album)?);
            continue;
        }
        let identita = identita_album(album);
        let fatto = scrivi_elenco(
            &tx,
            &album.brani,
            &voce.piano,
            Sorgente {
                kind: "album",
                id: &identita,
                titolo: &album.titolo,
            },
            snapshot.provenienza.nome(),
            None,
        )?;
        scritti_album = scritti_album.saturating_add(fatto.spotify_album_ids_written);
        rapporto.albums.push(fatto);
    }
    rapporto.album_ids_written = scritti_album;

    if scelte.artisti {
        rapporto.artists_linked = lega_artisti(&tx, &snapshot.artisti)?;
    }

    if !piano.cronologia.is_empty() {
        let (righe, toccati) = scrivi_cronologia(&tx, &piano.cronologia)?;
        rapporto.history_rows = righe;
        rapporto.stats_updated = aggiorna_statistiche(&tx, &toccati, adesso)?;
    }

    // `None` e non una sorgente sola: un account ne ha toccate tante, e
    // restringere qui vorrebbe dire ripetere la riconciliazione una volta per
    // playlist su una tabella che le contiene tutte.
    let ritorno = crate::desiderati::riconcilia(&tx, None)?;
    rapporto.playlist_restored = ritorno.voci_rimesse;
    rapporto.wanted_closed = ritorno.righe_chiuse;

    ricorda_account(&tx, snapshot, adesso)?;

    // Solo se qualcosa è cambiato: ricostruire gli aggregati su una libreria
    // grande non è gratis.
    if scritti_album > 0 {
        rebuild_aggregates(&tx)?;
    }

    if commit {
        tx.commit()
            .map_err(|err| db_error("chiusura dell'importazione", &err))?;
    } else {
        // Abbandonata di proposito: vedi la nota in testa al modulo.
        tx.rollback()
            .map_err(|err| db_error("annullamento del piano", &err))?;
    }
    Ok(rapporto)
}

/// L'identificativo con cui i «Brani che ti piacciono» compaiono in
/// `spotify_wanted`.
///
/// Non è un identificativo di Spotify e non fa finta di esserlo: quell'elenco
/// non ne ha uno, perché non è una playlist. Il prefisso `aether:` lo dichiara,
/// così nessuno prova a metterlo in un indirizzo.
const SORGENTE_PREFERITI: &str = "aether:spotify:preferiti";

/// Come si chiama, per chi legge la coda di scaricamento.
const TITOLO_PREFERITI: &str = "Brani che ti piacciono";

/// Un errore che riguarda **una** playlist e non l'importazione.
///
/// I tre casi sono dichiarati uno per uno, non riconosciuti per esclusione: un
/// `db.queryFailed` deve fermare tutto, e una lista di eccezioni si allarga solo
/// quando qualcuno decide di allargarla.
fn e_rifiuto(err: &AppError) -> bool {
    matches!(
        err.code(),
        ErrorCode::LibraryPlaylistNameInvalid { .. }
            | ErrorCode::LibraryPlaylistIsSmart { .. }
            | ErrorCode::LibraryPlaylistExists { .. }
            | ErrorCode::SpotifyTracklistTruncated { .. }
    )
}

/// L'identificativo con cui una playlist compare in `spotify_wanted`.
///
/// `(track_key, source_id)` è l'indice unico di quella tabella: due playlist che
/// condividessero l'identificativo diventerebbero una riga sola per ogni brano
/// che manca a entrambe, e la seconda perderebbe il proprio posto in coda.
///
/// Dall'API l'identificativo c'è. Dall'archivio no — `Playlist1.json` non lo
/// contiene — e allora se ne fabbrica uno dalla chiave del nome: stabile fra due
/// letture dello stesso archivio, che è la sola proprietà che serve, e distinto
/// per playlist, che è la sola che conta.
fn identita_playlist(playlist: &PlaylistSpotify) -> String {
    match &playlist.spotify_id {
        Some(id) => id.clone(),
        None => format!(
            "aether:archivio:playlist:{}",
            PlaylistKey::compute(Some(&playlist.nome))
        ),
    }
}

/// Lo stesso, per un album salvato.
fn identita_album(album: &AlbumSpotify) -> String {
    match &album.spotify_id {
        Some(id) => id.clone(),
        None => format!(
            "aether:archivio:album:{}",
            PlaylistKey::compute(Some(&album.titolo))
        ),
    }
}

/// Crea o sostituisce la playlist, la riempie, e scrive il resto.
fn scrivi_playlist(
    tx: &Transaction<'_>,
    playlist: &PlaylistSpotify,
    piano: &SpotifyPlan,
    provenienza: &str,
) -> Result<SpotifyImportReport, AppError> {
    let identita = identita_playlist(playlist);
    let (id, creata, sostituita) = prepara_playlist(
        tx,
        &playlist.nome,
        playlist.spotify_id.as_deref(),
        playlist.troncatura(),
    )?;
    let voci = riempi_playlist(
        tx,
        id,
        &piano
            .abbinati
            .iter()
            .map(|a| (a.indice, a.track_id))
            .collect::<Vec<_>>(),
    )?;

    let mut rapporto = scrivi_elenco(
        tx,
        &playlist.brani,
        piano,
        Sorgente {
            kind: "playlist",
            id: &identita,
            titolo: &playlist.nome,
        },
        provenienza,
        Some(id),
    )?;
    rapporto.playlist_id = Some(id);
    rapporto.playlist_name = Some(playlist.nome.trim().to_owned());
    rapporto.playlist_created = creata;
    rapporto.playlist_replaced = sostituita;
    rapporto.playlist_entries = voci;
    rapporto.truncated = playlist
        .troncatura()
        .map(|(read, expected)| Truncation { read, expected });
    Ok(rapporto)
}

/// Il rapporto di un elenco, e le due scritture che ogni elenco fa: gli
/// identificativi esterni sui brani ritrovati, i desiderati su quelli no.
fn scrivi_elenco(
    tx: &Transaction<'_>,
    brani: &[SpotifyTrack],
    piano: &SpotifyPlan,
    sorgente: Sorgente<'_>,
    provenienza: &str,
    playlist_id: Option<i64>,
) -> Result<SpotifyImportReport, AppError> {
    let mut rapporto = SpotifyImportReport {
        kind: sorgente.kind.to_owned(),
        // Qui `source` è la strada dell'account — `api` o `archivio` — dove
        // nell'importazione da un link è il livello del lettore keyless. È lo
        // stesso campo perché risponde alla stessa domanda: da dove vengono
        // questi brani.
        source: provenienza.to_owned(),
        source_id: sorgente.id.to_owned(),
        title: sorgente.titolo.to_owned(),
        resolved: brani.len(),
        matched: piano.abbinati.len(),
        missing: piano.mancanti.len(),
        matched_isrc: piano.per_gradino(Gradino::Isrc),
        matched_exact: piano.per_gradino(Gradino::ChiaveEsatta),
        matched_by_title: piano.per_gradino(Gradino::ArtistaTitolo),
        matched_stripped: piano.per_gradino(Gradino::Ripulito),
        ..SpotifyImportReport::default()
    };

    for indice in &piano.mancanti {
        if let Some(brano) = brani.get(*indice) {
            rapporto.missing_tracks.push(MissingTrack {
                position: indice.saturating_add(1),
                title: brano.title.clone(),
                artist: brano.artist.clone(),
                album: brano.album.clone(),
            });
        }
    }

    let mut album = 0_usize;
    let mut isrc = 0_usize;
    for abbinato in &piano.abbinati {
        let Some(brano) = brani.get(abbinato.indice) else {
            continue;
        };
        let (scritto_album, scritto_isrc) = scrivi_identificativi(tx, abbinato.track_id, brano)?;
        album = album.saturating_add(usize::from(scritto_album));
        isrc = isrc.saturating_add(usize::from(scritto_isrc));
    }
    rapporto.spotify_album_ids_written = album;
    rapporto.isrc_written = isrc;

    rapporto.wanted_rows = scrivi_desiderati(tx, brani, sorgente, &piano.mancanti, playlist_id)?;
    Ok(rapporto)
}

/// Segna come preferiti i brani ritrovati.
///
/// `liked = 0` nel `WHERE` fa due cose insieme: rende la scrittura idempotente e
/// fa sì che il conteggio dica quanti brani sono stati segnati **adesso**, non
/// quanti risultano preferiti. Alla seconda passata è zero, ed è la risposta
/// giusta.
///
/// `liked_at` prende l'ora di adesso perché Spotify, per un brano salvato, la
/// data non la dà da nessuna delle due strade: `YourLibrary.json` non ha un
/// campo per la data, e `/v1/me/tracks` la dà per pagina ma questo tipo non la
/// porta fin qui. Un istante inventato all'indietro sarebbe peggio: `liked_at` è
/// il timestamp con cui [`merge_stats`] decide chi vince fra due dispositivi, e
/// datare al 2015 una decisione presa oggi la farebbe perdere contro qualunque
/// cosa.
fn segna_preferiti(
    tx: &Transaction<'_>,
    piano: &SpotifyPlan,
    adesso: i64,
) -> Result<usize, AppError> {
    let mut segna = tx
        .prepare_cached(
            "UPDATE tracks SET liked = 1, liked_at = ?2, stats_updated_at = ?2
              WHERE id = ?1 AND liked = 0",
        )
        .map_err(|err| db_error("segnatura dei preferiti", &err))?;
    let mut quanti = 0_usize;
    for abbinato in &piano.abbinati {
        quanti = quanti.saturating_add(
            segna
                .execute(rusqlite::params![abbinato.track_id, adesso])
                .map_err(|err| db_error("segnatura dei preferiti", &err))?,
        );
    }
    Ok(quanti)
}

/// Lega un album salvato ai brani che in libreria portano quel titolo.
///
/// # Perché sui brani e non su `albums`
///
/// Perché `albums` si ricostruisce: [`rebuild_aggregates`] fa `DELETE FROM
/// albums` e riscrive tutto dai brani, `spotify_id` compreso. Un identificativo
/// scritto lì sopravvivrebbe fino alla prima scansione e poi sparirebbe, senza
/// un errore e senza che nessuno lo colleghi alla scansione. `tracks
/// .spotify_album_id` è invece la sorgente da cui quella ricostruzione lo
/// **prende**, e da lì risale in `albums` a ogni passata.
///
/// # Perché serve l'artista
///
/// Perché «Greatest Hits» esiste per quattrocento artisti diversi. Un album
/// salvato di cui Spotify non dice l'autore si lascia stare: legarlo per il solo
/// titolo scriverebbe l'identificativo sbagliato su dischi che non c'entrano, e
/// [`aether_domain::album`] fonde le edizioni proprio guardando quel campo — cioè
/// unirebbe album diversi in uno.
fn lega_album(tx: &Transaction<'_>, album: &AlbumSpotify) -> Result<usize, AppError> {
    let (Some(id), Some(artista)) = (album.spotify_id.as_deref(), album.artista.as_deref()) else {
        return Ok(0);
    };
    let quanti = tx
        .execute(
            "UPDATE tracks SET spotify_album_id = ?1
              WHERE (spotify_album_id IS NULL OR spotify_album_id = '')
                AND TRIM(album) = TRIM(?2) COLLATE NOCASE
                AND TRIM(COALESCE(NULLIF(TRIM(album_artist), ''), artist))
                    = TRIM(?3) COLLATE NOCASE",
            rusqlite::params![id, album.titolo, artista],
        )
        .map_err(|err| db_error("collegamento di un album salvato", &err))?;
    Ok(quanti)
}

/// Scrive `artists.spotify_id` per gli artisti seguiti che la libreria conosce.
///
/// Solo per quelli che ci sono già: `artists` non è un elenco di gusti, è
/// l'insieme di chi firma un brano sul disco, e [`rebuild_aggregates`] cancella
/// da lì chiunque non sia più nominato. Inserire una riga per un artista che si
/// segue e non si ascolta la farebbe sparire alla prima scansione.
fn lega_artisti(
    tx: &Transaction<'_>,
    artisti: &[aether_domain::spotify_account::ArtistaSpotify],
) -> Result<usize, AppError> {
    let mut lega = tx
        .prepare_cached(
            "UPDATE artists SET spotify_id = ?2
              WHERE name = ?1 COLLATE NOCASE AND (spotify_id IS NULL OR spotify_id = '')",
        )
        .map_err(|err| db_error("collegamento degli artisti seguiti", &err))?;
    let mut quanti = 0_usize;
    for artista in artisti {
        let Some(id) = artista.spotify_id.as_deref() else {
            continue;
        };
        quanti = quanti.saturating_add(
            lega.execute(rusqlite::params![artista.nome.trim(), id])
                .map_err(|err| db_error("collegamento degli artisti seguiti", &err))?,
        );
    }
    Ok(quanti)
}

/// Versa la cronologia in `play_history`, e dice quali brani ha toccato.
///
/// `source = 'spotify'` è ciò che rende l'operazione annullabile: senza, questi
/// ascolti sarebbero indistinguibili da quelli veri il giorno dopo, e l'unico
/// modo di tornare indietro sarebbe ripristinare un backup — cioè perdere anche
/// tutto quel che si è fatto nel frattempo.
///
/// La guardia `WHERE NOT EXISTS` è la stessa di
/// [`crate::import_legacy`]: due ascolti dello stesso brano nello stesso
/// millisecondo sono lo stesso ascolto contato due volte. Su
/// `idx_play_history_brano_quando` costa una ricerca; sull'indice che c'era prima
/// costava una scansione di tutti gli ascolti di quel brano, moltiplicata per le
/// decine di migliaia di righe di un archivio.
fn scrivi_cronologia(
    tx: &Transaction<'_>,
    ascolti: &[aether_domain::spotify_account::AscoltoAbbinato],
) -> Result<(usize, HashSet<i64>), AppError> {
    let mut inserisci = tx
        .prepare_cached(
            "INSERT INTO play_history (track_id, played_at, ms_played, source)
             SELECT ?1, ?2, ?3, 'spotify'
              WHERE NOT EXISTS (
                SELECT 1 FROM play_history WHERE track_id = ?1 AND played_at = ?2
              )",
        )
        .map_err(|err| db_error("scrittura della cronologia importata", &err))?;

    let mut righe = 0_usize;
    let mut toccati = HashSet::new();
    for ascolto in ascolti {
        let ms = i64::try_from(ascolto.ms_ascoltati).unwrap_or(i64::MAX);
        righe = righe.saturating_add(
            inserisci
                .execute(rusqlite::params![ascolto.track_id, ascolto.iniziato_ms, ms])
                .map_err(|err| db_error("scrittura della cronologia importata", &err))?,
        );
        toccati.insert(ascolto.track_id);
    }
    Ok((righe, toccati))
}

/// Riallinea `play_count` e `last_played_at` ai brani toccati.
///
/// # Il massimo, non la somma
///
/// La decisione la prende [`merge_stats`], che è dove sta scritta per tutta
/// l'applicazione, e prende il **massimo**. Sommare sembra giusto — «gli ascolti
/// di prima più quelli importati» — ed è il difetto che raddoppia la storia a
/// ogni ripetizione della stessa importazione: le righe non si duplicano grazie
/// alla guardia, ma un conteggio che somma sì.
///
/// # Cosa questo comporta, detto
///
/// `play_count` diventa il massimo fra quel che c'era e **quante righe di
/// cronologia esistono adesso** per quel brano. Per i brani suonati in Aether i
/// due numeri crescono insieme (`playback::record_play` scrive tutti e due), e la
/// somma viene da sé. Per un brano che ha un conteggio ereditato dal vecchio
/// database senza righe di cronologia — `import_legacy` porta le statistiche
/// anche quando la cronologia è orfana — i cinquanta ascolti importati da Spotify
/// non si vedono finché non superano quel conteggio. È il prezzo
/// dell'idempotenza, ed è il verso giusto in cui sbagliare: un numero che non
/// sale abbastanza si nota, uno che raddoppia da solo no.
fn aggiorna_statistiche(
    tx: &Transaction<'_>,
    toccati: &HashSet<i64>,
    adesso: i64,
) -> Result<usize, AppError> {
    if toccati.is_empty() {
        return Ok(0);
    }

    // Una query sola, raggruppata, invece di due per brano: su un archivio che
    // tocca cinquemila brani sarebbero diecimila viaggi per una domanda che
    // l'indice `(track_id, played_at)` risponde in una scansione.
    let dalla_cronologia: Vec<(i64, i64, Option<i64>, TrackStats)> = {
        let mut lettura = tx
            .prepare(
                "SELECT h.track_id, COUNT(*), MAX(h.played_at),
                        t.play_count, t.last_played_at, t.rating, t.liked, t.liked_at,
                        t.stats_updated_at
                   FROM play_history AS h
                   JOIN tracks AS t ON t.id = h.track_id
                  GROUP BY h.track_id",
            )
            .map_err(|err| db_error("conteggio degli ascolti", &err))?;
        let righe = lettura
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    TrackStats {
                        play_count: row.get(3)?,
                        last_played_at: row.get(4)?,
                        rating: row.get::<_, i64>(5)?.clamp(0, 5).try_into().unwrap_or(0),
                        liked: row.get::<_, i64>(6)? != 0,
                        liked_at: row.get(7)?,
                        stats_updated_at: row.get(8)?,
                    },
                ))
            })
            .map_err(|err| db_error("conteggio degli ascolti", &err))?;
        righe
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("conteggio degli ascolti", &err))?
    };

    let mut scrivi = tx
        .prepare_cached(
            "UPDATE tracks SET play_count = ?2, last_played_at = ?3, stats_updated_at = ?4
              WHERE id = ?1",
        )
        .map_err(|err| db_error("aggiornamento delle statistiche", &err))?;

    let mut quanti = 0_usize;
    for (track_id, conteggio, ultimo, locale) in dalla_cronologia {
        if !toccati.contains(&track_id) {
            continue;
        }
        // Quel che la cronologia sa dire, e nient'altro: voto e preferito
        // restano di `locale` perché `merge_stats` li tratta come «non
        // espressi» quando arrivano a zero.
        let dalla_storia = TrackStats {
            play_count: conteggio,
            last_played_at: ultimo,
            stats_updated_at: adesso,
            ..TrackStats::default()
        };
        let fuse = merge_stats(&locale, &dalla_storia);
        if fuse.play_count == locale.play_count && fuse.last_played_at == locale.last_played_at {
            continue;
        }
        scrivi
            .execute(rusqlite::params![
                track_id,
                fuse.play_count,
                fuse.last_played_at,
                fuse.stats_updated_at,
            ])
            .map_err(|err| db_error("aggiornamento delle statistiche", &err))?;
        quanti = quanti.saturating_add(1);
    }
    Ok(quanti)
}

/// Registra di chi era l'account e quando lo si è letto.
///
/// Non fa niente senza un identificativo utente: una riga con chiave primaria
/// vuota sarebbe *la* riga, e il secondo account importato prenderebbe il posto
/// del primo invece di aggiungersi.
fn ricorda_account(
    tx: &Transaction<'_>,
    snapshot: &AccountSnapshot,
    adesso: i64,
) -> Result<(), AppError> {
    let Some(utente) = snapshot.spotify_user_id.as_deref() else {
        return Ok(());
    };
    tx.execute(
        "INSERT INTO spotify_account (spotify_user_id, display_name, last_sync_at, last_source)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(spotify_user_id) DO UPDATE SET
             display_name = excluded.display_name,
             last_sync_at = excluded.last_sync_at,
             last_source  = excluded.last_source",
        rusqlite::params![
            utente,
            snapshot.profilo,
            adesso,
            snapshot.provenienza.nome()
        ],
    )
    .map_err(|err| db_error("registrazione dell'account Spotify", &err))?;
    Ok(())
}

/// L'account che risulta collegato, come il database se lo ricorda.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRegistrato {
    /// L'identificativo dell'utente su Spotify.
    pub spotify_user_id: String,
    /// Il nome visualizzato, quando si è potuto leggere.
    pub display_name: Option<String>,
    /// Quando è finita l'ultima importazione riuscita.
    pub last_sync_at: Option<i64>,
    /// Da quale via: `api` o `archivio`.
    pub last_source: String,
}

/// Chi risulta collegato, se qualcuno.
///
/// `ORDER BY last_sync_at DESC` e non `LIMIT 1` su una tabella che di righe ne
/// ha una: la chiave primaria è l'utente Spotify, e chi importa due account
/// diversi ne ha due. Mostrare quello sincronizzato per ultimo è la risposta
/// giusta a «di chi è questo?».
///
/// # Errori
///
/// `db.queryFailed` quando il database non risponde.
pub fn registrato(connection: &Connection) -> Result<Option<AccountRegistrato>, AppError> {
    let esito = connection.query_row(
        "SELECT spotify_user_id, display_name, last_sync_at, last_source
           FROM spotify_account ORDER BY last_sync_at DESC LIMIT 1",
        [],
        |row| {
            Ok(AccountRegistrato {
                spotify_user_id: row.get(0)?,
                display_name: row.get(1)?,
                last_sync_at: row.get(2)?,
                last_source: row.get(3)?,
            })
        },
    );
    match esito {
        Ok(riga) => Ok(Some(riga)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(db_error("lettura dell'account collegato", &err)),
    }
}

/// Quanti ascolti in `play_history` vengono da un'importazione.
///
/// È il numero che rende «dimentica gli ascolti importati» un tasto che dice
/// quel che sta per cancellare, invece di uno che chiede di fidarsi.
///
/// # Errori
///
/// `db.queryFailed` quando il database non risponde.
pub fn quanti_importati(connection: &Connection) -> Result<i64, AppError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM play_history WHERE source = 'spotify'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio degli ascolti importati", &err))
}

/// Dimentica di quale account si trattava.
///
/// Una `DELETE`, non quattro stringhe vuote: è la ragione per cui
/// `spotify_account` è una tabella e non quattro chiavi in `settings`, ed è
/// scritta per esteso in `005_account.sql`.
///
/// **Non** tocca niente di quel che è stato importato — le playlist restano, i
/// preferiti restano, la cronologia resta. Scollegarsi è smettere di parlare con
/// Spotify; disfare è [`dimentica_importati`], che dice cosa cancella.
///
/// # Errori
///
/// `db.queryFailed` quando il database non risponde.
pub fn scollega(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute("DELETE FROM spotify_account", [])
        .map(|_| ())
        .map_err(|err| db_error("scollegamento dell'account", &err))
}

/// Dimentica gli ascolti importati da Spotify.
///
/// L'operazione che `play_history.source` esiste per rendere possibile. Cancella
/// le righe importate e riallinea i conteggi dei brani che ne avevano.
///
/// # Perché i conteggi non tornano esattamente com'erano
///
/// Perché `merge_stats` non sa scendere, e non deve: `play_count` è l'unica cosa
/// in tutta la libreria che non si può ricostruire, e una funzione che lo
/// abbassa è una funzione che prima o poi lo abbassa quando non doveva. Le righe
/// se ne vanno — «più ascoltati» smette di essere dominato dall'archivio — e il
/// contatore resta al valore più alto che ha toccato. Chi vuole anche quello
/// azzerato ha il ripristino da backup, che è l'operazione giusta per «rimetti
/// tutto com'era».
///
/// Restituisce quante righe se ne sono andate.
///
/// # Errori
///
/// `db.queryFailed` quando il database non risponde.
pub fn dimentica_importati(connection: &mut Connection) -> Result<usize, AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione", &err))?;
    let quante = tx
        .execute("DELETE FROM play_history WHERE source = 'spotify'", [])
        .map_err(|err| db_error("cancellazione degli ascolti importati", &err))?;
    tx.execute(
        "UPDATE tracks SET last_played_at = (
             SELECT MAX(played_at) FROM play_history WHERE track_id = tracks.id
         )
         WHERE EXISTS (SELECT 1 FROM play_history WHERE track_id = tracks.id)",
        [],
    )
    .map_err(|err| db_error("riallineamento degli ultimi ascolti", &err))?;
    tx.commit()
        .map_err(|err| db_error("chiusura della cancellazione", &err))?;
    Ok(quante)
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::keys::{TrackKey, TrackKeyInput};
    use aether_domain::spotify_account::{
        ArtistaSpotify, AscoltoSpotify, PlaylistSpotify, Provenienza,
    };

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
            "INSERT INTO tracks (path, track_key, title, artist, album, album_key, duration_ms,
                                 file_size, date_added, date_modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0)",
            rusqlite::params![
                format!("C:/musica/{artista} - {titolo}.mp3"),
                chiave.as_str(),
                titolo,
                artista,
                album,
                format!("{}|{}", artista.to_lowercase(), album.to_lowercase()),
                durata
            ],
        );
        assert!(esito.is_ok(), "l'inserimento di prova deve riuscire");
    }

    fn sp(artista: &str, titolo: &str, album: &str, durata: u64) -> SpotifyTrack {
        SpotifyTrack {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            album: Some(album.to_owned()),
            duration_ms: Some(durata),
            ..SpotifyTrack::default()
        }
    }

    fn conta(connection: &Connection, sql: &str) -> i64 {
        connection.query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
    }

    /// Due brani in libreria, e uno snapshot che li nomina tutti e due più un
    /// terzo che non c'è.
    fn libreria_di_prova(connection: &Connection) {
        aggiungi(connection, "Blur", "Song 2", "Blur", 180_000);
        aggiungi(
            connection,
            "Gorillaz",
            "Feel Good Inc",
            "Demon Days",
            222_000,
        );
    }

    fn snapshot_completo() -> AccountSnapshot {
        AccountSnapshot {
            profilo: Some("Tizio".to_owned()),
            spotify_user_id: Some("utente123".to_owned()),
            playlist: vec![PlaylistSpotify {
                nome: "Corsa".to_owned(),
                spotify_id: Some("37i9dQZF1DXcBWIGoYBM5M".to_owned()),
                brani: vec![
                    sp("Blur", "Song 2", "Blur", 180_000),
                    sp("Nessuno", "Mai Sentita", "Ignoto", 200_000),
                    sp("Gorillaz", "Feel Good Inc", "Demon Days", 222_000),
                ],
                ..PlaylistSpotify::default()
            }],
            preferiti: vec![sp("Blur", "Song 2", "Blur", 180_000)],
            artisti: vec![ArtistaSpotify {
                nome: "Blur".to_owned(),
                spotify_id: Some("7MSUfLeTdDEoZiJPDSBXgi".to_owned()),
            }],
            cronologia: vec![
                AscoltoSpotify {
                    finito_ms: 1_000_000,
                    ms_ascoltati: 120_000,
                    brano: sp("Blur", "Song 2", "Blur", 180_000),
                },
                AscoltoSpotify {
                    finito_ms: 2_000_000,
                    ms_ascoltati: 120_000,
                    brano: sp("Blur", "Song 2", "Blur", 180_000),
                },
            ],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        }
    }

    #[test]
    fn importare_due_volte_non_raddoppia_niente() {
        // La prova che conta più di tutte. Tre scritture ripetibili, tre
        // guardie diverse: la playlist si sostituisce, la cronologia ha
        // `WHERE NOT EXISTS`, e i conteggi passano da `merge_stats` che prende
        // il massimo. Basta che una delle tre manchi perché l'utente si
        // ritrovi la storia d'ascolto raddoppiata senza nessun avviso.
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = snapshot_completo();

        let Ok(primo) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la prima importazione deve riuscire");
        };
        assert_eq!(primo.history_rows, 2);
        assert_eq!(primo.liked_marked, 1);

        let Ok(secondo) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la seconda importazione deve riuscire");
        };

        assert_eq!(secondo.history_rows, 0, "nessun ascolto nuovo");
        assert_eq!(secondo.liked_marked, 0, "era già segnato");
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM play_history"), 2);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 1);
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM playlist_tracks"),
            2,
            "le voci si sostituiscono, non si accodano"
        );
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM spotify_wanted"),
            1,
            "e i desiderati non si accumulano"
        );
        assert_eq!(
            conta(&db, "SELECT play_count FROM tracks WHERE title = 'Song 2'"),
            2,
            "il massimo, non la somma"
        );
    }

    #[test]
    fn il_piano_non_scrive_niente() {
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = snapshot_completo();

        let Ok(rapporto) = plan(&mut db, &snapshot, &Scelte::default()) else {
            panic!("il piano deve riuscire");
        };
        assert_eq!(
            rapporto.matched(),
            3,
            "due in playlist, uno fra i preferiti"
        );
        assert_eq!(rapporto.missing(), 1);
        assert_eq!(rapporto.history_rows, 2);

        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 0);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM play_history"), 0);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM spotify_wanted"), 0);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM spotify_account"), 0);
        assert_eq!(conta(&db, "SELECT SUM(liked) FROM tracks"), 0);
    }

    #[test]
    fn il_piano_dice_esattamente_quel_che_farà_limportazione() {
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = snapshot_completo();

        let (Ok(previsto), Ok(fatto)) = (
            plan(&mut db, &snapshot, &Scelte::default()),
            import(&mut db, &snapshot, &Scelte::default()),
        ) else {
            panic!("entrambi devono riuscire");
        };
        // L'identificativo della playlist è l'unico campo che può differire: nel
        // piano la riga viene creata e annullata, e SQLite non riusa quel numero.
        let previsto = AccountImportReport {
            playlists: previsto
                .playlists
                .into_iter()
                .zip(fatto.playlists.iter())
                .map(|(mut p, f)| {
                    p.playlist_id = f.playlist_id;
                    p
                })
                .collect(),
            ..previsto
        };
        assert_eq!(previsto, fatto);
    }

    #[test]
    fn la_cronologia_importata_si_riconosce_e_si_puo_dimenticare() {
        // È l'unica scrittura dell'applicazione che non si potrebbe disfare
        // guardandola: quarantamila righe in mezzo a quelle vere sarebbero
        // indistinguibili il giorno dopo.
        let mut db = database();
        libreria_di_prova(&db);
        let Ok(_) = import(&mut db, &snapshot_completo(), &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        // Un ascolto vero, suonato qui: non deve andarsene con gli altri.
        let Ok(_) = db.execute(
            "INSERT INTO play_history (track_id, played_at, ms_played)
             SELECT id, 9_000_000, 120000 FROM tracks WHERE title = 'Song 2'",
            [],
        ) else {
            panic!("l'ascolto vero di prova si deve scrivere");
        };

        assert_eq!(
            conta(
                &db,
                "SELECT COUNT(*) FROM play_history WHERE source = 'spotify'"
            ),
            2
        );
        let Ok(quante) = dimentica_importati(&mut db) else {
            panic!("dimenticare deve riuscire");
        };
        assert_eq!(quante, 2);
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM play_history"),
            1,
            "l'ascolto vero resta"
        );
        assert_eq!(
            conta(
                &db,
                "SELECT last_played_at FROM tracks WHERE title = 'Song 2'"
            ),
            9_000_000,
            "e l'ultimo ascolto torna a essere il suo"
        );
    }

    #[test]
    fn l_ascolto_entra_all_istante_in_cui_e_cominciato() {
        // L'archivio dice quando il brano ha **smesso**; `play_history` vuole
        // quando ha cominciato. Senza la sottrazione ogni ascolto scivolerebbe
        // in avanti della propria durata: invisibile su una riga, evidente su
        // dieci anni, e irreparabile una volta scritto.
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = AccountSnapshot {
            cronologia: vec![AscoltoSpotify {
                finito_ms: 1_000_000,
                ms_ascoltati: 120_000,
                brano: sp("Blur", "Song 2", "Blur", 180_000),
            }],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(conta(&db, "SELECT played_at FROM play_history"), 880_000);
    }

    #[test]
    fn rinominare_una_playlist_su_spotify_non_ne_crea_una_seconda() {
        // Il difetto che `playlists.spotify_playlist_id` esiste per togliere:
        // `PlaylistKey` nasce dal nome, quindi senza l'identificativo la
        // playlist rinominata arriverebbe qui come una playlist nuova, accanto
        // alla vecchia col nome vecchio e i suoi brani.
        let mut db = database();
        libreria_di_prova(&db);
        let mut snapshot = snapshot_completo();
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la prima importazione deve riuscire");
        };

        if let Some(playlist) = snapshot.playlist.first_mut() {
            playlist.nome = "Corsa 2026".to_owned();
        }
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la seconda importazione deve riuscire");
        };

        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 1);
        let nome: String = db
            .query_row("SELECT name FROM playlists", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(nome, "Corsa 2026", "il nome segue quel che si è deciso là");
    }

    #[test]
    fn una_playlist_senza_identificativo_ne_riceve_uno_stabile() {
        // Dall'archivio l'identificativo non arriva: `Playlist1.json` non lo
        // contiene. Se ne fabbrica uno dalla chiave del nome, perché
        // `(track_key, source_id)` è unico e due playlist che lo condividessero
        // perderebbero una riga di desiderati per ogni brano che manca a
        // entrambe.
        let mut db = database();
        libreria_di_prova(&db);
        let mancante = sp("Nessuno", "Mai Sentita", "Ignoto", 200_000);
        let snapshot = AccountSnapshot {
            playlist: vec![
                PlaylistSpotify {
                    nome: "Prima".to_owned(),
                    brani: vec![mancante.clone()],
                    ..PlaylistSpotify::default()
                },
                PlaylistSpotify {
                    nome: "Seconda".to_owned(),
                    brani: vec![mancante],
                    ..PlaylistSpotify::default()
                },
            ],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let Ok(rapporto) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.wanted_rows(), 2);
        assert_eq!(
            conta(&db, "SELECT COUNT(DISTINCT source_id) FROM spotify_wanted"),
            2,
            "lo stesso brano manca da due playlist, e sono due desideri"
        );
    }

    #[test]
    fn una_playlist_che_non_si_puo_importare_non_ferma_le_altre() {
        // Duecento playlist, e una col nome vuoto: buttare via le
        // centonovantanove buone sarebbe una punizione sproporzionata.
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = AccountSnapshot {
            playlist: vec![
                PlaylistSpotify {
                    nome: "   ".to_owned(),
                    brani: vec![sp("Blur", "Song 2", "Blur", 180_000)],
                    ..PlaylistSpotify::default()
                },
                PlaylistSpotify {
                    nome: "Buona".to_owned(),
                    brani: vec![sp("Blur", "Song 2", "Blur", 180_000)],
                    ..PlaylistSpotify::default()
                },
            ],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let Ok(rapporto) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire lo stesso");
        };
        assert_eq!(rapporto.playlists.len(), 1);
        assert_eq!(rapporto.rejected_playlists.len(), 1);
        assert_eq!(
            rapporto.rejected_playlists.first().map(|r| r.code.as_str()),
            Some("library.playlistNameInvalid")
        );
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 1);
    }

    #[test]
    fn un_elenco_monco_non_sostituisce_una_playlist_che_esiste() {
        // La stessa guardia di `import_spotify`, che qui però non fa fallire
        // tutto: la playlist si salta e si dice quale.
        let mut db = database();
        libreria_di_prova(&db);
        let mut snapshot = snapshot_completo();
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la prima importazione, completa, deve riuscire");
        };
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlist_tracks"), 2);

        if let Some(playlist) = snapshot.playlist.first_mut() {
            playlist.dichiarati = Some(300);
        }
        let Ok(rapporto) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire, saltando la playlist monca");
        };
        assert_eq!(
            rapporto.rejected_playlists.first().map(|r| r.code.as_str()),
            Some("spotify.tracklistTruncated")
        );
        assert_eq!(
            conta(&db, "SELECT COUNT(*) FROM playlist_tracks"),
            2,
            "e soprattutto non ha cancellato niente"
        );
    }

    #[test]
    fn gli_artisti_seguiti_si_legano_solo_a_chi_la_libreria_conosce() {
        // `artists` non è un elenco di gusti: `rebuild_aggregates` cancella da
        // lì chi non firma nessun brano, e una riga inserita per un artista che
        // si segue e non si ascolta sparirebbe alla prima scansione.
        let mut db = database();
        libreria_di_prova(&db);
        let Ok(_) = db.execute("INSERT INTO artists (name) VALUES ('Blur')", []) else {
            panic!("l'artista di prova si deve inserire");
        };
        let snapshot = AccountSnapshot {
            artisti: vec![
                ArtistaSpotify {
                    nome: "Blur".to_owned(),
                    spotify_id: Some("7MSUfLeTdDEoZiJPDSBXgi".to_owned()),
                },
                ArtistaSpotify {
                    nome: "Mai Ascoltato".to_owned(),
                    spotify_id: Some("altro".to_owned()),
                },
            ],
            ..AccountSnapshot::vuoto(Provenienza::Api)
        };

        let Ok(rapporto) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.artists_seen, 2);
        assert_eq!(rapporto.artists_linked, 1);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM artists"), 1);
    }

    #[test]
    fn un_album_salvato_senza_tracce_si_lega_ai_brani_che_si_hanno() {
        // È tutto quel che l'archivio può dire di un album salvato: elenca il
        // titolo e l'artista, mai le tracce. L'identificativo va sui **brani**,
        // non su `albums`, perché `rebuild_aggregates` riscrive quella tabella
        // da capo a ogni scansione.
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = AccountSnapshot {
            album: vec![AlbumSpotify {
                titolo: "Demon Days".to_owned(),
                artista: Some("Gorillaz".to_owned()),
                spotify_id: Some("0bUTHlWbkSQysoM3VsWldT".to_owned()),
                brani: Vec::new(),
            }],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let Ok(rapporto) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.albums_seen, 1);
        assert_eq!(rapporto.album_ids_written, 1);
        let letto: Option<String> = db
            .query_row(
                "SELECT spotify_album_id FROM tracks WHERE title = 'Feel Good Inc'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(None);
        assert_eq!(letto.as_deref(), Some("0bUTHlWbkSQysoM3VsWldT"));
    }

    #[test]
    fn un_album_senza_artista_non_si_lega_a_niente() {
        // «Greatest Hits» esiste per quattrocento artisti diversi, e
        // `aether_domain::album` fonde le edizioni guardando proprio quel campo:
        // legare per il solo titolo unirebbe dischi che non c'entrano.
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = AccountSnapshot {
            album: vec![AlbumSpotify {
                titolo: "Demon Days".to_owned(),
                artista: None,
                spotify_id: Some("0bUTHlWbkSQysoM3VsWldT".to_owned()),
                brani: Vec::new(),
            }],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };
        let Ok(rapporto) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.album_ids_written, 0);
    }

    #[test]
    fn le_scelte_spente_non_scrivono() {
        // Chi ha già importato le playlist e vuole solo aggiungere la
        // cronologia arrivata dopo nel secondo archivio.
        let mut db = database();
        libreria_di_prova(&db);
        let solo_cronologia = Scelte {
            playlist: false,
            preferiti: false,
            album: false,
            artisti: false,
            cronologia: true,
        };
        let Ok(rapporto) = import(&mut db, &snapshot_completo(), &solo_cronologia) else {
            panic!("l'importazione deve riuscire");
        };
        assert!(rapporto.playlists.is_empty());
        assert_eq!(rapporto.liked_marked, 0);
        assert_eq!(rapporto.history_rows, 2);
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM playlists"), 0);
        assert_eq!(conta(&db, "SELECT SUM(liked) FROM tracks"), 0);
    }

    #[test]
    fn l_account_si_ricorda_e_si_aggiorna_invece_di_moltiplicarsi() {
        let mut db = database();
        libreria_di_prova(&db);
        let mut snapshot = snapshot_completo();
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la prima importazione deve riuscire");
        };
        snapshot.profilo = Some("Tizio Rinominato".to_owned());
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("la seconda importazione deve riuscire");
        };

        assert_eq!(conta(&db, "SELECT COUNT(*) FROM spotify_account"), 1);
        let nome: String = db
            .query_row("SELECT display_name FROM spotify_account", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(nome, "Tizio Rinominato");
        let strada: String = db
            .query_row("SELECT last_source FROM spotify_account", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(strada, "archivio");
    }

    #[test]
    fn senza_identificativo_utente_non_si_scrive_nessun_account() {
        // Una chiave primaria vuota sarebbe *la* riga, e il secondo account
        // importato prenderebbe il posto del primo invece di aggiungersi.
        let mut db = database();
        libreria_di_prova(&db);
        let snapshot = AccountSnapshot {
            spotify_user_id: None,
            ..snapshot_completo()
        };
        let Ok(_) = import(&mut db, &snapshot, &Scelte::default()) else {
            panic!("l'importazione deve riuscire lo stesso");
        };
        assert_eq!(conta(&db, "SELECT COUNT(*) FROM spotify_account"), 0);
    }

    #[test]
    fn un_conteggio_gia_piu_alto_non_scende() {
        // `merge_stats` prende il massimo e non sa scendere: `play_count` è
        // l'unica cosa in libreria che non si può ricostruire.
        let mut db = database();
        libreria_di_prova(&db);
        let Ok(_) = db.execute(
            "UPDATE tracks SET play_count = 40 WHERE title = 'Song 2'",
            [],
        ) else {
            panic!("il conteggio di prova si deve scrivere");
        };
        let Ok(_) = import(&mut db, &snapshot_completo(), &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(
            conta(&db, "SELECT play_count FROM tracks WHERE title = 'Song 2'"),
            40
        );
    }

    #[test]
    fn i_preferiti_gia_segnati_non_perdono_la_loro_data() {
        // `liked_at` è il timestamp della decisione, ed è ciò con cui
        // `merge_stats` sceglie fra due dispositivi: riscriverlo a ogni
        // importazione farebbe vincere sempre l'ultima macchina che ha
        // importato.
        let mut db = database();
        libreria_di_prova(&db);
        let Ok(_) = db.execute(
            "UPDATE tracks SET liked = 1, liked_at = 12345 WHERE title = 'Song 2'",
            [],
        ) else {
            panic!("il preferito di prova si deve scrivere");
        };
        let Ok(rapporto) = import(&mut db, &snapshot_completo(), &Scelte::default()) else {
            panic!("l'importazione deve riuscire");
        };
        assert_eq!(rapporto.liked_marked, 0);
        assert_eq!(
            conta(&db, "SELECT liked_at FROM tracks WHERE title = 'Song 2'"),
            12_345
        );
    }

    #[test]
    fn uno_snapshot_vuoto_non_e_un_guasto() {
        // È il caso di chi apre il primo dei due archivi che Spotify manda: le
        // playlist stanno nell'altro, e non è successo niente di male.
        let mut db = database();
        libreria_di_prova(&db);
        let vuoto = AccountSnapshot::vuoto(Provenienza::Archivio);
        let Ok(rapporto) = import(&mut db, &vuoto, &Scelte::default()) else {
            panic!("l'importazione di uno snapshot vuoto deve riuscire");
        };
        assert_eq!(rapporto.matched(), 0);
        assert_eq!(rapporto.missing(), 0);
        assert_eq!(rapporto.history_rows, 0);
        assert!(rapporto.full_history, "l'archivio la cronologia ce l'ha");
    }

    #[test]
    fn la_provenienza_arriva_fino_al_rapporto() {
        let mut db = database();
        libreria_di_prova(&db);
        let dall_api = AccountSnapshot {
            preferiti: vec![sp("Blur", "Song 2", "Blur", 180_000)],
            ..AccountSnapshot::vuoto(Provenienza::Api)
        };
        let Ok(rapporto) = plan(&mut db, &dall_api, &Scelte::default()) else {
            panic!("il piano deve riuscire");
        };
        assert_eq!(rapporto.source, "api");
        assert_eq!(rapporto.liked.source, "api");
        assert!(
            !rapporto.full_history,
            "cinquanta righe non sono «la cronologia»"
        );
    }
}
