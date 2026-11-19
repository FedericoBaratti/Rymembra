//! La skin attiva, compilata dal nucleo.
//!
//! Questo modulo non produce una riga di CSS: la produce `rymembra-skin`, e qui si
//! sceglie soltanto quale skin compilare. È la stessa regola dei comandi —
//! nessuna decisione fuori dal nucleo — e vale doppio qui, perché su Android il
//! foglio non servirà affatto e serviranno gli stessi token in un'altra forma.
//!
//! # Il foglio di base, e perché è copiato in `stile.css`
//!
//! L'IPC risponde qualche millisecondo dopo il primo disegno. In quei
//! millisecondi la finestra esiste e i token no, quindi `stile.css` porta il
//! blocco della skin di serie sotto `:root`, e il compilatore lo sovrascrive con
//! `:root[data-skin='<id>']`, che è più specifico. È lo stesso strato che nel
//! vecchio albero era il blocco `:root` di `global.css`.
//!
//! Una copia è una cosa che può divergere in silenzio, ed è esattamente il
//! difetto che il motore delle skin esiste per togliere. Perciò non è affidata a
//! un commento: il test in fondo la confronta con l'uscita del compilatore.
//!
//! # Il fondo dell'avvio a freddo, e come è stato chiuso
//!
//! `backgroundColor` in `tauri.conf.json` è quello di `plain`, e il test in
//! fondo lo tiene agganciato lì: è il colore che il sistema operativo dipinge
//! **prima** che esista una pagina, e non può venire dall'IPC perché l'IPC non
//! risponde ancora.
//!
//! Con le skin installate, chi ne sceglieva una chiara vedeva quindi un
//! fotogramma del fondo scuro di `plain` a ogni avvio. La via scelta non è
//! riscrivere la configurazione della finestra — quel valore resta agganciato a
//! `plain`, ed è giusto così — ma **non mostrare la finestra finché non c'è il
//! colore giusto**: nasce con `visible: false`, e il comando `pronto` in
//! `main.rs` la mostra dopo che skin e tema sono sul documento. Una rete di
//! sicurezza in Rust la mostra comunque dopo due secondi, perché
//! un'applicazione invisibile sarebbe un guasto peggiore del difetto che si
//! stava togliendo.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rymembra_app::settings::{CHIAVE_ACCENTO_DINAMICO, CHIAVE_SKIN};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_skin::package::limits::MAX_ARCHIVE_BYTES;
use serde::Serialize;

use crate::disparte::{comanda, leggi_la_libreria, stato_di};
use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// La skin usata quando non se ne chiede una.
pub const DI_SERIE: &str = "plain";

/// È una delle skin compilate dentro l'applicazione.
///
/// Non si installano, non si disinstallano, e un pacchetto con lo stesso id
/// sarebbe un file che nessuno leggerebbe mai: `sorgente` risponde prima di
/// guardare il disco.
fn di_serie(id: &str) -> bool {
    rymembra_skin::di_serie(id).is_some()
}

/// Dove stanno le skin installate.
fn cartella_skin(data_dir: &Path) -> PathBuf {
    data_dir.join("skin")
}

/// L'identificatore è anche un nome di file: non deve poter uscire dalla
/// cartella.
///
/// `..`, le barre e i due punti di un percorso di Windows sono tutto ciò che
/// serve a scrivere altrove, e un dispositivo di Windows (`con`, `nul`) è un
/// nome che non scrive affatto. Il formato dichiara già che un id è fatto di
/// lettere, cifre e trattini — qui si controlla, perché questo è il punto in cui
/// diventa un percorso. La regola è [`rymembra_domain::paths::id_di_file`].
fn id_sicuro(id: &str) -> bool {
    rymembra_domain::paths::id_di_file(id)
}

/// Il nome di una skin **già installata**: non esce dalla cartella, ma può
/// essere il nome di un dispositivo di Windows.
///
/// `aux`, `con` e compagni non si installano più ([`id_sicuro`]), ma fino alla
/// 3.2.0 si installavano — su Android e su Windows 11 il file si scrive — e
/// dopo l'aggiornamento sparivano dall'elenco in silenzio, senza un modo di
/// toglierli (R04b-6). Per leggere, elencare e disinstallare quel che c'è già
/// basta che il nome non porti altrove: lettere e cifre ASCII, trattini.
fn nome_installato(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Il manifest di una skin: quella di serie, o una installata.
///
/// Restituisce una `String` e non un `&'static str` perché ora le due sorgenti
/// sono di natura diversa — una è compilata dentro il binario, l'altra si legge
/// da un archivio. Il tipo di ritorno è lo stesso per entrambe, perché una skin
/// **è** il suo manifest: da qui in giù non c'è più differenza.
fn sorgente(data_dir: &Path, id: &str) -> Result<String, AppError> {
    if let Some(testo) = rymembra_skin::di_serie(id) {
        return Ok(testo.to_owned());
    }
    if !nome_installato(id) {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    // Un nome di dispositivo non si indossa — il formato lo rifiuta — e non si
    // apre nemmeno: su Windows 10 `skin\com1.ryskin` è la porta seriale, e
    // leggerla può bloccare il comando che dà alla finestra la sua
    // impaginazione. Si dice che non si legge prima di toccare il disco, e la
    // finestra ripiega col suo perché (R04b-6).
    if rymembra_domain::paths::riservato_da_windows(id) {
        return Err(AppError::new(ErrorCode::SkinManifestInvalid {
            detail: Some(format!(
                "«{id}» è il nome di un dispositivo di Windows: la skin va rinominata"
            )),
        }));
    }
    let percorso = cartella_skin(data_dir).join(format!("{id}.ryskin"));
    let bytes = std::fs::read(&percorso)
        .map_err(|_| AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }))?;
    // Le guardie dell'archivio non fidato — traversal del percorso, zip bomb,
    // tipo mentito — sono le stesse anche quando il file l'ha messo lì
    // l'utente: una skin scaricata da internet e copiata a mano nella cartella
    // non è più fidata di una arrivata dalla rete. Il tetto dei pixel no: vale
    // per installare, e una skin già installata resta indossata (D-53).
    let pacchetto = rymembra_skin::package::leggi_installato(&bytes)?;
    Ok(pacchetto.source)
}

/// Quel che una skin dice sull'impaginazione e sul movimento.
///
/// # Perché adesso attraversa l'IPC
///
/// `plain.json` dichiara `layout` e `motion.intensity` da sempre, e da sempre
/// nessuno li leggeva: il compilatore scrive `--motion-intensity` in un foglio
/// che nessuna regola interroga, e `SkinLayout` non usciva nemmeno dal crate.
/// Erano quattro campi che una skin poteva scrivere sapendo che non sarebbero
/// serviti a niente — cioè quattro modi di far perdere tempo a chi la scrive.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpaginazioneIpc {
    /// `compact`, `comfortable` o `spacious`.
    pub density: &'static str,
    /// `none`, `essential`, `full` o `maximum`.
    ///
    /// Si **compone** con `prefers-reduced-motion`, non lo sovrascrive: chi ha
    /// spento le animazioni nel sistema operativo vince sulla skin, sempre.
    pub motion: &'static str,
    /// L'albero: dove stanno le cose.
    ///
    /// Sempre popolato, anche per una skin che non dichiara niente — riceve
    /// l'albero di serie. È la ragione per cui il renderer non ha un caso
    /// «manca lo scafale» e l'albero di serie non è duplicato in TypeScript.
    pub shell: NodoIpc,
}

/// Il valore di una manopola, nella forma che JSON già ha.
///
/// Tre tipi e nessun involucro: `untagged` fa uscire `true`, `"large"` e `3`
/// invece di `{"kind":"flag","flag":true}`. Dall'altra parte diventa
/// `boolean | string | number`, che TypeScript restringe da solo con `typeof` —
/// un involucro sarebbe una forma da smontare a mano in ogni widget.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ValoreOpzioneIpc {
    /// Acceso o spento.
    Flag(bool),
    /// Una parola del vocabolario.
    Word(&'static str),
    /// Un intero.
    Count(u32),
}

/// Un nodo dello scafale, come lo riceve la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodoIpc {
    /// `zone` o `widget`.
    pub kind: &'static str,
    /// L'indirizzo, da mettere su `data-nodo`.
    ///
    /// Lo calcola il **compilatore** camminando l'albero, ed è lo stesso che
    /// finisce nei selettori del foglio: i due lati non devono accordarsi su
    /// niente, perché l'indirizzo è una funzione della posizione.
    pub at: String,
    /// Per una zona: `row`, `column` o `scroll`. Per un widget: il suo nome.
    pub name: &'static str,
    /// `hug`, `fill`, o una lunghezza — la stessa scrittura del documento.
    ///
    /// Sempre popolata, anche quando il documento taceva: chi legge non deve
    /// conoscere la tabella dei difetti. È anche ciò che permette all'editor di
    /// riscrivere l'albero intero da quel che ha ricevuto, invece di doverlo
    /// ricomporre dal documento e dai difetti insieme.
    pub size: String,
    /// L'aria fra i figli: `none`, `xs`, `s`, `m`, `l`, `xl`. Vuoto per un widget.
    pub gap: Option<&'static str>,
    /// `start`, `center`, `end`, `stretch`. Vuoto per un widget.
    pub align: Option<&'static str>,
    /// `start`, `center`, `end`, `between`. Vuoto per un widget.
    pub spread: Option<&'static str>,
    /// La parte del registro di questo nodo.
    ///
    /// Una zona la porta in classe; un widget no, perché la porta già il suo
    /// componente, e qui serve allo Studio per sapere quale parte selezionare.
    pub part: Option<&'static str>,
    /// Da quale prefab viene questo sottoalbero, quando viene da un prefab.
    ///
    /// L'espansione è già avvenuta: il renderer non ha bisogno di saperlo. Serve
    /// all'editor, per dire «questo viene da un prefab, e modificarlo qui
    /// modificherebbe anche gli altri usi».
    pub from_prefab: Option<String>,
    /// Il buco in cui la finestra infila il suo contenuto.
    pub slot: Option<&'static str>,
    /// Le manopole, **tutte**, coi difetti già applicati.
    pub options: BTreeMap<&'static str, ValoreOpzioneIpc>,
    /// I figli. Vuoto per un widget.
    pub children: Vec<NodoIpc>,
}

/// L'albero, con gli indirizzi già calcolati.
fn nodo(zona: &rymembra_skin::LayoutZone, via: &mut Vec<usize>) -> NodoIpc {
    use rymembra_skin::LayoutNode;

    let mut figli = Vec::with_capacity(zona.children.len());
    for (indice, figlio) in zona.children.iter().enumerate() {
        via.push(indice);
        figli.push(match figlio {
            LayoutNode::Zone(sotto) => nodo(sotto, via),
            LayoutNode::Widget(istanza) => NodoIpc {
                kind: "widget",
                at: indirizzo(via),
                name: istanza.def.name,
                size: misura(istanza.size),
                gap: None,
                align: None,
                spread: None,
                part: istanza.def.part,
                from_prefab: None,
                slot: istanza.def.slot,
                options: istanza
                    .options
                    .iter()
                    .map(|(opzione, valore)| (opzione.name, valore_opzione(*valore)))
                    .collect(),
                children: Vec::new(),
            },
        });
        via.pop();
    }

    NodoIpc {
        kind: "zone",
        at: indirizzo(via),
        name: zona.kind.as_str(),
        size: misura(zona.size),
        gap: Some(zona.gap.as_str()),
        align: Some(zona.align.as_str()),
        spread: Some(zona.spread.as_str()),
        part: zona.part.map(|def| def.name),
        from_prefab: zona.from_prefab.clone(),
        slot: None,
        options: BTreeMap::new(),
        children: figli,
    }
}

/// Una misura, nella stessa scrittura che il documento accetta in ingresso.
///
/// È la proprietà che permette all'editor di riscrivere l'albero da quel che ha
/// ricevuto: quel che esce di qui, rimesso dentro, si rilegge uguale.
fn misura(size: rymembra_skin::layout::TrackSize) -> String {
    use rymembra_skin::layout::TrackSize;
    match size {
        TrackSize::Hug => "hug".to_owned(),
        TrackSize::Fill => "fill".to_owned(),
        TrackSize::Fixed(length) => rymembra_skin::values::format_length(length),
    }
}

/// L'indirizzo di un nodo: il percorso degli indici dei figli.
///
/// Deve dare **la stessa stringa** di `indirizzo()` nel compilatore: è ciò che
/// tiene insieme il selettore del foglio e l'attributo nel DOM. Sono due righe
/// e non un `pub use` perché il compilatore non ha ragione di esporre la sua
/// forma interna — e il test `l_indirizzo_e_quello_del_foglio` confronta le due.
fn indirizzo(via: &[usize]) -> String {
    if via.is_empty() {
        return "radice".to_owned();
    }
    via.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join("-")
}

const fn valore_opzione(valore: rymembra_skin::layout::OptionValue) -> ValoreOpzioneIpc {
    use rymembra_skin::layout::OptionValue;
    match valore {
        OptionValue::Flag(acceso) => ValoreOpzioneIpc::Flag(acceso),
        OptionValue::Word(parola) => ValoreOpzioneIpc::Word(parola),
        OptionValue::Count(quanti) => ValoreOpzioneIpc::Count(quanti),
    }
}

/// Una skin compilata, nella forma che la finestra riceve.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinIpc {
    /// L'identificatore, da mettere su `data-skin`.
    pub id: String,
    /// Il foglio, da adottare così com'è.
    pub css: String,
    /// Il costo dei motivi e delle superfici, sul budget della singola superficie.
    pub cost: u32,
    /// I token che seguono la copertina: quando ci sarà la riproduzione, sono
    /// quelli che il runtime dovrà riscrivere a ogni brano.
    pub dynamic_tokens: Vec<String>,
    /// Il tema di base: `dark` o `light`.
    pub scheme: &'static str,
    /// Ha la variante del tema opposto.
    ///
    /// Senza, la skin ha un tema solo, e la scelta dell'utente non la cambia:
    /// `data-theme` dice comunque quello che si vede.
    pub variante: bool,
    /// Permette all'accento di seguire la copertina.
    ///
    /// È `capabilities.dynamicAccent`, cioè una dichiarazione dell'autore della
    /// skin, e vince sulla preferenza dell'utente: una skin può essere
    /// costruita attorno al suo accento — `vinile` lo è, e infatti dice di no.
    pub dynamic_accent: bool,
    /// Impaginazione e movimento dichiarati.
    pub layout: ImpaginazioneIpc,
    /// Le parti che entrano, escono o volano con una transizione di vista.
    ///
    /// La finestra avvolge quelle parti in `<ViewTransition>` e dà a ogni
    /// innesco la classe che il foglio aspetta. Un innesco che la skin non
    /// dichiara resta `none`: senza una regola che lo raccolga, il motore
    /// farebbe la sua dissolvenza incrociata di serie, che nessuno ha chiesto.
    pub presenze: Vec<PresenzaIpc>,
    /// La skin che si sarebbe dovuta indossare e non si è potuta leggere.
    ///
    /// `None` quasi sempre: c'è solo quando [`skin`] ha ripiegato su quella di
    /// serie perché la scelta non si compilava. La scelta **non** viene
    /// riscritta nel database — un pacchetto su un disco staccato torna
    /// leggibile da sé, e cambiare la preferenza al posto di chi l'ha espressa
    /// vorrebbe dire perderla per sempre a causa di un'assenza temporanea —
    /// quindi questo campo è l'unico modo che la finestra ha di sapere che
    /// quel che vede non è quel che ha chiesto, e di dirlo.
    pub ripiego: Option<String>,
    /// Perché la skin di [`Self::ripiego`] non si è indossata: il codice del
    /// guasto. `skin.notFound` è il pacchetto che non c'è, e torna da sé se il
    /// pacchetto torna; ogni altro codice è un pacchetto che c'è e non si legge
    /// più, e non torna da sé. Fino alla 3.2.0 la finestra diceva sempre la
    /// prima cosa (R06b-4).
    pub ripiego_guasto: Option<String>,
}

/// Una parte con una transizione di vista, e quali inneschi dichiara.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenzaIpc {
    /// Il nome della parte, che è anche il pezzo centrale della classe:
    /// `vt-<parte>-enter`.
    pub parte: &'static str,
    /// Ha un'entrata.
    pub enter: bool,
    /// Ha un'uscita.
    pub exit: bool,
    /// Vola fra due schermate.
    pub share: bool,
}

