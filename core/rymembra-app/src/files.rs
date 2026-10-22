//! L'accesso ai file, dietro un tratto.
//!
//! Sul desktop un brano è un percorso e `std::fs` lo apre. Su Android un brano è
//! un URI di MediaStore o una concessione dello Storage Access Framework, e per
//! aprirlo serve passare da Java. Le due cose non si assomigliano abbastanza da
//! poter essere la stessa funzione, e non si assomigliano abbastanza poco da
//! giustificare due scansioni diverse.
//!
//! Quindi: la scansione parla con questo tratto, e chi la esegue gli passa
//! l'implementazione della sua piattaforma. La **decisione** su cosa inserire e
//! cosa togliere resta una funzione pura in `rymembra-domain`, uguale ovunque; qui
//! cambia solo il modo di ottenere l'elenco e i byte.
//!
//! È anche ciò che rende la scansione provabile senza toccare il disco: un
//! doppio in memoria implementa il tratto e i test girano su alberi di file
//! finti, compresi quelli che un filesystem vero non lascerebbe costruire.

use std::io::{Read, Seek};
use std::ops::ControlFlow;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError, mpsc};

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::scan_plan::DiscoveredFile;

/// Ciò che serve a un lettore di metadati: leggere e riposizionarsi.
///
/// I formati audio hanno i tag in testa (ID3v2) o in coda (ID3v1, APE), quindi
/// non basta uno stream in avanti.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// L'esito di una camminata: quel che si è trovato, e se si è trovato tutto.
///
/// # Perché «completa» viaggia insieme ai file
///
/// Perché senza, un elenco parziale e un elenco intero si assomigliano come una
/// radice morta e una radice vuota — ed è lo stesso errore, un passo più in là.
/// La sonda di [`MusicFiles::radice_raggiungibile`] guarda la cartella *prima*
/// di camminarci dentro; se la rete cade **durante** la camminata di una
/// libreria grande, quella sonda ha già detto sì, un'enumerazione ingenua
/// salta in silenzio i rami che non rispondono più, e quel che torna è una
/// fotografia di mezza libreria che il piano legge come «l'altra metà è
/// sparita».
///
/// Non è un caso di scuola: è la stessa cancellazione che la sonda esiste per
/// impedire, con la rete caduta un momento più tardi.
/// Niente `Default`, e apposta: il suo `completa` sarebbe `false`, cioè
/// «parziale», che è il valore su cui a valle si decide di non cancellare
/// niente. Un campo del genere va scritto da chi sa com'è andata la camminata,
/// non ereditato per distrazione da chi non ci ha pensato.
///
/// # Il terzo caso, che qui dentro non c'è
///
/// Una camminata **arenata** — la share che non fallisce e non finisce, che
/// resta lì — non è né completa né parziale: è una camminata che non è tornata.
/// Questo tipo non sa esprimerla, perché per esistere bisogna che la funzione
/// che lo costruisce sia rientrata.
///
/// Il terzo caso vive quindi un piano più in su, in [`crate::library::cammina`],
/// che consuma [`MusicFiles::walk_a_rate`] a rate e conta il silenzio fra due
/// consegne. Lì una radice arenata si salta **senza risondare**: la sonda
/// risponderebbe «sì» — dalla cache del sistema, o perché la cartella si apre e
/// solo il contenuto non arriva — e quel «sì» farebbe passare l'elenco monco
/// per un elenco vero.
#[derive(Debug, Clone)]
pub struct Camminata {
    /// I file trovati.
    pub file: Vec<DiscoveredFile>,
    /// Nessun ramo è andato perso.
    ///
    /// `false` dice soltanto «questo elenco è parziale», non «la rete è giù»:
    /// una sottocartella a permessi negati su un disco locale lo abbassa
    /// esattamente come una share che muore. A distinguere i due casi è chi
    /// chiama, risondando la radice — vedi [`crate::library::cammina`].
    ///
    /// È lo stesso `bool` che [`MusicFiles::walk_a_rate`] restituisce alla fine
    /// della sua consegna, e i due non devono mai divergere: chi implementa
    /// l'uno implementa l'altro.
    ///
    /// **Dove** si è perso il ramo qui non c'è: lo dice soltanto la camminata a
    /// rate, con [`Voce::RamoPerso`], che è quella da cui passa la scansione.
    /// Chi cammina intero — il primo avvio, gli esempi — vuole sapere quali file
    /// ci sono, non decidere cosa cancellare.
    pub completa: bool,
}

/// Quel che una camminata a rate consegna, un pezzo per volta.
///
/// # Perché i rami persi viaggiano insieme ai file
///
/// Perché [`Camminata::completa`] da solo dice **che** si è perso qualcosa, e
/// non **dove**, e fra le due domande c'è di mezzo la libreria. Con la radice
/// che alla sonda risponde — il caso normale di una sottocartella a permessi
/// negati sul disco di casa, o di una share che singhiozza su un ramo solo — la
/// scansione prima proseguiva come se avesse visto tutto: ogni brano sotto la
/// cartella illeggibile risultava «sparito», e una scansione a mano lo toglieva,
/// portandosi via in cascata cronologia, playlist, voti e preferiti.
///
/// Con il ramo detto per nome la scansione può fare la cosa giusta e stretta:
/// quel che sta **sotto quel ramo** non è stato verificato — né tolto né
/// riscritto — e tutto il resto della radice si giudica come sempre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Voce {
    /// Un file trovato.
    File(DiscoveredFile),
    /// Una cartella dentro cui non si è potuto guardare, per intero.
    ///
    /// Il percorso è quello della cartella, nella stessa forma dei percorsi dei
    /// file — la radice più i nomi, come la camminata li costruisce — così che
    /// «sta sotto questo ramo» si chieda con la stessa `is_under` con cui il
    /// piano chiede «sta sotto questa radice». Può essere la radice stessa,
    /// quando non si sa dire di meglio.
    RamoPerso(String),
    /// Battito: una cartella è stata elencata, anche se dentro non c'era niente.
    ///
    /// Non porta un file e non decide niente, ed esiste per due ragioni. La
    /// prima è il silenzio che misura chi consuma: una libreria con mille
    /// cartelle di copertine e nessun brano è un disco che risponde, e senza
    /// battito sembrerebbe una share morta. La seconda è l'avanzamento: «N
    /// cartelle, M file» si conta da qui.
    ///
    /// Chi non ha cartelle da dichiarare — il corpo di serie di
    /// [`MusicFiles::walk_a_rate`], un doppio di prova — non è obbligato a
    /// mandarlo.
    Cartella,
}

/// Da dove arrivano i file musicali.
pub trait MusicFiles: Send + Sync {
    /// Elenca ricorsivamente i file sotto una radice.
    ///
    /// **Non filtra**: restituisce quel che trova, comprese le estensioni non
    /// audio e i file troppo piccoli. Il filtro è una decisione, e le decisioni
    /// stanno nel dominio, dove si provano. Una camminata che filtrasse da sé
    /// renderebbe invisibile al piano il motivo per cui un file è stato
    /// scartato — che è l'informazione che serve quando un brano «non compare».
    ///
    /// Una radice illeggibile non è un errore fatale: si riporta vuota, perché
    /// un disco esterno staccato non deve impedire la scansione degli altri.
    ///
    /// **Ma «vuota» non vuol dire «da svuotare»**: chi chiama deve prima chiedere
    /// a [`Self::radice_raggiungibile`] se quella cartella esiste ancora, perché
    /// una camminata vuota su una radice morta e una su una radice davvero
    /// svuotata si assomigliano come due gocce d'acqua, e trattarle uguali
    /// significa cancellare l'intera libreria di chi ha staccato il NAS.
    ///
    /// **E «parziale» non vuol dire «completa»**: chi implementa deve abbassare
    /// [`Camminata::completa`] appena perde un ramo, per lo stesso motivo un
    /// passo più in là. Vedi [`Camminata`].
    ///
    /// **Torna quando ha finito**, e su una share che non risponde «quando ha
    /// finito» può voler dire mai: chi non se lo può permettere — la scansione,
    /// che gira mentre la finestra deve restare viva — passa da
    /// [`Self::walk_a_rate`], che consegna man mano e a cui si può smettere di
    /// dare ascolto.
    fn walk(&self, root: &str) -> Result<Camminata, AppError>;

