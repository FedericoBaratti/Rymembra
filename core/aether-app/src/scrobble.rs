//! La coda degli scrobble: quel che è stato ascoltato e non è ancora uscito.
//!
//! # Perché una coda, e non «manda e basta»
//!
//! Perché mandare è una richiesta HTTP, e una richiesta HTTP fallisce per
//! ragioni che non hanno niente a che vedere con la musica: il portatile è stato
//! chiuso, il treno è entrato in galleria, il servizio è in manutenzione. Senza
//! una coda su disco ognuna di quelle ragioni cancella un ascolto in silenzio,
//! nel momento in cui non c'è nessuno da avvisare.
//!
//! E la cronologia non basta a rimediare dopo: `play_history` sa **cosa** è stato
//! ascoltato, non **cosa è già stato mandato a chi**. Senza quest'altra
//! informazione l'unico recupero possibile sarebbe rimandare tutto — e su
//! Last.fm uno scrobble duplicato resta duplicato per sempre.
//!
//! # Chi decide cosa entra
//!
//! Non questo modulo. Ci entra quel che è già un ascolto secondo
//! [`aether_domain::listen::counts_as_play`], cioè le stesse righe che finiscono
//! in `play_history`: `record_play` non ne scrive altre, e l'importazione da
//! Spotify passa per la stessa soglia. Una seconda regola qui sarebbe la terza
//! misura dello stesso ascolto.
//!
//! L'unico filtro che questo modulo applica è [`Ascolto::valido`] — artista e
//! titolo non vuoti — e non è una regola di merito: è che un file senza tag
//! esiste in ogni libreria, e un ascolto senza artista è una riga che i servizi
//! rifiutano per sempre restando in coda a ritentare.
//!
//! # Chi decide se ritentare
//!
//! Il catalogo, come per i desiderati: [`fallite`] riceve la ritentabilità già
//! decisa da [`AppError::is_retryable`] e si limita a contare. Un guasto non
//! ritentabile — la sessione revocata, l'ascolto rifiutato — porta i tentativi
//! **direttamente al tetto**, invece di consumarne uno alla volta dieci volte
//! per arrivare alla stessa conclusione.
//!
//! # Cosa non passa mai di qui
//!
//! Il «sta ascoltando adesso». Non è un ascolto: non si conserva, non si conta,
//! e uno rimasto indietro perché non c'era rete descriverebbe un brano finito
//! venti minuti fa — cioè direbbe il falso. Si manda subito o non si manda, e
//! quel gesto sta nella finestra, non in questa tabella.

use aether_domain::errors::AppError;
use aether_domain::scrobble::{Ascolto, Servizio};
use rusqlite::Connection;

use crate::library::{db_error, now_ms};

/// Quante volte si riprova un ascolto prima di lasciarlo perdere.
///
/// Dieci, contro i tre dei desiderati, e la differenza è voluta: là un
/// tentativo costa uno scaricamento da YouTube, qui costa una riga in un
/// documento JSON. Il rischio da coprire è opposto — non consumare la coda, ma
/// **non perdere un ascolto** perché il servizio è stato giù per una giornata.
///
/// Chi arriva al tetto non sparisce: resta in tabella come abbandonato, si
/// conta, e si può rimettere in fila o buttare. Un ascolto che sparisce da solo
/// è la cosa che questa tabella esiste per impedire.
pub const MASSIMI_TENTATIVI: u32 = 10;

/// Il nome utente su ListenBrainz, quando è collegato.
///
/// In `settings` e non nel portachiavi: è un nome pubblico, sta sulla pagina di
/// chiunque. Il **token** invece sta nel portachiavi, e questo modulo non lo
/// vede mai.
pub const CHIAVE_LB_UTENTE: &str = "scrobble.listenbrainz.utente";

/// Il nome utente su Last.fm, quando è collegato.
pub const CHIAVE_LFM_UTENTE: &str = "scrobble.lastfm.utente";