/// Una skin disponibile, per il selettore.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceSkin {
    /// L'identificatore.
    pub id: String,
    /// Il nome mostrato.
    pub nome: String,
    /// Chi l'ha fatta.
    pub autore: String,
    /// La versione dichiarata nel manifest, o vuota se non si legge.
    ///
    /// Serve alla domanda che precede una sostituzione: due skin con lo stesso
    /// nome si distinguono solo così. Vedi [`SostituzioneIpc`].
    pub versione: String,
    /// A cosa somiglia.
    pub descrizione: Option<String>,
    /// È quella compilata dentro l'applicazione: non si può disinstallare.
    pub di_serie: bool,
    /// È quella attiva adesso.
    pub attiva: bool,
    /// I tre colori della scheda, scritti dall'autore.
    ///
    /// Vuoto quando la skin non li dichiara — e allora chi disegna la scheda
    /// deve indovinare, il che è precisamente il motivo per cui il campo esiste.
    pub anteprima: Vec<String>,
    /// Il tema di base: `dark` o `light`.
    pub scheme: &'static str,
    /// Ha la variante del tema opposto.
    pub variante: bool,
    /// Quel che chi l'ha appena installata deve sapere prima di indossarla.
    ///
    /// C'è solo nella risposta di un'installazione: l'elenco non lo calcola,
    /// perché lì la skin è già stata scelta o lasciata stare. Vedi
    /// [`EsameIpc`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub esame: Option<EsameIpc>,
    /// Il codice del guasto di una skin installata che non passa più la
    /// validazione: da aprire nello Studio, non da indossare.
    ///
    /// # Perché c'è una voce per una skin rotta
    ///
    /// Fino alla 3.0 una skin così spariva dall'elenco in silenzio: nessun
    /// avviso, nessun modo di aprirla per aggiustarla — lo Studio stesso non la
    /// leggeva — e nessun modo di toglierla, se non cancellando un file dalla
    /// cartella dei dati. Adesso resta, con quel che non va, e le due strade
    /// che ha: lo Studio o la disinstallazione. Un archivio che non si apre
    /// nemmeno resta fuori: lì non c'è un testo da aggiustare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guasto: Option<String>,
    /// La skin che un'installazione ha sostituito, tenuta da parte: da qui la
    /// si rimette. Assente se non ce n'è. Vedi [`installa_file`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sostituita: Option<SostituitaIpc>,
}

impl VoceSkin {
    /// La voce di un documento letto.
    fn di(documento: rymembra_skin::SkinDocument, di_serie: bool, attiva: bool) -> Self {
        Self {
            anteprima: anteprima(&documento.meta),
            scheme: documento.scheme.as_str(),
            variante: documento.variant.is_some(),
            id: documento.id,
            nome: documento.meta.name,
            autore: documento.meta.author,
            versione: documento.meta.version,
            descrizione: documento.meta.description,
            di_serie,
            attiva,
            esame: None,
            guasto: None,
            sostituita: None,
        }
    }

    /// La voce di una skin installata il cui manifest non passa la validazione.
    ///
    /// L'id è quello del file — `installa` lo scrive col nome dell'id, quindi
    /// è lo stesso con cui lo Studio la riapre — e nome e autore si prendono
    /// dal testo quando ci sono, come stringhe e basta: il resto del documento
    /// è proprio quel che non si sa leggere.
    fn rotta(id: String, sorgente: &str, guasto: &AppError, attiva: bool) -> Self {
        let grezzo: Option<serde_json::Value> = serde_json::from_str(sorgente).ok();
        let meta = |campo: &str| {
            grezzo
                .as_ref()
                .and_then(|v| v.get("meta")?.get(campo)?.as_str().map(ToOwned::to_owned))
        };
        Self {
            nome: meta("name").unwrap_or_else(|| id.clone()),
            autore: meta("author").unwrap_or_default(),
            versione: meta("version").unwrap_or_default(),
            descrizione: None,
            anteprima: Vec::new(),
            scheme: rymembra_skin::Scheme::Dark.as_str(),
            variante: false,
            di_serie: false,
            attiva,
            esame: None,
            guasto: Some(guasto.code().kind().code().to_owned()),
            sostituita: None,
            id,
        }
    }
}

/// Una skin come la si riconosce in una domanda: il nome, la versione e chi
/// l'ha fatta. Due skin con lo stesso id si distinguono solo così.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VersioneIpc {
    /// Il nome mostrato.
    pub nome: String,
    /// La versione, o vuota se il manifest non la dice.
    pub versione: String,
    /// Chi l'ha fatta, o vuoto.
    pub autore: String,
}

impl VersioneIpc {
    /// Quella di un pacchetto, letta dal solo manifest e senza validarlo: la
    /// skin installata può non passare più la validazione, e resta comunque
    /// quella che si sta per sostituire. Senza un manifest che si legga, l'id.
    fn del_pacchetto(id: &str, bytes: &[u8]) -> Self {
        let grezzo: Option<serde_json::Value> = rymembra_skin::package::leggi_manifesto(bytes)
            .ok()
            .and_then(|manifesto| serde_json::from_str(&manifesto.source).ok());
        let meta = |campo: &str| {
            grezzo
                .as_ref()
                .and_then(|v| v.get("meta")?.get(campo)?.as_str().map(ToOwned::to_owned))
        };
        Self {
            nome: meta("name").unwrap_or_else(|| id.to_owned()),
            versione: meta("version").unwrap_or_default(),
            autore: meta("author").unwrap_or_default(),
        }
    }

    fn della_voce(voce: &VoceSkin) -> Self {
        Self {
            nome: voce.nome.clone(),
            versione: voce.versione.clone(),
            autore: voce.autore.clone(),
        }
    }
}

/// La copia della skin che un'installazione ha sostituito.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SostituitaIpc {
    /// Com'era.
    #[serde(flatten)]
    pub skin: VersioneIpc,
    /// Il file della copia: reinstallarlo (`skin_installa` con `sostituisci`)
    /// la rimette al suo posto, e tiene da parte quella di adesso.
    pub percorso: String,
}

/// Una sostituzione da confermare: la skin installata con questo id, e quella
/// che prenderebbe il suo posto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SostituzioneIpc {
    /// L'id che le due hanno in comune.
    pub id: String,
    /// Quella installata adesso.
    pub presente: VersioneIpc,
    /// Quella che arriva.
    pub nuova: VersioneIpc,
    /// L'impronta di quella installata adesso, cioè di quella che la domanda
    /// mostra. Il sì la riporta (`skin_installa` con `attesa`): se intanto è
    /// cambiata, si chiede di nuovo invece di sostituire una skin che nessuno ha
    /// visto (R04b-5).
    pub impronta: String,
}

/// Quel che risponde l'installazione di un file `.ryskin`.
///
/// Delle due, una: o la skin è installata, o c'è già una skin diversa con lo
/// stesso id e prima di sostituirla si chiede — e allora non è stato scritto
/// niente. Vedi [`installa_file`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallazioneIpc {
    /// La skin installata, con il suo esame.
    pub installata: Option<VoceSkin>,
    /// La skin che l'installazione sostituirebbe.
    pub da_sostituire: Option<SostituzioneIpc>,
    /// L'impronta del pacchetto appena installato, quando c'è. È quella che
    /// «Rimettila» porta come `attesa`: un secondo clic trova installata
    /// un'altra skin, e non scambia di nuovo (R04b-4).
    pub impronta: Option<String>,
}

/// Un costo e il budget su cui si misura.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MisuraIpc {
    /// Quanto costa.
    pub costo: u32,
    /// Quanto dovrebbe costare al più.
    pub budget: u32,
}

impl From<rymembra_skin::document::Misura> for MisuraIpc {
    fn from(misura: rymembra_skin::document::Misura) -> Self {
        Self {
            costo: misura.costo,
            budget: misura.budget,
        }
    }
}

/// Un ciclo che dura quanto lo stato che lo accende.
#[derive(Debug, Clone, Serialize)]
pub struct CicloIpc {
    /// La parte che gira.
    pub parte: &'static str,
    /// Lo stato che lo accende: `playing`, `hover`, …
    pub stato: &'static str,
    /// Il nome dell'animazione.
    pub animazione: String,
}

/// Quel che chi installa una skin deve sapere prima di indossarla.
///
/// # Perché arriva con l'installazione
///
/// Una skin si scambia, e chi ne installa una arrivata da un altro non sa cosa
/// si sta mettendo addosso finché non l'ha addosso: una finestra che scatta,
/// un testo grigio su grigio. Fino alla 2.4 installare voleva dire indossare,
/// subito. Ora la risposta porta l'esame del nucleo — i budget, i cicli, i
/// contrasti — e se [`EsameIpc::da_confermare`] è vero la finestra chiede prima
/// di indossarla. Che cosa meriti una domanda lo decide il nucleo
/// (`rymembra_skin::document::Esame::da_confermare`), non l'interfaccia.
///
/// Quel che sfora **più del doppio** di un budget non arriva qui: non si
/// installa affatto (`controlla_installazione`), e le skin che lampeggiano non
/// passano nemmeno la validazione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsameIpc {
    /// La superficie più cara, sul budget di una superficie.
    pub superficie: MisuraIpc,
    /// La parte che si muove di più, sul budget di una parte.
    pub movimento_parte: MisuraIpc,
    /// Il movimento di tutta la finestra, sul suo budget.
    pub movimento: MisuraIpc,
    /// Lo scafale, sul suo budget.
    pub scafale: MisuraIpc,
    /// Si muove.
    pub anima: bool,
    /// Quel che gira per sempre, finché dura lo stato che lo accende.
    pub cicli: Vec<CicloIpc>,
    /// Quante coppie di testo e superficie non si leggono.
    pub contrasti: usize,
    /// Quanti anelli del fuoco non si vedono: sotto 3:1 contro le superfici,
    /// tema per tema (`rymembra_skin::focus_ring_pairs`), o larghi zero.
    pub anelli: usize,
    /// Quanti token di testo obbligatori la skin non dichiara.
    pub testi_mancanti: usize,
    /// Le parti essenziali trasparenti o ritagliate, coi percorsi.
    pub nascoste: Vec<String>,
    /// Le animazioni che fanno apparire e sparire una parte troppo in fretta.
    pub lampi: Vec<String>,
    /// C'è qualcosa da chiedere prima di indossarla.
    pub da_confermare: bool,
}

impl From<rymembra_skin::document::Esame> for EsameIpc {
    fn from(esame: rymembra_skin::document::Esame) -> Self {
        Self {
            da_confermare: esame.da_confermare(),
            superficie: esame.superficie.into(),
            movimento_parte: esame.movimento_parte.into(),
            movimento: esame.movimento.into(),
            scafale: esame.scafale.into(),
            anima: esame.anima,
            cicli: esame
                .cicli
                .into_iter()
                .map(|c| CicloIpc {
                    parte: c.parte,
                    stato: c.stato,
                    animazione: c.animazione,
                })
                .collect(),
            contrasti: esame.contrasti,
            anelli: esame.anelli,
            testi_mancanti: esame.testi_mancanti,
            nascoste: esame.nascoste,
            lampi: esame.lampi,
        }
    }
}

/// I tre colori della scheda, nella scrittura che il CSS accetta.
fn anteprima(meta: &rymembra_skin::document::SkinMeta) -> Vec<String> {
    meta.preview
        .map(|colori| {
            colori
                .iter()
                .map(|c| rymembra_skin::values::format_color(*c))
                .collect()
        })
        .unwrap_or_default()
}

/// Compila la skin di un identificatore.
///
/// Separata dal comando perché il comando ha bisogno dello stato di Tauri e
/// questa no: è la parte che si prova come una chiamata di funzione, ed è tutta
/// la parte che decide qualcosa.
fn compila(data_dir: &Path, id: &str) -> Result<SkinIpc, AppError> {
    let sorgente = sorgente(data_dir, id)?;
    let documento = rymembra_skin::parse_skin_json(&sorgente)?;
    Ok(skin_ipc(
        &documento,
        &rymembra_skin::Ambito::app(&documento.id),
    ))
}

/// Una skin letta, compilata nell'ambito dato e pronta per la finestra.
///
/// Due chiamanti: la skin scelta, nell'ambito della finestra, e la prova di
/// una bozza dello Studio, con le risorse della bozza (`studio_compila_app`).
pub fn skin_ipc(
    documento: &rymembra_skin::SkinDocument,
    ambito: &rymembra_skin::Ambito,
) -> SkinIpc {
    let compilata = rymembra_skin::compile::compila(documento, ambito);
    SkinIpc {
        id: compilata.id,
        css: compilata.css,
        cost: compilata.cost,
        dynamic_tokens: compilata
            .dynamic_tokens
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        scheme: documento.scheme.as_str(),
        variante: documento.variant.is_some(),
        dynamic_accent: documento.capabilities.dynamic_accent,
        layout: impaginazione(documento),
        presenze: compilata
            .presenze
            .iter()
            .map(|p| PresenzaIpc {
                parte: p.parte,
                enter: p.enter,
                exit: p.exit,
                share: p.share,
            })
            .collect(),
        // Compilare è riuscito, quindi non si è ripiegato su niente: lo mette
        // chi ha dovuto ripiegare, che è l'unico a saperlo.
        ripiego: None,
        ripiego_guasto: None,
    }
}

/// L'impaginazione dichiarata, o i difetti del formato quando la skin tace.
///
/// I difetti vengono da `SkinLayout::default()` e da `MotionIntensity::Full`,
/// cioè dal formato: ripeterli qui vorrebbe dire avere due posti in cui è
/// scritto cosa succede a chi non dichiara niente.
pub fn impaginazione(documento: &rymembra_skin::SkinDocument) -> ImpaginazioneIpc {
    use rymembra_skin::document::{Density, SkinLayout};

    // Si prende in prestito invece di copiare: da quando `SkinLayout` porta lo
    // scafale non è più `Copy`, e copiare un albero per leggerne tre enum
    // sarebbe pagare la struttura senza usarla.
    let difetto = SkinLayout::default();
    let layout = documento.layout.as_ref().unwrap_or(&difetto);
    ImpaginazioneIpc {
        density: match layout.density {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
            Density::Spacious => "spacious",
        },
        motion: documento
            .motion
            .as_ref()
            .map_or(rymembra_skin::document::MotionIntensity::Full, |m| {
                m.intensity
            })
            .as_str(),
        shell: nodo(&layout.shell, &mut Vec::new()),
    }
}

/// Legge l'identificatore della skin scelta.
///
/// Anche un guasto del database vale «quella di serie», ed è il motivo per cui
/// questa funzione non restituisce un `Result`: la skin è ciò che serve a
/// disegnare la finestra, e una finestra che non si disegna non può mostrare a
/// nessuno perché non si è disegnata.
fn skin_scelta(connection: &rusqlite::Connection) -> String {
    rymembra_app::settings::read(connection, CHIAVE_SKIN)
        .ok()
        .flatten()
        // Un valore assente o illeggibile vale «quella di serie»: una skin
        // scelta e poi cancellata a mano dal disco non deve impedire l'avvio.
        .filter(|id| di_serie(id) || nome_installato(id))
        .unwrap_or_else(|| DI_SERIE.to_owned())
}

/// Compila una skin.
///
/// Senza `id` compila **quella scelta**, non quella di serie: è ciò che rende la
/// scelta persistente senza che la finestra debba ricordarsela e rimandarla
/// indietro a ogni avvio.
///
/// # La finestra nera
///
/// Senza `id` questa chiamata **non fallisce** per colpa della skin scelta: se
/// non si compila si ripiega su quella di serie, e `ripiego` dice quale è
/// rimasta indietro.
///
/// Il difetto era questo. L'impaginazione dell'intera finestra viene da qui —
/// `layout.shell` è l'albero, e il renderer senza albero non disegna niente —
/// quindi un `.ryskin` che non si legge non faceva una finestra brutta: ne
/// faceva una **vuota**, con i soli tre tasti di sistema, per sempre, senza un
/// messaggio e senza una via d'uscita, perché anche le Impostazioni stanno
/// dentro l'impaginazione. Il nucleo intanto rispondeva a tutto.
///
/// Ci si arriva cancellando un pacchetto a mano, spostando la cartella dei
/// dati, o riaprendo su un'altra macchina un profilo che porta con sé la scelta
/// ma non il file. Il caso era già noto in due punti — `skin_disinstalla`
/// riporta alla skin di serie per non lasciare «una finestra senza foglio al
/// riavvio», e `backup::applica` scrive `skin.active` solo dopo che i file sono
/// arrivati — ma erano due guardie su chi **scrive**. Chi legge non ne aveva
/// nessuna, e bastava un file in meno per scavalcarle tutte e due.
///
/// La ricetta è quella che `skin_elenco` applica da sempre a sé stesso: quel
/// che non si legge sparisce, e non si porta dietro tutto il resto.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore **chiesto per nome** non è di nessuna
/// skin conosciuta; i codici della validazione se il manifest non è valido —
/// cosa che per una skin di serie sarebbe un guasto nostro, e che il test di
/// fedeltà del nucleo scopre prima di qui. Chiedere per nome vuol dire il
/// selettore, e a chi sta scegliendo una skin si deve dire che quella è rotta;
/// all'avvio, invece, l'unica risposta utile è una finestra che si disegna.
///
/// # Il lucchetto, solo per la domanda al database
///
/// Quale skin è scelta lo sa il database; leggere il pacchetto e compilarlo è
/// disco e processore, e fino alla 3.0 si faceva col lucchetto della libreria
/// in mano — lo stesso di ogni comando e della scansione. L'anteprima al
/// passaggio del puntatore sulle schede delle skin ne chiedeva una a ogni
/// scheda. Adesso il lucchetto serve alla sola domanda, e solo se nessuno ha
/// detto per nome quale skin vuole.
#[tauri::command]
pub async fn skin(app: tauri::AppHandle, id: Option<String>) -> Esito<SkinIpc> {
    comanda("skin", move || {
        let stato = stato_di::<Stato>(&app)?;
        let (dati, scelta) = match id.as_deref() {
            Some(chiesta) => (crate::studio::cartella_dati(&stato), chiesta.to_owned()),
            None => {
                let scelta = con_libreria(&stato, |libreria| {
                    Ok((libreria.data_dir.clone(), skin_scelta(&libreria.connection)))
                });
                return da_indossare(scelta).map_err(errore);
            }
        };
        dati.and_then(|dati| compila_o_ripiega(&dati, id.as_deref(), &scelta))
            .map_err(errore)
    })
    .await
}

