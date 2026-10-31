//! I testi: dove si trovano, e in che ordine si guarda.
//!
//! Il formato lo interpreta [`rymembra_domain::testo`], che è puro. Qui si apre il
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
//!    intero, [`rymembra_domain::testo::leggi`] se ne accorge da sé.
//!
//! Quel che si trova nelle fonti 1, 2 e 4 si copia nella 3, che da lì in poi fa
//! da cache. Quel che sta nella 3 non torna mai indietro da solo: scrivere nei
//! file di qualcun altro è un gesto separato, e in questo modulo non c'è.
//!
//! # Quel che si sincronizza a mano sta nel database
//!
//! Un testo battuto nell'editor vive nella riga di `lyrics`, per intero — tempi
//! delle parole compresi — e quella è la sua fonte. Il `.lrc` accanto al brano è
//! un'**esportazione** per gli altri lettori, e dalla 3.0 si scrive solo se
//! l'utente l'ha chiesto ([`accanto_ai_file`], spento di serie): Rymembra non
//! mette file nelle cartelle della musica di sua iniziativa, così come non
//! tocca i byte dell'audio. I sidecar che ci sono già si leggono come sempre.
//!
//! # Quattro esiti, e nessuna schermata bianca
//!
//! Ogni brano finisce in uno di quattro stati — sincronizzato, piatto,
//! strumentale, niente — e il quarto è l'unico che chiede qualcosa a chi guarda.
//! È la ragione per cui [`Fonte::Nessuna`] esiste come valore invece che come
//! `Option`: «non ho ancora guardato» e «ho guardato e non c'è» sono due cose,
//! e la seconda va ricordata.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension as _};
use rymembra_domain::enrich::GRAZIA_MS;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::indizi::e_segnaposto;
use rymembra_domain::testo::{self, Aderenza, Testo};
use rymembra_meta::Fornitori;
use rymembra_meta::lrclib;

use crate::library::{db_error, now_ms};

mod catalogo;
mod disco;

pub use catalogo::{
    Copertura, LOTTO, RIPROVA_DOPO_UN_GUASTO_MS, RIPROVA_MS, candidati_in_rete,
    cerca_di_nuovo_in_rete, cerca_in_rete, copertura, da_cercare, dimentica_esito,
    quanti_da_cercare, ricorda_esito, ricorda_scelta, rifiuta, rimanda,
};
use disco::leggi_sidecar_con_data;
pub use disco::{
    CHIAVE_ACCANTO, accanto_ai_file, leggi_sidecar, percorso_sidecar, scrivi_sidecar,
    tag_sincronizzato,
};

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
    /// Il percorso del file, per il sidecar. `None` per un brano di catalogo:
    /// le parole si cercano lo stesso — il catalogo dei testi si interroga per
    /// artista, titolo e durata, e quelli un flusso ce li ha — ma non c'è
    /// nessun file accanto a cui posare un `.lrc`.
    ///
    /// `None` anche per un brano d'immagine CD, per la stessa ragione detta
    /// dall'altro lato: il suo percorso, `…\Album.cue|03`, non è un file, e il
    /// file vero è l'immagine di tutto il disco. Un `.lrc` accanto sarebbe di
    /// tutti i brani insieme — `Album.lrc`, lo stesso per la traccia 1 e per la
    /// 12 — e il tag sincronizzato dentro l'immagine, se ci fosse, non è di
    /// nessun brano in particolare. I suoi testi stanno nel database, come
    /// quelli di un brano di catalogo. È la stessa risposta di
    /// [`crate::playback::Collocazione::percorso`].
    pub path: Option<String>,
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
///
/// Il percorso di un brano d'immagine esce `NULL` già da qui, e non più sotto:
/// vedi [`BranoDaTestare::path`]. Così ogni strada che posa o legge un sidecar
/// — anche quelle della finestra, che passano da [`brano`] — lo tratta da sé
/// come un brano senza file, invece di dover ricordare a ognuna la domanda.
const COLONNE: &str = "t.id, CASE WHEN t.immagine IS NULL THEN t.path END, t.track_key,
     t.title, t.artist, t.album, t.duration_ms, t.lyrics";

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
///
/// Legge il disco **con la connessione in mano**: va bene per le prove e per chi
/// non tiene nessun lucchetto. Chi la connessione la tiene sotto il lucchetto
/// della libreria usa i tre tempi — [`leggi_riga`], [`leggi_dal_disco`] fuori
/// dal lucchetto, [`componi`] — per la ragione scritta su [`DalDisco`].
#[must_use]
pub fn per_questo_brano(connection: &Connection, brano: &BranoDaTestare) -> TestoBrano {
    let riga = leggi_riga(connection, &brano.track_key);
    let disco = leggi_dal_disco(brano, riga.as_ref());
    componi(connection, brano, riga, disco)
}