/// La chiave dell'applicazione Last.fm, scritta dall'utente.
///
/// In `settings` per la stessa ragione del `client_id` di Spotify: viaggia in
/// chiaro nell'indirizzo del consenso, quindi non è un segreto. Il **segreto**
/// che le sta accanto, quello sì, sta nel portachiavi.
pub const CHIAVE_LFM_API_KEY: &str = "scrobble.lastfm.api_key";

/// Mandare quel che si ascolta è acceso.
pub const CHIAVE_ATTIVO: &str = "scrobble.attivo";

/// Una riga della coda, pronta da mandare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voce {
    /// La riga, per poterla chiudere dopo.
    pub id: i64,
    /// L'ascolto, nella forma che i due protocolli si aspettano.
    pub ascolto: Ascolto,
    /// Quante volte ci si è già provati.
    pub tentativi: u32,
}

/// Quanto c'è in coda.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Conteggi {
    /// Quanti aspettano di partire.
    pub in_attesa: i64,
    /// Quanti hanno finito i tentativi.
    pub abbandonati: i64,
}

/// Da millisecondi a secondi, che è l'unità dei due protocolli.
///
/// `div_euclid` e non `/`: la divisione fra interi è un avviso di questo
/// workspace, e su un istante prima del 1970 — che non capita, ma che una
/// libreria importata da un sistema con l'orologio storto può produrre — la
/// divisione normale arrotonderebbe verso lo zero invece che verso il basso,
/// spostando l'ascolto avanti di un secondo.
const fn secondi(ms: i64) -> i64 {
    ms.div_euclid(1000)
}

/// Accoda un ascolto per i servizi indicati.
///
/// Restituisce quante righe ha davvero scritto: zero significa che c'erano già,
/// che è la condizione normale quando si riaccoda.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura non riesce.
pub fn accoda(
    connection: &Connection,
    servizi: &[Servizio],
    ascolto: &Ascolto,
) -> Result<usize, AppError> {
    if !ascolto.valido() || servizi.is_empty() {
        return Ok(0);
    }
    let quando_ms = ascolto.quando_s.saturating_mul(1000);
    let ora = now_ms();
    let mut scritte = 0;
    for servizio in servizi {
        scritte += connection
            .execute(
                "INSERT OR IGNORE INTO scrobble_queue
                   (service, played_at, artist, title, album, album_artist,
                    duration_ms, track_number, mbid, queued_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    servizio.chiave(),
                    quando_ms,
                    ascolto.artista.trim(),
                    ascolto.titolo.trim(),
                    ascolto.album,
                    ascolto.artista_album,
                    ascolto.durata_ms.and_then(|ms| i64::try_from(ms).ok()),
                    ascolto.numero_traccia,
                    ascolto.mbid_registrazione,
                    ora,
                ],
            )
            .map_err(|err| db_error("accodamento di uno scrobble", &err))?;
    }
    Ok(scritte)
}

/// L'ascolto di un brano della libreria, com'era al momento in cui è finito.
///
/// I tag li legge **adesso**, perché adesso è il momento più vicino all'ascolto
/// in cui li si possa leggere: è la copia che poi la coda si porta dietro.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn ascolto_di(
    connection: &Connection,
    track_id: i64,
    quando_ms: i64,
) -> Result<Option<Ascolto>, AppError> {
    let letto = connection
        .query_row(
            "SELECT artist, title, album, album_artist, duration_ms, track_number, mb_recording_id
               FROM tracks WHERE id = ?1",
            [track_id],
            |riga| {
                Ok(Ascolto {
                    artista: riga.get::<_, String>(0)?,
                    titolo: riga.get::<_, String>(1)?,
                    album: vuoto_e_niente(riga.get::<_, Option<String>>(2)?),
                    artista_album: vuoto_e_niente(riga.get::<_, Option<String>>(3)?),
                    durata_ms: riga
                        .get::<_, Option<i64>>(4)?
                        .filter(|ms| *ms > 0)
                        .and_then(|ms| u64::try_from(ms).ok()),
                    numero_traccia: riga
                        .get::<_, Option<i64>>(5)?
                        .and_then(|n| u32::try_from(n).ok()),
                    mbid_registrazione: vuoto_e_niente(riga.get::<_, Option<String>>(6)?),
                    quando_s: secondi(quando_ms),
                })
            },
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(db_error("lettura del brano da scrobblare", &altro)),
        })?;
    Ok(letto)
}

