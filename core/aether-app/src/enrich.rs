//! L'arricchimento: dalla libreria alla rete e ritorno.
//!
//! `aether_domain::enrich` decide, `aether_meta` chiede, questo modulo esegue.
//!
//! # Tre fasi, e il confine fra loro è il lucchetto
//!
//! ```text
//! candidati()   ──►  legge un lotto di gruppi          (lucchetto preso)
//! decidi()      ──►  cerca, scarica, decide            (SENZA lucchetto)
//! scrivi_file() ──►  tag sui file, copertine su disco  (SENZA lucchetto)
//! registra()    ──►  righe, annullamento, aggregati    (lucchetto preso)
//! ```
//!
//! La divisione non è organizzativa: è la ragione per cui l'applicazione
//! continua a rispondere mentre l'arricchimento gira. Una passata su una
//! libreria vera dura minuti — MusicBrainz concede una richiesta al secondo — e
//! tenere il lucchetto per tutta la sua durata vorrebbe dire niente riproduzione,
//! niente ricerca, niente scansione per tutto quel tempo. È la stessa disciplina
//! che [`crate::desiderati`] impone alla coda di scaricamento, con la stessa
//! garanzia strutturale: `aether-meta` non riceve mai una `rusqlite::Connection`,
//! quindi il codice che terrebbe il lucchetto durante una richiesta non si può
//! nemmeno scrivere.
//!
//! # Che cosa si scrive, e cosa no
//!
//! Si scrive **solo** su verdetto [`Verdetto::Applica`]. Un candidato plausibile
//! e non provato non lascia traccia sul file: si annota la data del tentativo e
//! si passa oltre. È la scelta che rende accettabile una passata automatica su
//! cui nessuno guarda prima — la ragione per esteso sta in testa a
//! `aether_domain::enrich`.
//!
//! # Il punto che si sbaglia una volta sola
//!
//! Scrivere i tag cambia la data di modifica del file sul disco. Se non la si
//! rilegge e non la si aggiorna in `tracks.date_modified` **nella stessa
//! transazione**, la scansione successiva vede ogni file arricchito come
//! «cambiato», lo rilegge per intero e lo riscrive dai tag — riportando
//! `enrich_status` a uno stato che non riflette più la verità. E siccome anche
//! quella scansione non aggiornerebbe niente di diverso, il ciclo si
//! ripeterebbe a ogni passata, per sempre.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use aether_domain::album::{
    AlbumMember, UNKNOWN_ALBUM, UNKNOWN_ARTIST, album_group_key, pick_canonical_artist,
};
use aether_domain::enrich::{
    Candidate, Fields, LocalAlbum, LocalTrack, Verdetto, album_distance, decide_album, plan_write,
    resolve_track,
};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::keys::{TrackKey, TrackKeyInput};
use aether_domain::paths::file_stem;
use aether_meta::Fornitori;
use aether_meta::copertine::{Copertina, Sorgenti};
use aether_meta::deposito::{Deposito, Voce};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::covers::{CoverSource, CoverStore, StoredCover};
use crate::library::{db_error, now_ms, rebuild_aggregates};
use crate::tag_scrittura;

/// Quanti gruppi d'album si prendono per passata.
///
/// Dodici. Il numero non nasce dalla memoria — un gruppo pesa nulla — ma dal
/// **tempo**: ogni gruppo costa da due a quattro richieste a MusicBrainz, cioè
/// tre o quattro secondi, e i brani che restano fuori dall'abbinamento d'album
/// ne costano tre ciascuno. Dodici gruppi sono una passata da qualche minuto,
/// che è la durata giusta per qualcosa che gira ogni mezz'ora e deve potersi
/// fermare senza lasciare niente a metà.
pub const LOTTO: usize = 12;

/// Ogni quanto si ritenta un brano rimasto incerto o andato in errore.
pub const RITENTA_INCERTI_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Ogni quanto si ritenta un brano che nessun catalogo ha riconosciuto.
///
/// Un mese, cioè molto più a lungo dell'incerto. L'asimmetria è voluta: un
/// «non c'è» è una proprietà del catalogo, che cambia lentamente, mentre un
/// «non ne sono sicuro» può risolversi al primo tag che l'utente corregge a
/// mano.
pub const RITENTA_NESSUNO_MS: i64 = 30 * 24 * 60 * 60 * 1000;

// ── quel che si legge dal database ──────────────────────────────────────────

/// Un brano da arricchire, con quel che serve a riscriverlo.
#[derive(Debug, Clone)]
pub struct BranoDaArricchire {
    /// Il brano nella forma che il dominio confronta.
    pub brano: LocalTrack,
    /// Dove sta il file.
    pub path: String,
    /// Ha già una corrispondenza applicata in passato.
    pub gia_arricchito: bool,
    /// I tag di adesso, da fotografare prima di toccarli.
    pub originali: TagOriginali,
}

/// Un gruppo d'album da arricchire.
///
/// Contiene **tutti** i brani del gruppo, non solo quelli che ne hanno bisogno:
/// l'abbinamento confronta il numero di tracce con quello della pubblicazione,
/// e un gruppo dimezzato non corrisponderebbe a niente.
#[derive(Debug, Clone)]
pub struct Gruppo {
    /// La chiave di raggruppamento.
    pub album_key: String,
    /// Il titolo dell'album, come `albums` lo ha già calcolato.
    pub titolo: String,
    /// L'interprete canonico, come `albums` lo ha già calcolato.
    pub artista: String,
    /// I brani, ordinati per disco e traccia.
    pub brani: Vec<BranoDaArricchire>,
}

/// I tag di un brano prima che l'arricchimento li tocchi.
///
/// # Perché non è `Fields`
///
/// `Fields` vive nel dominio, e il dominio non serializza niente da sé — è una
/// regola dichiarata nel suo `Cargo.toml`, perché una `derive` di `serde` là
/// costringerebbe le decisioni pure ad avere un'opinione su come vengono
/// trasmesse. Questa struttura ha la stessa forma e sa diventare JSON, perché è
/// questo crate a parlare col mondo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagOriginali {
    /// Il titolo.
    pub title: Option<String>,
    /// L'interprete.
    pub artist: Option<String>,
    /// L'album.
    pub album: Option<String>,
    /// L'interprete dell'album.
    pub album_artist: Option<String>,
    /// L'anno.
    pub year: Option<i32>,
    /// Il genere.
    pub genre: Option<String>,
    /// Il numero di traccia.
    pub track_number: Option<u32>,
    /// Il numero di disco.
    pub disc_number: Option<u32>,
}

impl TagOriginali {
    /// La stessa fotografia nella forma che [`tag_scrittura`] sa scrivere.
    #[must_use]
    pub fn in_campi(&self) -> Fields {
        Fields {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            album_artist: self.album_artist.clone(),
            year: self.year,
            genre: self.genre.clone(),
            track_number: self.track_number,
            disc_number: self.disc_number,
            ..Fields::default()
        }
    }
}

// ── quel che si decide ──────────────────────────────────────────────────────

/// Una scrittura decisa e non ancora avvenuta.
#[derive(Debug, Clone)]
pub struct Scrittura {
    /// Quale riga.
    pub track_id: i64,
    /// Quale file.
    pub path: String,
    /// I campi da scrivere, già passati da `plan_write`.
    pub campi: Fields,
    /// La copertina da salvare e incorporare, se se n'è trovata una.
    pub copertina: Option<Copertina>,
    /// Chi ha deciso: `mb-release`, `mb-recording`, `itunes`, `deezer`.
    pub fonte: &'static str,
    /// Quanto ci si credeva, da 0 a 1.
    pub confidenza: f64,
    /// I tag di prima, per poter tornare indietro.
    pub originali: TagOriginali,
}

