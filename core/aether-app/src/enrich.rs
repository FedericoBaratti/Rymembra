//! L'arricchimento: dalla libreria alla rete e ritorno.
//!
//! `aether_domain::enrich` decide, `aether_meta` chiede, questo modulo esegue.
//!
//! # Tre fasi, e il confine fra loro è il lucchetto
//!
//! ```text
//! candidati()   ──►  legge un lotto di gruppi          (lucchetto preso)
//! decidi()      ──►  cerca, scarica, decide            (SENZA lucchetto)
//! applica()     ──►  copertine su disco                (SENZA lucchetto)
//! registra()    ──►  righe, provenienza, aggregati     (lucchetto preso)
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
//! e non provato non lascia traccia: si annota la data del tentativo e si passa
//! oltre. È la scelta che rende accettabile una passata automatica su cui
//! nessuno guarda prima — la ragione per esteso sta in testa a
//! `aether_domain::enrich`.
//!
//! # Il file dell'utente non si tocca. Mai.
//!
//! Fino alla 2.3.0 questa era la sola scrittura automatica e non sorvegliata di
//! tutta Aether: ogni mezz'ora si aprivano i file di qualcun altro e se ne
//! riscrivevano i tag. Dalla 2.3.1 quel che si trova finisce **in database** —
//! nelle colonne di `tracks`, e annotato in `track_meta_arricchita` — mentre il
//! file resta byte per byte quello che era.
//!
//! Non è solo una promessa mantenuta: è ciò che fa cadere per intero il difetto
//! più insidioso che questo modulo si portava dietro. Scrivere i tag cambiava
//! la data di modifica sul disco, e bisognava ricordarsi di rileggerla e di
//! riscriverla in `tracks.date_modified` **nella stessa transazione**;
//! dimenticarlo faceva vedere alla scansione successiva ogni file arricchito
//! come «cambiato», glielo faceva rileggere per intero e riscrivere dai tag — a
//! ogni passata, per sempre. Oggi la data del file non cambia, quindi non c'è
//! niente da rileggere e niente da tenere in passo: `aggiorna_brano` **non**
//! scrive `date_modified` né `file_size`, e scriverli sarebbe il difetto, non
//! il rimedio.
//!
//! Quel che l'arricchimento mette sulla riga si toglie con [`dimentica`], che
//! rilegge i tag dal file — intatto, ed è tutto il punto — e ci rimette sopra
//! le correzioni a mano. La via di ritorno per i **file già riscritti dalle
//! versioni passate** esiste ancora e sta tutta in [`riporta_nei_file`]: è
//! un'uscita a termine, non una funzione di questo modulo. Vedi la sua nota.

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
    /// I tag di adesso, per i campi che la corrispondenza non copre.
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

