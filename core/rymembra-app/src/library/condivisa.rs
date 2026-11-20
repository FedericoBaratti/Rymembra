//! La libreria come la vedono i dispositivi collegati via internet.
//!
//! # Solo i file
//!
//! Si condividono i brani che sono **file** in una cartella sorvegliata,
//! locale o di rete: è la musica di questo dispositivo, e la si può leggere a
//! pezzi. Non si condividono i brani delle cartelle in cloud (`nuvola://…`),
//! che l'altro può collegare da sé con il suo account e che passerebbero due
//! volte per internet, né quelli di catalogo, che un file non ce l'hanno e
//! che l'altro trova nel catalogo da sé. Un album o un artista c'è se ha
//! almeno un brano condiviso.
//!
//! # Mai ricondivise
//!
//! Le righe specchio (`lontano://…`, vedi [`crate::specchio`]) sono la musica
//! di **un altro** dispositivo, e non si condividono: un terzo dispositivo che
//! le vedesse attraverso questo avrebbe catene, doppioni e ascolti contati al
//! proprietario sbagliato. Lo dice [`CONDIVISO`], che vale per **ogni**
//! domanda, vecchia o nuova.
//!
//! # Il catalogo
//!
//! Per la libreria unica un dispositivo chiede il catalogo intero all'altro,
//! una volta e poi per differenze. [`riassunto_catalogo`] divide le righe
//! condivise in **secchi di 500 id** (`id / 500`, quindi stabili: una riga
//! tolta o aggiunta in mezzo non sposta le altre) e dà l'impronta di ognuno;
//! [`pagina_catalogo`] ne serve le righe, [`BranoCatalogo`]. Chi chiede
//! rilegge solo i secchi la cui impronta è cambiata, e un brano ascoltato
//! cambia un secchio solo. Il riassunto si ricorda per generazione, come gli
//! elenchi di Sfoglia (vedi [`Ricordate`]).
//!
//! # Niente percorsi
//!
//! [`BranoRemoto`] è un [`super::TrackSummary`] senza il percorso: all'altro
//! non serve (legge per id), e un percorso dice di una macchina più di quel
//! che serve — il nome dell'utente, le cartelle, i dischi. Al loro posto
//! l'estensione (per il decodificatore), la dimensione, la versione e, per i
//! brani d'immagine CD, il pezzo da suonare.
//!
//! Lo stesso per gli album: la chiave di un disco senza «artista dell'album»
//! porta dentro la cartella in cui sta (`moon safari c:/users/…/moon safari`),
//! e all'altro arriva al suo posto un'impronta ([`impronta_album`]), stabile
//! finché non cambia la chiave, che solo chi serve sa ritradurre.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use rusqlite::OptionalExtension as _;

use super::interrogazioni::{COLONNE_ALBUM, album_from_row};
use super::*;

/// La condizione dei brani condivisi, per `tracks` con l'alias `t`. Il cloud
/// per intervallo di prefisso, come in `enrich`: `'/' + 1` è `'0'`.
///
/// Le righe specchio (`lontano://`) ne stanno fuori: vedi il `//!`.
///
/// E le righe che non stanno **adesso** sotto una cartella sorvegliata
/// ([`SOTTO_LE_RADICI`]): una cartella tolta dall'elenco lascia in libreria le
/// sue righe — la scansione le considera fuori competenza, non sparite, e non le
/// toglie mai — e senza questa riga restavano elencate, valutabili e con il testo
/// leggibile per sempre (S1-8). L'elenco si legge nella stessa istruzione, da
/// `settings`: una cartella tolta esce alla domanda dopo, non alla scansione dopo.
pub(crate) const CONDIVISO: &str = "t.path IS NOT NULL AND t.source_service IS NULL \
     AND NOT (t.path >= 'nuvola://' AND t.path < 'nuvola:/0') \
     AND NOT (t.path >= 'lontano://' AND t.path < 'lontano:/0') \
     AND rym_sotto_le_radici(t.path, (SELECT value FROM settings WHERE key = 'library.roots'))";

/// La stessa per `tracks` senza alias, dove l'espressione dell'artista deve
/// restare quella dell'indice.
const CONDIVISO_SENZA_ALIAS: &str = "path IS NOT NULL AND source_service IS NULL \
     AND NOT (path >= 'nuvola://' AND path < 'nuvola:/0') \
     AND NOT (path >= 'lontano://' AND path < 'lontano:/0') \
     AND rym_sotto_le_radici(path, (SELECT value FROM settings WHERE key = 'library.roots'))";

/// La funzione SQL di [`CONDIVISO`]: il percorso sta sotto una delle cartelle
/// dell'elenco (il JSON di `library.roots`)?
///
/// Quel che [`file_del_brano`] già chiedeva a Rust, con la stessa regola
/// (`rymembra_domain::paths::is_under`: grafia delle barre e delle maiuscole di
/// questa piattaforma, confine di separatore) e dentro SQL, perché valga per
/// **ogni** domanda. Le cartelle del cloud (`nuvola://`) non contano: i loro file
/// non sono di qui. Nessuna cartella — un elenco che manca, o che non si legge —
/// vale che niente è condiviso.
pub(crate) const SOTTO_LE_RADICI: &str = "rym_sotto_le_radici";

