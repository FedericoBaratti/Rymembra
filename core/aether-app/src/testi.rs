//! I testi: dove si trovano, e in che ordine si guarda.
//!
//! Il formato lo interpreta [`aether_domain::testo`], che è puro. Qui si apre il
//! disco e si interroga il database — cioè le due cose che quel modulo non sa
//! fare — e si tiene la catena delle fonti.
//!
//! # La catena, e perché in quest'ordine
//!
//! 1. **Il sidecar** `brano.lrc` accanto al file. È l'unica fonte che sta *sul
//!    disco dell'utente e riferita a quel file esatto*: se c'è, è la risposta.
//!    Lo si rilegge ogni volta invece di fidarsi della copia in tabella, perché
//!    chi lo modifica con un editor di testo deve vedere il cambiamento senza
//!    dover sapere che esiste una cache. L'unica eccezione è il sidecar
//!    **senza tempi** davanti a una riga che i tempi ce li ha: la ragione sta
//!    dov'è scritta l'eccezione, in `per_questo_brano`.
//! 2. **La riga in `lyrics`**, che è dove vivono il testo scaricato e quello
//!    sincronizzato a mano. È indicizzata su `track_key`: lo stesso brano in due
//!    formati diversi condivide una riga sola, e spostare il file non la perde.
//! 3. **Il tag del file**, cioè `tracks.lyrics`, che la scansione riempie da
//!    `USLT`/`©lyr`/`LYRICS`. Ultimo perché è quasi sempre senza tempi — e
//!    quando invece i tempi ce li ha, perché qualcuno ci ha messo dentro un LRC
//!    intero, [`aether_domain::testo::leggi`] se ne accorge da sé.
//!
//! Quel che si trova nelle fonti 1 e 3 si copia nella 2, che da lì in poi fa da
//! cache. Quel che sta nella 2 non torna mai indietro da solo: scrivere nei file
//! di qualcun altro è un gesto separato, e in questo modulo non c'è.
//!
//! # Quattro esiti, e nessuna schermata bianca
//!
//! Ogni brano finisce in uno di quattro stati — sincronizzato, piatto,
//! strumentale, niente — e il quarto è l'unico che chiede qualcosa a chi guarda.
//! È la ragione per cui [`Fonte::Nessuna`] esiste come valore invece che come
//! `Option`: «non ho ancora guardato» e «ho guardato e non c'è» sono due cose,
//! e la seconda va ricordata.

use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::testo::{self, Aderenza, Cercato, Testo};
use aether_meta::Fornitori;
use aether_meta::lrclib::{self, Voce};
use rusqlite::{Connection, OptionalExtension as _};

use crate::library::{db_error, now_ms};

/// Da dove viene il testo che si sta mostrando.
///
/// Serializzato in camelCase come tutto quel che attraversa l'IPC: la finestra
/// lo mostra, perché «da dove viene questo testo» è la prima domanda di chi
/// vede una riga sbagliata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fonte {
    /// Non se n'è trovato nessuno.
    Nessuna,
    /// Un file `.lrc` accanto al brano.
    Sidecar,
    /// Il tag del file.
    Tag,
    /// Il catalogo dei testi.
    Lrclib,
    /// Sincronizzato a mano, qui dentro.
    Mano,
}

impl Fonte {
    /// Il nome con cui sta nella colonna `source`.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Nessuna => "",
            Self::Sidecar => "sidecar",
            Self::Tag => "tag",
            Self::Lrclib => "lrclib",
            Self::Mano => "mano",
        }
    }

    /// Il verso opposto. Un valore che non si conosce vale [`Fonte::Nessuna`].
    #[must_use]
    pub fn da_nome(grezzo: &str) -> Self {
        match grezzo {
            "sidecar" => Self::Sidecar,
            "tag" => Self::Tag,
            "lrclib" => Self::Lrclib,
            "mano" => Self::Mano,
            _ => Self::Nessuna,
        }
    }

    /// Questo testo l'ha scritto l'utente, e non lo si sovrascrive mai.
    #[must_use]
    pub const fn e_di_chi_ascolta(self) -> bool {
        matches!(self, Self::Mano)
    }
}

/// Il testo di un brano, con tutto quel che serve a mostrarlo.
#[derive(Debug, Clone)]
pub struct TestoBrano {
    /// Il testo, nella forma del dominio.
    pub testo: Testo,
    /// Da dove viene.
    pub fonte: Fonte,
    /// La correzione dell'utente, che si somma a quella del file.
    pub scarto_ms: i32,
    /// Quanto i tempi stanno dentro questo file.
    pub aderenza: Aderenza,
    /// Si è già chiesto al catalogo, e la risposta è stata questa.
    pub cercato: bool,
    /// Vale la pena chiedere al catalogo, adesso.
    ///
    /// Vero quando quel che si ha **non scorre** — niente tempi, e non è uno
    /// strumentale — e al catalogo non gli si è ancora chiesto. È la stessa
    /// regola della coda della passata, e sta qui invece che nella finestra per
    /// la ragione di sempre: una politica che vive in due posti diventa due
    /// politiche diverse al primo cambiamento.
    ///
    /// Il testo piatto conta come «non si ha»: era il difetto per cui un brano
    /// con le parole nel tag, o con una vecchia risposta senza tempi del
    /// catalogo, non veniva mai più chiesto e restava per sempre fermo.
    pub da_chiedere: bool,
}

impl Default for TestoBrano {
    fn default() -> Self {
        Self {
            testo: Testo::default(),
            fonte: Fonte::Nessuna,
            scarto_ms: 0,
            aderenza: Aderenza::Buona,
            cercato: false,
            da_chiedere: true,
        }
    }
}

/// Le poche colonne di `tracks` che servono a cercare un testo.
#[derive(Debug, Clone)]
pub struct BranoDaTestare {
    /// L'identificativo della riga.
    pub id: i64,
    /// Il percorso del file, per il sidecar.
    pub path: String,
    /// L'identità fra dispositivi, che è la chiave della tabella `lyrics`.
    pub track_key: String,
    /// Il titolo, per interrogare il catalogo.
    pub title: String,
    /// L'artista, per interrogare il catalogo.
    pub artist: String,
    /// L'album, per interrogare il catalogo.
    pub album: String,
    /// La durata del file, per il veto e per l'aderenza.
    pub duration_ms: i64,
    /// Il testo che stava nel tag, se ce n'era uno.
    pub lyrics: Option<String>,
}

