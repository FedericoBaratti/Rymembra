//! La passata di backup e quella di ripristino, messe in fila.
//!
//! Qui si decide **cosa** caricare e **in che ordine**; il come lo sanno
//! [`crate::drive`] e [`crate::pacchetti`], e il cosa significhi
//! [`aether_app::backup`].
//!
//! # Tre tipi di file, non un archivio unico
//!
//! ```text
//! appDataFolder/
//!   aether-libreria.v1.json.gz     ~70 KB per 1 400 brani, cambia in continuazione
//!   aether-skin-<id>.aeskin        fino a 20 MB, non cambia quasi mai
//!   aether-bozza-<id>.json.gz      il solo manifest della bozza
//! ```
//!
//! Un archivio solo sarebbe più semplice da scrivere e rispedirebbe venti
//! megabyte **a ogni cuoricino**. Separati, un voto messo costa settanta
//! chilobyte e una skin caricata una volta resta caricata.
//!
//! # «Non è cambiato niente», in una chiamata sola
//!
//! Ogni file porta la sua impronta in `appProperties`. Una sola `files.list`
//! restituisce tutto quel che possediamo con l'impronta che **noi** abbiamo
//! calcolato: confrontarla con quella di adesso dice, senza scaricare niente, se
//! c'è qualcosa da fare. Su una libreria ferma, una passata è una richiesta HTTP
//! e nient'altro.
//!
//! # Il file remoto non regredisce mai
//!
//! Prima di sovrascrivere si guarda **chi** l'ha scritto. Se è stato un altro
//! dispositivo, si scarica il suo, lo si fonde in memoria con il nostro e si
//! carica l'unione: gli ascolti dell'altro computer non si perdono per il solo
//! fatto che questo ha salvato per ultimo.
//!
//! Il database locale, invece, **non si tocca**. Per vedere qui quegli ascolti
//! bisogna premere «Ripristina», ed è deliberato: una sincronia che riscrive la
//! libreria da sola è una sincronia di cui non ci si accorge finché non ha
//! cancellato qualcosa.
//!
//! # Due dispositivi nello stesso istante
//!
//! Il controllo «chi l'ha scritto» qui sopra ha una fessura: fra la lettura e
//! la scrittura passa un secondo, e in quel secondo l'altro computer può
//! scrivere anche lui. Drive, poi, **permette** due file con lo stesso nome, e
//! due dispositivi che salvano per la prima volta insieme ne creano due. Fino
//! alla 2.4.0 la risposta era «vince l'ultimo dell'elenco», e i dati dell'altro
//! restavano in un file che nessuno avrebbe più letto.
//!
//! Adesso le copie del file dei metadati si **fondono tutte**, e dopo aver
//! scritto si **rilegge** l'elenco: se c'è una copia che non è la nostra, o la
//! nostra ha un'impronta diversa da quella appena scritta, qualcuno è passato
//! in mezzo e si rifà il giro con quel che c'è adesso. Le copie in più, una
//! volta fuse, si cancellano. La fusione è idempotente e commutativa, quindi
//! rifarla non costa altro che una lettura. Vedi [`salva`].

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::Path;

use aether_app::backup::{self, Contenuto, Salvataggio, StatoLocale};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::restore::RestorePlan;
use aether_sync::Magazzino;

use crate::drive::{self, FileRemoto};
use crate::pacchetti;

/// Il nome del file dei metadati.
///
/// La versione sta nel **nome** e non solo dentro: il giorno in cui il formato
/// cambiasse in modo non leggibile all'indietro, una versione vecchia di Aether
/// continuerebbe a trovare e a scrivere il suo `.v1`, invece di litigare con un
/// file che non capisce.
pub const NOME_LIBRERIA: &str = "aether-libreria.v1.json.gz";

/// Il prefisso dei pacchetti di skin.
const PREFISSO_SKIN: &str = "aether-skin-";

/// Il prefisso dei manifest di bozza.
const PREFISSO_BOZZA: &str = "aether-bozza-";

/// Quanto si accetta che un file compresso diventi una volta aperto.
///
/// Sessantaquattro mebibyte: molto più di qualunque backup vero, molto meno di
/// quel che serve a una bomba di decompressione per riempire la memoria. Il file
/// viene dalla rete, e «l'abbiamo scritto noi» non è una garanzia su cosa ci sia
/// dentro adesso.
const LIMITE_DECOMPRESSO: u64 = 64 * 1024 * 1024;

/// Quante volte, al più, si rilegge l'elenco e si rifonde dopo aver scritto.
///
/// Tre: il primo giro scrive, il secondo assorbe chi ha scritto in mezzo, il
/// terzo copre il caso raro di un terzo dispositivo. Oltre, ci si ferma con
/// quel che si ha: la passata dopo — fra un quarto d'ora — rifà la stessa cosa
/// su un elenco che nel frattempo si sarà calmato.
const GIRI_DI_FUSIONE: usize = 3;

/// Il nome su Drive del pacchetto di una skin.
#[must_use]
pub(crate) fn nome_skin(id: &str) -> String {
    format!("{PREFISSO_SKIN}{id}.aeskin")
}

/// Il nome su Drive del manifest di una bozza.
#[must_use]
pub(crate) fn nome_bozza(id: &str) -> String {
    format!("{PREFISSO_BOZZA}{id}.json.gz")
}

/// L'identificatore dentro il nome di un pacchetto di skin.
#[must_use]
pub(crate) fn id_da_nome_skin(nome: &str) -> Option<&str> {
    nome.strip_prefix(PREFISSO_SKIN)?.strip_suffix(".aeskin")
}

