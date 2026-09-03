//! I brani desiderati: leggerli, e ricordarsi com'è andata.
//!
//! # Il modulo che mancava
//!
//! `desiderati` esiste dalla migrazione 2 e fino a oggi era una tabella in
//! **sola scrittura**: fuori da `legacy/` compariva in due file soli — quello
//! che ci inserisce dentro ([`crate::import_esterno`]) e la sua stessa DDL.
//! Nessuna `SELECT`, nessun comando, nessuna interfaccia. L'importazione da
//! Spotify ci depositava i brani mancanti e terminava, ed è letteralmente il
//! punto in cui il lavoro si fermava.
//!
//! Questo modulo è quella `SELECT`, più i tre modi di chiudere una riga.
//!
//! # Tre esiti e non due
//!
//! `fatto`, `fallito`, `introvabile`. La terza voce è quella che si è tentati di
//! togliere, e va tenuta: un brano che su YouTube **non c'è** e uno che non si è
//! riusciti a prendere **adesso** hanno bisogno di due comportamenti opposti. Se
//! collassano in «fallito», o si ritenta per sempre qualcosa che non arriverà
//! mai, o — molto peggio — si smette di ritentare qualcosa che sarebbe bastato
//! richiedere. Il secondo caso perde musica in silenzio, ed è il difetto che
//! `TrackDownloadResult` nel vecchio albero documentava per esteso.
//!
//! # Chi decide se ritentare
//!
//! Non questo modulo: il catalogo. [`segna_fallito`] riceve la ritentabilità già
//! decisa da [`aether_domain::AppError::is_retryable`] e si limita a contare i
//! tentativi. È la stessa disciplina di `aether_catalogo`, e per la stessa
//! ragione: la ritentabilità dev'essere una proprietà del guasto, non
//! un'opinione del punto in cui lo si scopre.
//!
//! # Il viaggio di ritorno
//!
//! Una riga di `desiderati` porta `playlist_id` e `position` fin dalla
//! migrazione 2. Servono a una cosa sola — rimettere il brano nella playlist da
//! cui mancava, una volta che è arrivato — e per parecchio tempo **nessuno li ha
//! mai riletti**: l'importazione creava la playlist con i soli brani già in
//! libreria, la coda scaricava gli altri, e lì finiva. Una playlist di cinquanta
//! brani di cui se ne avevano dieci restava di dieci per sempre, con gli altri
//! quaranta sul disco e sciolti.
//!
//! [`riconcilia`] è quel viaggio di ritorno. È scritta per poter essere chiamata
//! **quando capita e quante volte capita**: ogni scrittura è un `INSERT OR
//! IGNORE` sulla chiave primaria `(playlist_id, position)` o una `UPDATE`
//! condizionata, quindi due passate di fila non raddoppiano niente e una passata
//! su una libreria che non è cambiata non fa niente.

use std::collections::HashMap;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{Disponibilita, Licenza};
use aether_domain::{BranoEsterno, Fonte};
use rusqlite::Connection;

/// Quante volte si riprova un brano prima di dichiararlo fallito.
///
/// Tre, e i tre non sono i livelli di `aether_catalogo` — quelli girano
/// dentro un solo tentativo. Questi sono passaggi della coda distanti nel tempo,
/// e servono a coprire il caso in cui a essere rotta era la rete e non il video.
pub const MASSIMI_TENTATIVI: u32 = 3;

/// Lo stato di una riga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stato {
    /// Da prendere.
    Attesa,
    /// Preso.
    Fatto,
    /// Non preso, e non si riprova più.
    Fallito,
    /// Nessuna fonte lecita ce l'ha.
    ///
    /// Terminale, e diverso da [`Stato::Fallito`]. **Non** vuol dire «non
    /// esiste»: vuol dire che non esiste in nessun posto da cui sia lecito
    /// prenderlo, il che è un'informazione — è quella che riempie la lista di
    /// cosa resta da comprare. Vedi [`da_comprare`].
    Introvabile,
}

impl Stato {
    /// Come si scrive in tabella.
    #[must_use]
    pub const fn come_testo(self) -> &'static str {
        match self {
            Self::Attesa => "attesa",
            Self::Fatto => "fatto",
            Self::Fallito => "fallito",
            Self::Introvabile => "introvabile",
        }
    }

    /// Come si rilegge. Uno stato sconosciuto vale [`Stato::Fallito`].
    ///
    /// Non `Attesa`: una riga scritta da una versione futura con uno stato che
    /// non conosciamo non va rimessa in coda: la rimetteremmo in coda **ogni
    /// volta**, e la coda non finirebbe mai.
    #[must_use]
    pub fn da_testo(grezzo: &str) -> Self {
        match grezzo {
            "attesa" => Self::Attesa,
            "fatto" => Self::Fatto,
            "introvabile" => Self::Introvabile,
            _ => Self::Fallito,
        }
    }
}

/// Un brano da scaricare, con quel che serve per farlo.
#[derive(Debug, Clone, PartialEq)]
pub struct Desiderato {
    /// La chiave della riga in `desiderati`.
    pub id: i64,
    /// I metadati autorevoli della fonte da cui viene.
    ///
    /// Compreso [`BranoEsterno::fonte_url`], che è il campo su cui la coda
    /// decide se cercare il file o se ce l'ha già, e
    /// [`BranoEsterno::disponibilita`], che è quello su cui decide se andarlo a
    /// prendere affatto.
    pub brano: BranoEsterno,
    /// Da quale fonte.
    pub fonte: Fonte,
    /// Da quale playlist o album veniva, per poterlo dire all'utente.
    pub provenienza: String,
    /// L'identificativo di quel contenitore presso la fonte.
    ///
    /// È la chiave dell'importazione: `provenienza` è un nome, e due playlist
    /// possono chiamarsi uguale. Serve a chi mostra la coda per attaccare il
    /// brano in corso alla riga giusta.
    pub sorgente_id: String,
    /// La posizione nel contenitore d'origine, da 0.
    pub posizione: u32,
    /// Quante volte ci si è già provati.
    pub tentativi: u32,
}

/// Traduce un guasto di SQLite nominando l'operazione.
fn db_error(cosa: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(cosa.to_owned()),
    })
    .with_cause(err.to_string())
}

/// L'orologio, in millisecondi dall'epoca.
fn adesso() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Le colonne lette da [`leggi_da_scaricare`], in un posto solo.
const COLONNE: &str = "id, title, artist, album, album_artist, duration_ms, isrc,
     spotify_track_id, spotify_album_id, cover_url, track_number, disc_number, year,
     source_title, position, download_attempts, fonte_url, source_id, source_service,
     licenza, disponibilita";

