//! La skin attiva, compilata dal nucleo.
//!
//! Questo modulo non produce una riga di CSS: la produce `aether-skin`, e qui si
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

use aether_app::settings::{CHIAVE_ACCENTO_DINAMICO, CHIAVE_SKIN};
use aether_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// La skin usata quando non se ne chiede una.
pub const DI_SERIE: &str = "plain";

/// Dove stanno le skin installate.
fn cartella_skin(data_dir: &Path) -> PathBuf {
    data_dir.join("skin")
}

/// L'identificatore è anche un nome di file: non deve poter uscire dalla
/// cartella.
///
/// `..`, le barre e i due punti di un percorso di Windows sono tutto ciò che
/// serve a scrivere altrove. Il formato dichiara già che un id è fatto di
/// lettere, cifre e trattini — qui si controlla, perché questo è il punto in cui
/// diventa un percorso.
fn id_sicuro(id: &str) -> bool {
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
    if id == DI_SERIE {
        return Ok(aether_skin::PLAIN_SOURCE.to_owned());
    }
    if !id_sicuro(id) {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    let percorso = cartella_skin(data_dir).join(format!("{id}.aeskin"));
    let bytes = std::fs::read(&percorso)
        .map_err(|_| AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }))?;
    // `read_skin_package` porta le guardie dell'archivio non fidato — traversal
    // del percorso, zip bomb, tipo mentito — e sono le stesse anche quando il
    // file l'ha messo lì l'utente: una skin scaricata da internet e copiata a
    // mano nella cartella non è più fidata di una arrivata dalla rete.
    let pacchetto = aether_skin::read_skin_package(&bytes)?;
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
    /// `bottom-bar`, `floating` o `compact`.
    pub player: &'static str,
    /// `rail`, `expanded` o `hidden`.
    pub sidebar: &'static str,
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
    /// La classe del registro delle parti che questo nodo porta.
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
fn nodo(zona: &aether_skin::LayoutZone, via: &mut Vec<usize>) -> NodoIpc {
    use aether_skin::LayoutNode;

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
fn misura(size: aether_skin::layout::TrackSize) -> String {
    use aether_skin::layout::TrackSize;
    match size {
        TrackSize::Hug => "hug".to_owned(),
        TrackSize::Fill => "fill".to_owned(),
        TrackSize::Fixed(length) => aether_skin::values::format_length(length),
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

const fn valore_opzione(valore: aether_skin::layout::OptionValue) -> ValoreOpzioneIpc {
    use aether_skin::layout::OptionValue;
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
    /// Ha una variante chiara: senza, l'interruttore del tema non si mostra.
    pub light: bool,
    /// Permette all'accento di seguire la copertina.
    ///
    /// È `capabilities.dynamicAccent`, cioè una dichiarazione dell'autore della
    /// skin, e vince sulla preferenza dell'utente: una skin può essere
    /// costruita attorno al suo accento — `sala` lo è, e infatti dice di no.
    pub dynamic_accent: bool,
    /// Impaginazione e movimento dichiarati.
    pub layout: ImpaginazioneIpc,
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
    /// Ha una variante chiara.
    pub chiara: bool,
}

/// I tre colori della scheda, nella scrittura che il CSS accetta.
fn anteprima(meta: &aether_skin::document::SkinMeta) -> Vec<String> {
    meta.preview
        .map(|colori| {
            colori
                .iter()
                .map(|c| aether_skin::values::format_color(*c))
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
    let documento = aether_skin::parse_skin_json(&sorgente)?;
    let compilata = aether_skin::compile_skin(&documento);
    Ok(SkinIpc {
        id: compilata.id,
        css: compilata.css,
        cost: compilata.cost,
        dynamic_tokens: compilata
            .dynamic_tokens
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        light: documento.capabilities.light,
        dynamic_accent: documento.capabilities.dynamic_accent,
        layout: impaginazione(&documento),
    })
}

/// L'impaginazione dichiarata, o i difetti del formato quando la skin tace.
///
/// I difetti vengono da `SkinLayout::default()` e da `MotionIntensity::Full`,
/// cioè dal formato: ripeterli qui vorrebbe dire avere due posti in cui è
/// scritto cosa succede a chi non dichiara niente.
pub fn impaginazione(documento: &aether_skin::SkinDocument) -> ImpaginazioneIpc {
    use aether_skin::document::{Density, PlayerLayout, SidebarLayout, SkinLayout};

    // Si prende in prestito invece di copiare: da quando `SkinLayout` porta lo
    // scafale non è più `Copy`, e copiare un albero per leggerne tre enum
    // sarebbe pagare la struttura senza usarla.
    let difetto = SkinLayout::default();
    let layout = documento.layout.as_ref().unwrap_or(&difetto);
    ImpaginazioneIpc {
        player: match layout.player {
            PlayerLayout::BottomBar => "bottom-bar",
            PlayerLayout::Floating => "floating",
            PlayerLayout::Compact => "compact",
        },
        sidebar: match layout.sidebar {
            SidebarLayout::Rail => "rail",
            SidebarLayout::Expanded => "expanded",
            SidebarLayout::Hidden => "hidden",
        },
        density: match layout.density {
            Density::Compact => "compact",
            Density::Comfortable => "comfortable",
            Density::Spacious => "spacious",
        },
        motion: documento
            .motion
            .as_ref()
            .map_or(aether_skin::document::MotionIntensity::Full, |m| {
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
    aether_app::settings::read(connection, CHIAVE_SKIN)
        .ok()
        .flatten()
        // Un valore assente o illeggibile vale «quella di serie»: una skin
        // scelta e poi cancellata a mano dal disco non deve impedire l'avvio.
        .filter(|id| id == DI_SERIE || id_sicuro(id))
        .unwrap_or_else(|| DI_SERIE.to_owned())
}

/// Compila una skin.
///
/// Senza `id` compila **quella scelta**, non quella di serie: è ciò che rende la
/// scelta persistente senza che la finestra debba ricordarsela e rimandarla
/// indietro a ogni avvio.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non è di nessuna skin conosciuta; i
/// codici della validazione se il manifest non è valido — cosa che per una skin
/// di serie sarebbe un guasto nostro, e che il test di fedeltà del nucleo scopre
/// prima di qui.
#[tauri::command(async)]
pub fn skin(stato: State<'_, Stato>, id: Option<String>) -> Esito<SkinIpc> {
    con_libreria(&stato, |libreria| {
        let id = id
            .clone()
            .unwrap_or_else(|| skin_scelta(&libreria.connection));
        compila(&libreria.data_dir, &id)
    })
    .map_err(errore)
}

/// Le skin disponibili: quella di serie più quelle installate.
///
/// Una skin installata che non si legge più — archivio corrotto, manifest di un
/// formato futuro — **non fa fallire l'elenco**: sparisce da sola. Un selettore
/// che non si apre perché uno dei suoi elementi è rotto è il modo di rendere
/// irrecuperabile un guasto che riguardava una skin sola.
#[tauri::command(async)]
pub fn skin_elenco(stato: State<'_, Stato>) -> Esito<Vec<VoceSkin>> {
    con_libreria(&stato, |libreria| {
        let scelta = skin_scelta(&libreria.connection);
        let mut voci = Vec::new();

        let plain = aether_skin::plain()?;
        voci.push(VoceSkin {
            id: plain.id.clone(),
            nome: plain.meta.name.clone(),
            autore: plain.meta.author.clone(),
            descrizione: plain.meta.description.clone(),
            di_serie: true,
            attiva: scelta == plain.id,
            anteprima: anteprima(&plain.meta),
            chiara: plain.capabilities.light,
        });

        let dir = cartella_skin(&libreria.data_dir);
        let mut installate: Vec<VoceSkin> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|voce| voce.path())
            .filter(|p| p.extension().is_some_and(|e| e == "aeskin"))
            .filter_map(|p| {
                let bytes = std::fs::read(&p).ok()?;
                let pacchetto = aether_skin::read_skin_package(&bytes).ok()?;
                let documento = pacchetto.document;
                Some(VoceSkin {
                    attiva: scelta == documento.id,
                    id: documento.id,
                    anteprima: anteprima(&documento.meta),
                    chiara: documento.capabilities.light,
                    nome: documento.meta.name,
                    autore: documento.meta.author,
                    descrizione: documento.meta.description,
                    di_serie: false,
                })
            })
            .collect();
        installate.sort_by(|a, b| a.nome.cmp(&b.nome));
        voci.append(&mut installate);
        Ok(voci)
    })
    .map_err(errore)
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
fn installa(data_dir: &Path, bytes: &[u8]) -> Result<VoceSkin, AppError> {
    let pacchetto = aether_skin::read_skin_package(bytes)?;
    let documento = pacchetto.document;
    if documento.id == DI_SERIE || !id_sicuro(&documento.id) {
        return Err(AppError::new(ErrorCode::SkinNotFound {
            id: documento.id.clone(),
        })
        .with_cause("l'identificatore non è utilizzabile come nome di file".to_owned()));
    }
    let dir = cartella_skin(data_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|err| aether_app::files::io_error(&dir.display().to_string(), &err))?;
    let destinazione = dir.join(format!("{}.aeskin", documento.id));
    std::fs::write(&destinazione, bytes)
        .map_err(|err| aether_app::files::io_error(&destinazione.display().to_string(), &err))?;
    Ok(VoceSkin {
        id: documento.id,
        anteprima: anteprima(&documento.meta),
        chiara: documento.capabilities.light,
        nome: documento.meta.name,
        autore: documento.meta.author,
        descrizione: documento.meta.description,
        di_serie: false,
        attiva: false,
    })
}

/// Installa una skin da un file `.aeskin`.
#[tauri::command(async)]
pub fn skin_installa(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    percorso: String,
) -> Esito<VoceSkin> {
    let esito = con_libreria(&stato, |libreria| {
        let bytes =
            std::fs::read(&percorso).map_err(|err| aether_app::files::io_error(&percorso, &err))?;
        installa(&libreria.data_dir, &bytes)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Installa una skin dal suo manifest, senza passare dal disco.
///
/// # Perché non basta esportare
///
/// Lo Studio sapeva scrivere un `.aeskin` da qualche parte sul disco
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
#[tauri::command(async)]
pub fn skin_installa_sorgente(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    sorgente: String,
) -> Esito<VoceSkin> {
    let esito = con_libreria(&stato, |libreria| {
        // `write_skin_package` valida da sé — è la stessa chiamata di
        // `studio_esporta`, e gli errori del manifest fermano qui.
        let bytes = aether_skin::write_skin_package(&aether_skin::package::WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[],
        })?;
        installa(&libreria.data_dir, &bytes)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
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
#[tauri::command(async)]
pub fn skin_disinstalla(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: String,
) -> Esito<SkinIpc> {
    let esito = con_libreria(&stato, |libreria| {
        if id == DI_SERIE || !id_sicuro(&id) {
            return Err(
                AppError::new(ErrorCode::SkinNotFound { id: id.clone() }).with_cause(
                    "la skin di serie è compilata dentro e non si disinstalla".to_owned(),
                ),
            );
        }
        let file = cartella_skin(&libreria.data_dir).join(format!("{id}.aeskin"));
        if !file.exists() {
            return Err(AppError::new(ErrorCode::SkinNotFound { id: id.clone() }));
        }
        std::fs::remove_file(&file)
            .map_err(|err| aether_app::files::io_error(&file.display().to_string(), &err))?;

        // Se era quella indossata, la finestra resterebbe con un foglio che non
        // ha più una sorgente: si torna a quella di serie prima di rispondere,
        // così chi riceve l'esito ha già il CSS da applicare.
        let attiva = skin_scelta(&libreria.connection);
        if attiva == id {
            let compilata = compila(&libreria.data_dir, DI_SERIE)?;
            aether_app::settings::write(&libreria.connection, CHIAVE_SKIN, &compilata.id)?;
            return Ok(compilata);
        }
        compila(&libreria.data_dir, &attiva)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Sceglie la skin attiva e la compila.
///
/// La scelta si scrive **dopo** che la compilazione è riuscita: salvare prima
/// vorrebbe dire poter rendere l'applicazione illeggibile al riavvio scegliendo
/// una skin rotta.
#[tauri::command(async)]
pub fn skin_scegli(app: tauri::AppHandle, stato: State<'_, Stato>, id: String) -> Esito<SkinIpc> {
    let esito = con_libreria(&stato, |libreria| {
        let compilata = compila(&libreria.data_dir, &id)?;

        aether_app::settings::write(&libreria.connection, CHIAVE_SKIN, &compilata.id)?;

        Ok(compilata)
    })
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

// ── L'accento che segue la copertina ────────────────────────────────────────
//
// Tre comandi e nessuna decisione, come tutto il resto di questo modulo: quale
// colore esca dalla copertina lo decide `aether_app::tinta`, se sia leggibile lo
// decide `aether_skin::dinamico`, e qui si legge un'impostazione e si mette in
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
    aether_app::settings::read_json(connection, CHIAVE_ACCENTO_DINAMICO)
        .ok()
        .flatten()
        // Assente o illeggibile vale spento: è la stessa regola di `skin_scelta`
        // — un'impostazione corrotta non deve poter cambiare l'aspetto
        // dell'applicazione in un modo che l'utente non ha chiesto.
        .unwrap_or(false)
}

/// Se l'accento debba seguire la copertina.
#[tauri::command]
pub fn accento_dinamico(stato: State<'_, Stato>) -> Esito<bool> {
    con_libreria(&stato, |libreria| Ok(accento_scelto(&libreria.connection))).map_err(errore)
}

/// Accende o spegne l'accento dinamico. Riporta com'è rimasto.
#[tauri::command(async)]
pub fn accento_dinamico_attiva(stato: State<'_, Stato>, attivo: bool) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        aether_app::settings::write_json(&libreria.connection, CHIAVE_ACCENTO_DINAMICO, &attivo)?;
        Ok(attivo)
    })
    .map_err(errore)
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
/// Solo quelli del database e della lettura della skin. Una copertina che non
/// si decodifica non è un errore: è un disco senza tinta.
///
/// # L'impronta si controlla
///
/// `covers.tinta` compone un percorso dall'impronta e apre quel file, quindi
/// un'impronta che sia in realtà `../../qualcosa` è una lettura arbitraria di
/// file chiesta dalla finestra. Il controllo è lo stesso del protocollo
/// `aether-cover` — [`crate::copertine::e_un_impronta`] — e un'impronta che non
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
#[tauri::command(async)]
pub fn accento_copertina(
    stato: State<'_, Stato>,
    copertina: Option<String>,
    chiaro: bool,
) -> Esito<Option<Vec<VariabileIpc>>> {
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
    // La skin si rilegge a ogni brano invece di tenerla in memoria: è un file
    // da qualche kilobyte, e tenerne una copia vorrebbe dire tenerla allineata
    // a `skin_scegli` — cioè aggiungere uno stato che può divergere per
    // risparmiare un microsecondo ogni canzone. Adesso che la lettura non è più
    // sotto il lucchetto, quel microsecondo lo paga solo chi chiama.
    let sorgente = sorgente(&data_dir, &skin).map_err(errore)?;
    let documento = aether_skin::parse_skin_json(&sorgente).map_err(errore)?;

    let tinta = aether_skin::values::Rgba { r, g, b, a: 1.0 };
    Ok(
        aether_skin::dinamico::accento_dinamico(&documento, tinta, chiaro).map(|variabili| {
            variabili
                .into_iter()
                .map(|(nome, valore)| VariabileIpc { nome, valore })
                .collect()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const STILE: &str = include_str!("../../src/stile.css");
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

    fn compilata() -> aether_skin::CompiledSkin {
        let documento =
            aether_skin::parse_skin_json(aether_skin::PLAIN_SOURCE).expect("la skin di serie");
        aether_skin::compile_skin(&documento)
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
            "\nrigenera con: cargo run -p aether-skin --example compila\n"
        );
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
            "\nrigenera con: cargo run -p aether-skin --example compila\n"
        );
    }

    #[test]
    fn gli_indirizzi_dei_nodi_sono_quelli_del_foglio() {
        // I due `indirizzo()` — quello del compilatore e quello qui — devono
        // dare la stessa stringa, perché uno finisce nel selettore e l'altro
        // nell'attributo. Sono due righe uguali in due crate diversi, e questo
        // test è ciò che impedisce loro di divergere in silenzio.
        let documento =
            aether_skin::parse_skin_json(aether_skin::PLAIN_SOURCE).expect("la skin di serie");
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
        let documento = aether_skin::parse_skin_json(
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
            aether_skin::values::parse_color(configurato),
            aether_skin::values::parse_color(surface),
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
            "con",
            "",
        ] {
            let err = compila(vuota(), cattivo).expect_err("respinta");
            assert_eq!(
                err.code().kind().code(),
                "skin.notFound",
                "«{cattivo}» non è stato respinto"
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
        aether_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""))
    }

    fn pacchetto(sorgente: &str) -> Vec<u8> {
        aether_skin::write_skin_package(&aether_skin::package::WritePackageInput {
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
        assert!(dir.path().join("skin").join("nocturne.aeskin").is_file());

        // Da qui in poi è una skin come le altre: il giro completo — manifest,
        // pacchetto, cartella, compilazione — è quel che questo test prova, e
        // senza `compila` proverebbe solo che un file è stato scritto.
        let compilata = compila(dir.path(), "nocturne").expect("compilata");
        assert_eq!(compilata.id, "nocturne");
        assert!(compilata.css.contains(":root[data-skin='nocturne']"));
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
            serde_json::from_str(aether_skin::PLAIN_SOURCE).expect("la skin di serie");
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
        assert!(voce.chiara);
    }

    #[test]
    fn una_skin_non_puo_prendere_il_posto_di_quella_di_serie() {
        // `plain` viene dal binario e non dalla cartella: un `plain.aeskin`
        // installato sarebbe un file che non si può più togliere dall'elenco e
        // che nessuno legge, perché `sorgente` risponde prima di guardare il
        // disco.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let err =
            installa(dir.path(), &pacchetto(aether_skin::PLAIN_SOURCE)).expect_err("respinta");

        assert_eq!(err.code().kind().code(), "skin.notFound");
        assert!(!dir.path().join("skin").join("plain.aeskin").exists());
    }
}