/// Le colonne di [`BranoDaTestare`], nell'ordine in cui le legge `brano_da_riga`.
///
/// Con l'alias `t.` davanti a ognuna: la stessa stringa serve alla lettura di un
/// brano solo e alla coda della passata, che è una join, e due elenchi da tenere
/// allineati diventerebbero due elenchi diversi al primo campo aggiunto.
const COLONNE: &str = "t.id, t.path, t.track_key, t.title, t.artist, t.album,
     t.duration_ms, t.lyrics";

fn brano_da_riga(row: &rusqlite::Row<'_>) -> rusqlite::Result<BranoDaTestare> {
    Ok(BranoDaTestare {
        id: row.get(0)?,
        path: row.get(1)?,
        track_key: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        album: row.get(5)?,
        duration_ms: row.get(6)?,
        lyrics: row.get(7)?,
    })
}

/// Il brano, o `library.trackNotFound`.
///
/// # Errori
///
/// `library.trackNotFound` se la riga non c'è più, `db.queryFailed` se la
/// lettura fallisce.
pub fn brano(connection: &Connection, id: i64) -> Result<BranoDaTestare, AppError> {
    let sql = format!("SELECT {COLONNE} FROM tracks t WHERE t.id = ?1");
    connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("lettura di un brano per il testo", &err))?
        .query_row([id], brano_da_riga)
        .optional()
        .map_err(|err| db_error("lettura di un brano per il testo", &err))?
        .ok_or_else(|| AppError::new(ErrorCode::LibraryTrackNotFound { track_id: Some(id) }))
}

/// Il testo di un brano, percorrendo la catena delle fonti.
///
/// Non parla con la rete: chi la interroga è chi chiama, e lo fa solo se questa
/// risponde [`Fonte::Nessuna`] con `cercato` falso. La separazione è la stessa
/// che vale per l'arricchimento — quel che si può sapere senza rete si sa
/// subito, e il resto è un'altra decisione.
///
/// # Errori
///
/// `library.trackNotFound`, `db.queryFailed`. Un sidecar illeggibile **non** è
/// un errore: è una fonte in meno, e si passa alla successiva.
pub fn per_brano(connection: &Connection, id: i64) -> Result<TestoBrano, AppError> {
    let brano = brano(connection, id)?;
    Ok(per_questo_brano(connection, &brano))
}

/// Come [`per_brano`], quando la riga è già in mano.
///
/// Non fallisce: ogni fonte che non risponde è una fonte in meno, e l'esito
/// «niente» è un esito legittimo. Un guasto del database qui costerebbe una
/// cache, non un testo.
#[must_use]
pub fn per_questo_brano(connection: &Connection, brano: &BranoDaTestare) -> TestoBrano {
    let durata = u64::try_from(brano.duration_ms).unwrap_or(0);
    let riga = leggi_riga(connection, &brano.track_key);

    // Uno strumentale è una risposta, e vince su tutto: non c'è niente da
    // cercare altrove, ed è la fonte 2 l'unica che possa saperlo.
    if riga.as_ref().is_some_and(|r| r.strumentale) {
        return TestoBrano {
            testo: Testo {
                strumentale: true,
                ..Testo::default()
            },
            fonte: riga.as_ref().map_or(Fonte::Nessuna, |r| r.fonte),
            scarto_ms: riga.map_or(0, |r| r.scarto_ms),
            aderenza: Aderenza::Buona,
            cercato: true,
            da_chiedere: false,
        };
    }

    let scarto_ms = riga.as_ref().map_or(0, |r| r.scarto_ms);
    let cercato = riga.as_ref().is_some_and(|r| r.cercato);

    // ── 1. il sidecar ───────────────────────────────────────────────────────
    if let Some(grezzo) = leggi_sidecar(Path::new(&brano.path)) {
        let testo = testo::leggi(&grezzo);
        // Un sidecar **senza tempi** non scavalca una riga che i tempi ce li
        // ha. Il primato del sidecar esiste perché è il file dell'utente,
        // riferito a quel brano esatto, e perché chi lo modifica deve vedere il
        // cambiamento — non perché sia migliore per definizione. Un `.lrc` che
        // i tempi non li porta è un testo incollato lì dentro, e preferirlo
        // vorrebbe dire mostrare un testo fermo avendo in tabella quello che
        // scorre; peggio, il `ricorda` qui sotto lo scriverebbe **sopra** i
        // tempi, che a quel punto sarebbero persi per davvero.
        let copre_i_tempi = !testo.sincronizzato() && riga.as_ref().is_some_and(Riga::ha_i_tempi);
        if !testo.vuoto() && !copre_i_tempi {
            // Si ricorda, ma non si sovrascrive quel che l'utente ha
            // sincronizzato: il `.lrc` di `mano` **è** quello che l'utente ha
            // sincronizzato, quindi riscriverlo sarebbe scriverci sopra sé
            // stesso, e in un anno di riscritture un carattere si perde.
            if !riga.as_ref().is_some_and(|r| r.fonte.e_di_chi_ascolta()) {
                ricorda(connection, brano, &grezzo, &testo, Fonte::Sidecar);
            }
            return finisci(testo, Fonte::Sidecar, scarto_ms, durata, cercato);
        }
    }

    // ── 2. la riga in tabella ───────────────────────────────────────────────
    if let Some(riga) = riga
        && let Some(grezzo) = riga.grezzo()
    {
        let testo = testo::leggi(grezzo);
        if !testo.vuoto() {
            return finisci(testo, riga.fonte, scarto_ms, durata, cercato);
        }
    }

    // ── 3. il tag ───────────────────────────────────────────────────────────
    if let Some(grezzo) = brano.lyrics.as_deref().filter(|t| !t.trim().is_empty()) {
        let testo = testo::leggi(grezzo);
        if !testo.vuoto() {
            ricorda(connection, brano, grezzo, &testo, Fonte::Tag);
            return finisci(testo, Fonte::Tag, scarto_ms, durata, cercato);
        }
    }

    TestoBrano {
        scarto_ms,
        cercato,
        da_chiedere: !cercato,
        ..TestoBrano::default()
    }
}

