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
//! 2. **I tag sincronizzati del file**, cioè `SYLT` (ID3v2) e `SYNCEDLYRICS`
//!    (Vorbis e APE): un LRC intero dentro il file musicale. Li legge
//!    [`tag_sincronizzato`], **non** la scansione — vedi là il perché — e li si
//!    guarda solo quando non si ha già qualcosa che scorre.
//! 3. **La riga in `lyrics`**, che è dove vivono il testo scaricato e quello
//!    sincronizzato a mano. È indicizzata su `track_key`: lo stesso brano in due
//!    formati diversi condivide una riga sola, e spostare il file non la perde.
//! 4. **Il tag piatto del file**, cioè `tracks.lyrics`, che la scansione riempie
//!    da `USLT`/`©lyr`/`LYRICS`. Ultimo perché è quasi sempre senza tempi — e
//!    quando invece i tempi ce li ha, perché qualcuno ci ha messo dentro un LRC
//!    intero, [`aether_domain::testo::leggi`] se ne accorge da sé.
//!
//! Quel che si trova nelle fonti 1, 2 e 4 si copia nella 3, che da lì in poi fa
//! da cache. Quel che sta nella 3 non torna mai indietro da solo: scrivere nei
//! file di qualcun altro è un gesto separato, e in questo modulo non c'è.
//!
//! # Quattro esiti, e nessuna schermata bianca
//!
//! Ogni brano finisce in uno di quattro stati — sincronizzato, piatto,
//! strumentale, niente — e il quarto è l'unico che chiede qualcosa a chi guarda.
//! È la ragione per cui [`Fonte::Nessuna`] esiste come valore invece che come
//! `Option`: «non ho ancora guardato» e «ho guardato e non c'è» sono due cose,
//! e la seconda va ricordata.

use std::path::{Path, PathBuf};

use aether_domain::enrich::{GRAZIA_MS, titolo_da_cercare};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::indizi::e_segnaposto;
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
    ///
    /// È anche il valore **neutro** della colonna `source`: una riga di
    /// `lyrics` può esistere prima di qualunque testo — la scrive
    /// [`imposta_scarto`] quando si tocca il cursore della correzione a testo
    /// non ancora arrivato — e in quel momento la provenienza non c'è perché
    /// non c'è niente di cui dirla. Vale la stringa vuota, che `ricorda` e
    /// [`ricorda_esito`] sovrascrivono come sovrascrivono `sidecar`, `tag` e
    /// `lrclib`: quel che non si può toccare è `mano`, e solo quello.
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
    // cercare altrove, ed è la riga in tabella l'unica che possa saperlo.
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

    // ── 2. i tag sincronizzati del file ─────────────────────────────────────
    // `SYLT` e `SYNCEDLYRICS`: un LRC intero dentro il file musicale. Non lo
    // legge la scansione — quella tocca decine di migliaia di file e aprire
    // ognuno una seconda volta per un frammento che quasi nessuno scrive
    // costerebbe minuti su ogni libreria — quindi lo si legge qui, un file alla
    // volta, e **solo quando non si ha già qualcosa che scorre**: chi ha già i
    // tempi non ha niente da guadagnare da questa apertura.
    //
    // Sta prima della riga in tabella e dopo il sidecar per la stessa ragione
    // per cui il tag piatto sta ultimo: il file dell'utente viene prima della
    // copia, ma un sidecar è un gesto e un tag è quel che c'era nel file
    // comprato. Quel che si trova si ricorda in tabella, così l'apertura si
    // paga una volta sola.
    if !riga.as_ref().is_some_and(Riga::ha_i_tempi)
        && let Some(grezzo) = tag_sincronizzato(Path::new(&brano.path))
    {
        let testo = testo::leggi(&grezzo);
        // Solo se i tempi ci sono davvero: un `SYLT` mal scritto che si legge
        // come testo piatto non vale l'apertura, e soprattutto non vale una
        // riscrittura della riga.
        if testo.sincronizzato() {
            if !riga.as_ref().is_some_and(|r| r.fonte.e_di_chi_ascolta()) {
                ricorda(connection, brano, &grezzo, &testo, Fonte::Tag);
            }
            return finisci(testo, Fonte::Tag, scarto_ms, durata, cercato);
        }
    }

    // ── 3. la riga in tabella ───────────────────────────────────────────────
    if let Some(riga) = riga
        && let Some(grezzo) = riga.grezzo()
    {
        let testo = testo::leggi(grezzo);
        if !testo.vuoto() {
            let mut esito = finisci(testo, riga.fonte, scarto_ms, durata, cercato);
            // È l'unico ramo in cui il confronto ha senso: qui il testo arriva
            // da una riga scritta **per un altro momento**, e la durata che le
            // sta accanto dice per quale. Nei rami del sidecar e del tag il
            // testo esce dal file che si sta ascoltando, quindi confrontarlo
            // con sé stesso non direbbe niente — e `ricorda` ha appena scritto
            // in colonna la durata di questo file.
            if esito.aderenza == Aderenza::Buona && riga.altra_edizione(durata) {
                esito.aderenza = Aderenza::Sospetta;
            }
            return esito;
        }
    }

    // ── 4. il tag piatto ────────────────────────────────────────────────────
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

// ── i tag sincronizzati ─────────────────────────────────────────────────────

/// Il nome sotto cui Vorbis e APE tengono un LRC.
///
/// Non è uno standard: è la convenzione che hanno preso i taggatori — Mp3tag,
/// MusicBee, foobar2000 con il suo componente — e che i lettori che i tempi li
/// mostrano vanno a cercare. `UNSYNCEDLYRICS` è l'altro, ed è il testo piatto:
/// quello lo legge già la scansione, mappato su `lofty::prelude::ItemKey`.
const CHIAVE_SINCRONIZZATA: &str = "SYNCEDLYRICS";