/// I tag di un brano, come la riga li porta prima di una passata.
///
/// # Non è più una fotografia
///
/// Si chiamava così perché era quel che finiva in `enrich_undo` prima che
/// l'arricchimento riscrivesse il file: la copia da cui si tornava indietro.
/// Dalla 2.3.1 il file non lo tocca nessuno, e per tornare indietro basta
/// rileggerlo — quindi questi valori servono a due cose sole: riempire i campi
/// che la corrispondenza non copre quando si ricalcolano le chiavi, e
/// rileggere le fotografie che le versioni passate hanno lasciato in tabella.
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
    /// La copertina da salvare nello store, se se n'è trovata una.
    ///
    /// Salvare, non incorporare: nel file non entra. Vedi la nota in testa a
    /// [`crate::tag_scrittura::scrivi_campi`].
    pub copertina: Option<Copertina>,
    /// Chi ha deciso: `mb-release`, `mb-recording`, `itunes`, `deezer`.
    pub fonte: &'static str,
    /// Quanto ci si credeva, da 0 a 1.
    pub confidenza: f64,
    /// I tag di adesso, per i campi che [`plan_write`] ha deciso di non toccare.
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
///
/// `error` compare ancora fra gli stati da ritentare, e ci resta: dalla 2.3.1
/// **nessuno lo scrive più** — la fase che poteva fallire su un brano era la
/// scrittura dei tag sul file, e quella non c'è più — ma le librerie arricchite
/// dalle versioni passate ne hanno in tabella, e toglierlo di qui lascerebbe
/// quei brani fuori dai candidati per sempre.
///
/// Gli stati che non compaiono qui — `ok`, e `undone` scritto da [`dimentica`]
/// — sono **terminali**: il gruppo di condizioni sugli stati li esclude tutti.
/// Per `undone` è la sostanza dell'annullamento: riportare lo stato a `NULL`
/// farebbe riqualificare il brano come «mai provato», e la passata automatica
/// successiva riapplicherebbe — dalla cache, con la stessa confidenza — proprio
/// ciò che l'utente ha appena disfatto.
fn candidato_where() -> String {
    format!(
        "t.mb_recording_id IS NULL
         AND (
           t.artist = '{UNKNOWN_ARTIST}' OR t.album = '{UNKNOWN_ALBUM}'
           OR t.cover_art_hash IS NULL
           OR t.source = 'catalogo'
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
                    mb_recording_id, enrich_status, mb_release_id, mb_release_group_id
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
            let mb_release: Option<String> = row.get(14)?;
            let mb_release_group: Option<String> = row.get(15)?;
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
                    has_mb_recording_id: mb.is_some(),
                    has_mb_release_id: mb_release.is_some(),
                    has_mb_release_group_id: mb_release_group.is_some(),
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
    // non un'astensione: vuol dire che il brano era già a posto, e registrarlo
    // come «riuscito» è ciò che impedisce di richiederlo a ogni passata per
    // sempre. `applica` lo segnerà come non applicato, perché i campi sono
    // vuoti, e non nascerà nessuna annotazione da dimenticare.
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

// ── fase tre: mettere a terra quel che si è deciso ──────────────────────────

/// Cosa è successo applicando una decisione.
///
/// Il nome parla ancora di file perché questa è la fase che tocca il disco, ma
/// dalla 2.3.1 l'unica cosa che ci finisce sopra è **una copertina nello store
/// di Aether**: i file dell'utente non si aprono nemmeno. Vedi la nota in testa
/// al modulo.
#[derive(Debug)]
pub struct EsitoFile {
    /// Quale riga.
    pub track_id: i64,
    /// La copertina salvata nello store.
    pub copertina: Option<StoredCover>,
    /// La provenienza della copertina, per `cover_art.source`.
    pub copertina_da: Option<&'static str>,
    /// C'era davvero qualcosa da applicare.
    ///
    /// Falso quando la corrispondenza c'era ma non cambiava niente: il brano era
    /// già a posto. Serve a non annotare in `track_meta_arricchita` un brano su
    /// cui l'arricchimento non ha deciso nulla — «dimentica l'arricchimento» lo
    /// riporterebbe indietro da un'ipotesi mai applicata, e il pannello
    /// prometterebbe più brani da dimenticare di quanti ce ne siano.
    pub applicato: bool,
}

/// Salva le copertine trovate. **Non tocca il database, e non tocca i file.**
///
/// # Perché resta una fase a sé, ora che non scrive più tag
///
/// Perché è l'unica parte del lavoro che fa I/O su disco, e va fatta **senza il
/// lucchetto della libreria**: ricodificare e scrivere una copertina costa
/// millisecondi per brano, e farlo dentro la transazione vorrebbe dire tenere
/// ferma la riproduzione per tutta la durata. La divisione delle fasi è quella
/// di sempre; è la fase tre a essere diventata molto più piccola.
///
/// Restituisce un esito per ogni scrittura, e i guasti a parte: un'immagine
/// illeggibile non butta via titolo e anno del brano a cui apparteneva.
#[must_use]
pub fn applica(covers: &CoverStore, scritture: &[Scrittura]) -> (Vec<EsitoFile>, Vec<AppError>) {
    let mut esiti = Vec::with_capacity(scritture.len());
    let mut guasti = Vec::new();

    for scrittura in scritture {
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

        esiti.push(EsitoFile {
            track_id: scrittura.track_id,
            copertina,
            copertina_da,
            applicato: !scrittura.e_vuota(),
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
///
/// L'unico uso rimasto è [`riporta_nei_file`], che i file li riscrive davvero.
/// La passata normale non ne ha bisogno, e non deve averne: se ricomparisse
/// dentro [`registra`] vorrebbe dire che qualcuno ha ricominciato a toccare i
/// file dell'utente.
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
        // Solo per i brani su cui si è deciso davvero qualcosa: annotare un
        // brano già a posto lo farebbe contare fra quelli «da dimenticare»,
        // prometterebbe un ritorno che non ha niente da riportare indietro, e
        // gonfierebbe il numero che il pannello mostra sul pulsante.
        if esito.applicato {
            registra_arricchimento(tx, scrittura, adesso)?;
        }
        if let Some(copertina) = esito.copertina.as_ref() {
            registra_copertina(tx, copertina, esito.copertina_da, adesso)?;
            if !copertina.already_present {
                conto.copertine = conto.copertine.saturating_add(1);
            }
        }
        aggiorna_brano(tx, scrittura, esito, adesso)?;
        // E subito dopo si rimette sopra quel che l'utente aveva corretto a
        // mano. `plan_write` non scrive un campo [`Origine::Manuale`], quindi i
        // valori restano quelli giusti — ma `aggiorna_brano` ricalcola
        // `track_key` e `album_key` dai tag **del file**, e quelli non sanno
        // della correzione: senza questa riga il brano appena arricchito
        // tornerebbe ad avere l'identità che aveva quando si chiamava «Artista
        // sconosciuto». Gli aggregati li rifà [`ricostruisci`], a fine passata.
        crate::incerti::riapplica(tx, scrittura.track_id)?;
        conto.applicati = conto.applicati.saturating_add(1);
    }

    for (track_id, verdetto) in &decisione.astensioni {
        segna_astensione(tx, *track_id, *verdetto, adesso)?;
        conto.astenuti = conto.astenuti.saturating_add(1);
    }

    Ok(conto)
}

/// I campi che l'arricchimento ha messo sulla riga, nella forma che va in JSON.
///
/// # Perché non è `Fields`
///
/// La stessa ragione di [`TagOriginali`], e vale la pena non farla dimenticare:
/// `Fields` vive nel dominio, e il dominio non serializza niente da sé. Questa
/// ha la stessa forma e sa diventare JSON, perché è questo crate a parlare col
/// database.
///
/// Gli `Option` vuoti non si scrivono: la riga di `track_meta_arricchita` dice
/// **cosa** viene dall'arricchimento, e un campo assente vuol dire «questo no».
/// Serializzarli come `null` renderebbe indistinguibile «non l'ho scritto io»
/// da «l'ho scritto vuoto», che è la distinzione per cui l'annotazione esiste.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampiArricchiti {
    /// Il titolo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// L'interprete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// L'album.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// L'interprete dell'album.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    /// L'anno.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// Il genere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// Il numero di traccia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    /// Il numero di disco.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u32>,
    /// L'identificativo MusicBrainz della registrazione.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mb_recording_id: Option<String>,
    /// L'identificativo MusicBrainz della pubblicazione.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mb_release_id: Option<String>,
    /// L'identificativo MusicBrainz del gruppo di pubblicazione.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mb_release_group_id: Option<String>,
}

impl From<&Fields> for CampiArricchiti {
    fn from(campi: &Fields) -> Self {
        Self {
            title: campi.title.clone(),
            artist: campi.artist.clone(),
            album: campi.album.clone(),
            album_artist: campi.album_artist.clone(),
            year: campi.year,
            genre: campi.genre.clone(),
            track_number: campi.track_number,
            disc_number: campi.disc_number,
            mb_recording_id: campi.mb_recording_id.clone(),
            mb_release_id: campi.mb_release_id.clone(),
            mb_release_group_id: campi.mb_release_group_id.clone(),
        }
    }
}

/// Annota che questi campi della riga vengono dall'arricchimento.
///
/// # `ON CONFLICT DO UPDATE`, e non `INSERT OR IGNORE`
///
/// È il contrario di quel che faceva la fotografia dei tag in `enrich_undo`, e
/// il contrario è giusto perché le due righe dicono due cose opposte. Quella
/// diceva «com'era il file **prima** che Aether ci mettesse le mani», e andava
/// scritta una volta sola: sovrascriverla avrebbe riportato il file a una
/// versione che l'utente non aveva mai visto, cioè a quella scritta da noi il
/// giro prima. Questa dice «cosa c'è **adesso** sulla riga che venga da un
/// catalogo», e la verità è sempre l'ultima passata: una decisione più recente,
/// da una fonte più sicura, sostituisce quella di prima.
///
/// Nessun percorso in tabella, per la stessa ragione: l'annotazione descrive la
/// riga, non un file, e la riga la si ritrova per `track_id` anche dopo che il
/// suo file è stato spostato.
fn registra_arricchimento(
    tx: &Transaction<'_>,
    scrittura: &Scrittura,
    adesso: i64,
) -> Result<(), AppError> {
    let campi = serde_json::to_string(&CampiArricchiti::from(&scrittura.campi)).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("serializzazione dei campi arricchiti".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    tx.prepare_cached(
        "INSERT INTO track_meta_arricchita (track_id, campi, fonte, confidenza, set_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(track_id) DO UPDATE SET
           campi = excluded.campi,
           fonte = excluded.fonte,
           confidenza = excluded.confidenza,
           set_at = excluded.set_at",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![
            scrittura.track_id,
            campi,
            scrittura.fonte,
            scrittura.confidenza,
            adesso
        ])
    })
    .map(|_| ())
    .map_err(|err| db_error("annotazione dei metadati arricchiti", &err))
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

/// Riscrive la riga con quel che l'arricchimento ha deciso.
///
/// # `date_modified` e `file_size` non compaiono, ed è il cuore del pacchetto
///
/// Fin qui li scriveva, e **doveva**: i tag li aveva appena riscritti lui, la
/// data sul disco era cambiata, e non riportarla in tabella avrebbe fatto
/// rileggere per intero ogni file arricchito alla scansione successiva. Ora il
/// file non lo tocca nessuno: la riga porta già la data e la dimensione vere, e
/// riscriverle sarebbe nel migliore dei casi inutile e nel peggiore sbagliato —
/// una misura presa mentre un altro programma sta salvando quel file
/// dichiarerebbe «visto, è questo» un contenuto che nessuno ha letto, e la
/// scansione dopo lo salterebbe.
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
           enrich_status = 'ok',
           enrich_attempted_at = ?16,
           enrich_source = ?17,
           enrich_confidence = ?18
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

// ── dimenticare l'arricchimento ─────────────────────────────────────────────

/// Quanti brani un ritorno indietro ha riportato, e quanti no.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Annullati {
    /// Brani riportati a quel che dice il loro file.
    pub riportati: usize,
    /// Brani che non si sono potuti riportare.
    pub falliti: usize,
}

/// Dimentica l'arricchimento e rimette ogni riga sui tag del suo file.
///
/// # Perché basta rileggere il file
///
/// Perché il file non l'ha toccato nessuno, ed è tutto il punto della 2.3.1.
/// Fino alla 2.3.0 disfare voleva dire **riscrivere** migliaia di file dai tag
/// fotografati in `enrich_undo` prima di toccarli — un'operazione lunga,
/// rischiosa (un file bloccato, una chiavetta staccata a metà) e con una
/// fotografia da mantenere per sempre, perché senza di quella non si tornava
/// indietro. Adesso la fotografia è il file stesso: si rilegge, e per
/// costruzione dice esattamente quel che diceva prima della passata.
///
/// # L'ordine, e perché la correzione a mano viene per ultima
///
/// Per ogni brano: si cancella l'annotazione, si riscrive la riga dai tag
/// grezzi, e **poi** [`crate::incerti::riapplica`] rimette sopra quel che
/// l'utente aveva corretto. Dimenticare l'arricchimento non è dimenticare le
/// correzioni a mano: quelle stanno in un'altra tabella proprio perché questo
/// gesto non possa portarle via — vedi il commento della migrazione 019.
///
/// La copertina resta. Non sta nel file, sta nello store di Aether, e toglierla
/// vorrebbe dire rendere grigio in griglia un disco che l'utente vede
/// illustrato da mesi: è l'unica cosa che l'arricchimento aggiunge senza
/// sostituire niente, e dimenticarla non riporterebbe indietro nessun dato.
///
/// La rilettura dei file avviene **fuori** dalla transazione, come in ogni
/// altra passata di questo modulo: aprire migliaia di file tenendo il lucchetto
/// della libreria vorrebbe dire un'applicazione ferma per tutto quel tempo.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un file che non si rilegge —
/// una radice di rete staccata, un disco esterno spento — **non** fa fallire
/// il gesto: si conta fra i `falliti`, la sua annotazione **resta** dov'è, e
/// riprovare quando quel disco c'è porta a termine il lavoro. Cancellarla
/// lascerebbe la riga arricchita senza più niente che dica da dove viene.
pub fn dimentica(connection: &mut Connection) -> Result<Annullati, AppError> {
    let righe: Vec<(i64, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT m.track_id, t.path
                 FROM track_meta_arricchita m
                 JOIN tracks t ON t.id = m.track_id
                 ORDER BY m.set_at DESC",
            )
            .map_err(|err| db_error("elenco dei brani arricchiti", &err))?;
        let righe = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|err| db_error("elenco dei brani arricchiti", &err))?;
        righe
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error("elenco dei brani arricchiti", &err))?
    };

    let mut conto = Annullati::default();
    let mut riletti: Vec<(i64, String, Fields)> = Vec::new();
    for (track_id, percorso) in &righe {
        let Ok(tags) = crate::metadata::read_tags(&crate::files::LocalFiles, percorso) else {
            conto.falliti = conto.falliti.saturating_add(1);
            continue;
        };
        riletti.push((
            *track_id,
            percorso.clone(),
            Fields {
                title: tags.title,
                artist: tags.artist,
                album: tags.album,
                album_artist: tags.album_artist,
                year: tags.year,
                genre: tags.genre,
                track_number: tags.track_number,
                disc_number: tags.disc_number,
                mb_recording_id: tags.mb_recording_id,
                mb_release_id: tags.mb_release_id,
                mb_release_group_id: tags.mb_release_group_id,
            },
        ));
    }

    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione di ritorno", &err))?;
    for (track_id, percorso, grezzi) in &riletti {
        rimetti_i_tag_del_file(&tx, *track_id, percorso, grezzi)?;
        conto.riportati = conto.riportati.saturating_add(1);
    }
    ricostruisci(&tx)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione di ritorno", &err))?;

    Ok(conto)
}