/// La skin da indossare quando nessuno l'ha chiesta per nome: quella scelta,
/// letta dalla libreria (cartella dati e identificatore), col suo ripiego.
///
/// # Quando la libreria non c'è
///
/// Una libreria che non si è aperta — una migrazione fermata a metà, un
/// database scritto da una versione più nuova, un file rotto, un permesso
/// negato — non dice quale skin è scelta, e fino alla 3.2.0 il suo errore
/// usciva di qui intero. Senza skin non c'è impaginazione, e senza impaginazione
/// la finestra restava vuota: niente fascia d'errore, niente Impostazioni, e
/// davanti a una finestra vuota il gesto naturale è reinstallare o cancellare la
/// cartella dei dati, cioè proprio quelli che fanno perdere la libreria.
///
/// Si indossa quella di serie, che viene dal binario e non ha bisogno di niente
/// sul disco. Il perché lo dice la fascia, con l'errore di `avvio`, che è lo
/// stesso della libreria. `ripiego` resta vuoto: non è la skin scelta a non
/// compilarsi, e la nota delle Impostazioni direbbe una cosa falsa.
fn da_indossare(scelta: Result<(PathBuf, String), AppError>) -> Result<SkinIpc, AppError> {
    match scelta {
        Ok((dati, scelta)) => compila_o_ripiega(&dati, None, &scelta),
        Err(guasto) => {
            crate::nota!(
                "[skin] libreria non aperta ({}): si indossa quella di serie",
                guasto.code().kind().code()
            );
            // La cartella non serve: quella di serie non tocca il disco.
            compila(Path::new(""), DI_SERIE)
        }
    }
}

/// La skin da indossare, col ripiego già deciso. Vedi [`skin`].
///
/// Fuori dal comando perché è la regola che tiene in piedi la finestra, e una
/// regola del genere va provata senza uno `Stato` di Tauri attorno.
fn compila_o_ripiega(
    data_dir: &Path,
    chiesta: Option<&str>,
    scelta: &str,
) -> Result<SkinIpc, AppError> {
    match compila(data_dir, scelta) {
        Ok(skin) => Ok(skin),
        Err(guasto) if chiesta.is_none() && scelta != DI_SERIE => {
            // Se non si compila nemmeno quella di serie il guasto è nostro e va
            // detto: quella non viene da un file, viene dal binario, e se è
            // rotta lei non c'è nessun ripiego da offrire.
            let mut serie = compila(data_dir, DI_SERIE)?;
            crate::nota!(
                "[skin] «{scelta}» non si compila ({}): si ripiega su quella di serie",
                guasto.code().kind().code()
            );
            serie.ripiego = Some(scelta.to_owned());
            serie.ripiego_guasto = Some(guasto.code().kind().code().to_owned());
            Ok(serie)
        }
        Err(guasto) => Err(guasto),
    }
}

/// Le skin disponibili: quelle di serie più quelle installate.
///
/// Una skin installata il cui archivio non si apre **non fa fallire
/// l'elenco**: sparisce da sola. Un selettore che non si apre perché uno dei
/// suoi elementi è rotto è il modo di rendere irrecuperabile un guasto che
/// riguardava una skin sola. Una il cui manifest si legge e non passa la
/// validazione invece resta, col suo guasto: vedi [`VoceSkin::guasto`].
///
/// Dei pacchetti si legge il **solo** manifest
/// (`rymembra_skin::package::leggi_manifesto`), e fuori dal lucchetto della
/// libreria: al database serve una riga sola, quale skin è scelta. Fino alla
/// 3.0 ogni apertura delle Impostazioni decomprimeva ogni carattere e ogni
/// immagine di ogni skin installata, col lucchetto in mano.
#[tauri::command]
pub async fn skin_elenco(app: tauri::AppHandle) -> Esito<Vec<VoceSkin>> {
    comanda("elenco delle skin", move || {
        let stato = stato_di::<Stato>(&app)?;
        let (dati, scelta) = con_libreria(&stato, |libreria| {
            Ok((libreria.data_dir.clone(), skin_scelta(&libreria.connection)))
        })
        .map_err(errore)?;
        elenco(&dati, &scelta).map_err(errore)
    })
    .await
}

/// Quel che fa [`skin_elenco`], senza lo stato di Tauri attorno.
fn elenco(data_dir: &Path, scelta: &str) -> Result<Vec<VoceSkin>, AppError> {
    let mut voci = Vec::new();
    for (id, testo) in rymembra_skin::DI_SERIE {
        let documento = rymembra_skin::parse_skin_json(testo)?;
        voci.push(VoceSkin::di(documento, true, scelta == *id));
    }

    let dir = cartella_skin(data_dir);
    let mut installate: Vec<VoceSkin> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|voce| voce.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ryskin"))
        .filter_map(|p| {
            let nome_file = p.file_stem()?.to_str()?.to_owned();
            let bytes = std::fs::read(&p).ok()?;
            // Un archivio che non si apre non ha niente da mostrare né da
            // aggiustare: resta fuori, come sempre.
            let manifesto = rymembra_skin::package::leggi_manifesto(&bytes).ok()?;
            let letto = rymembra_skin::parse_skin_json(&manifesto.source).and_then(|documento| {
                rymembra_skin::package::risorse_presenti(&documento, &manifesto.risorse)?;
                Ok(documento)
            });
            match letto {
                // Un pacchetto con l'id di una skin di serie non si legge mai:
                // mostrarlo sarebbe una seconda voce che indossa la prima.
                Ok(documento) if di_serie(&documento.id) => None,
                Ok(documento) => {
                    let attiva = scelta == documento.id;
                    Some(VoceSkin::di(documento, false, attiva))
                }
                // Un nome di dispositivo di Windows invece resta, rotto: c'era
                // prima che il formato lo rifiutasse, e da qui lo si toglie.
                Err(_) if di_serie(&nome_file) || !nome_installato(&nome_file) => None,
                Err(guasto) => {
                    let attiva = scelta == nome_file;
                    Some(VoceSkin::rotta(
                        nome_file,
                        &manifesto.source,
                        &guasto,
                        attiva,
                    ))
                }
            }
        })
        .collect();
    installate.sort_by(|a, b| a.nome.cmp(&b.nome));
    // La copia dell'ultima sostituita, per rimetterla: vedi `installa_file`.
    for voce in &mut installate {
        voce.sostituita = sostituita_di(data_dir, &voce.id);
    }
    voci.append(&mut installate);
    Ok(voci)
}

/// Mette un pacchetto nella cartella delle skin.
///
/// Il pacchetto si legge **prima** di copiarlo: un archivio che non passa le
/// guardie non deve arrivare nella cartella delle skin, altrimenti resta lì a
/// far fallire ogni elenco successivo. Il nome del file lo decide
/// l'identificatore dichiarato dentro il manifest, non quello che aveva fuori.
///
/// Separata dai due comandi che la chiamano perché la provenienza dei byte —
/// un file scelto dall'utente, o un manifest appena impacchettato dallo Studio
/// — non cambia niente di quel che va controllato: una skin fatta in casa entra
/// dalla stessa porta di una scaricata da internet.
///
/// # I budget, e l'esame
///
/// Una skin valida che sfora un budget più del doppio si ferma qui, prima di
/// arrivare nella cartella: nello Studio resta un avviso — chi la sta facendo
/// la deve poter aprire, provare e aggiustare — ma chi la installa non ha modo
/// di sapere che la finestra scatterà a ogni fotogramma finché non scatta. Il
/// criterio è del nucleo (`controlla_installazione`), come tutto il resto.
///
/// Quel che si installa torna con il suo esame: vedi [`EsameIpc`].
///
/// Il file si scrive intero o niente (`scrivi_atomico`): una skin installata
/// è la prima cosa che l'applicazione rilegge all'avvio fidandosi, e un
/// archivio troncato a metà dall'uscita era una finestra che ripiegava sulla
/// skin di serie senza sapere perché.
pub(crate) fn installa(data_dir: &Path, bytes: &[u8]) -> Result<VoceSkin, AppError> {
    let documento = da_installare(bytes)?;
    posa(data_dir, documento, bytes)
}

/// Il documento di un pacchetto che si può installare: le guardie, l'id e i
/// budget. Non scrive niente.
fn da_installare(bytes: &[u8]) -> Result<rymembra_skin::SkinDocument, AppError> {
    let pacchetto = rymembra_skin::read_skin_package(bytes)?;
    let documento = pacchetto.document;
    if di_serie(&documento.id) || !id_sicuro(&documento.id) {
        return Err(AppError::new(ErrorCode::SkinNotFound {
            id: documento.id.clone(),
        })
        .with_cause(
            "l'identificatore è di una skin di serie, o non è utilizzabile come nome di file"
                .to_owned(),
        ));
    }
    rymembra_skin::document::controlla_installazione(&documento)?;
    Ok(documento)
}

/// Scrive il pacchetto al posto suo, e risponde con la voce e il suo esame.
fn posa(
    data_dir: &Path,
    documento: rymembra_skin::SkinDocument,
    bytes: &[u8],
) -> Result<VoceSkin, AppError> {
    let esame = EsameIpc::from(rymembra_skin::document::esamina(&documento));
    let dir = cartella_skin(data_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|err| rymembra_app::files::io_error(&dir.display().to_string(), &err))?;
    rymembra_app::scrittura::scrivi_atomico(&dir.join(format!("{}.ryskin", documento.id)), bytes)?;
    let sostituita = sostituita_di(data_dir, &documento.id);
    Ok(VoceSkin {
        esame: Some(esame),
        sostituita,
        ..VoceSkin::di(documento, false, false)
    })
}

/// Dove stanno le copie delle skin sostituite.
///
/// Dentro la cartella delle skin e non accanto ai pacchetti: l'elenco, il
/// backup e il profilo leggono solo i `.ryskin` della cartella, quindi una
/// copia non diventa una seconda skin con lo stesso id, non va su Drive e non
/// entra in un `.ryprofile`. È di questo computer, come un cestino.
fn cartella_sostituite(data_dir: &Path) -> PathBuf {
    cartella_skin(data_dir).join("sostituite")
}

/// Quante copie si tengono per id.
///
/// Fino alla 3.2.0 era una: la seconda sostituzione della stessa skin
/// cancellava la prima per sempre, miniatura compresa, e la domanda non lo
/// diceva (R04b-5). Tre bastano a tornare indietro di qualche passo senza far
/// crescere la cartella con ogni versione mai provata (D-52).
const COPIE_PER_ID: usize = 3;

/// Le copie di questo id, una per file: `sostituite/<id>/<ms>-<impronta>.ryskin`.
fn cartella_copie(data_dir: &Path, id: &str) -> PathBuf {
    cartella_sostituite(data_dir).join(id)
}

/// La copia unica della 3.2.0, `sostituite/<id>.ryskin`: si legge come la più
/// vecchia delle copie, e se ne va quando le copie sono più di tre.
fn copia_di_prima(data_dir: &Path, id: &str) -> PathBuf {
    cartella_sostituite(data_dir).join(format!("{id}.ryskin"))
}

/// Le copie tenute da parte per questo id, dalla più recente.
///
/// Il nome comincia coi millisecondi scritti a venti cifre, quindi l'ordine dei
/// nomi è l'ordine del tempo e non serve leggere i metadati del disco.
fn copie(data_dir: &Path, id: &str) -> Vec<PathBuf> {
    let mut nuove: Vec<PathBuf> = std::fs::read_dir(cartella_copie(data_dir, id))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|voce| voce.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ryskin"))
        .collect();
    nuove.sort();
    nuove.reverse();
    let vecchia = copia_di_prima(data_dir, id);
    if vecchia.is_file() {
        nuove.push(vecchia);
    }
    nuove
}

/// I byte di una copia, al più quanti ne ha un pacchetto.
fn leggi_copia(file: &Path) -> Option<Vec<u8>> {
    leggi_al_piu(std::fs::File::open(file).ok()?, &file.display().to_string()).ok()
}

/// L'ultima copia tenuta da parte per questo id, se c'è.
fn sostituita_di(data_dir: &Path, id: &str) -> Option<SostituitaIpc> {
    copie(data_dir, id).into_iter().find_map(|file| {
        let bytes = leggi_copia(&file)?;
        Some(SostituitaIpc {
            skin: VersioneIpc::del_pacchetto(id, &bytes),
            percorso: file.display().to_string(),
        })
    })
}

/// Mette da parte il pacchetto che sta per essere sostituito, e tiene le
/// ultime [`COPIE_PER_ID`].
///
/// Gli stessi byte già da parte non si copiano due volte: due skin che si
/// scambiano il posto avanti e indietro non devono riempire le tre copie della
/// stessa versione.
fn metti_da_parte(data_dir: &Path, id: &str, bytes: &[u8]) -> Result<(), AppError> {
    let gia = copie(data_dir, id)
        .iter()
        .any(|file| leggi_copia(file).is_some_and(|c| c == bytes));
    if !gia {
        let cartella = cartella_copie(data_dir, id);
        std::fs::create_dir_all(&cartella)
            .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;
        let adesso = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let impronta = rymembra_app::backup::impronta(bytes);
        let breve = impronta.get(..16).unwrap_or(&impronta);
        rymembra_app::scrittura::scrivi_atomico(
            &cartella.join(format!("{adesso:020}-{breve}.ryskin")),
            bytes,
        )?;
    }
    for di_troppo in copie(data_dir, id).into_iter().skip(COPIE_PER_ID) {
        if let Err(err) = std::fs::remove_file(&di_troppo) {
            crate::nota!(
                "[skin] una copia vecchia di «{id}» non si toglie ({}): resta",
                err.kind()
            );
        }
    }
    Ok(())
}

/// Toglie dalle copie quella che è appena tornata installata: è la skin di
/// adesso, e tenerla anche da parte la mostrerebbe due volte.
fn togli_dalle_copie(data_dir: &Path, id: &str, bytes: &[u8]) {
    for file in copie(data_dir, id) {
        if leggi_copia(&file).is_some_and(|c| c == bytes) {
            let _ = std::fs::remove_file(&file);
        }
    }
}

/// Installa un pacchetto scelto dall'utente: dal selettore, o trascinato sulla
/// finestra.
///
/// # Una skin con lo stesso id non si sostituisce senza chiedere
///
/// Fino alla 3.2 il pacchetto prendeva il posto di quello installato con lo
/// stesso id, in silenzio, e il backup portava la sostituzione su Drive: la
/// skin fatta nello Studio, coi suoi caratteri e la sua miniatura, se ne
/// andava per un `notte-1.0.0.ryskin` di qualcun altro trascinato sulla
/// finestra. Adesso, se c'è già una skin **diversa** con quell'id:
///
/// - senza `sostituisci` non si scrive niente, e la risposta porta le due —
///   nome, versione, autore — e l'impronta di quella installata, perché la
///   finestra chieda;
/// - con `sostituisci` quella di prima va fra le copie ([`metti_da_parte`],
///   le ultime tre), e da lì si rimette reinstallandola: le due si scambiano
///   il posto.
///
/// # Il sì vale per quel che si è visto
///
/// `attesa` è l'impronta della skin installata che il gesto vuole togliere:
/// quella che la domanda ha mostrato, o quella che «Rimettila» sa di aver
/// messo al suo posto. Se adesso ce n'è un'altra — un secondo clic sulla
/// stessa notizia, un altro file arrivato fra la domanda e il sì — non si
/// scrive niente e la risposta è una domanda nuova, con la skin che c'è
/// davvero (R04b-4, R04b-5).
///
/// Lo stesso pacchetto un'altra volta non è una sostituzione, e non chiede.
/// Le skin di serie non si sostituiscono mai: le ferma [`da_installare`].
///
/// [`installa`] resta la strada dello Studio, dove la bozza che porta già
/// quell'id è la skin stessa che si aggiorna (vedi [`installa_sorgente`]).
fn installa_file(
    data_dir: &Path,
    bytes: &[u8],
    sostituisci: bool,
    attesa: Option<&str>,
) -> Result<InstallazioneIpc, AppError> {
    let documento = da_installare(bytes)?;
    let file = cartella_skin(data_dir).join(format!("{}.ryskin", documento.id));
    let presente = match std::fs::File::open(&file) {
        Ok(aperto) => Some(leggi_al_piu(aperto, &file.display().to_string())?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(rymembra_app::files::io_error(
                &file.display().to_string(),
                &err,
            ));
        }
    };
    let impronta = rymembra_app::backup::impronta(bytes);
    let Some(presente) = presente.filter(|prima| prima.as_slice() != bytes) else {
        return Ok(InstallazioneIpc {
            installata: Some(posa(data_dir, documento, bytes)?),
            da_sostituire: None,
            impronta: Some(impronta),
        });
    };
    let di_adesso = rymembra_app::backup::impronta(&presente);
    let visto = attesa.is_none_or(|attesa| attesa == di_adesso);
    if !sostituisci || !visto {
        let nuova = VoceSkin::di(documento, false, false);
        return Ok(InstallazioneIpc {
            installata: None,
            da_sostituire: Some(SostituzioneIpc {
                presente: VersioneIpc::del_pacchetto(&nuova.id, &presente),
                nuova: VersioneIpc::della_voce(&nuova),
                id: nuova.id,
                impronta: di_adesso,
            }),
            impronta: None,
        });
    }
    // La copia prima del pacchetto nuovo: se la scrittura della copia non
    // riesce, quella di prima resta dov'era.
    let id = documento.id.clone();
    metti_da_parte(data_dir, &id, &presente)?;
    let installata = posa(data_dir, documento, bytes)?;
    togli_dalle_copie(data_dir, &id, bytes);
    Ok(InstallazioneIpc {
        installata: Some(VoceSkin {
            sostituita: sostituita_di(data_dir, &id),
            ..installata
        }),
        da_sostituire: None,
        impronta: Some(impronta),
    })
}