impl Scrittura {
    /// Non c'è niente da fare su questo brano.
    #[must_use]
    pub fn e_vuota(&self) -> bool {
        self.campi.e_vuoto() && self.copertina.is_none()
    }
}

/// Cosa si è deciso per un gruppo, prima che si scriva qualunque cosa.
#[derive(Debug, Default)]
pub struct Decisione {
    /// La chiave del gruppo.
    pub album_key: String,
    /// Le scritture decise.
    pub scritture: Vec<Scrittura>,
    /// I brani su cui ci si è astenuti, col verdetto da ricordare.
    pub astensioni: Vec<(i64, Verdetto)>,
    /// Nessuna fonte ha risposto.
    ///
    /// Non si scrive niente **e non si ricorda niente**: marchiare come
    /// introvabili i brani incontrati mentre il portatile era staccato dal wifi
    /// vorrebbe dire non riprovarli per un mese.
    pub non_raggiungibile: bool,
}

// ── fase uno: leggere i candidati ───────────────────────────────────────────

/// La condizione che rende un brano candidato all'arricchimento.
///
/// Una costante e non una stringa ripetuta: la stessa clausola serve a scegliere
/// cosa fare e a contare cosa resta, e due copie divergerebbero sul giorno in
/// cui qualcuno ne aggiusta una sola — producendo un contatore che dice «zero da
/// fare» mentre la passata continua a trovare lavoro.
///
/// I due segnaposto sono le scadenze di ritentativo: `?1` per gli incerti, `?2`
/// per quelli che nessuno ha riconosciuto.
fn candidato_where() -> String {
    format!(
        "t.mb_recording_id IS NULL
         AND (
           t.artist = '{UNKNOWN_ARTIST}' OR t.album = '{UNKNOWN_ALBUM}'
           OR t.cover_art_hash IS NULL
           OR t.source = 'youtube'
         )
         AND (
           t.enrich_status IS NULL
           OR (t.enrich_status IN ('needs-review', 'error')
               AND COALESCE(t.enrich_attempted_at, 0) < ?1)
           OR (t.enrich_status = 'no-match'
               AND COALESCE(t.enrich_attempted_at, 0) < ?2)
         )"
    )
}

/// Quanti brani aspettano ancora di essere arricchiti.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn quanti_mancano(connection: &Connection, adesso: i64) -> Result<i64, AppError> {
    let sql = format!("SELECT COUNT(*) FROM tracks t WHERE {}", candidato_where());
    connection
        .query_row(
            &sql,
            rusqlite::params![
                adesso.saturating_sub(RITENTA_INCERTI_MS),
                adesso.saturating_sub(RITENTA_NESSUNO_MS),
            ],
            |row| row.get(0),
        )
        .map_err(|err| db_error("conteggio dei brani da arricchire", &err))
}

/// I prossimi gruppi d'album da arricchire.
///
/// I più recenti per primi: chi ha appena scaricato o importato qualcosa vuole
/// vedere quello sistemarsi, non un disco che ha in libreria da due anni.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn candidati(
    connection: &Connection,
    adesso: i64,
    quanti: usize,
) -> Result<Vec<Gruppo>, AppError> {
    let sql = format!(
        "SELECT t.album_key, MAX(t.date_added) AS recente
         FROM tracks t
         WHERE {} AND t.album_key IS NOT NULL AND t.album_key <> ''
         GROUP BY t.album_key
         ORDER BY recente DESC
         LIMIT ?3",
        candidato_where()
    );
    let chiavi: Vec<String> = {
        let mut statement = connection
            .prepare(&sql)
            .map_err(|err| db_error("elenco dei gruppi da arricchire", &err))?;
        let righe = statement
            .query_map(
                rusqlite::params![
                    adesso.saturating_sub(RITENTA_INCERTI_MS),
                    adesso.saturating_sub(RITENTA_NESSUNO_MS),
                    i64::try_from(quanti).unwrap_or(i64::MAX),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|err| db_error("elenco dei gruppi da arricchire", &err))?;
        righe
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("elenco dei gruppi da arricchire", &err))?
    };

    let mut gruppi = Vec::with_capacity(chiavi.len());
    for album_key in chiavi {
        if let Some(gruppo) = leggi_gruppo(connection, &album_key)? {
            gruppi.push(gruppo);
        }
    }
    Ok(gruppi)
}