    /// Come [`Self::walk`], ma consegnando ogni file appena lo si trova.
    ///
    /// # Perché a rate
    ///
    /// Perché la domanda che serve alla scansione non è «quanto è durata
    /// l'enumerazione», che su una libreria da centomila file dura
    /// legittimamente dei minuti, ma «da quanto non arriva più niente». Le due
    /// si somigliano solo finché il disco risponde: una share viva e lenta
    /// consegna piano ma consegna, una share morta smette e basta. Con un
    /// [`Self::walk`] intero le due sono la stessa attesa, e limitarla vorrebbe
    /// dire scegliere fra dichiarare morta una libreria grande e non accorgersi
    /// mai di una share caduta.
    ///
    /// `su_voce` viene chiamato una volta per file e una volta per ogni ramo
    /// che si perde; chi sa farlo manda anche un [`Voce::Cartella`] per ogni
    /// cartella elencata. Lo chiama sempre e solo il filo di chi ha chiamato,
    /// anche quando sotto la camminata gira su più fili — è per questo che può
    /// essere un `FnMut` qualunque. L'ordine è quello in cui le voci arrivano e
    /// **non è un contratto**: chi ha bisogno di un ordine se lo fa.
    /// Restituendo [`ControlFlow::Break`] dice «basta così» e la camminata si
    /// ferma lì: dopo un `Break`, `su_voce` non viene chiamato più.
    ///
    /// # Il `bool`
    ///
    /// È [`Camminata::completa`], con lo stesso significato e lo stesso peso a
    /// valle: `true` vuol dire «nessun ramo è andato perso», e solo su un `true`
    /// chi chiama si può permettere di concludere che quel che non è arrivato
    /// non c'è. Una camminata **interrotta** da `su_voce` restituisce quindi
    /// `false`: chi ha detto basta non ha visto tutto, per definizione.
    ///
    /// # Il patto dei rami persi
    ///
    /// Chi abbassa `completa` perché un ramo non si è letto **deve** averlo
    /// consegnato come [`Voce::RamoPerso`]. Chi non sa dire dove l'ha perso
    /// consegna la radice intera: è la risposta larga, e l'unica onesta — a
    /// valle vuol dire «di questa radice non si toglie niente», che è quel che
    /// un elenco monco senza indirizzo merita. Una camminata che si dice monca
    /// senza aver consegnato rami viene trattata allo stesso modo da chi la
    /// consuma, così che il patto rotto costi prudenza e non brani.
    ///
    /// Il corpo di serie chiama [`Self::walk`] e consegna quel che ha
    /// restituito: è corretto per ogni implementazione — i doppi di prova, un
    /// provider Android — e sbagliato per due cose. La prima è il tempo: chi
    /// cammina su un filesystem che può non rispondere lo sovrascrive
    /// consegnando davvero man mano, altrimenti il silenzio che chi consuma sta
    /// misurando è un silenzio che comincia soltanto alla fine. La seconda è la
    /// grana dei rami persi: [`Camminata`] non sa dove si è perso qualcosa, e il
    /// corpo di serie consegna la radice intera.
    fn walk_a_rate(
        &self,
        root: &str,
        su_voce: &mut dyn FnMut(Voce) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        let camminata = self.walk(root)?;
        for file in camminata.file {
            if su_voce(Voce::File(file)).is_break() {
                return Ok(false);
            }
        }
        if !camminata.completa && su_voce(Voce::RamoPerso(root.to_owned())).is_break() {
            return Ok(false);
        }
        Ok(camminata.completa)
    }

    /// La radice esiste ed è leggibile **adesso**?
    ///
    /// Distingue «radice vuota» da «NAS spento»: trattarle uguali significava
    /// marcare «sparito» ogni brano della seconda alla prima scansione a rete
    /// giù, e con lui portarsi via voti, preferiti e cronologia — le sole cose
    /// in tutta la libreria che una riscansione non sa ricostruire.
    ///
    /// Il valore di serie è `true`, cioè «non lo so, fai come prima»: le
    /// implementazioni che non hanno un modo di chiederlo — un doppio in
    /// memoria, un provider Android — non devono per questo bloccare la
    /// scansione.
    fn radice_raggiungibile(&self, root: &str) -> bool {
        let _ = root;
        true
    }

    /// Apre un file in lettura.
    ///
    /// `Sync` oltre a `Send` perché lo stesso flusso serve al motore audio, e
    /// symphonia lo pretende: il suo `MediaSource` è `Send + Sync`. Non è un
    /// requisito gratuito — significa che un'implementazione non può nascondere
    /// uno stato mutabile senza lucchetto dietro un `&self` — ma è quello che
    /// permette a lettura dei tag e riproduzione di passare dallo stesso tratto
    /// invece che da due.
    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError>;
}

/// Traduce un errore di I/O nel catalogo, distinguendo i casi che cambiano cosa
/// deve fare il chiamante.
///
/// La distinzione non è cosmetica: `NotFound` durante una scansione significa
/// «il file è appena sparito, salta e prosegui», mentre `PermissionDenied`
/// significa «tutta questa cartella è inaccessibile, dirlo all'utente». Il
/// vecchio albero li appiattiva entrambi in un avviso nel log.
pub fn io_error(path: &str, err: &std::io::Error) -> AppError {
    use std::io::ErrorKind as K;
    // Prima di tutto il resto: i guasti di rete arrivano da Windows come codici
    // grezzi che `ErrorKind` non sa nominare — diventerebbero un generico
    // `fs.readFailed`, cioè «questo file non si legge», quando la verità è «la
    // condivisione non c'è più». Le due frasi mandano a cercare il guasto in due
    // posti diversi, e solo una delle due offre «Riprova».
    if errore_di_rete(err) {
        return AppError::new(ErrorCode::FsNetworkUnavailable {
            path: Some(path.to_owned()),
        })
        .with_cause(err.to_string());
    }
    let code = match err.kind() {
        K::NotFound => ErrorCode::FsNotFound {
            path: path.to_owned(),
        },
        K::PermissionDenied => ErrorCode::FsPermissionDenied {
            path: path.to_owned(),
        },
        K::StorageFull | K::QuotaExceeded => ErrorCode::FsDiskFull {
            path: Some(path.to_owned()),
        },
        K::InvalidFilename => ErrorCode::FsPathInvalid {
            path: path.to_owned(),
        },
        _ => ErrorCode::FsReadFailed {
            path: path.to_owned(),
            detail: Some(err.kind().to_string()),
        },
    };
    AppError::new(code).with_cause(err.to_string())
}

/// Il guasto viene dalla rete, non dal file?
///
/// L'elenco dei numeri di sistema e il perché di ognuno stanno in
/// [`rymembra_domain::errors::rete`], e non qui, perché la stessa domanda se la
/// fa anche il motore audio quando la condivisione muore **a metà** di un
/// brano: due elenchi separati sarebbero due elenchi che un giorno diranno
/// cose diverse sullo stesso guasto.
fn errore_di_rete(err: &std::io::Error) -> bool {
    rymembra_domain::errors::rete::e_di_rete(err)
}

/// Il percorso sta su una condivisione di rete?
///
/// # A cosa serve, e a cosa no
///
/// Serve al **messaggio**, non alla decisione. Un `true` cambia la frase che
/// l'utente legge — «questa cartella sta su una condivisione, e il database lì
/// non ce la fa» invece di «il disco ha rifiutato una lettura» — e nient'altro:
/// niente si rifiuta e niente si sposta in base a questo `bool`.
///
/// E non potrebbe essere altrimenti, perché la risposta è **incompleta per
/// costruzione**: riconosce un percorso UNC dalla sua forma, e non ha modo di
/// sapere che `Z:\Musica` è una lettera mappata su `\\nas\musica`. Per saperlo
/// servirebbe `GetDriveTypeW`, cioè `unsafe` — che la radice del workspace
/// vieta con `forbid` — oppure una dipendenza nuova per leggere una lettera di
/// unità. Un riconoscimento che sbaglia per difetto va bene per scegliere una
/// frase; non andrebbe bene per decidere se aprire una libreria.
///
/// # Le forme riconosciute
///
/// - `\\server\condivisione\…`, e `//server/condivisione/…` con gli slash, che
///   Windows accetta uguale;
/// - `\\?\UNC\server\condivisione\…`, la stessa cosa in forma verbatim.
///
/// **Non** di rete: `C:\…`; `\\?\C:\…`, che è il prefisso scritto da
/// [`percorso_lungo`] su un disco locale — prenderlo per un UNC vorrebbe dire
/// che allungare un percorso locale lo fa diventare di rete; e `\\.\…`, che è
/// lo spazio dei nomi dei dispositivi e non ha un server dietro.
#[must_use]
pub fn percorso_di_rete(path: &Path) -> bool {
    // Su un percorso che non è UTF-8 non si risponde. Costa una frase meno
    // precisa, e una frase meno precisa è meglio di un percorso ricostruito a
    // tentoni per rispondere a una domanda che non decide niente.
    let Some(testo) = path.to_str() else {
        return false;
    };
    let separatore = |c: char| c == '\\' || c == '/';
    let mut inizio = testo.chars();
    if !matches!(inizio.next(), Some(c) if separatore(c))
        || !matches!(inizio.next(), Some(c) if separatore(c))
    {
        return false;
    }
    // I due separatori sono ASCII: tagliare a due byte non spezza un carattere.
    let resto = &testo[2..];
    let mut marcatore = resto.chars();
    match (marcatore.next(), marcatore.next()) {
        // `\\?\…`: verbatim. Di rete solo nella forma `\\?\UNC\server\…`.
        (Some('?'), Some(c)) if separatore(c) => {
            let dopo = &resto[2..];
            dopo.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("UNC"))
                && dopo.chars().nth(3).is_some_and(separatore)
        }
        // `\\.\PhysicalDrive0`: un dispositivo, non un server.
        (Some('.'), Some(c)) if separatore(c) => false,
        // Dopo due separatori, quel che resta è un nome di server.
        _ => !resto.is_empty(),
    }
}