/// Registra [`SOTTO_LE_RADICI`] su questa connessione, se manca: la stessa
/// tecnica di `interrogazioni::assicura_ordine` (si prova con una `SELECT`
/// preparata una volta sola, perché ridefinire una funzione scade tutte le
/// istruzioni preparate). **Ogni** domanda che passa da [`CONDIVISO`] la chiama
/// per prima.
///
/// La funzione ricorda l'ultimo elenco che ha visto — le cartelle già normalizzate
/// con `path_key` — e lo rifà solo se cambia: dentro una scansione dell'indice è
/// lo stesso per tutte le righe, e confrontarlo costa un confronto di byte. Una riga
/// che comincia come una cartella scritta così com'è, con un separatore dopo,
/// non passa dalla normalizzazione: è la scorciatoia per il caso comune, ed è
/// sempre un sottoinsieme di quel che `is_under` risponde.
///
/// # Errori
///
/// `db.queryFailed` se SQLite non accetta la funzione.
pub(crate) fn assicura_le_radici(connection: &Connection) -> Result<(), AppError> {
    /// I due separatori di percorso: lo schema `/` e quello di Windows.
    const SEPARATORI: [char; 2] = ['/', '\\'];
    use rusqlite::functions::FunctionFlags;
    use rusqlite::types::ValueRef;
    use rymembra_domain::paths::{key_is_under, path_key};
    if connection
        .prepare_cached(&format!("SELECT {SOTTO_LE_RADICI}('', NULL)"))
        .is_ok()
    {
        return Ok(());
    }
    let regole = PathRules::for_current_platform();
    // L'elenco visto per ultimo, com'era scritto, e di ogni cartella la grafia
    // senza i separatori in coda e la chiave normalizzata.
    type Cartelle = Vec<(String, String)>;
    let ricordo: std::cell::RefCell<Option<(Vec<u8>, Cartelle)>> = std::cell::RefCell::new(None);
    connection
        .create_scalar_function(
            SOTTO_LE_RADICI,
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            move |contesto| {
                let (ValueRef::Text(percorso), ValueRef::Text(elenco)) =
                    (contesto.get_raw(0), contesto.get_raw(1))
                else {
                    return Ok(false);
                };
                let mut visto = ricordo.borrow_mut();
                if visto.as_ref().is_none_or(|(prima, _)| prima != elenco) {
                    let cartelle: Vec<String> = serde_json::from_slice(elenco).unwrap_or_default();
                    let radici = cartelle
                        .into_iter()
                        .filter(|r| !r.starts_with("nuvola://"))
                        .filter_map(|r| {
                            let chiave = path_key(&r, regole);
                            (!chiave.is_empty())
                                .then(|| (r.trim_end_matches(SEPARATORI).to_owned(), chiave))
                        })
                        .collect();
                    *visto = Some((elenco.to_vec(), radici));
                }
                let radici = visto
                    .as_ref()
                    .map_or(&[][..], |(_, radici)| radici.as_slice());
                let percorso = String::from_utf8_lossy(percorso);
                let scorciatoia = radici.iter().any(|(grezza, _)| {
                    !grezza.is_empty()
                        && percorso
                            .strip_prefix(grezza.as_str())
                            .is_some_and(|resto| resto.starts_with(SEPARATORI))
                });
                if scorciatoia {
                    return Ok(true);
                }
                let figlio = path_key(&percorso, regole);
                Ok(radici
                    .iter()
                    .any(|(_, chiave)| key_is_under(&figlio, chiave)))
            },
        )
        .map_err(|err| db_error("cartelle sorvegliate di un brano condiviso", &err))
}

/// Il segno di quel che si ricorda: la generazione della libreria **e** le
/// cartelle sorvegliate.
///
/// Togliere una cartella non scrive su `tracks`, quindi non cambia la
/// generazione, e un elenco ricordato sotto la generazione di prima servirebbe
/// ancora le righe di una cartella che non è più condivisa. Le cartelle nella
/// chiave fanno ricalcolare quel che le guarda. Restituisce anche la generazione
/// da sola, che è quella che il catalogo dice a chi lo legge: letta **prima**,
/// come in `Ricordate`.
fn segno(connection: &Connection) -> Result<(i64, i64), AppError> {
    let generazione = generazione(connection)?;
    let radici = crate::settings::read(connection, crate::settings::CHIAVE_CARTELLE)?;
    let mut impronta = blake3::Hasher::new();
    impronta.update(&generazione.to_le_bytes());
    impronta.update(radici.as_deref().unwrap_or_default().as_bytes());
    let mut otto = [0_u8; 8];
    otto.copy_from_slice(&impronta.finalize().as_bytes()[..8]);
    Ok((generazione, i64::from_le_bytes(otto)))
}

/// Le colonne di un album come le vede un altro dispositivo: `total_tracks` conta
/// i brani **condivisi**, non tutte le righe dell'album.
///
/// Una riga specchio può avere la chiave d'album di un disco di qui (D4 del
/// collaudo del 18/11: lo stesso disco su due dispositivi è una scheda sola), e
/// `albums.total_tracks` le conta: il conto di un disco di qui, mandato a chi lo
/// chiede, includerebbe brani di un terzo dispositivo — e dire a un pari quanti
/// brani ha un disco, contandoli anche da un altro, è ricondividere.
fn colonne_album_condivise() -> String {
    COLONNE_ALBUM.replace(
        "a.total_tracks",
        &format!("(SELECT COUNT(*) FROM tracks t WHERE t.album_key = a.album_key AND {CONDIVISO})"),
    )
}

/// Il pezzo di un'immagine CD da suonare, in fotogrammi da 1/75 di secondo
/// dall'inizio del file. Vedi la migrazione 034.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segmento {
    /// Da dove.
    pub inizio_cd: i64,
    /// Fin dove; `None` per l'ultimo brano, che finisce col file.
    pub fine_cd: Option<i64>,
}

/// Un brano di un altro dispositivo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranoRemoto {
    /// L'id nella libreria **dell'altro**.
    pub id: i64,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// L'impronta dell'album ([`impronta_album`]): da ripetere all'altro.
    pub album_key: Option<String>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// L'anno.
    pub year: Option<i64>,
    /// L'impronta della copertina, da chiedere all'altro.
    pub cover_art_hash: Option<String>,
    /// Quante volte l'ha ascoltato il proprietario.
    pub play_count: i64,
    /// È fra i suoi preferiti.
    pub liked: bool,
    /// Il suo voto.
    pub rating: i64,
    /// L'estensione del file da leggere, minuscola e senza punto.
    pub estensione: String,
    /// Quanti byte.
    pub dimensione: i64,
    /// Cambia se il file cambia: dimensione e data.
    pub versione: String,
    /// Per un brano d'immagine CD, il pezzo.
    pub segmento: Option<Segmento>,
}

/// Il file di un brano condiviso, per leggerlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCondiviso {
    /// Dove sta. Per un brano d'immagine, l'immagine.
    pub percorso: String,
    /// Quanti byte, secondo la libreria.
    pub dimensione: i64,
    /// L'ultima modifica, ms, secondo la libreria.
    pub modificato_ms: i64,
}

/// L'impronta di una chiave d'album, per l'altro: trentadue cifre di BLAKE3.
#[must_use]
pub fn impronta_album(album_key: &str) -> String {
    let hash = blake3::hash(album_key.as_bytes());
    hash.to_hex().chars().take(32).collect()
}

/// La chiave d'album di un'impronta, fra gli album con brani condivisi.
fn chiave_dell_impronta(
    connection: &Connection,
    impronta: &str,
) -> Result<Option<String>, AppError> {
    let cosa = "album di un'impronta";
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT DISTINCT t.album_key FROM tracks t
             WHERE t.album_key IS NOT NULL AND {CONDIVISO}"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    let chiavi = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| db_error(cosa, &err))?;
    for chiave in chiavi {
        let chiave = chiave.map_err(|err| db_error(cosa, &err))?;
        if impronta_album(&chiave) == impronta {
            return Ok(Some(chiave));
        }
    }
    Ok(None)
}