/// Installa una skin da un file `.ryskin`.
///
/// `attesa` è l'impronta della skin installata che il gesto vuole togliere:
/// vedi [`installa_file`].
///
/// In disparte, sul pool bloccante: legge un file che l'utente ha scelto — da
/// una chiavetta, da una share che si sveglia — e ne scrive un altro. Un
/// `(async)` su una funzione sincrona faceva aspettare quel disco a un *worker*
/// del runtime, togliendo il posto a ogni altro comando asincrono (R10-5).
#[tauri::command]
pub async fn skin_installa(
    app: tauri::AppHandle,
    percorso: String,
    sostituisci: Option<bool>,
    attesa: Option<String>,
) -> Esito<InstallazioneIpc> {
    crate::disparte::in_disparte("installazione di una skin", move || {
        installa_adesso(&app, &percorso, sostituisci, attesa)
    })
    .await
    .map_err(errore)?
}

/// Il corpo di [`skin_installa`], sul pool bloccante.
fn installa_adesso(
    app: &tauri::AppHandle,
    percorso: &str,
    sostituisci: Option<bool>,
    attesa: Option<String>,
) -> Esito<InstallazioneIpc> {
    // Senza il lucchetto della libreria: installare è leggere un file e
    // scriverne un altro, e il database non c'entra.
    let esito = crate::importa::gestito::<Stato>(app, "installazione di una skin")
        .and_then(|stato| crate::studio::cartella_dati(&stato))
        .and_then(|dati| {
            let file = std::fs::File::open(percorso)
                .map_err(|err| rymembra_app::files::io_error(percorso, &err))?;
            // La via breve: quel che il disco dice del file, prima di leggerne
            // un byte. `leggi_al_piu` resta il tetto vero, perché un file può
            // crescere fra la domanda e la lettura.
            if let Some(bytes) = file
                .metadata()
                .ok()
                .map(|m| m.len())
                .filter(|&n| n > MAX_ARCHIVE_BYTES)
            {
                return Err(troppo_grande(bytes));
            }
            let bytes = leggi_al_piu(file, percorso)?;
            installa_file(
                &dati,
                &bytes,
                sostituisci.unwrap_or(false),
                attesa.as_deref(),
            )
        })
        .map_err(errore);
    crate::nuvola::se_riuscito(app, esito)
}

/// Un pacchetto oltre il tetto dell'archivio.
fn troppo_grande(bytes: u64) -> AppError {
    AppError::new(ErrorCode::SkinTooLarge {
        bytes,
        limit_bytes: MAX_ARCHIVE_BYTES,
    })
}

/// I byte di un pacchetto, mai più del tetto dell'archivio più uno.
///
/// # Perché non `std::fs::read`
///
/// Perché il tetto dei venti megabyte arrivava dopo, in `read_skin_package`, su
/// byte già tutti in memoria. Un file da otto gigabyte con l'estensione giusta —
/// un film rinominato, un trascinamento sbagliato — si smista come skin, e
/// finiva intero in memoria per sentirsi dire che era troppo grande. Qui se ne
/// legge al più un byte oltre il tetto: basta a dire che lo supera.
fn leggi_al_piu(lettore: impl std::io::Read, origine: &str) -> Result<Vec<u8>, AppError> {
    use std::io::Read as _;

    let mut bytes = Vec::new();
    lettore
        .take(MAX_ARCHIVE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| rymembra_app::files::io_error(origine, &err))?;
    let letti = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if letti > MAX_ARCHIVE_BYTES {
        return Err(troppo_grande(letti));
    }
    Ok(bytes)
}

/// Installa una skin dal suo manifest, senza passare dal disco.
///
/// # Perché non basta esportare
///
/// Lo Studio sapeva scrivere un `.ryskin` da qualche parte sul disco
/// (`studio_esporta`) e l'applicazione sapeva installarne uno scelto a mano
/// (`skin_installa`): chi faceva un tema doveva fare quel giro per vedere il
/// proprio lavoro nell'elenco delle skin. Fra i due passaggi non c'era nessuna
/// decisione da prendere — solo un file da posare e ritrovare — e un passaggio
/// senza decisioni è un passaggio da togliere.
///
/// Il manifest si impacchetta e si **rilegge** prima di posarlo. Rileggere quel
/// che si è appena scritto sembra uno spreco e non lo è: è la stessa porta da
/// cui entra una skin arrivata dalla rete, e passarci vuol dire che il giro
/// completo attraverso il formato è provato prima che il file esista.
#[tauri::command]
pub async fn skin_installa_sorgente(
    app: tauri::AppHandle,
    sorgente: String,
    bozza: Option<String>,
) -> Esito<VoceSkin> {
    comanda("installazione di una skin", move || {
        let stato = stato_di::<Stato>(&app)?;
        let esito = crate::studio::cartella_dati(&stato)
            .and_then(|dati| installa_sorgente(&dati, &sorgente, bozza.as_deref()))
            .map_err(errore);
        crate::nuvola::se_riuscito(&app, esito)
    })
    .await
}

/// Quel che fa [`skin_installa_sorgente`], senza lo stato di Tauri attorno.
///
/// # La skin di un altro
///
/// Con una bozza, il documento non può prendere il posto di una skin
/// installata che porta un id diverso da quello della bozza. È la regola di
/// `studio::rinomina_bozza`, ripetuta qui perché il rifiuto della rinomina
/// non bastava: il testo restava in `bozze/mia` con `"id": "altrui"`, valido,
/// e «Salva e usa» (o «Tienila» dopo la prova) lo installava sotto l'id che
/// dichiarava — sopra il pacchetto dell'altra skin, coi suoi caratteri e la
/// sua miniatura, senza ritorno. Una bozza che porta già quell'id è invece la
/// skin stessa che si aggiorna, ed è il secondo «Salva e usa» di ogni skin.
///
/// Il confronto non bada alle maiuscole: su Windows `bozze/Mia` e `bozze/mia`
/// sono la stessa cartella, e le versioni di prima lasciavano nascere bozze
/// col nome maiuscolo per una skin che il formato vuole minuscola.
///
/// Qui e non in [`installa`], che è anche la strada di `skin_installa`: chi
/// sceglie a mano un `.ryskin` con l'id di una skin che ha già vuole
/// aggiornarla.
fn installa_sorgente(
    data_dir: &Path,
    sorgente: &str,
    bozza: Option<&str>,
) -> Result<VoceSkin, AppError> {
    if let Some(bozza) = bozza {
        let id = rymembra_skin::parse_skin_json(sorgente)?.id;
        let di_un_altro = !bozza.eq_ignore_ascii_case(&id)
            && id_sicuro(&id)
            && cartella_skin(data_dir)
                .join(format!("{id}.ryskin"))
                .exists();
        if di_un_altro {
            return Err(
                AppError::new(ErrorCode::SkinIdConflict { id }).with_cause(format!(
                    "la bozza «{bozza}» dichiara l'id di una skin installata che non è la sua"
                )),
            );
        }
    }
    // La miniatura e le risorse vengono dalla bozza che lo Studio ha
    // aperto: prima si impacchettava il solo manifest, e una skin col suo
    // carattere installata da qui perdeva il carattere e la miniatura.
    let (preview, assets) = match bozza {
        Some(id) => crate::studio::corredo_della_bozza(data_dir, id)?,
        None => (None, Vec::new()),
    };
    // `write_skin_package` valida da sé — è la stessa chiamata di
    // `studio_esporta`, e gli errori del manifest fermano qui, compresa
    // una risorsa nominata che la bozza non ha.
    let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
        source: sorgente,
        preview: preview.as_deref(),
        assets: &assets,
    })?;
    installa(data_dir, &bytes)
}

/// Disinstalla una skin, e restituisce quella che resta attiva.
///
/// # Perché mancava, e perché l'assenza si vedeva
///
/// `VoceSkin::di_serie` esiste apposta e porta scritto «non si può
/// disinstallare» — cioè afferma che le altre sì. Non c'era il comando: una skin
/// installata per curiosità restava nell'elenco per sempre, e l'unico modo di
/// toglierla era cancellare un file dalla cartella dei dati.
///
/// # Togliere quella attiva
///
/// Si torna a quella di serie, qui e subito. L'alternativa — rifiutare finché
/// non se ne sceglie un'altra — costringerebbe a un giro di due passi per
/// un'operazione che ne ha uno; lasciare la scelta puntata su un file che non
/// c'è più darebbe invece una finestra senza foglio al riavvio, che è il caso
/// che `skin_scegli` sta attento a non produrre mai.
#[tauri::command]
pub async fn skin_disinstalla(app: tauri::AppHandle, id: String) -> Esito<SkinIpc> {
    comanda("disinstallazione di una skin", move || {
        let stato = stato_di::<Stato>(&app)?;
        let esito = disinstalla(&stato, &id).map_err(errore);
        crate::nuvola::se_riuscito(&app, esito)
    })
    .await
}

/// Toglie dal disco il pacchetto di una skin installata e le sue copie.
///
/// Le copie delle versioni sostituite se ne vanno con lei: fino alla 3.2.0
/// restavano in `sostituite/`, fino a venti megabyte l'una, e nessun elenco le
/// mostrava più (R04b-5, D-52). Un nome di dispositivo di Windows si toglie
/// come gli altri: è installato da prima che il formato lo rifiutasse
/// ([`nome_installato`]).
fn togli_dal_disco(data_dir: &Path, id: &str) -> Result<(), AppError> {
    if di_serie(id) || !nome_installato(id) {
        return Err(
            AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }).with_cause(
                "le skin di serie sono compilate dentro e non si disinstallano".to_owned(),
            ),
        );
    }
    // Per un nome di dispositivo si passa dal percorso verbatim (`\\?\` su
    // Windows), che apre il file e non il dispositivo: su Windows 10
    // `skin\com1.ryskin` «esiste» sempre, ed è la porta seriale.
    let dove = if rymembra_domain::paths::riservato_da_windows(id) {
        std::fs::canonicalize(cartella_skin(data_dir))
            .map_err(|_| AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }))?
    } else {
        cartella_skin(data_dir)
    };
    let file = dove.join(format!("{id}.ryskin"));
    if !file.is_file() {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    std::fs::remove_file(&file)
        .map_err(|err| rymembra_app::files::io_error(&file.display().to_string(), &err))?;
    // Le copie dopo il pacchetto: se una non si toglie, la skin è comunque
    // disinstallata, e lo si dice nel diario invece di fallire il gesto.
    let cartella = cartella_copie(data_dir, id);
    if cartella.exists()
        && let Err(err) = std::fs::remove_dir_all(&cartella)
    {
        crate::nota!("[skin] le copie di «{id}» non si tolgono ({})", err.kind());
    }
    let vecchia = copia_di_prima(data_dir, id);
    if vecchia.exists()
        && let Err(err) = std::fs::remove_file(&vecchia)
    {
        crate::nota!("[skin] la copia di «{id}» non si toglie ({})", err.kind());
    }
    Ok(())
}

/// Quel che fa [`skin_disinstalla`]. Il lucchetto della libreria si prende per
/// le due domande al database e per nient'altro: togliere un file e compilare
/// la skin che resta sono disco e processore.
fn disinstalla(stato: &Stato, id: &str) -> Result<SkinIpc, AppError> {
    let dati = crate::studio::cartella_dati(stato)?;
    togli_dal_disco(&dati, id)?;

    // Se era quella indossata, la finestra resterebbe con un foglio che non
    // ha più una sorgente: si torna a quella di serie prima di rispondere,
    // così chi riceve l'esito ha già il CSS da applicare.
    let attiva = con_libreria(stato, |libreria| Ok(skin_scelta(&libreria.connection)))?;
    if attiva == id {
        let compilata = compila(&dati, DI_SERIE)?;
        con_libreria(stato, |libreria| {
            rymembra_app::settings::write(&libreria.connection, CHIAVE_SKIN, &compilata.id)
        })?;
        return Ok(compilata);
    }
    compila(&dati, &attiva)
}

/// Sceglie la skin attiva e la compila.
///
/// La scelta si scrive **dopo** che la compilazione è riuscita: salvare prima
/// vorrebbe dire poter rendere l'applicazione illeggibile al riavvio scegliendo
/// una skin rotta.
#[tauri::command]
pub async fn skin_scegli(app: tauri::AppHandle, id: String) -> Esito<SkinIpc> {
    comanda("scelta di una skin", move || {
        let stato = stato_di::<Stato>(&app)?;
        // Compilare fuori dal lucchetto, scrivere dentro: il database serve solo
        // alla riga che ricorda la scelta.
        let esito = crate::studio::cartella_dati(&stato)
            .and_then(|dati| compila(&dati, &id))
            .and_then(|compilata| {
                con_libreria(&stato, |libreria| {
                    rymembra_app::settings::write(&libreria.connection, CHIAVE_SKIN, &compilata.id)
                })?;
                Ok(compilata)
            })
            .map_err(errore);
        crate::nuvola::se_riuscito(&app, esito)
    })
    .await
}

// ── L'accento che segue la copertina ────────────────────────────────────────
//
// Tre comandi e nessuna decisione, come tutto il resto di questo modulo: quale
// colore esca dalla copertina lo decide `rymembra_app::tinta`, se sia leggibile lo
// decide `rymembra_skin::dinamico`, e qui si legge un'impostazione e si mette in
// fila l'una dopo l'altra.

/// Una proprietà personalizzata da scrivere sulla radice della finestra.
///
/// Il valore arriva **già in CSS**: la finestra non compone colori. È la stessa
/// ragione per cui il foglio della skin arriva compilato invece che come token
/// da montare — un secondo posto in cui si scrive un `rgb()` è un secondo posto
/// in cui il contrasto può divergere.
#[derive(Debug, Clone, Serialize)]
pub struct VariabileIpc {
    /// Il nome, `--accent` e simili.
    pub nome: String,
    /// Il valore, pronto da passare a `setProperty`.
    pub valore: String,
}

/// La preferenza è accesa?
fn accento_scelto(connection: &rusqlite::Connection) -> bool {
    rymembra_app::settings::read_json(connection, CHIAVE_ACCENTO_DINAMICO)
        .ok()
        .flatten()
        // Assente o illeggibile vale spento: è la stessa regola di `skin_scelta`
        // — un'impostazione corrotta non deve poter cambiare l'aspetto
        // dell'applicazione in un modo che l'utente non ha chiesto.
        .unwrap_or(false)
}

/// Se l'accento debba seguire la copertina.
#[tauri::command]
pub async fn accento_dinamico(app: tauri::AppHandle) -> Esito<bool> {
    leggi_la_libreria(app, "accento dinamico", |libreria| {
        Ok(accento_scelto(&libreria.connection))
    })
    .await
    .map_err(errore)
}

/// Accende o spegne l'accento dinamico. Riporta com'è rimasto.
#[tauri::command]
pub async fn accento_dinamico_attiva(app: tauri::AppHandle, attivo: bool) -> Esito<bool> {
    // Un interruttore dove l'ultimo vince: sulla corsia della libreria.
    crate::corsia::LIBRERIA
        .comanda("accento dinamico", move || {
            let stato = stato_di::<Stato>(&app)?;
            con_libreria(&stato, |libreria| {
                rymembra_app::settings::write_json(
                    &libreria.connection,
                    CHIAVE_ACCENTO_DINAMICO,
                    &attivo,
                )?;
                Ok(attivo)
            })
            .map_err(errore)
        })
        .await
}