/// Quanti caratteri bastano a mettersi al sicuro da `MAX_PATH`.
///
/// Windows taglia a 260 **compreso** il terminatore, e quei 260 sono il tetto
/// del percorso intero: un nome di file lungo appeso a una cartella già
/// profonda ci arriva senza che nessuno dei due sembri lungo da solo.
/// Duecentoquaranta lascia venti caratteri di margine per il nome che si sta
/// per appendere — `rymembra.db-wal` sono tredici — e sta abbastanza sopra i
/// percorsi veri (un album annidato su una share ne usa centoventi) perché il
/// ramo verbatim resti l'eccezione.
///
/// Si contano i **byte** di UTF-8 e non i caratteri, e la differenza è nella
/// direzione giusta: `MAX_PATH` conta unità UTF-16, che per i nomi accentati
/// sono meno dei byte UTF-8, quindi un percorso pieno di accenti supera la
/// soglia un po' prima del necessario. Prefissare presto non costa niente;
/// prefissare tardi costa il guasto che questa costante esiste per evitare.
const SOGLIA_PERCORSO_LUNGO: usize = 240;

/// Il percorso in forma verbatim (`\\?\`), quando è lungo e si può fare senza
/// cambiargli significato.
///
/// Fuori da Windows, e sotto la soglia, restituisce il percorso com'era: è il
/// caso normale, e deve costare una `to_path_buf` e nient'altro.
///
/// # Perché non basta appiccicare il prefisso
///
/// Perché `\\?\` **disattiva la normalizzazione** del sistema. Dentro un
/// percorso verbatim lo slash non è più un separatore, `.` e `..` non si
/// risolvono, i doppi separatori non si fondono, i punti e gli spazi in coda a
/// un nome non si tagliano più, e la cartella di lavoro non si consulta. Un
/// percorso che aveva bisogno di una di quelle cose, prefissato, **punta a un
/// file diverso** da quello che si voleva aprire — e il guasto che ne viene non
/// somiglia a «percorso troppo lungo», somiglia a «il file non c'è».
///
/// Quindi il prefisso si mette solo su un percorso che è già assoluto e già
/// canonico, e in ogni altro caso si restituisce il percorso intatto. Rinunciare
/// lascia un errore su un percorso lungo, che è il guasto che c'era prima;
/// sbagliare ne introdurrebbe uno su un percorso che funzionava.
///
/// **Non si canonicalizza per riuscirci.** `std::fs::canonicalize` tocca il
/// disco, e su una share morta è esattamente la chiamata che non torna — cioè
/// il guasto che tutto il resto di questo modulo è costruito per evitare.
#[must_use]
pub fn percorso_lungo(path: &Path) -> PathBuf {
    // `cfg!` e non `#[cfg]`: il corpo si compila su ogni piattaforma, così le
    // prove di `con_prefisso_verbatim` girano anche dove Windows non c'è.
    if cfg!(windows)
        && let Some(verbatim) = path.to_str().and_then(con_prefisso_verbatim)
    {
        return PathBuf::from(verbatim);
    }
    path.to_path_buf()
}

/// La decisione di [`percorso_lungo`], senza il filesystem e senza la
/// piattaforma: `None` vuol dire «lascialo com'è».
///
/// Separata perché è l'unica parte che può sbagliare, ed è provabile ovunque.
fn con_prefisso_verbatim(testo: &str) -> Option<String> {
    if testo.len() < SOGLIA_PERCORSO_LUNGO {
        return None;
    }
    // Gli slash: verbatim non li riconosce come separatori, e un `C:/a/b`
    // prefissato diventa un unico nome di file con degli slash dentro.
    if testo.contains('/') {
        return None;
    }
    // Già verbatim, o nello spazio dei nomi dei dispositivi: non si prefissa
    // due volte e non si prefissa un dispositivo.
    if testo.starts_with(r"\\?\") || testo.starts_with(r"\\.\") {
        return None;
    }
    let unc = testo.starts_with(r"\\");
    if !unc && !lettera_di_unita(testo) {
        return None;
    }
    // I segmenti, saltando i due vuoti che il doppio separatore iniziale di un
    // UNC produce. Per la forma con lettera di unità il primo segmento è `C:`,
    // che passa i controlli come tutti gli altri.
    for segmento in testo.split('\\').skip(if unc { 2 } else { 0 }) {
        // Vuoto: un doppio separatore in mezzo, che il sistema fonderebbe e
        // verbatim no. `.` e `..`: un percorso relativo travestito da assoluto.
        if segmento.is_empty() || segmento == "." || segmento == ".." {
            return None;
        }
        // Punto o spazio in coda: il sistema li taglia, verbatim li tiene, e il
        // nome che ne esce non è più il nome del file che c'è sul disco.
        if segmento.ends_with('.') || segmento.ends_with(' ') {
            return None;
        }
    }
    Some(if unc {
        // `\\server\cond` → `\\?\UNC\server\cond`: si mangia **uno** dei due
        // separatori iniziali, perché `UNC` prende il posto del primo.
        format!(r"\\?\UNC{}", &testo[1..])
    } else {
        format!(r"\\?\{testo}")
    })
}

/// Il percorso comincia con `X:\`?
fn lettera_di_unita(testo: &str) -> bool {
    let mut caratteri = testo.chars();
    matches!(caratteri.next(), Some(c) if c.is_ascii_alphabetic())
        && matches!(caratteri.next(), Some(':'))
        && matches!(caratteri.next(), Some('\\'))
}

/// I file musicali sul filesystem locale.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFiles;

impl MusicFiles for LocalFiles {
    /// Ricostruita sopra [`MusicFiles::walk_a_rate`], e non il contrario.
    ///
    /// La camminata vera sta di là perché è di là che serve consegnare man
    /// mano; qui si raccoglie quel che quella consegna. Due camminate sarebbero
    /// due idee su cosa conta come ramo perso, e prima o poi una intera e una a
    /// rate direbbero due cose diverse sulla stessa cartella.
    fn walk(&self, root: &str) -> Result<Camminata, AppError> {
        let mut file = Vec::new();
        let completa = self.walk_a_rate(root, &mut |voce| {
            // I rami persi qui non servono: `completa` dice già che l'elenco è
            // monco, e chi cammina intero non decide cancellazioni. I battiti
            // delle cartelle nemmeno: nessuno misura il silenzio di chi aspetta
            // la fine.
            if let Voce::File(trovato) = voce {
                file.push(trovato);
            }
            ControlFlow::Continue(())
        })?;
        Ok(Camminata { file, completa })
    }

    /// Quattro fili su una pila di cartelle, e le voci al chiamante per canale.
    ///
    /// # Perché più fili
    ///
    /// Perché su una share il costo di una cartella non è il disco, è il giro
    /// di rete: elencarla vuol dire chiedere e aspettare, e mentre si aspetta
    /// non si fa niente. Sul NAS dell'utente — 2.561 cartelle, 29.503 file —
    /// un filo solo ci metteva 10-12 secondi, quattro ne mettono 6,7, e oltre
    /// quattro non si guadagna più niente: il pavimento è il server.
    ///
    /// # Cosa non cambia
    ///
    /// - `su_voce` lo chiama solo il filo di chi ha chiamato: i fili mandano
    ///   le voci per canale, e il patto del tratto resta quello che era.
    /// - I collegamenti non si seguono: vedi `elenca_cartella`.
    /// - Una cartella che non si apre, o il cui elenco si interrompe a metà, è
    ///   un [`Voce::RamoPerso`] con il suo nome esatto; un file di cui non si
    ///   leggono tipo o data lo è della cartella che lo contiene.
    /// - Un `Break` ferma tutti i fili, e la funzione restituisce `false`.
    ///
    /// Cambia l'ordine: i file di una cartella arrivano in ordine di nome, le
    /// cartelle nell'ordine in cui i fili le finiscono.
    fn walk_a_rate(
        &self,
        root: &str,
        su_voce: &mut dyn FnMut(Voce) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        Ok(cammina_a_fili(
            root,
            FILI_CAMMINATA,
            elenca_cartella,
            su_voce,
        ))
    }

    fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
        // `percorso_lungo` e non `Path::new` nudo: un album annidato su una
        // share può sfondare i 260 caratteri di `MAX_PATH`, e da lì l'apertura
        // fallisce con «nome del file non valido» su un file che esiste. Il
        // percorso dell'errore resta quello originale — è quello che l'utente
        // riconosce, e un prefisso `\\?\` in un avviso non spiega niente a nessuno.
        let file = std::fs::File::open(percorso_lungo(Path::new(path)))
            .map_err(|err| io_error(path, &err))?;
        Ok(Box::new(std::io::BufReader::with_capacity(
            BUFFER_LETTURA,
            file,
        )))
    }

    fn radice_raggiungibile(&self, root: &str) -> bool {
        let percorso = root.to_owned();
        // Con la scadenza e non con una `metadata` nuda: su una share morta
        // quella chiamata non fallisce, aspetta il timeout di SMB — e la sonda
        // che serve a proteggere la libreria diventerebbe essa stessa il motivo
        // per cui la scansione si pianta.
        //
        // Scaduta vale **irraggiungibile**: la radice che non risponde in otto
        // secondi non è una radice da cui si possa concludere che dei brani sono
        // spariti.
        crate::scadenza::con_scadenza(SONDA_RADICE_NOME, SONDA_RADICE, move || {
            std::fs::metadata(percorso_lungo(Path::new(&percorso))).is_ok()
        })
        .unwrap_or(false)
    }
}