/// La versione di un file, come la dice [`BranoRemoto::versione`]. Chi serve
/// la ricalcola dal disco a ogni lettura.
#[must_use]
pub fn versione(dimensione: i64, modificato_ms: i64) -> String {
    format!("{dimensione}-{modificato_ms}")
}

const COLONNE_REMOTO: &str = "t.id, t.title, t.artist, t.album, t.album_key, t.track_number,
     t.disc_number, t.duration_ms, t.year, t.cover_art_hash, t.play_count, t.liked, t.rating,
     COALESCE(t.immagine, t.path), t.file_size, t.date_modified, t.inizio_cd, t.fine_cd";

fn brano_da_riga(row: &Row<'_>) -> rusqlite::Result<BranoRemoto> {
    let percorso: String = row.get(13)?;
    let dimensione: i64 = row.get(14)?;
    let modificato: i64 = row.get(15)?;
    let inizio: Option<i64> = row.get(16)?;
    Ok(BranoRemoto {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        album_key: row.get::<_, Option<String>>(4)?.map(|k| impronta_album(&k)),
        track_number: row.get(5)?,
        disc_number: row.get(6)?,
        duration_ms: row.get(7)?,
        year: row.get(8)?,
        cover_art_hash: row.get(9)?,
        play_count: row.get(10)?,
        liked: row.get::<_, i64>(11)? != 0,
        rating: row.get(12)?,
        estensione: estensione(&percorso),
        dimensione,
        versione: versione(dimensione, modificato),
        segmento: inizio.map(|inizio_cd| Segmento {
            inizio_cd,
            fine_cd: row.get(17).ok().flatten(),
        }),
    })
}

/// L'estensione, senza dire altro del percorso. Con `/` e `\` tutti e due:
/// il percorso è di questa macchina, ma una cartella di rete li mescola.
fn estensione(percorso: &str) -> String {
    let nome = percorso.rsplit(['/', '\\']).next().unwrap_or("");
    nome.rsplit_once('.')
        .map(|(_, est)| est.to_ascii_lowercase())
        .filter(|est| est.len() <= 8 && est.bytes().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_default()
}

fn righe_di_brani(
    connection: &Connection,
    sql: &str,
    parametri: impl rusqlite::Params,
    cosa: &str,
) -> Result<Vec<BranoRemoto>, AppError> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|err| db_error(cosa, &err))?;
    let rows = statement
        .query_map(parametri, brano_da_riga)
        .map_err(|err| db_error(cosa, &err))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

fn righe_di_album(
    connection: &Connection,
    sql: &str,
    parametri: impl rusqlite::Params,
    cosa: &str,
) -> Result<Vec<AlbumSummary>, AppError> {
    let mut statement = connection
        .prepare_cached(sql)
        .map_err(|err| db_error(cosa, &err))?;
    let rows = statement
        .query_map(parametri, album_from_row)
        .map_err(|err| db_error(cosa, &err))?;
    let mut album = rows
        .collect::<Result<Vec<AlbumSummary>, _>>()
        .map_err(|err| db_error(cosa, &err))?;
    for disco in &mut album {
        disco.album_key = impronta_album(&disco.album_key);
    }
    Ok(album)
}

/// Quel che Sfoglia ricorda fra una pagina e l'altra, a nome della generazione
/// della libreria in cui l'ha calcolato: vedi [`brani`] e [`artisti`].
///
/// # Perché
///
/// Un altro dispositivo che sfoglia questa libreria chiede una pagina dopo
/// l'altra, e due pagine costavano quanto tutta la libreria, col lucchetto in
/// mano sul computer che magari sta suonando (R21-7). I brani: il filtro dei
/// brani condivisi non sta nell'indice, e una pagina a offset 29 000 faceva
/// leggere a SQLite le 29 000 righe che la precedono — 150 ms a pagina.
/// Gli artisti: ogni pagina rifaceva il conto di tutti (30 ms) per
/// restituirne una fetta. Qui si ricorda l'elenco intero, una volta.
///
/// # Perché non può restare indietro
///
/// La chiave è la generazione della libreria ([`generazione`]): la cambiano i
/// trigger della migrazione 030 a ogni scrittura su `tracks`, da qualunque
/// modulo e da qualunque connessione, e i filtri e gli ordinamenti di qui
/// leggono soltanto colonne di `tracks`. Si legge **prima** di calcolare, non
/// dopo: una scrittura nel mezzo lascia l'elenco sotto un segno che nessuno
/// rivedrà, e costa al massimo un calcolo in più. È lo schema di
/// `smart::conteggi_ricordati`, e come lui sta nel processo e non nel database:
/// è una scorciatoia, e una che sopravvive al riavvio è un dato in più da tenere
/// d'accordo.
#[derive(Default)]
pub(super) struct Ricordate {
    /// Gli id dei brani condivisi, nell'ordine di un ordinamento.
    brani: HashMap<(i64, &'static str), Arc<Vec<i64>>>,
    /// Gli artisti condivisi, tutti, senza le copertine.
    artisti: HashMap<i64, Arc<Vec<ArtistSummary>>>,
    /// Il riassunto del catalogo condiviso: vedi [`riassunto_catalogo`].
    catalogo: HashMap<i64, Arc<RiassuntoCatalogo>>,
    /// Quante volte si è calcolato un elenco intero: lo guardano le prove.
    #[cfg(test)]
    costruiti: usize,
}

#[cfg(test)]
impl Ricordate {
    /// Quante volte si è calcolato un elenco intero.
    pub(super) fn costruiti(&self) -> usize {
        self.costruiti
    }

    /// Quanti riassunti del catalogo si tengono.
    pub(super) fn catalogo_ricordati(&self) -> usize {
        self.catalogo.len()
    }
}

/// Le cose ricordate dal processo: una libreria sola è aperta, e le sue pagine
/// le chiedono a turno tutti i dispositivi collegati.
fn ricordate() -> &'static Mutex<Ricordate> {
    static RICORDATE: LazyLock<Mutex<Ricordate>> =
        LazyLock::new(|| Mutex::new(Ricordate::default()));
    &RICORDATE
}

/// La cache, anche se un filo è caduto tenendola: dentro ci sono mappe intere, e
/// ogni voce porta la sua generazione.
fn presa(cache: &Mutex<Ricordate>) -> MutexGuard<'_, Ricordate> {
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Una pagina di brani condivisi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brani(
    connection: &Connection,
    ordine: TrackOrder,
    da: i64,
    quanti: i64,
) -> Result<Vec<BranoRemoto>, AppError> {
    brani_con_scarto(
        connection,
        ricordate(),
        ordine,
        da,
        quanti,
        SCARTO_DA_RICORDARE,
    )
}