/// La condizione che tiene fuori dalla coda ciò che non va scaricato.
///
/// Due clausole, e nessuna delle due è prudenza generica.
///
/// **Il brano c'è già.** Fra l'importazione e il momento in cui la coda arriva a
/// quella riga può essere passata una scansione che ha portato dentro il file —
/// perché l'utente ce l'aveva in una cartella non sorvegliata e l'ha aggiunta, o
/// perché l'ha scaricato un'altra importazione. Riscaricarlo vuol dire un
/// secondo file identico accanto al primo.
///
/// **Una riga sola per brano.** L'identità di una riga è `(track_key,
/// source_id)`: lo stesso brano in due playlist sono **due** righe. Senza questa
/// clausola verrebbero scaricate tutte e due, e siccome
/// [`aether_domain::destinazione::destinazione`] calcola il percorso dai soli
/// metadati, tutte e due finiscono sullo **stesso file** — con due prelievi che
/// ci scrivono dentro insieme, se le due righe capitano nello stesso lotto. Si
/// prende la più vecchia; [`segna_fatto`] chiude le altre.
const SOLO_DA_PRENDERE: &str = "
    w.download_state = ?1 AND w.download_attempts < ?2
    AND NOT EXISTS (SELECT 1 FROM tracks AS t WHERE t.track_key = w.track_key)
    AND w.id = (
        SELECT MIN(g.id) FROM desiderati AS g
         WHERE g.track_key = w.track_key
           AND g.download_state = ?1 AND g.download_attempts < ?2
    )";

/// I brani ancora da prendere, i più vecchi per primi.
///
/// L'ordine è quello d'inserimento (`id`), non uno a caso: l'utente ha importato
/// una playlist e si aspetta di vederla scendere dall'alto. Un ordine arbitrario
/// sembrerebbe un guasto anche funzionando.
///
/// I brani che hanno già esaurito i tentativi non tornano: la condizione sta
/// nella query e non nel chiamante, o basterebbe un chiamante distratto per
/// rimettere in ciclo perpetuo un guasto permanente. Per il resto di quel che
/// non torna, vedi [`SOLO_DA_PRENDERE`].
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn leggi_da_scaricare(
    connection: &Connection,
    limite: usize,
) -> Result<Vec<Desiderato>, AppError> {
    let sql = format!(
        "SELECT {COLONNE} FROM desiderati AS w
         WHERE {SOLO_DA_PRENDERE}
         ORDER BY w.id LIMIT ?3"
    );
    let mut query = connection
        .prepare(&sql)
        .map_err(|err| db_error("lettura dei desiderati", &err))?;

    let righe = query
        .query_map(
            rusqlite::params![
                Stato::Attesa.come_testo(),
                MASSIMI_TENTATIVI,
                i64::try_from(limite).unwrap_or(i64::MAX),
            ],
            riga,
        )
        .map_err(|err| db_error("lettura dei desiderati", &err))?;

    let mut esito = Vec::new();
    for trovato in righe {
        esito.push(trovato.map_err(|err| db_error("lettura dei desiderati", &err))?);
    }
    Ok(esito)
}

/// Ricostruisce un [`Desiderato`] da una riga.
fn riga(riga: &rusqlite::Row<'_>) -> rusqlite::Result<Desiderato> {
    let durata: Option<i64> = riga.get(5)?;
    let numero_traccia: Option<i64> = riga.get(10)?;
    let numero_disco: Option<i64> = riga.get(11)?;
    let anno: Option<i64> = riga.get(12)?;
    let posizione: Option<i64> = riga.get(14)?;
    let tentativi: i64 = riga.get(15)?;

    Ok(Desiderato {
        id: riga.get(0)?,
        brano: BranoEsterno {
            title: riga.get(1)?,
            artist: riga.get(2)?,
            album: riga.get(3)?,
            album_artist: riga.get(4)?,
            // I numeri passano da `try_from`: una colonna con un valore assurdo
            // — scritta a mano, o da una versione con un bug — deve valere
            // «non lo so» e non far cadere la lettura di tutta la coda.
            duration_ms: durata.and_then(|d| u64::try_from(d).ok()),
            disc_number: numero_disco.and_then(|n| u32::try_from(n).ok()),
            track_number: numero_traccia.and_then(|n| u32::try_from(n).ok()),
            year: anno.and_then(|a| i32::try_from(a).ok()),
            isrc: riga.get(6)?,
            spotify_track_id: riga.get(7)?,
            spotify_album_id: riga.get(8)?,
            cover_url: riga.get(9)?,
            fonte_url: riga.get(16)?,
            // Una riga in attesa non ha ancora una licenza: nessuno è andato a
            // chiederla. Quella che c'è, quando c'è, è quella sotto cui il file
            // è stato preso davvero, ed è l'unica traccia che resta del patto
            // per cui quel file sta sul disco.
            licenza: riga
                .get::<_, Option<String>>(19)?
                .as_deref()
                .map_or(Licenza::Sconosciuta, licenza_da_testo),
            disponibilita: Disponibilita::da_testo(&riga.get::<_, String>(20)?),
            ..BranoEsterno::default()
        },
        provenienza: riga.get(13)?,
        posizione: posizione.and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        tentativi: u32::try_from(tentativi).unwrap_or(u32::MAX),
        sorgente_id: riga.get(17)?,
        fonte: Fonte::da_testo(&riga.get::<_, String>(18)?),
    })
}

/// Quanti ne restano da prendere.
///
/// Conta quel che [`leggi_da_scaricare`] restituirà, non le righe in attesa:
/// sono la stessa condizione ([`SOLO_DA_PRENDERE`]) apposta. Contando le righe,
/// un brano presente in due playlist varrebbe due e la barra dell'avanzamento si
/// fermerebbe a un passo dalla fine per sempre — la coda avrebbe fatto tutto e
/// il numero direbbe di no.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn conta_in_attesa(connection: &Connection) -> Result<u32, AppError> {
    let sql = format!("SELECT COUNT(*) FROM desiderati AS w WHERE {SOLO_DA_PRENDERE}");
    let quanti: i64 = connection
        .query_row(
            &sql,
            rusqlite::params![Stato::Attesa.come_testo(), MASSIMI_TENTATIVI],
            |riga| riga.get(0),
        )
        .map_err(|err| db_error("conteggio dei desiderati", &err))?;
    Ok(u32::try_from(quanti).unwrap_or(u32::MAX))
}

