//! Il profilo: portarsi dietro Aether, non solo le sue impostazioni.
//!
//! Fino alla 2.3.0 questo modulo scriveva un JSON di diciassette preferenze e
//! diceva, in testa a sé stesso, che per il resto c'era il backup su Drive. Da
//! questa release porta anche la libreria — cronologia, voti, preferiti,
//! playlist, correzioni, testi, copertine, pacchetti skin — e il file è un
//! archivio `.aeprofile`.
//!
//! # I due si sovrappongono, ed è voluto
//!
//! La frase di prima («per quello c'è il backup su Drive») non è più vera, e
//! sostituirla con «adesso fa tutto questo» sarebbe raccontare male una scelta
//! precisa. I due meccanismi portano quasi le stesse cose e restano due, perché
//! rispondono a due domande diverse:
//!
//! - Il **backup** è automatico e continuo. Gira da solo ogni quarto d'ora,
//!   scrive in una cartella riservata del Drive di chi lo ha collegato, e
//!   risponde a «il portatile è caduto dalle scale». Presuppone un account, una
//!   rete, e un consenso dato mesi prima.
//! - Il **profilo** è un gesto. Lo si chiede, si sceglie dove metterlo, e sta su
//!   una chiavetta. Risponde a «mi hanno dato un computer nuovo e non voglio
//!   collegare niente», e funziona in un ufficio senza rete, su una macchina che
//!   non ha mai visto Google, e per una persona che non vuole che i propri
//!   ascolti passino da un servizio.
//!
//! Un meccanismo che funziona solo se hai un account non è una via di fuga: è
//! una dipendenza in più. Per questo i due si sovrappongono di proposito, e non
//! si è tolto niente all'uno per dare all'altro.
//!
//! # Un elenco di quel che **esce**, non di quel che resta
//!
//! Vale ancora, e vale solo per le **preferenze**. La ragione ha un nome:
//! `nuvola.dispositivo`.
//!
//! Quella chiave è l'identificativo con cui il backup distingue questo computer
//! dagli altri quando fonde le statistiche. Due macchine che dichiarano lo
//! stesso identificativo non danno nessun errore: si sovrascrivono a vicenda nel
//! backup, e il danno si scopre mesi dopo, guardando conteggi d'ascolto che non
//! tornano.
//!
//! Con un elenco di **esclusioni**, una chiave nuova viaggia per difetto — e la
//! prossima `nuvola.dispositivo` uscirebbe in silenzio, scritta da qualcuno che
//! non sapeva che questo file esistesse. Con un elenco di **inclusioni**, una
//! chiave nuova non viaggia finché qualcuno non la mette in [`CATALOGO`]: il
//! guasto peggiore diventa «una preferenza non si è portata dietro», che si nota
//! subito e non rompe niente.
//!
//! Perché «si nota subito» sia vero, l'esportazione **dichiara** cosa ha lasciato
//! fuori e perché. Un elenco di inclusioni silenzioso avrebbe lo stesso difetto
//! dell'altro, spostato di un passo.
//!
//! # I segreti non entrano, e non c'è nemmeno la riga che potrebbe
//!
//! Token, chiavi di sessione e segreti stanno nel portachiavi di sistema
//! (`aether_oauth::portachiavi`), e questo modulo non lo vede. Non è una
//! precauzione presa qui: è che non c'è niente da cui prenderli.
//!
//! La chiave dell'applicazione Last.fm è il caso limite che ha fatto scrivere
//! questa riga. Fino alla 2.3.0 stava in `settings`, con un commento che
//! diceva «non è un segreto»: viaggia in chiaro nell'indirizzo del consenso,
//! quindi tecnicamente non lo è. Ma un profilo si manda in giro, e quella è una
//! credenziale **personale** — chi la ottiene manda scrobble a nome di
//! qualcun altro finché non gliela si revoca. La 2.3.1 la travasa nel
//! portachiavi all'avvio (`crate::scrobble::CHIAVE_LFM_API_KEY`) e comunque non
//! sta in [`CATALOGO`]: una prova qui sotto lo tiene vero.
//!
//! # Cosa viaggia oltre le preferenze, e cosa no
//!
//! Il contenuto del profilo **è** quello della sincronia. Non una sua copia
//! riscritta: l'archivio contiene, byte per byte, lo stesso documento che
//! `aether_sync::documento::serializza` produrrebbe per questo dispositivo, più
//! un secondo documento per il complemento — vedi [`biblioteca`]. Riusare
//! invece di duplicare non è economia di righe: un secondo serializzatore
//! sarebbe una seconda idea di cosa sia un voto, e le due divergerebbero al
//! primo campo aggiunto da una parte sola.
//!
//! Restano **fuori** affinità, impronte e settimana: sono derivati
//! ricalcolabili, e il perché sta nel `//!` di [`biblioteca`]. Il manifesto li
//! elenca, perché chi apre l'archivio deve poter leggere cosa non c'è senza
//! doverlo dedurre da cosa c'è.
//!
//! # L'identità, e perché una libreria diversa prende solo le preferenze
//!
//! I documenti parlano per `track_key`. Su un'altra libreria quelle chiavi
//! nominano canzoni diverse o nessuna, e riversarci sopra una cronologia
//! d'ascolto vorrebbe dire attribuire a qualcuno ascolti che non ha fatto — in
//! silenzio, e senza modo di distinguerli dai propri.
//!
//! Quindi il manifesto porta un'`identita`: un identificativo che nasce con la
//! libreria e le resta addosso. Chi importa un profilo su una libreria **senza**
//! identità la adotta — è il caso della reinstallazione e del secondo computer
//! della stessa persona, cioè i due casi per cui il profilo esiste. Chi lo
//! importa su una libreria che ne ha già una **diversa** prende preferenze,
//! copertine e pacchetti skin, e la parte di libreria si rifiuta dicendolo.
//!
//! Nessuna terza chiave, e vale la pena scriverlo perché la tentazione c'è: dal
//! 2.3.1 `tracks` ha anche `content_key`, calcolata dai tag grezzi. L'abbinamento
//! resta `track_key`, che è quel che i documenti già in circolazione contengono,
//! e il manifesto lo dichiara nel campo `abbinamento`.
//!
//! # Un profilo è un dono, non un'autorità
//!
//! Niente cancella niente. Nessuna lapide del profilo si applica, nessuna
//! preferenza si cancella, nessuna riga di libreria sparisce. Chi importa
//! aggiunge quel che gli manca; quel che aveva resta. È la differenza fra
//! questa funzione e un ripristino, e va detta dove si vede perché le due si
//! somigliano abbastanza da confondersi.
//!
//! # Perché l'importazione ha un piano
//!
//! Come ogni altra operazione irreversibile del programma. Un profilo cambia la
//! skin, il tema, le cartelle sorvegliate e l'equalizzatore tutti insieme, e
//! senza un piano l'unico modo di sapere cosa cambierà è guardare cos'è
//! cambiato. Il piano dice, chiave per chiave, cosa c'è adesso e cosa ci sarebbe
//! — e per i percorsi, quali di quelli che arrivano **non esistono su questo
//! computer**, che è il caso normale quando il profilo viene da una macchina
//! diversa.
//!
//! Il piano non è una previsione scritta altrove: è l'esecuzione, dentro una
//! transazione che viene abbandonata. Vale anche per la parte nuova, ed è
//! l'unica ragione per cui [`crate::sincronia::applica_in`] esiste.
//!
//! **La sola scrittura che il piano fa** è [`crate::sincronia::allinea`], e va
//! detta: mette le tabelle della sincronia in pari con la libreria, non dipende
//! dal profilo, e la passata automatica la farebbe comunque entro pochi minuti.
//! Sta fuori dalla transazione perché apre la sua, e ci sta di proposito: senza,
//! il piano confronterebbe l'archivio con uno stato che l'esecuzione non
//! userebbe, e i suoi numeri smetterebbero di essere quelli veri.
//!
//! # Il peso
//!
//! Un profilo con le copertine di una libreria vera pesa centinaia di megabyte.
//! Nessuna funzione di questo modulo tiene l'archivio in memoria: la raccolta
//! sotto il lucchetto produce due documenti piccoli, la scrittura copia i file
//! uno alla volta con il buffer di un `BufReader`, e la lettura estrae una voce
//! per volta direttamente sul disco. Vedi [`archivio`].