/// Riporta una riga ai tag grezzi del suo file, e toglie l'annotazione.
///
/// I ripieghi sono **gli stessi di `library::read_track`**, e devono esserlo:
/// il titolo che diventa la radice del nome del file, «Artista sconosciuto»,
/// «Album sconosciuto». Sceglierne altri qui vorrebbe dire che dimenticare
/// l'arricchimento lascia una riga diversa da quella che una riscansione dello
/// stesso file produrrebbe un minuto dopo — e la differenza si vedrebbe come un
/// brano che cambia nome da solo.
///
/// `date_modified` e `file_size` non si toccano: il file non è cambiato né
/// quando l'arricchimento ha deciso, né adesso.
fn rimetti_i_tag_del_file(
    tx: &Transaction<'_>,
    track_id: i64,
    percorso: &str,
    grezzi: &Fields,
) -> Result<(), AppError> {
    let titolo = grezzi
        .title
        .clone()
        .unwrap_or_else(|| file_stem(percorso).to_owned());
    let artista = grezzi
        .artist
        .clone()
        .unwrap_or_else(|| UNKNOWN_ARTIST.to_owned());
    let album = grezzi
        .album
        .clone()
        .unwrap_or_else(|| UNKNOWN_ALBUM.to_owned());
    let track_key = TrackKey::compute(TrackKeyInput {
        artist: Some(&artista),
        title: Some(&titolo),
        album: Some(&album),
    })
    .into_string();
    let album_key = album_group_key(&album, percorso);

    tx.prepare_cached(
        "UPDATE tracks SET
           title = ?2, artist = ?3, album = ?4, album_artist = ?5,
           year = ?6, genre = ?7, track_number = ?8, disc_number = ?9,
           mb_recording_id = ?10, mb_release_id = ?11, mb_release_group_id = ?12,
           track_key = ?13, album_key = ?14,
           enrich_status = 'undone', enrich_source = NULL, enrich_confidence = NULL
         WHERE id = ?1",
    )
    .and_then(|mut statement| {
        statement.execute(rusqlite::params![
            track_id,
            titolo,
            artista,
            album,
            grezzi.album_artist,
            grezzi.year,
            grezzi.genre,
            grezzi.track_number,
            grezzi.disc_number,
            grezzi.mb_recording_id,
            grezzi.mb_release_id,
            grezzi.mb_release_group_id,
            track_key,
            album_key,
        ])
    })
    .map_err(|err| db_error("ritorno di un brano ai tag del file", &err))?;

    // E subito dopo la parola dell'utente, che resta l'ultima: vedi il `//!` di
    // [`crate::incerti`] per l'ordine di risoluzione per intero.
    crate::incerti::riapplica(tx, track_id)?;

    tx.prepare_cached("DELETE FROM track_meta_arricchita WHERE track_id = ?1")
        .and_then(|mut statement| statement.execute([track_id]))
        .map(|_| ())
        .map_err(|err| db_error("cancellazione di un'annotazione di arricchimento", &err))
}