/// Mette insieme l'esito, con l'aderenza calcolata sulla durata vera.
fn finisci(
    testo: Testo,
    fonte: Fonte,
    scarto_ms: i32,
    durata_ms: u64,
    cercato: bool,
) -> TestoBrano {
    let aderenza = testo::verifica_durata(&testo, durata_ms);
    // La regola sta scritta per esteso su `TestoBrano::da_chiedere`: si chiede
    // quando quel che si ha non scorre e non si è ancora chiesto.
    let da_chiedere = !cercato && !testo.sincronizzato() && !testo.strumentale;
    TestoBrano {
        testo,
        fonte,
        scarto_ms,
        aderenza,
        cercato,
        da_chiedere,
    }
}

// ── il sidecar ──────────────────────────────────────────────────────────────

/// I nomi che un sidecar può avere, dal più ricco al più comune.
///
/// `.a2.lrc` porta i tempi delle singole parole; `.lrc` quelli delle righe.
/// Guardare prima il primo costa una `stat` in più su ogni brano che non ce
/// l'ha, cioè quasi tutti — ma la spesa è una chiamata al filesystem, e
/// l'alternativa è non accorgersi mai del formato migliore.
const CODE: [&str; 2] = ["a2.lrc", "lrc"];

/// Il percorso di un sidecar per questo brano.
#[must_use]
pub fn percorso_sidecar(brano: &Path, coda: &str) -> PathBuf {
    brano.with_extension(coda)
}

/// Il contenuto del primo sidecar che esiste, se ne esiste uno.
///
/// # Le codifiche che non sono UTF-8
///
/// Un `.lrc` scaricato dieci anni fa può essere in Latin-1 o in Shift-JIS, e
/// non c'è niente nel file che lo dica. Si prova UTF-8 e, se fallisce, si legge
/// in modo tollerante: i caratteri che non si capiscono diventano un segno di
/// sostituzione, il resto del testo arriva. L'alternativa — riconoscere la
/// codifica — è una dipendenza in più per un caso che si risolve da sé appena
/// qualcuno risincronizza il brano, e con lui il file.
#[must_use]
pub fn leggi_sidecar(brano: &Path) -> Option<String> {
    for coda in CODE {
        let percorso = percorso_sidecar(brano, coda);
        let Ok(byte) = std::fs::read(&percorso) else {
            continue;
        };
        let grezzo = match String::from_utf8(byte) {
            Ok(testo) => testo,
            Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
        };
        if !grezzo.trim().is_empty() {
            return Some(grezzo);
        }
    }
    None
}

// ── la riga in tabella ──────────────────────────────────────────────────────

/// Quel che la tabella `lyrics` sa di un brano.
#[derive(Debug, Clone)]
pub struct Riga {
    /// L'LRC così com'era.
    pub synced: Option<String>,
    /// Il testo senza tempi.
    pub plain: Option<String>,
    /// Da dove viene.
    pub fonte: Fonte,
    /// La correzione dell'utente.
    pub scarto_ms: i32,
    /// Il brano non ha parole.
    pub strumentale: bool,
    /// Si è già chiesto al catalogo.
    pub cercato: bool,
}

impl Riga {
    /// Il testo da interpretare: i tempi se ci sono, altrimenti le parole.
    #[must_use]
    pub fn grezzo(&self) -> Option<&str> {
        self.synced
            .as_deref()
            .or(self.plain.as_deref())
            .filter(|t| !t.trim().is_empty())
    }

    /// La riga porta un LRC, cioè un testo che scorre.
    ///
    /// Guarda la colonna e non il testo interpretato: in `synced` ci finisce
    /// solo quel che [`aether_domain::testo::Testo::sincronizzato`] aveva già
    /// riconosciuto come tale, e rileggerlo qui sarebbe interpretare due volte
    /// lo stesso file per rispondere alla stessa domanda.
    #[must_use]
    pub fn ha_i_tempi(&self) -> bool {
        self.synced.as_deref().is_some_and(|t| !t.trim().is_empty())
    }
}

/// La riga di `lyrics` per questa chiave, se c'è.
///
/// Inghiotte i guasti: perdere questa lettura costa una fonte, e propagare
/// l'errore costerebbe il testo che le altre due fonti avrebbero dato.
#[must_use]
pub fn leggi_riga(connection: &Connection, track_key: &str) -> Option<Riga> {
    connection
        .prepare_cached(
            "SELECT synced, plain, source, offset_ms, instrumental, checked_at
             FROM lyrics WHERE track_key = ?1",
        )
        .ok()?
        .query_row([track_key], |row| {
            Ok(Riga {
                synced: row.get(0)?,
                plain: row.get(1)?,
                fonte: Fonte::da_nome(&row.get::<_, String>(2)?),
                scarto_ms: row.get(3)?,
                strumentale: row.get::<_, i64>(4)? != 0,
                cercato: row.get::<_, Option<i64>>(5)?.is_some(),
            })
        })
        .optional()
        .ok()
        .flatten()
}

/// Scrive quel che si è trovato, senza far cadere niente se non ci riesce.
///
/// `checked_at` **non** si tocca: questa funzione registra quel che si è trovato
/// sul disco, e non dice niente su cosa abbia risposto la rete. Confondere i due
/// vorrebbe dire che un brano con il testo nel tag non viene mai cercato, cioè
/// non ottiene mai i tempi.
fn ricorda(
    connection: &Connection,
    brano: &BranoDaTestare,
    grezzo: &str,
    testo: &Testo,
    fonte: Fonte,
) {
    let (synced, plain) = if testo.sincronizzato() {
        (Some(grezzo), None)
    } else {
        (None, Some(grezzo))
    };
    let _ = connection.execute(
        "INSERT INTO lyrics
           (track_key, plain, synced, source, offset_ms, instrumental, duration_ms, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?6)
         ON CONFLICT(track_key) DO UPDATE SET
           plain = excluded.plain,
           synced = excluded.synced,
           source = excluded.source,
           duration_ms = excluded.duration_ms,
           updated_at = excluded.updated_at
         WHERE lyrics.source <> 'mano'",
        rusqlite::params![
            brano.track_key,
            plain,
            synced,
            fonte.nome(),
            brano.duration_ms,
            now_ms(),
        ],
    );
}