/// Quanti fili elencano cartelle insieme, per ogni radice.
///
/// Quattro, misurato e non scelto: sul NAS dell'utente un filo solo cammina la
/// libreria in 10-12 secondi, quattro in 6,7, di più non fanno meglio. Il limite
/// è il server che risponde, non chi chiede — e ogni filo in più è soltanto un
/// filo in più da lasciare appeso se la share muore a metà.
const FILI_CAMMINATA: usize = 4;

/// Quante voci possono aspettare nel canale prima che i fili si fermino.
///
/// Un tetto, e non un canale aperto: se chi consuma va piano, i fili aspettano
/// lui invece di ammucchiare in memoria l'elenco di un NAS intero.
const VOCI_IN_VOLO: usize = 512;

/// Ogni quanti file di una cartella si consegna senza aspettarne la fine.
///
/// I file di una cartella arrivano in ordine di nome, e per ordinarli bisogna
/// averli tutti: quindi si aspetta la fine dell'elenco. Ma una cartella piatta
/// da diecimila file su una share lenta è un elenco che dura, e chi consuma sta
/// misurando il silenzio: oltre questa soglia si consegna quel che si ha,
/// ordinato dentro il lotto. Gli album ci stanno sotto tutti.
const LOTTO_DI_FILE: usize = 512;

/// Chi elenca una cartella: [`elenca_cartella`], o un sostituto nelle prove.
///
/// Consegna file, battito e rami persi sul canale, e restituisce le
/// sottocartelle da camminare. `None` vuol dire «smetti»: chi ascoltava se n'è
/// andato, o qualcuno ha alzato la bandiera.
type Elenca = fn(&Path, &mpsc::SyncSender<Voce>, &AtomicBool) -> Option<Vec<PathBuf>>;

/// Quel che i fili di una camminata hanno in comune.
struct Cantiere {
    /// La pila delle cartelle e il conto di quelle non ancora finite.
    lavori: Mutex<Lavori>,
    /// Sveglia chi aspetta lavoro: è arrivata una cartella, o è tutto finito.
    sveglia: Condvar,
    /// «Basta»: chi consuma ha detto `Break`, o non c'è più nessuno che ascolti.
    basta: AtomicBool,
}

/// Lo stato della pila, sotto lucchetto.
struct Lavori {
    /// Le cartelle trovate e non ancora prese da nessuno.
    da_elencare: Vec<PathBuf>,
    /// Le cartelle trovate e non ancora finite: in pila **o in mano a un filo**.
    ///
    /// È questo, e non la pila vuota, a dire che la camminata è finita: con la
    /// pila vuota un filo può stare ancora elencando la cartella da cui ne
    /// usciranno altre cento.
    in_sospeso: usize,
}

impl Cantiere {
    fn nuovo(radice: PathBuf) -> Self {
        Self {
            lavori: Mutex::new(Lavori {
                da_elencare: vec![radice],
                in_sospeso: 1,
            }),
            sveglia: Condvar::new(),
            basta: AtomicBool::new(false),
        }
    }

    fn lavori(&self) -> MutexGuard<'_, Lavori> {
        // Avvelenato vuol dire che un filo è caduto con il lucchetto in mano.
        // Dentro ci sono una pila e un contatore, coerenti fra un'istruzione e
        // l'altra: arrendersi per questo farebbe di un panico una camminata
        // appesa.
        self.lavori.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// La prossima cartella da elencare, aspettando se serve. `None` quando
    /// non ce ne saranno altre, o quando qualcuno ha detto basta.
    fn prendi(&self) -> Option<PathBuf> {
        let mut lavori = self.lavori();
        loop {
            if self.basta.load(Ordering::SeqCst) || lavori.in_sospeso == 0 {
                return None;
            }
            if let Some(cartella) = lavori.da_elencare.pop() {
                return Some(cartella);
            }
            lavori = self
                .sveglia
                .wait(lavori)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Una cartella è finita, e da lei sono uscite `sotto`.
    ///
    /// Le figlie si aggiungono e la madre si toglie **sotto lo stesso
    /// lucchetto**: contare prima la fine e poi le figlie lascerebbe un istante
    /// con il conto a zero, e un filo che guardasse proprio allora se ne
    /// andrebbe con mezza libreria ancora da vedere.
    fn fatta(&self, mut sotto: Vec<PathBuf>) {
        // Alla rovescia, perché la pila si svuota dalla coda: così le sorelle
        // escono in ordine di nome, e con un filo solo la camminata sarebbe
        // quella di sempre.
        sotto.sort_unstable_by(|a, b| b.cmp(a));
        let mut lavori = self.lavori();
        lavori.in_sospeso = lavori
            .in_sospeso
            .saturating_add(sotto.len())
            .saturating_sub(1);
        lavori.da_elencare.append(&mut sotto);
        drop(lavori);
        self.sveglia.notify_all();
    }

    /// Ferma tutti i fili: chi elenca alla prossima voce, chi aspetta subito.
    fn ferma(&self) {
        self.basta.store(true, Ordering::SeqCst);
        // Il lucchetto, preso e lasciato, prima della sveglia: chi ha appena
        // guardato `basta` e sta per mettersi ad aspettare lo tiene in mano, e
        // una sveglia suonata in quell'istante andrebbe persa — con lui
        // addormentato per sempre.
        drop(self.lavori());
        self.sveglia.notify_all();
    }
}

/// Se un filo cade fuori dall'elenco di una cartella, ferma gli altri.
///
/// Il panico **dentro** una cartella lo raccoglie [`lavora`], e diventa il
/// ramo perso di quella cartella. Questo copre il resto — la contabilità della
/// pila — dove un filo che sparisse senza dirlo lascerebbe il conto delle
/// cartelle in sospeso sopra zero per sempre, e gli altri ad aspettarlo.
struct SeCade<'a>(&'a Cantiere);

impl Drop for SeCade<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.ferma();
        }
    }
}

/// Il lavoro di un filo: prendere cartelle ed elencarle, finché ce ne sono.
fn lavora(cantiere: &Cantiere, manda: &mpsc::SyncSender<Voce>, elenca: Elenca) {
    let _se_cade = SeCade(cantiere);
    while let Some(cartella) = cantiere.prendi() {
        // Un panico dentro una cartella — un nome che il sistema restituisce
        // storto, un difetto nostro — costa quella cartella e non la
        // camminata: diventa il suo ramo perso, e gli altri fili proseguono.
        let esito = std::panic::catch_unwind(AssertUnwindSafe(|| {
            elenca(&cartella, manda, &cantiere.basta)
        }));
        let sotto = match esito {
            Ok(Some(sotto)) => sotto,
            Ok(None) => {
                cantiere.ferma();
                Vec::new()
            }
            Err(_) => {
                if manda
                    .send(Voce::RamoPerso(testo_del_percorso(&cartella)))
                    .is_err()
                {
                    cantiere.ferma();
                }
                Vec::new()
            }
        };
        cantiere.fatta(sotto);
    }
}

/// La camminata di [`LocalFiles::walk_a_rate`], con l'elenco di una cartella
/// passato da fuori: le prove ci mettono una cartella che panica.
///
/// Restituisce [`Camminata::completa`]: `true` solo se nessun ramo è andato
/// perso e nessuno ha detto basta.
fn cammina_a_fili(
    root: &str,
    fili: usize,
    elenca: Elenca,
    su_voce: &mut dyn FnMut(Voce) -> ControlFlow<()>,
) -> bool {
    let cantiere = Cantiere::nuovo(PathBuf::from(root));
    std::thread::scope(|ambito| {
        let (manda, ricevi) = mpsc::sync_channel(VOCI_IN_VOLO);
        let mut avviati = Vec::with_capacity(fili);
        for numero in 0..fili {
            let manda = manda.clone();
            let cantiere = &cantiere;
            let filo = std::thread::Builder::new()
                .name(format!("rymembra-camminata-{numero}"))
                .spawn_scoped(ambito, move || lavora(cantiere, &manda, elenca));
            // Un filo che il sistema non dà è un filo in meno, non una
            // camminata in meno: bastano quelli che ci sono.
            if let Ok(filo) = filo {
                avviati.push(filo);
            }
        }
        // L'originale si lascia subito: il canale si chiude quando l'ultimo
        // filo lascia il suo, ed è così che il ciclo qui sotto sa che è finita.
        drop(manda);
        if avviati.is_empty() {
            // Nessun filo, nessuno ha guardato: la radice intera non è stata
            // verificata, ed è quel che si dice.
            let _ = su_voce(Voce::RamoPerso(root.to_owned()));
            return false;
        }

        // Ogni ramo perso abbassa questo, e non è pignoleria: è l'unica
        // differenza fra «qui non c'è più niente» e «di qui non si è visto
        // tutto», e a valle decide se dei brani si cancellano.
        let mut completa = true;
        let mut interrotta = false;
        for voce in &ricevi {
            if matches!(voce, Voce::RamoPerso(_)) {
                completa = false;
            }
            if su_voce(voce).is_break() {
                interrotta = true;
                break;
            }
        }
        // La bandiera, poi il canale chiuso, poi l'attesa dei fili: chi elenca
        // smette alla prossima voce, chi è fermo su un canale pieno si sblocca
        // con un errore, chi aspetta lavoro si sveglia. Aspettarli con il
        // canale ancora aperto vorrebbe dire aspettare per sempre quello fermo
        // a consegnare.
        cantiere.ferma();
        drop(ricevi);
        let caduti = avviati
            .into_iter()
            .map(std::thread::ScopedJoinHandle::join)
            .filter(Result::is_err)
            .count();
        // Chi ha detto basta non ha visto tutto: `false`, non `completa`.
        if interrotta {
            return false;
        }
        if caduti > 0 {
            // Un filo caduto fuori da una cartella: non si sa che cosa avesse
            // in mano, e la risposta onesta è la più larga.
            let _ = su_voce(Voce::RamoPerso(root.to_owned()));
            return false;
        }
        completa
    })
}