/// Una stringa vuota è un campo assente, non un campo vuoto.
///
/// `tracks.album` è `NOT NULL` e vale `""` quando il tag non c'è. Mandare un
/// `release_name` vuoto vorrebbe dire dichiarare al servizio che l'album si
/// chiama così.
fn vuoto_e_niente(valore: Option<String>) -> Option<String> {
    valore.filter(|t| !t.trim().is_empty())
}

/// Accoda tutta la cronologia già registrata.
///
/// È il gesto che chiude il cerchio dell'importazione da Spotify: anni di
/// ascolti arrivati nell'archivio diventano la propria cronologia su un servizio
/// che non appartiene a nessuna piattaforma. **Ha senso solo verso
/// ListenBrainz**: Last.fm rifiuta le date vecchie e ha un tetto giornaliero, e
/// quarantamila righe là dentro non entrerebbero comunque.
///
/// `sorgente` restringe a una provenienza di `play_history` (`local`,
/// `spotify`); `da_ms` a partire da un istante. `INSERT OR IGNORE` sul vincolo
/// di unicità: chiamarla due volte non raddoppia niente.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn accoda_cronologia(
    connection: &Connection,
    servizio: Servizio,
    sorgente: Option<&str>,
    da_ms: i64,
) -> Result<usize, AppError> {
    connection
        .execute(
            "INSERT OR IGNORE INTO scrobble_queue
               (service, played_at, artist, title, album, album_artist,
                duration_ms, track_number, mbid, queued_at)
             SELECT ?1,
                    h.played_at,
                    trim(t.artist),
                    trim(t.title),
                    NULLIF(trim(t.album), ''),
                    NULLIF(trim(COALESCE(t.album_artist, '')), ''),
                    NULLIF(t.duration_ms, 0),
                    t.track_number,
                    t.mb_recording_id,
                    ?2
               FROM play_history AS h
               JOIN tracks AS t ON t.id = h.track_id
              WHERE trim(t.artist) <> ''
                AND trim(t.title) <> ''
                AND h.played_at >= ?3
                AND (?4 IS NULL OR h.source = ?4)",
            rusqlite::params![servizio.chiave(), now_ms(), da_ms, sorgente],
        )
        .map_err(|err| db_error("accodamento della cronologia", &err))
}

/// Cosa resta da mandare a un servizio, dal più vecchio.
///
/// Dal più vecchio perché è l'ordine in cui i due servizi si aspettano di
/// ricevere, e perché una coda che parte dai nuovi lascia i vecchi in fondo per
/// sempre.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn da_mandare(
    connection: &Connection,
    servizio: Servizio,
    quanti: usize,
) -> Result<Vec<Voce>, AppError> {
    let limite = i64::try_from(quanti).unwrap_or(i64::MAX);
    let mut istruzione = connection
        .prepare(
            "SELECT id, played_at, artist, title, album, album_artist,
                    duration_ms, track_number, mbid, attempts
               FROM scrobble_queue
              WHERE service = ?1 AND attempts < ?2
              ORDER BY played_at ASC, id ASC
              LIMIT ?3",
        )
        .map_err(|err| db_error("preparazione della coda scrobble", &err))?;

    let righe = istruzione
        .query_map(
            rusqlite::params![servizio.chiave(), MASSIMI_TENTATIVI, limite],
            |riga| {
                Ok(Voce {
                    id: riga.get(0)?,
                    ascolto: Ascolto {
                        artista: riga.get::<_, String>(2)?,
                        titolo: riga.get::<_, String>(3)?,
                        album: riga.get::<_, Option<String>>(4)?,
                        artista_album: riga.get::<_, Option<String>>(5)?,
                        durata_ms: riga
                            .get::<_, Option<i64>>(6)?
                            .and_then(|ms| u64::try_from(ms).ok()),
                        numero_traccia: riga
                            .get::<_, Option<i64>>(7)?
                            .and_then(|n| u32::try_from(n).ok()),
                        mbid_registrazione: riga.get::<_, Option<String>>(8)?,
                        quando_s: secondi(riga.get::<_, i64>(1)?),
                    },
                    tentativi: riga.get::<_, i64>(9)?.try_into().unwrap_or(u32::MAX),
                })
            },
        )
        .map_err(|err| db_error("lettura della coda scrobble", &err))?;

    let mut voci = Vec::new();
    for riga in righe {
        voci.push(riga.map_err(|err| db_error("lettura di una riga della coda scrobble", &err))?);
    }
    Ok(voci)
}