/// Legge un gruppo per intero, brani compresi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn leggi_gruppo(connection: &Connection, album_key: &str) -> Result<Option<Gruppo>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, path, title, artist, album, album_artist, duration_ms,
                    track_number, disc_number, year, genre, cover_art_hash,
                    mb_recording_id, enrich_status
             FROM tracks
             WHERE album_key = ?1
             ORDER BY COALESCE(disc_number, 1), COALESCE(track_number, 0), id",
        )
        .map_err(|err| db_error("lettura di un gruppo da arricchire", &err))?;
    let righe = statement
        .query_map([album_key], |row| {
            let path: String = row.get(1)?;
            let title: String = row.get(2)?;
            let artist: String = row.get(3)?;
            let album: Option<String> = row.get(4)?;
            let album_artist: Option<String> = row.get(5)?;
            let duration_ms: i64 = row.get(6)?;
            let track_number: Option<i64> = row.get(7)?;
            let disc_number: Option<i64> = row.get(8)?;
            let year: Option<i64> = row.get(9)?;
            let genre: Option<String> = row.get(10)?;
            let cover: Option<String> = row.get(11)?;
            let mb: Option<String> = row.get(12)?;
            let stato: Option<String> = row.get(13)?;
            Ok(BranoDaArricchire {
                brano: LocalTrack {
                    id: row.get(0)?,
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    album_artist: album_artist.clone(),
                    duration_ms: u64::try_from(duration_ms).ok().filter(|d| *d > 0),
                    track_number: track_number.and_then(|n| u32::try_from(n).ok()),
                    disc_number: disc_number.and_then(|n| u32::try_from(n).ok()),
                    year: year.and_then(|a| i32::try_from(a).ok()),
                    genre: genre.clone(),
                    file_stem: Some(file_stem(&path).to_owned()),
                    has_cover: cover.is_some(),
                    has_mb_id: mb.is_some(),
                },
                path,
                gia_arricchito: stato.as_deref() == Some("ok"),
                originali: TagOriginali {
                    title: Some(title),
                    artist: Some(artist),
                    album,
                    album_artist,
                    year: year.and_then(|a| i32::try_from(a).ok()),
                    genre,
                    track_number: track_number.and_then(|n| u32::try_from(n).ok()),
                    disc_number: disc_number.and_then(|n| u32::try_from(n).ok()),
                },
            })
        })
        .map_err(|err| db_error("lettura di un gruppo da arricchire", &err))?;
    let brani: Vec<BranoDaArricchire> = righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura di un gruppo da arricchire", &err))?;
    if brani.is_empty() {
        return Ok(None);
    }

    // Titolo e interprete si prendono da `albums`, che `rebuild_aggregates` ha
    // già calcolato — il titolo più frequente e l'interprete canonico, quello
    // che perde il `feat.`. Ricalcolarli qui vorrebbe dire una seconda idea di
    // come si chiama un album, e la seconda idea diverge.
    let da_albums: Option<(String, String)> = connection
        .query_row(
            "SELECT title, artist FROM albums WHERE album_key = ?1",
            [album_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let (titolo, artista) = da_albums.unwrap_or_else(|| {
        // Nessuna riga in `albums`: succede fra una scansione e la ricostruzione
        // degli aggregati. Si ripiega sulle stesse funzioni del dominio che
        // `rebuild_aggregates` userebbe, invece di inventare un terzo criterio.
        let membri: Vec<AlbumMember> = brani
            .iter()
            .map(|b| AlbumMember {
                album_key: album_key.to_owned(),
                album: b.brano.album.clone().unwrap_or_default(),
                album_artist: b.brano.album_artist.clone(),
                artist: Some(b.brano.artist.clone()),
                ..AlbumMember::default()
            })
            .collect();
        let titolo = brani
            .first()
            .and_then(|b| b.brano.album.clone())
            .unwrap_or_else(|| UNKNOWN_ALBUM.to_owned());
        (titolo, pick_canonical_artist(&membri))
    });

    Ok(Some(Gruppo {
        album_key: album_key.to_owned(),
        titolo,
        artista,
        brani,
    }))
}

// ── fase due: decidere ──────────────────────────────────────────────────────

/// Il titolo dell'album è un segnaposto e non un titolo.
fn album_senza_nome(titolo: &str) -> bool {
    let rifilato = titolo.trim();
    rifilato.is_empty() || rifilato == UNKNOWN_ALBUM
}

/// Decide cosa fare di un gruppo. **Parla con la rete, non col database.**
///
/// # L'ordine dei due percorsi
///
/// Prima si prova ad abbinare l'album intero: dodici prove che si confermano a
/// vicenda, due o tre richieste in tutto. Solo quel che resta fuori — perché il
/// gruppo non ha un titolo d'album, perché è un singolo, perché la
/// pubblicazione non si è trovata — passa dal percorso per brano, che costa tre
/// richieste a testa e porta una prova sola.
///
/// La differenza si vede su una libreria vera: un disco di dodici tracce costa
/// tre richieste invece di trentasei, e le sue corrispondenze sono più solide
/// perché nessuna delle dodici è stata decisa da sola.
#[must_use]
pub fn decidi(fornitori: &Fornitori, gruppo: &Gruppo) -> Decisione {
    let mut decisione = Decisione {
        album_key: gruppo.album_key.clone(),
        ..Decisione::default()
    };

    if !album_senza_nome(&gruppo.titolo) && gruppo.brani.len() > 1 {
        match per_album(fornitori, gruppo) {
            Ok(Some(scritture)) => {
                decisione.scritture = scritture;
                return decisione;
            }
            Ok(None) => {}
            Err(_) => {
                // MusicBrainz non risponde: senza di lui il percorso per brano
                // può al massimo raccogliere due pareri su tre, e nessuno dei
                // due porta gli identificativi. Si smette e si riprova dopo.
                decisione.non_raggiungibile = true;
                return decisione;
            }
        }
    }

    for brano in &gruppo.brani {
        if brano.gia_arricchito {
            continue;
        }
        match per_brano(fornitori, brano) {
            Esito::Applicato(scrittura) => decisione.scritture.push(*scrittura),
            Esito::Astenuto(verdetto) => {
                decisione.astensioni.push((brano.brano.id, verdetto));
            }
            Esito::NonRaggiungibile => {
                decisione.non_raggiungibile = true;
                return decisione;
            }
        }
    }
    decisione
}

/// Prova ad abbinare l'album intero.
///
/// `Ok(None)` significa «ho chiesto e non l'ho trovato»: si scende al percorso
/// per brano. `Err` significa «non ho potuto chiedere».
fn per_album(fornitori: &Fornitori, gruppo: &Gruppo) -> Result<Option<Vec<Scrittura>>, AppError> {
    let locali: Vec<LocalTrack> = gruppo.brani.iter().map(|b| b.brano.clone()).collect();
    let pubblicazioni = fornitori.pubblicazioni(&gruppo.titolo, &gruppo.artista, locali.len())?;

    let locale = LocalAlbum {
        title: &gruppo.titolo,
        artist: &gruppo.artista,
        tracks: &locali,
    };
    let mut migliore: Option<(usize, aether_domain::enrich::AlbumMatch)> = None;
    for (indice, remota) in pubblicazioni.iter().enumerate() {
        let abbinamento = album_distance(&locale, remota);
        let sostituisci = migliore
            .as_ref()
            .is_none_or(|(_, attuale)| abbinamento.distance < attuale.distance);
        if sostituisci {
            migliore = Some((indice, abbinamento));
        }
    }
    let Some((indice, abbinamento)) = migliore else {
        return Ok(None);
    };
    if decide_album(&abbinamento) != Verdetto::Applica {
        return Ok(None);
    }
    let Some(remota) = pubblicazioni.get(indice) else {
        return Ok(None);
    };

    // Una copertina per tutto il disco, non una per brano: è la stessa immagine
    // — lo store la salverebbe comunque una volta sola — ma sono undici
    // richieste in meno, e su un cancello da quattro al secondo si sentono.
    let mancano_copertine = gruppo.brani.iter().any(|b| !b.brano.has_cover);
    let copertina = mancano_copertine
        .then(|| {
            let pubblicazioni_id: Vec<String> = remota.mb_release_id.iter().cloned().collect();
            fornitori.copertina(&Sorgenti {
                url_diretto: None,
                mb_release_group_id: remota.mb_release_group_id.as_deref(),
                mb_release_ids: &pubblicazioni_id,
            })
        })
        .flatten();

    let confidenza = (1.0 - abbinamento.distance).clamp(0.0, 1.0);
    let mut scritture = Vec::new();
    for (posizione, brano) in gruppo.brani.iter().enumerate() {
        let Some(Some(indice_traccia)) = abbinamento.assignment.get(posizione) else {
            continue;
        };
        let Some(traccia) = remota.tracks.get(*indice_traccia) else {
            continue;
        };
        // La sostituzione dei campi già scritti è **per traccia**, non per
        // disco: dentro un album che corrisponde può esserci una traccia il cui
        // titolo concorda appena, e su quella si riempiono i buchi senza
        // riscrivere quel che c'è.
        let titolo_forte = aether_domain::enrich::similarity(&brano.brano.title, &traccia.title)
            >= aether_domain::enrich::TITOLO_FORTE;

        let trovati = Fields {
            title: Some(traccia.title.clone()),
            artist: traccia.artist.clone(),
            album: Some(remota.title.clone()),
            album_artist: Some(remota.artist.clone()).filter(|a| !a.is_empty()),
            year: remota.year,
            genre: None,
            track_number: traccia.track_number,
            disc_number: traccia.disc_number,
            mb_recording_id: traccia.mb_recording_id.clone(),
            mb_release_id: remota.mb_release_id.clone(),
            mb_release_group_id: remota.mb_release_group_id.clone(),
        };
        let campi = plan_write(&brano.brano, &trovati, titolo_forte);
        let sua_copertina = (!brano.brano.has_cover)
            .then(|| copertina.clone())
            .flatten();
        let scrittura = Scrittura {
            track_id: brano.brano.id,
            path: brano.path.clone(),
            campi,
            copertina: sua_copertina,
            fonte: "mb-release",
            confidenza,
            originali: brano.originali.clone(),
        };
        if !scrittura.e_vuota() {
            scritture.push(scrittura);
        }
    }
    Ok(Some(scritture))
}

/// Cosa è successo interrogando le fonti per un singolo brano.
enum Esito {
    /// C'è una corrispondenza provata.
    Applicato(Box<Scrittura>),
    /// C'è o non c'è, ma non si scrive.
    Astenuto(Verdetto),
    /// Nessuna fonte ha risposto.
    NonRaggiungibile,
}

/// Risolve un singolo brano contro tutte e tre le fonti.
fn per_brano(fornitori: &Fornitori, brano: &BranoDaArricchire) -> Esito {
    let esito = fornitori.candidati(&brano.brano.title, &brano.brano.artist);
    if esito.tutte_giu {
        return Esito::NonRaggiungibile;
    }
    let Some(abbinamento) = resolve_track(&brano.brano, &esito.candidati) else {
        return Esito::Astenuto(Verdetto::Nessuno);
    };
    if abbinamento.verdict != Verdetto::Applica {
        return Esito::Astenuto(abbinamento.verdict);
    }
    let Some(vincitore) = esito.candidati.get(abbinamento.best) else {
        return Esito::Astenuto(Verdetto::Nessuno);
    };

    let copertina = (!brano.brano.has_cover)
        .then(|| {
            let pubblicazioni: Vec<String> = vincitore.mb_release_id.iter().cloned().collect();
            fornitori.copertina(&Sorgenti {
                url_diretto: vincitore.cover_url.as_deref(),
                mb_release_group_id: vincitore.mb_release_group_id.as_deref(),
                mb_release_ids: &pubblicazioni,
            })
        })
        .flatten();

    let trovati = campi_da_candidato(vincitore);
    let campi = plan_write(&brano.brano, &trovati, true);
    let scrittura = Scrittura {
        track_id: brano.brano.id,
        path: brano.path.clone(),
        campi,
        copertina,
        fonte: fonte_di(vincitore),
        confidenza: abbinamento.score,
        originali: brano.originali.clone(),
    };
    // Anche quando non c'è niente da cambiare si restituisce un'applicazione, e
    // non un'astensione: vuol dire che il file era già a posto, e registrarlo
    // come «riuscito» è ciò che impedisce di richiederlo a ogni passata per
    // sempre. `scrivi_file` salterà la scrittura, perché i campi sono vuoti.
    Esito::Applicato(Box::new(scrittura))
}

/// I campi che un candidato per brano propone.
fn campi_da_candidato(candidato: &Candidate) -> Fields {
    Fields {
        title: Some(candidato.title.clone()),
        artist: Some(candidato.artist.clone()).filter(|a| !a.is_empty()),
        album: candidato.album.clone(),
        // L'interprete dell'album è quello del brano: è la stessa scelta di
        // `scrivi_tag`, con la stessa ragione — è la chiave con cui un album si
        // raggruppa in una sola uscita, e lasciarla vuota lo spezza in libreria.
        album_artist: Some(candidato.artist.clone()).filter(|a| !a.is_empty()),
        year: candidato.year,
        genre: candidato.genre.clone(),
        track_number: None,
        disc_number: None,
        mb_recording_id: candidato.mb_recording_id.clone(),
        mb_release_id: candidato.mb_release_id.clone(),
        mb_release_group_id: candidato.mb_release_group_id.clone(),
    }
}

/// Il nome della fonte, per `tracks.enrich_source`.
fn fonte_di(candidato: &Candidate) -> &'static str {
    match candidato.fonte {
        Some(aether_domain::enrich::FonteMeta::MusicBrainz) => "mb-recording",
        Some(aether_domain::enrich::FonteMeta::Itunes) => "itunes",
        Some(aether_domain::enrich::FonteMeta::Deezer) => "deezer",
        None => "ignota",
    }
}