/// Segna una riga come presa — e con lei le sorelle.
///
/// # Le sorelle
///
/// Le altre righe che nominano lo **stesso brano** da un'altra importazione. La
/// coda ne scarica una sola ([`SOLO_DA_PRENDERE`]); se le altre restassero in
/// attesa, la prima diventerebbe la più vecchia rimasta al giro dopo e il brano
/// si riscaricherebbe una volta per playlist che lo contiene.
///
/// Ricevono lo stesso percorso e lo stesso video, perché è la verità: quel file
/// è il loro brano tanto quanto è quello della riga scaricata. Ed è anche quel
/// che permette a [`riconcilia`] di rimetterlo in **tutte** le playlist da cui
/// mancava, non solo nella prima.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn segna_fatto(
    connection: &Connection,
    id: i64,
    percorso: &str,
    fonte_url: &str,
    licenza: &Licenza,
) -> Result<(), AppError> {
    let quando = adesso();
    let licenza = licenza.nome();
    let scaricabile = Disponibilita::Scaricabile.nome();
    connection
        .execute(
            "UPDATE desiderati
             SET download_state = ?2, download_path = ?3, fonte_url = ?4,
                 licenza = ?5, disponibilita = ?6,
                 download_error = NULL, updated_at = ?7
             WHERE id = ?1",
            rusqlite::params![
                id,
                Stato::Fatto.come_testo(),
                percorso,
                fonte_url,
                licenza,
                scaricabile,
                quando
            ],
        )
        .map_err(|err| db_error("registrazione di un prelievo riuscito", &err))?;

    connection
        .execute(
            "UPDATE desiderati
             SET download_state = ?2, download_path = ?3, fonte_url = ?4,
                 licenza = ?5, disponibilita = ?6,
                 download_error = NULL, updated_at = ?7
             WHERE id <> ?1
               AND download_state = ?8
               AND track_key = (SELECT track_key FROM desiderati WHERE id = ?1)",
            rusqlite::params![
                id,
                Stato::Fatto.come_testo(),
                percorso,
                fonte_url,
                licenza,
                scaricabile,
                quando,
                Stato::Attesa.come_testo(),
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("chiusura delle righe sorelle", &err))
}

/// Rilegge una licenza dal nome con cui era stata scritta.
///
/// Il verso opposto di [`Licenza::nome`], e sta qui e non nel dominio per una
/// ragione: il dominio non serializza niente da sé, e questa è una lettura di
/// colonna. Tutto ciò che non si riconosce vale «non lo so», che è la variante
/// che **non** permette di copiare — un valore storto in quella colonna deve
/// stringere i permessi, non allargarli.
fn licenza_da_testo(grezzo: &str) -> Licenza {
    match grezzo {
        "pubblicoDominio" => Licenza::PubblicoDominio,
        "oml" => Licenza::OpenMusicLicense,
        "liberaNonCommerciale" => Licenza::LiberaNonCommerciale,
        "tutteRiservate" => Licenza::TutteRiservate,
        altro => altro
            .strip_prefix("cc-")
            .filter(|codice| !codice.is_empty())
            .map_or(Licenza::Sconosciuta, |codice| {
                Licenza::CreativeCommons(codice.to_owned())
            }),
    }
}

/// Segna un tentativo andato male.
///
/// `ritentabile` arriva da [`aether_domain::AppError::is_retryable`], cioè dal
/// catalogo. La riga torna in attesa solo se il guasto lo permette **e** restano
/// tentativi; altrimenti diventa `fallito`. L'errore si conserva in ogni caso:
/// è la sola cosa che poi spiega all'utente perché quel brano non c'è.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn segna_fallito(
    connection: &Connection,
    id: i64,
    errore: &str,
    ritentabile: bool,
) -> Result<(), AppError> {
    // Lo stato si calcola in SQL sul valore aggiornato di `download_attempts`,
    // non su quello letto prima: fra la lettura e la scrittura può esserci
    // passato un altro filo della coda, e decidere su un conteggio vecchio
    // regalerebbe un tentativo in più (o in meno) a seconda del tempismo.
    connection
        .execute(
            "UPDATE desiderati
             SET download_attempts = download_attempts + 1,
                 download_error = ?3,
                 download_state = CASE
                     WHEN ?4 = 1 AND download_attempts + 1 < ?5 THEN ?6
                     ELSE ?2
                 END,
                 updated_at = ?7
             WHERE id = ?1",
            rusqlite::params![
                id,
                Stato::Fallito.come_testo(),
                errore,
                i32::from(ritentabile),
                MASSIMI_TENTATIVI,
                Stato::Attesa.come_testo(),
                adesso(),
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("registrazione di uno scaricamento fallito", &err))
}

/// Segna un brano che su YouTube non esiste.
///
/// Terminale per costruzione: non passa dal conteggio dei tentativi, perché
/// ritentare una ricerca che ha risposto «non c'è» darà «non c'è».
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn segna_introvabile(connection: &Connection, id: i64, motivo: &str) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE desiderati
             SET download_state = ?2, download_error = ?3, updated_at = ?4
             WHERE id = ?1",
            rusqlite::params![id, Stato::Introvabile.come_testo(), motivo, adesso()],
        )
        .map(|_| ())
        .map_err(|err| db_error("registrazione di un brano introvabile", &err))
}

/// Rimette in coda i falliti, azzerandone i tentativi.
///
/// Gli `introvabile` **non** si toccano: è il gesto «riprova quelli che possono
/// riuscire», e rimettere in fila anche i brani che su YouTube non ci sono lo
/// trasformerebbe in «rifai tutto da capo», che è un'altra cosa e dura molto di
/// più.
///
/// Restituisce quanti ne sono tornati in coda.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn riprova_falliti(connection: &Connection) -> Result<u32, AppError> {
    let quanti = connection
        .execute(
            "UPDATE desiderati
             SET download_state = ?1, download_attempts = 0, updated_at = ?3
             WHERE download_state = ?2",
            rusqlite::params![
                Stato::Attesa.come_testo(),
                Stato::Fallito.come_testo(),
                adesso()
            ],
        )
        .map_err(|err| db_error("rimessa in coda dei falliti", &err))?;
    Ok(u32::try_from(quanti).unwrap_or(u32::MAX))
}

// ── il viaggio di ritorno ───────────────────────────────────────────────────

/// Cosa ha rimesso a posto una passata di [`riconcilia`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Riconciliazione {
    /// Quante voci sono tornate nelle loro playlist.
    pub voci_rimesse: usize,
    /// Quante righe si sono chiuse perché il brano ormai in libreria c'è.
    pub righe_chiuse: usize,
}

/// Come si ritrova in `tracks` il brano di una riga di `desiderati`.
///
/// Due strade, e la seconda non è ridondanza.
///
/// La **chiave d'identità** è la strada normale, ed è quella per cui
/// `desiderati.track_key` esiste fin dalla migrazione 2: il file scaricato
/// riceve i tag di Spotify da `tag_scrittura`, la scansione li rilegge e ne
/// ricava la stessa chiave.
///
/// Il **percorso** copre il caso in cui quella catena si spezza. Scrivere i tag
/// può fallire senza che lo scaricamento fallisca — `scarica.rs` lo annota e
/// prosegue apposta, perché il file c'è ed è ascoltabile — e allora in libreria
/// quel brano entra con il titolo di YouTube e una chiave che non combacia con
/// niente. Senza questa seconda strada sarebbe scaricato, in libreria, e
/// invisibile alla playlist che l'aveva chiesto: il guasto più difficile da
/// vedere fra quelli possibili qui.
const STESSO_BRANO: &str = "
    (t.track_key = w.track_key
     OR (w.download_path IS NOT NULL AND t.path = w.download_path))";