/// L'identificatore dentro il nome di un manifest di bozza.
#[must_use]
pub(crate) fn id_da_nome_bozza(nome: &str) -> Option<&str> {
    nome.strip_prefix(PREFISSO_BOZZA)?.strip_suffix(".json.gz")
}

/// A che punto è una passata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cosa {
    /// Il file dei metadati.
    Metadati,
    /// I pacchetti di skin.
    Skin,
    /// I manifest delle bozze.
    Bozze,
}

impl Cosa {
    /// Il nome che attraversa l'IPC.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Metadati => "metadati",
            Self::Skin => "skin",
            Self::Bozze => "bozze",
        }
    }
}

/// Un passo avanti, da riferire a chi guarda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Avanzamento {
    /// Quanti ne sono stati fatti.
    pub fatti: usize,
    /// Quanti in tutto.
    pub totale: usize,
    /// Di cosa si tratta.
    pub cosa: Cosa,
}

/// Com'è andata una passata di salvataggio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passata {
    /// Quanti file sono stati caricati.
    pub caricati: usize,
    /// Quanti erano già lassù identici.
    pub saltati: usize,
    /// L'identificativo del file dei metadati su Drive.
    pub file_id: String,
    /// L'impronta del contenuto appena caricato.
    ///
    /// Chi chiama la scrive in `settings`: è ciò che permette alla passata
    /// successiva di sapere cosa aveva lasciato lassù.
    pub impronta: String,
}

/// Il backup scaricato, pronto per essere pianificato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaRipristinare {
    /// Quel che il file dei metadati dice.
    pub contenuto: Contenuto,
    /// Quando è stato scritto, in millisecondi.
    pub generato_ms: i64,
    /// Da quale dispositivo.
    pub generato_da: String,
    /// I pacchetti di skin disponibili, per identificatore.
    pub skin: BTreeMap<String, FileRemoto>,
    /// I manifest di bozza disponibili, per identificatore.
    pub bozze: BTreeMap<String, FileRemoto>,
}

/// Quanti file di skin e di bozza sono stati scritti sul disco.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scritti {
    /// Le skin installate adesso.
    pub skin: usize,
    /// Le bozze scritte adesso.
    pub bozze: usize,
    /// Gli identificatori che il piano chiedeva e che lassù non c'erano.
    ///
    /// Succede quando una passata di salvataggio si è interrotta fra il
    /// caricamento dei metadati e quello dei pacchetti. Non è un guasto del
    /// ripristino: è un pezzo che manca, e va detto invece che far fallire tutto
    /// il resto.
    ///
    /// Ci finisce anche un pacchetto che c'era ma **non è sceso**, o non si è
    /// validato: fino alla 2.4.0 il primo scarico fallito fermava l'intero
    /// ripristino, e una skin da quindici megabyte oltre il tetto del client
    /// voleva dire nessuna playlist ripristinata.
    pub mancanti: Vec<String>,
    /// Perché ognuno dei mancanti che c'era non è arrivato: `nome — codice:
    /// causa`, una riga per file, per il diario.
    ///
    /// Solo codici e cause, che non contengono né token né contenuti.
    pub guasti: Vec<String>,
}

/// Salva la libreria e i pacchetti su Drive.
///
/// **Nessuna `Connection` attraversa questa funzione**, ed è la ragione per cui
/// il crate esiste separato: chi chiama legge il database sotto il lucchetto,
/// lo rilascia, e solo dopo arriva qui. Tutto ciò che segue dura secondi o
/// minuti, e il lettore continua a suonare.
///
/// # Errori
///
/// `net.*` per la rete; `net.badSchema` se lassù c'è un backup scritto da una
/// versione di Aether più nuova — nel qual caso **non si sovrascrive niente**.
pub fn salva<M: Magazzino + ?Sized>(
    drive: &M,
    data_dir: &Path,
    stato: &StatoLocale,
    dispositivo: &str,
    adesso_ms: i64,
    mut avanzamento: impl FnMut(Avanzamento),
) -> Result<Passata, AppError> {
    let skin_locali = pacchetti::elenca_skin(data_dir);
    let bozze_locali = pacchetti::elenca_bozze(data_dir);
    let contenuto = backup::snapshot(stato, skin_locali.clone(), bozze_locali.clone());

    let elenco = drive.elenca()?;
    let remoti = per_nome(elenco.clone());
    let mut caricati = 0usize;
    let mut saltati = 0usize;

    // ── i metadati ──────────────────────────────────────────────────────────
    avanzamento(Avanzamento {
        fatti: 0,
        totale: 1,
        cosa: Cosa::Metadati,
    });
    let libreria = salva_la_libreria(drive, elenco, &contenuto, dispositivo, adesso_ms)?;
    if libreria.caricati == 0 {
        saltati = saltati.saturating_add(1);
    }
    caricati = caricati.saturating_add(libreria.caricati);
    let file_id = libreria.file_id;
    let nostra = libreria.impronta;
    avanzamento(Avanzamento {
        fatti: 1,
        totale: 1,
        cosa: Cosa::Metadati,
    });

    // ── le skin ─────────────────────────────────────────────────────────────
    let totale = skin_locali.len();
    for (fatti, (id, impronta)) in skin_locali.iter().enumerate() {
        let nome = nome_skin(id);
        let remoto = remoti.get(&nome);
        if remoto.is_some_and(|voce| voce.impronta.as_deref() == Some(impronta.as_str())) {
            saltati = saltati.saturating_add(1);
        } else {
            let byte = pacchetti::leggi_skin(data_dir, id)?;
            drive.scrivi(
                &nome,
                remoto.map(|voce| voce.id.as_str()),
                "application/zip",
                &byte,
                impronta,
            )?;
            caricati = caricati.saturating_add(1);
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Skin,
        );
    }

    // ── le bozze ────────────────────────────────────────────────────────────
    let totale = bozze_locali.len();
    for (fatti, (id, impronta)) in bozze_locali.iter().enumerate() {
        let nome = nome_bozza(id);
        let remoto = remoti.get(&nome);
        if remoto.is_some_and(|voce| voce.impronta.as_deref() == Some(impronta.as_str())) {
            saltati = saltati.saturating_add(1);
        } else {
            let byte = comprimi(&pacchetti::leggi_bozza(data_dir, id)?)?;
            drive.scrivi(
                &nome,
                remoto.map(|voce| voce.id.as_str()),
                "application/gzip",
                &byte,
                impronta,
            )?;
            caricati = caricati.saturating_add(1);
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Bozze,
        );
    }

    Ok(Passata {
        caricati,
        saltati,
        file_id,
        impronta: nostra,
    })
}