// ── fase tre: scrivere sui file ─────────────────────────────────────────────

/// Cosa è successo scrivendo su un file.
#[derive(Debug)]
pub struct EsitoFile {
    /// Quale riga.
    pub track_id: i64,
    /// La data di modifica **dopo** la scrittura. Vedi la nota in testa.
    pub date_modified: i64,
    /// La dimensione dopo la scrittura: incorporare una copertina la cambia.
    pub file_size: i64,
    /// La copertina salvata nello store.
    pub copertina: Option<StoredCover>,
    /// La provenienza della copertina, per `cover_art.source`.
    pub copertina_da: Option<&'static str>,
    /// I campi che la rilettura non ha confermato.
    pub discordi: Vec<&'static str>,
    /// Il file è stato davvero riscritto.
    ///
    /// Falso quando la corrispondenza c'era ma non cambiava niente: il brano era
    /// già a posto. Serve a due cose che sarebbero difetti se mancassero — non
    /// aprire e riscrivere un file per lasciarlo identico (cosa che ne
    /// cambierebbe la data di modifica e farebbe rileggere tutto alla scansione
    /// successiva), e non registrare una riga di annullamento per una scrittura
    /// che non è avvenuta.
    pub toccato: bool,
}

/// Scrive i tag sui file e salva le copertine. **Non tocca il database.**
///
/// Restituisce un esito per ogni scrittura **riuscita**. Una fallita non compare:
/// scriverne i campi nel database mentre il file è rimasto com'era produrrebbe
/// una libreria che mostra un titolo che il file non ha — e la scansione
/// successiva lo rimetterebbe com'era, cancellando l'arricchimento senza dire
/// niente a nessuno.
#[must_use]
pub fn scrivi_file(
    covers: &CoverStore,
    scritture: &[Scrittura],
    scrivi_tag: bool,
) -> (Vec<EsitoFile>, Vec<AppError>) {
    let mut esiti = Vec::with_capacity(scritture.len());
    let mut guasti = Vec::new();

    for scrittura in scritture {
        let percorso = Path::new(&scrittura.path);
        let byte_copertina = scrittura.copertina.as_ref().map(|c| c.byte.as_slice());
        // Una corrispondenza che non cambia niente non apre il file. Aprirlo e
        // riscriverlo identico ne cambierebbe la data di modifica, e la
        // scansione successiva rileggerebbe per intero ogni brano che era già
        // a posto — a ogni passata, per sempre.
        let toccato = scrivi_tag && !scrittura.e_vuota();

        if toccato
            && let Err(err) =
                tag_scrittura::scrivi_campi(percorso, &scrittura.campi, byte_copertina)
        {
            guasti.push(err);
            continue;
        }

        // La copertina si salva **dopo** la scrittura dei tag: se quella
        // fallisce, non si è ricodificata un'immagine per niente.
        let (copertina, copertina_da) = match scrittura.copertina.as_ref() {
            Some(trovata) => {
                let fonte = match trovata.provenienza {
                    aether_meta::copertine::Provenienza::Fornitore => CoverSource::Provider,
                    aether_meta::copertine::Provenienza::CoverArtArchive => {
                        CoverSource::CoverArtArchive
                    }
                };
                match covers.store(&trovata.byte, fonte) {
                    Ok(salvata) => (Some(salvata), Some(fonte.as_str())),
                    Err(err) => {
                        // Un'immagine illeggibile non butta via il resto: si
                        // perde la copertina, si tengono titolo e anno.
                        guasti.push(err);
                        (None, None)
                    }
                }
            }
            None => (None, None),
        };

        let (date_modified, file_size) = misura(percorso);
        let discordi = if toccato {
            tag_scrittura::rileggi_e_confronta(percorso, &scrittura.campi)
        } else {
            Vec::new()
        };

        esiti.push(EsitoFile {
            track_id: scrittura.track_id,
            date_modified,
            file_size,
            copertina,
            copertina_da,
            discordi,
            toccato,
        });
    }

    (esiti, guasti)
}

/// La data di modifica e la dimensione di un file, nella forma del database.
///
/// I millisecondi **già troncati all'intero**, come `plan_scan` li confronta: un
/// valore con i decimali farebbe vedere «cambiato» lo stesso file a ogni
/// passata. Un file che non si misura vale zero, che è quel che rende la
/// scansione successiva un aggiornamento invece di un ciclo infinito.
fn misura(percorso: &Path) -> (i64, i64) {
    let Ok(dati) = std::fs::metadata(percorso) else {
        return (0, 0);
    };
    let modificato = dati
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0);
    (modificato, i64::try_from(dati.len()).unwrap_or(i64::MAX))
}

// ── fase quattro: registrare ────────────────────────────────────────────────

/// Quanto ha prodotto una passata su un gruppo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Registrati {
    /// Brani con i campi riscritti.
    pub applicati: usize,
    /// Brani su cui ci si è astenuti.
    pub astenuti: usize,
    /// Copertine nuove salvate.
    pub copertine: usize,
}