/// Sposta il testo avanti o indietro per questo brano, e se lo ricorda.
///
/// Il verso è quello dello standard: positivo anticipa. Vedi
/// [`aether_domain::testo::posizione_corretta`], che è l'unico posto in cui
/// questo numero si somma a quello del file.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura non riesce. Qui l'errore si propaga, al
/// contrario che in [`ricorda`]: è un gesto esplicito di chi ascolta, e uno
/// spostamento che non si salva deve dirlo invece di tornare da solo a zero al
/// prossimo brano.
pub fn imposta_scarto(
    connection: &Connection,
    track_key: &str,
    scarto_ms: i32,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO lyrics (track_key, source, offset_ms, updated_at)
             VALUES (?1, 'mano', ?2, ?3)
             ON CONFLICT(track_key) DO UPDATE SET
               offset_ms = excluded.offset_ms,
               updated_at = excluded.updated_at",
            rusqlite::params![track_key, scarto_ms, now_ms()],
        )
        .map(|_| ())
        .map_err(|err| db_error("scrittura dello scarto di un testo", &err))
}

// ── il catalogo ─────────────────────────────────────────────────────────────

/// Dopo quanto si torna a chiedere un brano che il catalogo non aveva.
///
/// Quattordici giorni. Deve essere **più lungo** della memoria del «non ce
/// l'ho» di `aether_meta::deposito` — tre giorni — altrimenti la passata
/// rimetterebbe in coda ogni due giorni brani per cui la risposta arriverebbe
/// comunque dalla cache HTTP: traffico zero, ma una passata che scorre sempre
/// gli stessi mille brani senza mai arrivare a quelli nuovi.
///
/// Più lungo ancora sarebbe sbagliato nell'altro verso: un testo che oggi manca
/// può essere caricato domani da qualcuno, ed è tutto il punto di un catalogo
/// alimentato dalla comunità.
pub const RIPROVA_MS: i64 = 14 * 24 * 60 * 60 * 1000;

/// Quanti brani prende una passata.
///
/// Cinquanta, contro i dodici gruppi dell'arricchimento, e la differenza è nel
/// costo di una richiesta: là ogni gruppo sono più letture a una al secondo con
/// dei file da riscrivere in fondo, qui è una richiesta ogni quarto di secondo
/// e una riga di database. Cinquanta brani sono una dozzina di secondi.
pub const LOTTO: usize = 50;

/// Chiede il testo di un brano al catalogo. **Non tocca il database.**
///
/// La firma è quella che è per una ragione sola: chi la chiama non ha modo di
/// tenere il lucchetto della libreria mentre aspetta la rete, perché qui dentro
/// una `Connection` non entra. È la stessa disciplina dell'arricchimento, e non
/// è affidata all'attenzione di chi legge.
///
/// # Errori
///
/// L'errore di rete così com'è. `Ok(None)` è «il catalogo non ce l'ha», che è
/// un fatto sul brano e si ricorda; l'errore è «non si sa», che è un fatto
/// sulla rete di adesso e non si ricorda.
pub fn cerca_in_rete(
    fornitori: &Fornitori,
    brano: &BranoDaTestare,
) -> Result<Option<Voce>, AppError> {
    let album = brano.album.trim();
    lrclib::cerca(
        fornitori,
        &Cercato {
            titolo: &brano.title,
            artista: &brano.artist,
            album: (!album.is_empty()).then_some(album),
            durata_ms: u64::try_from(brano.duration_ms).ok().filter(|d| *d > 0),
        },
    )
}

/// Scrive quel che il catalogo ha risposto, sì o no che sia.
///
/// Il «no» si scrive esattamente come il «sì», e vale quanto: una riga con i
/// testi a `NULL` e `checked_at` pieno è la memoria di un brano che nessuno
/// conosce, ed è quel che impedisce alla passata di ricominciare ogni volta dai
/// duecento file che non troverà mai.
///
/// Non sovrascrive mai una sincronizzazione fatta a mano: la condizione sta
/// nella `WHERE` dell'`UPSERT`, cioè nel database, e non in un `if` che il
/// prossimo punto di chiamata potrebbe dimenticare.
///
/// # Errori
///
/// `db.queryFailed`. Qui l'errore si propaga: una passata che crede di aver
/// registrato un esito senza averlo fatto rifà lo stesso lavoro per sempre.
pub fn ricorda_esito(
    connection: &Connection,
    brano: &BranoDaTestare,
    voce: Option<&Voce>,
) -> Result<(), AppError> {
    let (piatto, sincronizzato, strumentale, id_catalogo) = match voce {
        Some(voce) => (
            voce.piatto.as_deref(),
            voce.sincronizzato.as_deref(),
            voce.candidato.strumentale,
            Some(voce.candidato.id),
        ),
        None => (None, None, false, None),
    };
    connection
        .execute(
            "INSERT INTO lyrics
               (track_key, plain, synced, source, lrclib_id, offset_ms, instrumental,
                duration_ms, updated_at, checked_at)
             VALUES (?1, ?2, ?3, 'lrclib', ?4, 0, ?5, ?6, ?7, ?7)
             ON CONFLICT(track_key) DO UPDATE SET
               plain = excluded.plain,
               synced = excluded.synced,
               source = excluded.source,
               lrclib_id = excluded.lrclib_id,
               instrumental = excluded.instrumental,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at,
               checked_at = excluded.checked_at
             WHERE lyrics.source <> 'mano'",
            rusqlite::params![
                brano.track_key,
                piatto,
                sincronizzato,
                id_catalogo,
                i64::from(strumentale),
                brano.duration_ms,
                now_ms(),
            ],
        )
        .map(|_| ())
        .map_err(|err| db_error("scrittura dell'esito di un testo", &err))?;

    // Se l'`UPSERT` non ha toccato niente perché il testo è di chi ascolta,
    // resta comunque da segnare che si è guardato: senza, la passata
    // rimetterebbe in coda per sempre un brano che ha già il suo testo.
    let _ = connection.execute(
        "UPDATE lyrics SET checked_at = ?2 WHERE track_key = ?1 AND source = 'mano'",
        rusqlite::params![brano.track_key, now_ms()],
    );
    Ok(())
}