/// Le variabili da scrivere perché l'accento segua una copertina.
///
/// `null` in tutti i casi in cui non si deve toccare niente: la preferenza è
/// spenta, la skin non lo permette, la copertina non c'è o è in bianco e nero,
/// o nessuna chiarezza di quella tonalità regge il contrasto. Chi chiama non
/// deve distinguerli — in tutti e cinque la risposta è «tieni l'accento della
/// skin», e cinque codici d'errore diversi per la stessa azione sarebbero
/// cinque rami da provare per niente.
///
/// # Errori
///
/// Solo quelli del database: una skin scelta che non si legge ripiega su
/// quella di serie, come la finestra (vedi `documento_per_accento`). Una
/// copertina che non si decodifica non è un errore: è un disco senza tinta.
///
/// # L'impronta si controlla
///
/// `covers.tinta` compone un percorso dall'impronta e apre quel file, quindi
/// un'impronta che sia in realtà `../../qualcosa` è una lettura arbitraria di
/// file chiesta dalla finestra. Il controllo è lo stesso del protocollo
/// `rymembra-cover` — [`crate::copertine::e_un_impronta`] — e un'impronta che non
/// lo passa vale come una copertina che non c'è: nessuna tinta, nessun errore
/// da mostrare.
/// # Perché il lucchetto si molla a metà
///
/// Perché di tutto quel che c'è qui dentro, al database servono due domande da
/// una riga: quale skin è scelta e se l'accento dinamico è acceso. Tutto il
/// resto — aprire e decodificare la copertina, leggere il file della skin,
/// analizzarlo, calcolare la tinta — è disco e processore, e non tocca la
/// connessione nemmeno una volta.
///
/// Tenerlo dentro `con_libreria` voleva dire tenere il lucchetto **globale**
/// della libreria — quello che è uno solo, perché la connessione è una sola —
/// per tutta la durata di due letture da disco e di un parsing. Dietro ci si
/// accodava ogni comando e ogni filo di sottofondo: su un disco lento, o su una
/// copertina grande, quella è una finestra che non risponde. Ed è chiamata a
/// ogni cambio di brano.
///
/// `covers` e `data_dir` si copiano perché costano niente: il primo è un
/// maniglione clonabile sulla cartella, il secondo un `PathBuf`.
#[tauri::command]
pub async fn accento_copertina(
    app: tauri::AppHandle,
    copertina: Option<String>,
    chiaro: bool,
) -> Esito<Option<Vec<VariabileIpc>>> {
    comanda("accento della copertina", move || {
        let stato = stato_di::<Stato>(&app)?;
        let Some(impronta) = copertina else {
            return Ok(None);
        };
        if !crate::copertine::e_un_impronta(&impronta) {
            return Ok(None);
        }

        // ── col lucchetto: due domande e due copie. ─────────────────────────────
        let chiesto = con_libreria(&stato, |libreria| {
            Ok(accento_scelto(&libreria.connection).then(|| {
                (
                    libreria.covers.clone(),
                    libreria.data_dir.clone(),
                    skin_scelta(&libreria.connection),
                )
            }))
        })
        .map_err(errore)?;
        let Some((copertine, data_dir, skin)) = chiesto else {
            return Ok(None);
        };

        // ── senza lucchetto: il disco e il processore. ──────────────────────────
        let Some([r, g, b]) = copertine.tinta(&impronta) else {
            return Ok(None);
        };
        // La skin si rilegge a ogni brano invece di tenerla in memoria: tenerne una
        // copia vorrebbe dire tenerla allineata a `skin_scegli` — cioè aggiungere
        // uno stato che può divergere. Si legge però il solo manifest, non le
        // risorse: il pacchetto intero da disco resta, l'inflate di ogni carattere
        // e ogni immagine no. Fuori dal lucchetto, quindi lo paga solo chi chiama.
        // E con il ripiego di `compila_o_ripiega`: vedi `documento_per_accento`.
        let documento = documento_per_accento(&data_dir, &skin).map_err(errore)?;

        let tinta = rymembra_skin::values::Rgba { r, g, b, a: 1.0 };
        Ok(
            rymembra_skin::dinamico::accento_dinamico(&documento, tinta, chiaro).map(|variabili| {
                variabili
                    .into_iter()
                    .map(|(nome, valore)| VariabileIpc { nome, valore })
                    .collect()
            }),
        )
    })
    .await
}

/// Il documento su cui calcolare l'accento: quello della skin scelta, o quello
/// di serie se la scelta non si legge.
///
/// # Il solo manifest
///
/// Si legge con [`rymembra_skin::package::leggi_manifesto`], come
/// [`elenco`]: le stesse guardie su indice e manifest, senza decomprimere i
/// caratteri e le immagini, che all'accento non servono. Con [`sorgente`] —
/// la lettura completa di [`compila`] — ogni cambio di brano decomprimeva e
/// controllava l'intero pacchetto, fino a 60 MB, per buttare tutto tranne il
/// testo; e il commento accanto diceva «un file da qualche kilobyte».
///
/// # Il ripiego
///
/// Se la skin scelta non si legge, la finestra indossa quella di serie
/// ([`compila_o_ripiega`]): l'accento si calcola su quella. Prima tornava un
/// errore che la finestra ingoiava, e in ripiego l'accento non seguiva più
/// nessuna copertina, senza un messaggio.
///
/// Resta un caso che i due non vedono uguale, e va detto: un pacchetto col
/// manifest sano e una risorsa guasta (i byte di un `.png` che non sono un
/// PNG) non si compila, quindi la finestra indossa quella di serie, mentre qui
/// il manifest si legge e l'accento segue i colori della skin scelta. Decidere
/// anche questo vorrebbe dire rifare la lettura completa a ogni brano — il
/// costo che questa funzione toglie — per un pacchetto che `skin_elenco`
/// mostra comunque come rotto.
fn documento_per_accento(
    data_dir: &Path,
    scelta: &str,
) -> Result<rymembra_skin::SkinDocument, AppError> {
    let della_scelta = || -> Result<rymembra_skin::SkinDocument, AppError> {
        if let Some(testo) = rymembra_skin::di_serie(scelta) {
            return rymembra_skin::parse_skin_json(testo);
        }
        if !id_sicuro(scelta) {
            return Err(AppError::new(ErrorCode::SkinNotFound {
                id: scelta.to_owned(),
            }));
        }
        let percorso = cartella_skin(data_dir).join(format!("{scelta}.ryskin"));
        let bytes = std::fs::read(&percorso).map_err(|_| {
            AppError::new(ErrorCode::SkinNotFound {
                id: scelta.to_owned(),
            })
        })?;
        let manifesto = rymembra_skin::package::leggi_manifesto(&bytes)?;
        let documento = rymembra_skin::parse_skin_json(&manifesto.source)?;
        rymembra_skin::package::risorse_presenti(&documento, &manifesto.risorse)?;
        Ok(documento)
    };
    della_scelta().or_else(|_| {
        rymembra_skin::parse_skin_json(rymembra_skin::di_serie(DI_SERIE).unwrap_or_default())
    })
}

// ── Le risorse: caratteri e immagini ───────────────────────────────────────

/// Risponde a una richiesta di risorsa di una skin.
///
/// Gli indirizzi li scrive **solo** il compilatore, a partire da nomi che il
/// documento ha già validato (`rymembra_skin::compile::ORIGINE_RISORSE`):
///
/// - `http://rymembra-skin.localhost/<id>/<nome>` per una skin installata, che
///   la risorsa la tiene dentro il suo `.ryskin`;
/// - `http://rymembra-skin.localhost/bozza/<id>/<nome>` per la bozza dello
///   Studio, che la tiene in `skin/bozze/<id>/assets/`.
///
/// Qui si controlla tutto un'altra volta, perché questa è una porta e la
/// finestra ci può scrivere qualunque cosa: l'id con lo stesso alfabeto di
/// [`id_sicuro`], il nome con la lista chiusa del pacchetto, il tipo dai byte.
/// Nessun percorso si compone con qualcosa che non abbia passato i tre.
pub fn servi_risorsa(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    use tauri::Manager as _;

    let percorso = request.uri().path().trim_start_matches('/').to_owned();
    // L'impronta che il motore ha già in cache, se ce l'ha: con `no-cache`
    // chiede ogni volta, e se l'impronta è ancora quella la risposta è un 304
    // senza corpo.
    let gia = request
        .headers()
        .get(tauri::http::header::IF_NONE_MATCH)
        .and_then(|valore| valore.to_str().ok())
        .map(ToOwned::to_owned);
    let dati = ctx
        .app_handle()
        .try_state::<Stato>()
        .and_then(|stato| stato.cartella_dati().map(Path::to_path_buf));
    let Some(dati) = dati else {
        responder.respond(risposta_risorsa(Err(503)));
        return;
    };
    // Aprire un archivio è una lettura su disco, e questo è il filo della
    // finestra: vedi la stessa nota in `copertine::servi`.
    tauri::async_runtime::spawn_blocking(move || {
        responder.respond(risposta_risorsa(risorsa(&dati, &percorso, gia.as_deref())));
    });
}

/// Una risorsa da servire, o l'impronta che il motore ha già.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Servita {
    /// Il motore ha già questi byte: un 304 con l'impronta.
    Uguale(String),
    /// Il tipo, i byte e l'impronta.
    Piena {
        tipo: &'static str,
        corpo: Arc<Vec<u8>>,
        impronta: String,
    },
}

/// L'impronta di un file: dimensione e ultima modifica.
///
/// Non un hash dei byte: per farlo bisognerebbe leggerli, e il punto è non
/// leggerli. Un pacchetto reinstallato, o una risorsa della bozza sostituita,
/// cambia l'una o l'altra.
fn impronta_di(prefisso: &str, metadati: &std::fs::Metadata) -> String {
    let quando = metadati
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("\"{prefisso}-{}-{quando}\"", metadati.len())
}

/// Le risorse di un pacchetto installato, lette una volta per versione del
/// file.
struct Scaffale {
    /// Il file del pacchetto: il percorso intero e non l'id, così due
    /// cartelle dati — le prove ne aprono una per prova — non si scambiano
    /// mai le risorse.
    file: PathBuf,
    impronta: String,
    risorse: Vec<(String, &'static str, Arc<Vec<u8>>)>,
}

/// Quanti pacchetti si tengono aperti in memoria.
///
/// Due: quello indossato, e quello su cui passa il puntatore nelle
/// Impostazioni. Un pacchetto tiene al più venti megabyte di risorse, e un
/// terzo sarebbe memoria per un caso che non c'è.
const QUANTI_SCAFFALI: usize = 2;

/// I pacchetti già aperti.
///
/// # Perché
///
/// Fino alla 3.0 ogni richiesta di una risorsa rileggeva l'intero `.ryskin`
/// e decomprimeva **tutte** le sue voci — il manifest, la miniatura, ogni
/// carattere e ogni immagine — per restituirne una: una skin con tre
/// caratteri e uno sfondo apriva l'archivio quattro volte a ogni avvio e a
/// ogni cambio di tema. Qui l'archivio si apre una volta per versione del
/// file — la versione è l'impronta di [`impronta_di`] — e le risposte
/// successive escono dalla memoria.
static SCAFFALI: Mutex<Vec<Scaffale>> = Mutex::new(Vec::new());

/// La risorsa di un indirizzo, o lo stato HTTP con cui rifiutarla.
///
/// `gia` è l'impronta che il motore dice di avere: se è ancora quella, la
/// risposta è [`Servita::Uguale`] e i byte non si leggono nemmeno.
fn risorsa(dati: &Path, percorso: &str, gia: Option<&str>) -> Result<Servita, u16> {
    let pezzi: Vec<&str> = percorso.split('/').collect();
    let (bozza, id, nome) = match pezzi.as_slice() {
        ["bozza", id, nome] => (true, *id, *nome),
        [id, nome] => (false, *id, *nome),
        _ => return Err(400),
    };
    if !id_sicuro(id) || rymembra_skin::tipo_risorsa(nome).is_none() {
        return Err(400);
    }
    if bozza {
        let file = cartella_skin(dati)
            .join("bozze")
            .join(id)
            .join("assets")
            .join(nome);
        let impronta = impronta_di("b", &std::fs::metadata(&file).map_err(|_| 404_u16)?);
        if gia == Some(impronta.as_str()) {
            return Ok(Servita::Uguale(impronta));
        }
        let bytes = std::fs::read(file).map_err(|_| 404_u16)?;
        // Il tipo dai byte, come dentro un pacchetto: un `.woff2` che non lo è
        // non arriva al motore dei caratteri.
        let tipo = rymembra_skin::riconosci_risorsa(nome, &bytes).ok_or(415_u16)?;
        return Ok(Servita::Piena {
            tipo,
            corpo: Arc::new(bytes),
            impronta,
        });
    }

    let file = cartella_skin(dati).join(format!("{id}.ryskin"));
    let impronta = impronta_di(id, &std::fs::metadata(&file).map_err(|_| 404_u16)?);
    if gia == Some(impronta.as_str()) {
        return Ok(Servita::Uguale(impronta));
    }
    let trova = |scaffale: &Scaffale| {
        scaffale
            .risorse
            .iter()
            .find(|(n, _, _)| n == nome)
            .map(|(_, tipo, corpo)| Servita::Piena {
                tipo,
                corpo: Arc::clone(corpo),
                impronta: impronta.clone(),
            })
            .ok_or(404_u16)
    };
    {
        let scaffali = SCAFFALI.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(scaffale) = scaffali
            .iter()
            .find(|s| s.file == file && s.impronta == impronta)
        {
            return trova(scaffale);
        }
    }
    // Fuori dal lucchetto: due skin diverse non si aspettano, e due richieste
    // dello stesso pacchetto al peggio lo aprono due volte. Le guardie
    // dell'archivio ci sono tutte; la validazione del documento no, perché qui
    // si servono byte che il pacchetto contiene, non si decide di indossarlo.
    // Il tetto dei pixel nemmeno: vale per installare, e una skin installata
    // prima del tetto resta indossata con le sue immagini (D-53).
    let archivio = std::fs::read(&file).map_err(|_| 404_u16)?;
    let pacchetto =
        rymembra_skin::package::leggi_installato_grezzo(&archivio).map_err(|_| 404_u16)?;
    let scaffale = Scaffale {
        file: file.clone(),
        impronta: impronta.clone(),
        risorse: pacchetto
            .assets
            .into_iter()
            .map(|a| (a.name, a.mime, Arc::new(a.bytes)))
            .collect(),
    };
    let esito = trova(&scaffale);
    let mut scaffali = SCAFFALI.lock().unwrap_or_else(PoisonError::into_inner);
    scaffali.retain(|s| s.file != file);
    scaffali.insert(0, scaffale);
    scaffali.truncate(QUANTI_SCAFFALI);
    esito
}

fn risposta_risorsa(esito: Result<Servita, u16>) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::header;