/// Quel che del testo sta sul disco accanto al brano: il sidecar e il tag.
///
/// # Perché si legge a parte
///
/// Perché «accanto al brano» può voler dire su una cartella di rete, e prima si
/// leggeva dentro [`per_questo_brano`] con il lucchetto della libreria in mano:
/// una condivisione lenta a rispondere teneva fermo, per tutto quel tempo, ogni
/// comando che chiedeva la libreria — la lista dei brani, la ricerca, il
/// contatore dei preferiti — per aprire il pannello del testo. Adesso chi tiene
/// il lucchetto lo lascia prima di leggere il disco e lo riprende per scrivere:
/// è la stessa forma dei tre tempi della rete.
#[derive(Debug, Clone, Default)]
pub struct DalDisco {
    /// Il contenuto del sidecar, se c'è.
    pub sidecar: Option<String>,
    /// Quando il sidecar è stato modificato l'ultima volta, in millisecondi.
    pub sidecar_ms: Option<i64>,
    /// L'LRC dentro il file musicale, se c'è e se serviva aprirlo.
    pub tag: Option<String>,
}

/// Legge il sidecar e — solo se serve — il tag sincronizzato del file.
///
/// `riga` è quel che la tabella sa del brano, letto prima: decide se aprire il
/// file musicale, che è la lettura cara. Si apre solo se il sidecar non basta e
/// la riga non ha già i tempi, esattamente come quando le due letture stavano
/// in fila dentro [`per_questo_brano`].
#[must_use]
pub fn leggi_dal_disco(brano: &BranoDaTestare, riga: Option<&Riga>) -> DalDisco {
    if riga.is_some_and(|r| r.strumentale) {
        return DalDisco::default();
    }
    // Senza un file non c'è né un `.lrc` accanto né un tag da aprire: restano
    // la riga in tabella e il catalogo, che è esattamente quel che serve a un
    // brano di catalogo.
    let Some(percorso) = brano.path.as_deref() else {
        return DalDisco::default();
    };
    let (sidecar, sidecar_ms) = match leggi_sidecar_con_data(Path::new(percorso)) {
        Some((grezzo, quando)) => (Some(grezzo), quando),
        None => (None, None),
    };
    let basta_il_sidecar = sidecar.as_deref().is_some_and(|grezzo| {
        let testo = testo::leggi(grezzo);
        let copre_i_tempi = !testo.sincronizzato() && riga.is_some_and(Riga::ha_i_tempi);
        !testo.vuoto() && !copre_i_tempi
    });
    let tag = if basta_il_sidecar || riga.is_some_and(Riga::ha_i_tempi) {
        None
    } else {
        tag_sincronizzato(Path::new(percorso))
    };
    DalDisco {
        sidecar,
        sidecar_ms,
        tag,
    }
}