/// Elenca una cartella: consegna i suoi file e il battito, restituisce le
/// sottocartelle.
///
/// # I collegamenti non si seguono
///
/// Il tipo viene da [`std::fs::DirEntry::file_type`], che **non segue** il
/// collegamento: un link simbolico è un link, non la cartella a cui punta.
/// Seguirlo permetterebbe a un anello di far girare la scansione all'infinito,
/// e a un link verso la cartella accanto di contare ogni brano due volte. Su
/// Windows vale lo stesso per le giunzioni (`mklink /J`): sono punti di
/// rianalisi «surrogati di nome» come i link, e std le riporta come tali —
/// `is_symlink()` vero, `is_dir()` falso. La prova `una_giunzione_non_si_segue`
/// lo controlla sul sistema vero invece di fidarsi.
///
/// La radice invece si apre per quel che è: una radice che è essa stessa una
/// giunzione l'ha scelta l'utente, e si è sempre seguita.
///
/// # I metadati non costano
///
/// Su Windows [`std::fs::DirEntry::metadata`] restituisce quel che l'elenco ha
/// già portato — dimensione e data arrivano insieme al nome — e non tocca il
/// disco. Un `fs::metadata` per file sarebbe un giro di rete in più per ogni
/// brano, e per di più seguirebbe i collegamenti.
///
/// # I percorsi
///
/// La cartella si apre con [`percorso_lungo`], perché un album annidato su una
/// share può sfondare `MAX_PATH`; ma i percorsi consegnati si costruiscono
/// dalla cartella **senza** prefisso, la radice più i nomi: sono quelli che
/// finiscono in libreria, e un `\\?\` lì dentro farebbe di ogni brano un brano
/// nuovo.
fn elenca_cartella(
    cartella: &Path,
    manda: &mpsc::SyncSender<Voce>,
    basta: &AtomicBool,
) -> Option<Vec<PathBuf>> {
    let ramo_perso = || Voce::RamoPerso(testo_del_percorso(cartella));
    let elenco = match std::fs::read_dir(percorso_lungo(cartella)) {
        Ok(elenco) => elenco,
        // Una cartella illeggibile non ferma la camminata: si perde quel ramo,
        // non l'intera libreria. Ma **si dice**, e si dice quale: è il nome
        // del ramo che permette a valle di non giudicare quel che ci sta
        // sotto, invece di giudicarlo sparito. Se è la radice a non aprirsi, il
        // ramo è la radice.
        Err(_) => return manda.send(ramo_perso()).ok().map(|()| Vec::new()),
    };
    let mut sotto = Vec::new();
    let mut file = Vec::new();
    // L'elenco è arrivato in fondo: è la condizione del battito.
    let mut intera = true;
    // Un file elencato di cui non si legge il tipo o la data.
    let mut monca = false;
    for voce in elenco {
        if basta.load(Ordering::Relaxed) {
            return None;
        }
        // L'elenco che si interrompe a metà — la share che cade mentre
        // consegna i nomi — è un ramo perso come la cartella che non si apre:
        // quel che non è arrivato non si sa cosa fosse. Quel che è arrivato
        // prima si consegna e si cammina lo stesso: sta sotto il ramo perso e
        // a valle non si giudica, ma un brano nuovo trovato lì resta trovato.
        let Ok(voce) = voce else {
            intera = false;
            break;
        };
        let percorso = cartella.join(voce.file_name());
        // Un file elencato di cui non si legge il tipo o la data è un file su
        // cui non si può decidere niente: sparisce dall'elenco, quindi
        // l'elenco non è più intero. Il ramo perso è la sua cartella, e non il
        // file solo: l'errore sui metadati di un file è quasi sempre l'errore
        // di chi serve quella cartella, e i suoi vicini non sono una garanzia.
        let Ok(tipo) = voce.file_type() else {
            monca = true;
            continue;
        };
        if tipo.is_dir() {
            sotto.push(percorso);
            continue;
        }
        // Link, giunzioni, dispositivi: non si seguono e non si contano.
        if !tipo.is_file() {
            continue;
        }
        let Ok(metadati) = voce.metadata() else {
            monca = true;
            continue;
        };
        file.push(DiscoveredFile {
            path: testo_del_percorso(&percorso),
            size_bytes: metadati.len(),
            modified_ms: modified_ms(&metadati),
        });
        if file.len() >= LOTTO_DI_FILE {
            consegna_in_ordine(&mut file, manda)?;
        }
    }
    consegna_in_ordine(&mut file, manda)?;
    if intera {
        manda.send(Voce::Cartella).ok()?;
    }
    if !intera || monca {
        manda.send(ramo_perso()).ok()?;
    }
    Some(sotto)
}

/// Consegna i file raccolti in ordine di nome, e svuota il mucchio.
///
/// Stanno tutti nella stessa cartella, quindi l'ordine dei percorsi è l'ordine
/// dei nomi. `None` se chi ascoltava se n'è andato.
fn consegna_in_ordine(
    file: &mut Vec<DiscoveredFile>,
    manda: &mpsc::SyncSender<Voce>,
) -> Option<()> {
    file.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    for trovato in file.drain(..) {
        manda.send(Voce::File(trovato)).ok()?;
    }
    Some(())
}

/// Il percorso nella forma che viaggia: una stringa, anche dove il nome non è
/// UTF-8.
fn testo_del_percorso(percorso: &Path) -> String {
    percorso.to_string_lossy().into_owned()
}

/// Quanto si aspetta una radice prima di dichiararla irraggiungibile.
///
/// Otto secondi: molto più di quel che serve a un disco locale (microsecondi) o
/// a una share viva (millisecondi), molto meno dei quaranta e passa che Windows
/// impiega a rinunciare da solo su un percorso di rete morto. Il numero non deve
/// essere preciso — deve solo stare largo sul caso buono e stretto sul caso
/// cattivo.
const SONDA_RADICE: std::time::Duration = std::time::Duration::from_secs(8);

/// Il nome che il filo della sonda porta nel diario dei panici.
const SONDA_RADICE_NOME: &str = "sonda-radice";

/// Quanti byte alla volta si chiedono al filesystem leggendo un brano.
///
/// Sessantaquattro kibibyte, e il numero conta soltanto sulla rete. Su un disco
/// locale la dimensione del buffer sposta qualche microsecondo; su SMB **ogni
/// riempimento è un giro di rete**, e con gli otto kibibyte di serie di
/// `BufReader` un file da mezzo megabyte diventa una sessantina di andate e
/// ritorni con la latenza del Wi-Fi addosso a ognuna.
///
/// Sessantaquattro e non di più perché è la lettura massima che SMB2 negozia di
/// suo con Windows: chiederne di più non fa meno giri, li fa uguali con un
/// buffer più grosso. E per un lettore di tag il buffer grosso è sprecato due
/// volte, perché `lofty` guarda la testa del file e poi salta in coda: quel che
/// si legge in mezzo lo si è letto per niente.
const BUFFER_LETTURA: usize = 64 * 1024;