/// Chi è in coda per il catalogo, come condizione `WHERE` su `tracks t`.
///
/// Una stringa sola perché a questa domanda rispondono in due — [`da_cercare`],
/// che prende il lotto, e [`quanti_da_cercare`], che dice a chi guarda quanto
/// manca — e due condizioni scritte a mano due volte diventano due condizioni
/// diverse al primo ripensamento. Il sintomo sarebbe una barra che arriva a zero
/// mentre la passata continua a lavorare.
///
/// `?1` è la soglia del ritentativo: `adesso − `[`RIPROVA_MS`].
const IN_CODA: &str = "t.id IN (SELECT MIN(id) FROM tracks GROUP BY track_key)
       AND NOT EXISTS (
         SELECT 1 FROM lyrics l
         WHERE l.track_key = t.track_key
           AND (l.synced IS NOT NULL
                OR l.instrumental = 1
                OR l.checked_at >= ?1)
       )";

/// Quanti brani aspettano il catalogo, adesso.
///
/// Non è [`Copertura::mancanti`]: quello conta i brani di cui non si ha
/// **niente**, questo conta le domande ancora aperte — e un brano di cui si ha
/// il testo piatto è una domanda aperta, perché i tempi possono esserci.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn quanti_da_cercare(connection: &Connection, adesso_ms: i64) -> Result<i64, AppError> {
    let sql = format!("SELECT COUNT(*) FROM tracks t WHERE {IN_CODA}");
    connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("quanti testi restano da cercare", &err))?
        .query_row([adesso_ms.saturating_sub(RIPROVA_MS)], |row| row.get(0))
        .map_err(|err| db_error("quanti testi restano da cercare", &err))
}

/// I prossimi brani da chiedere al catalogo, i più ascoltati per primi.
///
/// # Uno per brano, non uno per file
///
/// `IN (SELECT MIN(id) … GROUP BY track_key)` tiene un file solo per ogni
/// brano: chi ha la stessa canzone in FLAC e in mp3 la fa cercare una volta, e
/// la riga che ne esce vale per tutt'e due perché la tabella è indicizzata su
/// `track_key`.
///
/// # Chi si ascolta viene prima
///
/// L'ordine è per numero di ascolti. Su una libreria di duemila brani la prima
/// passata copre le canzoni che si sentono davvero mentre le altre aspettano il
/// loro turno — che è la differenza fra «fra due minuti serve» e «fra venti
/// minuti è tutto pronto».
///
/// # Un testo senza tempi non è un brano già fatto
///
/// Quel che esclude un brano dalla coda sono i **tempi**, lo strumentale e un
/// `checked_at` recente — non l'esistenza di un testo qualunque. Il testo
/// piatto arriva quasi sempre dal tag del file, e [`ricorda`] lo scrive senza
/// toccare `checked_at` proprio perché al catalogo non gli si è ancora chiesto
/// niente: escluderlo qui vorrebbe dire che chi ha i testi dentro i propri mp3
/// non ottiene i tempi mai, per nessun brano, e non capisce perché.
///
/// Il costo è che un brano che il catalogo conosce solo in piatto torna in coda
/// ogni [`RIPROVA_MS`]. È la stessa spesa che si accetta per i brani che il
/// catalogo non conosce affatto, e per la stessa ragione: quel che manca oggi
/// può caricarlo domani qualcuno.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn da_cercare(
    connection: &Connection,
    adesso_ms: i64,
    quanti: usize,
) -> Result<Vec<BranoDaTestare>, AppError> {
    let sql = format!(
        "SELECT {COLONNE} FROM tracks t
         WHERE {IN_CODA}
         ORDER BY t.play_count DESC, t.id
         LIMIT ?2"
    );
    let soglia = adesso_ms.saturating_sub(RIPROVA_MS);
    let quanti = i64::try_from(quanti).unwrap_or(i64::MAX);
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(|err| db_error("coda dei testi da cercare", &err))?;
    let righe = statement
        .query_map(rusqlite::params![soglia, quanti], brano_da_riga)
        .map_err(|err| db_error("coda dei testi da cercare", &err))?;
    righe
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| db_error("coda dei testi da cercare", &err))
}

/// Quanti brani stanno in ciascuno dei quattro stati.
///
/// Contati dal database a ogni richiesta e non tenuti in un contatore, per la
/// ragione già scritta a proposito dell'arricchimento: un contatore
/// divergerebbe al primo brano cancellato, e il sintomo sarebbe un pannello che
/// promette più di quel che c'è.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copertura {
    /// Brani con i tempi: scorrono.
    pub sincronizzati: i64,
    /// Brani col solo testo: si leggono.
    pub piatti: i64,
    /// Brani che non hanno parole.
    pub strumentali: i64,
    /// Brani per cui non si è ancora trovato niente.
    pub mancanti: i64,
}

/// I quattro numeri, su tutta la libreria.
///
/// Si contano i **brani** e non i file: `DISTINCT track_key`, come la coda. Una
/// libreria con ogni album in due formati direbbe altrimenti il doppio di
/// tutto, e la percentuale resterebbe giusta mentre i numeri mentirebbero.
///
/// # Errori
///
/// `db.queryFailed`.
pub fn copertura(connection: &Connection) -> Result<Copertura, AppError> {
    connection
        .prepare_cached(
            "SELECT
               COALESCE(SUM(CASE WHEN l.instrumental = 1 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN l.instrumental = 0 AND l.synced IS NOT NULL
                                 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN l.instrumental = 0 AND l.synced IS NULL
                                      AND l.plain IS NOT NULL
                                 THEN 1 ELSE 0 END), 0),
               COUNT(*)
             FROM (SELECT DISTINCT track_key FROM tracks) t
             LEFT JOIN lyrics l ON l.track_key = t.track_key",
        )
        .map_err(|err| db_error("copertura dei testi", &err))?
        .query_row([], |row| {
            let strumentali: i64 = row.get(0)?;
            let sincronizzati: i64 = row.get(1)?;
            let piatti: i64 = row.get(2)?;
            let totale: i64 = row.get(3)?;
            Ok(Copertura {
                sincronizzati,
                piatti,
                strumentali,
                mancanti: totale
                    .saturating_sub(strumentali)
                    .saturating_sub(sincronizzati)
                    .saturating_sub(piatti)
                    .max(0),
            })
        })
        .map_err(|err| db_error("copertura dei testi", &err))
}

// ── sincronizzato a mano ────────────────────────────────────────────────────