pub mod archivio;
pub mod biblioteca;
pub mod radici;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::library::db_error;
use crate::settings;

pub use radici::Rimappatura;

/// Cosa rappresenta una chiave, e quindi come la si tratta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Genere {
    /// Una scelta: viaggia sempre.
    Preferenza,
    /// Un percorso sul disco: viaggia, ma su un'altra macchina può non esistere.
    Percorso,
}

/// Quel che esce, e cos'è.
///
/// L'unico posto in cui si decide che una chiave può lasciare questo computer.
/// Aggiungerne una qui è una riga; dimenticarsene costa una preferenza che non
/// si porta dietro, ed è per questo che l'omissione è il difetto che si vuole
/// avere invece dell'altro.
const CATALOGO: &[(&str, Genere)] = &[
    // ── l'aspetto ──
    (crate::preferenze::CHIAVE_TEMA, Genere::Preferenza),
    // La lingua viaggia, e su un'altra macchina può nominare un file che di là
    // non c'è: la finestra ripiega sul sistema, che è il comportamento giusto —
    // meglio dell'inglese imposto da un profilo scritto altrove.
    (crate::preferenze::CHIAVE_LINGUA, Genere::Preferenza),
    ("skin.active", Genere::Preferenza),
    ("skin.dynamicAccent", Genere::Preferenza),
    // Viaggia: «la X non spegne» è un'abitudine di chi ascolta, non un fatto di
    // questa macchina — al contrario di `player.output`, che è il nome di una
    // scheda audio. Su un altro computer nomina lo stesso vassoio.
    (crate::preferenze::CHIAVE_SECONDO_PIANO, Genere::Preferenza),
    // Viaggia, e più di ogni altra cosa in questo elenco: una preferenza di
    // accessibilità è esattamente quella che deve ritrovarsi identica su un
    // altro computer. Chi chiede meno movimento non lo chiede a una macchina.
    (
        crate::preferenze::CHIAVE_MOVIMENTO_RIDOTTO,
        Genere::Preferenza,
    ),
    // `ui.zoom` (`crate::preferenze::CHIAVE_ZOOM`) **non** c'è, ed è la sesta
    // omissione deliberata: l'unica in questa sezione, e sta proprio accanto
    // alla riga di sopra perché il confronto fra le due è la ragione.
    // «Meno movimento» vuol dire la stessa cosa su qualunque computer — è una
    // frase su chi guarda. «Tutto una volta e mezza» vuol dire una cosa sola su
    // **questo** monitor, a **questa** risoluzione, e per giunta **con la
    // scalatura di Windows già applicata sotto**: lo zoom di Aether è la
    // correzione che resta dopo quella del sistema operativo, quindi il numero
    // giusto qui dipende da entrambe. Portato altrove, dove almeno una delle
    // due è diversa, non descrive più niente. È la famiglia di `player.output`.
    //
    // A differenza di `player.spectrum.quality`, però, il danno **si vede**:
    // chi arriva su un portatile con la scalatura a 1,75 se ne accorge al primo
    // fotogramma e preme `Ctrl+0`, che è un tasto e non una caccia. L'omissione
    // pesa quindi meno delle altre — ma è un'omissione, e un elenco di
    // inclusioni si difende scrivendo perché una riga manca, non contando su
    // chi la ritroverà.
    // ── la riproduzione ──
    // `player.queue` **non** c'è, ed è la seconda ragione per cui questo elenco
    // è di inclusioni: contiene identificativi di righe di `tracks`, che su
    // un'altra libreria nominano canzoni diverse o nessuna. Le playlist invece
    // adesso viaggiano, e non è una contraddizione: quelle si fondono per
    // `track_key`, che è l'identità del brano, mentre la coda è un elenco di
    // `id`, che è l'identità di una **riga**.
    ("player.volume", Genere::Preferenza),
    ("player.eq", Genere::Preferenza),
    ("player.eq.presets", Genere::Preferenza),
    ("player.replaygain", Genere::Preferenza),
    // Viaggia, al contrario del timer di spegnimento: «continua quando la coda
    // finisce» è come uno vuole che il lettore si comporti, e vale su ogni
    // macchina. Il timer invece è una decisione di stasera, e ritrovarlo su un
    // altro computer sarebbe una musica che si spegne da sola senza motivo.
    ("player.autoplay", Genere::Preferenza),
    // Viaggia per la stessa ragione: quanto si vuole che due brani si
    // sovrappongano è un gusto d'ascolto, non un fatto di questa macchina.
    ("player.crossfade", Genere::Preferenza),
    // Viaggiano tutte e due: quante barre si vogliono vedere e se la scena
    // parte accesa sono gusti di chi guarda, e restano veri su qualunque
    // computer. Le barre erano una promessa che il codice faceva senza
    // mantenerla — la carta di `CHIAVE_SPETTRO_BANDE` diceva che la scelta non
    // sparisce cambiando dispositivo, e l'unico meccanismo che la può portare
    // altrove è questo elenco, dove non c'era.
    (crate::playback::CHIAVE_SPETTRO_BANDE, Genere::Preferenza),
    (crate::playback::CHIAVE_SPETTRO_VISIBILE, Genere::Preferenza),
    // Viaggia, e sta fra i gusti e non fra i fatti di questa macchina: «voglio
    // leggere com'è fatto il file che sto sentendo» è un'abitudine di chi
    // ascolta, e resta vera su qualunque computer. Al contrario della qualità
    // dello spettro qui sotto, questa riga non descrive nessun hardware — dice
    // se una riga di testo si disegna, e quella riga costa uguale su un fisso e
    // su un portatile.
    (crate::playback::CHIAVE_FORMATO_VISIBILE, Genere::Preferenza),
    // `player.output` **non** c'è, ed è l'altra faccia di `player.queue`: è il
    // nome di una scheda audio, cioè il fatto di questa macchina per
    // eccellenza. Portarlo altrove vorrebbe dire arrivare su un computer con
    // scritto «FiiO K11» in una preferenza che là non nomina niente — non
    // rotto, perché `dispositivi::scegli` ripiega sul predefinito, ma una
    // riga di impostazioni che indica un oggetto inesistente. L'omissione qui
    // è deliberata: la prova che la tiene tale sta in fondo al file.
    // `player.spectrum.quality` **non** c'è, ed è la terza omissione
    // deliberata di questo elenco. La ragione è quella di `player.output`: è un
    // fatto di questa macchina, non un gusto di chi ascolta — «alta» vuol dire
    // «quel che questa scheda video regge», e portata da un fisso a un
    // portatile descrive un hardware che di là non esiste.
    //
    // A differenza delle altre due, però, il danno sarebbe **invisibile**. Una
    // coda che nomina righe sbagliate si vede subito; un'uscita audio che non
    // c'è ripiega sul predefinito e si nota. Qui la scena continua a
    // disegnarsi: solo peggio, e più calda, su una macchina che nessuno ha
    // misurato. Un difetto che non si manifesta è un difetto che non si
    // corregge, ed è la ragione per cui l'omissione qui conta più delle altre.
    // `audio.latenza_ms` **non** c'è, ed è la quarta omissione deliberata,
    // sorella di `player.output`: è il ritardo misurato — e poi ritoccato a
    // mano — fra quel che il lettore crede di aver suonato e quel che esce
    // dalle casse **di questa catena**. Cambia con la scheda, col driver e con
    // le cuffie; portarlo su un altro computer vorrebbe dire spostare i testi
    // di un decimo di secondo dalla parte sbagliata, in silenzio e senza che
    // nessuno colleghi le due cose. Vale la stessa nota di
    // `player.spectrum.quality`: il danno sarebbe invisibile.
    // ── la tastiera ──
    (crate::preferenze::CHIAVE_SCORCIATOIE, Genere::Preferenza),
    // ── gli automatismi ──
    ("enrich.auto", Genere::Preferenza),
    ("scrobble.attivo", Genere::Preferenza),
    ("nuvola.attivo", Genere::Preferenza),
    // `scrobble.lastfm.api_key` **non** c'è, ed è la quinta omissione
    // deliberata — la sola che riguardi una credenziale. Non è un segreto nel
    // senso stretto: viaggia in chiaro nell'indirizzo del consenso, e chiunque
    // guardi quella barra la vede. È però **personale**, cioè legata a un
    // account di sviluppatore che una persona ha registrato a suo nome; chi la
    // prende manda scrobble come lei finché non gliela si revoca. Un profilo è
    // fatto per essere passato in giro, e una credenziale personale dentro un
    // file che si passa in giro è la definizione del guasto. Dalla 2.3.1 sta nel
    // portachiavi, che è il posto dove questo modulo non arriva.
    // ── i percorsi ──
    // Viaggiano, e il piano dice quali non esistono di qua. L'alternativa —
    // lasciarli fuori — renderebbe inutile il caso più comune di tutti: la
    // reinstallazione sulla stessa macchina.
    ("library.roots", Genere::Percorso),
    ("download.folder", Genere::Percorso),
];