/// La data di modifica in millisecondi interi.
///
/// Il troncamento è deliberato e deve stare qui, non a valle: è la forma con cui
/// il database la salva e con cui la scansione confronta. Se il confronto
/// avvenisse fra un valore troncato e uno con i decimali, ogni passata vedrebbe
/// cambiato ogni file e una riscansione da centomila brani rileggerebbe tutti i
/// metadati invece di nessuno.
///
/// Un orologio che non si legge dà 0: un file «modificato all'epoca zero» verrà
/// riletto a ogni scansione, che è lento ma corretto — il contrario, dargli
/// l'ora attuale, lo farebbe sembrare aggiornato per sempre.
fn modified_ms(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn la_camminata_trova_i_file_annidati_e_non_filtra() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let nested = dir.path().join("Album").join("CD1");
        std::fs::create_dir_all(&nested).expect("cartelle");
        for name in ["a.mp3", "b.txt", "c.flac"] {
            let mut f = std::fs::File::create(nested.join(name)).expect("file");
            f.write_all(b"x").expect("scrittura");
        }

        let root = dir.path().to_string_lossy().into_owned();
        let found = LocalFiles.walk(&root).expect("camminata");

        // Anche il .txt: filtrare è una decisione, e sta nel dominio. Qui si
        // riporta quel che c'è, altrimenti il piano non potrebbe dire PERCHÉ un
        // file non è entrato in libreria.
        assert_eq!(found.file.len(), 3);
        assert!(found.file.iter().any(|f| f.path.ends_with("b.txt")));
        // Niente si è perso per strada: è la condizione che permette al piano di
        // fidarsi di questo elenco per decidere delle rimozioni.
        assert!(found.completa, "una cartella sana si cammina per intero");
    }

    #[test]
    fn la_camminata_a_rate_consegna_gli_stessi_file_di_quella_intera() {
        // Le due non devono divergere mai, e non per eleganza: la scansione
        // cammina a rate, ogni altro chiamante cammina intero, e se le due
        // vedessero cartelle diverse la libreria dipenderebbe da chi l'ha
        // guardata.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let nested = dir.path().join("Album").join("CD2");
        std::fs::create_dir_all(&nested).expect("cartelle");
        for name in ["a.mp3", "b.txt", "c.flac"] {
            let mut f = std::fs::File::create(nested.join(name)).expect("file");
            f.write_all(b"x").expect("scrittura");
        }
        let root = dir.path().to_string_lossy().into_owned();

        let mut a_rate = Vec::new();
        let completa = LocalFiles
            .walk_a_rate(&root, &mut |voce| {
                a_rate.push(voce);
                ControlFlow::Continue(())
            })
            .expect("camminata a rate");
        let intera = LocalFiles.walk(&root).expect("camminata intera");

        assert_eq!(completa, intera.completa);
        assert!(completa, "una cartella sana si cammina per intero");
        let mut percorsi: Vec<&str> = a_rate
            .iter()
            .filter_map(|voce| match voce {
                Voce::File(f) => Some(f.path.as_str()),
                Voce::Cartella => None,
                Voce::RamoPerso(ramo) => panic!("una cartella sana non perde rami: {ramo}"),
            })
            .collect();
        let mut attesi: Vec<&str> = intera.file.iter().map(|f| f.path.as_str()).collect();
        // Gli stessi file, non lo stesso ordine: con più fili l'ordine fra
        // cartelle diverse è quello in cui i fili finiscono, e non è un patto.
        percorsi.sort_unstable();
        attesi.sort_unstable();
        assert_eq!(percorsi, attesi, "stessi file");
    }

    #[test]
    fn chi_dice_basta_ferma_la_camminata_a_rate() {
        // Il patto dell'annullamento visto da qui: chi smette di ascoltare
        // ferma l'enumerazione invece di lasciarla girare per nessuno. E quel
        // che torna è `false`, cioè «parziale»: chi ha detto basta non ha
        // visto tutto, e su un elenco così non si cancella niente.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for name in ["a.mp3", "b.mp3", "c.mp3", "d.mp3"] {
            std::fs::write(dir.path().join(name), b"x").expect("file");
        }
        let root = dir.path().to_string_lossy().into_owned();

        let mut quanti = 0_usize;
        let completa = LocalFiles
            .walk_a_rate(&root, &mut |_| {
                quanti += 1;
                if quanti >= 2 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .expect("camminata a rate");

        assert_eq!(quanti, 2, "si ferma al secondo, non arriva al quarto");
        assert!(
            !completa,
            "una camminata interrotta non è un elenco su cui fidarsi"
        );
    }

    /// Tutto quel che una camminata a rate consegna, diviso per specie.
    #[derive(Debug, Default)]
    struct Raccolta {
        file: Vec<String>,
        battiti: usize,
        rami: Vec<String>,
    }

    impl Raccolta {
        fn metti(&mut self, voce: Voce) -> ControlFlow<()> {
            match voce {
                Voce::File(trovato) => self.file.push(trovato.path),
                Voce::RamoPerso(ramo) => self.rami.push(ramo),
                Voce::Cartella => self.battiti += 1,
            }
            ControlFlow::Continue(())
        }
    }

    /// La camminata vera, raccolta per intero, con il suo `bool`.
    fn raccogli(root: &str) -> (Raccolta, bool) {
        let mut raccolta = Raccolta::default();
        let completa = LocalFiles
            .walk_a_rate(root, &mut |voce| raccolta.metti(voce))
            .expect("camminata a rate");
        (raccolta, completa)
    }

    /// Un albero di prova: `larghezza` cartelle per livello fino a
    /// `profondita`, e `per_cartella` file in ognuna, radice compresa.
    ///
    /// Restituisce i file creati e quante cartelle ci sono, radice compresa.
    fn albero(
        radice: &Path,
        larghezza: usize,
        profondita: usize,
        per_cartella: usize,
    ) -> (Vec<String>, usize) {
        let mut file = Vec::new();
        let mut cartelle = 0;
        let mut da_fare = vec![(radice.to_path_buf(), 0)];
        while let Some((qui, livello)) = da_fare.pop() {
            cartelle += 1;
            // Creati alla rovescia: sul disco l'ordine di creazione non deve
            // diventare l'ordine di consegna.
            for n in (0..per_cartella).rev() {
                let percorso = qui.join(format!("{n:02}.mp3"));
                std::fs::write(&percorso, b"x").expect("file");
                file.push(percorso.to_string_lossy().into_owned());
            }
            if livello < profondita {
                for n in 0..larghezza {
                    let sotto = qui.join(format!("c{n}"));
                    std::fs::create_dir(&sotto).expect("cartella");
                    da_fare.push((sotto, livello + 1));
                }
            }
        }
        (file, cartelle)
    }

    #[test]
    fn un_albero_largo_e_profondo_si_vede_tutto_e_una_volta_sola() {
        // Il rischio di quattro fili su una pila sola non è la lentezza, è la
        // contabilità: una cartella presa da due fili conta i suoi brani due
        // volte, una sfuggita al conto delle cartelle in sospeso fa finire la
        // camminata prima del tempo — e a valle «non trovato» vuol dire
        // «sparito». Duecentocinquantanove cartelle su quattro livelli, con dei
        // file a ogni livello, radice compresa.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let (mut attesi, cartelle) = albero(dir.path(), 6, 3, 3);
        assert_eq!(cartelle, 1 + 6 + 36 + 216);
        let root = dir.path().to_string_lossy().into_owned();

        let (raccolta, completa) = raccogli(&root);

        assert!(completa, "un albero sano si cammina per intero");
        assert!(raccolta.rami.is_empty(), "rami persi: {:?}", raccolta.rami);
        assert_eq!(
            raccolta.battiti, cartelle,
            "un battito per ogni cartella elencata, radice compresa"
        );
        // Ogni file esattamente una volta: stesso numero, stesso insieme.
        let distinti: std::collections::HashSet<&String> = raccolta.file.iter().collect();
        assert_eq!(
            distinti.len(),
            raccolta.file.len(),
            "un file contato due volte"
        );
        let mut trovati = raccolta.file.clone();
        trovati.sort_unstable();
        attesi.sort_unstable();
        assert_eq!(trovati, attesi);

        // E dentro ogni cartella, in ordine di nome: le cartelle si mescolano
        // fra fili, i file di una stessa cartella no.
        let mut per_cartella: std::collections::HashMap<PathBuf, Vec<&String>> =
            std::collections::HashMap::new();
        for file in &raccolta.file {
            let cartella = Path::new(file).parent().expect("cartella").to_path_buf();
            per_cartella.entry(cartella).or_default().push(file);
        }
        for (cartella, file) in per_cartella {
            assert!(
                file.is_sorted(),
                "i file di {} non sono arrivati in ordine: {file:?}",
                cartella.display()
            );
        }
    }

    #[test]
    fn le_cartelle_vuote_battono_anche_loro() {
        // Una cartella vuota è un disco che ha risposto: senza battito, una
        // libreria con molte cartelle e pochi brani sembrerebbe ferma a chi
        // misura il silenzio, e l'avanzamento non le conterebbe.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for nome in ["uno", "due", "tre"] {
            std::fs::create_dir(dir.path().join(nome)).expect("cartella");
        }
        std::fs::create_dir_all(dir.path().join("a").join("b").join("c")).expect("cartelle");
        let root = dir.path().to_string_lossy().into_owned();

        let (raccolta, completa) = raccogli(&root);

        assert!(raccolta.file.is_empty());
        assert!(raccolta.rami.is_empty(), "rami persi: {:?}", raccolta.rami);
        assert_eq!(
            raccolta.battiti,
            1 + 3 + 3,
            "radice, tre sorelle, tre annidate"
        );
        // Vuota ma vista: è la sonda, a valle, a decidere se un vuoto è un
        // vuoto. Qui niente si è perso.
        assert!(completa);
    }

    #[test]
    fn chi_dice_basta_a_meta_albero_ferma_tutti_i_fili() {
        // Con un filo solo «basta» voleva dire smettere di girare il ciclo.
        // Con quattro vuol dire fermare anche gli altri tre, e soprattutto non
        // chiamare più chi ha detto basta: dopo il `Break` niente, nemmeno le
        // voci che erano già in volo nel canale.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let (file, _) = albero(dir.path(), 5, 2, 4);
        assert!(
            file.len() > 100,
            "serve un albero più grande del punto di arresto"
        );
        let root = dir.path().to_string_lossy().into_owned();

        let mut consegnati = 0_usize;
        let mut dopo = 0_usize;
        let mut fermato = false;
        let completa = LocalFiles
            .walk_a_rate(&root, &mut |voce| {
                if fermato {
                    dopo += 1;
                    return ControlFlow::Continue(());
                }
                if matches!(voce, Voce::File(_)) {
                    consegnati += 1;
                }
                if consegnati >= 10 {
                    fermato = true;
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .expect("camminata a rate");

        assert_eq!(consegnati, 10, "si ferma al decimo file");
        assert_eq!(dopo, 0, "dopo il basta non si consegna più niente");
        assert!(!completa, "chi ha detto basta non ha visto tutto");
    }

    #[test]
    fn un_filo_che_cade_su_una_cartella_perde_solo_quella() {
        // Un panico mentre si elenca una cartella non deve far cadere la
        // scansione, né lasciarla appesa ad aspettare un filo che non c'è più:
        // costa quella cartella, che diventa un ramo perso, e il resto si
        // cammina come sempre.
        fn elenca_o_cade(
            cartella: &Path,
            manda: &mpsc::SyncSender<Voce>,
            basta: &AtomicBool,
        ) -> Option<Vec<PathBuf>> {
            if cartella.file_name().is_some_and(|nome| nome == "rotta") {
                panic!("una cartella che fa cadere chi la elenca, a comando");
            }
            elenca_cartella(cartella, manda, basta)
        }
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for (cartella, nome) in [
            ("sana", "a.mp3"),
            ("rotta", "b.mp3"),
            ("rotta/sotto", "c.mp3"),
            ("altra", "d.mp3"),
        ] {
            let qui = dir.path().join(cartella);
            std::fs::create_dir_all(&qui).expect("cartella");
            std::fs::write(qui.join(nome), b"x").expect("file");
        }
        let root = dir.path().to_string_lossy().into_owned();

        let mut raccolta = Raccolta::default();
        let completa = cammina_a_fili(&root, FILI_CAMMINATA, elenca_o_cade, &mut |voce| {
            raccolta.metti(voce)
        });

        assert!(!completa, "un ramo perso è un elenco parziale");
        assert_eq!(
            raccolta.rami,
            vec![dir.path().join("rotta").to_string_lossy().into_owned()]
        );
        let mut trovati = raccolta.file;
        trovati.sort_unstable();
        let mut attesi = vec![
            dir.path()
                .join("sana")
                .join("a.mp3")
                .to_string_lossy()
                .into_owned(),
            dir.path()
                .join("altra")
                .join("d.mp3")
                .to_string_lossy()
                .into_owned(),
        ];
        attesi.sort_unstable();
        assert_eq!(trovati, attesi);
        assert_eq!(raccolta.battiti, 3, "radice, sana e altra");
    }

    #[cfg(windows)]
    #[test]
    fn una_giunzione_non_si_segue() {
        // Le giunzioni sono il collegamento di Windows che si crea senza
        // privilegi, e quindi quello che si trova davvero sui dischi. Una verso
        // la cartella accanto conterebbe ogni brano due volte; una verso la
        // radice non finirebbe mai.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let radice = dir.path().join("radice");
        let vera = radice.join("vera");
        std::fs::create_dir_all(&vera).expect("cartelle");
        std::fs::write(vera.join("a.mp3"), b"x").expect("file");
        let giunzione = |collegamento: &Path, bersaglio: &Path| {
            std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(collegamento)
                .arg(bersaglio)
                .output()
                .is_ok_and(|esito| esito.status.success())
        };
        if !giunzione(&radice.join("ponte"), &vera) || !giunzione(&vera.join("anello"), &radice) {
            eprintln!("mklink /J non riuscito: prova saltata");
            return;
        }
        // Un link simbolico vero, se il sistema lo concede (serve la modalità
        // sviluppatore o i privilegi): se c'è, vale lo stesso patto.
        let _ = std::os::windows::fs::symlink_dir(&vera, radice.join("simbolico"));

        // Prima quel che std dice di una giunzione, sul sistema vero: è su
        // questo che la camminata si regge, e non va preso sulla parola.
        let ponte = std::fs::read_dir(&radice)
            .expect("elenco")
            .filter_map(Result::ok)
            .find(|voce| voce.file_name() == "ponte")
            .expect("la giunzione sta nell'elenco");
        let tipo = ponte.file_type().expect("tipo");
        assert!(
            tipo.is_symlink(),
            "una giunzione deve risultare un collegamento"
        );
        assert!(!tipo.is_dir(), "e non una cartella da attraversare");

        let (raccolta, completa) = raccogli(&radice.to_string_lossy());

        assert_eq!(
            raccolta.file,
            vec![vera.join("a.mp3").to_string_lossy().into_owned()],
            "il brano una volta sola, dalla cartella vera"
        );
        assert_eq!(raccolta.battiti, 2, "radice e vera, nient'altro");
        assert!(raccolta.rami.is_empty(), "rami persi: {:?}", raccolta.rami);
        assert!(completa);
    }

    #[cfg(windows)]
    #[test]
    fn un_percorso_oltre_max_path_si_cammina_e_torna_senza_prefisso() {
        // Un album annidato su una share sfonda i 260 caratteri senza che
        // nessun nome sembri lungo. La cartella si apre con il prefisso
        // verbatim, ma il percorso consegnato resta quello senza: è quello che
        // finisce in libreria, e con il prefisso ogni brano sarebbe nuovo.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let mut profonda = dir.path().to_path_buf();
        while profonda.as_os_str().len() < 300 {
            profonda.push("una cartella dal nome lungo");
        }
        std::fs::create_dir_all(&profonda).expect("cartelle profonde");
        let brano = profonda.join("brano.mp3");
        std::fs::write(&brano, b"x").expect("file");
        let root = dir.path().to_string_lossy().into_owned();

        let (raccolta, completa) = raccogli(&root);

        assert!(completa, "rami persi: {:?}", raccolta.rami);
        assert_eq!(raccolta.file, vec![brano.to_string_lossy().into_owned()]);
        let trovato = raccolta.file.first().expect("il brano");
        assert!(trovato.len() > 260);
        assert!(!trovato.starts_with(r"\\?\"), "il prefisso non deve uscire");
    }

    #[test]
    fn una_radice_inesistente_non_fa_cadere_la_scansione() {
        // Il disco esterno staccato: si riporta vuoto, e le altre radici
        // vengono scansionate lo stesso.
        let found = LocalFiles.walk("Z:/non/esiste").expect("nessun errore");
        assert!(found.file.is_empty());
        // Vuota **e** dichiarata parziale: non aver potuto guardare non è la
        // stessa cosa di aver guardato e non aver trovato niente, ed è la
        // differenza su cui il piano decide se cancellare.
        assert!(
            !found.completa,
            "una radice che non si apre non produce un elenco su cui fidarsi"
        );
    }

    #[test]
    fn una_radice_che_non_si_apre_consegna_se_stessa_come_ramo_perso() {
        // Il patto dei rami persi, sulla camminata vera: chi abbassa
        // `completa` dice anche dove. Qui il ramo è la radice intera, perché è
        // lei a non aprirsi — e a valle vuol dire «di qui non si toglie niente».
        let mut rami = Vec::new();
        let completa = LocalFiles
            .walk_a_rate("Z:/non/esiste", &mut |voce| {
                if let Voce::RamoPerso(ramo) = voce {
                    rami.push(ramo);
                }
                ControlFlow::Continue(())
            })
            .expect("nessun errore");
        assert!(!completa);
        assert_eq!(rami, vec!["Z:/non/esiste".to_owned()]);
    }

    #[test]
    fn la_camminata_di_serie_consegna_la_radice_quando_e_monca() {
        // Chi implementa soltanto `walk` non sa dire dove ha perso un ramo: il
        // corpo di serie di `walk_a_rate` consegna allora la radice intera,
        // che è la risposta larga — e a valle costa prudenza, non brani.
        struct Monco;
        impl MusicFiles for Monco {
            fn walk(&self, _root: &str) -> Result<Camminata, AppError> {
                Ok(Camminata {
                    file: vec![DiscoveredFile {
                        path: r"C:\m\a.mp3".to_owned(),
                        size_bytes: 1,
                        modified_ms: 1,
                    }],
                    completa: false,
                })
            }
            fn open(&self, path: &str) -> Result<Box<dyn ReadSeek + Send + Sync>, AppError> {
                Err(AppError::new(ErrorCode::FsNotFound {
                    path: path.to_owned(),
                }))
            }
        }
        let mut voci = Vec::new();
        let completa = Monco
            .walk_a_rate(r"C:\m", &mut |voce| {
                voci.push(voce);
                ControlFlow::Continue(())
            })
            .expect("camminata");
        assert!(!completa);
        assert_eq!(voci.len(), 2);
        assert_eq!(voci.last(), Some(&Voce::RamoPerso(r"C:\m".to_owned())));
    }

    #[test]
    fn la_sonda_delle_radici_distingue_quel_che_ce_da_quel_che_non_ce() {
        // La sonda vera, non quella dei doppi: tutte le prove sulle radici morte
        // in `library.rs` girano su `FintoDisco`, che sovrascrive
        // `radice_raggiungibile` — quindi l'implementazione che poi gira in
        // produzione, con il filo e la scadenza, non era esercitata da niente.
        //
        // Quel che si può provare senza una share vera è il resto: che la sonda
        // risponda, e che risponda giusto nei due casi normali. Il ramo della
        // scadenza scaduta è provato in `scadenza.rs`, dove non serve un NAS.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let root = dir.path().to_string_lossy().into_owned();
        assert!(
            LocalFiles.radice_raggiungibile(&root),
            "una cartella che esiste deve rispondere"
        );
        assert!(
            !LocalFiles.radice_raggiungibile("Z:/non/esiste/di/sicuro"),
            "e una che non esiste no: da qui passa la decisione di non cancellare"
        );
    }

    #[test]
    fn la_sonda_di_una_radice_viva_non_ci_mette_niente() {
        // Il numero che conta non è otto secondi, è «molto meno di otto
        // secondi»: la sonda gira una volta per radice a ogni scansione, e se
        // costasse gira sull'ordine dei secondi anche a disco sano sarebbe lei
        // il motivo per cui una scansione parte tardi.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let root = dir.path().to_string_lossy().into_owned();
        let prima = std::time::Instant::now();
        assert!(LocalFiles.radice_raggiungibile(&root));
        let passato = prima.elapsed();
        assert!(
            passato < std::time::Duration::from_secs(1),
            "la sonda su un disco locale ci ha messo {passato:?}"
        );
    }

    #[test]
    fn la_data_di_modifica_e_intera() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, b"x").expect("scrittura");
        let root = dir.path().to_string_lossy().into_owned();
        let found = LocalFiles.walk(&root).expect("camminata");
        let file = found.file.first().expect("un file");
        assert!(file.modified_ms > 0, "l'orologio deve essere leggibile qui");
        // Due camminate di fila devono dare lo stesso valore: se il troncamento
        // non fosse stabile, ogni passata vedrebbe «cambiato» ogni file.
        let ancora = LocalFiles.walk(&root).expect("camminata");
        assert_eq!(
            ancora.file.first().map(|f| f.modified_ms),
            Some(file.modified_ms)
        );
    }

    #[test]
    fn un_percorso_unc_si_riconosce() {
        // Le tre forme che si vedono davvero: quella normale, quella con gli
        // slash (che Windows accetta, e che i percorsi scritti a mano dentro
        // questo repository usano) e quella verbatim.
        assert!(percorso_di_rete(Path::new(r"\\nas\musica\Album\a.flac")));
        assert!(percorso_di_rete(Path::new("//nas/musica/Album/a.flac")));
        assert!(percorso_di_rete(Path::new(r"\\?\UNC\nas\musica\a.flac")));
        // Minuscolo uguale: il prefisso verbatim non è sensibile al caso, e un
        // percorso che arriva da un file di configurazione può averlo scritto
        // come gli pare.
        assert!(percorso_di_rete(Path::new(r"\\?\unc\nas\musica")));
        // Il solo nome del server, senza condivisione: è già di rete, e la
        // frase da mostrare è la stessa.
        assert!(percorso_di_rete(Path::new(r"\\nas")));
    }

    #[test]
    fn un_percorso_locale_non_e_di_rete() {
        assert!(!percorso_di_rete(Path::new(r"C:\Users\tizio\Musica")));
        assert!(!percorso_di_rete(Path::new("/home/tizio/musica")));
        assert!(!percorso_di_rete(Path::new("Musica/a.flac")));
        // Il prefisso che `percorso_lungo` scrive su un disco locale: se questo
        // passasse per UNC, allungare un percorso lo farebbe diventare di rete,
        // e il messaggio parlerebbe di condivisioni a chi non ne ha.
        assert!(!percorso_di_rete(Path::new(r"\\?\C:\Users\tizio\Musica")));
        // Lo spazio dei nomi dei dispositivi: due separatori in testa e nessun
        // server dietro.
        assert!(!percorso_di_rete(Path::new(r"\\.\PhysicalDrive0")));
        // E una lettera mappata resta **non** riconosciuta, che è il limite
        // dichiarato nella documentazione della funzione: se un giorno
        // qualcuno la facesse rispondere `true` a tentoni, questa riga cade e
        // lo si vede prima di spedirlo.
        assert!(!percorso_di_rete(Path::new(r"Z:\Musica")));
    }

    #[test]
    fn percorso_lungo_non_tocca_i_corti() {
        // Il caso normale, che è tutti i casi tranne uno: il percorso torna
        // identico, e nessun `\\?\` va a finire dentro un nome salvato in
        // database o mostrato in un avviso.
        for corto in [
            r"C:\Users\tizio\Musica\Album\a.flac",
            r"\\nas\musica\a.flac",
            "/home/tizio/musica/a.flac",
            "a.flac",
        ] {
            assert_eq!(
                percorso_lungo(Path::new(corto)),
                PathBuf::from(corto),
                "«{corto}» non è lungo e non va toccato"
            );
        }
        // E la decisione pura dice la stessa cosa, anche dove Windows non c'è.
        assert_eq!(con_prefisso_verbatim(r"C:\Musica\a.flac"), None);
    }

    #[test]
    fn percorso_lungo_prefissa_solo_quel_che_resta_se_stesso() {
        // Trenta cartelle annidate, senza separatore in coda: duecentosessanta
        // e passa caratteri, che è il caso vero di un album dentro un box set
        // dentro una discografia su una share.
        let lungo = vec!["cartella"; 30].join("\\");

        // Oltre la soglia e già canonico: il prefisso si mette.
        let profondo = format!(r"C:\Musica\{lungo}\a.flac");
        assert!(profondo.len() > SOGLIA_PERCORSO_LUNGO);
        assert_eq!(
            con_prefisso_verbatim(&profondo),
            Some(format!(r"\\?\{profondo}"))
        );

        // UNC: `UNC` prende il posto di **uno** dei due separatori iniziali. Se
        // se li mangiasse entrambi, o nessuno, il percorso non si aprirebbe.
        let condiviso = format!(r"\\nas\musica\{lungo}\a.flac");
        assert_eq!(
            con_prefisso_verbatim(&condiviso).as_deref(),
            Some(format!(r"\\?\UNC\nas\musica\{lungo}\a.flac").as_str())
        );

        // E i cinque casi in cui si rinuncia, perché il prefisso cambierebbe il
        // significato del percorso invece di allungarlo. Rinunciare lascia il
        // guasto che c'era prima; sbagliare ne introduce uno nuovo su un
        // percorso che funzionava.
        let con_slash = lungo.replace('\\', "/");
        for (percorso, motivo) in [
            (
                format!("C:/Musica/{con_slash}/a.flac"),
                "gli slash non sono separatori dentro un verbatim",
            ),
            (
                format!(r"C:\Musica\..\{lungo}\a.flac"),
                "verbatim non risolve `..`",
            ),
            (
                format!(r"Musica\{lungo}\a.flac"),
                "un relativo non si risolve rispetto a niente",
            ),
            (
                format!(r"C:\Musica\{lungo}\cartella.\a.flac"),
                "il punto in coda, che il sistema taglia e verbatim tiene",
            ),
            (
                format!(r"C:\Musica\{lungo}\\a.flac"),
                "il doppio separatore, che il sistema fonde e verbatim no",
            ),
            (
                format!(r"\\?\C:\Musica\{lungo}\a.flac"),
                "è già verbatim, e non si prefissa due volte",
            ),
        ] {
            assert_eq!(
                con_prefisso_verbatim(&percorso),
                None,
                "si doveva rinunciare: {motivo}"
            );
        }
    }

    #[test]
    fn gli_errori_di_io_si_distinguono() {
        use rymembra_domain::errors::ErrorCodeKind;
        let not_found = io_error("x", &std::io::Error::from(std::io::ErrorKind::NotFound));
        let denied = io_error(
            "x",
            &std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        // Durante una scansione i due vogliono reazioni diverse: il primo «salta
        // e prosegui», il secondo «dillo all'utente». Il vecchio albero li
        // appiattiva entrambi in un avviso nel log.
        assert_eq!(not_found.code().kind(), ErrorCodeKind::FsNotFound);
        assert_eq!(denied.code().kind(), ErrorCodeKind::FsPermissionDenied);
    }

    #[cfg(windows)]
    #[test]
    fn i_guasti_della_share_si_riconoscono_dal_numero() {
        use rymembra_domain::errors::ErrorCodeKind;
        // Questi tre sono quelli che si vedono davvero staccando la rete mentre
        // Rymembra legge: senza il ramo dei codici grezzi arriverebbero tutti come
        // `fs.readFailed`, cioè «il file è rotto» invece di «la share non c'è».
        for numero in [53, 59, 64] {
            let err = io_error(
                "//srv/musica/a.mp3",
                &std::io::Error::from_raw_os_error(numero),
            );
            assert_eq!(
                err.code().kind(),
                ErrorCodeKind::FsNetworkUnavailable,
                "il codice {numero} deve dire «rete», non «file»"
            );
            assert!(err.code().is_retryable(), "la rete torna: si ritenta");
        }
        // …e un file che davvero non c'è resta un file che non c'è: il ramo
        // nuovo non deve inghiottire il caso normale.
        let mancante = io_error("C:/m/a.mp3", &std::io::Error::from_raw_os_error(2));
        assert_eq!(mancante.code().kind(), ErrorCodeKind::FsNotFound);
    }
}