/// Da quanti brani prima di una pagina conviene ricordare l'elenco degli id
/// dell'ordinamento: sotto, l'`OFFSET` di SQLite costa pochi millisecondi — la
/// cima si serve com'era, e non paga un elenco intero a chi guarda solo la
/// prima pagina.
const SCARTO_DA_RICORDARE: i64 = 2_000;

/// Quanti elenchi di id si tengono: quattro ordinamenti, per due generazioni.
/// Otto numeri da otto byte per brano: cento mila brani sono sei megabyte al
/// massimo, e oltre si ricomincia da vuoti invece di tenere ogni generazione
/// che una sessione ha visto.
const ELENCHI_RICORDATI: usize = 8;

/// Quanti elenchi di artisti si tengono: la generazione di adesso, e una.
const ARTISTI_RICORDATI: usize = 2;

/// [`brani`] con la sua cache e il suo scarto: per le prove.
///
/// Dallo `scarto` in poi (o quando l'elenco di questo ordinamento c'è già, per
/// questa generazione) la pagina è una fetta degli id ricordati, riletta per id.
/// Se le righe lette non sono quante gli id — una scrittura fra l'elenco e la
/// lettura — si ripiega sulla pagina diretta: una pagina corta direbbe all'altro
/// che non ce n'è altre.
pub(super) fn brani_con_scarto(
    connection: &Connection,
    cache: &Mutex<Ricordate>,
    ordine: TrackOrder,
    da: i64,
    quanti: i64,
    scarto: i64,
) -> Result<Vec<BranoRemoto>, AppError> {
    // Prima di leggere, non dopo: vedi `Ricordate`.
    assicura_le_radici(connection)?;
    let chiave = (segno(connection)?.1, ordine.sql());
    let ricordato = presa(cache).brani.get(&chiave).cloned();
    let id = match ricordato {
        Some(id) => id,
        None if da < scarto => return brani_diretti(connection, ordine, da, quanti),
        None => {
            let id = Arc::new(id_dei_brani(connection, ordine)?);
            let mut ricordate = presa(cache);
            if ricordate.brani.len() >= ELENCHI_RICORDATI && !ricordate.brani.contains_key(&chiave)
            {
                ricordate.brani.clear();
            }
            ricordate.brani.insert(chiave, Arc::clone(&id));
            #[cfg(test)]
            {
                ricordate.costruiti += 1;
            }
            id
        }
    };
    let fetta: Vec<i64> = id
        .iter()
        .skip(usize::try_from(da).unwrap_or(0))
        .take(usize::try_from(quanti).unwrap_or(0))
        .copied()
        .collect();
    if fetta.is_empty() {
        return Ok(Vec::new());
    }
    let letti = brani_per_id(connection, &fetta)?;
    if letti.len() == fetta.len() {
        Ok(letti)
    } else {
        brani_diretti(connection, ordine, da, quanti)
    }
}

/// Gli id dei brani condivisi, nell'ordine dell'ordinamento.
fn id_dei_brani(connection: &Connection, ordine: TrackOrder) -> Result<Vec<i64>, AppError> {
    let cosa = "elenco dei brani condivisi";
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT t.id FROM tracks t WHERE {CONDIVISO} ORDER BY {}",
            ordine.sql()
        ))
        .map_err(|err| db_error(cosa, &err))?;
    statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// I brani condivisi con questi id, nell'ordine degli id: quelli che non lo
/// sono più, o non ci sono più, mancano.
fn brani_per_id(connection: &Connection, id: &[i64]) -> Result<Vec<BranoRemoto>, AppError> {
    let segnaposti = vec!["?"; id.len()].join(",");
    let sql = format!(
        "SELECT {COLONNE_REMOTO} FROM tracks t WHERE t.id IN ({segnaposti}) AND {CONDIVISO}"
    );
    let mut per_id: HashMap<i64, BranoRemoto> = righe_di_brani(
        connection,
        &sql,
        rusqlite::params_from_iter(id),
        "brani condivisi per id",
    )?
    .into_iter()
    .map(|brano| (brano.id, brano))
    .collect();
    Ok(id.iter().filter_map(|id| per_id.remove(id)).collect())
}

/// Una pagina di brani condivisi, con l'`OFFSET` di SQLite.
pub(super) fn brani_diretti(
    connection: &Connection,
    ordine: TrackOrder,
    da: i64,
    quanti: i64,
) -> Result<Vec<BranoRemoto>, AppError> {
    assicura_le_radici(connection)?;
    let sql = format!(
        "SELECT {COLONNE_REMOTO} FROM tracks t WHERE {CONDIVISO}
         ORDER BY {} LIMIT ?1 OFFSET ?2",
        ordine.sql()
    );
    righe_di_brani(
        connection,
        &sql,
        rusqlite::params![quanti, da],
        "brani condivisi",
    )
}

/// I brani condivisi di un album, dalla sua impronta, nell'ordine del disco.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brani_album(connection: &Connection, impronta: &str) -> Result<Vec<BranoRemoto>, AppError> {
    assicura_le_radici(connection)?;
    let Some(album_key) = chiave_dell_impronta(connection, impronta)? else {
        return Ok(Vec::new());
    };
    let album_key = album_key.as_str();
    let sql = format!(
        "SELECT {COLONNE_REMOTO} FROM tracks t WHERE t.album_key = ?1 AND {CONDIVISO}
         ORDER BY t.disc_number, t.track_number, t.title COLLATE NOCASE"
    );
    righe_di_brani(connection, &sql, [album_key], "brani condivisi di un album")
}

/// Alcuni brani condivisi, per id: quelli che non lo sono, o non ci sono
/// più, mancano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn brano(connection: &Connection, id: i64) -> Result<Option<BranoRemoto>, AppError> {
    assicura_le_radici(connection)?;
    let sql = format!("SELECT {COLONNE_REMOTO} FROM tracks t WHERE t.id = ?1 AND {CONDIVISO}");
    Ok(
        righe_di_brani(connection, &sql, [id], "un brano condiviso")?
            .into_iter()
            .next(),
    )
}

/// Una pagina degli album con almeno un brano condiviso.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn album(connection: &Connection, da: i64, quanti: i64) -> Result<Vec<AlbumSummary>, AppError> {
    assicura_le_radici(connection)?;
    // Lo stesso ordine della griglia di qui (D-104).
    super::interrogazioni::assicura_ordine(connection)?;
    let ordine = super::interrogazioni::ORDINE_DEGLI_ALBUM;
    let sql = format!(
        "SELECT {} FROM albums a
         WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_key = a.album_key AND {CONDIVISO})
         ORDER BY {ordine}
         LIMIT ?1 OFFSET ?2",
        colonne_album_condivise()
    );
    righe_di_album(
        connection,
        &sql,
        rusqlite::params![quanti, da],
        "album condivisi",
    )
}