/// Come si riconosce un file di profilo.
pub use archivio::FIRMA;

/// La versione del formato v1: un JSON, che si legge ancora e non si scrive più.
const VERSIONE_V1: u32 = 1;

/// La chiave sotto cui sta l'identità di questa libreria.
///
/// Non sta in [`CATALOGO`] e non ci deve stare: è la cosa che distingue questa
/// libreria dalle altre, e portarla in giro come preferenza farebbe di due
/// librerie diverse una sola — cioè esattamente il guasto che esiste per
/// impedire. Viaggia nel **manifesto**, che è un posto in cui si legge e non si
/// scrive.
pub const CHIAVE_IDENTITA: &str = "profilo.identita";

/// Cosa il profilo lascia fuori di proposito, come lo dichiara il manifesto.
///
/// Testo e non codici: è un elenco che si legge aprendo l'archivio con un
/// gestore di file qualunque, e chi lo apre non ha il nostro schema sotto mano.
const FUORI: &[&str] = &[
    "i file musicali",
    "i token e le credenziali (stanno nel portachiavi di sistema)",
    "l'identificativo di dispositivo del backup (nuvola.dispositivo)",
    "la coda di riproduzione (nomina righe di questa libreria)",
    "l'uscita audio, la qualità dello spettro e la latenza (fatti di questa macchina)",
    "i nodi aperti del pannello Cartelle (percorsi di questa macchina)",
    "l'affinità, le impronte sonore e la settimana (derivati ricalcolabili)",
    "l'arricchimento in database (ri-derivabile da una passata)",
];

/// Il file v1, come sta sul disco.
///
/// Resta **leggibile** e non si scrive più. La struttura è quella di sempre,
/// intatta, e nell'archivio v2 questo stesso documento è `preferenze.json`: chi
/// apre un `.aeprofile` con un gestore di archivi trova dentro il file che
/// prima stava fuori.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuDisco {
    /// La firma, per non provare a leggere un JSON qualunque.
    aether: String,
    /// La versione del formato.
    versione: u32,
    /// Quando è stato scritto, in millisecondi.
    creato_ms: i64,
    /// Le chiavi e i loro valori.
    voci: BTreeMap<String, String>,
}

// ── l'identità della libreria ───────────────────────────────────────────────

/// L'identità di questa libreria, creandola se non c'è.
///
/// `genera` produce un identificativo nuovo. Gliela si passa invece di
/// generarlo qui per la stessa ragione per cui si passa `esiste`: la casualità
/// del sistema operativo sta in `aether-oauth`, e questo crate non la vede — e
/// così le prove girano deterministiche.
///
/// # Errori
///
/// `db.queryFailed` se la lettura o la scrittura falliscono, più quelli di
/// `genera`.
pub fn identita(
    connection: &Connection,
    genera: &dyn Fn() -> Result<String, AppError>,
) -> Result<String, AppError> {
    if let Some(id) =
        settings::read(connection, CHIAVE_IDENTITA)?.filter(|id| !id.trim().is_empty())
    {
        return Ok(id);
    }
    let id = genera()?;
    settings::write(connection, CHIAVE_IDENTITA, &id)?;
    Ok(id)
}

// ── esportare: la raccolta, sotto il lucchetto ──────────────────────────────

/// Tutto ciò che serve a scrivere un archivio, letto in una finestra sola.
///
/// La divisione in due fasi è la disciplina dei lucchetti scritta nei tipi:
/// [`raccogli`] gira **sotto** il lucchetto della libreria e costa
/// millisecondi, [`scrivi`] gira **fuori** da ogni lucchetto e può costare
/// minuti perché legge migliaia di jpeg dal disco. Se fossero una funzione
/// sola, esportare un profilo terrebbe ferma la riproduzione per tutto il
/// tempo del salvataggio.
#[derive(Debug, Clone)]
pub struct Raccolto {
    /// Le preferenze, nel formato v1 intatto.
    pub preferenze: SuDisco,
    /// Quali chiavi presenti nel database sono rimaste qui.
    pub lasciate: Vec<String>,
    /// Il documento di sincronia, già serializzato e compresso.
    ///
    /// Byte per byte quello che `aether_sync::documento::serializza`
    /// produrrebbe: è il riuso, e non una copia.
    pub sincronia: Vec<u8>,
    /// Quel che il documento di sincronia non copre.
    pub biblioteca: biblioteca::Biblioteca,
    /// L'identità di questa libreria.
    pub identita: String,
    /// Come si chiama questo dispositivo.
    pub dispositivo: String,
    /// Quando si è raccolto, in millisecondi.
    pub creato_ms: i64,
}