/// L'LRC che sta dentro il file musicale, se ce n'è uno.
///
/// # Perché non lo legge la scansione
///
/// Perché la scansione tocca decine di migliaia di file, e leggere questi
/// frammenti costa un'apertura in più per file — su una libreria vera sono
/// minuti, spesi su un tag che quasi nessuno scrive. Qui invece si apre **un**
/// file, quello che si sta guardando, e quel che si trova finisce in tabella:
/// l'apertura si paga una volta per brano, non una per scansione.
///
/// # Le due forme, e perché servono due strade
///
/// * **`SYLT`** è un frame ID3v2 binario, e in `lofty` 0.24 **non entra nel
///   `Tag` unificato**: arriva come `Frame::Binary` e va interpretato a mano
///   con `SynchronizedTextFrame::parse`. Vive solo dove vive ID3v2, cioè
///   mp3, aiff e wav (più, fuori standard, flac e mpc).
/// * **`SYNCEDLYRICS`** è una chiave di testo qualunque dentro Vorbis o APE, e
///   nemmeno lei attraversa il `Tag` unificato: `ItemKey` è un'enumerazione
///   chiusa e una chiave che non le corrisponde non ci finisce. Si legge dal
///   tag concreto, che è l'unico che le chiavi le tiene tutte.
///
/// Da qui la forma di questa funzione: si apre il file **come quel che è**
/// invece di passare da `Probe::read`, perché quel che serve sta esattamente
/// nei pezzi che la lettura generica butta via.
///
/// Non fallisce mai: un file che non si apre, un frame illeggibile, un formato
/// che questi tag non li ha, sono tutti «niente da qui», che è una fonte in
/// meno e non un errore. È la stessa disciplina di [`leggi_sidecar`].
///
/// # Dove non va chiamata
///
/// Apre un file che può stare su una condivisione di rete, quindi vale la
/// regola di `crate::testi`: questa catena non sta sul filo che disegna la
/// finestra. Non allarga però la superficie del problema, e vale la pena
/// dirlo: chi arriva fin qui su una share morta si è già fermato una riga più
/// su, dentro [`leggi_sidecar`], che apre un file nella stessa cartella un
/// istante prima.
#[must_use]
pub fn tag_sincronizzato(percorso: &Path) -> Option<String> {
    use lofty::config::ParseOptions;
    use lofty::file::{AudioFile as _, FileType};

    let file = std::fs::File::open(percorso).ok()?;
    let mut lettore = std::io::BufReader::new(file);
    // Il tipo dal contenuto e non dall'estensione: un `.mp3` che dentro è un
    // flac esiste, e sbagliare qui vorrebbe dire non trovare niente su un file
    // che il testo ce l'ha.
    //
    // La sonda sta in un blocco suo perché tiene il prestito del lettore, e il
    // lettore serve subito dopo per la lettura vera.
    let tipo = {
        let sonda = lofty::probe::Probe::new(&mut lettore)
            .guess_file_type()
            .ok()?;
        sonda.file_type()?
    };
    std::io::Seek::rewind(&mut lettore).ok()?;

    // Le proprietà audio non si leggono: sono la parte cara — su un VBR lungo
    // vuol dire percorrere il file intero — e qui non servono a niente.
    let opzioni = ParseOptions::new().read_properties(false);
    match tipo {
        FileType::Mpeg => {
            let letto = lofty::mpeg::MpegFile::read_from(&mut lettore, opzioni).ok()?;
            da_id3v2(letto.id3v2()).or_else(|| da_ape(letto.ape()))
        }
        FileType::Aiff => {
            let letto = lofty::iff::aiff::AiffFile::read_from(&mut lettore, opzioni).ok()?;
            da_id3v2(letto.id3v2())
        }
        FileType::Wav => {
            let letto = lofty::iff::wav::WavFile::read_from(&mut lettore, opzioni).ok()?;
            da_id3v2(letto.id3v2())
        }
        FileType::Flac => {
            let letto = lofty::flac::FlacFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(letto.vorbis_comments()).or_else(|| da_id3v2(letto.id3v2()))
        }
        FileType::Vorbis => {
            let letto = lofty::ogg::VorbisFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(Some(letto.vorbis_comments()))
        }
        FileType::Opus => {
            let letto = lofty::ogg::OpusFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(Some(letto.vorbis_comments()))
        }
        FileType::Speex => {
            let letto = lofty::ogg::SpeexFile::read_from(&mut lettore, opzioni).ok()?;
            da_vorbis(Some(letto.vorbis_comments()))
        }
        FileType::Ape => {
            let letto = lofty::ape::ApeFile::read_from(&mut lettore, opzioni).ok()?;
            da_ape(letto.ape())
        }
        FileType::WavPack => {
            let letto = lofty::wavpack::WavPackFile::read_from(&mut lettore, opzioni).ok()?;
            da_ape(letto.ape())
        }
        FileType::Mpc => {
            let letto = lofty::musepack::MpcFile::read_from(&mut lettore, opzioni).ok()?;
            da_ape(letto.ape()).or_else(|| da_id3v2(letto.id3v2()))
        }
        // Mp4 e aac non hanno un posto concordato dove mettere un LRC: `©lyr`
        // è il testo piatto, e la scansione lo legge già. Inventarne uno qui
        // vorrebbe dire cercare una chiave che nessun taggatore scrive.
        _ => None,
    }
}