/// Scrive nel database quel che le fasi precedenti hanno deciso e fatto.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn registra(
    tx: &Transaction<'_>,
    decisione: &Decisione,
    scritture: &[Scrittura],
    esiti: &[EsitoFile],
    adesso: i64,
) -> Result<Registrati, AppError> {
    let mut conto = Registrati::default();

    for esito in esiti {
        let Some(scrittura) = scritture.iter().find(|s| s.track_id == esito.track_id) else {
            continue;
        };
        // Solo per i file davvero riscritti: una riga di annullamento su un
        // brano che nessuno ha toccato farebbe riscrivere, il giorno
        // dell'annullamento, un file che l'arricchimento aveva lasciato stare.
        if esito.toccato {
            registra_annullamento(tx, scrittura, adesso)?;
        }
        if let Some(copertina) = esito.copertina.as_ref() {
            registra_copertina(tx, copertina, esito.copertina_da, adesso)?;
            if !copertina.already_present {
                conto.copertine = conto.copertine.saturating_add(1);
            }
        }
        aggiorna_brano(tx, scrittura, esito, adesso)?;
        conto.applicati = conto.applicati.saturating_add(1);
    }

    for (track_id, verdetto) in &decisione.astensioni {
        segna_astensione(tx, *track_id, *verdetto, adesso)?;
        conto.astenuti = conto.astenuti.saturating_add(1);
    }

    Ok(conto)
}

/// Fotografa i tag di prima, una volta sola.
///
/// `INSERT OR IGNORE`: la fotografia è quella di **prima che Aether ci mettesse
/// le mani**, non quella del passo precedente. Sovrascriverla farebbe sì che
/// «annulla» riporti il file a una versione che l'utente non ha mai visto —
/// quella scritta da noi la volta prima.
fn registra_annullamento(
    tx: &Transaction<'_>,
    scrittura: &Scrittura,
    adesso: i64,
) -> Result<(), AppError> {
    let tags = serde_json::to_string(&scrittura.originali).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione dei tag originali".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    tx.prepare_cached(
        "INSERT OR IGNORE INTO enrich_undo (track_id, path, tags, written_at)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![
            scrittura.track_id,
            scrittura.path,
            tags,
            adesso
        ])
    })
    .map(|_| ())
    .map_err(|err| db_error("fotografia dei tag originali", &err))
}

/// Registra una copertina nello schema, se non c'era già.
fn registra_copertina(
    tx: &Transaction<'_>,
    copertina: &StoredCover,
    provenienza: Option<&str>,
    adesso: i64,
) -> Result<(), AppError> {
    tx.prepare_cached(
        "INSERT OR IGNORE INTO cover_art
           (hash, mime_type, width, height, byte_size, source, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![
            copertina.hash,
            copertina.mime_type,
            copertina.width,
            copertina.height,
            i64::try_from(copertina.byte_size).unwrap_or(0),
            provenienza.unwrap_or("provider"),
            adesso,
        ])
    })
    .map(|_| ())
    .map_err(|err| db_error("registrazione di una copertina arricchita", &err))
}

/// Riscrive la riga con quel che si è appena scritto sul file.
fn aggiorna_brano(
    tx: &Transaction<'_>,
    scrittura: &Scrittura,
    esito: &EsitoFile,
    adesso: i64,
) -> Result<(), AppError> {
    // Le chiavi si ricalcolano dai valori **finali**, non da quelli trovati:
    // `plan_write` può aver deciso di non scrivere un campo, e una chiave
    // derivata da un titolo che sul file non c'è non ritroverebbe mai il brano.
    let titolo = scrittura
        .campi
        .title
        .clone()
        .or_else(|| scrittura.originali.title.clone())
        .unwrap_or_default();
    let artista = scrittura
        .campi
        .artist
        .clone()
        .or_else(|| scrittura.originali.artist.clone())
        .unwrap_or_default();
    let album = scrittura
        .campi
        .album
        .clone()
        .or_else(|| scrittura.originali.album.clone())
        .unwrap_or_else(|| UNKNOWN_ALBUM.to_owned());

    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artista),
        title: Some(&titolo),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, &scrittura.path);

    tx.prepare_cached(
        "UPDATE tracks SET
           title = COALESCE(?2, title),
           artist = COALESCE(?3, artist),
           album = COALESCE(?4, album),
           album_artist = COALESCE(?5, album_artist),
           year = COALESCE(?6, year),
           genre = COALESCE(?7, genre),
           track_number = COALESCE(?8, track_number),
           disc_number = COALESCE(?9, disc_number),
           mb_recording_id = COALESCE(?10, mb_recording_id),
           mb_release_id = COALESCE(?11, mb_release_id),
           mb_release_group_id = COALESCE(?12, mb_release_group_id),
           cover_art_hash = COALESCE(?13, cover_art_hash),
           track_key = ?14,
           album_key = ?15,
           date_modified = ?16,
           file_size = ?17,
           enrich_status = 'ok',
           enrich_attempted_at = ?18,
           enrich_source = ?19,
           enrich_confidence = ?20
         WHERE id = ?1",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![
            scrittura.track_id,
            scrittura.campi.title,
            scrittura.campi.artist,
            scrittura.campi.album,
            scrittura.campi.album_artist,
            scrittura.campi.year,
            scrittura.campi.genre,
            scrittura.campi.track_number,
            scrittura.campi.disc_number,
            scrittura.campi.mb_recording_id,
            scrittura.campi.mb_release_id,
            scrittura.campi.mb_release_group_id,
            esito.copertina.as_ref().map(|c| c.hash.as_str()),
            track_key,
            album_key,
            esito.date_modified,
            esito.file_size,
            adesso,
            scrittura.fonte,
            scrittura.confidenza,
        ])
    })
    .map(|_| ())
    .map_err(|err| db_error("aggiornamento di un brano arricchito", &err))
}

/// Ricorda che si è provato e non si è applicato niente.
///
/// `AND mb_recording_id IS NULL`: un brano già identificato non si declassa. Se
/// porta un identificativo MusicBrainz — perché lo aveva nei tag, o perché una
/// passata precedente lo ha applicato — una ricerca che oggi non lo ritrova non
/// è una ragione per marchiarlo come sconosciuto.
fn segna_astensione(
    tx: &Transaction<'_>,
    track_id: i64,
    verdetto: Verdetto,
    adesso: i64,
) -> Result<(), AppError> {
    tx.prepare_cached(
        "UPDATE tracks SET enrich_status = ?2, enrich_attempted_at = ?3
         WHERE id = ?1 AND mb_recording_id IS NULL",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![track_id, verdetto.as_str(), adesso])
    })
    .map(|_| ())
    .map_err(|err| db_error("annotazione di un'astensione", &err))
}

/// Segna un brano come fallito, senza toccarne i campi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn segna_errore(tx: &Transaction<'_>, track_id: i64, adesso: i64) -> Result<(), AppError> {
    tx.prepare_cached(
        "UPDATE tracks SET enrich_status = 'error', enrich_attempted_at = ?2 WHERE id = ?1",
    )
    .and_then(|mut statement| statement.execute(rusqlite::params![track_id, adesso]))
    .map(|_| ())
    .map_err(|err| db_error("annotazione di un guasto", &err))
}

/// Ricostruisce gli aggregati dopo una passata.
///
/// Va chiamata, e non è un dettaglio di igiene: l'arricchimento scrive
/// `mb_release_group_id` su brani che prima non l'avevano, e sono esattamente
/// gli identificativi con cui [`aether_domain::album::build_album_groups`]
/// **fonde** i gruppi. Senza la ricostruzione, un disco che l'arricchimento ha
/// appena dimostrato essere uno solo resterebbe spezzato nelle due schede in cui
/// era prima.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn ricostruisci(tx: &Transaction<'_>) -> Result<(), AppError> {
    rebuild_aggregates(tx).map(|_| ())
}