/// Gli album di un artista con almeno un suo brano condiviso: la stessa
/// chiave di [`super::albums_by_artist`].
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn album_artista(
    connection: &Connection,
    artista: &str,
    quanti: i64,
) -> Result<Vec<AlbumSummary>, AppError> {
    assicura_le_radici(connection)?;
    let sql = format!(
        "SELECT {} FROM albums a
         WHERE a.album_key IN (SELECT album_key FROM tracks
                               WHERE {ARTISTA_EFFETTIVO} = ?1 AND {CONDIVISO_SENZA_ALIAS})
         ORDER BY a.year, a.title COLLATE NOCASE
         LIMIT ?2",
        colonne_album_condivise()
    );
    righe_di_album(
        connection,
        &sql,
        rusqlite::params![artista, quanti],
        "album condivisi di un artista",
    )
}

/// Gli artisti con almeno un brano condiviso, in ordine alfabetico del nome
/// normalizzato, a pagine: come [`super::list_artists`], contando solo i
/// brani condivisi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn artisti(
    connection: &Connection,
    da: usize,
    quanti: usize,
) -> Result<Vec<ArtistSummary>, AppError> {
    artisti_con(connection, ricordate(), da, quanti)
}

/// [`artisti`] con la sua cache a parte: per le prove.
pub(super) fn artisti_con(
    connection: &Connection,
    cache: &Mutex<Ricordate>,
    da: usize,
    quanti: usize,
) -> Result<Vec<ArtistSummary>, AppError> {
    let cosa = "artisti condivisi";
    // Prima di contare, non dopo: vedi `Ricordate`. Con le cartelle sorvegliate
    // nella chiave: vedi `segno`.
    assicura_le_radici(connection)?;
    let (_, chiave) = segno(connection)?;
    let ricordati = presa(cache).artisti.get(&chiave).cloned();
    let tutti = match ricordati {
        Some(tutti) => tutti,
        None => {
            let tutti = Arc::new(artisti_ordinati(connection)?);
            let mut ricordate = presa(cache);
            if ricordate.artisti.len() >= ARTISTI_RICORDATI
                && !ricordate.artisti.contains_key(&chiave)
            {
                ricordate.artisti.clear();
            }
            ricordate.artisti.insert(chiave, Arc::clone(&tutti));
            #[cfg(test)]
            {
                ricordate.costruiti += 1;
            }
            tutti
        }
    };
    let mut pagina: Vec<ArtistSummary> = tutti.iter().skip(da).take(quanti).cloned().collect();
    if pagina.is_empty() {
        return Ok(pagina);
    }
    // Le copertine solo per la pagina: fino a quattro per artista, dagli
    // album più vecchi, come la griglia di qui.
    let mut copertine = connection
        .prepare_cached(&format!(
            "SELECT MIN(cover_art_hash) AS hash, MIN(year) AS anno
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} = ?1 AND cover_art_hash IS NOT NULL
               AND {CONDIVISO_SENZA_ALIAS}
             GROUP BY album_key
             ORDER BY anno
             LIMIT 8"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    for artista in &mut pagina {
        let righe = copertine
            .query_map([&artista.name], |row| row.get::<_, String>(0))
            .map_err(|err| db_error(cosa, &err))?;
        for riga in righe {
            let hash = riga.map_err(|err| db_error(cosa, &err))?;
            if artista.covers.len() < 4 && !artista.covers.contains(&hash) {
                artista.covers.push(hash);
            }
        }
    }
    Ok(pagina)
}

/// Tutti gli artisti con almeno un brano condiviso, in ordine, senza copertine.
fn artisti_ordinati(connection: &Connection) -> Result<Vec<ArtistSummary>, AppError> {
    let cosa = "artisti condivisi";
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {ARTISTA_EFFETTIVO} AS nome, COUNT(DISTINCT album_key), COUNT(*)
             FROM tracks
             WHERE {ARTISTA_EFFETTIVO} <> '' AND {CONDIVISO_SENZA_ALIAS}
             GROUP BY nome"
        ))
        .map_err(|err| db_error(cosa, &err))?;
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
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))?;
    artisti.sort_by(|a, b| {
        a.sort_name
            .cmp(&b.sort_name)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(artisti)
}

/// Cerca fra i brani condivisi, con la stessa traduzione della ricerca di
/// qui ([`fts_query`]).
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn cerca(
    connection: &Connection,
    testo: &str,
    da: i64,
    quanti: i64,
) -> Result<Vec<BranoRemoto>, AppError> {
    assicura_le_radici(connection)?;
    let espressione = fts_query(testo);
    if espressione.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {COLONNE_REMOTO}
         FROM tracks_fts
         JOIN tracks t ON t.id = tracks_fts.rowid
         WHERE tracks_fts MATCH ?1 AND {CONDIVISO}
         ORDER BY bm25(tracks_fts) LIMIT ?2 OFFSET ?3"
    );
    righe_di_brani(
        connection,
        &sql,
        rusqlite::params![espressione, quanti, da],
        "ricerca fra i brani condivisi",
    )
}

/// Quanti brani condivisi risponderebbero a questa ricerca.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn cerca_conteggio(connection: &Connection, testo: &str) -> Result<i64, AppError> {
    assicura_le_radici(connection)?;
    let espressione = fts_query(testo);
    if espressione.is_empty() {
        return Ok(0);
    }
    connection
        .prepare_cached(&format!(
            "SELECT COUNT(*) FROM tracks_fts JOIN tracks t ON t.id = tracks_fts.rowid
             WHERE tracks_fts MATCH ?1 AND {CONDIVISO}"
        ))
        .and_then(|mut statement| statement.query_row([&espressione], |row| row.get(0)))
        .map_err(|err| db_error("conteggio della ricerca fra i brani condivisi", &err))
}

/// Il file di un brano condiviso, se lo è e se sta ancora sotto una delle
/// cartelle sorvegliate: una cartella tolta smette di essere condivisa subito,
/// non alla prossima scansione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn file_del_brano(connection: &Connection, id: i64) -> Result<Option<FileCondiviso>, AppError> {
    assicura_le_radici(connection)?;
    let cosa = "file di un brano condiviso";
    // Le cartelle sorvegliate le guarda già [`CONDIVISO`], sul percorso della
    // riga. Qui in più sul file che si legge davvero: per un brano d'immagine CD
    // è l'immagine, che può stare dove il percorso virtuale `…\Album.cue|03` non
    // dice. La stessa funzione, la stessa regola.
    let trovato = connection
        .prepare_cached(&format!(
            "SELECT COALESCE(t.immagine, t.path), t.file_size, t.date_modified
             FROM tracks t WHERE t.id = ?1 AND {CONDIVISO}
               AND {SOTTO_LE_RADICI}(COALESCE(t.immagine, t.path),
                                     (SELECT value FROM settings WHERE key = 'library.roots'))"
        ))
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| {
                    Ok(FileCondiviso {
                        percorso: row.get(0)?,
                        dimensione: row.get(1)?,
                        modificato_ms: row.get(2)?,
                    })
                })
                .optional()
        })
        .map_err(|err| db_error(cosa, &err))?;
    Ok(trovato)
}