/// Rimette in playlist i brani arrivati, e chiude le righe che non servono più.
///
/// `sorgente` restringe a una sola importazione (`source_id`); `None` le guarda
/// tutte. L'importazione passa la propria — quel che è successo alle altre
/// playlist non è affar suo e non deve comparire nel suo rapporto — mentre la
/// coda, che ha appena riscansionato tutto, passa `None`.
///
/// # Perché si può chiamare a vuoto
///
/// Perché ogni riga si considera **una volta sola**: `placed_at` marca quelle
/// già passate di qui, e la `INSERT` guarda solo le altre. Chiamarla due volte
/// di fila dà zero la seconda volta. È voluto: i due chiamanti non si
/// coordinano, e non devono. L'`OR IGNORE` sulla chiave primaria
/// `(playlist_id, position)` resta come seconda rete, ma non è più l'unica:
/// da sola non bastava, perché le posizioni scritte all'importazione sono gli
/// indici sparsi di Spotify mentre togliere o riordinare un brano le
/// ricompatta — e appena uno slot si liberava, un brano che l'utente aveva
/// tolto ci rientrava alla passata dopo.
///
/// # Cosa **non** fa
///
/// Non tocca una posizione già occupata. Se dopo l'importazione l'utente ha
/// riordinato la playlist a mano, `playlists::riscrivi_ordine` ha ricompattato
/// le posizioni e quella del brano in arrivo può essere di qualcun altro: in quel
/// caso la voce non entra — e non riproverà, perché `placed_at` si scrive
/// anche per lei. Perdere l'aggiunta è il male minore rispetto a spostare un
/// brano che l'utente aveva messo lì di proposito.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn riconcilia(
    connection: &Connection,
    sorgente: Option<&str>,
) -> Result<Riconciliazione, AppError> {
    // Prima il ritorno in playlist, poi la chiusura delle righe: l'ordine
    // inverso funzionerebbe lo stesso — nessuna delle due condizioni guarda
    // quel che tocca l'altra — ma così il conteggio delle voci rimesse resta
    // leggibile anche se la seconda fallisce.
    // La clausola comune alle tre scritture qui sotto: le righe che questa
    // passata considera. Deve restare identica nelle tre, o una riga marcata
    // e una riga rimessa smettono di essere la stessa riga.
    let righe_da_considerare = format!(
        "w.playlist_id IS NOT NULL
            AND w.position IS NOT NULL
            AND w.placed_at IS NULL
            AND (?1 IS NULL OR w.source_id = ?1)
            AND EXISTS (SELECT 1 FROM tracks AS t WHERE {STESSO_BRANO})
            AND EXISTS (SELECT 1 FROM playlists AS p
                         WHERE p.id = w.playlist_id AND p.is_smart = 0)"
    );

    let sql_voci = format!(
        "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position)
         SELECT w.playlist_id,
                (SELECT MIN(t.id) FROM tracks AS t WHERE {STESSO_BRANO}),
                w.position
           FROM desiderati AS w
          WHERE {righe_da_considerare}"
    );
    let voci_rimesse = connection
        .execute(&sql_voci, rusqlite::params![sorgente])
        .map_err(|err| db_error("ritorno dei brani nelle playlist", &err))?;

    if voci_rimesse > 0 {
        // Solo le playlist con righe considerate in QUESTA passata: toccare
        // `updated_at` di una playlist non modificata può farle vincere una
        // fusione di sincronia che avrebbe perso, o resuscitarla dopo una
        // cancellazione remota. Va fatto prima della marcatura qui sotto,
        // finché `placed_at` è ancora NULL.
        let sql_playlist = format!(
            "UPDATE playlists SET updated_at = ?2
              WHERE id IN (SELECT DISTINCT w.playlist_id FROM desiderati AS w
                            WHERE {righe_da_considerare})"
        );
        connection
            .execute(&sql_playlist, rusqlite::params![sorgente, adesso()])
            .map_err(|err| db_error("aggiornamento delle playlist riconciliate", &err))?;
    }

    // La marcatura che rende tutto questo irripetibile. Anche per le righe la
    // cui posizione era occupata: la voce non è entrata, e non riproverà.
    let sql_marcatura = format!(
        "UPDATE desiderati SET placed_at = ?2
          WHERE id IN (SELECT w.id FROM desiderati AS w WHERE {righe_da_considerare})"
    );
    connection
        .execute(&sql_marcatura, rusqlite::params![sorgente, adesso()])
        .map_err(|err| db_error("marcatura dei desiderati riconciliati", &err))?;

    // Il marchio d'origine sui file arrivati da un prelievo. La scansione li
    // inserisce con `source = 'scan'` — non sa da dove vengono — ma qui il
    // percorso di scaricamento lo dice. È il valore che `enrich` interroga per
    // sapere quali brani hanno metadati da ricontrollare anche quando i tag
    // sembrano completi: senza questa riga la clausola `t.source = 'catalogo'`
    // non troverebbe mai niente di più recente della migrazione 10.
    connection
        .execute(
            "UPDATE tracks SET source = 'catalogo'
              WHERE source = 'scan'
                AND path IN (SELECT w.download_path FROM desiderati AS w
                              WHERE w.download_path IS NOT NULL)",
            [],
        )
        .map_err(|err| db_error("marchio d'origine dei brani prelevati", &err))?;

    // Senza `format!`, a differenza della query qui sopra: questa non interpola
    // nessuna costante, e un `format!` che non formatta niente fa credere a chi
    // legge che da qualche parte ci sia un pezzo di SQL costruito a mano.
    const SQL_RIGHE: &str = "UPDATE desiderati
            SET download_state = ?2, download_error = NULL, updated_at = ?3
          WHERE download_state = ?4
            AND (?1 IS NULL OR source_id = ?1)
            AND EXISTS (SELECT 1 FROM tracks AS t
                         WHERE t.track_key = desiderati.track_key)";
    let righe_chiuse = connection
        .execute(
            SQL_RIGHE,
            rusqlite::params![
                sorgente,
                Stato::Fatto.come_testo(),
                adesso(),
                Stato::Attesa.come_testo(),
            ],
        )
        .map_err(|err| db_error("chiusura dei desiderati già in libreria", &err))?;

    Ok(Riconciliazione {
        voci_rimesse,
        righe_chiuse,
    })
}

/// Quanti brani ci sono per stato: `(attesa, fatto, fallito, introvabile)`.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn conteggi(connection: &Connection) -> Result<Conteggi, AppError> {
    let mut query = connection
        .prepare("SELECT download_state, COUNT(*) FROM desiderati GROUP BY download_state")
        .map_err(|err| db_error("conteggio dei desiderati", &err))?;
    let righe = query
        .query_map([], |riga| {
            Ok((riga.get::<_, String>(0)?, riga.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("conteggio dei desiderati", &err))?;

    let mut esito = Conteggi::default();
    for trovato in righe {
        let (stato, quanti) = trovato.map_err(|err| db_error("conteggio dei desiderati", &err))?;
        let quanti = u32::try_from(quanti).unwrap_or(u32::MAX);
        match Stato::da_testo(&stato) {
            Stato::Attesa => esito.attesa = quanti,
            Stato::Fatto => esito.fatto = quanti,
            Stato::Fallito => esito.fallito = quanti,
            Stato::Introvabile => esito.introvabile = quanti,
        }
    }
    Ok(esito)
}

/// Quanti desiderati ci sono, per stato.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Conteggi {
    /// Da prendere.
    pub attesa: u32,
    /// Presi.
    pub fatto: u32,
    /// Non presi, e non si riprova.
    pub fallito: u32,
    /// Nessuna fonte lecita ce li ha. Vedi [`Stato::Introvabile`].
    pub introvabile: u32,
}

/// Un'importazione, vista dalla coda.
///
/// # Perché non c'è una tabella delle importazioni
///
/// Perché ce n'è già una. `desiderati` porta `source_kind`, `source_id` e
/// `source_title` su **ogni** riga fin dalla migrazione 2, e nessuna riga si
/// cancella mai: un'importazione *è* il gruppo delle righe con lo stesso
/// `source_id`. Il che vuol dire anche che l'elenco sopravvive alla chiusura
/// dell'applicazione senza che nessuno lo salvi da nessuna parte — cosa che una
/// tabella nuova avrebbe dovuto guadagnarsi con una migrazione.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sorgente {
    /// L'identificativo del contenitore sul servizio da cui viene.
    pub source_id: String,
    /// `brano`, `album`, `playlist` o `artista`.
    pub source_kind: String,
    /// Come si chiama.
    pub source_title: String,
    /// Il nome stabile della [`Fonte`] da cui è stata importata.
    ///
    /// Serve a chi disegna la coda per una ragione sola, e non cosmetica: da un
    /// link di un catalogo il file **non si sceglie** — è quello che l'utente
    /// ha incollato — quindi la riga in volo non ha un candidato da mostrare, e
    /// senza questo campo non saprebbe distinguere «non l'ho ancora scelto» da
    /// «non c'era niente da scegliere».
    pub source_service: String,
    /// Come stanno i suoi brani.
    pub conteggi: Conteggi,
    /// Quando è stata importata.
    pub aggiunta_ms: i64,
    /// L'ultimo movimento su una delle sue righe.
    pub aggiornata_ms: i64,
}