/// L'LRC dal frame `SYLT` di un tag ID3v2.
fn da_id3v2(tag: Option<&lofty::id3::v2::Id3v2Tag>) -> Option<String> {
    use lofty::id3::v2::{Frame, FrameFlags, FrameId, SynchronizedTextFrame};

    let frame = tag?.get(&FrameId::Valid(std::borrow::Cow::Borrowed("SYLT")))?;
    let Frame::Binary(binario) = frame else {
        return None;
    };
    let letto = SynchronizedTextFrame::parse(&binario.data, FrameFlags::default()).ok()?;
    lrc_da_sylt(&letto)
}

/// Il testo di `SYNCEDLYRICS` in un tag Vorbis.
fn da_vorbis(tag: Option<&lofty::ogg::VorbisComments>) -> Option<String> {
    tag?.get(CHIAVE_SINCRONIZZATA)
        .filter(|t| !t.trim().is_empty())
        .map(ToOwned::to_owned)
}

/// Il testo di `SYNCEDLYRICS` in un tag APE.
fn da_ape(tag: Option<&lofty::ape::ApeTag>) -> Option<String> {
    match tag?.get(CHIAVE_SINCRONIZZATA)?.value() {
        lofty::tag::ItemValue::Text(testo) if !testo.trim().is_empty() => Some(testo.clone()),
        _ => None,
    }
}

/// Un `SYLT` riscritto come LRC.
///
/// # I due modi in cui un `SYLT` è scritto, e come si distinguono
///
/// Lo standard ID3v2 dice che il testo è **sillabato**: ogni voce è un
/// frammento, e una nuova riga comincia dove il frammento porta un a capo in
/// testa. Quasi nessuno lo scrive così: i taggatori veri mettono una riga
/// intera per voce, e l'a capo non lo scrivono affatto.
///
/// Trattarli allo stesso modo rompe uno dei due — o si ottiene una canzone su
/// una riga sola, o duecento righe di due sillabe. Quindi si guarda: se
/// **almeno una** voce comincia con un a capo, il file è sillabato e si
/// ricompongono le righe; se nessuna lo fa, ogni voce **è** una riga. Non è
/// un'euristica su cosa il testo sembri: è la sola cosa che il formato lascia
/// dire a chi scrive.
///
/// I tempi in fotogrammi MPEG si scartano: convertirli vorrebbe sapere la
/// durata del fotogramma, cioè aver già decodificato il file, e sbagliarla
/// darebbe un testo che scorre a una velocità inventata. Meglio nessun testo
/// che un testo storto, che è la regola di tutto questo modulo.
fn lrc_da_sylt(frame: &lofty::id3::v2::SynchronizedTextFrame<'_>) -> Option<String> {
    use lofty::id3::v2::{SyncTextContentType, TimestampFormat};

    if frame.timestamp_format != TimestampFormat::MS {
        return None;
    }
    // Un `SYLT` può portare accordi, titoli di movimento o didascalie di scena:
    // sono tutte cose sincronizzate, e nessuna è il testo della canzone.
    if !matches!(
        frame.content_type,
        SyncTextContentType::Lyrics | SyncTextContentType::TextTranscription
    ) {
        return None;
    }

    let sillabato = frame
        .content
        .iter()
        .any(|(_, pezzo)| pezzo.starts_with('\n') || pezzo.starts_with('\r'));

    let mut righe: Vec<testo::Riga> = Vec::new();
    for (ms, pezzo) in &frame.content {
        let pulito = pezzo.trim_matches(['\n', '\r']);
        let apre = !sillabato || pezzo.starts_with('\n') || pezzo.starts_with('\r');
        match righe.last_mut() {
            Some(ultima) if !apre => ultima.testo.push_str(pulito),
            _ => righe.push(testo::Riga {
                ms: *ms,
                testo: pulito.to_owned(),
                parole: Vec::new(),
            }),
        }
    }
    // Le righe vuote in coda e in testa non dicono niente, e una raccolta di
    // sole righe vuote non è un testo.
    if righe.iter().all(|r| r.testo.trim().is_empty()) {
        return None;
    }

    // Si ricompone passando da `scrivi`, che è l'inverso provato di `leggi`:
    // così questo tag entra nel resto del programma sotto la stessa forma di un
    // `.lrc` sul disco, e non c'è un secondo formato interno da mantenere.
    Some(testo::scrivi(&Testo {
        righe,
        piatto: None,
        strumentale: false,
        offset_ms: 0,
    }))
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
    /// La durata del file su cui questi tempi sono stati battuti.
    ///
    /// È la colonna `duration_ms` di `011_testi.sql`, che esisteva da quella
    /// migrazione e non era mai stata riletta: si scriveva a ogni `UPSERT` e
    /// nessuna `SELECT` la prendeva, quindi la promessa scritta nel suo
    /// commento — accorgersi che il testo è giusto ma l'edizione è un'altra —
    /// non era mantenuta. La mantiene [`per_questo_brano`], che la confronta
    /// con la durata del file che si sta ascoltando.
    ///
    /// `None` per le righe scritte prima che qualcuno la leggesse, e per
    /// quelle nate da [`imposta_scarto`], che di testo non ne ha ancora uno.
    pub durata_ms: Option<u64>,
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

    /// Questi tempi sono stati battuti su un'edizione che dura un altro tanto.
    ///
    /// # Cosa aggiunge a [`aether_domain::testo::verifica_durata`]
    ///
    /// Quella guarda **dove finisce l'ultima riga**, e prende il caso in cui il
    /// testo sborda o si ferma molto prima. Non prende il caso più insidioso: un
    /// remaster che dura venti secondi in più dell'originale, con lo stesso
    /// numero di strofe e l'ultima riga comodamente dentro tutt'e due le durate.
    /// Lì l'aderenza è «buona» e il testo scorre sempre più in ritardo, e chi
    /// guarda dà la colpa al lettore.
    ///
    /// Il termine di paragone è la durata scritta accanto al testo quando lo si
    /// è preso — [`Self::durata_ms`] — contro la durata del file di adesso.
    /// Sopra [`GRAZIA_MS`], cioè dieci secondi, non è più la stessa incisione.
    /// Sotto, sono i mezzi secondi di silenzio che ogni codificatore aggiunge a
    /// modo suo, e segnalarli vorrebbe dire una fascia gialla su tutta la
    /// libreria.
    ///
    /// Falso quando una delle due durate non si conosce: «non si sa» non è «è
    /// sbagliato», ed è la stessa regola di `verifica_durata`.
    #[must_use]
    pub fn altra_edizione(&self, durata_ms: u64) -> bool {
        let Some(sua) = self.durata_ms.filter(|d| *d > 0) else {
            return false;
        };
        durata_ms > 0 && sua.abs_diff(durata_ms) > GRAZIA_MS
    }
}