// ── il catalogo ─────────────────────────────────────────────────────────────

/// Quanti id sta un secchio del catalogo: `id / 500` dice il secchio di una
/// riga, e il secchio `k` va da `k * 500` a `k * 500 + 499`.
pub const ID_PER_SECCHIO: i64 = 500;

/// Una riga del catalogo: un [`BranoRemoto`] con quel che serve a farne una
/// riga di libreria di un altro dispositivo — e a riconoscerla come lo stesso
/// brano di un'altra copia.
///
/// Tutto quel che porta sta nei tetti di [`crate::specchio::convalida`]: le
/// stringhe lunghe si tagliano **prima** di partire, perché cinquecento righe
/// stiano nella cornice del protocollo.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranoCatalogo {
    /// L'id nella libreria **dell'altro**.
    pub id: i64,
    /// Il titolo.
    pub title: String,
    /// L'interprete.
    pub artist: String,
    /// L'album.
    pub album: String,
    /// L'artista dell'album.
    pub album_artist: Option<String>,
    /// Il genere.
    pub genre: Option<String>,
    /// L'impronta dell'album ([`impronta_album`]).
    pub album_key: Option<String>,
    /// Numero di traccia.
    pub track_number: Option<i64>,
    /// Numero di disco.
    pub disc_number: Option<i64>,
    /// L'anno.
    pub year: Option<i64>,
    /// Durata in millisecondi.
    pub duration_ms: i64,
    /// L'impronta della copertina, da chiedere all'altro.
    pub cover_art_hash: Option<String>,
    /// Quante volte l'ha ascoltato il proprietario.
    pub play_count: i64,
    /// È fra i suoi preferiti.
    pub liked: bool,
    /// Il suo voto.
    pub rating: i64,
    /// Quando ha deciso il preferito.
    pub liked_at: Option<i64>,
    /// Quando ha deciso il voto.
    pub rating_at: Option<i64>,
    /// Quando l'ha aggiunto.
    pub date_added: i64,
    /// L'ultimo ascolto.
    pub last_played_at: Option<i64>,
    /// L'orologio delle sue statistiche.
    pub stats_updated_at: i64,
    /// Bitrate in kbps.
    pub bitrate: Option<i64>,
    /// Frequenza di campionamento.
    pub sample_rate: Option<i64>,
    /// Canali.
    pub channels: Option<i64>,
    /// Il formato.
    pub codec: Option<String>,
    /// L'identità fra dispositivi, com'è scritta nella sua libreria.
    pub track_key: String,
    /// La sua chiave di contenuto, se ce l'ha.
    pub content_key: Option<String>,
    /// Identificativo MusicBrainz della registrazione.
    pub mb_recording_id: Option<String>,
    /// Identificativo MusicBrainz del gruppo di pubblicazione.
    pub mb_release_group_id: Option<String>,
    /// Identificativo MusicBrainz della pubblicazione.
    pub mb_release_id: Option<String>,
    /// L'estensione del file da leggere, minuscola e senza punto.
    pub estensione: String,
    /// Quanti byte.
    pub dimensione: i64,
    /// Cambia se il file cambia: dimensione e data.
    pub versione: String,
    /// Per un brano d'immagine CD, il pezzo.
    pub segmento: Option<Segmento>,
}

/// Un secchio nel riassunto del catalogo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecchioCatalogo {
    /// Il primo id del secchio (`k * 500`).
    pub da_id: i64,
    /// L'ultimo (`k * 500 + 499`).
    pub a_id: i64,
    /// Cambia se cambia una sola riga del secchio, o una sua statistica.
    pub impronta: String,
    /// Quante righe ha.
    pub righe: i64,
}

/// Il catalogo condiviso in riassunto: vedi [`riassunto_catalogo`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiassuntoCatalogo {
    /// La generazione della libreria in cui è stato calcolato.
    pub generazione: i64,
    /// I secchi, per id crescente; quelli vuoti non ci sono.
    pub secchi: Vec<SecchioCatalogo>,
}

/// Le righe di un secchio: vedi [`pagina_catalogo`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginaCatalogo {
    /// La generazione in cui sono state lette.
    pub generazione: i64,
    /// Il secchio chiesto.
    pub da_id: i64,
    /// Idem.
    pub a_id: i64,
    /// Le righe condivise con un id fra i due, per id crescente.
    pub brani: Vec<BranoCatalogo>,
}

const COLONNE_CATALOGO: &str = "t.id, t.title, t.artist, t.album, t.album_key, t.track_number,
     t.disc_number, t.duration_ms, t.year, t.cover_art_hash, t.play_count, t.liked, t.rating,
     COALESCE(t.immagine, t.path), t.file_size, t.date_modified, t.inizio_cd, t.fine_cd,
     t.album_artist, t.genre, t.liked_at, t.rating_at, t.date_added, t.last_played_at,
     t.stats_updated_at, t.bitrate, t.sample_rate, t.channels, t.codec, t.track_key,
     t.content_key, t.mb_recording_id, t.mb_release_group_id, t.mb_release_id";

/// Le righe condivise ([`CONDIVISO`]) che il **catalogo** manda: quelle la cui
/// `track_key` sta nel tetto che chi riceve accetta
/// (`specchio::convalida::MAX_CHIAVE`, in **byte**: `length` su un blob, che su
/// un testo conta i caratteri).
///
/// Una chiave d'identità non si taglia — tagliata direbbe un altro brano —, e chi
/// riceve scarta la riga che non ci sta: mandarla tagliata, com'era, faceva
/// diventare lo stesso brano due brani con lo stesso inizio di chiave (S2-8). Lo
/// stesso filtro serve il riassunto e le pagine: se contassero righe diverse, i
/// secchi non coinciderebbero con le loro impronte e chi legge non si fermerebbe.
const NEL_CATALOGO: &str = "length(CAST(t.track_key AS BLOB)) <= 300";

/// Il tetto di [`NEL_CATALOGO`] è quello di chi riceve.
const _: () = assert!(crate::specchio::convalida::MAX_CHIAVE == 300);