/// Mette insieme il testo di un brano da quel che si è letto, e ricorda in
/// tabella quel che il disco ha dato.
///
/// Non tocca il disco: il disco l'ha già letto [`leggi_dal_disco`].
#[must_use]
pub fn componi(
    connection: &Connection,
    brano: &BranoDaTestare,
    riga: Option<Riga>,
    disco: DalDisco,
) -> TestoBrano {
    let durata = u64::try_from(brano.duration_ms).unwrap_or(0);

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
    // Tranne quando è **più vecchio** di un testo sincronizzato a mano che dice
    // altro: vuol dire che il salvataggio ha scritto la riga e non è riuscito a
    // scrivere il file — una cartella di sola lettura, una share — e il file
    // rimasto è quello di prima. Senza questa guardia chi ha appena
    // sincronizzato vedrebbe tornare il testo vecchio. Un sidecar modificato
    // **dopo** invece vince, com'è sempre stato: chi lo tocca a mano deve
    // vedere il cambiamento.
    //
    // «Dice altro» vuol dire **altre righe**, e non altri byte. La riga salvata
    // a mano è il `.lrc` semplice, e il sidecar che si legge per primo è il
    // `.a2.lrc` con i tempi delle parole: byte diversi per la stessa
    // sincronizzazione. Finché `updated_at` restava quello del salvataggio il
    // confronto delle date li teneva d'accordo; ma lo scarto riscrive
    // `updated_at`, e un colpo su «−100» dopo una sincronia a parole faceva
    // passare la riga davanti al suo stesso `.a2.lrc` — le parole si spegnevano.
    let superato_dalla_mano = |grezzo: &str| {
        riga.as_ref().is_some_and(|r| {
            r.fonte.e_di_chi_ascolta()
                && r.ha_i_tempi()
                && disco
                    .sidecar_ms
                    .is_some_and(|quando| quando < r.aggiornata_ms)
                && !stesse_righe(r.synced.as_deref(), grezzo)
        })
    };
    if let Some(grezzo) = disco.sidecar.as_deref().filter(|g| !superato_dalla_mano(g)) {
        let grezzo = grezzo.to_owned();
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
        && let Some(grezzo) = disco.tag
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

/// I due LRC hanno le stesse righe agli stessi tempi — e le stesse parole, se
/// la riga salvata le porta.
///
/// Serve a riconoscere il `.a2.lrc` di una sincronizzazione nella riga che ne
/// conserva il gemello semplice: vedi `superato_dalla_mano` in [`componi`]. Le
/// traduzioni contano, perché sono righe.
///
/// # Le parole, e perché contano solo a volte
///
/// Fino alla 2.4 la riga salvata portava sempre il gemello **semplice**, e le
/// parole stavano solo nell'`.a2.lrc`: confrontarle avrebbe dichiarato diversi
/// due file che erano la stessa sincronizzazione. Dalla 3.0 la riga è la fonte e
/// porta tutto, parole comprese — e allora contano, perché sono proprio quel che
/// si è appena rifatto. Chi ha ribattuto le sole parole, con i `.lrc` accanto al
/// brano spenti, ha ancora lì l'`.a2.lrc` di prima, con le stesse righe e le
/// parole vecchie: senza questo confronto quel file, più vecchio della riga, la
/// scavalcherebbe lo stesso.
fn stesse_righe(salvato: Option<&str>, grezzo: &str) -> bool {
    let Some(salvato) = salvato else {
        return false;
    };
    if salvato == grezzo {
        return true;
    }
    let letto = testo::leggi(salvato);
    let con_le_parole = letto.righe.iter().any(|riga| !riga.parole.is_empty());
    let righe = |testo: Testo| {
        testo
            .righe
            .into_iter()
            .map(|riga| {
                let parole = if con_le_parole {
                    riga.parole
                } else {
                    Vec::new()
                };
                (riga.ms, riga.testo, riga.secondaria, parole)
            })
            .collect::<Vec<_>>()
    };
    righe(letto) == righe(testo::leggi(grezzo))
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
    /// Quando la riga è stata scritta l'ultima volta, in millisecondi.
    ///
    /// Serve a un confronto solo: un testo sincronizzato a mano più recente del
    /// sidecar che sta accanto al brano. Vedi [`componi`].
    pub aggiornata_ms: i64,
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
    /// solo quel che [`rymembra_domain::testo::Testo::sincronizzato`] aveva già
    /// riconosciuto come tale, e rileggerlo qui sarebbe interpretare due volte
    /// lo stesso file per rispondere alla stessa domanda.
    #[must_use]
    pub fn ha_i_tempi(&self) -> bool {
        self.synced.as_deref().is_some_and(|t| !t.trim().is_empty())
    }

    /// Questi tempi sono stati battuti su un'edizione che dura un altro tanto.
    ///
    /// # Cosa aggiunge a [`rymembra_domain::testo::verifica_durata`]
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
            "SELECT synced, plain, source, offset_ms, instrumental, checked_at, duration_ms,
                    updated_at
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
                aggiornata_ms: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
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
///
/// Un brano con id ≤ 0 non si ricorda, in silenzio: non è una riga di questa
/// libreria (vedi `playback::riga_della_libreria`), e la sua `track_key` —
/// che `lyrics` usa come chiave, senza nessuna chiave esterna a fare da
/// guardia — sarebbe una riga di testo per un brano che a sessione finita non
/// esiste più. Il testo letto dal disco si mostra lo stesso: è solo la cache
/// che non si scrive, e una cache che non si scrive non è un guasto da dire.
fn ricorda(
    connection: &Connection,
    brano: &BranoDaTestare,
    grezzo: &str,
    testo: &Testo,
    fonte: Fonte,
) {
    if brano.id <= 0 {
        return;
    }
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
           updated_at = excluded.updated_at,
           -- Il testo in riga adesso è quello del disco, non la voce scelta.
           scelta = 0
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
/// [`rymembra_domain::testo::posizione_corretta`], che è l'unico posto in cui
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

// ── sincronizzato a mano ────────────────────────────────────────────────────

/// Scrive un testo sincronizzato a mano: la riga, e poi — se l'utente l'ha
/// chiesto — il `.lrc` accanto al brano.
///
/// Il file si scrive solo con [`accanto_ai_file`] acceso, che di serie è spento:
/// la riga è la fonte, e il file un'esportazione. Vedi il `//!` del modulo.
///
/// Le due metà in una chiamata sola, per chi non tiene lucchetti: le prove, e
/// chiunque abbia la connessione in mano senza nessuno che aspetti dietro. Chi
/// la connessione la tiene sotto il lucchetto della libreria chiama le due metà
/// separate — [`registra_a_mano`] dentro, [`scrivi_sidecar`] fuori — perché la
/// scrittura del file può stare su una share.
///
/// # Perché la riga viene prima, adesso
///
/// Prima veniva prima il file, con una ragione scritta qui: meglio fallire del
/// tutto che avere una riga che promette un file che non c'è. Il prezzo di
/// quella regola era un lavoro perso per intero — mezz'ora di battute su un
/// brano che sta in una cartella di sola lettura, o su una share che si è
/// staccata, e l'editor che risponde «non si è potuto scrivere» senza aver
/// salvato niente da nessuna parte. Il testo è di chi l'ha battuto, e il posto
/// dove non si perde è il database: il file accanto al brano è il modo di
/// portarlo agli altri lettori, e se non si scrive si dice.
///
/// La promessa che la riga non mente resta, in un altro modo: un sidecar più
/// vecchio della riga non la scavalca — vedi [`componi`].
///
/// # I byte dell'audio non si toccano
///
/// Non si scrive in `USLT`, non si riapre il file con lofty, non serve
/// `enrich_undo`. Un `.lrc` accanto al brano lo leggono foobar2000, VLC,
/// Poweramp e Navidrome, e cancellarlo è un gesto che chiunque sa fare senza
/// strumenti — che è la definizione di reversibile. Quello che c'era prima, se
/// non era lo stesso, resta in un `.lrc.bak`: vedi [`scrivi_sidecar`].
///
/// # Errori
///
/// `db.queryFailed` se la riga non si salva, e allora il file non si tocca;
/// `fs.writeFailed` se il sidecar non si scrive, **dopo** che la riga è salva.
pub fn salva_a_mano(
    connection: &Connection,
    brano: &BranoDaTestare,
    lrc: &str,
) -> Result<(), AppError> {
    registra_a_mano(connection, brano, lrc)?;
    // Per un brano di catalogo la riga in tabella è tutto quel che c'è da
    // salvare, ed è abbastanza: la tabella `lyrics` è indicizzata su
    // `track_key`, non sul percorso. Il sidecar serve a far sopravvivere il
    // testo a una reinstallazione **accanto al file**, e un file non c'è.
    let Some(percorso) = brano.path.as_deref() else {
        return Ok(());
    };
    if !accanto_ai_file(connection) {
        return Ok(());
    }
    // `std::fs` e non `MusicFiles`: quel tratto sa aprire in lettura, perché è
    // quel che serve al motore. Scrivere accanto a un file altrui è un mestiere
    // in più, e quando arriverà Android — dove non c'è un percorso ma una
    // concessione — sarà quello il momento di allargare il tratto, non adesso
    // con un'astrazione che avrebbe un implementatore solo.
    scrivi_sidecar(Path::new(percorso), "lrc", Some(lrc))
}

/// La metà di [`salva_a_mano`] che scrive nel database, e nient'altro.
///
/// La guardia sull'id sta qui e non in [`salva_a_mano`], che passa di qui per
/// prima: così un id ≤ 0 si ferma prima della riga **e** prima del sidecar.
///
/// # Errori
///
/// `db.queryFailed`; `library.trackNotFound` per un id ≤ 0, come
/// [`ricorda_esito`].
pub fn registra_a_mano(
    connection: &Connection,
    brano: &BranoDaTestare,
    lrc: &str,
) -> Result<(), AppError> {
    crate::playback::riga_della_libreria(brano.id)?;
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
/// `rymembra_domain::album::UNKNOWN_ARTIST`, cioè «Artista sconosciuto»
/// (`library/righe.rs`, dentro `read_track`). È un segnaposto utile in casa — tiene
/// insieme la vista Artisti — e in un catalogo pubblico è una voce che nessuno
/// troverà mai più, perché nessuno cercherà il testo di «Artista sconosciuto».
/// Quel contributo non è rumore: è un contributo **perduto**, e lo scopre solo
/// chi va a guardare sul sito.
///
/// Si rifiuta quindi su **titolo** e **artista**, che sono l'identità con cui il
/// catalogo indicizza e con cui ogni altra persona ritroverà quel testo. Il
/// controllo passa da [`rymembra_domain::indizi::e_segnaposto`] e non da un
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
    let salvato = riga
        .synced
        .filter(|lrc| !lrc.trim().is_empty())
        .ok_or_else(|| rifiuto("un testo senza tempi al catalogo non serve"))?;
    // Al catalogo va il gemello **semplice**: dalla 3.0 la riga porta anche i
    // tempi delle parole, e mandarli vorrebbe dire far arrivare i `<mm:ss.xx>`
    // a chi li mostrerà come testo.
    let sincronizzato = senza_parole(&salvato);

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

/// Lo stesso LRC senza i tempi delle singole parole.
///
/// Un LRC che parole non ne porta esce **identico**, byte per byte: riscriverlo
/// passando da [`testo::scrivi`] ne cambierebbe la forma — i centesimi, le
/// etichette in testa — per niente.
fn senza_parole(lrc: &str) -> String {
    let letto = testo::leggi(lrc);
    if letto.righe.iter().all(|riga| riga.parole.is_empty()) {
        return lrc.to_owned();
    }
    testo::scrivi(&Testo {
        righe: letto
            .righe
            .into_iter()
            .map(|riga| testo::Riga {
                parole: Vec::new(),
                ..riga
            })
            .collect(),
        ..letto
    })
}

/// Manda al catalogo quel che [`da_restituire`] ha preparato. **Non tocca il
/// database.**
///
/// Ci mette qualche secondo, e non per la rete: la prova di lavoro di LRCLIB è
/// spiegata in [`rymembra_meta::lrclib::pubblica`]. Chi chiama deve dirlo prima,
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
mod prove;