/// Scrive un testo sincronizzato a mano: il `.lrc` accanto al brano, e la riga.
///
/// # Perché il file viene prima
///
/// Perché il file **è** la verità e la riga è la copia veloce — sta scritto
/// nella migrazione `011_testi.sql`. Se il disco è pieno o la cartella è di sola
/// lettura, meglio fallire senza aver scritto niente da nessuna parte che
/// ritrovarsi una riga di database che promette un file che non esiste: al
/// prossimo avvio la catena leggerebbe la riga, non troverebbe il sidecar, e
/// nessuno saprebbe perché il testo c'è in Aether e in nessun altro lettore.
///
/// # I byte dell'audio non si toccano
///
/// Non si scrive in `USLT`, non si riapre il file con lofty, non serve
/// `enrich_undo`. Un `.lrc` accanto al brano lo leggono foobar2000, VLC,
/// Poweramp e Navidrome, e cancellarlo è un gesto che chiunque sa fare senza
/// strumenti — che è la definizione di reversibile.
///
/// # Errori
///
/// `fs.writeFailed` se il sidecar non si scrive, `db.queryFailed` se la riga non
/// si salva.
pub fn salva_a_mano(
    connection: &Connection,
    brano: &BranoDaTestare,
    lrc: &str,
) -> Result<(), AppError> {
    let percorso = percorso_sidecar(Path::new(&brano.path), "lrc");
    // `std::fs` e non `MusicFiles`: quel tratto sa aprire in lettura, perché è
    // quel che serve al motore. Scrivere accanto a un file altrui è un mestiere
    // in più, e quando arriverà Android — dove non c'è un percorso ma una
    // concessione — sarà quello il momento di allargare il tratto, non adesso
    // con un'astrazione che avrebbe un implementatore solo.
    std::fs::write(&percorso, lrc).map_err(|err| {
        AppError::new(ErrorCode::FsWriteFailed {
            path: percorso.display().to_string(),
            detail: None,
        })
        .with_cause(err.to_string())
    })?;

    connection
        .execute(
            "INSERT INTO lyrics
               (track_key, plain, synced, source, offset_ms, instrumental,
                duration_ms, updated_at, checked_at)
             VALUES (?1, NULL, ?2, 'mano', 0, 0, ?3, ?4, ?4)
             ON CONFLICT(track_key) DO UPDATE SET
               plain = NULL,
               synced = excluded.synced,
               source = 'mano',
               offset_ms = 0,
               instrumental = 0,
               duration_ms = excluded.duration_ms,
               updated_at = excluded.updated_at,
               checked_at = excluded.checked_at",
            rusqlite::params![brano.track_key, lrc, brano.duration_ms, now_ms()],
        )
        .map(|_| ())
        .map_err(|err| db_error("salvataggio di un testo sincronizzato", &err))?;
    Ok(())
}

/// Un testo pronto a tornare al catalogo, staccato dal database.
///
/// Possiede le sue stringhe invece di prestarle da una riga, ed è tutto il
/// punto: chi pubblica tiene questo in mano e **non** una `Connection`, quindi
/// il lucchetto della libreria è già stato lasciato quando parte la richiesta.
/// La stessa disciplina di [`cerca_in_rete`], scritta nei tipi invece che in un
/// commento che si può disattendere.
#[derive(Debug, Clone)]
pub struct DaRestituire {
    /// Il titolo, come sta nei tag del file.
    pub titolo: String,
    /// L'artista.
    pub artista: String,
    /// L'album.
    pub album: String,
    /// La durata del file, in millisecondi.
    pub durata_ms: u64,
    /// Il testo senza tempi, ricavato dall'LRC.
    pub piatto: String,
    /// L'LRC così com'è sul disco.
    pub sincronizzato: String,
}