/// Toglie dalla coda quel che è arrivato.
///
/// **Anche quel che il servizio ha ricevuto e ignorato**: un ascolto che Last.fm
/// scarta perché la data è troppo vecchia non tornerà mai accettato, e
/// rimetterlo in fila vorrebbe dire rimandarlo per sempre. È consegnato, e il
/// motivo dell'ignoro lo racconta il rapporto, non la coda.
///
/// # Il lotto si chiude tutto insieme, o non si chiude
///
/// La transazione non è un'ottimizzazione. Fuori da una, ogni `DELETE` è una
/// transazione implicita a sé: una caduta a metà ciclo lascerebbe una parte del
/// lotto cancellata e il resto in coda, e quel resto **il servizio l'ha già
/// ricevuto**. La passata dopo lo rimanderebbe, cioè scrobble doppi su Last.fm e
/// ListenBrainz — e un ascolto contato due volte è esattamente il dato che una
/// riscansione non sa rimettere a posto.
///
/// Che poi cinquanta righe costino un commit invece di cinquanta è il secondo
/// motivo, non il primo.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn fatte(connection: &Connection, id: &[i64]) -> Result<usize, AppError> {
    if id.is_empty() {
        return Ok(0);
    }
    let transazione = connection
        .unchecked_transaction()
        .map_err(|err| db_error("apertura della chiusura scrobble", &err))?;
    let mut tolte = 0;
    {
        let mut istruzione = transazione
            .prepare("DELETE FROM scrobble_queue WHERE id = ?1")
            .map_err(|err| db_error("preparazione della chiusura scrobble", &err))?;
        for uno in id {
            tolte += istruzione
                .execute([uno])
                .map_err(|err| db_error("chiusura di uno scrobble", &err))?;
        }
    }
    transazione
        .commit()
        .map_err(|err| db_error("chiusura del lotto scrobble", &err))?;
    Ok(tolte)
}

/// Segna un tentativo andato male.
///
/// `definitivo` porta i tentativi al tetto in un colpo solo, invece di
/// consumarne uno per volta: quando il guasto non è ritentabile, le altre nove
/// prove darebbero tutte lo stesso esito e nel frattempo terrebbero la riga in
/// testa alla coda.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn fallite(
    connection: &Connection,
    id: &[i64],
    causa: &str,
    definitivo: bool,
) -> Result<usize, AppError> {
    if id.is_empty() {
        return Ok(0);
    }
    let sql = if definitivo {
        "UPDATE scrobble_queue SET attempts = ?2, last_error = ?3 WHERE id = ?1"
    } else {
        "UPDATE scrobble_queue SET attempts = MIN(attempts + 1, ?2), last_error = ?3 WHERE id = ?1"
    };
    // In transazione per la stessa ragione di [`fatte`], al contrario: un lotto
    // segnato per metà lascerebbe l'altra metà con i tentativi di prima, cioè in
    // testa alla coda a ritentare un guasto già dichiarato definitivo.
    let transazione = connection
        .unchecked_transaction()
        .map_err(|err| db_error("apertura del fallimento scrobble", &err))?;
    let mut segnate = 0;
    {
        let mut istruzione = transazione
            .prepare(sql)
            .map_err(|err| db_error("preparazione del fallimento scrobble", &err))?;
        let causa = ritaglia(causa);
        for uno in id {
            segnate += istruzione
                .execute(rusqlite::params![uno, MASSIMI_TENTATIVI, causa])
                .map_err(|err| db_error("registrazione di un fallimento scrobble", &err))?;
        }
    }
    transazione
        .commit()
        .map_err(|err| db_error("chiusura dei fallimenti scrobble", &err))?;
    Ok(segnate)
}