/// Legge dal database tutto quel che il profilo porta.
///
/// **Sotto il lucchetto**, e in una finestra sola: `allinea`, `contenuto` e la
/// raccolta della biblioteca devono descrivere la stessa libreria, non tre
/// istanti diversi.
///
/// `skin` e `bozze` sono le impronte dei pacchetti su disco, e arrivano da
/// fuori come già fa `sincronia::contenuto`: i pacchetti stanno in una cartella
/// e questo crate non la apre.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o l'allineamento falliscono; quelli di
/// `genera_identita`.
pub fn raccogli(
    connection: &mut Connection,
    dispositivo: &str,
    skin: BTreeMap<String, String>,
    bozze: BTreeMap<String, String>,
    adesso_ms: i64,
    genera_identita: &dyn Fn() -> Result<String, AppError>,
) -> Result<Raccolto, AppError> {
    let identita = identita(connection, genera_identita)?;
    crate::sincronia::allinea(connection, dispositivo, adesso_ms)?;

    let contenuto = crate::sincronia::contenuto(connection, dispositivo, skin, bozze)?;
    let documento = aether_sync::Documento::nuovo(dispositivo, contenuto, adesso_ms);
    let sincronia = aether_sync::documento::serializza(&documento)?;

    let biblioteca = biblioteca::raccogli(connection)?;

    let mut voci = BTreeMap::new();
    for (chiave, _) in CATALOGO {
        if let Some(valore) = settings::read(connection, chiave)? {
            voci.insert((*chiave).to_owned(), valore);
        }
    }
    let lasciate = lasciate(connection, &voci)?;

    Ok(Raccolto {
        preferenze: SuDisco {
            aether: FIRMA.to_owned(),
            versione: VERSIONE_V1,
            creato_ms: adesso_ms,
            voci,
        },
        lasciate,
        sincronia,
        biblioteca,
        identita,
        dispositivo: dispositivo.to_owned(),
        creato_ms: adesso_ms,
    })
}

/// Le chiavi che stanno nel database e che il catalogo non porta via.
fn lasciate(
    connection: &Connection,
    portate: &BTreeMap<String, String>,
) -> Result<Vec<String>, AppError> {
    let mut istruzione = connection
        .prepare("SELECT key FROM settings ORDER BY key")
        .map_err(|err| db_error("elenco delle impostazioni", &err))?;
    let righe = istruzione
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("lettura delle impostazioni", &err))?;

    let mut fuori = Vec::new();
    for riga in righe {
        let chiave = riga.map_err(|err| db_error("lettura di una impostazione", &err))?;
        if !portate.contains_key(&chiave) {
            fuori.push(chiave);
        }
    }
    Ok(fuori)
}

// ── esportare: la scrittura, fuori da ogni lucchetto ────────────────────────

/// Dove stanno i file che l'archivio deve inghiottire.
pub struct Sorgenti<'a> {
    /// Lo store delle copertine. `Stato::copertine()` non chiede il lucchetto,
    /// ed è la ragione per cui questa fase può stare fuori.
    pub copertine: &'a crate::covers::CoverStore,
    /// I pacchetti skin installati: identificativo → file `.aeskin`.
    pub skin: BTreeMap<String, PathBuf>,
    /// Le bozze dello Studio: identificativo → file `skin.json`.
    pub bozze: BTreeMap<String, PathBuf>,
}

/// Cos'è uscito.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Esportazione {
    /// Dove è finito.
    pub percorso: String,
    /// Quante preferenze ha portato via.
    pub voci: usize,
    /// Quali chiavi presenti nel database sono rimaste qui.
    ///
    /// Dichiarate e non taciute: un elenco di inclusioni silenzioso avrebbe lo
    /// stesso difetto di uno di esclusioni, spostato di un passo.
    pub lasciate: Vec<String>,
    /// Quante righe di cronologia.
    pub cronologia: usize,
    /// Quante copertine distinte.
    pub copertine: usize,
    /// Quanti pacchetti skin.
    pub skin: usize,
    /// Quante bozze dello Studio.
    pub bozze: usize,
    /// Quanto pesa il file.
    pub byte: u64,
    /// Pesa più di mezzo gigabyte.
    ///
    /// Si dice invece di scriverlo in silenzio: mezzo gigabyte è il punto in
    /// cui un file smette di essere una cosa che si mette su una chiavetta
    /// senza pensarci, e chi lo scopre dopo lo scopre mentre non c'è più posto.
    pub pesante: bool,
}

/// A che punto è un'esportazione o un'importazione.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passo {
    /// Quante voci sono state fatte.
    pub fatti: usize,
    /// Quante ne sono in tutto.
    pub totale: usize,
}

/// Scrive l'archivio, una voce alla volta.
///
/// **Fuori da ogni lucchetto.** Legge migliaia di jpeg dal disco e li copia
/// dentro lo zip senza tenerne in memoria più di uno alla volta.
///
/// # Errori
///
/// `fs.writeFailed` se la scrittura fallisce; `internal.unexpected` se la
/// serializzazione dei due documenti fallisce.
pub fn scrivi(
    raccolto: &Raccolto,
    sorgenti: &Sorgenti<'_>,
    destinazione: &Path,
    avanza: &dyn Fn(Passo),
) -> Result<Esportazione, AppError> {
    let impronte = raccolto.biblioteca.impronte();
    let totale = 3 + impronte.len() + sorgenti.skin.len() + sorgenti.bozze.len();
    let mut fatti = 0;
    let annota = |fatti: &mut usize| {
        *fatti += 1;
        avanza(Passo {
            fatti: *fatti,
            totale,
        });
    };

    let manifesto = archivio::Manifesto {
        aether: FIRMA.to_owned(),
        versione: archivio::VERSIONE,
        creato_ms: raccolto.creato_ms,
        identita: raccolto.identita.clone(),
        dispositivo: raccolto.dispositivo.clone(),
        applicazione: env!("CARGO_PKG_VERSION").to_owned(),
        abbinamento: "track_key".to_owned(),
        contiene: archivio::Contiene {
            preferenze: raccolto.preferenze.voci.len(),
            sincronia: true,
            biblioteca: true,
            copertine: impronte.len(),
            skin: sorgenti.skin.len(),
            bozze: sorgenti.bozze.len(),
        },
        fuori: FUORI.iter().map(|riga| (*riga).to_owned()).collect(),
    };

    let mut scrittore = archivio::Scrittore::crea(destinazione)?;

    scrittore.aggiungi_testo(archivio::NOME_MANIFESTO, &testo_json(&manifesto)?)?;
    annota(&mut fatti);
    scrittore.aggiungi_testo(
        archivio::NOME_PREFERENZE,
        &testo_json(&raccolto.preferenze)?,
    )?;
    annota(&mut fatti);
    scrittore.aggiungi_byte(
        &archivio::nome_sincronia(&raccolto.dispositivo),
        &raccolto.sincronia,
    )?;
    scrittore.aggiungi_testo(
        archivio::NOME_BIBLIOTECA,
        &biblioteca::serializza(&raccolto.biblioteca)?,
    )?;
    annota(&mut fatti);

    let mut copertine = 0;
    for hash in &impronte {
        // La piena e la miniatura. La miniatura non è un lusso: è quel che
        // l'elenco disegna, ed è sedici volte meno pixel — un profilo senza
        // miniature costringerebbe l'altro computer a ricodificarle tutte alla
        // prima apertura della libreria.
        if scrittore.aggiungi_file(
            &archivio::nome_copertina(hash, false),
            &sorgenti.copertine.path_for(hash),
        )? {
            copertine += 1;
        }
        scrittore.aggiungi_file(
            &archivio::nome_copertina(hash, true),
            &sorgenti.copertine.thumbnail_path_for(hash),
        )?;
        annota(&mut fatti);
    }

    let mut skin = 0;
    for (id, percorso) in &sorgenti.skin {
        if archivio::id_sicuro(id) && scrittore.aggiungi_file(&archivio::nome_skin(id), percorso)? {
            skin += 1;
        }
        annota(&mut fatti);
    }
    let mut bozze = 0;
    for (id, percorso) in &sorgenti.bozze {
        if archivio::id_sicuro(id)
            && scrittore.aggiungi_file(&archivio::nome_bozza(id), percorso)?
        {
            bozze += 1;
        }
        annota(&mut fatti);
    }

    let byte = scrittore.chiudi()?;

    Ok(Esportazione {
        percorso: destinazione.display().to_string(),
        voci: raccolto.preferenze.voci.len(),
        lasciate: raccolto.lasciate.clone(),
        cronologia: raccolto.biblioteca.cronologia.len(),
        copertine,
        skin,
        bozze,
        byte,
        pesante: byte > archivio::limiti::AVVISO,
    })
}