/// Le colonne di [`COLONNE_REMOTO`] seguite da quelle del catalogo, nei limiti
/// della cornice.
fn catalogo_da_riga(row: &Row<'_>) -> rusqlite::Result<BranoCatalogo> {
    use crate::specchio::convalida::{
        MAX_CHIAVE, MAX_CODEC, MAX_GENERE, MAX_ID_MB, MAX_NOME, tronca, tronca_opz,
    };
    let remoto = brano_da_riga(row)?;
    Ok(BranoCatalogo {
        id: remoto.id,
        title: tronca(&remoto.title, MAX_NOME),
        artist: tronca(&remoto.artist, MAX_NOME),
        album: tronca(&remoto.album, MAX_NOME),
        album_artist: tronca_opz(row.get(18)?, MAX_NOME),
        genre: tronca_opz(row.get(19)?, MAX_GENERE),
        album_key: remoto.album_key,
        track_number: remoto.track_number,
        disc_number: remoto.disc_number,
        year: remoto.year,
        duration_ms: remoto.duration_ms,
        cover_art_hash: remoto.cover_art_hash,
        play_count: remoto.play_count,
        liked: remoto.liked,
        rating: remoto.rating,
        liked_at: row.get(20)?,
        rating_at: row.get(21)?,
        date_added: row.get(22)?,
        last_played_at: row.get(23)?,
        stats_updated_at: row.get(24)?,
        bitrate: row.get(25)?,
        sample_rate: row.get(26)?,
        channels: row.get(27)?,
        codec: tronca_opz(row.get(28)?, MAX_CODEC),
        // Mai tagliate: una chiave tagliata direbbe un altro brano (S2-8). Le righe la
        // cui `track_key` non sta nel tetto non arrivano qui ([`NEL_CATALOGO`]), e
        // una chiave di contenuto troppo lunga si manda assente: chi riceve fa a
        // meno di lei, e il brano ripiega su `track_key`, come per ogni chiave che
        // non sta nel tetto (`specchio::convalida::convalida_riga`).
        track_key: row.get::<_, String>(29)?,
        content_key: row
            .get::<_, Option<String>>(30)?
            .filter(|chiave| chiave.len() <= MAX_CHIAVE),
        mb_recording_id: tronca_opz(row.get(31)?, MAX_ID_MB),
        mb_release_group_id: tronca_opz(row.get(32)?, MAX_ID_MB),
        mb_release_id: tronca_opz(row.get(33)?, MAX_ID_MB),
        estensione: remoto.estensione,
        dimensione: remoto.dimensione,
        versione: remoto.versione,
        segmento: remoto.segmento,
    })
}

/// Il catalogo condiviso in riassunto, calcolato una volta per generazione
/// della libreria: vedi [`Ricordate`].
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn riassunto_catalogo(connection: &Connection) -> Result<Arc<RiassuntoCatalogo>, AppError> {
    riassunto_con(connection, ricordate())
}

/// Quanti riassunti si tengono: la generazione di adesso, e una.
const RIASSUNTI_RICORDATI: usize = 2;

/// [`riassunto_catalogo`] con la sua cache a parte: per le prove.
pub(super) fn riassunto_con(
    connection: &Connection,
    cache: &Mutex<Ricordate>,
) -> Result<Arc<RiassuntoCatalogo>, AppError> {
    // Prima di calcolare, non dopo: vedi `Ricordate`. Con le cartelle sorvegliate
    // nella chiave: vedi `segno`; a chi legge si dice la sola generazione.
    assicura_le_radici(connection)?;
    let (generazione, chiave) = segno(connection)?;
    if let Some(ricordato) = presa(cache).catalogo.get(&chiave).cloned() {
        return Ok(ricordato);
    }
    let calcolato = Arc::new(calcola_il_riassunto(connection, generazione)?);
    let mut ricordate = presa(cache);
    if ricordate.catalogo.len() >= RIASSUNTI_RICORDATI && !ricordate.catalogo.contains_key(&chiave)
    {
        ricordate.catalogo.clear();
    }
    ricordate.catalogo.insert(chiave, Arc::clone(&calcolato));
    #[cfg(test)]
    {
        ricordate.costruiti += 1;
    }
    Ok(calcolato)
}

/// Il riassunto, dalle righe: una passata sola, per id, con l'impronta di ogni
/// secchio che cresce riga per riga.
fn calcola_il_riassunto(
    connection: &Connection,
    generazione: i64,
) -> Result<RiassuntoCatalogo, AppError> {
    let cosa = "riassunto del catalogo condiviso";
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {COLONNE_CATALOGO} FROM tracks t WHERE {CONDIVISO} AND {NEL_CATALOGO} ORDER BY t.id"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    let mut righe = statement.query([]).map_err(|err| db_error(cosa, &err))?;
    let mut secchi: Vec<SecchioCatalogo> = Vec::new();
    let mut corrente: Option<(i64, blake3::Hasher, i64)> = None;
    while let Some(riga) = righe.next().map_err(|err| db_error(cosa, &err))? {
        let brano = catalogo_da_riga(riga).map_err(|err| db_error(cosa, &err))?;
        let secchio = brano.id.div_euclid(ID_PER_SECCHIO);
        if corrente.as_ref().is_some_and(|(k, _, _)| *k != secchio) {
            chiudi_il_secchio(&mut secchi, corrente.take());
        }
        let (_, hasher, quante) =
            corrente.get_or_insert_with(|| (secchio, blake3::Hasher::new(), 0));
        // Il JSON della riga com'è partirebbe: se cambia una cifra, cambia.
        let json = serde_json::to_vec(&brano).map_err(|err| {
            AppError::new(ErrorCode::InternalUnexpected { detail: None })
                .with_cause(err.to_string())
        })?;
        hasher.update(&json);
        hasher.update(b"\n");
        *quante += 1;
    }
    chiudi_il_secchio(&mut secchi, corrente.take());
    Ok(RiassuntoCatalogo {
        generazione,
        secchi,
    })
}

fn chiudi_il_secchio(
    secchi: &mut Vec<SecchioCatalogo>,
    corrente: Option<(i64, blake3::Hasher, i64)>,
) {
    let Some((k, hasher, righe)) = corrente else {
        return;
    };
    secchi.push(SecchioCatalogo {
        da_id: k.saturating_mul(ID_PER_SECCHIO),
        a_id: k
            .saturating_mul(ID_PER_SECCHIO)
            .saturating_add(ID_PER_SECCHIO - 1),
        impronta: hasher.finalize().to_hex().chars().take(32).collect(),
        righe,
    });
}