/// Com'è finito il file dei metadati.
struct Libreria {
    /// L'identificativo del file che resta.
    file_id: String,
    /// L'impronta di quel che c'è dentro.
    impronta: String,
    /// Quante volte lo si è caricato: zero se era già lassù identico.
    caricati: usize,
}

/// Il file dei metadati: fonde tutte le copie, scrive, rilegge.
///
/// # Il giro
///
/// 1. Le copie del file che l'elenco nomina. Se ce n'è **una** e ha già
///    l'impronta di quel che si ha, non c'è niente da fare: è la scorciatoia
///    che rende gratuita una passata su una libreria ferma.
/// 2. Si leggono tutte e si fondono col contenuto di qui. **Sempre**, anche
///    quando una è stata scritta da questo dispositivo: la fusione è
///    idempotente, e la guardia «l'ha scritto un altro computer» sarebbe falsa
///    proprio dopo una fusione — il file A∪B è firmato da questo dispositivo, e
///    non rifonderlo caricherebbe solo A.
/// 3. Si scrive sulla prima copia leggibile, e le altre — ormai dentro — si
///    cancellano. Una copia illeggibile è stata messa da parte da
///    [`leggi_remoto`] e non si riusa: aggiornarne l'id la rinominerebbe
///    indietro e ne riscriverebbe il contenuto, distruggendo l'unica copia di
///    ciò che è andato storto.
/// 4. Si **rilegge** l'elenco. Se c'è una copia che non è quella appena
///    scritta, o quella scritta ha un'impronta diversa, qualcuno ha scritto in
///    mezzo: si rifà il giro, al più [`GIRI_DI_FUSIONE`] volte.
///
/// Una copia che non compare nella rilettura non fa ricominciare niente: un
/// elenco che arriva un istante indietro non deve far creare un secondo file.
fn salva_la_libreria<M: Magazzino + ?Sized>(
    drive: &M,
    mut elenco: Vec<FileRemoto>,
    locale: &Contenuto,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<Libreria, AppError> {
    let mut contenuto = locale.clone();
    let mut caricati = 0usize;
    let mut ultimo: Option<(String, String)> = None;
    for _ in 0..GIRI_DI_FUSIONE {
        let copie: Vec<FileRemoto> = elenco
            .iter()
            .filter(|file| file.nome == NOME_LIBRERIA)
            .cloned()
            .collect();
        let nostra = backup::impronta(&backup::canonico(&contenuto)?);
        if let [unica] = copie.as_slice()
            && unica.impronta.as_deref() == Some(nostra.as_str())
        {
            return Ok(Libreria {
                file_id: unica.id.clone(),
                impronta: nostra,
                caricati,
            });
        }

        let mut tenuta: Option<&FileRemoto> = None;
        let mut doppioni: Vec<String> = Vec::new();
        for copia in &copie {
            if let Some(loro) = leggi_remoto(drive, copia)? {
                contenuto = backup::fondi(&contenuto, &loro.content);
                if tenuta.is_none() {
                    tenuta = Some(copia);
                } else {
                    doppioni.push(copia.id.clone());
                }
            }
        }
        let nostra = backup::impronta(&backup::canonico(&contenuto)?);
        // Dopo la fusione può non esserci niente da caricare: una copia sola,
        // e lassù c'è già esattamente questo. Succede a chi ha appena
        // ricevuto, dall'altro dispositivo, tutto quel che aveva di nuovo.
        if copie.len() == 1
            && let Some(unica) = tenuta
            && unica.impronta.as_deref() == Some(nostra.as_str())
        {
            return Ok(Libreria {
                file_id: unica.id.clone(),
                impronta: nostra,
                caricati,
            });
        }

        let salvataggio = Salvataggio::nuovo(contenuto.clone(), adesso_ms, dispositivo.to_owned());
        let byte = comprimi(&backup::serializza(&salvataggio)?)?;
        let scritto = drive.scrivi(
            NOME_LIBRERIA,
            tenuta.map(|file| file.id.as_str()),
            "application/gzip",
            &byte,
            &nostra,
        )?;
        caricati = caricati.saturating_add(1);
        // Le copie in più sono dentro quella scritta adesso: via. Un guasto
        // qui non si racconta, la passata dopo le rifonde e riprova.
        for doppione in &doppioni {
            drop(drive.cancella(doppione));
        }
        ultimo = Some((scritto.id.clone(), nostra.clone()));

        // ── la rilettura ────────────────────────────────────────────────────
        elenco = drive.elenca()?;
        let altre = elenco
            .iter()
            .any(|file| file.nome == NOME_LIBRERIA && file.id != scritto.id);
        let cambiata = elenco.iter().any(|file| {
            file.nome == NOME_LIBRERIA
                && file.id == scritto.id
                && file
                    .impronta
                    .as_deref()
                    .is_some_and(|impronta| impronta != nostra)
        });
        if !altre && !cambiata {
            return Ok(Libreria {
                file_id: scritto.id,
                impronta: nostra,
                caricati,
            });
        }
    }
    // Finiti i giri con qualcuno che continua a scrivere: si tiene l'ultima
    // scrittura. Ogni giro ne fa una, quindi c'è.
    let (file_id, impronta) = ultimo.ok_or_else(|| {
        AppError::new(ErrorCode::InternalInvariantViolated {
            what: "un giro di fusione senza scrittura".to_owned(),
        })
    })?;
    Ok(Libreria {
        file_id,
        impronta,
        caricati,
    })
}

/// Scarica il backup, senza applicare niente.
///
/// `None` se lassù non c'è ancora nessun file di metadati: è la condizione di
/// chi ha appena collegato l'account, non un guasto.
///
/// I pacchetti di skin **non** si scaricano qui: pesano fino a venti megabyte
/// l'uno, e il piano ha bisogno solo di sapere quali esistono e quanto pesano —
/// cose che stanno già nell'elenco. Si scaricano al momento di applicare.
///
/// # Errori
///
/// `net.*`; `sync.remoteCorrupt` se il file lassù non si legge, dopo averlo
/// messo da parte con un nome che dice cos'era.
pub fn scarica_salvataggio<M: Magazzino + ?Sized>(
    drive: &M,
) -> Result<Option<DaRipristinare>, AppError> {
    let remoti = per_nome(drive.elenca()?);
    let Some(file) = remoti.get(NOME_LIBRERIA) else {
        return Ok(None);
    };
    let Some(salvataggio) = leggi_remoto(drive, file)? else {
        // Era corrotto ed è stato messo da parte: per chi ripristina è come se
        // non ci fosse, ed è la verità.
        return Ok(None);
    };

    let mut skin = BTreeMap::new();
    let mut bozze = BTreeMap::new();
    for (nome, voce) in &remoti {
        if let Some(id) = id_da_nome_skin(nome) {
            skin.insert(id.to_owned(), voce.clone());
        } else if let Some(id) = id_da_nome_bozza(nome) {
            bozze.insert(id.to_owned(), voce.clone());
        }
    }

    Ok(Some(DaRipristinare {
        contenuto: salvataggio.content,
        generato_ms: salvataggio.generated_at,
        generato_da: salvataggio.generated_by,
        skin,
        bozze,
    }))
}

/// Scarica e scrive le skin e le bozze che il piano propone.
///
/// **Prima della transazione sul database**, e non dopo: se si scrivesse prima
/// la transazione e poi un download fallisse, resterebbe committato uno
/// `skin.active` che punta a una skin che non esiste, e Aether si riaprirebbe
/// senza sapersi disegnare.
///
/// Un pacchetto che non si scarica o non si valida non ferma gli altri: finisce
/// in [`Scritti::mancanti`], con il perché in [`Scritti::guasti`]. Un
/// ripristino parziale è meglio di nessun ripristino, purché si dica quale
/// parte manca.
///
/// # Errori
///
/// Uno solo: il token scaduto a metà (`net.http` 401, vedi
/// [`drive::e_scaduto`]). Quello non riguarda un pacchetto ma la connessione, e
/// chi chiama rinfresca il token e rifà tutto — i pacchetti già scritti, al
/// secondo giro, sono «c'era già».
pub fn scarica_pacchetti<M: Magazzino + ?Sized>(
    drive: &M,
    data_dir: &Path,
    da: &DaRipristinare,
    piano: &RestorePlan,
    mut avanzamento: impl FnMut(Avanzamento),
) -> Result<Scritti, AppError> {
    let mut scritti = Scritti::default();

    let totale = piano.skins_to_install.len();
    for (fatti, id) in piano.skins_to_install.iter().enumerate() {
        match da.skin.get(id) {
            None => scritti.mancanti.push(nome_skin(id)),
            Some(remoto) => {
                let esito = pacchetto(drive, &remoto.id, false)?
                    .and_then(|byte| pacchetti::scrivi_skin(data_dir, id, &byte));
                match esito {
                    Ok(true) => scritti.skin = scritti.skin.saturating_add(1),
                    // `false` è «c'era già»: non è un lavoro svolto né un
                    // guasto. `Err` è un pacchetto che non è sceso o non si
                    // valida, e vale come mancante: la skin non c'è, e dirlo è
                    // più utile che far fallire tutto il ripristino per un tema.
                    Ok(false) => {}
                    Err(err) => scritti.manca(nome_skin(id), &err),
                }
            }
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Skin,
        );
    }

    let totale = piano.drafts_to_write.len();
    for (fatti, id) in piano.drafts_to_write.iter().enumerate() {
        match da.bozze.get(id) {
            None => scritti.mancanti.push(nome_bozza(id)),
            Some(remoto) => {
                let esito = pacchetto(drive, &remoto.id, true)?
                    .and_then(|byte| pacchetti::scrivi_bozza(data_dir, id, &byte));
                match esito {
                    Ok(true) => scritti.bozze = scritti.bozze.saturating_add(1),
                    Ok(false) => {}
                    Err(err) => scritti.manca(nome_bozza(id), &err),
                }
            }
        }
        segnala(
            &mut avanzamento,
            fatti.saturating_add(1),
            totale,
            Cosa::Bozze,
        );
    }

    Ok(scritti)
}

impl Scritti {
    /// Un file che c'era e non è arrivato, con il suo perché.
    fn manca(&mut self, nome: String, err: &AppError) {
        self.guasti.push(format!(
            "{nome} — {}: {}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        ));
        self.mancanti.push(nome);
    }
}

/// I byte di un pacchetto, o il guasto che riguarda **quel** pacchetto.
///
/// Il `Result` esterno è la connessione — solo il token scaduto, che fa
/// rinfrescare e rifare tutto; quello interno è il file, e finisce fra i
/// mancanti. `compresso` per i manifest delle bozze, che viaggiano in gzip.
fn pacchetto<M: Magazzino + ?Sized>(
    drive: &M,
    id: &str,
    compresso: bool,
) -> Result<Result<Vec<u8>, AppError>, AppError> {
    match drive.leggi(id) {
        Err(err) if drive::e_scaduto(&err) => Err(err),
        Err(err) => Ok(Err(err)),
        Ok(byte) if compresso => Ok(decomprimi(&byte)),
        Ok(byte) => Ok(Ok(byte)),
    }
}

/// I file remoti, per nome.
///
/// Drive **permette** due file con lo stesso nome nella stessa cartella. Per il
/// file dei metadati il caso è trattato davvero — le copie si fondono, vedi
/// [`salva_la_libreria`]; per i pacchetti vince l'ultimo che compare
/// nell'elenco, e la passata successiva sovrascriverà quello: un pacchetto è
/// identificato dalla sua impronta, e due copie dello stesso tema non hanno
/// niente da fondere.
fn per_nome(file: Vec<FileRemoto>) -> BTreeMap<String, FileRemoto> {
    file.into_iter()
        .map(|voce| (voce.nome.clone(), voce))
        .collect()
}

/// Scarica e interpreta il file dei metadati.
///
/// `None` quando il file c'era ma era illeggibile: in quel caso lo si
/// **rinomina** invece di sovrascriverlo, e chi chiama procede come se non ci
/// fosse — cioè crea un file **nuovo**, senza riusare l'identificativo: un
/// `PATCH` su quello riporterebbe il file messo da parte al nome di prima e ne
/// riscriverebbe il contenuto, distruggendo l'unica copia di ciò che è andato
/// storto.
///
/// # Perché rinominare e non cancellare
///
/// Perché nella cartella privata dell'applicazione il cestino non esiste
/// (`notSupportedForAppDataFolderFiles`), quindi le uniche due possibilità sono
/// rinominare e cancellare davvero. E perché un backup illeggibile è l'unica
/// copia rimasta di qualcosa: se un giorno si scoprisse *perché* si era rotto,
/// averlo buttato via sarebbe irrimediabile. Costa settanta chilobyte tenerlo.
fn leggi_remoto<M: Magazzino + ?Sized>(
    drive: &M,
    file: &FileRemoto,
) -> Result<Option<Salvataggio>, AppError> {
    let byte = drive.leggi(&file.id)?;
    let esito = decomprimi(&byte).and_then(|aperto| backup::interpreta(&aperto));
    match esito {
        Ok(salvataggio) => Ok(Some(salvataggio)),
        Err(err) if e_corrotto(&err) => {
            // Il rinomino è best-effort: se anche fallisse, la cosa importante è
            // non aver sovrascritto in silenzio.
            let messo_da_parte = format!("{NOME_LIBRERIA}.corrotto-{}", adesso_grezzo());
            drop(drive.rinomina(&file.id, &messo_da_parte));
            Ok(None)
        }
        // Una versione più nuova non è una corruzione, e **non** si mette da
        // parte: sovrascrivere il backup di un Aether più recente sarebbe
        // l'unico modo in cui questa funzione può perdere dati per davvero.
        Err(err) => Err(err),
    }
}

/// L'errore dice che il file remoto è illeggibile?
fn e_corrotto(err: &AppError) -> bool {
    matches!(err.code(), ErrorCode::SyncRemoteCorrupt)
}

/// Un numero crescente per distinguere i file messi da parte.
fn adesso_grezzo() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |quanto| quanto.as_millis())
}

/// Riferisce l'avanzamento, con la stessa strozzatura del resto dell'app.
///
/// Ogni venticinque elementi, e **sempre l'ultimo**. Senza la strozzatura, una
/// libreria con quaranta skin manderebbe quaranta eventi alla finestra; senza
/// «sempre l'ultimo», una barra di avanzamento si fermerebbe al 96%.
fn segnala(avanzamento: &mut impl FnMut(Avanzamento), fatti: usize, totale: usize, cosa: Cosa) {
    if fatti.is_multiple_of(25) || fatti == totale {
        avanzamento(Avanzamento {
            fatti,
            totale,
            cosa,
        });
    }
}

/// Comprime con gzip.
///
/// # Errori
///
/// `internal.unexpected`: comprimere in memoria non ha un modo di fallire che
/// non sia un guasto nostro.
pub(crate) fn comprimi(dati: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut compressore = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    compressore
        .write_all(dati)
        .and_then(|()| compressore.finish())
        .map_err(|err| {
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("compressione del backup".to_owned()),
            })
            .with_cause(err.to_string())
        })
}