// ── l'annullamento ──────────────────────────────────────────────────────────

/// Quanti brani un annullamento ha riportato indietro.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Annullati {
    /// Brani riportati ai tag di prima.
    pub riportati: usize,
    /// Righe di annullamento che non si sono potute applicare.
    pub falliti: usize,
}

/// Riporta indietro tutto quel che l'arricchimento ha scritto.
///
/// # Perché esiste, e perché non è una comodità
///
/// La passata è automatica e scrive nei file dell'utente senza che nessuno
/// guardi prima. Una funzione che disfa non è un extra: è la metà che rende
/// accettabile l'altra. Senza, l'unica risposta a «mi ha rovinato i tag»
/// sarebbe «ripristina un backup».
///
/// Si legge il percorso di **adesso** da `tracks` — fra la scrittura e questo
/// momento può esserci passato un riordino — e si ripiega su quello registrato
/// quando la riga non c'è più.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un singolo file che non si
/// riscrive **non** fa fallire l'annullamento: si conta e si prosegue, perché
/// arrendersi al primo file bloccato lascerebbe l'utente con metà libreria
/// riportata indietro e nessun modo di finire il lavoro.
pub fn annulla(connection: &mut Connection, scrivi_tag: bool) -> Result<Annullati, AppError> {
    let righe: Vec<(i64, String, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT u.track_id, COALESCE(t.path, u.path), u.tags
                 FROM enrich_undo u
                 LEFT JOIN tracks t ON t.id = u.track_id
                 ORDER BY u.written_at DESC",
            )
            .map_err(|err| db_error("elenco degli annullamenti", &err))?;
        let righe = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|err| db_error("elenco degli annullamenti", &err))?;
        righe
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("elenco degli annullamenti", &err))?
    };

    let mut conto = Annullati::default();
    let mut riusciti: Vec<(i64, TagOriginali, i64, i64)> = Vec::new();

    for (track_id, percorso, tags) in &righe {
        let Ok(originali) = serde_json::from_str::<TagOriginali>(tags) else {
            conto.falliti = conto.falliti.saturating_add(1);
            continue;
        };
        let path = Path::new(percorso);
        if scrivi_tag && tag_scrittura::ripristina_campi(path, &originali.in_campi()).is_err() {
            conto.falliti = conto.falliti.saturating_add(1);
            continue;
        }
        let (modificato, dimensione) = misura(path);
        riusciti.push((*track_id, originali, modificato, dimensione));
    }

    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione di annullamento", &err))?;
    for (track_id, originali, modificato, dimensione) in &riusciti {
        ripristina_riga(&tx, *track_id, originali, *modificato, *dimensione)?;
        conto.riportati = conto.riportati.saturating_add(1);
    }
    ricostruisci(&tx)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione di annullamento", &err))?;

    Ok(conto)
}

/// Riporta una riga ai valori di prima, e cancella la sua fotografia.
fn ripristina_riga(
    tx: &Transaction<'_>,
    track_id: i64,
    originali: &TagOriginali,
    date_modified: i64,
    file_size: i64,
) -> Result<(), AppError> {
    let percorso: Option<String> = tx
        .query_row("SELECT path FROM tracks WHERE id = ?1", [track_id], |row| {
            row.get(0)
        })
        .ok();
    let titolo = originali.title.clone().unwrap_or_default();
    let artista = originali.artist.clone().unwrap_or_default();
    let album = originali
        .album
        .clone()
        .unwrap_or_else(|| UNKNOWN_ALBUM.to_owned());
    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artista),
        title: Some(&titolo),
        album: Some(&album),
    })
    .into_string();
    let album_key = percorso
        .as_deref()
        .map(|p| album_group_key(&album, p))
        .unwrap_or_default();

    tx.prepare_cached(
        "UPDATE tracks SET
           title = ?2, artist = ?3, album = ?4, album_artist = ?5,
           year = ?6, genre = ?7, track_number = ?8, disc_number = ?9,
           mb_recording_id = NULL, mb_release_id = NULL, mb_release_group_id = NULL,
           track_key = ?10,
           album_key = CASE WHEN ?11 = '' THEN album_key ELSE ?11 END,
           date_modified = ?12, file_size = ?13,
           enrich_status = NULL, enrich_attempted_at = NULL,
           enrich_source = NULL, enrich_confidence = NULL
         WHERE id = ?1",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![
            track_id,
            titolo,
            artista,
            album,
            originali.album_artist,
            originali.year,
            originali.genre,
            originali.track_number,
            originali.disc_number,
            track_key,
            album_key,
            date_modified,
            file_size,
        ])
    })
    .map_err(|err| db_error("ripristino di un brano", &err))?;

    tx.prepare_cached("DELETE FROM enrich_undo WHERE track_id = ?1")
        .and_then(|mut statement| statement.execute([track_id]))
        .map(|_| ())
        .map_err(|err| db_error("cancellazione di un annullamento", &err))
}

// ── il deposito su SQLite ───────────────────────────────────────────────────

/// La memoria delle risposte dei servizi, in `enrich_cache`.
///
/// # Perché una connessione tutta sua
///
/// Perché è l'unica parte dell'arricchimento che scrive **mentre** si parla con
/// la rete, cioè proprio nella fase in cui il lucchetto della libreria non si ha.
/// Una seconda connessione allo stesso file è sicura — il database sta in WAL,
/// e `busy_timeout` copre la contesa fra due scrittori — e in cambio la cache
/// non partecipa a nessuna transazione della libreria. È giusto che sia così:
/// perdere una riga di cache è perdere una richiesta HTTP, non un dato.
pub struct DepositoSqlite {
    connessione: Mutex<Connection>,
}

impl std::fmt::Debug for DepositoSqlite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepositoSqlite").finish_non_exhaustive()
    }
}

impl DepositoSqlite {
    /// Apre il deposito sullo stesso file della libreria.
    ///
    /// Butta via quel che è scaduto, una volta per apertura: è il momento in cui
    /// costa meno — nessuno sta aspettando — e senza, la tabella crescerebbe di
    /// una riga per richiesta e non calerebbe mai.
    ///
    /// # Errori
    ///
    /// `db.openFailed` se il file non si apre.
    pub fn apri(percorso: &Path, adesso: i64) -> Result<Self, AppError> {
        let connessione = Connection::open(percorso).map_err(|err| {
            AppError::new(ErrorCode::DbOpenFailed {
                path: Some(percorso.display().to_string()),
            })
            .with_cause(err.to_string())
        })?;
        // Solo l'attesa: `journal_mode` è salvato nel file e le migrazioni le ha
        // già applicate la connessione principale. Questa non deve toccare lo
        // schema — se lo facesse, due connessioni migrerebbero lo stesso
        // database insieme.
        let _ = connessione.busy_timeout(std::time::Duration::from_secs(5));
        let _ = connessione.execute("DELETE FROM enrich_cache WHERE expires_at < ?1", [adesso]);
        Ok(Self {
            connessione: Mutex::new(connessione),
        })
    }
}

impl Deposito for DepositoSqlite {
    fn leggi(&self, servizio: &str, chiave: &str) -> Option<Voce> {
        let connessione = self
            .connessione
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let letta: Option<Option<Vec<u8>>> = connessione
            .query_row(
                "SELECT body FROM enrich_cache
                 WHERE service = ?1 AND key = ?2 AND expires_at > ?3",
                rusqlite::params![servizio, chiave, now_ms()],
                |row| row.get(0),
            )
            .ok();
        // Il `NULL` nella colonna **è** l'informazione: significa «il servizio
        // ha risposto che non ce l'ha». Collassarlo su `None` farebbe
        // richiedere per sempre le stesse cose che non esistono.
        letta.map(|corpo| corpo.map_or(Voce::Niente, Voce::Corpo))
    }