/// Un documento come testo indentato.
///
/// Indentato e non compatto: il manifesto e le preferenze sono le due voci che
/// una persona apre per guardarci dentro, e lo zip le comprime comunque —
/// l'indentazione costa qualche decina di byte dopo `deflate`.
fn testo_json<T: Serialize>(valore: &T) -> Result<String, AppError> {
    serde_json::to_string_pretty(valore).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("il profilo non si è serializzato".to_owned()),
        })
        .with_cause(err.to_string())
    })
}

// ── importare: leggere ──────────────────────────────────────────────────────

/// Un profilo letto, in tutte e due le versioni del formato.
#[derive(Debug, Clone)]
pub struct Letto {
    /// Il manifesto. Per un v1 è sintetico: quel formato non ne aveva uno.
    pub manifesto: archivio::Manifesto,
    /// Le preferenze.
    pub preferenze: SuDisco,
    /// Il documento di sincronia, se c'è.
    pub sincronia: Option<aether_sync::Documento>,
    /// Quel che il documento non copre.
    pub biblioteca: biblioteca::Biblioteca,
    /// Le copertine dentro l'archivio, per sapere quante ne mancherebbero qui.
    pub copertine: Vec<archivio::Ammessa>,
    /// Da dove viene.
    pub percorso: PathBuf,
}

impl Letto {
    /// Porta qualcosa oltre alle preferenze?
    #[must_use]
    pub fn ha_libreria(&self) -> bool {
        self.sincronia.is_some()
            || !self.biblioteca.cronologia.is_empty()
            || !self.biblioteca.correzioni.is_empty()
            || !self.biblioteca.testi.is_empty()
            || !self.biblioteca.desiderati.is_empty()
            || !self.biblioteca.copertine.is_empty()
    }
}

/// Apre un profilo, riconoscendo da sé quale dei due formati è.
///
/// Il riconoscimento passa dal **primo byte** e non dall'estensione, che
/// chiunque può cambiare: `PK\x03\x04` è un archivio, tutto il resto si prova a
/// leggere come il JSON del v1.
///
/// # Errori
///
/// `fs.readFailed` se il file non si apre; `settings.corrupt` se non è un
/// profilo, o se è di una versione che questa build non sa leggere.
pub fn leggi(percorso: &Path) -> Result<Letto, AppError> {
    if !archivio::e_un_archivio(percorso)? {
        let json = std::fs::read_to_string(percorso).map_err(|err| {
            AppError::new(ErrorCode::FsReadFailed {
                path: percorso.display().to_string(),
                detail: Some(err.kind().to_string()),
            })
            .with_cause(err.to_string())
        })?;
        return leggi_v1(&json, percorso);
    }

    let mut lettore = archivio::Lettore::apri(percorso)?;
    let manifesto = lettore.manifesto.clone();

    let preferenze = match lettore.voce_di(|voce| *voce == archivio::Voce::Preferenze) {
        Some(voce) => {
            let byte = lettore.byte(&voce)?;
            let documento: SuDisco = serde_json::from_slice(&byte).map_err(|err| {
                corrotto(format!("le preferenze del profilo non si leggono: {err}"))
            })?;
            controlla_firma(&documento)?;
            documento
        }
        // Un archivio senza preferenze non è un guasto: è un profilo di una
        // libreria che non ne aveva nessuna da portare. Le altre parti si
        // leggono lo stesso.
        None => SuDisco {
            aether: FIRMA.to_owned(),
            versione: VERSIONE_V1,
            creato_ms: manifesto.creato_ms,
            voci: BTreeMap::new(),
        },
    };

    let sincronia = match lettore.voce_di(|voce| matches!(voce, archivio::Voce::Sincronia(_))) {
        Some(voce) => {
            let byte = lettore.byte(&voce)?;
            Some(aether_sync::documento::interpreta(&byte)?)
        }
        None => None,
    };

    let biblioteca = match lettore.voce_di(|voce| *voce == archivio::Voce::Biblioteca) {
        Some(voce) => {
            let byte = lettore.byte(&voce)?;
            biblioteca::interpreta(&byte)?
        }
        None => biblioteca::Biblioteca::default(),
    };

    let copertine = lettore.voci_di(|voce| {
        matches!(
            voce,
            archivio::Voce::Copertina {
                miniatura: false,
                ..
            }
        )
    });

    Ok(Letto {
        manifesto,
        preferenze,
        sincronia,
        biblioteca,
        copertine,
        percorso: percorso.to_owned(),
    })
}

/// Legge il formato v1: un JSON, e nient'altro dentro.
///
/// Si legge ancora e non si scrive più. Non è compatibilità per abitudine: un
/// profilo v1 è un file che qualcuno ha su una chiavetta da prima di questo
/// aggiornamento, e la cosa peggiore che una versione nuova possa fare a un
/// file vecchio è non aprirlo.
fn leggi_v1(json: &str, percorso: &Path) -> Result<Letto, AppError> {
    let documento: SuDisco = serde_json::from_str(json)
        .map_err(|err| corrotto(format!("il file non è un profilo di Aether: {err}")))?;
    controlla_firma(&documento)?;
    if documento.versione > VERSIONE_V1 {
        return Err(corrotto(format!(
            "profilo di formato {} scritto da una versione più recente: questa legge fino al {VERSIONE_V1}",
            documento.versione
        )));
    }
    Ok(Letto {
        manifesto: archivio::Manifesto {
            aether: FIRMA.to_owned(),
            versione: documento.versione,
            creato_ms: documento.creato_ms,
            // Un v1 non porta libreria, quindi non ha niente da riattaccare e
            // l'identità non gli serve. Vuota, e il confronto la salta: un
            // profilo che porta solo preferenze le porta a chiunque.
            identita: String::new(),
            dispositivo: String::new(),
            applicazione: String::new(),
            abbinamento: "track_key".to_owned(),
            contiene: archivio::Contiene {
                preferenze: documento.voci.len(),
                ..archivio::Contiene::default()
            },
            fuori: Vec::new(),
        },
        preferenze: documento,
        sincronia: None,
        biblioteca: biblioteca::Biblioteca::default(),
        copertine: Vec::new(),
        percorso: percorso.to_owned(),
    })
}

/// La firma dice che è un profilo di Aether?
fn controlla_firma(documento: &SuDisco) -> Result<(), AppError> {
    if documento.aether == FIRMA {
        return Ok(());
    }
    Err(corrotto(format!(
        "il file dice di essere «{}», non un profilo di Aether",
        documento.aether
    )))
}