/// Apre un gzip, con un tetto a quanto può diventare.
///
/// # Errori
///
/// `sync.remoteCorrupt` se non è un gzip valido o se sfora il tetto. Sono due
/// cose diverse che si dicono nello stesso modo apposta: per chi riceve il file
/// sono la stessa situazione, cioè «lassù c'è qualcosa che non si può usare».
pub fn decomprimi(dati: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut fuori = Vec::new();
    let mut lettore = flate2::read::GzDecoder::new(dati).take(LIMITE_DECOMPRESSO + 1);
    lettore.read_to_end(&mut fuori).map_err(|err| {
        AppError::new(ErrorCode::SyncRemoteCorrupt).with_cause(format!("gzip illeggibile: {err}"))
    })?;
    if fuori.len() as u64 > LIMITE_DECOMPRESSO {
        return Err(AppError::new(ErrorCode::SyncRemoteCorrupt)
            .with_cause("il file compresso si apre oltre il limite"));
    }
    Ok(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_nomi_fanno_andata_e_ritorno() {
        assert_eq!(nome_skin("notte"), "aether-skin-notte.aeskin");
        assert_eq!(id_da_nome_skin(&nome_skin("notte")), Some("notte"));
        assert_eq!(nome_bozza("in-corso"), "aether-bozza-in-corso.json.gz");
        assert_eq!(id_da_nome_bozza(&nome_bozza("in-corso")), Some("in-corso"));
    }

    #[test]
    fn i_nomi_non_si_confondono_fra_loro() {
        // Il file dei metadati non deve essere scambiato per una bozza: hanno
        // entrambi il suffisso `.json.gz`, e una classificazione sbagliata lo
        // farebbe scrivere come manifest dentro `skin\bozze\`.
        assert_eq!(id_da_nome_bozza(NOME_LIBRERIA), None);
        assert_eq!(id_da_nome_skin(NOME_LIBRERIA), None);
        assert_eq!(id_da_nome_skin(&nome_bozza("x")), None);
        assert_eq!(id_da_nome_bozza(&nome_skin("x")), None);
    }

    #[test]
    fn la_versione_sta_nel_nome() {
        // Così una versione precedente continua a trovare il suo file invece di
        // litigare con uno che non capisce.
        assert!(NOME_LIBRERIA.contains(".v1."));
    }

    #[test]
    fn il_gzip_fa_andata_e_ritorno() {
        let dati = b"{\"tracks\":[]}".repeat(500);
        let stretto = comprimi(&dati).expect("compresso");
        assert!(stretto.len() < dati.len(), "deve pur comprimere");
        assert_eq!(decomprimi(&stretto), Ok(dati));
    }

    #[test]
    fn quel_che_non_e_gzip_e_remoto_corrotto() {
        // E non un `internal`: per chi riceve il file la situazione è «lassù c'è
        // qualcosa che non si può usare», ed è il codice che autorizza a metterlo
        // da parte e ricominciare.
        let err = decomprimi(b"non e gzip").expect_err("illeggibile");
        assert_eq!(err.code().kind().code(), "sync.remoteCorrupt");
    }

    #[test]
    fn una_bomba_di_decompressione_non_riempie_la_memoria() {
        // Un gzip di zeri: cento megabyte si comprimono in pochi chilobyte, e
        // senza il tetto verrebbero aperti tutti.
        let zeri = vec![0u8; usize::try_from(LIMITE_DECOMPRESSO + 1024).unwrap_or(usize::MAX)];
        let bomba = comprimi(&zeri).expect("compressa");
        assert!(bomba.len() < 200_000, "deve essere piccola per contare");
        let err = decomprimi(&bomba).expect_err("oltre il limite");
        assert_eq!(err.code().kind().code(), "sync.remoteCorrupt");
    }

    #[test]
    fn l_avanzamento_si_strozza_ma_non_perde_l_ultimo() {
        // Senza la strozzatura, quaranta skin sarebbero quaranta eventi; senza
        // «sempre l'ultimo», la barra si fermerebbe prima della fine.
        let mut visti = Vec::new();
        let mut riferisci = |passo: Avanzamento| visti.push(passo.fatti);
        for fatti in 1..=53usize {
            segnala(&mut riferisci, fatti, 53, Cosa::Skin);
        }
        assert_eq!(visti, [25, 50, 53]);
    }

    #[test]
    fn due_file_con_lo_stesso_nome_ne_lasciano_uno() {
        let doppio = vec![
            FileRemoto {
                id: "vecchio".to_owned(),
                nome: NOME_LIBRERIA.to_owned(),
                byte: 10,
                impronta: None,
            },
            FileRemoto {
                id: "nuovo".to_owned(),
                nome: NOME_LIBRERIA.to_owned(),
                byte: 20,
                impronta: Some("abc".to_owned()),
            },
        ];
        let per_nome = per_nome(doppio);
        assert_eq!(per_nome.len(), 1);
        assert_eq!(
            per_nome.get(NOME_LIBRERIA).map(|voce| voce.id.as_str()),
            Some("nuovo"),
            "vince l'ultimo, e la passata dopo sovrascrive quello"
        );
    }

    #[test]
    fn i_nomi_delle_cose_attraversano_l_ipc_come_stringhe_stabili() {
        // La finestra ci fa uno `switch` sopra: cambiarli è un cambio di
        // protocollo, non un ritocco di stile.
        assert_eq!(Cosa::Metadati.nome(), "metadati");
        assert_eq!(Cosa::Skin.nome(), "skin");
        assert_eq!(Cosa::Bozze.nome(), "bozze");
    }

    /// Un Drive in memoria, che come quello vero ammette due file con lo
    /// stesso nome.
    ///
    /// `intrusione` è l'altro dispositivo: alla prima scrittura del file dei
    /// metadati ne crea un'altra copia, come farebbe un computer che salva per
    /// la prima volta nello stesso secondo.
    #[derive(Default)]
    struct Finto {
        file: std::sync::Mutex<Vec<(FileRemoto, Vec<u8>)>>,
        prossimo: std::sync::atomic::AtomicUsize,
        intrusione: std::sync::Mutex<Option<Vec<u8>>>,
    }

    impl Finto {
        fn con(file: Vec<(FileRemoto, Vec<u8>)>) -> Self {
            Self {
                file: std::sync::Mutex::new(file),
                ..Self::default()
            }
        }

        fn copie_della_libreria(&self) -> Vec<(FileRemoto, Vec<u8>)> {
            self.file
                .lock()
                .expect("file")
                .iter()
                .filter(|(file, _)| file.nome == NOME_LIBRERIA)
                .cloned()
                .collect()
        }
    }

    impl Magazzino for Finto {
        fn elenca(&self) -> Result<Vec<FileRemoto>, AppError> {
            Ok(self
                .file
                .lock()
                .expect("file")
                .iter()
                .map(|(file, _)| file.clone())
                .collect())
        }

        fn leggi(&self, id: &str) -> Result<Vec<u8>, AppError> {
            match id {
                "rotto" => return Err(AppError::new(ErrorCode::NetOffline { url: None })),
                "scaduto" => {
                    return Err(AppError::new(ErrorCode::NetHttp {
                        status: 401,
                        url: None,
                    }));
                }
                _ => {}
            }
            self.file
                .lock()
                .expect("file")
                .iter()
                .find(|(file, _)| file.id == id)
                .map(|(_, byte)| byte.clone())
                .ok_or_else(|| {
                    AppError::new(ErrorCode::FsNotFound {
                        path: id.to_owned(),
                    })
                })
        }

        fn scrivi(
            &self,
            nome: &str,
            esistente: Option<&str>,
            _tipo: &str,
            dati: &[u8],
            impronta: &str,
        ) -> Result<FileRemoto, AppError> {
            let mut file = self.file.lock().expect("file");
            let scritto = if let Some(voce) =
                esistente.and_then(|id| file.iter_mut().find(|(f, _)| f.id == id))
            {
                voce.0.impronta = Some(impronta.to_owned());
                voce.1 = dati.to_vec();
                voce.0.clone()
            } else {
                let n = self
                    .prossimo
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let nuovo = FileRemoto {
                    id: format!("f{n}"),
                    nome: nome.to_owned(),
                    byte: u64::try_from(dati.len()).unwrap_or(0),
                    impronta: Some(impronta.to_owned()),
                };
                file.push((nuovo.clone(), dati.to_vec()));
                nuovo
            };
            if nome == NOME_LIBRERIA
                && let Some(loro) = self.intrusione.lock().expect("intrusione").take()
            {
                file.push((
                    FileRemoto {
                        id: "intruso".to_owned(),
                        nome: NOME_LIBRERIA.to_owned(),
                        byte: u64::try_from(loro.len()).unwrap_or(0),
                        impronta: Some("loro".to_owned()),
                    },
                    loro,
                ));
            }
            Ok(scritto)
        }

        fn rinomina(&self, id: &str, nome: &str) -> Result<(), AppError> {
            if let Some(voce) = self
                .file
                .lock()
                .expect("file")
                .iter_mut()
                .find(|(f, _)| f.id == id)
            {
                voce.0.nome = nome.to_owned();
            }
            Ok(())
        }

        fn cancella(&self, id: &str) -> Result<(), AppError> {
            self.file.lock().expect("file").retain(|(f, _)| f.id != id);
            Ok(())
        }
    }

    /// Il file dei metadati di un altro dispositivo, che sorveglia `cartella`.
    fn di_un_altro(cartella: &str) -> Vec<u8> {
        let contenuto = Contenuto {
            roots: vec![cartella.to_owned()],
            ..Contenuto::default()
        };
        let salvataggio = Salvataggio::nuovo(contenuto, 1, "altro".to_owned());
        comprimi(&backup::serializza(&salvataggio).expect("serializzato")).expect("compresso")
    }

    fn copia(id: &str, impronta: &str) -> FileRemoto {
        FileRemoto {
            id: id.to_owned(),
            nome: NOME_LIBRERIA.to_owned(),
            byte: 0,
            impronta: Some(impronta.to_owned()),
        }
    }

    /// Le cartelle dentro il file dei metadati che è rimasto.
    fn cartelle_dentro(byte: &[u8]) -> Vec<String> {
        backup::interpreta(&decomprimi(byte).expect("gzip"))
            .expect("un salvataggio")
            .content
            .roots
    }

    fn qui() -> StatoLocale {
        StatoLocale {
            roots: vec![r"C:\Qui".to_owned()],
            ..StatoLocale::default()
        }
    }

    /// Due dispositivi che hanno salvato per la prima volta insieme lasciano
    /// due copie: si fondono tutte, e ne resta una.
    #[test]
    fn le_copie_doppie_si_fondono_e_ne_resta_una() {
        let dati = tempfile::tempdir().expect("cartella");
        let finto = Finto::con(vec![
            (copia("a", "x"), di_un_altro(r"D:\A")),
            (copia("b", "y"), di_un_altro(r"E:\B")),
        ]);
        let passata =
            salva(&finto, dati.path(), &qui(), "questo", 2, |_| {}).expect("la passata riesce");
        let copie = finto.copie_della_libreria();
        assert_eq!(
            copie.len(),
            1,
            "le copie in più si cancellano dopo la fusione"
        );
        let (rimasta, byte) = copie.first().expect("una copia");
        assert_eq!(rimasta.id, "a", "si scrive sulla prima copia leggibile");
        assert_eq!(passata.file_id, "a");
        let mut cartelle = cartelle_dentro(byte);
        cartelle.sort();
        assert_eq!(
            cartelle,
            [r"C:\Qui", r"D:\A", r"E:\B"],
            "niente dei due dispositivi si è perso"
        );
    }

    /// Chi scrive mentre questo dispositivo scrive non si perde: la rilettura
    /// dopo la scrittura lo trova, e si rifà il giro.
    #[test]
    fn chi_scrive_in_mezzo_si_ritrova_nella_rilettura() {
        let dati = tempfile::tempdir().expect("cartella");
        let finto = Finto::default();
        *finto.intrusione.lock().expect("intrusione") = Some(di_un_altro(r"D:\Altrove"));
        let passata =
            salva(&finto, dati.path(), &qui(), "questo", 2, |_| {}).expect("la passata riesce");
        let copie = finto.copie_della_libreria();
        assert_eq!(copie.len(), 1, "la copia dell'altro è stata fusa e tolta");
        let (rimasta, byte) = copie.first().expect("una copia");
        assert_eq!(rimasta.id, passata.file_id);
        assert_eq!(rimasta.impronta.as_deref(), Some(passata.impronta.as_str()));
        let mut cartelle = cartelle_dentro(byte);
        cartelle.sort();
        assert_eq!(cartelle, [r"C:\Qui", r"D:\Altrove"]);
        assert_eq!(
            passata.caricati, 2,
            "una scrittura, una rilettura, una rifusione"
        );
    }

    /// Una libreria ferma resta gratuita: una copia sola e già uguale, nessuna
    /// scrittura.
    #[test]
    fn una_libreria_ferma_non_si_ricarica() {
        let dati = tempfile::tempdir().expect("cartella");
        let finto = Finto::default();
        salva(&finto, dati.path(), &qui(), "questo", 2, |_| {}).expect("prima");
        let seconda = salva(&finto, dati.path(), &qui(), "questo", 3, |_| {}).expect("seconda");
        assert_eq!(seconda.caricati, 0);
        assert_eq!(seconda.saltati, 1);
        assert_eq!(finto.copie_della_libreria().len(), 1);
    }

    /// Un pacchetto con l'identificatore dato.
    fn pacchetto_di(id: &str) -> Vec<u8> {
        let sorgente =
            aether_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""));
        aether_skin::write_skin_package(&aether_skin::package::WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[],
        })
        .expect("pacchetto")
    }

    fn da_ripristinare(skin: &[(&str, &str)]) -> DaRipristinare {
        DaRipristinare {
            contenuto: Contenuto::default(),
            generato_ms: 0,
            generato_da: String::new(),
            skin: skin
                .iter()
                .map(|(id, file)| {
                    (
                        (*id).to_owned(),
                        FileRemoto {
                            id: (*file).to_owned(),
                            nome: nome_skin(id),
                            byte: 0,
                            impronta: None,
                        },
                    )
                })
                .collect(),
            bozze: BTreeMap::new(),
        }
    }

    /// Il difetto che il collaudo della 2.4.0 non aveva visto: il primo scarico
    /// fallito fermava l'intero ripristino. Adesso quel pacchetto è un mancante
    /// con il suo perché, e gli altri arrivano.
    #[test]
    fn un_pacchetto_che_non_scende_va_fra_i_mancanti() {
        let dati = tempfile::tempdir().expect("cartella");
        let mut buona = copia("buona", "z");
        buona.nome = nome_skin("giorno");
        let finto = Finto::con(vec![(buona, pacchetto_di("giorno"))]);
        let da = da_ripristinare(&[("giorno", "buona"), ("notte", "rotto")]);
        let piano = RestorePlan {
            skins_to_install: vec!["giorno".to_owned(), "notte".to_owned()],
            ..RestorePlan::default()
        };
        let scritti = scarica_pacchetti(&finto, dati.path(), &da, &piano, |_| {})
            .expect("il ripristino non si ferma");
        assert_eq!(scritti.skin, 1, "il pacchetto che c'era è arrivato");
        assert_eq!(scritti.mancanti, vec![nome_skin("notte")]);
        assert!(
            scritti
                .guasti
                .first()
                .is_some_and(|riga| riga.contains("net.offline")),
            "il perché finisce nel diario: {:?}",
            scritti.guasti
        );
        assert!(dati.path().join("skin").join("giorno.aeskin").exists());
    }

    /// Il token scaduto a metà non è un pacchetto mancante: torna su, perché
    /// chi chiama rinfreschi e rifaccia tutto.
    #[test]
    fn un_token_scaduto_a_meta_ripristino_non_diventa_un_mancante() {
        let dati = tempfile::tempdir().expect("cartella");
        let finto = Finto::default();
        let da = da_ripristinare(&[("notte", "scaduto")]);
        let piano = RestorePlan {
            skins_to_install: vec!["notte".to_owned()],
            ..RestorePlan::default()
        };
        let err = scarica_pacchetti(&finto, dati.path(), &da, &piano, |_| {})
            .expect_err("il 401 torna a chi chiama");
        assert!(drive::e_scaduto(&err));
    }
}