/// Le righe condivise di un secchio, per id: da `da_id` a `a_id` compresi.
///
/// # Errori
///
/// `lontano.protocollo` per un secchio più largo di [`ID_PER_SECCHIO`] id o
/// con gli estremi al contrario; `db.queryFailed` se il database non risponde.
pub fn pagina_catalogo(
    connection: &Connection,
    da_id: i64,
    a_id: i64,
) -> Result<PaginaCatalogo, AppError> {
    assicura_le_radici(connection)?;
    if da_id < 0 || a_id < da_id || a_id - da_id >= ID_PER_SECCHIO {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("un secchio del catalogo più largo di quanto si serve"));
    }
    let cosa = "righe di un secchio del catalogo";
    // Prima di leggere, non dopo: vedi `Ricordate`.
    let generazione = generazione(connection)?;
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT {COLONNE_CATALOGO} FROM tracks t
              WHERE t.id BETWEEN ?1 AND ?2 AND {CONDIVISO} AND {NEL_CATALOGO}
              ORDER BY t.id LIMIT ?3"
        ))
        .map_err(|err| db_error(cosa, &err))?;
    let brani = statement
        .query_map(
            rusqlite::params![da_id, a_id, ID_PER_SECCHIO],
            catalogo_da_riga,
        )
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error(cosa, &err))?;
    Ok(PaginaCatalogo {
        generazione,
        da_id,
        a_id,
        brani,
    })
}

/// Quando si è deciso ogni campo di una valutazione che arriva da un altro
/// dispositivo (S2-9).
///
/// Un cuore a un istante e le stelle a uno dopo viaggiano in una voce sola, che
/// ha per orologio il più recente dei due: se bastasse quello, il cuore deciso
/// prima si «ringiovanirebbe» con le stelle date dopo, e batterebbe un cuore che
/// il proprietario ha tolto in mezzo. Con un orologio per campo vince il più
/// recente **di ognuno**. Una build di prima non manda i due campi, e vale
/// `quando_ms` per tutti e due: com'era.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrologiValutazione {
    /// L'orologio della voce, e di tutti i campi che non ne hanno uno proprio.
    pub quando_ms: i64,
    /// Quando si è deciso il mi piace, se è un momento diverso.
    pub liked_ms: Option<i64>,
    /// Quando si sono decise le stelle, se è un momento diverso.
    pub rating_ms: Option<i64>,
}

impl OrologiValutazione {
    /// Un orologio solo, per tutti e due i campi.
    #[must_use]
    pub const fn unico(quando_ms: i64) -> Self {
        Self {
            quando_ms,
            liked_ms: None,
            rating_ms: None,
        }
    }
}

/// Un mi piace o un voto arrivato da un altro dispositivo per un brano di qui:
/// vince l'orologio più recente, **per campo** — con `liked_at` e `rating_at`,
/// gli stessi di un gesto fatto qui — e quello che non cambia non si tocca.
/// `true` se qualcosa è cambiato davvero.
///
/// `liked` e `rating` sono `None` quando chi manda non li ha cambiati: un
/// voto dato altrove non deve riscrivere il preferito con quel che là era
/// rimasto indietro. Innocuo se arriva due volte: la seconda ha lo stesso
/// orologio e non è più recente.
///
/// # Errori
///
/// `lontano.fileCambiato` per un brano che non c'è o non è più condiviso;
/// `lontano.protocollo` per un voto fuori da 0–5 o un orologio che non è un
/// istante; `db.queryFailed` se il database non risponde.
pub fn valuta(
    connection: &Connection,
    brano: i64,
    liked: Option<bool>,
    rating: Option<i64>,
    quando_ms: i64,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    valuta_con_orologi(
        connection,
        brano,
        liked,
        rating,
        OrologiValutazione::unico(quando_ms),
        adesso_ms,
    )
}

/// [`valuta`] con un orologio per ogni campo ([`OrologiValutazione`]).
///
/// # Errori
///
/// Quelli di [`valuta`]; anche per un orologio di un campo che non è un istante.
pub fn valuta_con_orologi(
    connection: &Connection,
    brano: i64,
    liked: Option<bool>,
    rating: Option<i64>,
    orologi: OrologiValutazione,
    adesso_ms: i64,
) -> Result<bool, AppError> {
    let OrologiValutazione {
        quando_ms,
        liked_ms,
        rating_ms,
    } = orologi;
    if quando_ms <= 0
        || liked_ms.is_some_and(|ms| ms <= 0)
        || rating_ms.is_some_and(|ms| ms <= 0)
        || rating.is_some_and(|r| !(0..=5).contains(&r))
    {
        return Err(AppError::new(ErrorCode::LontanoProtocollo)
            .with_cause("una valutazione con un voto o un orologio che non stanno"));
    }
    assicura_le_radici(connection)?;
    if self::brano(connection, brano)?.is_none() {
        return Err(AppError::new(ErrorCode::LontanoFileCambiato)
            .with_cause("il brano non è più condiviso"));
    }
    // Un orologio avanti non fissa un voto che nessun gesto vero scavalcherebbe.
    let quando_liked = liked_ms.unwrap_or(quando_ms).min(adesso_ms);
    let quando_rating = rating_ms.unwrap_or(quando_ms).min(adesso_ms);
    let toccate = connection
        .execute(
            &format!(
                "UPDATE tracks SET
                   liked = CASE WHEN ?2 IS NOT NULL AND ?4 > COALESCE(liked_at, -1)
                                THEN ?2 ELSE liked END,
                   liked_at = CASE WHEN ?2 IS NOT NULL AND ?4 > COALESCE(liked_at, -1)
                                   THEN ?4 ELSE liked_at END,
                   rating = CASE WHEN ?3 IS NOT NULL AND ?5 > COALESCE(rating_at, -1)
                                 THEN ?3 ELSE rating END,
                   rating_at = CASE WHEN ?3 IS NOT NULL AND ?5 > COALESCE(rating_at, -1)
                                    THEN ?5 ELSE rating_at END,
                   stats_updated_at = MAX(stats_updated_at,
                                          CASE WHEN ?2 IS NOT NULL THEN ?4 ELSE 0 END,
                                          CASE WHEN ?3 IS NOT NULL THEN ?5 ELSE 0 END)
                 WHERE id = ?1 AND {CONDIVISO_SENZA_ALIAS}
                   AND ((?2 IS NOT NULL AND ?4 > COALESCE(liked_at, -1))
                     OR (?3 IS NOT NULL AND ?5 > COALESCE(rating_at, -1)))"
            ),
            rusqlite::params![brano, liked, rating, quando_liked, quando_rating],
        )
        .map_err(|err| db_error("valutazione di un brano condiviso", &err))?;
    Ok(toccate > 0)
}