/// Il file non è un profilo leggibile.
fn corrotto(causa: String) -> AppError {
    AppError::new(ErrorCode::SettingsCorrupt {
        quarantined_as: None,
    })
    .with_cause(causa)
}

// ── importare: il piano, e l'esecuzione ─────────────────────────────────────

/// Una chiave che l'importazione cambierebbe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cambio {
    /// Quale chiave.
    pub chiave: String,
    /// Cos'è.
    pub genere: Genere,
    /// Cosa c'è adesso. `None` se la chiave non c'è.
    pub prima: Option<String>,
    /// Cosa ci sarebbe.
    pub dopo: String,
}

/// Quante cose la parte di libreria porterebbe.
///
/// Una struttura piatta e non i due tipi che stanno sotto — `Cambiamenti` della
/// sincronia e `Portato` della biblioteca — perché è quel che la finestra
/// legge, e due oggetti annidati con due convenzioni di nome diverse sono due
/// modi di sbagliare a scrivere un campo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Portati {
    /// Brani il cui conteggio d'ascolto cambierebbe.
    pub ascolti: usize,
    /// Voti che arriverebbero.
    pub voti: usize,
    /// Cuoricini messi o tolti.
    pub preferiti: usize,
    /// Posizioni di riascolto aggiornate.
    pub posizioni: usize,
    /// Playlist create o rifatte.
    pub playlist: usize,
    /// Cartelle sorvegliate aggiunte.
    pub cartelle: usize,
    /// Righe di cronologia nuove.
    pub cronologia: usize,
    /// Correzioni ai metadati.
    pub correzioni: usize,
    /// Testi.
    pub testi: usize,
    /// Brani aggiunti all'elenco di quelli che mancano.
    pub desiderati: usize,
    /// Brani che guadagnerebbero una copertina.
    pub copertine: usize,
    /// File di copertina che qui non ci sono e si scriverebbero.
    pub file_copertine: usize,
}

impl Portati {
    /// La parte di libreria non porterebbe niente.
    #[must_use]
    pub const fn e_vuoto(&self) -> bool {
        self.ascolti == 0
            && self.voti == 0
            && self.preferiti == 0
            && self.posizioni == 0
            && self.playlist == 0
            && self.cartelle == 0
            && self.cronologia == 0
            && self.correzioni == 0
            && self.testi == 0
            && self.desiderati == 0
            && self.copertine == 0
            && self.file_copertine == 0
    }
}

/// Cosa farebbe l'importazione.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Piano {
    /// Quando il profilo è stato scritto.
    pub creato_ms: i64,
    /// Quale formato è: 1 il JSON di prima, 2 l'archivio.
    pub versione: u32,
    /// Il profilo viene da un'altra libreria.
    ///
    /// Quando è vero la parte di libreria **non** si applica: le chiavi di
    /// brano di là nominano canzoni che qui non ci sono. Preferenze, copertine
    /// e pacchetti skin passano lo stesso, perché non parlano di brani.
    pub identita_diversa: bool,
    /// Le chiavi che cambierebbero.
    pub cambi: Vec<Cambio>,
    /// Le chiavi del file che questa versione non porta.
    ///
    /// Non è un guasto: è un profilo scritto da una versione che ne conosceva
    /// una in più, o una chiave tolta dal catalogo. Si dice, e si tira dritto.
    pub sconosciute: Vec<String>,
    /// I percorsi che arrivano e che su questo computer non esistono.
    pub percorsi_mancanti: Vec<String>,
    /// Quante chiavi sono già uguali a quel che c'è.
    pub invariate: usize,
    /// Le radici da rimappare, con quanti brani di qui stanno sotto ciascuna.
    pub rimappature: Vec<Rimappatura>,
    /// Percorsi di brani riscritti dalla rimappatura.
    pub percorsi_riscritti: usize,
    /// Brani il cui file, sotto il prefisso nuovo, non si trova.
    pub brani_irrintracciabili: usize,
    /// Brani il cui percorso nuovo è già di un'altra riga.
    pub brani_gia_presenti: usize,
    /// Quante cose la parte di libreria porterebbe.
    pub portati: Portati,
}

impl Piano {
    /// L'importazione non cambierebbe niente.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        self.cambi.is_empty() && self.portati.e_vuoto() && self.percorsi_riscritti == 0
    }
}

/// Quel che il profilo ha bisogno di sapere di questo computer.
pub struct Ambiente<'a> {
    /// Questo percorso c'è su questo computer?
    ///
    /// Gliela si passa invece di guardare il disco da qui, così le prove
    /// girano senza avere le cartelle di nessuno.
    pub esiste: &'a dyn Fn(&str) -> bool,
    /// Adesso, in millisecondi.
    pub adesso_ms: i64,
    /// Come si chiama questo dispositivo.
    pub dispositivo: &'a str,
    /// Le rimappature che chi importa ha scelto. Vuote alla prima lettura.
    pub rimappature: &'a [Rimappatura],
    /// Lo store delle copertine, per contare quante ne mancano davvero.
    pub copertine: Option<&'a crate::covers::CoverStore>,
}

/// Cosa cambierebbe importare questo profilo. Non applica niente.
///
/// Esegue davvero, dentro una transazione che viene abbandonata: i numeri che
/// si leggono non sono una previsione fatta da un'altra parte del codice, sono
/// il risultato. L'unica scrittura che resta è
/// [`crate::sincronia::allinea`] — vedi il `//!`.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o l'allineamento falliscono.
pub fn piano(
    connection: &mut Connection,
    letto: &Letto,
    ambiente: &Ambiente<'_>,
) -> Result<Piano, AppError> {
    esegui(connection, letto, ambiente, false)
}

/// Applica il profilo, e restituisce lo stesso piano di [`piano`].
///
/// # Errori
///
/// Come [`piano`], più `db.queryFailed` se la scrittura non riesce.
pub fn importa(
    connection: &mut Connection,
    letto: &Letto,
    ambiente: &Ambiente<'_>,
) -> Result<Piano, AppError> {
    esegui(connection, letto, ambiente, true)
}