    let (stato, tipo, corpo, impronta) = match esito {
        Ok(Servita::Piena {
            tipo,
            corpo,
            impronta,
        }) => (200, tipo, corpo.as_ref().clone(), Some(impronta)),
        Ok(Servita::Uguale(impronta)) => (304, "text/plain", Vec::new(), Some(impronta)),
        Err(stato) => (stato, "text/plain", Vec::new(), None),
    };
    let mut risposta = tauri::http::Response::builder()
        .status(stato)
        .header(header::CONTENT_TYPE, tipo)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        // Un carattere si chiede in modalità CORS anche da un foglio della
        // stessa finestra: senza questa intestazione il motore lo scarica e lo
        // butta. Le risorse di una skin non sono un segreto per nessuno.
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        // Un pacchetto reinstallato con lo stesso id porta byte nuovi allo
        // stesso indirizzo, e una bozza cambia sotto le mani di chi la scrive:
        // il motore chiede ogni volta, e l'impronta gli dice se può tenersi
        // quel che ha.
        .header(header::CACHE_CONTROL, "no-cache");
    if let Some(impronta) = impronta {
        risposta = risposta.header(header::ETAG, impronta);
    }
    risposta
        .body(corpo)
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// Un `.ryskin` scritto a mano, voci salvate senza comprimerle.
///
/// Per le prove di quel che lo scrittore del nucleo non produce mai: un
/// pacchetto col manifest che non passa la validazione, cioè la skin
/// installata da una versione di Rymembra che lo accettava. Lo zip più piccolo
/// che il lettore apre: intestazione locale, indice, coda — col CRC vero,
/// perché il lettore lo controlla.
#[cfg(test)]
pub(crate) fn zip_a_mano(voci: &[(&str, &[u8])]) -> Vec<u8> {
    fn crc32(byte: &[u8]) -> u32 {
        let mut crc = !0_u32;
        for b in byte {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    let quanto = |n: usize| u32::try_from(n).expect("piccolo");
    let mut fuori = Vec::new();
    let mut indice = Vec::new();
    for (nome, contenuto) in voci {
        let dove = quanto(fuori.len());
        let crc = crc32(contenuto);
        let mut comune = Vec::new();
        comune.extend_from_slice(&20_u16.to_le_bytes()); // versione necessaria
        comune.extend_from_slice(&0_u16.to_le_bytes()); // bandiere
        comune.extend_from_slice(&0_u16.to_le_bytes()); // metodo: salvato
        comune.extend_from_slice(&0_u16.to_le_bytes()); // ora
        comune.extend_from_slice(&0x21_u16.to_le_bytes()); // data: 1980-01-01
        comune.extend_from_slice(&crc.to_le_bytes());
        comune.extend_from_slice(&quanto(contenuto.len()).to_le_bytes());
        comune.extend_from_slice(&quanto(contenuto.len()).to_le_bytes());
        comune.extend_from_slice(&u16::try_from(nome.len()).expect("corto").to_le_bytes());
        comune.extend_from_slice(&0_u16.to_le_bytes()); // campo extra
        fuori.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        fuori.extend_from_slice(&comune);
        fuori.extend_from_slice(nome.as_bytes());
        fuori.extend_from_slice(contenuto);
        indice.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        indice.extend_from_slice(&20_u16.to_le_bytes()); // fatto da
        indice.extend_from_slice(&comune);
        indice.extend_from_slice(&0_u16.to_le_bytes()); // commento
        indice.extend_from_slice(&0_u16.to_le_bytes()); // disco
        indice.extend_from_slice(&0_u16.to_le_bytes()); // attributi interni
        indice.extend_from_slice(&0_u32.to_le_bytes()); // attributi esterni
        indice.extend_from_slice(&dove.to_le_bytes());
        indice.extend_from_slice(nome.as_bytes());
    }
    let inizio_indice = quanto(fuori.len());
    let quante = u16::try_from(voci.len()).expect("poche");
    fuori.extend_from_slice(&indice);
    fuori.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    fuori.extend_from_slice(&0_u16.to_le_bytes());
    fuori.extend_from_slice(&0_u16.to_le_bytes());
    fuori.extend_from_slice(&quante.to_le_bytes());
    fuori.extend_from_slice(&quante.to_le_bytes());
    fuori.extend_from_slice(&quanto(indice.len()).to_le_bytes());
    fuori.extend_from_slice(&inizio_indice.to_le_bytes());
    fuori.extend_from_slice(&0_u16.to_le_bytes());
    fuori
}

#[cfg(test)]
mod tests {
    use super::*;

    const STILE: &str = include_str!("../../src/stile.css");

    #[test]
    fn una_skin_installata_che_non_passa_piu_resta_in_elenco_da_aggiustare() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        installa(dir.path(), &pacchetto(&sorgente_con_id("nocturne"))).expect("installata");
        // La skin rotta: un token che il formato non conosce.
        let rotta = sorgente_con_id("vecchia")
            .replace("\"name\": \"Plain\"", "\"name\": \"La vecchia\"")
            .replacen(
                "\"tokens\": {",
                "\"tokens\": { \"color.accents\": \"#ff0000\",",
                1,
            );
        let cartella = dir.path().join("skin");
        std::fs::write(
            cartella.join("vecchia.ryskin"),
            zip_a_mano(&[("skin.json", rotta.as_bytes())]),
        )
        .expect("scritta");
        // E un archivio che non si apre affatto: quello resta fuori.
        std::fs::write(cartella.join("rotta.ryskin"), b"non sono uno zip").expect("scritta");
        // Il pacchetto a mano si legge con le guardie, e non con la validazione.
        let letto = std::fs::read(cartella.join("vecchia.ryskin")).expect("letto");
        assert!(rymembra_skin::package::leggi_pacchetto_grezzo(&letto).is_ok());
        assert!(rymembra_skin::read_skin_package(&letto).is_err());

        let voci = elenco(dir.path(), "vecchia").expect("l'elenco");
        let vecchia = voci
            .iter()
            .find(|v| v.id == "vecchia")
            .expect("la skin rotta è in elenco");
        assert_eq!(vecchia.nome, "La vecchia");
        assert!(vecchia.attiva);
        assert!(vecchia.guasto.is_some(), "{vecchia:?}");
        assert!(
            voci.iter()
                .any(|v| v.id == "nocturne" && v.guasto.is_none())
        );
        assert!(!voci.iter().any(|v| v.id == "rotta"));
        // Indossarla no: compilarla fallisce, e la finestra ripiega.
        let indossata = compila_o_ripiega(dir.path(), None, "vecchia").expect("ripiegata");
        assert_eq!(indossata.ripiego.as_deref(), Some("vecchia"));
    }
    const CONFIGURAZIONE: &str = include_str!("../tauri.conf.json");

    /// Il corpo di un blocco CSS: quel che sta fra la graffa aperta e la chiusa.
    fn corpo(css: &str, apertura: &str) -> String {
        let inizio = css
            .find(apertura)
            .map(|i| i + apertura.len())
            .unwrap_or_else(|| panic!("non trovo «{apertura}»"));
        let resto = css.get(inizio..).unwrap_or_default();
        let fine = resto
            .find("\n}")
            .unwrap_or_else(|| panic!("blocco non chiuso"));
        resto.get(..fine).unwrap_or_default().trim().to_owned()
    }

    fn compilata() -> rymembra_skin::CompiledSkin {
        let documento =
            rymembra_skin::parse_skin_json(rymembra_skin::PLAIN_SOURCE).expect("la skin di serie");
        rymembra_skin::compile_skin(&documento)
    }

    #[test]
    fn il_foglio_di_base_e_quello_che_produce_il_compilatore() {
        // Fra i due marcatori, `stile.css` porta l'uscita del compilatore sotto
        // `:root` invece che sotto `:root[data-skin='plain']`. Se qualcuno
        // ritocca un colore lì, questo test lo vede: senza, la finestra
        // lampeggerebbe del colore vecchio per un fotogramma a ogni avvio, che è
        // il tipo di difetto che si nota una volta su venti e non si riproduce.
        let generato = corpo(
            STILE,
            "/* ── inizio blocco generato ────────────────────────────────────────────────── */\n:root {",
        );
        let atteso = corpo(&compilata().css, ":root[data-skin='plain'] {");
        assert_eq!(
            generato, atteso,
            "\nrigenera con: cargo run -p rymembra-skin --example compila\n"
        );
        // E le misure della cornice, che sul telefono sono del motore
        // (`rymembra_skin::compile::MISURE_DEL_TELEFONO`): stanno in un blocco
        // che sul telefono non vale, anche qui.
        let generato = corpo(STILE, ":root:where(:not([data-forma='telefono'])) {");
        let atteso = corpo(
            &compilata().css,
            ":root[data-skin='plain']:where(:not([data-forma='telefono'])) {",
        );
        assert_eq!(
            generato, atteso,
            "\nrigenera con: cargo run -p rymembra-skin --example compila\n"
        );
    }

    #[test]
    fn il_telefono_del_foglio_e_quello_della_finestra() {
        // Il compilatore scrive le sovrascritture del telefono e la guardia
        // delle misure su `data-forma='telefono'`: è la parola che `main.tsx`
        // mette sulla radice. Fino alla 3.2.0 il compilatore scriveva
        // `[data-mobile]`, che non metteva nessuno (R09-2).
        let main = include_str!("../../src/main.tsx");
        assert!(
            main.contains(r#"dataset["forma"] = MOBILE ? "telefono" : "computer""#),
            "l'attributo della forma in `main.tsx` è cambiato"
        );
        let documento = rymembra_skin::parse_skin_json(&rymembra_skin::PLAIN_SOURCE.replacen(
            "\"tokens\": {",
            "\"platforms\": { \"mobile\": { \"radius.panel\": \"12px\" } },\n  \"tokens\": {",
            1,
        ))
        .expect("plain col telefono");
        let css = rymembra_skin::compile_skin(&documento).css;
        assert!(
            css.contains(":root[data-skin='plain'][data-forma='telefono'] {"),
            "{css}"
        );
        assert!(!css.contains("data-mobile"));
        // E il foglio del motore per il telefono riscrive le stesse misure che
        // il compilatore gli lascia.
        let telefono = include_str!("../../src/stile/telefono.css");
        let radice = corpo(telefono, ":root[data-forma=\"telefono\"] {");
        for misura in rymembra_skin::compile::MISURE_DEL_TELEFONO {
            assert!(
                radice.contains(&format!("{misura}:")),
                "{misura} manca in telefono.css"
            );
        }
    }

    #[test]
    fn anche_la_variante_chiara_e_quella_del_compilatore() {
        // Il blocco di base da solo bastava finché il tema chiaro non esisteva:
        // `themes.light` sovrascriveva otto voci e lasciava superfici e testo
        // scuri, quindi il primo fotogramma senza token era sbagliato in modo
        // impercettibile. Ora che il chiaro è completo, chi lo sceglie vedrebbe
        // una finestra **nera** per il fotogramma che precede la risposta
        // dell'IPC — quindi anche questo blocco va copiato, e quindi anche
        // questo blocco va agganciato al compilatore.
        let generato = corpo(STILE, ":root[data-theme='light'] {");
        let atteso = corpo(
            &compilata().css,
            ":root[data-skin='plain'][data-theme='light'] {",
        );
        assert_eq!(
            generato, atteso,
            "\nrigenera con: cargo run -p rymembra-skin --example compila\n"
        );
    }

    #[test]
    fn gli_indirizzi_dei_nodi_sono_quelli_del_foglio() {
        // I due `indirizzo()` — quello del compilatore e quello qui — devono
        // dare la stessa stringa, perché uno finisce nel selettore e l'altro
        // nell'attributo. Sono due righe uguali in due crate diversi, e questo
        // test è ciò che impedisce loro di divergere in silenzio.
        let documento =
            rymembra_skin::parse_skin_json(rymembra_skin::PLAIN_SOURCE).expect("la skin di serie");
        let albero = impaginazione(&documento).shell;
        let css = compilata().css;

        let mut da_visitare = vec![albero];
        let mut quanti = 0;
        while let Some(nodo) = da_visitare.pop() {
            let selettore = format!("[data-nodo='{}']", nodo.at);
            assert!(css.contains(&selettore), "manca {selettore} nel foglio");
            quanti += 1;
            da_visitare.extend(nodo.children);
        }
        // E non solo: ogni indirizzo del foglio è di un nodo che esiste.
        assert_eq!(quanti, css.matches("[data-nodo='").count());
    }

    #[test]
    fn lo_scafale_arriva_anche_a_chi_non_lo_dichiara() {
        // Il renderer non ha un caso «manca»: una skin che tace riceve l'albero
        // di serie, e lo riceve dal nucleo invece che da una copia in
        // TypeScript.
        let documento = rymembra_skin::parse_skin_json(
            r##"{"format":1,"id":"muta","meta":{"name":"M","author":"A","version":"1.0.0"},"tokens":{}}"##,
        )
        .expect("valida");
        let shell = impaginazione(&documento).shell;
        assert_eq!(shell.kind, "zone");
        assert_eq!(shell.at, "radice");
        assert_eq!(shell.part, Some("app-shell"));
        assert!(!shell.children.is_empty());
    }

    #[test]
    fn il_fondo_della_finestra_e_quello_della_skin_di_serie() {
        // Il colore dell'avvio a freddo: lo dipinge il sistema operativo prima
        // che esista una pagina, quindi deve stare nella configurazione e non
        // può venire dall'IPC. Nel vecchio albero è il difetto ancora aperto —
        // MainActivity e capacitor.config cablano `#09090d`, il fondo di UNA
        // skin — e la differenza qui non è che il valore non sia cablato: è che
        // se si scolla dalla skin di serie, questo test lo dice.
        let css = compilata().css;
        let surface = css
            .lines()
            .find_map(|riga| riga.trim().strip_prefix("--color-surface-0: "))
            .map(|valore| valore.trim_end_matches(';'))
            .expect("la skin di serie dichiara il fondo");

        let configurato = CONFIGURAZIONE
            .lines()
            .find_map(|riga| riga.trim().strip_prefix("\"backgroundColor\": "))
            .map(|valore| valore.trim_end_matches(',').trim_matches('"'))
            .expect("la finestra dichiara un fondo");

        assert_eq!(
            rymembra_skin::values::parse_color(configurato),
            rymembra_skin::values::parse_color(surface),
            "il fondo della finestra ({configurato}) non è quello di «{DI_SERIE}» ({surface})"
        );
    }

    /// Una cartella dati che non contiene nessuna skin installata.
    fn vuota() -> &'static Path {
        Path::new("skin-che-non-esiste")
    }

    #[test]
    fn una_skin_che_non_esiste_lo_dice() {
        let err = compila(vuota(), "nocturne").expect_err("compilata");
        assert_eq!(err.code().kind().code(), "skin.notFound");
        // Ritentare non ha senso e l'interfaccia deve saperlo senza leggere il
        // messaggio.
        assert!(!err.is_retryable());
    }

    #[test]
    fn la_skin_scelta_che_sparisce_non_lascia_la_finestra_vuota() {
        // Il caso vero: il database dice «cyberpunk-neon», il file non c'è più.
        // All'avvio nessuno chiede una skin per nome, quindi `chiesta` è
        // `None`: si deve ottenere una finestra che si disegna.
        let indossata = compila_o_ripiega(vuota(), None, "cyberpunk-neon").expect("ripiegata");
        assert_eq!(indossata.id, DI_SERIE);
        assert_eq!(indossata.ripiego.as_deref(), Some("cyberpunk-neon"));
        // E l'albero c'è: è quello la differenza fra una finestra sobria e una
        // finestra vuota.
        assert!(!indossata.layout.shell.children.is_empty());
    }

    #[test]
    fn chi_sceglie_una_skin_rotta_se_lo_sente_dire() {
        // Dal selettore si chiede per nome, e lì il ripiego sarebbe una bugia:
        // chi ha appena cliccato «nocturne» vedrebbe apparire «plain» senza
        // che nessuno gli abbia detto perché.
        let err = compila_o_ripiega(vuota(), Some("nocturne"), "nocturne").expect_err("respinta");
        assert_eq!(err.code().kind().code(), "skin.notFound");
    }

    #[test]
    fn una_skin_che_si_legge_non_ripiega_su_niente() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        installa(dir.path(), &pacchetto(&sorgente_con_id("nocturne"))).expect("installata");
        let indossata = compila_o_ripiega(dir.path(), None, "nocturne").expect("compilata");
        assert_eq!(indossata.id, "nocturne");
        assert_eq!(indossata.ripiego, None);
    }

    #[test]
    fn senza_libreria_si_indossa_quella_di_serie() {
        // Una libreria che non si apre — una migrazione fallita, un database
        // scritto da una versione più nuova, un file rotto — non dice quale
        // skin è scelta. Senza skin non c'è impaginazione, e senza
        // impaginazione la finestra è vuota: niente fascia che dica perché,
        // niente Impostazioni. Quella di serie viene dal binario, e basta lei
        // a disegnare la finestra che spiega il guasto.
        for guasto in [
            ErrorCode::DbVersionAhead {
                db_version: 99,
                app_version: rymembra_app::db::LATEST_VERSION,
            },
            ErrorCode::DbMigrationFailed {
                from: 38,
                to: 40,
                step: Some("insieme-indirizzi".to_owned()),
            },
        ] {
            let indossata =
                da_indossare(Err(AppError::new(guasto))).expect("la finestra si disegna");
            assert_eq!(indossata.id, DI_SERIE);
            assert_eq!(
                indossata.ripiego, None,
                "non è la skin scelta che non si compila: la scelta non si sa"
            );
        }
        // E con la libreria aperta la regola resta quella di sempre.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        installa(dir.path(), &pacchetto(&sorgente_con_id("nocturne"))).expect("installata");
        let indossata =
            da_indossare(Ok((dir.path().to_path_buf(), "nocturne".to_owned()))).expect("compilata");
        assert_eq!(indossata.id, "nocturne");
    }

    #[test]
    fn quella_di_serie_si_compila_senza_toccare_il_disco() {
        // `vuota()` non esiste: la skin di serie deve venire dal binario, non
        // dalla cartella delle installate. È la garanzia che l'applicazione si
        // disegna anche con la cartella dati appena creata.
        let compilata = compila(vuota(), DI_SERIE).expect("compilata");
        assert_eq!(compilata.id, DI_SERIE);
        assert!(compilata.css.contains(":root[data-skin='plain']"));
        assert_eq!(compilata.cost, 0);
    }

    #[test]
    fn un_identificatore_non_puo_uscire_dalla_cartella() {
        // L'id diventa un nome di file, quindi è un vettore di traversal. Non
        // basta che il formato dichiari che è alfanumerico: quel che arriva
        // qui viene dalla finestra e da `settings`, e nessuno dei due è il
        // parser del manifest.
        for cattivo in [
            "../../../../windows/system32/config/sam",
            "..\\..\\altrove",
            "c:/assoluto",
            "",
        ] {
            let err = compila(vuota(), cattivo).expect_err("respinta");
            assert_eq!(
                err.code().kind().code(),
                "skin.notFound",
                "«{cattivo}» non è stato respinto"
            );
        }
        // Né un dispositivo di Windows: `skin/con.ryskin` su Windows 10 non è
        // un file. Non si installa, e una scelta col suo nome non si apre: si
        // respinge col guasto del manifest, perché un pacchetto così può
        // esserci, installato prima che il formato lo rifiutasse (R04b-6).
        for dispositivo in ["con", "nul", "COM1", "lpt9"] {
            assert!(!id_sicuro(dispositivo), "«{dispositivo}»");
            let err = compila(vuota(), dispositivo).expect_err("respinta");
            assert_eq!(
                err.code().kind().code(),
                "skin.manifestInvalid",
                "«{dispositivo}»"
            );
        }
        // E gli identificatori legittimi passano il controllo — se non
        // passassero, il guardiano avrebbe chiuso anche la porta d'ingresso.
        assert!(id_sicuro("nocturne"));
        assert!(id_sicuro("nothing-red"));
        assert!(id_sicuro("skin_2"));
    }

    /// Un manifest valido con l'id che si vuole: quello di serie, rinominato.
    fn sorgente_con_id(id: &str) -> String {
        rymembra_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""))
    }

    fn pacchetto(sorgente: &str) -> Vec<u8> {
        rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
            source: sorgente,
            preview: None,
            assets: &[],
        })
        .expect("impacchettata")
    }

    #[test]
    fn una_skin_si_installa_dal_suo_manifest() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let voce =
            installa(dir.path(), &pacchetto(&sorgente_con_id("nocturne"))).expect("installata");

        assert_eq!(voce.id, "nocturne");
        assert!(!voce.di_serie);
        assert!(dir.path().join("skin").join("nocturne.ryskin").is_file());

        // Da qui in poi è una skin come le altre: il giro completo — manifest,
        // pacchetto, cartella, compilazione — è quel che questo test prova, e
        // senza `compila` proverebbe solo che un file è stato scritto.
        let compilata = compila(dir.path(), "nocturne").expect("compilata");
        assert_eq!(compilata.id, "nocturne");
        assert!(compilata.css.contains(":root[data-skin='nocturne']"));
    }

    /// La skin di serie con una tinta che pulsa sedici volte — 23 di movimento
    /// — assegnata agli stati dati della scheda.
    fn sorgente_cara(id: &str, stati: &[&str]) -> String {
        let mut documento: serde_json::Value =
            serde_json::from_str(&sorgente_con_id(id)).expect("la skin di serie");
        documento["motion"]["animations"] = serde_json::json!({
            "tinta": { "duration": "2s", "iterations": 16, "frames": [
                { "at": 0, "backgroundColor": "#101014" },
                { "at": 50, "backgroundColor": "#20202a" },
                { "at": 100, "backgroundColor": "#101014" }
            ] }
        });
        let mut innesco = serde_json::Map::new();
        for stato in stati {
            innesco.insert((*stato).to_owned(), "tinta".into());
        }
        documento["parts"]["card"] = serde_json::json!({ "motion": innesco });
        serde_json::to_string_pretty(&documento).expect("riscritta")
    }

    /// Un file che non finisce mai, o quasi: conta i byte che gli si chiedono.
    struct Fiume {
        dati: u64,
        letti: u64,
    }

    impl std::io::Read for Fiume {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let resto = self.dati - self.letti;
            let quanti = usize::try_from(resto).unwrap_or(usize::MAX).min(buf.len());
            buf[..quanti].fill(0);
            self.letti += quanti as u64;
            Ok(quanti)
        }
    }

    #[test]
    fn un_file_oltre_il_tetto_non_si_legge_tutto() {
        // Un `.mkv` da otto gigabyte rinominato `.ryskin` e trascinato sulla
        // finestra: si smista come skin per estensione, e finiva tutto in
        // memoria prima che il tetto dei venti megabyte dicesse di no. Qui
        // ne bastano sessantaquattro per vedere quanti se ne leggono.
        let mut fiume = Fiume {
            dati: 64 * 1024 * 1024,
            letti: 0,
        };
        let esito = leggi_al_piu(&mut fiume, "film.ryskin");
        assert!(
            fiume.letti <= MAX_ARCHIVE_BYTES + 1,
            "letti {} MB per un tetto di {} MB",
            fiume.letti >> 20,
            MAX_ARCHIVE_BYTES >> 20
        );
        let err = esito.expect_err("oltre il tetto");
        assert_eq!(err.code().kind().code(), "skin.tooLarge");

        // Un pacchetto vero si legge intero.
        let pacchetto = pacchetto(&sorgente_con_id("nocturne"));
        let letto = leggi_al_piu(pacchetto.as_slice(), "nocturne.ryskin").expect("letto");
        assert_eq!(letto, pacchetto);
    }

    /// Una miniatura: la firma e l'intestazione di una PNG del lato dato.
    fn miniatura(lato: u8) -> Vec<u8> {
        let mut png = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0, 0, 0, lato, 0, 0, 0, lato, 8, 6, 0, 0, 0, 0, 0, 0, 0]);
        png
    }

    /// `notte`, col nome, la versione e la miniatura dati.
    fn notte(nome: &str, versione: &str, lato: u8) -> Vec<u8> {
        let mut documento: serde_json::Value =
            serde_json::from_str(&sorgente_con_id("notte")).expect("la skin di serie");
        documento["meta"]["name"] = nome.into();
        documento["meta"]["version"] = versione.into();
        let sorgente = serde_json::to_string_pretty(&documento).expect("riscritta");
        let png = miniatura(lato);
        rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
            source: &sorgente,
            preview: Some(&png),
            assets: &[],
        })
        .expect("impacchettata")
    }

    #[test]
    fn una_skin_con_lo_stesso_id_non_ne_sostituisce_un_altra_senza_dirlo() {
        // La skin `notte` fatta nello Studio, con la sua miniatura; poi un
        // `notte-1.0.0.ryskin` di qualcun altro trascinato sulla finestra.
        // Fino alla 3.2 il secondo prendeva il posto del primo in silenzio, e
        // il backup portava la sostituzione su Drive.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let file = dir.path().join("skin").join("notte.ryskin");
        let mia = notte("La mia notte", "2.1.0", 1);
        let altrui = notte("Notte", "1.0.0", 2);
        let prima = installa_file(dir.path(), &mia, false, None).expect("installata");
        assert!(prima.installata.is_some() && prima.da_sostituire.is_none());

        // Senza conferma non si scrive niente, e la domanda dice chi è chi.
        let domanda = installa_file(dir.path(), &altrui, false, None).expect("risposta");
        assert!(domanda.installata.is_none(), "{domanda:?}");
        assert_eq!(
            domanda.da_sostituire,
            Some(SostituzioneIpc {
                id: "notte".to_owned(),
                presente: VersioneIpc {
                    nome: "La mia notte".to_owned(),
                    versione: "2.1.0".to_owned(),
                    autore: "Rymembra".to_owned(),
                },
                nuova: VersioneIpc {
                    nome: "Notte".to_owned(),
                    versione: "1.0.0".to_owned(),
                    autore: "Rymembra".to_owned(),
                },
                impronta: rymembra_app::backup::impronta(&mia),
            })
        );
        assert_eq!(std::fs::read(&file).expect("letta"), mia);

        // Lo stesso pacchetto un'altra volta non sostituisce niente, e non chiede.
        let uguale = installa_file(dir.path(), &mia, false, None).expect("installata");
        assert!(uguale.installata.is_some() && uguale.da_sostituire.is_none());

        // Con la conferma si sostituisce, e quella di prima resta da parte.
        let fatta = installa_file(dir.path(), &altrui, true, None).expect("sostituita");
        let voce = fatta.installata.expect("installata");
        assert_eq!(std::fs::read(&file).expect("letta"), altrui);
        let copia = voce.sostituita.clone().expect("la copia");
        assert_eq!(copia.skin.nome, "La mia notte");
        assert_eq!(copia.skin.versione, "2.1.0");
        assert_eq!(std::fs::read(&copia.percorso).expect("la copia"), mia);
        // L'elenco la sa, e il backup non la vede: non è una skin installata.
        let elencata = elenco(dir.path(), "notte").expect("elenco");
        let notte = elencata
            .iter()
            .find(|v| v.id == "notte")
            .expect("in elenco");
        assert_eq!(notte.sostituita, Some(copia.clone()));
        assert_eq!(notte.versione, "1.0.0");
        assert_eq!(
            rymembra_cloud::pacchetti::elenca_skin(dir.path())
                .keys()
                .collect::<Vec<_>>(),
            ["notte"]
        );

        // Rimetterla è reinstallare la copia: le due si scambiano il posto.
        let copiata = std::fs::read(&copia.percorso).expect("la copia");
        let tornata = installa_file(dir.path(), &copiata, true, None).expect("rimessa");
        assert!(tornata.installata.is_some());
        assert_eq!(std::fs::read(&file).expect("letta"), mia);
        // Quella rimessa non è più fra le copie, quella tolta sì.
        assert!(!std::path::Path::new(&copia.percorso).exists());
        let ora = sostituita_di(dir.path(), "notte").expect("la copia nuova");
        assert_eq!(std::fs::read(&ora.percorso).expect("la copia"), altrui);

        // E una skin di serie resta fuori, come prima.
        let serie = installa_file(
            dir.path(),
            &pacchetto(rymembra_skin::PLAIN_SOURCE),
            true,
            None,
        );
        assert!(serie.is_err());
    }

    #[test]
    fn una_skin_oltre_il_doppio_del_budget_non_arriva_nella_cartella() {
        // Valida — lo Studio la apre e la mostra con un avviso — ma chi la
        // installa non la deve trovare nell'elenco a far scattare la finestra.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let err = installa(
            dir.path(),
            &pacchetto(&sorgente_cara("pesante", &["hover", "current"])),
        )
        .expect_err("respinta");
        assert_eq!(err.code().kind().code(), "skin.overBudget");
        assert!(!dir.path().join("skin").join("pesante.ryskin").exists());
    }

    #[test]
    fn chi_installa_riceve_l_esame_e_l_elenco_no() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let sobria =
            installa(dir.path(), &pacchetto(&sorgente_con_id("nocturne"))).expect("installata");
        let esame = sobria.esame.expect("l'esame");
        assert!(!esame.da_confermare);
        assert!(esame.cicli.is_empty());

        // Sopra il budget e sotto il doppio: si installa, e si chiede.
        let vivace = installa(dir.path(), &pacchetto(&sorgente_cara("vivace", &["hover"])))
            .expect("installata");
        let esame = vivace.esame.as_ref().expect("l'esame");
        assert!(esame.da_confermare);
        assert_eq!(
            (esame.movimento_parte.costo, esame.movimento_parte.budget),
            (23, rymembra_skin::MOTION_COST_BUDGET)
        );

        // Nell'IPC il campo manca del tutto quando non c'è: l'elenco resta
        // quello di prima.
        let voce = VoceSkin::di(
            rymembra_skin::parse_skin_json(rymembra_skin::PLAIN_SOURCE).expect("la skin di serie"),
            true,
            true,
        );
        let scritto = serde_json::to_value(&voce).expect("serializzata");
        assert!(scritto.get("esame").is_none(), "{scritto}");
        let scritto = serde_json::to_value(&vivace).expect("serializzata");
        assert_eq!(scritto["esame"]["daConfermare"], true, "{scritto}");
        assert_eq!(scritto["esame"]["movimentoParte"]["costo"], 23, "{scritto}");
    }

    #[test]
    fn il_manifest_che_scrive_la_finestra_e_accettato() {
        // La finestrella «Crea tema» non compone un manifest a mano: riscrive
        // `id` e `meta` su una skin che c'è già e riserializza tutto
        // (`studio/nuovo.ts`, che si appoggia a `studio/patch.ts`). Il testo che
        // arriva qui è quindi **riformattato** e porta un `meta.basedOn` che la
        // base non aveva — due differenze che il test con la sola sostituzione
        // dell'id non vedrebbe.
        let mut documento: serde_json::Value =
            serde_json::from_str(rymembra_skin::PLAIN_SOURCE).expect("la skin di serie");
        documento["id"] = "notturno".into();
        documento["meta"]["name"] = "Notturno".into();
        documento["meta"]["author"] = "Io".into();
        documento["meta"]["version"] = "1.0.0".into();
        documento["meta"]["description"] = "Blu di notte".into();
        documento["meta"]["basedOn"] = DI_SERIE.into();
        let sorgente = serde_json::to_string_pretty(&documento).expect("riscritta");

        let dir = tempfile::tempdir().expect("cartella temporanea");
        let voce = installa(dir.path(), &pacchetto(&sorgente)).expect("installata");

        assert_eq!(voce.id, "notturno");
        assert_eq!(voce.nome, "Notturno");
        assert_eq!(voce.autore, "Io");
        assert_eq!(voce.descrizione.as_deref(), Some("Blu di notte"));
        // Le tre fasce arrivano dalla base: un tema appena creato è la base, e
        // la sua scheda in Impostazioni non deve essere grigia.
        assert_eq!(voce.anteprima.len(), 3);
        assert_eq!(voce.scheme, "dark");
        assert!(voce.variante);
    }

    #[test]
    fn l_accento_non_dipende_dalle_risorse() {
        // Per l'accento serve il manifest: un pacchetto con un carattere o
        // un'immagine non si decomprime tutto a ogni brano.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        std::fs::write(
            cartella.join("trama.ryskin"),
            zip_a_mano(&[
                ("skin.json", sorgente_con_id("trama").as_bytes()),
                ("assets/x.png", b"non sono un png"),
            ]),
        )
        .expect("scritta");
        let documento = documento_per_accento(dir.path(), "trama").expect("il manifest");
        assert_eq!(documento.id, "trama");
    }

    #[test]
    fn ripiego_anche_per_l_accento() {
        // La skin scelta non c'è più: la finestra indossa quella di serie, e
        // l'accento la segue invece di sparire a ogni brano.
        let documento = documento_per_accento(vuota(), "sparita").expect("quella di serie");
        assert_eq!(documento.id, DI_SERIE);
        let rotta = tempfile::tempdir().expect("cartella temporanea");
        let cartella = rotta.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        std::fs::write(cartella.join("guasta.ryskin"), b"non sono uno zip").expect("scritta");
        let documento = documento_per_accento(rotta.path(), "guasta").expect("quella di serie");
        assert_eq!(documento.id, DI_SERIE);
    }

    /// I token che il motore scrive in `stile.css` per conto suo e non con
    /// la skin, che non si trovano nel foglio come `--nome: valore;`.
    fn difetti_mancanti(stile: &str) -> Vec<String> {
        rymembra_skin::tokens::DIFETTI_DEL_MOTORE
            .iter()
            .filter_map(|(id, valore)| {
                let css = rymembra_skin::tokens::token(id).map_or("?", |def| def.css);
                let riga = format!("{css}: {valore};");
                (!stile.contains(&riga)).then_some(riga)
            })
            .collect()
    }

    #[test]
    fn i_difetti_del_motore_sono_quelli_di_stile_css() {
        // Due copie degli stessi numeri, e apposta: `stile.css` li scrive sotto
        // `:root` per la finestra vera, dove un token che la skin non dichiara
        // ricade lì; il compilatore in modo anteprima li prende da
        // `DIFETTI_DEL_MOTORE`, perché l'anteprima dello Studio sta dentro la
        // finestra e altrimenti erediterebbe i valori della skin indossata. Il
        // foglio non si legge dal nucleo, quindi il confronto sta qui.
        assert_eq!(difetti_mancanti(STILE), Vec::<String>::new());
        // E la prova sa fallire: un numero cambiato nel foglio si vede.
        let cambiato = STILE.replacen("--ritmo-1: 4px;", "--ritmo-1: 5px;", 1);
        assert_eq!(difetti_mancanti(&cambiato), ["--ritmo-1: 4px;"]);
    }

    #[test]
    fn salva_e_usa_non_sovrascrive_la_skin_installata_di_un_altro() {
        // Il caso vero: la bozza «mia» dichiara l'id di una skin installata che
        // non è sua. La rinomina l'ha già rifiutata (`studio::rinomina_bozza`),
        // il testo è rimasto in `bozze/mia` con `"id": "altrui"`, e «Salva e
        // usa» resta acceso perché il documento è valido.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let altrui = sorgente_con_id("altrui").replacen(
            "\"id\": \"altrui\",",
            "\"id\": \"altrui\", \"fonts\": { \"Prova\": { \"src\": \"prova.woff2\" } },",
            1,
        );
        let installata =
            rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
                source: &altrui,
                preview: None,
                assets: &[rymembra_skin::package::SkinAsset {
                    name: "prova.woff2".to_owned(),
                    mime: "font/woff2",
                    bytes: b"wOF2 finto con la firma giusta".to_vec(),
                }],
            })
            .expect("impacchettata");
        installa(dir.path(), &installata).expect("installata");
        let file = dir.path().join("skin").join("altrui.ryskin");
        let prima = std::fs::read(&file).expect("letta");

        let bozza = dir.path().join("skin").join("bozze").join("mia");
        std::fs::create_dir_all(&bozza).expect("bozza");
        std::fs::write(bozza.join("skin.json"), sorgente_con_id("altrui")).expect("scritta");

        let err = installa_sorgente(dir.path(), &sorgente_con_id("altrui"), Some("mia"))
            .expect_err("la skin di un altro non si sovrascrive");
        assert_eq!(err.code().kind().code(), "skin.idConflict");
        assert_eq!(
            std::fs::read(&file).expect("letta"),
            prima,
            "il pacchetto dell'altra skin è cambiato"
        );

        // La propria skin, invece, si riscrive a ogni «Salva e usa».
        installa_sorgente(dir.path(), &sorgente_con_id("mia"), Some("mia")).expect("la prima");
        installa_sorgente(dir.path(), &sorgente_con_id("mia"), Some("mia")).expect("la seconda");
        // Anche da una bozza rimasta in una cartella con le maiuscole, che le
        // versioni di prima lasciavano nascere: su Windows è la stessa cartella.
        let maiuscola = dir.path().join("skin").join("bozze").join("Mia");
        std::fs::create_dir_all(&maiuscola).expect("bozza");
        std::fs::write(maiuscola.join("skin.json"), sorgente_con_id("mia")).expect("scritta");
        installa_sorgente(dir.path(), &sorgente_con_id("mia"), Some("Mia")).expect("la terza");
        // E senza bozza non c'è niente da confrontare: la strada di prima.
        installa_sorgente(dir.path(), &sorgente_con_id("mia"), None).expect("senza bozza");
    }

    #[test]
    fn una_skin_non_puo_prendere_il_posto_di_quella_di_serie() {
        // `plain` viene dal binario e non dalla cartella: un `plain.ryskin`
        // installato sarebbe un file che non si può più togliere dall'elenco e
        // che nessuno legge, perché `sorgente` risponde prima di guardare il
        // disco.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let err =
            installa(dir.path(), &pacchetto(rymembra_skin::PLAIN_SOURCE)).expect_err("respinta");

        assert_eq!(err.code().kind().code(), "skin.notFound");
        assert!(!dir.path().join("skin").join("plain.ryskin").exists());

        // E vale per tutte quelle di serie, non solo per la prima.
        let vinile = rymembra_skin::di_serie("vinile").expect("vinile è di serie");
        installa(dir.path(), &pacchetto(vinile)).expect_err("respinta");
        assert!(!dir.path().join("skin").join("vinile.ryskin").exists());
    }

    #[test]
    fn le_skin_di_serie_si_compilano_dal_binario() {
        for (id, _) in rymembra_skin::DI_SERIE {
            let compilata = compila(vuota(), id).expect("compilata");
            assert_eq!(compilata.id, *id);
            assert_eq!(compilata.ripiego, None);
        }
        // Vinile nasce chiara e ha la variante scura; Neon è scura e basta.
        let vinile = compila(vuota(), "vinile").expect("vinile");
        assert_eq!((vinile.scheme, vinile.variante), ("light", true));
        let neon = compila(vuota(), "neon").expect("neon");
        assert_eq!((neon.scheme, neon.variante), ("dark", false));
    }

    #[test]
    fn le_presenze_arrivano_alla_finestra() {
        // Vinile fa entrare e uscire la colonna: senza questa voce la finestra
        // non saprebbe di doverla avvolgere in una transizione di vista.
        let vinile = compila(vuota(), "vinile").expect("vinile");
        let colonna = vinile
            .presenze
            .iter()
            .find(|p| p.parte == "column")
            .expect("la colonna è una presenza di vinile");
        assert!(colonna.enter && colonna.exit && !colonna.share);
        // Plain non dichiara presenze sue: resta solo la pagina, dalla rotta.
        let plain = compila(vuota(), DI_SERIE).expect("plain");
        assert!(plain.presenze.iter().all(|p| p.parte == "page-content"));
    }

    /// Il più piccolo `woff2` che il controllo dei byte accetti.
    const WOFF2: &[u8] = b"wOF2 finto ma con la firma giusta";

    #[test]
    fn una_risorsa_si_serve_dal_pacchetto_o_dalla_bozza() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let archivio =
            rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
                source: &sorgente_con_id("nocturne"),
                preview: None,
                assets: &[rymembra_skin::package::SkinAsset {
                    name: "titoli.woff2".to_owned(),
                    mime: "font/woff2",
                    bytes: WOFF2.to_vec(),
                }],
            })
            .expect("impacchettata");
        installa(dir.path(), &archivio).expect("installata");
        assert_eq!(
            piena(risorsa(dir.path(), "nocturne/titoli.woff2", None)),
            Ok(("font/woff2", WOFF2.to_vec()))
        );
        assert_eq!(risorsa(dir.path(), "nocturne/altro.woff2", None), Err(404));

        let bozza = dir
            .path()
            .join("skin")
            .join("bozze")
            .join("prova")
            .join("assets");
        std::fs::create_dir_all(&bozza).expect("cartella della bozza");
        std::fs::write(bozza.join("titoli.woff2"), WOFF2).expect("scritta");
        std::fs::write(bozza.join("finto.png"), b"non sono un png").expect("scritta");
        assert_eq!(
            piena(risorsa(dir.path(), "bozza/prova/titoli.woff2", None)),
            Ok(("font/woff2", WOFF2.to_vec()))
        );
        // Il nome dice png, i byte no: non passa.
        assert_eq!(risorsa(dir.path(), "bozza/prova/finto.png", None), Err(415));
    }

    /// Il tipo e i byte di una risorsa servita per intero.
    fn piena(esito: Result<Servita, u16>) -> Result<(&'static str, Vec<u8>), u16> {
        esito.map(|servita| match servita {
            Servita::Piena { tipo, corpo, .. } => (tipo, corpo.as_ref().clone()),
            Servita::Uguale(_) => ("uguale", Vec::new()),
        })
    }

    /// L'impronta di una risorsa servita.
    fn impronta(esito: &Result<Servita, u16>) -> String {
        match esito {
            Ok(Servita::Piena { impronta, .. } | Servita::Uguale(impronta)) => impronta.clone(),
            Err(stato) => panic!("non servita: {stato}"),
        }
    }

    #[test]
    fn chi_ha_gia_la_risorsa_riceve_un_304_e_un_pacchetto_nuovo_cambia_impronta() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let pacchetto = |bytes: &[u8]| {
            rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
                source: &sorgente_con_id("nocturne"),
                preview: None,
                assets: &[rymembra_skin::package::SkinAsset {
                    name: "titoli.woff2".to_owned(),
                    mime: "font/woff2",
                    bytes: bytes.to_vec(),
                }],
            })
            .expect("impacchettata")
        };
        installa(dir.path(), &pacchetto(WOFF2)).expect("installata");
        let prima = risorsa(dir.path(), "nocturne/titoli.woff2", None);
        let segno = impronta(&prima);
        // La seconda volta esce dalla memoria, uguale.
        assert_eq!(risorsa(dir.path(), "nocturne/titoli.woff2", None), prima);
        // Chi ha già quell'impronta non riceve i byte.
        assert_eq!(
            risorsa(dir.path(), "nocturne/titoli.woff2", Some(&segno)),
            Ok(Servita::Uguale(segno.clone()))
        );
        // Reinstallata con un carattere diverso: impronta nuova, byte nuovi.
        let altro = b"wOF2 un altro carattere, piu lungo del primo";
        installa(dir.path(), &pacchetto(altro)).expect("reinstallata");
        let dopo = risorsa(dir.path(), "nocturne/titoli.woff2", Some(&segno));
        assert_ne!(impronta(&dopo), segno);
        assert_eq!(piena(dopo), Ok(("font/woff2", altro.to_vec())));
    }

    #[test]
    fn il_protocollo_delle_risorse_non_esce_dalla_cartella() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for cattivo in [
            "",
            "nocturne",
            "../rymembra.db",
            "nocturne/../../rymembra.db",
            "bozza/../x.png",
            "bozza/prova/../../x.png",
            "nocturne/titoli.svg",
            "nocturne/Titoli.woff2",
            "nocturne/sotto/titoli.woff2",
            "no turne/titoli.woff2",
            "bozza/prova/%2e%2e.png",
        ] {
            assert_eq!(risorsa(dir.path(), cattivo, None), Err(400), "«{cattivo}»");
        }
    }

    #[test]
    fn la_csp_lascia_passare_caratteri_e_immagini_delle_skin() {
        for chiave in ["\"csp\"", "\"devCsp\""] {
            let riga = CONFIGURAZIONE
                .lines()
                .find(|riga| riga.trim_start().starts_with(chiave))
                .expect("la chiave c'è");
            for direttiva in ["img-src", "font-src"] {
                let valore = riga
                    .split(';')
                    .find(|pezzo| pezzo.contains(direttiva))
                    .unwrap_or_default();
                assert!(
                    valore.contains("http://rymembra-skin.localhost"),
                    "{chiave}: {direttiva} non ammette le risorse delle skin"
                );
            }
        }
    }

    // ── F10: copie, impronte, ripiego, nomi di dispositivo, tetto dei pixel ──

    /// Una PNG che dice di essere grande quanto si chiede: firma e `IHDR`.
    fn png_di(larghezza: u32, altezza: u32) -> Vec<u8> {
        let mut png = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&larghezza.to_be_bytes());
        png.extend_from_slice(&altezza.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        png
    }

    /// `notte` con una versione che la distingue.
    fn notte_v(versione: &str) -> Vec<u8> {
        notte("Notte", versione, 1)
    }

    #[test]
    fn una_skin_installata_oltre_i_pixel_resta_indossata() {
        // Con la 3.2.0 di prova si installava una skin con uno sfondo 8K; il tetto
        // dei pixel nato dopo la rifiutava a ogni avvio, e la finestra diceva
        // che il pacchetto non c'era (R06b-4). Il tetto vale per installare e
        // nello Studio; quel che è già installato resta indossato (D-53).
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        let sorgente = sorgente_con_id("grande");
        let archivio = zip_a_mano(&[
            ("skin.json", sorgente.as_bytes()),
            ("assets/sfondo.png", &png_di(7680, 4320)),
        ]);
        std::fs::write(cartella.join("grande.ryskin"), &archivio).expect("scritta");

        let compilata = compila(dir.path(), "grande").expect("si indossa ancora");
        assert_eq!(compilata.id, "grande");
        assert!(matches!(
            risorsa(dir.path(), "grande/sfondo.png", None),
            Ok(Servita::Piena { .. })
        ));
        // Installarla di nuovo invece no: il tetto vale lì.
        let err = da_installare(&archivio).expect_err("oltre il tetto");
        assert_eq!(err.code().kind().code(), "skin.assetRejected");
    }

    #[test]
    fn il_ripiego_dice_perche() {
        // «Il pacchetto non è nella cartella dei dati» era la frase per ogni
        // guasto: anche per un pacchetto che c'è e non si legge più (R06b-4).
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let assente = compila_o_ripiega(dir.path(), None, "sparita").expect("ripiegata");
        assert_eq!(assente.ripiego.as_deref(), Some("sparita"));
        assert_eq!(assente.ripiego_guasto.as_deref(), Some("skin.notFound"));

        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        let rotta = sorgente_con_id("vecchia").replacen(
            "\"tokens\": {",
            "\"tokens\": { \"color.accents\": \"#ff0000\",",
            1,
        );
        std::fs::write(
            cartella.join("vecchia.ryskin"),
            zip_a_mano(&[("skin.json", rotta.as_bytes())]),
        )
        .expect("scritta");
        let rotta = compila_o_ripiega(dir.path(), None, "vecchia").expect("ripiegata");
        let guasto = rotta.ripiego_guasto.expect("il codice del guasto");
        assert_ne!(guasto, "skin.notFound");
        assert!(guasto.starts_with("skin."), "{guasto}");
    }

    #[test]
    fn una_skin_aux_installata_prima_si_vede_ancora() {
        // `aux` si installava e funzionava; dopo che il formato ha cominciato a
        // rifiutare i nomi dei dispositivi di Windows spariva dall'elenco in
        // silenzio, e non si poteva nemmeno togliere (R04b-6). Il percorso
        // verbatim (`\\?\` su Windows) scrive un file vero col nome del
        // dispositivo, come ci sarebbe arrivato installandolo su Android.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let dati = std::fs::canonicalize(dir.path()).expect("percorso");
        let cartella = dati.join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        let sorgente = sorgente_con_id("aux");
        std::fs::write(
            cartella.join("aux.ryskin"),
            zip_a_mano(&[("skin.json", sorgente.as_bytes())]),
        )
        .expect("scritta");

        let voci = elenco(&dati, "aux").expect("l'elenco");
        let aux = voci.iter().find(|v| v.id == "aux").expect("in elenco");
        assert!(aux.attiva);
        assert!(aux.guasto.is_some(), "{aux:?}");
        let indossata = compila_o_ripiega(&dati, None, "aux").expect("ripiegata");
        assert_eq!(indossata.ripiego.as_deref(), Some("aux"));
        togli_dal_disco(&dati, "aux").expect("tolta");
        assert!(!cartella.join("aux.ryskin").exists());
        // Quel che esce dalla cartella resta fuori, come sempre.
        assert!(togli_dal_disco(&dati, "../aux").is_err());
    }

    #[test]
    fn le_sostituzioni_tengono_tre_copie_per_id() {
        // Fino alla 3.2.0 la copia era una per id: la seconda sostituzione
        // cancellava la prima per sempre, miniatura compresa (R04b-5, D-52).
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let versioni: Vec<Vec<u8>> = ["1.0.0", "2.0.0", "3.0.0", "4.0.0", "5.0.0"]
            .iter()
            .map(|v| notte_v(v))
            .collect();
        installa_file(dir.path(), &versioni[0], false, None).expect("installata");
        for nuova in &versioni[1..] {
            let fatta = installa_file(dir.path(), nuova, true, None).expect("sostituita");
            assert!(fatta.installata.is_some());
        }
        let tenute: Vec<Vec<u8>> = copie(dir.path(), "notte")
            .iter()
            .map(|c| std::fs::read(c).expect("copia"))
            .collect();
        // Le ultime tre sostituite, dalla più recente.
        assert_eq!(
            tenute,
            [
                versioni[3].clone(),
                versioni[2].clone(),
                versioni[1].clone()
            ]
        );
        let ultima = sostituita_di(dir.path(), "notte").expect("l'ultima");
        assert_eq!(ultima.skin.versione, "4.0.0");

        // Una copia di prima, `sostituite/<id>.ryskin`, si ritrova come le altre.
        let altra = dir
            .path()
            .join("skin")
            .join("sostituite")
            .join("alba.ryskin");
        std::fs::write(&altra, notte_v("0.1.0")).expect("scritta");
        assert_eq!(copie(dir.path(), "alba"), [altra]);
    }

    #[test]
    fn disinstallare_toglie_anche_le_copie() {
        // Una copia per ogni id disinstallato restava nella cartella, fino a
        // venti megabyte l'una, e nessun elenco la mostrava (R04b-5, D-52).
        let dir = tempfile::tempdir().expect("cartella temporanea");
        installa_file(dir.path(), &notte_v("1.0.0"), false, None).expect("installata");
        installa_file(dir.path(), &notte_v("2.0.0"), true, None).expect("sostituita");
        let vecchia = dir
            .path()
            .join("skin")
            .join("sostituite")
            .join("notte.ryskin");
        std::fs::write(&vecchia, notte_v("0.1.0")).expect("la copia di prima");
        assert_eq!(copie(dir.path(), "notte").len(), 2);

        togli_dal_disco(dir.path(), "notte").expect("tolta");
        assert!(!dir.path().join("skin").join("notte.ryskin").exists());
        assert!(copie(dir.path(), "notte").is_empty());
        assert!(!vecchia.exists());
    }

    #[test]
    fn rimettere_due_volte_non_torna_indietro() {
        // «Rimettila» con un doppio clic: la seconda chiamata trovava installata
        // quella appena rimessa, la scambiava di nuovo, e lasciava addosso la
        // skin che si voleva togliere (R04b-4).
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let file = dir.path().join("skin").join("notte.ryskin");
        let mia = notte_v("1.0.0");
        let altrui = notte_v("2.0.0");
        installa_file(dir.path(), &mia, false, None).expect("installata");
        let fatta = installa_file(dir.path(), &altrui, true, None).expect("sostituita");
        let impronta = fatta.impronta.expect("l'impronta di quella installata");
        let copia = fatta
            .installata
            .and_then(|v| v.sostituita)
            .expect("la copia");

        let rimessa = std::fs::read(&copia.percorso).expect("la copia");
        let prima = installa_file(dir.path(), &rimessa, true, Some(&impronta)).expect("rimessa");
        assert!(prima.installata.is_some());
        assert_eq!(std::fs::read(&file).expect("letta"), mia);
        // Il secondo clic rilegge il file della copia, come fa la notizia. La
        // copia di quella rimessa non c'è più — è installata — e se ci fosse,
        // l'impronta direbbe che l'installata non è quella da togliere.
        if let Ok(di_nuovo) = std::fs::read(&copia.percorso) {
            let seconda =
                installa_file(dir.path(), &di_nuovo, true, Some(&impronta)).expect("risposta");
            assert!(seconda.installata.is_none(), "{seconda:?}");
        }
        let seconda = installa_file(dir.path(), &altrui, true, Some(&impronta)).expect("risposta");
        assert!(seconda.installata.is_none(), "{seconda:?}");
        assert_eq!(std::fs::read(&file).expect("letta"), mia);
        // E la copia di quella tolta resta, per rimetterla.
        assert_eq!(
            sostituita_di(dir.path(), "notte").map(|s| s.skin.versione),
            Some("2.0.0".to_owned())
        );
    }

    #[test]
    fn il_si_vale_per_la_skin_che_la_domanda_ha_mostrato() {
        // La domanda mostra «Installata: A». Prima del sì un'altra installazione
        // mette B: il sì sostituiva B, che nessuno aveva visto (R04b-5).
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let a = notte_v("1.0.0");
        let b = notte_v("2.0.0");
        let c = notte_v("3.0.0");
        installa_file(dir.path(), &a, false, None).expect("installata");
        let domanda = installa_file(dir.path(), &c, false, None)
            .expect("risposta")
            .da_sostituire
            .expect("la domanda");
        installa_file(dir.path(), &b, true, None).expect("sostituita nel frattempo");
        let risposta =
            installa_file(dir.path(), &c, true, Some(&domanda.impronta)).expect("risposta");
        let nuova = risposta.da_sostituire.expect("una domanda nuova");
        assert_eq!(nuova.presente.versione, "2.0.0");
        assert_eq!(
            std::fs::read(dir.path().join("skin").join("notte.ryskin")).expect("letta"),
            b
        );
    }

    #[test]
    fn una_scelta_che_e_un_dispositivo_non_apre_il_dispositivo() {
        // Una scelta `com1` arriva da un profilo: su Windows 10
        // `skin\com1.ryskin` è la porta seriale, e leggerla poteva bloccare il
        // comando che dà alla finestra la sua impaginazione. Si ripiega prima
        // di toccare il disco, col perché.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for dispositivo in ["com1", "con", "nul"] {
            let indossata = compila_o_ripiega(dir.path(), None, dispositivo).expect("ripiegata");
            assert_eq!(indossata.ripiego.as_deref(), Some(dispositivo));
            assert_eq!(
                indossata.ripiego_guasto.as_deref(),
                Some("skin.manifestInvalid"),
                "{dispositivo}"
            );
        }
    }
}