/// La causa, tagliata a una lunghezza che ha senso mostrare.
///
/// Senza taglio, una pagina HTML di errore restituita da un proxy finirebbe
/// intera dentro una colonna che l'interfaccia mostra in una riga.
fn ritaglia(causa: &str) -> String {
    causa.chars().take(300).collect()
}

/// Rimette in fila gli abbandonati.
///
/// Serve dopo aver rimediato a quel che li aveva fermati — ricollegare un
/// account, aspettare che il servizio torni su. Azzera i tentativi, non
/// l'errore: quel che è successo l'altra volta resta scritto finché non ne
/// succede un altro.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn rimetti_in_fila(
    connection: &Connection,
    servizio: Option<Servizio>,
) -> Result<usize, AppError> {
    connection
        .execute(
            "UPDATE scrobble_queue SET attempts = 0
              WHERE attempts >= ?1 AND (?2 IS NULL OR service = ?2)",
            rusqlite::params![MASSIMI_TENTATIVI, servizio.map(Servizio::chiave)],
        )
        .map_err(|err| db_error("rimessa in fila degli scrobble", &err))
}

/// Butta via delle righe della coda.
///
/// `solo_abbandonati` limita a quelle che hanno finito i tentativi — che è
/// l'unico uso previsto dall'interfaccia. Il caso generale esiste per lo
/// scollegamento: chi toglie un servizio non vuole trovarne la coda intatta al
/// ricollegamento successivo, con dentro ascolti di mesi prima.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn dimentica(
    connection: &Connection,
    servizio: Option<Servizio>,
    solo_abbandonati: bool,
) -> Result<usize, AppError> {
    connection
        .execute(
            "DELETE FROM scrobble_queue
              WHERE (?1 IS NULL OR service = ?1)
                AND (?2 = 0 OR attempts >= ?3)",
            rusqlite::params![
                servizio.map(Servizio::chiave),
                i64::from(solo_abbandonati),
                MASSIMI_TENTATIVI
            ],
        )
        .map_err(|err| db_error("svuotamento della coda scrobble", &err))
}