/// Le importazioni, la più recente per prima.
///
/// Una query sola per tutte: `GROUP BY source_id, download_state` dà una riga
/// per coppia, e i quattro conteggi di una sorgente si ripiegano qui. Chiedere
/// i conteggi una sorgente per volta vorrebbe dire *n* interrogazioni sotto il
/// lucchetto della libreria, dentro un ciclo che gira a ogni brano concluso.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn per_sorgente(connection: &Connection) -> Result<Vec<Sorgente>, AppError> {
    let mut query = connection
        .prepare(
            // `source_kind`, `source_title` e `source_service` senza aggregato:
            // dentro un gruppo sono lo stesso valore su tutte le righe. Se un
            // titolo è cambiato sul servizio fra due importazioni, SQLite ne
            // sceglie uno dei due — che è un nome leggermente vecchio, non un
            // numero sbagliato. Nemmeno `source_service` entra nel `GROUP BY`,
            // e per una ragione più forte: un `source_id` viene da un servizio
            // solo, quindi raggrupparci sopra non spezzerebbe niente ma
            // suggerirebbe che possa spezzare.
            "SELECT source_id, source_kind, source_title, source_service, download_state,
                    COUNT(*), MIN(added_at), MAX(COALESCE(updated_at, added_at))
             FROM desiderati
             GROUP BY source_id, download_state",
        )
        .map_err(|err| db_error("elenco delle importazioni", &err))?;
    let righe = query
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                riga.get::<_, String>(1)?,
                riga.get::<_, String>(2)?,
                riga.get::<_, String>(3)?,
                riga.get::<_, String>(4)?,
                riga.get::<_, i64>(5)?,
                riga.get::<_, i64>(6)?,
                riga.get::<_, i64>(7)?,
            ))
        })
        .map_err(|err| db_error("elenco delle importazioni", &err))?;

    let mut per_id: HashMap<String, Sorgente> = HashMap::new();
    for trovato in righe {
        let (id, genere, titolo, servizio, stato, quanti, aggiunta, aggiornata) =
            trovato.map_err(|err| db_error("elenco delle importazioni", &err))?;
        let quanti = u32::try_from(quanti).unwrap_or(u32::MAX);
        let sorgente = per_id.entry(id.clone()).or_insert_with(|| Sorgente {
            source_id: id,
            source_kind: genere,
            source_title: titolo,
            source_service: servizio,
            conteggi: Conteggi::default(),
            aggiunta_ms: aggiunta,
            aggiornata_ms: aggiornata,
        });
        // Somma e non assegnazione: due stati sconosciuti scritti da una
        // versione futura ricadono tutti e due su `Fallito`, e assegnando il
        // secondo cancellerebbe il primo.
        let dove = match Stato::da_testo(&stato) {
            Stato::Attesa => &mut sorgente.conteggi.attesa,
            Stato::Fatto => &mut sorgente.conteggi.fatto,
            Stato::Fallito => &mut sorgente.conteggi.fallito,
            Stato::Introvabile => &mut sorgente.conteggi.introvabile,
        };
        *dove = dove.saturating_add(quanti);
        sorgente.aggiunta_ms = sorgente.aggiunta_ms.min(aggiunta);
        sorgente.aggiornata_ms = sorgente.aggiornata_ms.max(aggiornata);
    }

    let mut esito: Vec<Sorgente> = per_id.into_values().collect();
    // La più recente in cima: è quella che si sta guardando. A parità —
    // due link confermati nello stesso millisecondo — l'identificativo, o
    // l'ordine sarebbe quello di una tabella di hash, cioè diverso a ogni giro.
    esito.sort_by(|a, b| {
        b.aggiunta_ms
            .cmp(&a.aggiunta_ms)
            .then_with(|| a.source_id.cmp(&b.source_id))
    });
    Ok(esito)
}

/// Un brano che nessuna fonte lecita ha, e che quindi si compra.
///
/// # Perché è un tipo suo e non un [`Desiderato`]
///
/// Perché un `Desiderato` porta tutto quel che serve a **prenderlo** — la
/// licenza, la disponibilità, il numero di tentativi, l'indirizzo del file — e
/// qui non si prende niente. Quel che serve è quel che serve a **cercarlo in un
/// negozio**: chi lo canta, come si chiama, e in che disco stava. Portarsi
/// dietro il resto vorrebbe dire suggerire che da questa riga si possa ancora
/// scaricare, che è precisamente la cosa che non si può fare.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaComprare {
    /// Il titolo, come la fonte l'ha nominato.
    pub titolo: String,
    /// Chi lo canta. Vuoto quando la fonte non l'ha dato.
    pub artista: String,
    /// In che disco stava. Vuoto quando la fonte non l'ha dato.
    pub album: String,
    /// Da quale importazione veniva, per dire dove manca.
    pub provenienza: String,
    /// Perché nessuno ce l'ha: il codice del catalogo degli errori.
    ///
    /// Distingue le due ragioni, che sono cose diverse per chi legge:
    /// `download.noResults` è «non l'ho trovato da nessuna parte»,
    /// `download.notPermitted` è «l'ho trovato e la licenza non me lo lascia
    /// prendere». La seconda vuol dire che da qualche parte si **ascolta**.
    pub motivo: String,
}