/// Il corpo di [`piano`] e [`importa`]: la stessa cosa, e una delle due la butta.
fn esegui(
    connection: &mut Connection,
    letto: &Letto,
    ambiente: &Ambiente<'_>,
    conferma: bool,
) -> Result<Piano, AppError> {
    // Fuori dalla transazione, e per una ragione tecnica prima che di
    // disciplina: `allinea` apre la propria. Vedi il `//!` sul perché sta anche
    // nel piano invece che solo nell'esecuzione.
    crate::sincronia::allinea(connection, ambiente.dispositivo, ambiente.adesso_ms)?;

    let mia = settings::read(connection, CHIAVE_IDENTITA)?.filter(|id| !id.trim().is_empty());
    let sua = letto.manifesto.identita.trim();
    // Un profilo che non porta libreria non ha un'identità da confrontare, e
    // chiedergliela vorrebbe dire rifiutare le preferenze di un v1.
    let identita_diversa =
        letto.ha_libreria() && !sua.is_empty() && mia.is_some_and(|id| id != sua);

    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione del profilo", &err))?;

    let mut piano = Piano {
        creato_ms: letto.manifesto.creato_ms,
        versione: letto.manifesto.versione,
        identita_diversa,
        ..Piano::default()
    };

    // ── le rimappature, prima di tutto: cambiano i valori che si scrivono ──
    let radici_del_profilo = letto
        .preferenze
        .voci
        .get("library.roots")
        .map(|valore| percorsi_di(valore))
        .unwrap_or_default();
    piano.rimappature = radici::proposte(&tx, &radici_del_profilo, ambiente.esiste)?;

    let riscritti = radici::applica_in(&tx, ambiente.rimappature, ambiente.esiste)?;
    piano.percorsi_riscritti = riscritti.percorsi;
    piano.brani_irrintracciabili = riscritti.irrintracciabili;
    piano.brani_gia_presenti = riscritti.gia_presenti;

    // ── le preferenze ──
    for (chiave, valore) in &letto.preferenze.voci {
        let Some((_, genere)) = CATALOGO.iter().find(|(nome, _)| nome == chiave) else {
            piano.sconosciute.push(chiave.clone());
            continue;
        };
        let valore = if *genere == Genere::Percorso {
            radici::rimappa_valore(valore, ambiente.rimappature)
        } else {
            valore.clone()
        };
        let prima = settings::read(&tx, chiave)?;
        if prima.as_ref() == Some(&valore) {
            piano.invariate += 1;
            continue;
        }
        if *genere == Genere::Percorso {
            piano.percorsi_mancanti.extend(
                percorsi_di(&valore)
                    .into_iter()
                    .filter(|p| !(ambiente.esiste)(p)),
            );
        }
        settings::write(&tx, chiave, &valore)?;
        piano.cambi.push(Cambio {
            chiave: chiave.clone(),
            genere: *genere,
            prima,
            dopo: valore,
        });
    }

    // ── la libreria, se le due identità si riconoscono ──
    if !identita_diversa {
        if let Some(originale) = &letto.sincronia {
            // Le cartelle sorvegliate del documento sono percorsi di quella
            // macchina, e vanno rimappate **prima** della fusione. Senza,
            // `applica_cartelle` riscriverebbe `library.roots` dalla mappa
            // fusa e rimetterebbe dentro il prefisso vecchio subito dopo che
            // la preferenza l'aveva sostituito — cioè la rimappatura
            // funzionerebbe sui brani e non sulle radici, che è il modo più
            // confuso possibile di non funzionare.
            let mut suo = originale.clone();
            if !ambiente.rimappature.is_empty() {
                suo.content.cartelle = std::mem::take(&mut suo.content.cartelle)
                    .into_iter()
                    .map(|(percorso, stato)| {
                        (
                            radici::rimappa_percorso(&percorso, ambiente.rimappature),
                            stato,
                        )
                    })
                    .collect();
            }
            let mio = aether_sync::Documento::nuovo(
                ambiente.dispositivo,
                // Le impronte di skin e bozze si passano vuote: `applica_in`
                // non le scrive nel database — servono al motore della
                // sincronia per decidere cosa caricare, e qui non si carica
                // niente. Riempirle vorrebbe dire aprire una cartella da un
                // crate che non deve aprirla.
                crate::sincronia::contenuto(
                    &tx,
                    ambiente.dispositivo,
                    BTreeMap::new(),
                    BTreeMap::new(),
                )?,
                ambiente.adesso_ms,
            );
            let fuso = aether_sync::fondi(&[mio, suo]);
            let cambiamenti = crate::sincronia::applica_in(&tx, &fuso, ambiente.adesso_ms)?;
            piano.portati.ascolti = cambiamenti.ascolti;
            piano.portati.voti = cambiamenti.voti;
            piano.portati.preferiti = cambiamenti.preferiti;
            piano.portati.posizioni = cambiamenti.posizioni;
            piano.portati.playlist = cambiamenti.playlist;
            piano.portati.cartelle = cambiamenti.cartelle;
        }

        let portato = biblioteca::applica_in(&tx, &letto.biblioteca, ambiente.adesso_ms)?;
        piano.portati.cronologia = portato.cronologia;
        piano.portati.correzioni = portato.correzioni;
        piano.portati.testi = portato.testi;
        piano.portati.desiderati = portato.desiderati;
        piano.portati.copertine = portato.copertine;

        // Una libreria senza identità adotta quella del profilo: è la
        // reinstallazione e il secondo computer, cioè i due casi per cui il
        // profilo esiste. Senza questa riga un reimport successivo si vedrebbe
        // rifiutare la parte di libreria che aveva appena scritto lui.
        if !sua.is_empty() && settings::read(&tx, CHIAVE_IDENTITA)?.is_none() {
            settings::write(&tx, CHIAVE_IDENTITA, sua)?;
        }
    }

    // ── e i percorsi si rileggono, perché la sincronia ha l'ultima parola ──
    //
    // `applica_cartelle` riscrive `library.roots` dalla mappa datata delle
    // cartelle, che è l'**unione** di quelle di qui e di quelle del profilo, e
    // gira dopo il ciclo delle preferenze. È giusto che sia lui a decidere: la
    // preferenza sostituirebbe l'elenco, la mappa lo unisce, e un profilo non
    // deve poter togliere una cartella sorvegliata.
    //
    // Ma allora quel che il ciclo ha scritto in `Cambio::dopo` non è più il
    // valore finale, e un piano che mostra un valore diverso da quello che
    // resta non è «l'esecuzione annullata»: è una previsione, cioè la cosa che
    // questo modulo ha sempre rifiutato di essere. Quindi si rilegge, e chi era
    // un cambiamento e non lo è più torna fra le invariate.
    let mut invariate_in_piu = 0;
    piano.cambi.retain_mut(|cambio| {
        if cambio.genere != Genere::Percorso {
            return true;
        }
        if let Ok(Some(adesso)) = settings::read(&tx, &cambio.chiave) {
            cambio.dopo = adesso;
        }
        if cambio.prima.as_ref() == Some(&cambio.dopo) {
            invariate_in_piu += 1;
            return false;
        }
        true
    });
    piano.invariate += invariate_in_piu;

    // I file di copertina che mancano si contano guardando il disco, che è
    // l'unico posto che lo sa. Non si estrae niente: estrarre nel piano
    // vorrebbe dire scrivere durante una funzione che promette di non farlo.
    piano.portati.file_copertine = ambiente.copertine.map_or(0, |store| {
        letto
            .copertine
            .iter()
            .filter(|voce| match &voce.voce {
                archivio::Voce::Copertina { hash, .. } => !store.path_for(hash).exists(),
                _ => false,
            })
            .count()
    });

    if conferma {
        tx.commit()
            .map_err(|err| db_error("chiusura della transazione del profilo", &err))?;
    } else {
        tx.rollback()
            .map_err(|err| db_error("abbandono della transazione del profilo", &err))?;
    }
    Ok(piano)
}

/// I percorsi dentro un valore.
///
/// Due forme, perché nel database ce ne sono due: `library.roots` è un elenco
/// JSON, `download.folder` è un percorso e basta. Distinguerle guardando la
/// chiave sarebbe una terza tabella da tenere allineata; guardare il valore
/// funziona e non ha niente da allineare.
fn percorsi_di(valore: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(valore)
        .ok()
        .unwrap_or_else(|| vec![valore.to_owned()])
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect()
}

// ── importare: gli allegati, fuori da ogni lucchetto ────────────────────────