/// Quanto c'è in coda, per un servizio o per tutti.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn conteggi(connection: &Connection, servizio: Option<Servizio>) -> Result<Conteggi, AppError> {
    connection
        .query_row(
            "SELECT COALESCE(SUM(attempts <  ?1), 0),
                    COALESCE(SUM(attempts >= ?1), 0)
               FROM scrobble_queue
              WHERE (?2 IS NULL OR service = ?2)",
            rusqlite::params![MASSIMI_TENTATIVI, servizio.map(Servizio::chiave)],
            |riga| {
                Ok(Conteggi {
                    in_attesa: riga.get(0)?,
                    abbandonati: riga.get(1)?,
                })
            },
        )
        .map_err(|err| db_error("conteggio della coda scrobble", &err))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn libreria() -> Connection {
        let c = crate::db::open_in_memory().expect("database").connection;
        c.execute_batch(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, album_artist,
                                 duration_ms, track_number, file_size, date_added, date_modified,
                                 mb_recording_id)
             VALUES (1, 'C:/m/a.mp3', 'k1', 'Anima e ghiaccio', 'Colle der Fomento',
                     'Odio pieno', 'Colle der Fomento', 245000, 3, 100, 0, 0,
                     'aaaaaaaa-0000-4000-8000-000000000001'),
                    (2, 'C:/m/b.mp3', 'k2', 'Senza titolo', '', '', NULL, 0, NULL, 100, 0, 0, NULL);",
        )
        .expect("brani di prova");
        c
    }

    fn ascolto() -> Ascolto {
        let mut a = Ascolto::nuovo("Colle der Fomento", "Anima e ghiaccio", 1_700_000_000);
        a.album = Some("Odio pieno".to_owned());
        a.durata_ms = Some(245_000);
        a
    }

    #[test]
    fn accodare_due_volte_lo_stesso_ascolto_scrive_una_riga() {
        let c = libreria();
        let a = ascolto();
        assert_eq!(accoda(&c, &[Servizio::ListenBrainz], &a), Ok(1));
        assert_eq!(
            accoda(&c, &[Servizio::ListenBrainz], &a),
            Ok(0),
            "il vincolo di unicità è ciò che rende ripetibile l'accodamento"
        );
        assert_eq!(
            conteggi(&c, None).expect("conteggi"),
            Conteggi {
                in_attesa: 1,
                abbandonati: 0
            }
        );
    }

    #[test]
    fn lo_stesso_ascolto_verso_due_servizi_sono_due_righe() {
        let c = libreria();
        assert_eq!(
            accoda(&c, &[Servizio::ListenBrainz, Servizio::LastFm], &ascolto()),
            Ok(2)
        );
        assert_eq!(
            conteggi(&c, Some(Servizio::LastFm))
                .expect("conteggi")
                .in_attesa,
            1
        );
    }

    #[test]
    fn un_ascolto_senza_artista_non_entra_in_coda() {
        let c = libreria();
        let mut a = ascolto();
        a.artista = "  ".to_owned();
        assert_eq!(
            accoda(&c, &[Servizio::ListenBrainz], &a),
            Ok(0),
            "una riga che i servizi rifiutano per sempre non deve nemmeno entrare"
        );
    }

    #[test]
    fn l_ascolto_di_un_brano_porta_i_tag_e_il_mbid() {
        let c = libreria();
        let a = ascolto_di(&c, 1, 1_700_000_000_500)
            .expect("lettura")
            .expect("il brano c'è");
        assert_eq!(a.artista, "Colle der Fomento");
        assert_eq!(a.album.as_deref(), Some("Odio pieno"));
        assert_eq!(a.durata_ms, Some(245_000));
        assert_eq!(a.numero_traccia, Some(3));
        assert_eq!(
            a.mbid_registrazione.as_deref(),
            Some("aaaaaaaa-0000-4000-8000-000000000001")
        );
        assert_eq!(
            a.quando_s, 1_700_000_000,
            "i millisecondi diventano secondi"
        );

        assert_eq!(ascolto_di(&c, 999, 0).expect("lettura"), None);
    }

    #[test]
    fn i_campi_vuoti_non_diventano_stringhe_vuote() {
        let c = libreria();
        let a = ascolto_di(&c, 2, 0)
            .expect("lettura")
            .expect("il brano c'è");
        assert_eq!(
            a.album, None,
            "un album vuoto mandato come release_name dichiarerebbe che l'album si chiama così"
        );
        assert_eq!(a.durata_ms, None, "durata zero vuol dire sconosciuta");
        assert!(!a.valido(), "senza artista non è mandabile");
    }

    #[test]
    fn la_cronologia_entra_in_blocco_e_salta_i_brani_senza_tag() {
        let c = libreria();
        c.execute_batch(
            "INSERT INTO play_history (track_id, played_at, ms_played, source) VALUES
               (1, 1000, 200000, 'local'),
               (1, 2000, 200000, 'spotify'),
               (2, 3000, 200000, 'spotify');",
        )
        .expect("cronologia");

        let accodati = accoda_cronologia(&c, Servizio::ListenBrainz, None, 0).expect("accodamento");
        assert_eq!(
            accodati, 2,
            "il brano senza artista né titolo resta fuori: i servizi lo rifiuterebbero"
        );
        assert_eq!(
            accoda_cronologia(&c, Servizio::ListenBrainz, None, 0),
            Ok(0),
            "una seconda passata non raddoppia niente"
        );
    }

    #[test]
    fn la_cronologia_si_puo_restringere_alla_provenienza() {
        let c = libreria();
        c.execute_batch(
            "INSERT INTO play_history (track_id, played_at, ms_played, source) VALUES
               (1, 1000, 200000, 'local'),
               (1, 2000, 200000, 'spotify');",
        )
        .expect("cronologia");

        assert_eq!(
            accoda_cronologia(&c, Servizio::ListenBrainz, Some("spotify"), 0),
            Ok(1)
        );
        let voci = da_mandare(&c, Servizio::ListenBrainz, 10).expect("coda");
        assert_eq!(voci.len(), 1);
        assert_eq!(voci[0].ascolto.quando_s, 2, "2000 ms sono 2 secondi");
    }

    #[test]
    fn la_coda_esce_dal_piu_vecchio() {
        let c = libreria();
        for quando in [3_000_000, 1_000_000, 2_000_000] {
            let mut a = ascolto();
            a.quando_s = quando;
            accoda(&c, &[Servizio::ListenBrainz], &a).expect("accodamento");
        }
        let voci = da_mandare(&c, Servizio::ListenBrainz, 10).expect("coda");
        let istanti: Vec<i64> = voci.iter().map(|v| v.ascolto.quando_s).collect();
        assert_eq!(istanti, vec![1_000_000, 2_000_000, 3_000_000]);
    }

    #[test]
    fn un_servizio_non_vede_la_coda_dell_altro() {
        let c = libreria();
        accoda(&c, &[Servizio::LastFm], &ascolto()).expect("accodamento");
        assert!(
            da_mandare(&c, Servizio::ListenBrainz, 10)
                .expect("coda")
                .is_empty()
        );
        assert_eq!(da_mandare(&c, Servizio::LastFm, 10).expect("coda").len(), 1);
    }

    #[test]
    fn quel_che_e_arrivato_esce_dalla_coda() {
        let c = libreria();
        accoda(&c, &[Servizio::ListenBrainz], &ascolto()).expect("accodamento");
        let voci = da_mandare(&c, Servizio::ListenBrainz, 10).expect("coda");
        let id: Vec<i64> = voci.iter().map(|v| v.id).collect();

        assert_eq!(fatte(&c, &id), Ok(1));
        assert_eq!(conteggi(&c, None).expect("conteggi").in_attesa, 0);
        assert_eq!(fatte(&c, &[]), Ok(0));
    }

    #[test]
    fn un_guasto_definitivo_non_consuma_dieci_tentativi() {
        let c = libreria();
        accoda(&c, &[Servizio::ListenBrainz], &ascolto()).expect("accodamento");
        let id: Vec<i64> = da_mandare(&c, Servizio::ListenBrainz, 10)
            .expect("coda")
            .iter()
            .map(|v| v.id)
            .collect();

        fallite(&c, &id, "sessione revocata", true).expect("fallimento");
        assert_eq!(
            conteggi(&c, None).expect("conteggi"),
            Conteggi {
                in_attesa: 0,
                abbandonati: 1
            }
        );
        assert!(
            da_mandare(&c, Servizio::ListenBrainz, 10)
                .expect("coda")
                .is_empty(),
            "un abbandonato non tiene più occupata la testa della coda"
        );
    }

    #[test]
    fn un_guasto_ritentabile_consuma_un_tentativo_alla_volta() {
        let c = libreria();
        accoda(&c, &[Servizio::ListenBrainz], &ascolto()).expect("accodamento");
        let id: Vec<i64> = da_mandare(&c, Servizio::ListenBrainz, 10)
            .expect("coda")
            .iter()
            .map(|v| v.id)
            .collect();

        for atteso in 1..=3 {
            fallite(&c, &id, "il servizio è giù", false).expect("fallimento");
            let voci = da_mandare(&c, Servizio::ListenBrainz, 10).expect("coda");
            assert_eq!(voci.first().map(|v| v.tentativi), Some(atteso));
        }

        for _ in 0..MASSIMI_TENTATIVI {
            fallite(&c, &id, "il servizio è giù", false).expect("fallimento");
        }
        assert_eq!(conteggi(&c, None).expect("conteggi").abbandonati, 1);
    }

    #[test]
    fn gli_abbandonati_si_rimettono_in_fila_o_si_buttano() {
        let c = libreria();
        accoda(&c, &[Servizio::ListenBrainz, Servizio::LastFm], &ascolto()).expect("accodamento");
        let tutti: Vec<i64> = [Servizio::ListenBrainz, Servizio::LastFm]
            .into_iter()
            .flat_map(|s| {
                da_mandare(&c, s, 10)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|v| v.id)
            })
            .collect();
        fallite(&c, &tutti, "guasto", true).expect("fallimento");

        assert_eq!(rimetti_in_fila(&c, Some(Servizio::LastFm)), Ok(1));
        assert_eq!(
            conteggi(&c, None).expect("conteggi"),
            Conteggi {
                in_attesa: 1,
                abbandonati: 1
            }
        );

        assert_eq!(
            dimentica(&c, None, true),
            Ok(1),
            "solo gli abbandonati: quello rimesso in fila resta"
        );
        assert_eq!(conteggi(&c, None).expect("conteggi").in_attesa, 1);

        assert_eq!(dimentica(&c, None, false), Ok(1), "e poi anche gli altri");
    }

    #[test]
    fn la_causa_non_entra_intera() {
        let c = libreria();
        accoda(&c, &[Servizio::ListenBrainz], &ascolto()).expect("accodamento");
        let id: Vec<i64> = da_mandare(&c, Servizio::ListenBrainz, 10)
            .expect("coda")
            .iter()
            .map(|v| v.id)
            .collect();
        fallite(&c, &id, &"x".repeat(5_000), false).expect("fallimento");

        let scritto: String = c
            .query_row(
                "SELECT last_error FROM scrobble_queue WHERE id = ?1",
                [id.first().copied().unwrap_or(0)],
                |r| r.get(0),
            )
            .expect("lettura");
        assert_eq!(scritto.chars().count(), 300);
    }

    #[test]
    fn un_lotto_chiuso_a_meta_non_lascia_niente_di_chiuso() {
        // Il difetto che questa prova impedisce: senza transazione, ogni `DELETE`
        // è una transazione implicita a sé. Una caduta a metà lotto cancellava le
        // righe già passate e lasciava le altre in coda — ma il servizio le aveva
        // ricevute **tutte**, quindi la passata dopo rimandava le superstiti. Il
        // sintomo, dall'altra parte, sono scrobble doppi: l'unico dato che una
        // riscansione non sa rimettere a posto.
        let c = libreria();
        for quando in [1_700_000_000, 1_700_000_300, 1_700_000_600] {
            let mut a = ascolto();
            a.quando_s = quando;
            accoda(&c, &[Servizio::ListenBrainz], &a).expect("accodamento");
        }
        let id: Vec<i64> = da_mandare(&c, Servizio::ListenBrainz, 10)
            .expect("coda")
            .iter()
            .map(|v| v.id)
            .collect();
        assert_eq!(id.len(), 3, "tre righe da chiudere");

        // La seconda cancellazione fallisce: è la caduta a metà lotto, in forma
        // riproducibile.
        let seconda = id.get(1).copied().unwrap_or(0);
        c.execute_batch(&format!(
            "CREATE TRIGGER cade_a_meta BEFORE DELETE ON scrobble_queue
                 WHEN OLD.id = {seconda}
             BEGIN SELECT RAISE(ABORT, 'caduta simulata'); END;"
        ))
        .expect("trigger");

        assert!(fatte(&c, &id).is_err(), "il lotto non si è potuto chiudere");
        assert_eq!(
            conteggi(&c, None).expect("conteggi").in_attesa,
            3,
            "o si chiude tutto il lotto, o non se ne chiude niente: una riga \
             cancellata qui sarebbe uno scrobble mandato due volte"
        );
    }

    #[test]
    fn un_servizio_sconosciuto_non_entra_nella_tabella() {
        let c = libreria();
        let esito = c.execute(
            "INSERT INTO scrobble_queue (service, played_at, artist, title, queued_at)
             VALUES ('libre.fm', 1, 'a', 'b', 0)",
            [],
        );
        assert!(
            esito.is_err(),
            "il CHECK è ciò che impedisce a una coda di smettere di svuotarsi in silenzio"
        );
    }
}