/// I brani che restano da comprare, il più recente per primo.
///
/// # Perché esiste
///
/// Perché la sostituzione onesta di uno scaricamento che non si può fare non è
/// un errore rosso: è dire **dove** prendere quel brano. Le righe
/// [`Stato::Introvabile`] sono esattamente quelle, e fino a qui restavano un
/// numero in un conteggio — cioè un'informazione che c'era e non usciva.
///
/// Distinte per titolo e artista: lo stesso brano mancante da tre playlist è un
/// brano che non hai, non tre. È la stessa regola di `mancantiAccount` di là, e
/// per la stessa ragione — una lista della spesa con tre volte la stessa riga
/// fa sembrare il problema tre volte più grande.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn da_comprare(connection: &Connection, limite: u32) -> Result<Vec<DaComprare>, AppError> {
    let mut query = connection
        .prepare(
            // `MAX(updated_at)` e non `updated_at`: il gruppo raccoglie le
            // righe dello stesso brano in playlist diverse, e la data che conta
            // è quella dell'ultima volta che si è provato — è quella che mette
            // in cima quel che si è appena scoperto di non avere.
            "SELECT title, COALESCE(artist, ''), COALESCE(album, ''),
                    source_title, COALESCE(download_error, ''),
                    MAX(COALESCE(updated_at, added_at)) AS quando
             FROM desiderati
             WHERE download_state = ?1
             GROUP BY LOWER(title), LOWER(COALESCE(artist, ''))
             ORDER BY quando DESC
             LIMIT ?2",
        )
        .map_err(|err| db_error("elenco di quel che resta da comprare", &err))?;
    let righe = query
        .query_map(
            rusqlite::params![Stato::Introvabile.come_testo(), limite],
            |riga| {
                Ok(DaComprare {
                    titolo: riga.get(0)?,
                    artista: riga.get(1)?,
                    album: riga.get(2)?,
                    provenienza: riga.get(3)?,
                    motivo: riga.get(4)?,
                })
            },
        )
        .map_err(|err| db_error("elenco di quel che resta da comprare", &err))?;

    let mut esito = Vec::new();
    for trovato in righe {
        esito.push(trovato.map_err(|err| db_error("elenco di quel che resta da comprare", &err))?);
    }
    Ok(esito)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un database con lo schema vero, migrazioni comprese.
    fn connessione() -> Connection {
        let connection = Connection::open_in_memory().expect("database in memoria");
        crate::db::MIGRATIONS.iter().for_each(|passo| {
            connection
                .execute_batch(passo.sql)
                .unwrap_or_else(|err| panic!("migrazione {}: {err}", passo.name));
        });
        connection
    }

    fn inserisci_da(
        connection: &Connection,
        titolo: &str,
        sorgente: &str,
        nome_sorgente: &str,
        quando: i64,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO desiderati
                 (track_key, title, artist, album, duration_ms, track_number,
                  source_kind, source_id, source_title, position, added_at)
                 VALUES (?1, ?2, 'Un artista', 'Un album', 200000, 4,
                         'playlist', ?3, ?4, 2, ?5)",
                rusqlite::params![
                    format!("chiave-{titolo}"),
                    titolo,
                    sorgente,
                    nome_sorgente,
                    quando
                ],
            )
            .expect("inserimento");
        connection.last_insert_rowid()
    }

    fn inserisci(connection: &Connection, titolo: &str) -> i64 {
        inserisci_da(connection, titolo, "sorgente", "La playlist", 0)
    }

    /// Una riga di desiderato che sa da quale playlist manca, e da che posto.
    fn inserisci_in_playlist(
        connection: &Connection,
        titolo: &str,
        sorgente: &str,
        playlist_id: i64,
        posizione: i64,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO desiderati
                 (track_key, title, artist, source_kind, source_id, source_title,
                  playlist_id, position, added_at)
                 VALUES (?1, ?2, 'Un artista', 'playlist', ?3, 'La playlist', ?4, ?5, 0)",
                rusqlite::params![
                    format!("chiave-{titolo}"),
                    titolo,
                    sorgente,
                    playlist_id,
                    posizione
                ],
            )
            .expect("inserimento del desiderato");
        connection.last_insert_rowid()
    }

    /// Una playlist vera, vuota.
    fn playlist(connection: &Connection, nome: &str) -> i64 {
        connection
            .execute(
                "INSERT INTO playlists (playlist_key, name, created_at, updated_at, is_smart)
                 VALUES (?1, ?2, 0, 0, 0)",
                rusqlite::params![nome.to_lowercase(), nome],
            )
            .expect("inserimento della playlist");
        connection.last_insert_rowid()
    }

    /// Un brano in libreria, con la chiave che le righe di prova usano.
    fn in_libreria(connection: &Connection, titolo: &str, percorso: &str) -> i64 {
        connection
            .execute(
                "INSERT INTO tracks (path, track_key, title, artist, album, duration_ms,
                                     file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, 'Un artista', 'Un album', 200000, 0, 0, 0)",
                rusqlite::params![percorso, format!("chiave-{titolo}"), titolo],
            )
            .expect("inserimento del brano");
        connection.last_insert_rowid()
    }

    fn voci_di(connection: &Connection, playlist_id: i64) -> Vec<(i64, i64)> {
        let mut query = connection
            .prepare(
                "SELECT position, track_id FROM playlist_tracks
                 WHERE playlist_id = ?1 ORDER BY position",
            )
            .expect("query valida");
        let righe = query
            .query_map([playlist_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query valida");
        righe.filter_map(Result::ok).collect()
    }

    #[test]
    fn un_brano_arrivato_torna_al_suo_posto_nella_playlist() {
        // Il viaggio di ritorno, ed è la ragione per cui questa funzione esiste:
        // prima di lei una playlist di cinquanta brani di cui se ne avevano
        // dieci restava di dieci per sempre.
        let connection = connessione();
        let p = playlist(&connection, "La playlist");
        // Alla posizione 1 c'è già un brano che l'importazione aveva ritrovato.
        let gia_qui = in_libreria(&connection, "Bravo", "C:/M/bravo.m4a");
        connection
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 VALUES (?1, ?2, 1)",
                rusqlite::params![p, gia_qui],
            )
            .expect("voce di playlist");
        // E due che mancavano, ai posti 0 e 2 dell'elenco di Spotify.
        inserisci_in_playlist(&connection, "Alpha", "una", p, 0);
        inserisci_in_playlist(&connection, "Charlie", "una", p, 2);

        // Finché non arrivano, non succede niente.
        assert_eq!(
            riconcilia(&connection, None).expect("riconciliazione"),
            Riconciliazione::default()
        );

        let alpha = in_libreria(&connection, "Alpha", "C:/M/alpha.m4a");
        let charlie = in_libreria(&connection, "Charlie", "C:/M/charlie.m4a");
        let esito = riconcilia(&connection, None).expect("riconciliazione");

        assert_eq!(esito.voci_rimesse, 2);
        assert_eq!(
            voci_di(&connection, p),
            vec![(0, alpha), (1, gia_qui), (2, charlie)],
            "ognuno al posto che aveva su Spotify, non in fondo"
        );
    }

    #[test]
    fn riconciliare_due_volte_non_raddoppia_niente() {
        // I due chiamanti — l'importazione e la coda — non si coordinano, e non
        // devono: la seconda passata deve poter essere a vuoto.
        let connection = connessione();
        let p = playlist(&connection, "La playlist");
        inserisci_in_playlist(&connection, "Alpha", "una", p, 0);
        in_libreria(&connection, "Alpha", "C:/M/alpha.m4a");

        let prima = riconcilia(&connection, None).expect("prima");
        let seconda = riconcilia(&connection, None).expect("seconda");
        assert_eq!(prima.voci_rimesse, 1);
        assert_eq!(seconda.voci_rimesse, 0);
        assert_eq!(seconda.righe_chiuse, 0);
        assert_eq!(voci_di(&connection, p).len(), 1);
    }

    #[test]
    fn un_brano_tolto_dalla_playlist_non_ci_rientra() {
        // Le righe di `desiderati` non si cancellano mai, e le posizioni
        // registrate sono gli indici sparsi di Spotify: quando l'utente toglie
        // un brano e le posizioni si ricompattano, lo slot registrato torna
        // libero — e prima di `placed_at` il brano tolto ci rientrava alla
        // passata dopo, per sempre.
        let connection = connessione();
        let p = playlist(&connection, "La playlist");
        inserisci_in_playlist(&connection, "Alpha", "una", p, 0);
        inserisci_in_playlist(&connection, "Bravo", "una", p, 9);
        let alpha = in_libreria(&connection, "Alpha", "C:/M/alpha.m4a");
        let bravo = in_libreria(&connection, "Bravo", "C:/M/bravo.m4a");

        let prima = riconcilia(&connection, None).expect("prima");
        assert_eq!(prima.voci_rimesse, 2);

        // L'utente toglie Bravo. Qui basta la cancellazione: la ricompattazione
        // di `riscrivi_ordine` renderebbe lo scenario solo più favorevole al
        // vecchio difetto, non meno.
        connection
            .execute(
                "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                rusqlite::params![p, bravo],
            )
            .expect("rimozione");

        let dopo = riconcilia(&connection, None).expect("dopo");
        assert_eq!(dopo.voci_rimesse, 0, "il brano tolto non rientra");
        assert_eq!(voci_di(&connection, p), vec![(0, alpha)]);
    }

    #[test]
    fn il_percorso_ritrova_il_brano_a_cui_i_tag_non_sono_stati_scritti() {
        // `scrivi_tag` può fallire senza far fallire lo scaricamento: il file
        // entra in libreria con il titolo di YouTube, e la chiave non combacia
        // più. Senza la seconda strada quel brano resterebbe fuori dalla sua
        // playlist per sempre, pur essendo sul disco.
        let connection = connessione();
        let p = playlist(&connection, "La playlist");
        let id = inserisci_in_playlist(&connection, "Alpha", "una", p, 0);
        connection
            .execute(
                "UPDATE desiderati SET download_state = 'fatto', download_path = ?2
                 WHERE id = ?1",
                rusqlite::params![id, "C:/M/alpha.m4a"],
            )
            .expect("scaricamento registrato");
        // In libreria è entrato con un altro titolo, quindi con un'altra chiave.
        let vero = in_libreria(&connection, "Alpha (Official Video)", "C:/M/alpha.m4a");

        let esito = riconcilia(&connection, None).expect("riconciliazione");
        assert_eq!(esito.voci_rimesse, 1);
        assert_eq!(voci_di(&connection, p), vec![(0, vero)]);
    }

    #[test]
    fn una_riga_il_cui_brano_e_gia_in_libreria_si_chiude() {
        // L'utente ha messo i file a mano e ha riscansionato. Senza questa
        // chiusura la coda li riscaricherebbe accanto a quelli che ci sono.
        let connection = connessione();
        inserisci_da(&connection, "Alpha", "una", "La prima", 0);
        in_libreria(&connection, "Alpha", "C:/M/alpha.m4a");

        let esito = riconcilia(&connection, None).expect("riconciliazione");
        assert_eq!(esito.righe_chiuse, 1);
        assert_eq!(conta_in_attesa(&connection), Ok(0));
        assert_eq!(conteggi(&connection).expect("conteggi").fatto, 1);
    }

    #[test]
    fn la_sorgente_restringe_a_una_sola_importazione() {
        let connection = connessione();
        inserisci_da(&connection, "Alpha", "una", "La prima", 0);
        inserisci_da(&connection, "Bravo", "due", "La seconda", 0);
        in_libreria(&connection, "Alpha", "C:/M/alpha.m4a");
        in_libreria(&connection, "Bravo", "C:/M/bravo.m4a");

        let esito = riconcilia(&connection, Some("una")).expect("riconciliazione");
        assert_eq!(esito.righe_chiuse, 1, "solo la riga di «una»");
        // Quel che resta si legge dallo **stato**, non dalla coda.
        // `conta_in_attesa` qui direbbe zero, e avrebbe ragione: conta quel che
        // `leggi_da_scaricare` restituirebbe, e `SOLO_DA_PRENDERE` scarta le
        // righe il cui brano è già in libreria — qui lo sono tutte e due. Ma la
        // cosa che questo test vuole provare è un'altra: che la riga di «due»
        // non è stata **chiusa**. Sono due domande diverse, e vanno fatte a due
        // contatori diversi.
        let dopo = conteggi(&connection).expect("conteggi");
        assert_eq!(dopo.fatto, 1, "chiusa solo quella di «una»");
        assert_eq!(dopo.attesa, 1, "quella di «due» è rimasta in attesa");
    }

    #[test]
    fn lo_stesso_brano_in_due_playlist_si_scarica_una_volta_sola() {
        // Due righe, stessa `track_key`, due `source_id`. Senza la
        // deduplicazione la coda le prende tutte e due, e siccome il percorso di
        // destinazione si calcola dai metadati sono due yt-dlp sullo stesso file.
        let connection = connessione();
        let condiviso = |sorgente: &str| {
            connection
                .execute(
                    "INSERT INTO desiderati
                     (track_key, title, artist, source_kind, source_id, source_title, added_at)
                     VALUES ('chiave-Alpha', 'Alpha', 'Un artista', 'playlist', ?1, 'P', 0)",
                    [sorgente],
                )
                .expect("inserimento");
            connection.last_insert_rowid()
        };
        let primo = condiviso("una");
        let secondo = condiviso("due");

        let coda = leggi_da_scaricare(&connection, 10).expect("lettura");
        assert_eq!(coda.len(), 1, "un brano, un solo scaricamento");
        assert_eq!(coda.first().map(|d| d.id), Some(primo));
        assert_eq!(
            conta_in_attesa(&connection),
            Ok(1),
            "e il totale dice quel che la coda farà davvero"
        );

        // Preso: si chiude anche la riga dell'altra importazione.
        segna_fatto(
            &connection,
            primo,
            "C:/M/alpha.m4a",
            "https://archive.org/download/x/t01.flac",
            &Licenza::LiberaNonCommerciale,
        )
        .expect("segna");
        assert_eq!(conta_in_attesa(&connection), Ok(0));
        let stato: String = connection
            .query_row(
                "SELECT download_state FROM desiderati WHERE id = ?1",
                [secondo],
                |r| r.get(0),
            )
            .expect("lettura");
        assert_eq!(stato, "fatto", "la sorella si chiude con la riga scaricata");
    }

    #[test]
    fn un_brano_gia_in_libreria_non_entra_nella_coda() {
        let connection = connessione();
        inserisci(&connection, "Alpha");
        assert_eq!(conta_in_attesa(&connection), Ok(1));

        in_libreria(&connection, "Alpha", "C:/M/alpha.m4a");
        assert!(
            leggi_da_scaricare(&connection, 10)
                .expect("lettura")
                .is_empty(),
            "il file c'è già: riscaricarlo ne farebbe un secondo identico"
        );
        assert_eq!(conta_in_attesa(&connection), Ok(0));
    }

    #[test]
    fn la_tabella_finalmente_si_legge() {
        // La prova che il modulo esiste per esistere: prima di oggi non c'era
        // nessuna riga di codice che facesse questo.
        let connection = connessione();
        inserisci(&connection, "Primo");
        inserisci(&connection, "Secondo");

        let coda = leggi_da_scaricare(&connection, 10).expect("lettura");
        assert_eq!(coda.len(), 2);
        let primo = coda.first().expect("c'è il primo");
        assert_eq!(primo.brano.title, "Primo");
        assert_eq!(primo.brano.artist.as_deref(), Some("Un artista"));
        assert_eq!(primo.brano.duration_ms, Some(200_000));
        assert_eq!(primo.brano.track_number, Some(4));
        assert_eq!(primo.provenienza, "La playlist");
        assert_eq!(primo.sorgente_id, "sorgente");
        assert_eq!(primo.posizione, 2);
        assert_eq!(primo.tentativi, 0);
    }

    #[test]
    fn le_importazioni_si_raggruppano_per_sorgente() {
        // È tutto ciò che serve perché l'interfaccia possa dire «questa
        // playlist è a 1 su 2 e quest'altra è finita» invece di un solo numero
        // per tutta la coda: senza, due importazioni insieme sono
        // indistinguibili.
        let connection = connessione();
        let preso = inserisci_da(&connection, "Preso", "una", "La prima", 10);
        inserisci_da(&connection, "Atteso", "una", "La prima", 10);
        let perso = inserisci_da(&connection, "Perso", "due", "La seconda", 20);
        segna_fatto(
            &connection,
            preso,
            "C:/M/a.m4a",
            "https://archive.org/download/x/t01.flac",
            &Licenza::LiberaNonCommerciale,
        )
        .expect("segna");
        segna_introvabile(&connection, perso, "DL_NO_RESULTS").expect("segna");

        let elenco = per_sorgente(&connection).expect("elenco");
        assert_eq!(elenco.len(), 2);

        // La più recente in cima: è quella che si sta guardando.
        let prima = elenco.first().expect("c'è la prima");
        assert_eq!(prima.source_id, "due");
        assert_eq!(prima.source_title, "La seconda");
        assert_eq!(prima.source_kind, "playlist");
        assert_eq!(prima.conteggi.introvabile, 1);
        assert_eq!(prima.conteggi.attesa, 0);

        let seconda = elenco.get(1).expect("c'è la seconda");
        assert_eq!(seconda.source_id, "una");
        assert_eq!(
            seconda.conteggi,
            Conteggi {
                attesa: 1,
                fatto: 1,
                fallito: 0,
                introvabile: 0
            }
        );
        assert_eq!(seconda.aggiunta_ms, 10);
        // L'ultimo movimento e non l'inserimento: `segna_fatto` ha scritto ora.
        assert!(seconda.aggiornata_ms > seconda.aggiunta_ms);
    }

    #[test]
    fn di_serie_un_desiderato_e_in_attesa() {
        // La migrazione aggiunge la colonna alle righe che c'erano già: chi
        // aveva importato prima di questa versione deve ritrovarsi la sua coda,
        // non una tabella di brani in uno stato indefinito.
        let connection = connessione();
        inserisci(&connection, "Vecchio");
        assert_eq!(conta_in_attesa(&connection), Ok(1));
    }

    #[test]
    fn un_brano_preso_esce_dalla_coda() {
        let connection = connessione();
        let id = inserisci(&connection, "Preso");
        segna_fatto(
            &connection,
            id,
            "C:/M/a.m4a",
            "https://archive.org/download/x/t01.flac",
            &Licenza::LiberaNonCommerciale,
        )
        .expect("segna");

        assert_eq!(conta_in_attesa(&connection), Ok(0));
        assert!(
            leggi_da_scaricare(&connection, 10)
                .expect("lettura")
                .is_empty()
        );
        assert_eq!(conteggi(&connection).expect("conteggi").fatto, 1);
    }

    #[test]
    fn un_guasto_passeggero_torna_in_coda() {
        let connection = connessione();
        let id = inserisci(&connection, "Rete");
        segna_fallito(&connection, id, "DL_NETWORK", true).expect("segna");

        let coda = leggi_da_scaricare(&connection, 10).expect("lettura");
        assert_eq!(coda.len(), 1);
        assert_eq!(coda.first().map(|d| d.tentativi), Some(1));
    }

    #[test]
    fn un_guasto_permanente_non_torna_in_coda() {
        let connection = connessione();
        let id = inserisci(&connection, "Privato");
        segna_fallito(&connection, id, "DL_PRIVATE", false).expect("segna");

        assert_eq!(conta_in_attesa(&connection), Ok(0));
        assert_eq!(conteggi(&connection).expect("conteggi").fallito, 1);
    }

    #[test]
    fn i_tentativi_finiscono() {
        // Senza questo freno, una rete che non va rimetterebbe lo stesso brano
        // in coda per sempre e la coda non finirebbe mai.
        let connection = connessione();
        let id = inserisci(&connection, "Ostinato");
        for _ in 0..MASSIMI_TENTATIVI {
            segna_fallito(&connection, id, "DL_NETWORK", true).expect("segna");
        }
        assert_eq!(conta_in_attesa(&connection), Ok(0));
        assert_eq!(conteggi(&connection).expect("conteggi").fallito, 1);
    }

    #[test]
    fn introvabile_e_terminale_subito() {
        // Non passa dai tentativi: una ricerca che ha risposto «non c'è»
        // risponderà «non c'è» anche fra due minuti.
        let connection = connessione();
        let id = inserisci(&connection, "Inesistente");
        segna_introvabile(&connection, id, "DL_NO_RESULTS").expect("segna");

        assert_eq!(conta_in_attesa(&connection), Ok(0));
        assert_eq!(conteggi(&connection).expect("conteggi").introvabile, 1);
    }

    #[test]
    fn riprovare_i_falliti_non_tocca_gli_introvabili() {
        // La distinzione che giustifica due stati invece di uno: «riprova» deve
        // riprovare ciò che può riuscire, non rifare tutto.
        let connection = connessione();
        let fallito = inserisci(&connection, "Fallito");
        let introvabile = inserisci(&connection, "Introvabile");
        segna_fallito(&connection, fallito, "DL_NETWORK", false).expect("segna");
        segna_introvabile(&connection, introvabile, "DL_NO_RESULTS").expect("segna");

        assert_eq!(riprova_falliti(&connection), Ok(1));
        let coda = leggi_da_scaricare(&connection, 10).expect("lettura");
        assert_eq!(coda.len(), 1);
        assert_eq!(
            coda.first().map(|d| d.brano.title.as_str()),
            Some("Fallito")
        );
        // …e i tentativi ripartono da zero, o il ritentativo si esaurirebbe
        // subito contro il freno che aveva appena fermato quel brano.
        assert_eq!(coda.first().map(|d| d.tentativi), Some(0));
    }

    #[test]
    fn uno_stato_sconosciuto_non_torna_in_coda() {
        // Una riga scritta da una versione futura. Trattarla come «in attesa»
        // vorrebbe dire riprovarla a ogni avvio, per sempre.
        assert_eq!(Stato::da_testo("qualcosa_di_nuovo"), Stato::Fallito);
        assert_eq!(Stato::da_testo("attesa"), Stato::Attesa);
        assert_eq!(Stato::da_testo("fatto"), Stato::Fatto);
        assert_eq!(Stato::da_testo("introvabile"), Stato::Introvabile);
    }

    #[test]
    fn il_limite_e_lordine_si_rispettano() {
        let connection = connessione();
        for nome in ["A", "B", "C"] {
            inserisci(&connection, nome);
        }
        let coda = leggi_da_scaricare(&connection, 2).expect("lettura");
        assert_eq!(
            coda.iter()
                .map(|d| d.brano.title.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"],
            "la coda deve scendere nell'ordine in cui l'utente l'ha importata"
        );
    }
}