/// Estrae le copertine nello store, **solo quelle che qui non ci sono**.
///
/// Fuori da ogni lucchetto: `Stato::copertine()` non lo chiede, e questa è la
/// fase che può durare minuti su una libreria grande.
///
/// # Errori
///
/// `settings.corrupt` se l'archivio non si legge, `fs.writeFailed` se una
/// scrittura fallisce.
pub fn estrai_copertine(
    percorso: &Path,
    store: &crate::covers::CoverStore,
    avanza: &dyn Fn(Passo),
) -> Result<usize, AppError> {
    if !archivio::e_un_archivio(percorso)? {
        return Ok(0);
    }
    let mut lettore = archivio::Lettore::apri(percorso)?;
    let voci = lettore.voci_di(|voce| matches!(voce, archivio::Voce::Copertina { .. }));
    let totale = voci.len();
    let mut scritte = 0;
    for (fatti, voce) in voci.iter().enumerate() {
        let archivio::Voce::Copertina { hash, miniatura } = &voce.voce else {
            continue;
        };
        let destinazione = if *miniatura {
            store.thumbnail_path_for(hash)
        } else {
            store.path_for(hash)
        };
        if lettore.estrai(voce, &destinazione)? && !*miniatura {
            scritte += 1;
        }
        avanza(Passo {
            fatti: fatti + 1,
            totale,
        });
    }
    Ok(scritte)
}

/// Chiama `azione` per ogni pacchetto skin e ogni bozza dell'archivio.
///
/// Uno alla volta, mai tutti insieme: un `.aeskin` può pesare venti megabyte, e
/// una manciata di skin tenute in memoria insieme sarebbe la stessa quantità di
/// RAM che questa release si è impegnata a non consumare.
///
/// `azione` riceve l'identificativo, se è una bozza, e i byte; restituisce
/// `true` se ha scritto. **La validazione non sta qui**: un `.aeskin` si valida
/// con `aether_skin::read_skin_package`, e chi lo fa è chi lo installa —
/// `aether_cloud::pacchetti`, che porta già le tre guardie e la regola «non si
/// sovrascrive mai».
///
/// # Errori
///
/// `settings.corrupt` se l'archivio non si legge, più quelli di `azione`.
pub fn per_ogni_pacchetto(
    percorso: &Path,
    mut azione: impl FnMut(&str, bool, &[u8]) -> Result<bool, AppError>,
) -> Result<(usize, usize), AppError> {
    if !archivio::e_un_archivio(percorso)? {
        return Ok((0, 0));
    }
    let mut lettore = archivio::Lettore::apri(percorso)?;
    let voci =
        lettore.voci_di(|voce| matches!(voce, archivio::Voce::Skin(_) | archivio::Voce::Bozza(_)));
    let mut skin = 0;
    let mut bozze = 0;
    for voce in &voci {
        let (id, bozza) = match &voce.voce {
            archivio::Voce::Skin(id) => (id.clone(), false),
            archivio::Voce::Bozza(id) => (id.clone(), true),
            _ => continue,
        };
        let byte = lettore.byte(voce)?;
        if azione(&id, bozza, &byte)? {
            if bozza {
                bozze += 1;
            } else {
                skin += 1;
            }
        }
    }
    Ok((skin, bozze))
}

// ── la copia di sicurezza ───────────────────────────────────────────────────

/// Il nome della cartella, dentro i dati, in cui stanno le copie di sicurezza.
pub const CARTELLA_COPIE: &str = "profilo";

/// Quante copie si tengono.
///
/// Tre. Non una: chi importa un profilo sbagliato spesso se ne accorge dopo
/// averne importato un secondo per rimediare, e con una sola copia il rimedio
/// avrebbe cancellato l'unico modo di tornare indietro. Non dieci: sono
/// archivi che possono pesare centinaia di megabyte l'uno, e riempire il disco
/// di chi importa è un modo di aiutarlo che non aiuta.
pub const COPIE_TENUTE: usize = 3;

/// Il percorso della copia di sicurezza da scrivere adesso.
#[must_use]
pub fn copia_di_sicurezza(data_dir: &Path, adesso_ms: i64) -> PathBuf {
    data_dir
        .join(CARTELLA_COPIE)
        .join(format!("prima-{adesso_ms}.{}", archivio::ESTENSIONE))
}

/// Tiene le [`COPIE_TENUTE`] più recenti e butta le altre.
///
/// Restituisce quante ne ha tolte. Un file che non si cancella non è un guasto
/// dell'importazione: al peggio resta una copia in più, che è il verso giusto
/// dell'errore.
pub fn ruota_le_copie(data_dir: &Path) -> usize {
    let cartella = data_dir.join(CARTELLA_COPIE);
    let Ok(voci) = std::fs::read_dir(&cartella) else {
        return 0;
    };
    let mut copie: Vec<PathBuf> = voci
        .flatten()
        .map(|voce| voce.path())
        .filter(|percorso| {
            percorso
                .file_name()
                .and_then(|nome| nome.to_str())
                .is_some_and(|nome| {
                    nome.starts_with("prima-") && nome.ends_with(archivio::ESTENSIONE)
                })
        })
        .collect();
    // Per nome e non per data di modifica: il nome porta il millisecondo in cui
    // la copia è stata scritta, e un `mtime` si può cambiare copiando la
    // cartella da un'altra parte.
    copie.sort();
    let mut tolte = 0;
    while copie.len() > COPIE_TENUTE {
        let vecchia = copie.remove(0);
        if std::fs::remove_file(&vecchia).is_ok() {
            tolte += 1;
        }
    }
    tolte
}

/// L'ultima copia di sicurezza scritta, se ce n'è una.
#[must_use]
pub fn ultima_copia(data_dir: &Path) -> Option<PathBuf> {
    let cartella = data_dir.join(CARTELLA_COPIE);
    let mut copie: Vec<PathBuf> = std::fs::read_dir(&cartella)
        .ok()?
        .flatten()
        .map(|voce| voce.path())
        .filter(|percorso| {
            percorso
                .file_name()
                .and_then(|nome| nome.to_str())
                .is_some_and(|nome| {
                    nome.starts_with("prima-") && nome.ends_with(archivio::ESTENSIONE)
                })
        })
        .collect();
    copie.sort();
    copie.pop()
}

/// Rimette **solo** le preferenze di una copia di sicurezza.
///
/// È tutto quel che un annullamento può fare, e il nome della funzione lo dice
/// invece di prometterlo e basta: la parte di libreria è **additiva** — ascolti
/// sommati, playlist fuse, testi scritti — e disfarla vorrebbe dire sapere
/// quali righe c'erano prima, cioè conservare un giornale grande quanto
/// l'importazione. Non lo si conserva, e chi preme «annulla» deve leggere che
/// torna indietro la configurazione e non la storia.
///
/// # Errori
///
/// Quelli di [`leggi`], più `db.queryFailed` se la scrittura fallisce.
pub fn annulla(
    connection: &mut Connection,
    copia: &Path,
    ambiente: &Ambiente<'_>,
) -> Result<Piano, AppError> {
    let mut letto = leggi(copia)?;
    // Si butta via tutto il resto **prima** di applicare: così non c'è nessun
    // ramo dell'esecuzione in cui l'annullamento possa scrivere una riga di
    // libreria, e non è una promessa nel commento ma una struttura vuota.
    letto.sincronia = None;
    letto.biblioteca = biblioteca::Biblioteca::default();
    letto.copertine = Vec::new();
    importa(connection, &letto, ambiente)
}

#[cfg(test)]
mod prove;