/// Prepara il testo di un brano per il catalogo, o dice di no.
///
/// # Cosa si rifiuta di mandare, e perché
///
/// Solo quel che ha `source = 'mano'` esce di qui. Non è prudenza: è che
/// pubblicare è **attribuirsi** un contributo, e il testo che stava nel tag di
/// un file non l'ha scritto chi preme il pulsante. Rimandare al catalogo quel
/// che dal catalogo è appena arrivato sarebbe, nel migliore dei casi, rumore;
/// mandarci il tag di un file comprato altrove sarebbe girare il lavoro di
/// qualcun altro con la propria firma sopra.
///
/// Si rifiuta anche un testo senza tempi: LRCLIB il piatto ce l'ha quasi
/// sempre, e quel che manca al mondo — quel che questa app produce e nessun
/// altro — sono i tempi.
///
/// # Errori
///
/// `metadata.lyricsPublishRefused` con il motivo dentro, quando non c'è niente
/// da mandare; `db.queryFailed` se il brano non si legge.
pub fn da_restituire(connection: &Connection, id: i64) -> Result<DaRestituire, AppError> {
    let brano = brano(connection, id)?;
    let rifiuto = |perche: &str| {
        AppError::new(ErrorCode::MetadataLyricsPublishRefused {
            detail: Some(perche.to_owned()),
        })
    };

    let riga = leggi_riga(connection, &brano.track_key)
        .ok_or_else(|| rifiuto("questo brano non ha un testo salvato"))?;
    if !riga.fonte.e_di_chi_ascolta() {
        return Err(rifiuto(
            "si restituisce solo quel che si è sincronizzato a mano",
        ));
    }
    let sincronizzato = riga
        .synced
        .filter(|lrc| !lrc.trim().is_empty())
        .ok_or_else(|| rifiuto("un testo senza tempi al catalogo non serve"))?;

    // Il piatto si **ricava** dai tempi invece di prendere quello che sta nella
    // riga: così è per costruzione lo stesso testo, parola per parola, e non
    // c'è modo di spedire un piatto che dice una cosa e un LRC che ne dice
    // un'altra. È anche l'unico piatto di cui si sa la provenienza.
    let letto = testo::leggi(&sincronizzato);
    if letto.righe.is_empty() {
        return Err(rifiuto("l'LRC salvato non ha nessuna riga con un tempo"));
    }
    let piatto = letto
        .righe
        .iter()
        .map(|riga| riga.testo.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let durata_ms = u64::try_from(brano.duration_ms)
        .ok()
        .filter(|d| *d > 0)
        .ok_or_else(|| rifiuto("del brano non si conosce la durata"))?;

    Ok(DaRestituire {
        titolo: brano.title,
        artista: brano.artist,
        album: brano.album,
        durata_ms,
        piatto,
        sincronizzato,
    })
}

/// Manda al catalogo quel che [`da_restituire`] ha preparato. **Non tocca il
/// database.**
///
/// Ci mette qualche secondo, e non per la rete: la prova di lavoro di LRCLIB è
/// spiegata in [`aether_meta::lrclib::pubblica`]. Chi chiama deve dirlo prima,
/// non dopo.
///
/// # Errori
///
/// `metadata.lyricsPublishRefused` se il catalogo dice di no, l'errore di
/// trasporto se la rete non risponde.
pub fn restituisci(fornitori: &Fornitori, cosa: &DaRestituire) -> Result<(), AppError> {
    lrclib::pubblica(
        fornitori,
        &lrclib::DaPubblicare {
            titolo: &cosa.titolo,
            artista: &cosa.artista,
            album: &cosa.album,
            durata_ms: cosa.durata_ms,
            piatto: &cosa.piatto,
            sincronizzato: &cosa.sincronizzato,
        },
    )
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::db;

    /// Le righe di prova non sono di nessuna canzone: quel che si prova qui è
    /// da dove arriva un testo, non quale sia.
    const LRC: &str = "[00:01.00]prima riga\n[00:02.00]seconda riga\n";

    fn libreria() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let aperta = db::open(&dir.path().join("prova.db")).expect("apertura");
        (dir, aperta.connection)
    }

    fn inserisci(connection: &Connection, percorso: &Path, lyrics: Option<&str>) -> i64 {
        connection
            .execute(
                "INSERT INTO tracks
                   (path, track_key, title, artist, album, duration_ms, file_size,
                    date_added, date_modified, lyrics)
                 VALUES (?1, 'chiave', 'Titolo', 'Artista', 'Album', 120000, 1, 0, 0, ?2)",
                rusqlite::params![percorso.display().to_string(), lyrics],
            )
            .expect("inserimento");
        connection.last_insert_rowid()
    }

    #[test]
    fn il_sidecar_vince_su_tutto() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
        let id = inserisci(&connection, &brano, Some("un testo nel tag"));

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Sidecar);
        assert_eq!(esito.testo.righe.len(), 2);
        // …e si è ricordato in tabella, così l'altro file dello stesso brano lo
        // trova senza avere il sidecar accanto.
        let riga = leggi_riga(&connection, "chiave").expect("riga");
        assert_eq!(riga.fonte, Fonte::Sidecar);
        assert_eq!(riga.synced.as_deref(), Some(LRC));
    }

    #[test]
    fn senza_sidecar_si_ripiega_sul_tag() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, Some("prima riga\nseconda riga"));

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Tag);
        assert!(!esito.testo.sincronizzato());
        assert_eq!(
            esito.testo.piatto.as_deref(),
            Some("prima riga\nseconda riga")
        );
    }

    #[test]
    fn un_lrc_dentro_il_tag_si_riconosce_da_se() {
        // Capita davvero: c'è chi mette un LRC intero dentro `USLT`. Non serve
        // un ramo apposta — il lettore del dominio vede i tempi e li usa.
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, Some(LRC));

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Tag);
        assert!(esito.testo.sincronizzato());
    }

    #[test]
    fn un_sidecar_senza_tempi_non_copre_i_tempi_che_si_hanno() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        std::fs::write(brano.with_extension("lrc"), "prima riga\nseconda riga").expect("sidecar");
        let id = inserisci(&connection, &brano, None);
        connection
            .execute(
                "INSERT INTO lyrics (track_key, synced, source, checked_at, updated_at)
                 VALUES ('chiave', ?1, 'lrclib', 1, 1)",
                rusqlite::params![LRC],
            )
            .expect("riga con i tempi");

        // Il primato del sidecar cede davanti all'unica cosa che il sidecar non
        // ha: i tempi.
        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Lrclib);
        assert_eq!(esito.testo.righe.len(), 2);
        // E soprattutto: non ci ha scritto sopra.
        let riga = leggi_riga(&connection, "chiave").expect("riga");
        assert_eq!(riga.synced.as_deref(), Some(LRC));
    }

    #[test]
    fn un_testo_senza_tempi_resta_da_chiedere() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, Some("prima riga\nseconda riga"));

        // Le parole ci sono, i tempi no, e al catalogo non gli si è ancora
        // chiesto niente: è il caso in cui il pannello deve andare a vedere.
        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Tag);
        assert!(esito.da_chiedere);

        // Quel che scorre invece non si chiede più.
        connection
            .execute(
                "UPDATE tracks SET lyrics = ?1 WHERE id = ?2",
                rusqlite::params![LRC, id],
            )
            .expect("tag con i tempi");
        connection
            .execute("DELETE FROM lyrics WHERE track_key = 'chiave'", [])
            .expect("riga via");
        let esito = per_brano(&connection, id).expect("testo");
        assert!(esito.testo.sincronizzato());
        assert!(!esito.da_chiedere);
    }

    #[test]
    fn la_coda_rimette_in_fila_chi_ha_solo_le_parole() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        inserisci(&connection, &brano, Some("prima riga"));
        // La riga che la catena scrive leggendo il tag: parole sì, tempi no, e
        // `checked_at` vuoto perché al catalogo non gli si è chiesto niente.
        connection
            .execute(
                "INSERT INTO lyrics (track_key, plain, source, updated_at)
                 VALUES ('chiave', 'prima riga', 'tag', 1)",
                [],
            )
            .expect("riga piatta");
        assert_eq!(
            da_cercare(&connection, 1_000, 10).expect("coda").len(),
            1,
            "le parole senza tempi non sono una domanda già fatta"
        );

        // Con i tempi non c'è più niente da chiedere, e non c'è scadenza che
        // lo rimetta in coda.
        connection
            .execute(
                "UPDATE lyrics SET synced = ?1 WHERE track_key = 'chiave'",
                rusqlite::params![LRC],
            )
            .expect("tempi");
        assert!(
            da_cercare(&connection, 1_000 + RIPROVA_MS + 1, 10)
                .expect("coda")
                .is_empty()
        );

        // Un «senza tempi» che il catalogo ha già confermato aspetta invece il
        // suo turno: `RIPROVA_MS`, non la prossima passata.
        connection
            .execute(
                "UPDATE lyrics SET synced = NULL, source = 'lrclib', checked_at = 1000
                 WHERE track_key = 'chiave'",
                [],
            )
            .expect("già chiesto");
        assert!(da_cercare(&connection, 1_000, 10).expect("coda").is_empty());
        assert_eq!(
            da_cercare(&connection, 1_000 + RIPROVA_MS + 1, 10)
                .expect("coda")
                .len(),
            1
        );
    }

    #[test]
    fn niente_da_nessuna_parte_e_una_risposta() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Nessuna);
        assert!(esito.testo.vuoto());
        // Non è ancora stato chiesto a nessuno: è questo che permette a chi
        // chiama di andare in rete.
        assert!(!esito.cercato);
    }

    #[test]
    fn quel_che_ha_scritto_l_utente_non_si_sovrascrive() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
        let id = inserisci(&connection, &brano, None);
        connection
            .execute(
                "INSERT INTO lyrics (track_key, synced, source, updated_at)
                 VALUES ('chiave', '[00:09.00]a mano', 'mano', 1)",
                [],
            )
            .expect("riga a mano");

        // Il sidecar si mostra — è il file su disco, ed è la verità —
        // ma la riga di `mano` resta quella che era.
        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Sidecar);
        let riga = leggi_riga(&connection, "chiave").expect("riga");
        assert_eq!(riga.fonte, Fonte::Mano);
        assert_eq!(riga.synced.as_deref(), Some("[00:09.00]a mano"));
    }

    #[test]
    fn uno_strumentale_non_manda_a_cercare_altro() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, Some("un testo nel tag"));
        connection
            .execute(
                "INSERT INTO lyrics (track_key, source, instrumental, checked_at, updated_at)
                 VALUES ('chiave', 'lrclib', 1, 1, 1)",
                [],
            )
            .expect("riga strumentale");

        let esito = per_brano(&connection, id).expect("testo");
        assert!(esito.testo.strumentale);
        assert!(esito.cercato);
    }

    #[test]
    fn l_aderenza_si_misura_sulla_durata_di_questo_file() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        // Il brano dura due minuti; questo testo finisce al secondo due.
        std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
        let id = inserisci(&connection, &brano, None);

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.aderenza, Aderenza::Sospetta);
    }

    #[test]
    fn lo_scarto_si_salva_e_si_rilegge() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        std::fs::write(brano.with_extension("lrc"), LRC).expect("sidecar");
        let id = inserisci(&connection, &brano, None);

        imposta_scarto(&connection, "chiave", -250).expect("scarto");
        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.scarto_ms, -250);
    }

    #[test]
    fn quel_che_si_sincronizza_finisce_accanto_al_brano() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);
        let riga = super::brano(&connection, id).expect("riga");

        salva_a_mano(&connection, &riga, LRC).expect("salvataggio");

        // Il file, che è la verità…
        let sidecar = brano.with_extension("lrc");
        assert_eq!(std::fs::read_to_string(&sidecar).expect("sidecar"), LRC);
        // …e la riga, che ne è la copia e dice di chi è.
        let salvata = leggi_riga(&connection, "chiave").expect("riga salvata");
        assert_eq!(salvata.fonte, Fonte::Mano);
        assert!(salvata.cercato, "non lo si cerca più in rete");

        // E si rilegge come sincronizzato.
        let esito = per_brano(&connection, id).expect("testo");
        assert!(esito.testo.sincronizzato());
        assert_eq!(esito.fonte, Fonte::Sidecar);
    }

    #[test]
    fn risincronizzare_sovrascrive_quel_che_c_era() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);
        let riga = super::brano(&connection, id).expect("riga");

        salva_a_mano(&connection, &riga, LRC).expect("primo");
        salva_a_mano(&connection, &riga, "[00:09.00]rifatto").expect("secondo");

        let salvata = leggi_riga(&connection, "chiave").expect("riga");
        assert_eq!(salvata.synced.as_deref(), Some("[00:09.00]rifatto"));
        assert_eq!(salvata.plain, None);
    }

    #[test]
    fn un_brano_che_non_c_e_lo_dice() {
        let (_dir, connection) = libreria();
        let errore = per_brano(&connection, 999).expect_err("nessun brano");
        assert_eq!(errore.code().kind().code(), "library.trackNotFound");
    }

    #[test]
    fn un_sidecar_illeggibile_e_una_fonte_in_meno_non_un_errore() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        // Byte che non sono UTF-8: si legge lo stesso quel che si può.
        std::fs::write(brano.with_extension("lrc"), b"[00:01.00]prima \xff riga").expect("sidecar");
        let id = inserisci(&connection, &brano, None);

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Sidecar);
        assert_eq!(esito.testo.righe.len(), 1);
    }

    #[test]
    fn si_restituisce_solo_quel_che_si_e_sincronizzato_a_mano() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);
        let quale = super::brano(&connection, id).expect("brano");

        // Quel che è arrivato dal catalogo torna indietro da dove è venuto:
        // niente.
        ricorda_esito(
            &connection,
            &quale,
            Some(&Voce {
                candidato: testo::Candidato {
                    id: 7,
                    titolo: "Titolo".to_owned(),
                    artista: "Artista".to_owned(),
                    album: Some("Album".to_owned()),
                    durata_ms: Some(120_000),
                    sincronizzato: true,
                    strumentale: false,
                },
                piatto: Some("prima riga\nseconda riga".to_owned()),
                sincronizzato: Some(LRC.to_owned()),
            }),
        )
        .expect("esito");

        let errore = da_restituire(&connection, id).expect_err("non è roba nostra");
        assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
    }

    #[test]
    fn il_piatto_che_si_manda_esce_dai_tempi_che_si_mandano() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        // Un testo nel tag che dice tutt'altro: se finisse nel `plainLyrics`
        // il catalogo riceverebbe due testi diversi nello stesso invio.
        let id = inserisci(&connection, &brano, Some("tutt'altre parole"));
        let quale = super::brano(&connection, id).expect("brano");
        salva_a_mano(&connection, &quale, LRC).expect("salvataggio");

        let cosa = da_restituire(&connection, id).expect("pronto");
        assert_eq!(cosa.piatto, "prima riga\nseconda riga");
        assert_eq!(cosa.sincronizzato, LRC);
        assert_eq!(cosa.durata_ms, 120_000);
    }

    #[test]
    fn un_brano_senza_testo_non_ha_niente_da_restituire() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);

        let errore = da_restituire(&connection, id).expect_err("non c'è niente");
        assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
    }
}