/// La riga di `lyrics` per questa chiave, se c'è.
///
/// Inghiotte i guasti: perdere questa lettura costa una fonte, e propagare
/// l'errore costerebbe il testo che le altre fonti avrebbero dato.
#[must_use]
pub fn leggi_riga(connection: &Connection, track_key: &str) -> Option<Riga> {
    connection
        .prepare_cached(
            "SELECT synced, plain, source, offset_ms, instrumental, checked_at, duration_ms
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
                // Un numero negativo in colonna non è una durata: vale come
                // «non si sa», che è l'unico modo onesto di trattarlo.
                durata_ms: row
                    .get::<_, Option<i64>>(6)?
                    .and_then(|d| u64::try_from(d).ok()),
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
/// # La riga che nasce per lo scarto non ha una provenienza
///
/// Questo `INSERT` può creare la riga **prima** che di testo ce ne sia uno: il
/// cursore della correzione è raggiungibile appena ci sono delle righe, e
/// basta toccarlo mentre il catalogo sta ancora rispondendo. In quel momento la
/// provenienza è [`Fonte::Nessuna`], cioè la stringa vuota, e non `mano`.
///
/// La differenza non è di forma. `mano` vuol dire «l'ha scritto chi ascolta», e
/// sia `ricorda` sia [`ricorda_esito`] lo rispettano con un `WHERE
/// lyrics.source <> 'mano'` dentro l'`UPSERT` — nel database, non in un `if`.
/// Scriverlo qui avrebbe voluto dire dichiarare fatto a mano un testo che non
/// esiste, e da quel momento nessuna delle due funzioni avrebbe più scritto
/// niente per quel brano: né il sidecar, né il tag, né la risposta del
/// catalogo. Un testo mai più, e nessun modo di accorgersene — perché il
/// sintomo è un pannello vuoto, che è esattamente quel che si vede anche quando
/// il testo non c'è per davvero.
///
/// Lo scarto invece sopravvive comunque: `source` non si tocca sul ramo di
/// conflitto, quindi il `mano` di chi ha sincronizzato a mano resta `mano`, e la
/// stringa vuota lascia la riga aperta a chi arriverà dopo.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura non riesce. Qui l'errore si propaga, al
/// contrario che in `ricorda`: è un gesto esplicito di chi ascolta, e uno
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
             VALUES (?1, ?4, ?2, ?3)
             ON CONFLICT(track_key) DO UPDATE SET
               offset_ms = excluded.offset_ms,
               updated_at = excluded.updated_at",
            // Il nome viene dall'enumerazione e non scritto a mano nella `SQL`:
            // il valore neutro è una cosa sola, e se cambia deve cambiare in un
            // posto solo.
            rusqlite::params![track_key, scarto_ms, now_ms(), Fonte::Nessuna.nome()],
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
    con_ritentativi(|| {
        let (titolo, album, durata_ms) = da_chiedere_al_catalogo(brano);
        lrclib::cerca(
            fornitori,
            &Cercato {
                titolo: &titolo,
                artista: &brano.artist,
                album,
                durata_ms,
            },
        )
    })
}

/// Come [`cerca_in_rete`], ma senza fidarsi di quel che il deposito ricorda.
///
/// La chiama il ritentativo esplicito, cioè il pulsante «Cerca di nuovo». Il
/// perché per esteso sta su [`aether_meta::lrclib::cerca_di_nuovo`]; qui basta
/// dire che chi preme quel pulsante ha davanti un pannello vuoto, e rileggere
/// la risposta di ieri gli darebbe lo stesso pannello vuoto.
///
/// # Errori
///
/// Gli stessi di [`cerca_in_rete`].
pub fn cerca_di_nuovo_in_rete(
    fornitori: &Fornitori,
    brano: &BranoDaTestare,
) -> Result<Option<Voce>, AppError> {
    con_ritentativi(|| {
        let (titolo, album, durata_ms) = da_chiedere_al_catalogo(brano);
        lrclib::cerca_di_nuovo(
            fornitori,
            &Cercato {
                titolo: &titolo,
                artista: &brano.artist,
                album,
                durata_ms,
            },
        )
    })
}

/// Con che cosa si interroga il catalogo per questo brano.
///
/// # Il titolo si ripulisce prima di chiedere
///
/// [`titolo_da_cercare`] esisteva da quando esiste l'arricchimento e qui non
/// era mai stata usata: si mandava `title` grezzo, quindi «Poetica (Official
/// Video)» — cioè come si chiama il file di chi la sua libreria l'ha costruita
/// scaricando — arrivava così com'era a un catalogo che quella voce non ce
/// l'ha e non ce l'avrà mai.
///
/// Toglie solo il rumore di **pubblicazione**: i gruppi fra parentesi fatti
/// interamente di parole come «official», «video», «lyrics», e la coda `-
/// Topic` dei canali che YouTube genera da sé. `(Live)`, `- Remastered` e
/// `feat.` **restano**, ed è giusto: cercare la versione dal vivo di un file
/// dal vivo è quel che si vuole, e toglierlo dalla domanda farebbe trovare lo
/// studio. La regola sta scritta per esteso sulla costante `RUMORE_DI_CARICAMENTO`.
///
/// # Cosa non cambia, e perché il rischio è piccolo
///
/// Né la chiave con cui la risposta si ricorda né i veti di
/// [`aether_domain::testo::scegli`]: tutt'e due passano da
/// `normalize_for_match`, che i gruppi fra parentesi li toglie **tutti** già
/// da prima. Il peggio che può succedere è quindi una domanda esatta che
/// fallisce su un titolo che il catalogo conosceva per intero, cioè una
/// richiesta in più — e la domanda generosa, che è quella che poi decide, ci
/// guadagna sempre.
///
/// Ripiego sul grezzo se la ripulitura non lascia niente: un brano che si
/// chiama davvero «Video» esiste, e cercarne la stringa vuota non è cercare.
fn da_chiedere_al_catalogo(brano: &BranoDaTestare) -> (String, Option<&str>, Option<u64>) {
    let pulito = titolo_da_cercare(&brano.title);
    let titolo = if pulito.trim().is_empty() {
        brano.title.clone()
    } else {
        pulito
    };
    let album = brano.album.trim();
    (
        titolo,
        (!album.is_empty()).then_some(album),
        u64::try_from(brano.duration_ms).ok().filter(|d| *d > 0),
    )
}

/// Quanto si aspetta prima del secondo e del terzo tentativo.
///
/// Mezzo secondo e un secondo e mezzo. Il primo prende il caso che capita più
/// spesso di tutti — il wifi che si riaggancia, la connessione che cade sul
/// primo pacchetto — e il secondo dà tempo a un servizio che sta ripartendo.
/// Un quarto tentativo non c'è: oltre i due secondi non è più un inciampo, è
/// «adesso non si può», e insistere vorrebbe dire tenere fermo il pannello del
/// testo — o la passata — su un guasto che non passerà da solo.
const RITENTATIVI_MS: [u64; 2] = [500, 1500];

/// Ripete una domanda al catalogo quando il guasto è di quelli che passano.
///
/// # Cosa si ritenta, e cosa no
///
/// Solo `is_retryable`: un `400` è una domanda scritta male da noi e rifarla
/// tre volte darebbe tre volte lo stesso `400`.
///
/// E **mai** l'interruttore aperto, che pure si dichiara ritentabile. Quel
/// codice non descrive un guasto: descrive la [`aether_meta::Cadenza`] che ha
/// **già** deciso di non parlare con quel servizio per qualche minuto, e
/// riprovare significa ricevere lo stesso rifiuto in un microsecondo, tre
/// volte, dopo due secondi di attesa buttati. Il ritentativo che serve lì è
/// quello dell'interruttore, non il nostro.
///
/// `Ok(None)` — «il catalogo non ce l'ha» — non è un guasto e non si ripete:
/// è una risposta, ed è arrivata.
fn con_ritentativi<F>(mut chiedi: F) -> Result<Option<Voce>, AppError>
where
    F: FnMut() -> Result<Option<Voce>, AppError>,
{
    let mut fatti = 0_usize;
    loop {
        let err = match chiedi() {
            Ok(voce) => return Ok(voce),
            Err(err) => err,
        };
        let ancora = err.is_retryable()
            && !matches!(err.code(), ErrorCode::NetCircuitOpen { .. })
            && fatti < RITENTATIVI_MS.len();
        let Some(attesa) = RITENTATIVI_MS.get(fatti).copied().filter(|_| ancora) else {
            return Err(err);
        };
        std::thread::sleep(std::time::Duration::from_millis(attesa));
        fatti = fatti.saturating_add(1);
    }
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

/// Dimentica di aver già chiesto al catalogo per questo brano.
///
/// Azzera `checked_at`, e **solo** quello: il testo che c'è resta, la
/// correzione di chi ascolta resta, la provenienza resta. Quel che se ne va è
/// la frase «a questo brano si è già chiesto», che è ciò che tiene un brano
/// fuori dalla coda della passata per [`RIPROVA_MS`] — quattordici giorni.
///
/// Serve al ritentativo esplicito, ed è la metà locale del gesto: l'altra metà
/// è saltare il deposito, che sta in [`cerca_di_nuovo_in_rete`]. Servono tutt'e
/// due, perché ricordano due cose diverse in due posti diversi — il deposito
/// ricorda la *risposta*, questa colonna ricorda la *domanda* — e sanarne una
/// sola lascerebbe il pulsante senza effetto.
///
/// Tocca anche la riga di `mano`, al contrario degli `UPSERT` di questo modulo:
/// lì la protezione serve a non riscrivere il testo di chi ascolta, e qui il
/// testo non si tocca. Non ha comunque conseguenze — una riga con i tempi in
/// coda non ci torna — ma la regola va detta invece che dedotta.
///
/// # Errori
///
/// `db.queryFailed`. Si propaga: un ritentativo che crede di aver dimenticato
/// senza averlo fatto rifà la stessa domanda alla stessa cache.
pub fn dimentica_esito(connection: &Connection, track_key: &str) -> Result<(), AppError> {
    connection
        .execute(
            "UPDATE lyrics SET checked_at = NULL WHERE track_key = ?1",
            [track_key],
        )
        .map(|_| ())
        .map_err(|err| db_error("azzeramento della memoria di una ricerca", &err))
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
/// # Le sentinelle non escono di qui
///
/// Un file senza tag `artist` non ha un artista vuoto: la scansione ci scrive
/// `aether_domain::album::UNKNOWN_ARTIST`, cioè «Artista sconosciuto»
/// (`library.rs`, dentro `read_track`). È un segnaposto utile in casa — tiene
/// insieme la vista Artisti — e in un catalogo pubblico è una voce che nessuno
/// troverà mai più, perché nessuno cercherà il testo di «Artista sconosciuto».
/// Quel contributo non è rumore: è un contributo **perduto**, e lo scopre solo
/// chi va a guardare sul sito.
///
/// Si rifiuta quindi su **titolo** e **artista**, che sono l'identità con cui il
/// catalogo indicizza e con cui ogni altra persona ritroverà quel testo. Il
/// controllo passa da [`aether_domain::indizi::e_segnaposto`] e non da un
/// confronto con le due sentinelle: sul disco di chi ascolta i file arrivano già
/// taggati da qualcun altro, e «Unknown Artist», «Various Artists», `<unknown>`,
/// «Traccia 03» sono esattamente la stessa assenza scritta in un'altra lingua.
/// Il confronto è sul **valore**, piegato e senza spazi doppi, non su una
/// traduzione dell'interfaccia.
///
/// L'**album** invece non si rifiuta, si **svuota**. È facoltativo per il
/// catalogo — `cerca` lo omette già dall'URL quando non c'è — e un singolo senza
/// album esiste per davvero: rifiutare lì vorrebbe dire che un brano fuori da
/// ogni disco non si può contribuire, cioè perdere una funzione per proteggere un
/// campo che il catalogo accetta vuoto. Quel che non deve uscire è la stringa
/// «Album sconosciuto», e a non farla uscire basta non scriverla.
///
/// Il costo, dichiarato: un disco che si chiama davvero «Untitled 3» passa per
/// segnaposto e si rifiuta. È un rifiuto con la ragione scritta, non una perdita,
/// e si risolve dando un titolo al brano — al contrario del verso opposto, che si
/// risolve solo scrivendo a chi tiene il catalogo.
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

    // L'identità con cui il catalogo indicizzerà questo testo. Il perché del
    // rifiuto sta per esteso qui sopra: un segnaposto non è un nome sbagliato,
    // è un contributo che nessuno ritroverà.
    if e_segnaposto(Some(&brano.title)) {
        return Err(rifiuto("di questo brano non si conosce il titolo"));
    }
    if e_segnaposto(Some(&brano.artist)) {
        return Err(rifiuto("di questo brano non si conosce l'artista"));
    }
    // L'album si svuota invece di far cadere tutto: è facoltativo per il
    // catalogo, e un brano fuori da ogni disco ha diritto di essere contribuito.
    let album = if e_segnaposto(Some(&brano.album)) {
        String::new()
    } else {
        brano.album
    };

    Ok(DaRestituire {
        titolo: brano.title,
        artista: brano.artist,
        album,
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
    fn uno_scarto_messo_prima_del_testo_non_blocca_il_catalogo() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);
        let quale = super::brano(&connection, id).expect("brano");

        // Il cursore della correzione si tocca mentre il catalogo sta ancora
        // rispondendo: la riga nasce qui, e non ha ancora nessun testo.
        imposta_scarto(&connection, "chiave", -300).expect("scarto");
        let appena_nata = leggi_riga(&connection, "chiave").expect("riga");
        assert_eq!(
            appena_nata.fonte,
            Fonte::Nessuna,
            "una riga senza testo non è stata scritta da nessuno"
        );

        // E la risposta del catalogo, quando arriva, si scrive: era il difetto
        // — con `source = 'mano'` la `WHERE` dell'`UPSERT` non lasciava
        // scrivere più niente, e quel brano restava senza testo per sempre.
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

        let dopo = leggi_riga(&connection, "chiave").expect("riga");
        assert_eq!(dopo.fonte, Fonte::Lrclib);
        assert_eq!(dopo.synced.as_deref(), Some(LRC));
        // E lo scarto che si era messo è ancora quello: `source` cambia, la
        // correzione di chi ascolta no.
        assert_eq!(dopo.scarto_ms, -300);

        let esito = per_brano(&connection, id).expect("testo");
        assert_eq!(esito.fonte, Fonte::Lrclib);
        assert_eq!(esito.scarto_ms, -300);
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

    /// Un brano sincronizzato a mano e pronto da pubblicare, così le prove che
    /// seguono possono guastare **un** campo alla volta.
    fn pronto_da_pubblicare(dir: &tempfile::TempDir, connection: &Connection) -> i64 {
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(connection, &brano, None);
        let quale = super::brano(connection, id).expect("brano");
        salva_a_mano(connection, &quale, LRC).expect("salvataggio");
        id
    }

    #[test]
    fn non_si_restituisce_un_brano_senza_artista() {
        let (dir, connection) = libreria();
        let id = pronto_da_pubblicare(&dir, &connection);
        // Così com'è, si pubblica: è il termine di paragone di tutto il resto.
        da_restituire(&connection, id).expect("pronto");

        // Quel che la scansione scrive quando il tag `artist` non c'è.
        for assente in [
            aether_domain::album::UNKNOWN_ARTIST,
            // E le stesse assenze scritte da un taggatore altrui: il confronto
            // è sul valore, non su una traduzione.
            "Unknown Artist",
            "Various Artists",
            "<unknown>",
            "   ",
        ] {
            connection
                .execute(
                    "UPDATE tracks SET artist = ?1 WHERE id = ?2",
                    rusqlite::params![assente, id],
                )
                .expect("artista assente");
            let errore = da_restituire(&connection, id).err().unwrap_or_else(|| {
                panic!("«{assente}» non è un artista da mandare in un catalogo pubblico")
            });
            assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
        }
    }

    #[test]
    fn non_si_restituisce_un_brano_senza_titolo() {
        let (dir, connection) = libreria();
        let id = pronto_da_pubblicare(&dir, &connection);

        // `Traccia 01` è il ripiego dal nome del file, non un titolo.
        for assente in ["Traccia 01", "Unknown Title", "Untitled", ""] {
            connection
                .execute(
                    "UPDATE tracks SET title = ?1 WHERE id = ?2",
                    rusqlite::params![assente, id],
                )
                .expect("titolo assente");
            let errore = da_restituire(&connection, id)
                .err()
                .unwrap_or_else(|| panic!("«{assente}» non è un titolo"));
            assert_eq!(errore.code().kind().code(), "metadata.lyricsPublishRefused");
        }
    }

    #[test]
    fn un_album_sconosciuto_si_manda_vuoto_invece_di_far_cadere_tutto() {
        let (dir, connection) = libreria();
        let id = pronto_da_pubblicare(&dir, &connection);
        connection
            .execute(
                "UPDATE tracks SET album = ?1 WHERE id = ?2",
                rusqlite::params![aether_domain::album::UNKNOWN_ALBUM, id],
            )
            .expect("album assente");

        // Un singolo fuori da ogni disco si contribuisce comunque: l'album è
        // facoltativo per il catalogo. Quel che non deve uscire è la sentinella.
        let cosa = da_restituire(&connection, id).expect("si pubblica comunque");
        assert_eq!(cosa.album, "");
        assert_eq!(cosa.artista, "Artista");
        assert_eq!(cosa.titolo, "Titolo");
    }

    #[test]
    fn una_durata_diversa_rende_sospetto_un_testo_che_ci_sta_dentro() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        let id = inserisci(&connection, &brano, None);

        // Il file dura due minuti; questo testo finisce a un minuto e
        // cinquanta, cioè comodamente dentro: `verifica_durata` non ha niente
        // da ridire, ed è appunto il caso che non prendeva nessuno.
        let dentro = "[00:10.00]prima riga\n[01:50.00]ultima riga\n";
        connection
            .execute(
                "INSERT INTO lyrics (track_key, synced, source, duration_ms, checked_at, updated_at)
                 VALUES ('chiave', ?1, 'lrclib', 120000, 1, 1)",
                rusqlite::params![dentro],
            )
            .expect("riga della stessa edizione");
        assert_eq!(
            per_brano(&connection, id).expect("testo").aderenza,
            Aderenza::Buona,
            "stessa durata: non c'è niente da segnalare"
        );

        // La stessa riga, ma battuta su un'incisione che dura venticinque
        // secondi in più. I tempi ci stanno dentro lo stesso, e il testo
        // scorrerà sempre più in ritardo: è quel che la colonna `duration_ms`
        // di `011_testi.sql` prometteva di far notare.
        connection
            .execute(
                "UPDATE lyrics SET duration_ms = 145000 WHERE track_key = 'chiave'",
                [],
            )
            .expect("altra edizione");
        assert_eq!(
            per_brano(&connection, id).expect("testo").aderenza,
            Aderenza::Sospetta
        );

        // Mezzo secondo di differenza invece no: è quel che separa due
        // codifiche dello stesso master, e segnalarlo tingerebbe di giallo
        // mezza libreria.
        connection
            .execute(
                "UPDATE lyrics SET duration_ms = 120500 WHERE track_key = 'chiave'",
                [],
            )
            .expect("stessa incisione");
        assert_eq!(
            per_brano(&connection, id).expect("testo").aderenza,
            Aderenza::Buona
        );

        // E una riga vecchia, scritta prima che qualcuno leggesse quella
        // colonna, non diventa sospetta per il fatto di non saperlo.
        connection
            .execute(
                "UPDATE lyrics SET duration_ms = NULL WHERE track_key = 'chiave'",
                [],
            )
            .expect("durata ignota");
        assert_eq!(
            per_brano(&connection, id).expect("testo").aderenza,
            Aderenza::Buona,
            "«non si sa» non è «è sbagliato»"
        );
    }

    #[test]
    fn il_titolo_si_normalizza_prima_di_chiedere() {
        let (dir, connection) = libreria();
        let percorso = dir.path().join("brano.mp3");
        std::fs::write(&percorso, b"finto").expect("file");
        let id = inserisci(&connection, &percorso, None);
        let mut quale = super::brano(&connection, id).expect("brano");

        quale.title = "Poetica (Official Video)".to_owned();
        let (titolo, album, durata) = da_chiedere_al_catalogo(&quale);
        assert_eq!(titolo, "Poetica", "il rumore del caricamento non si cerca");
        assert_eq!(album, Some("Album"));
        assert_eq!(durata, Some(120_000));

        // Quel che dice del brano invece resta: cercare la versione dal vivo
        // di un file dal vivo è la cosa giusta.
        quale.title = "Poetica (Live)".to_owned();
        assert_eq!(da_chiedere_al_catalogo(&quale).0, "Poetica (Live)");

        // Un titolo fatto di solo rumore torna com'era: cercare la stringa
        // vuota non è cercare.
        quale.title = "(Official Video)".to_owned();
        assert_eq!(da_chiedere_al_catalogo(&quale).0, "(Official Video)");

        // Un album vuoto non diventa una stringa vuota da mandare: diventa
        // «non c'è», ed è `lrclib::url_esatta` a saltarlo.
        quale.album = "   ".to_owned();
        assert_eq!(da_chiedere_al_catalogo(&quale).1, None);
    }

    #[test]
    fn un_sylt_diventa_un_lrc() {
        use lofty::config::WriteOptions;
        use lofty::id3::v2::{
            FrameFlags, SyncTextContentType, SynchronizedTextFrame, TimestampFormat,
        };

        // Il caso comune: una riga intera per voce, nessun a capo in testa.
        let per_righe = SynchronizedTextFrame::new(
            lofty::TextEncoding::UTF8,
            *b"ita",
            TimestampFormat::MS,
            SyncTextContentType::Lyrics,
            None,
            vec![
                (1_000, "prima riga".to_owned()),
                (2_000, "seconda".to_owned()),
            ],
        );
        // Si passa dai byte, che è la forma in cui il frame arriva davvero:
        // in lofty `SYLT` non entra nel `Tag` unificato, sta in un
        // `Frame::Binary` e va interpretato a mano.
        let byte = per_righe.as_bytes(WriteOptions::default()).expect("byte");
        let riletto = SynchronizedTextFrame::parse(&byte, FrameFlags::default()).expect("frame");
        assert_eq!(
            lrc_da_sylt(&riletto).as_deref(),
            Some("[00:01.00]prima riga\n[00:02.00]seconda\n")
        );

        // Il caso dello standard: sillabe, e l'a capo che dice dove comincia
        // una riga. Le sillabe di una riga si ricompongono in una riga sola,
        // col tempo della prima.
        let sillabato = SynchronizedTextFrame::new(
            lofty::TextEncoding::UTF8,
            *b"eng",
            TimestampFormat::MS,
            SyncTextContentType::Lyrics,
            None,
            vec![
                (1_000, "\nHel".to_owned()),
                (1_200, "lo".to_owned()),
                (2_000, "\nworld".to_owned()),
            ],
        );
        assert_eq!(
            lrc_da_sylt(&sillabato).as_deref(),
            Some("[00:01.00]Hello\n[00:02.00]world\n")
        );

        // I tempi in fotogrammi MPEG non si convertono a occhio: senza la
        // durata del fotogramma sarebbe un testo che scorre a una velocità
        // inventata, e nessun testo è meglio di un testo storto.
        let a_fotogrammi = SynchronizedTextFrame::new(
            lofty::TextEncoding::UTF8,
            *b"eng",
            TimestampFormat::MPEG,
            SyncTextContentType::Lyrics,
            None,
            vec![(1, "prima".to_owned())],
        );
        assert_eq!(lrc_da_sylt(&a_fotogrammi), None);

        // E un `SYLT` che porta accordi non è il testo della canzone.
        let accordi = SynchronizedTextFrame::new(
            lofty::TextEncoding::UTF8,
            *b"eng",
            TimestampFormat::MS,
            SyncTextContentType::Chord,
            None,
            vec![(1_000, "Bb F Fsus".to_owned())],
        );
        assert_eq!(lrc_da_sylt(&accordi), None);
    }

    #[test]
    fn un_syncedlyrics_di_vorbis_si_legge() {
        let mut tag = lofty::ogg::VorbisComments::default();
        tag.push(CHIAVE_SINCRONIZZATA.to_owned(), LRC.to_owned());
        assert_eq!(da_vorbis(Some(&tag)).as_deref(), Some(LRC));

        // Un tag che quella chiave non ce l'ha è una fonte in meno, non un
        // errore — e nemmeno una stringa vuota lo è.
        let vuoto = lofty::ogg::VorbisComments::default();
        assert_eq!(da_vorbis(Some(&vuoto)), None);
        assert_eq!(da_vorbis(None), None);
        let mut spazi = lofty::ogg::VorbisComments::default();
        spazi.push(CHIAVE_SINCRONIZZATA.to_owned(), "  \n ".to_owned());
        assert_eq!(da_vorbis(Some(&spazi)), None);
    }

    #[test]
    fn un_file_che_non_e_musica_non_ha_tag_sincronizzati() {
        // La catena delle fonti chiama `tag_sincronizzato` su ogni brano senza
        // tempi, compresi quelli il cui file è sparito o illeggibile: deve
        // rispondere «niente» invece di far cadere la lettura.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let finto = dir.path().join("brano.mp3");
        std::fs::write(&finto, b"non e' un mp3").expect("file");
        assert_eq!(tag_sincronizzato(&finto), None);
        assert_eq!(tag_sincronizzato(&dir.path().join("non c'e'.mp3")), None);
    }

    #[test]
    fn dimenticare_l_esito_rimette_il_brano_in_coda() {
        let (dir, connection) = libreria();
        let brano = dir.path().join("brano.mp3");
        std::fs::write(&brano, b"finto").expect("file");
        inserisci(&connection, &brano, None);
        connection
            .execute(
                "INSERT INTO lyrics (track_key, source, checked_at, updated_at)
                 VALUES ('chiave', 'lrclib', 1000, 1000)",
                [],
            )
            .expect("un «non ce l'ho» ricordato");
        assert!(
            da_cercare(&connection, 1_000, 10).expect("coda").is_empty(),
            "gliel'abbiamo già chiesto"
        );

        dimentica_esito(&connection, "chiave").expect("dimenticato");
        assert_eq!(
            da_cercare(&connection, 1_000, 10).expect("coda").len(),
            1,
            "e adesso si può richiedere senza aspettare quattordici giorni"
        );
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