// ── la via d'uscita a termine: i file già riscritti ─────────────────────────

/// Riscrive nei file i tag di prima, per quel che le versioni passate hanno già toccato.
///
/// # È un'uscita a termine, non una funzione
///
/// Fino alla 2.3.0 l'arricchimento riscriveva i tag dentro i file dell'utente, e
/// fotografava in `enrich_undo` com'erano prima di toccarli. Quei file **sono
/// già stati riscritti**: questa release non li riporta indietro d'ufficio, e
/// non potrebbe farlo senza contraddirsi — riscrivere migliaia di file è
/// esattamente la cosa che ha smesso di fare, e dopo mesi quei tag possono
/// essere stati approvati, sincronizzati o rifatti altrove.
///
/// Resta quindi disponibile, ma solo dietro un gesto esplicito che dice a
/// chiare lettere che **scrive nei file** (il comando
/// `arricchimento_riporta_nei_file`), e solo finché ci sono righe in
/// `enrich_undo`: la 2.3.0 è l'ultima versione che ne ha scritte, la tabella
/// può solo svuotarsi, e il CHANGELOG dichiara che questa via sparirà in una
/// release futura. Non è la metà che rende accettabile una scrittura
/// automatica — quella scrittura non c'è più, ed è lei ad aver reso accettabile
/// il valore di serie dell'interruttore.
///
/// Si legge il percorso di **adesso** da `tracks` — fra la scrittura e questo
/// momento può esserci passato un riordino — e si ripiega su quello registrato
/// quando la riga non c'è più.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un singolo file che non si
/// riscrive **non** fa fallire il ritorno: si conta e si prosegue, perché
/// arrendersi al primo file bloccato lascerebbe l'utente con metà libreria
/// riportata indietro e nessun modo di finire il lavoro.
pub fn riporta_nei_file(connection: &mut Connection) -> Result<Annullati, AppError> {
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
        if tag_scrittura::ripristina_campi(path, &originali.in_campi()).is_err() {
            conto.falliti = conto.falliti.saturating_add(1);
            continue;
        }
        let (modificato, dimensione) = misura(path);
        riusciti.push((*track_id, originali, modificato, dimensione));
    }

    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione di riscrittura", &err))?;
    for (track_id, originali, modificato, dimensione) in &riusciti {
        ripristina_riga(&tx, *track_id, originali, *modificato, *dimensione)?;
        conto.riportati = conto.riportati.saturating_add(1);
    }
    ricostruisci(&tx)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione di riscrittura", &err))?;

    Ok(conto)
}