    fn scrivi(&self, servizio: &str, chiave: &str, voce: &Voce, vive_ms: i64) {
        let connessione = self
            .connessione
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let corpo: Option<&[u8]> = match voce {
            Voce::Corpo(byte) => Some(byte),
            Voce::Niente => None,
        };
        // Un guasto qui si inghiotte: perdere una riga di cache costa una
        // richiesta HTTP alla passata dopo, mentre propagarlo farebbe fallire
        // una ricerca che è già andata bene.
        let _ = connessione.execute(
            "INSERT INTO enrich_cache (service, key, body, expires_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(service, key) DO UPDATE SET
               body = excluded.body, expires_at = excluded.expires_at",
            rusqlite::params![servizio, chiave, corpo, now_ms().saturating_add(vive_ms)],
        );
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un mp3 minimo ma vero, che lofty sa aprire e riscrivere.
    ///
    /// Tre fotogrammi MPEG-1 Layer III e non uno, per la ragione già scritta in
    /// `tag_scrittura`: chi legge un mp3 conferma il primo fotogramma trovando
    /// il sincronismo di quello dopo, e con un fotogramma solo il file viene
    /// rifiutato come «invalid frame».
    fn scrivi_mp3(percorso: &Path) {
        const FOTOGRAMMA: usize = 417;
        let mut dati = Vec::with_capacity(FOTOGRAMMA * 3);
        for _ in 0..3 {
            dati.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            dati.resize(dati.len() + FOTOGRAMMA - 4, 0);
        }
        std::fs::write(percorso, &dati).expect("scrittura del file di prova");
    }

    /// Una libreria con un brano vero su disco.
    fn libreria() -> (tempfile::TempDir, Connection, std::path::PathBuf) {
        let cartella = tempfile::tempdir().expect("cartella temporanea");
        let percorso = cartella.path().join("brano.mp3");
        scrivi_mp3(&percorso);
        // Un titolo, così la rilettura ha qualcosa da confermare.
        tag_scrittura::scrivi_campi(
            &percorso,
            &Fields {
                title: Some("Titolo vecchio".to_owned()),
                artist: Some(UNKNOWN_ARTIST.to_owned()),
                ..Fields::default()
            },
            None,
        )
        .expect("tag di partenza");

        let aperto = crate::db::open(&cartella.path().join("aether.db")).expect("database");
        aperto
            .connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (1, ?1, 'k', 'Titolo vecchio', ?2, ?3, 'gruppo', 180000, 100, 0, 0)",
                rusqlite::params![
                    percorso.display().to_string(),
                    UNKNOWN_ARTIST,
                    UNKNOWN_ALBUM
                ],
            )
            .expect("inserimento del brano");
        (cartella, aperto.connection, percorso)
    }

    fn scrittura(percorso: &Path, campi: Fields) -> Scrittura {
        Scrittura {
            track_id: 1,
            path: percorso.display().to_string(),
            campi,
            copertina: None,
            fonte: "mb-release",
            confidenza: 0.95,
            originali: TagOriginali {
                title: Some("Titolo vecchio".to_owned()),
                artist: Some(UNKNOWN_ARTIST.to_owned()),
                album: Some(UNKNOWN_ALBUM.to_owned()),
                ..TagOriginali::default()
            },
        }
    }

    /// Esegue le due fasi di scrittura su una decisione già presa.
    fn applica(
        connection: &mut Connection,
        cartella: &Path,
        scritture: &[Scrittura],
    ) -> Registrati {
        let covers = CoverStore::open(cartella.join("copertine")).expect("store");
        let (esiti, guasti) = scrivi_file(&covers, scritture, true);
        assert!(guasti.is_empty(), "guasti in scrittura: {guasti:?}");
        let decisione = Decisione {
            album_key: "gruppo".to_owned(),
            ..Decisione::default()
        };
        let tx = connection.transaction().expect("transazione");
        let conto = registra(&tx, &decisione, scritture, &esiti, 1_000).expect("registrazione");
        tx.commit().expect("commit");
        conto
    }