/// Riporta una riga ai tag fotografati, e cancella la sua fotografia.
///
/// # La correzione a mano sopravvive anche a questo
///
/// I tag fotografati in `enrich_undo` sono quelli del **file**, e del file
/// nessuno ha mai chiesto il parere all'utente: chi aveva corretto a mano un
/// brano già arricchito e poi chiedeva di riportare i tag nei file si vedeva
/// portare via anche la propria correzione, perché questa riscrittura rifà i
/// campi descrittivi e `track_key` senza sapere che `track_overrides` esiste.
/// Era lo stesso difetto della scansione, nello stesso modulo, e si chiude allo
/// stesso modo: con [`crate::incerti::riapplica`] subito dopo l'`UPDATE`.
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
           enrich_status = 'undone', enrich_source = NULL, enrich_confidence = NULL
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

    // La parola dell'utente resta l'ultima, anche quando si riportano indietro
    // i tag del file: vedi la nota qui sopra.
    crate::incerti::riapplica(tx, track_id)?;

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

    /// Esegue le due fasi finali su una decisione già presa.
    fn passa(connection: &mut Connection, cartella: &Path, scritture: &[Scrittura]) -> Registrati {
        let covers = CoverStore::open(cartella.join("copertine")).expect("store");
        let (esiti, guasti) = applica(&covers, scritture);
        assert!(guasti.is_empty(), "guasti sulle copertine: {guasti:?}");
        let decisione = Decisione {
            album_key: "gruppo".to_owned(),
            ..Decisione::default()
        };
        let tx = connection.transaction().expect("transazione");
        let conto = registra(&tx, &decisione, scritture, &esiti, 1_000).expect("registrazione");
        tx.commit().expect("commit");
        conto
    }

    /// I byte del file e la sua data di modifica, per confrontarli con quelli di dopo.
    ///
    /// Tutt'e due e non uno solo: i byte da soli non accorgerebbero di una
    /// riscrittura che rimette lo stesso contenuto — e sarebbe comunque una
    /// riscrittura, con la data di modifica cambiata e la scansione successiva
    /// che rilegge tutto — mentre la data da sola non accorgerebbe di una
    /// scrittura su un orologio a bassa risoluzione.
    fn impronta(percorso: &Path) -> (Vec<u8>, std::time::SystemTime) {
        let byte = std::fs::read(percorso).expect("lettura del file");
        let quando = std::fs::metadata(percorso)
            .expect("metadati")
            .modified()
            .expect("data di modifica");
        (byte, quando)
    }

    #[test]
    fn la_passata_non_tocca_il_file() {
        // **La prova di questo pacchetto**, e la sola che valga da sola: una
        // passata che applica titolo, interprete, genere e identificativo non
        // deve lasciare sul file dell'utente né un byte diverso né un minuto
        // diverso. Commentando la riga che toglie `tag_scrittura::scrivi_campi`
        // da `applica`, questa prova fallisce su tutt'e due gli assert.
        let (cartella, mut connection, percorso) = libreria();
        let prima = impronta(&percorso);

        let scritture = vec![scrittura(
            &percorso,
            Fields {
                title: Some("Titolo nuovo".to_owned()),
                artist: Some("Interprete nuovo".to_owned()),
                genre: Some("Pop".to_owned()),
                mb_recording_id: Some("rec-1".to_owned()),
                ..Fields::default()
            },
        )];
        let conto = passa(&mut connection, cartella.path(), &scritture);
        assert_eq!(conto.applicati, 1);

        assert_eq!(
            prima,
            impronta(&percorso),
            "il file dell'utente è cambiato: byte o data di modifica"
        );
        // E i tag sul disco dicono ancora quel che dicevano: non è che siano
        // stati riscritti uguali, non sono stati riscritti.
        let letti =
            crate::metadata::read_tags(&crate::files::LocalFiles, &percorso.display().to_string())
                .expect("rilettura");
        assert_eq!(letti.title.as_deref(), Some("Titolo vecchio"));
        assert_eq!(letti.genre, None);
        assert_eq!(letti.mb_recording_id, None);

        // La riga invece è arricchita davvero, ed è il punto: il dato c'è,
        // semplicemente non sta dentro il file.
        let (titolo, genere, stato, data, dimensione): (
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
        ) = connection
            .query_row(
                "SELECT title, genre, enrich_status, date_modified, file_size
                 FROM tracks WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("lettura della riga");
        assert_eq!(titolo, "Titolo nuovo");
        assert_eq!(genere.as_deref(), Some("Pop"));
        assert_eq!(stato.as_deref(), Some("ok"));
        // E `date_modified` e `file_size` restano quelli che erano — 0 e 100,
        // come li ha messi il fixture. Riscriverli farebbe rileggere il file
        // alla scansione dopo, che è il difetto che il pacchetto chiude.
        assert_eq!(data, 0, "la data del file non è cambiata: non si riscrive");
        assert_eq!(dimensione, 100, "e nemmeno la dimensione");

        // …e quel che si è applicato è annotato, con la sua fonte.
        let (campi, fonte, confidenza): (String, Option<String>, Option<f64>) = connection
            .query_row(
                "SELECT campi, fonte, confidenza FROM track_meta_arricchita WHERE track_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("l'annotazione c'è");
        let arricchiti: CampiArricchiti = serde_json::from_str(&campi).expect("json");
        assert_eq!(arricchiti.title.as_deref(), Some("Titolo nuovo"));
        assert_eq!(arricchiti.album, None, "l'album non lo ha deciso nessuno");
        assert_eq!(fonte.as_deref(), Some("mb-release"));
        assert!(confidenza.is_some_and(|c| c > 0.9));
    }

    #[test]
    fn un_brano_gia_a_posto_non_si_annota() {
        // Corrispondenza trovata, niente da cambiare. Il brano risulta fatto —
        // altrimenti lo si richiederebbe a ogni passata per sempre — ma non
        // nasce nessuna annotazione: non c'è niente da dimenticare, e contarlo
        // gonfierebbe il numero che il pannello scrive sul pulsante.
        let (cartella, mut connection, percorso) = libreria();
        let prima = impronta(&percorso);

        let scritture = vec![scrittura(&percorso, Fields::default())];
        let conto = passa(&mut connection, cartella.path(), &scritture);

        assert_eq!(conto.applicati, 1);
        let annotazioni: i64 = connection
            .query_row("SELECT COUNT(*) FROM track_meta_arricchita", [], |row| {
                row.get(0)
            })
            .expect("conteggio");
        assert_eq!(annotazioni, 0, "non si è deciso niente da dimenticare");
        assert_eq!(prima, impronta(&percorso), "e il file men che meno");

        let stato: Option<String> = connection
            .query_row("SELECT enrich_status FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("lettura");
        assert_eq!(stato.as_deref(), Some("ok"));
    }

    #[test]
    fn l_arricchimento_non_scavalca_una_correzione_manuale() {
        // `plan_write` non scrive un campo [`Origine::Manuale`], quindi
        // `campi.title` arriva vuoto e la colonna si salva col `COALESCE`. Ma
        // `aggiorna_brano` ricalcola `track_key` e `album_key` dai tag **del
        // file**, che della correzione non sanno niente: senza
        // `incerti::riapplica` il brano appena arricchito tornerebbe ad avere
        // l'identità che aveva quando si chiamava «Artista sconosciuto», e la
        // sincronizzazione lo cercherebbe con quel nome su ogni altro
        // dispositivo.
        let (cartella, mut connection, percorso) = libreria();
        crate::incerti::correggi(
            &mut connection,
            1,
            &crate::provenienza::Correzioni {
                titolo: Some("Comfortably Numb".to_owned()),
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");
        let corretta: String = connection
            .query_row("SELECT track_key FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("chiave corretta");

        // L'arricchimento scrive quel che il manuale non copre: l'anno.
        let scritture = vec![scrittura(
            &percorso,
            Fields {
                year: Some(1979),
                ..Fields::default()
            },
        )];
        passa(&mut connection, cartella.path(), &scritture);

        let (titolo, artista, chiave, anno): (String, String, String, Option<i64>) = connection
            .query_row(
                "SELECT title, artist, track_key, year FROM tracks WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("riga");
        assert_eq!(titolo, "Comfortably Numb");
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(chiave, corretta, "l'identità non torna ai tag del file");
        // …e l'arricchimento fa comunque il suo lavoro su quel che nessuno aveva
        // deciso a mano: la riapplicazione è l'ultima parola, non un divieto.
        assert_eq!(anno, Some(1979));
    }

    #[test]
    fn l_annotazione_dice_l_ultima_passata() {
        // Il contrario esatto della fotografia che stava in `enrich_undo`, e
        // il contrario è giusto: quella diceva «com'era prima», e sovrascriverla
        // avrebbe riportato il file a una versione mai vista; questa dice «da
        // dove viene quel che c'è adesso», e la verità è l'ultima passata.
        let (cartella, mut connection, percorso) = libreria();
        passa(
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
        seconda.fonte = "itunes";
        seconda.confidenza = 0.72;
        passa(&mut connection, cartella.path(), &[seconda]);

        let (quante, campi, fonte): (i64, String, Option<String>) = connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM track_meta_arricchita), campi, fonte
                 FROM track_meta_arricchita WHERE track_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("l'annotazione c'è");
        assert_eq!(quante, 1, "una riga per brano, non una per passata");
        let arricchiti: CampiArricchiti = serde_json::from_str(&campi).expect("json");
        assert_eq!(arricchiti.title.as_deref(), Some("Secondo"));
        assert_eq!(fonte.as_deref(), Some("itunes"));
    }

    #[test]
    #[expect(
        clippy::integer_division,
        reason = "`i64::MAX / 2` è solo un «adesso» lontanissimo che non trabocca \
                  quando ci si somma una scadenza: il resto non esiste"
    )]
    fn dimenticare_riporta_la_riga_ai_tag_del_file() {
        let (cartella, mut connection, percorso) = libreria();
        passa(
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
        let prima = impronta(&percorso);

        let conto = dimentica(&mut connection).expect("dimenticato");
        assert_eq!(conto.riportati, 1);
        assert_eq!(conto.falliti, 0);

        // Non serve nessuna fotografia: la fotografia è il file, che è lì.
        let (titolo, artista, genere, mb, stato): (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = connection
            .query_row(
                "SELECT title, artist, genre, mb_recording_id, enrich_status
                 FROM tracks WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("lettura");
        assert_eq!(titolo, "Titolo vecchio");
        assert_eq!(artista, UNKNOWN_ARTIST);
        // Il genere non c'era nel file: dimenticarlo vuol dire toglierlo. È il
        // campo su cui un ritorno distratto lascerebbe indietro proprio quel
        // che l'arricchimento aveva **aggiunto**.
        assert_eq!(genere, None);
        assert_eq!(mb, None);
        // Uno stato terminale, non un ritorno a «mai provato»: con la riga
        // azzerata la passata automatica successiva rimetterebbe — dalla cache,
        // con la stessa confidenza — proprio quel che è appena stato disfatto.
        assert_eq!(stato.as_deref(), Some("undone"));
        assert_eq!(
            quanti_mancano(&connection, i64::MAX / 2).expect("conteggio"),
            0,
            "un brano dimenticato non torna candidato"
        );

        let rimaste: i64 = connection
            .query_row("SELECT COUNT(*) FROM track_meta_arricchita", [], |row| {
                row.get(0)
            })
            .expect("conteggio");
        assert_eq!(rimaste, 0);
        // E nemmeno dimenticare tocca il file: né prima né adesso.
        assert_eq!(prima, impronta(&percorso));
    }

    #[test]
    fn dimenticare_non_cancella_le_correzioni_manuali() {
        // È la ragione per cui `track_meta_arricchita` è una tabella sua e non
        // una colonna dentro `track_overrides`: fuse, questa cancellazione
        // porterebbe via anche la parola dell'utente.
        let (cartella, mut connection, percorso) = libreria();
        crate::incerti::correggi(
            &mut connection,
            1,
            &crate::provenienza::Correzioni {
                titolo: Some("Comfortably Numb".to_owned()),
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");
        let corretta: String = connection
            .query_row("SELECT track_key FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("chiave corretta");

        passa(
            &mut connection,
            cartella.path(),
            &[scrittura(
                &percorso,
                Fields {
                    year: Some(1979),
                    ..Fields::default()
                },
            )],
        );
        dimentica(&mut connection).expect("dimenticato");

        let (titolo, artista, chiave, anno): (String, String, String, Option<i64>) = connection
            .query_row(
                "SELECT title, artist, track_key, year FROM tracks WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("riga");
        assert_eq!(titolo, "Comfortably Numb");
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(chiave, corretta);
        // …e quel che veniva davvero dall'arricchimento se n'è andato.
        assert_eq!(anno, None);
        let salvate: i64 = connection
            .query_row("SELECT COUNT(*) FROM track_overrides", [], |row| row.get(0))
            .expect("conteggio");
        assert_eq!(salvate, 1, "la correzione è ancora salvata dov'era");
    }

    #[test]
    fn annullare_un_arricchimento_non_cancella_una_correzione_manuale() {
        // Il lascito dell'ondata 1, e l'ultimo posto in cui viveva il bug 1:
        // `ripristina_riga` rifà i campi descrittivi e `track_key` dai tag
        // fotografati in `enrich_undo`, che di `track_overrides` non sanno
        // niente. Chi aveva corretto a mano un brano già arricchito da una
        // versione passata, e poi chiedeva di riportare i tag nei file, perdeva
        // anche la propria correzione. Togliendo `incerti::riapplica` da
        // `ripristina_riga`, questa prova fallisce.
        let (_cartella, mut connection, percorso) = libreria();

        // Una fotografia come la scriveva la 2.3.0, e i tag che quella versione
        // aveva messo nel file.
        let originali = TagOriginali {
            title: Some("Titolo vecchio".to_owned()),
            artist: Some(UNKNOWN_ARTIST.to_owned()),
            ..TagOriginali::default()
        };
        connection
            .execute(
                "INSERT INTO enrich_undo (track_id, path, tags, written_at)
                 VALUES (1, ?1, ?2, 0)",
                rusqlite::params![
                    percorso.display().to_string(),
                    serde_json::to_string(&originali).expect("json")
                ],
            )
            .expect("fotografia di una versione passata");
        tag_scrittura::scrivi_campi(
            &percorso,
            &Fields {
                title: Some("Titolo arricchito".to_owned()),
                artist: Some("Interprete arricchito".to_owned()),
                ..Fields::default()
            },
        )
        .expect("tag come li aveva riscritti la 2.3.0");
        connection
            .execute(
                "UPDATE tracks SET title = 'Titolo arricchito',
                 artist = 'Interprete arricchito', enrich_status = 'ok' WHERE id = 1",
                [],
            )
            .expect("riga come l'aveva lasciata la 2.3.0");

        // …e poi l'utente corregge a mano.
        crate::incerti::correggi(
            &mut connection,
            1,
            &crate::provenienza::Correzioni {
                titolo: Some("Comfortably Numb".to_owned()),
                artista: Some("Pink Floyd".to_owned()),
                ..crate::provenienza::Correzioni::default()
            },
        )
        .expect("correzione");
        let corretta: String = connection
            .query_row("SELECT track_key FROM tracks WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("chiave corretta");

        let conto = riporta_nei_file(&mut connection).expect("ritorno nei file");
        assert_eq!(conto.riportati, 1);
        assert_eq!(conto.falliti, 0);

        // I tag del file sono tornati indietro — è quel che il comando promette.
        let letti =
            crate::metadata::read_tags(&crate::files::LocalFiles, &percorso.display().to_string())
                .expect("rilettura");
        assert_eq!(letti.title.as_deref(), Some("Titolo vecchio"));
        assert_eq!(letti.artist.as_deref(), Some(UNKNOWN_ARTIST));

        // Ma la parola dell'utente è ancora l'ultima, sulla riga e sull'identità.
        let (titolo, artista, chiave): (String, String, String) = connection
            .query_row(
                "SELECT title, artist, track_key FROM tracks WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("riga");
        assert_eq!(titolo, "Comfortably Numb");
        assert_eq!(artista, "Pink Floyd");
        assert_eq!(chiave, corretta);
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