    #[test]
    fn la_data_di_modifica_si_aggiorna_dopo_la_scrittura() {
        // Il difetto che questa prova impedisce, ed è il più insidioso di tutto
        // il modulo: senza, la scansione successiva vede ogni file arricchito
        // come «cambiato», lo rilegge per intero e lo riscrive dai tag —
        // riportando `enrich_status` a uno stato che non riflette più la
        // verità. E siccome anche quella scansione non aggiornerebbe niente di
        // diverso, il ciclo si ripeterebbe a ogni passata, per sempre.
        let (cartella, mut connection, percorso) = libreria();
        let scritture = vec![scrittura(
            &percorso,
            Fields {
                title: Some("Titolo nuovo".to_owned()),
                ..Fields::default()
            },
        )];
        applica(&mut connection, cartella.path(), &scritture);

        let (in_riga, dimensione): (i64, i64) = connection
            .query_row(
                "SELECT date_modified, file_size FROM tracks WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("lettura della riga");
        let sul_disco = std::fs::metadata(&percorso).expect("metadati");
        assert!(in_riga > 0, "la data non è stata riletta dal disco");
        assert_eq!(
            dimensione,
            i64::try_from(sul_disco.len()).unwrap_or(0),
            "anche la dimensione cambia: scrivere un tag allunga il file"
        );
    }

    #[test]
    fn un_brano_gia_a_posto_non_si_riscrive_e_non_si_annulla() {
        // Corrispondenza trovata, niente da cambiare. Il file non si apre —
        // aprirlo ne cambierebbe la data di modifica per niente — e non nasce
        // nessuna riga di annullamento, che il giorno dell'annullamento
        // riscriverebbe un file che nessuno aveva toccato.
        let (cartella, mut connection, percorso) = libreria();
        let prima = std::fs::metadata(&percorso)
            .expect("metadati")
            .modified()
            .ok();

        let scritture = vec![scrittura(&percorso, Fields::default())];
        let conto = applica(&mut connection, cartella.path(), &scritture);

        assert_eq!(conto.applicati, 1);
        let annullamenti: i64 = connection
            .query_row("SELECT COUNT(*) FROM enrich_undo", [], |row| row.get(0))
            .expect("conteggio");
        assert_eq!(annullamenti, 0, "non si è scritto niente da annullare");
        assert_eq!(
            std::fs::metadata(&percorso)
                .expect("metadati")
                .modified()
                .ok(),
            prima,
            "il file non doveva essere aperto"
        );
        // …e il brano risulta comunque fatto, altrimenti lo si richiederebbe a
        // ogni passata per sempre.
        let stato: Option<String> = connection
            .query_row("SELECT enrich_status FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("lettura");
        assert_eq!(stato.as_deref(), Some("ok"));
    }

    #[test]
    fn la_fotografia_si_scrive_una_volta_sola() {
        // È lo stato **prima che Aether ci mettesse le mani**, non quello del
        // passo precedente: sovrascriverla farebbe sì che «annulla» riporti il
        // file a una versione che l'utente non ha mai visto.
        let (cartella, mut connection, percorso) = libreria();
        applica(
            &mut connection,
            cartella.path(),
            &[scrittura(
                &percorso,
                Fields {
                    title: Some("Primo".to_owned()),
                    ..Fields::default()
                },
            )],
        );
        let mut seconda = scrittura(
            &percorso,
            Fields {
                title: Some("Secondo".to_owned()),
                ..Fields::default()
            },
        );
        seconda.originali.title = Some("Primo".to_owned());
        applica(&mut connection, cartella.path(), &[seconda]);

        let tags: String = connection
            .query_row(
                "SELECT tags FROM enrich_undo WHERE track_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("la fotografia c'è");
        let originali: TagOriginali = serde_json::from_str(&tags).expect("json");
        assert_eq!(
            originali.title.as_deref(),
            Some("Titolo vecchio"),
            "deve essere la prima fotografia, non la seconda"
        );
    }

    #[test]
    fn l_annullamento_riporta_i_tag_e_la_riga() {
        let (cartella, mut connection, percorso) = libreria();
        applica(
            &mut connection,
            cartella.path(),
            &[scrittura(
                &percorso,
                Fields {
                    title: Some("Titolo nuovo".to_owned()),
                    artist: Some("Interprete nuovo".to_owned()),
                    genre: Some("Pop".to_owned()),
                    mb_recording_id: Some("rec-1".to_owned()),
                    ..Fields::default()
                },
            )],
        );

        let conto = annulla(&mut connection, true).expect("annullamento");
        assert_eq!(conto.riportati, 1);
        assert_eq!(conto.falliti, 0);

        let letti =
            crate::metadata::read_tags(&crate::files::LocalFiles, &percorso.display().to_string())
                .expect("rilettura");
        assert_eq!(letti.title.as_deref(), Some("Titolo vecchio"));
        assert_eq!(letti.artist.as_deref(), Some(UNKNOWN_ARTIST));
        // Il genere non c'era prima: riportarcelo vuol dire toglierlo. È il
        // campo su cui un annullamento distratto lascerebbe indietro proprio
        // quel che l'arricchimento aveva **aggiunto**.
        assert_eq!(letti.genre, None);
        assert_eq!(letti.mb_recording_id, None);

        let (titolo, stato): (String, Option<String>) = connection
            .query_row(
                "SELECT title, enrich_status FROM tracks WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("lettura");
        assert_eq!(titolo, "Titolo vecchio");
        assert_eq!(stato, None, "il brano torna candidato");
        let rimaste: i64 = connection
            .query_row("SELECT COUNT(*) FROM enrich_undo", [], |row| row.get(0))
            .expect("conteggio");
        assert_eq!(rimaste, 0);
    }

    #[test]
    fn un_brano_gia_identificato_non_si_declassa() {
        // Porta un identificativo MusicBrainz — dai tag, o da una passata
        // precedente. Una ricerca che oggi non lo ritrova non è una ragione per
        // marchiarlo come sconosciuto.
        let (_cartella, mut connection, _percorso) = libreria();
        connection
            .execute(
                "UPDATE tracks SET mb_recording_id = 'rec-1' WHERE id = 1",
                [],
            )
            .expect("identificativo");

        let tx = connection.transaction().expect("transazione");
        segna_astensione(&tx, 1, Verdetto::Nessuno, 500).expect("astensione");
        tx.commit().expect("commit");

        let stato: Option<String> = connection
            .query_row("SELECT enrich_status FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("lettura");
        assert_eq!(stato, None);
    }

    #[test]
    fn un_astensione_si_ricorda_col_suo_verdetto() {
        let (_cartella, mut connection, _percorso) = libreria();
        let tx = connection.transaction().expect("transazione");
        segna_astensione(&tx, 1, Verdetto::DaRivedere, 500).expect("astensione");
        tx.commit().expect("commit");

        let (stato, quando): (Option<String>, Option<i64>) = connection
            .query_row(
                "SELECT enrich_status, enrich_attempted_at FROM tracks WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("lettura");
        assert_eq!(stato.as_deref(), Some("needs-review"));
        assert_eq!(quando, Some(500));
    }

    #[test]
    fn i_candidati_scadono_secondo_il_loro_verdetto() {
        // Un «non c'è» si ritenta dopo un mese, un «non ne sono sicuro» dopo
        // una settimana: l'asimmetria è quel che impedisce di richiedere ogni
        // giorno le stesse cose che nessun catalogo ha.
        let (_cartella, connection, _percorso) = libreria();
        let adesso = 100 * 24 * 60 * 60 * 1000_i64;

        connection
            .execute(
                "UPDATE tracks SET enrich_status = 'no-match', enrich_attempted_at = ?1",
                [adesso.saturating_sub(RITENTA_INCERTI_MS + 1000)],
            )
            .expect("stato");
        assert_eq!(
            quanti_mancano(&connection, adesso).expect("conteggio"),
            0,
            "una settimana non basta per un «non c'è»"
        );

        connection
            .execute(
                "UPDATE tracks SET enrich_attempted_at = ?1",
                [adesso.saturating_sub(RITENTA_NESSUNO_MS + 1000)],
            )
            .expect("stato");
        assert_eq!(
            quanti_mancano(&connection, adesso).expect("conteggio"),
            1,
            "dopo un mese si riprova"
        );
    }

    #[test]
    fn un_brano_gia_arricchito_non_e_piu_candidato() {
        let (_cartella, connection, _percorso) = libreria();
        assert_eq!(quanti_mancano(&connection, 1_000).expect("conteggio"), 1);
        connection
            .execute("UPDATE tracks SET enrich_status = 'ok'", [])
            .expect("stato");
        assert_eq!(quanti_mancano(&connection, 1_000).expect("conteggio"), 0);
    }

    #[test]
    fn un_gruppo_si_legge_intero_anche_se_solo_un_brano_ne_ha_bisogno() {
        // L'abbinamento confronta il numero di tracce con quello della
        // pubblicazione: un gruppo dimezzato non corrisponderebbe a niente.
        let (cartella, connection, _percorso) = libreria();
        let secondo = cartella.path().join("secondo.mp3");
        scrivi_mp3(&secondo);
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified,
                                     cover_art_hash, enrich_status)
                 VALUES (2, ?1, 'k2', 'Secondo', 'Artista', 'Album', 'gruppo',
                         200000, 100, 0, 0, NULL, 'ok')",
                [secondo.display().to_string()],
            )
            .expect("secondo brano");

        let gruppo = leggi_gruppo(&connection, "gruppo")
            .expect("lettura")
            .expect("il gruppo c'è");
        assert_eq!(gruppo.brani.len(), 2);
        assert!(
            gruppo.brani.iter().any(|b| b.gia_arricchito),
            "chi è già a posto si riconosce, ma resta nel gruppo"
        );
    }

    #[test]
    fn il_deposito_ricorda_anche_i_no() {
        // È la voce che vale di più: senza, un «non c'è» si richiederebbe per
        // sempre, e sono proprio i brani che nessuno riconosce a essere tanti.
        let cartella = tempfile::tempdir().expect("cartella");
        let percorso = cartella.path().join("aether.db");
        let _aperto = crate::db::open(&percorso).expect("database");
        let deposito = DepositoSqlite::apri(&percorso, 0).expect("deposito");

        assert_eq!(deposito.leggi("mb", "mai-chiesto"), None);

        deposito.scrivi("mb", "c-e", &Voce::Corpo(b"{}".to_vec()), 60_000);
        deposito.scrivi("mb", "non-c-e", &Voce::Niente, 60_000);

        assert_eq!(
            deposito.leggi("mb", "c-e"),
            Some(Voce::Corpo(b"{}".to_vec()))
        );
        assert_eq!(deposito.leggi("mb", "non-c-e"), Some(Voce::Niente));
    }

    #[test]
    fn una_voce_scaduta_vale_come_mai_chiesta() {
        let cartella = tempfile::tempdir().expect("cartella");
        let percorso = cartella.path().join("aether.db");
        let _aperto = crate::db::open(&percorso).expect("database");
        let deposito = DepositoSqlite::apri(&percorso, 0).expect("deposito");
        // Scadenza già passata: `vive_ms` negativo la mette nel passato.
        deposito.scrivi("mb", "vecchia", &Voce::Corpo(b"{}".to_vec()), -60_000);
        assert_eq!(deposito.leggi("mb", "vecchia"), None);
    }
}
