//! Lo Skin Studio: il registro, la validazione, la bozza.
//!
//! # Lo Studio non aggiunge potere
//!
//! Rende visibile un contratto che oggi si scopre leggendo Rust. Tutto quel che
//! serve a un editor di skin esiste già nel crate: settantatré token con la
//! loro descrizione, i loro estremi e il flag di obbligatorietà, quattro preset
//! che ne scrivono blocchi coerenti, cinquantuno parti coi loro gruppi, undici
//! effetti col costo dichiarato, un vocabolario chiuso per parte, e
//! `nearest_parts()` per i refusi. Nessuno di questi dati usciva dal processo.
//!
//! Questo modulo non decide niente: legge il registro e chiama `parse_skin`,
//! `check_skin`, `compile_skin`. È la stessa regola dei comandi — nessuna
//! decisione fuori dal nucleo — e vale doppio qui, perché la stessa validazione
//! dovrà girare identica dentro l'APK.
//!
//! # Perché la sorgente di verità è il testo JSON
//!
//! Perché `SkinDocument` **non è riserializzabile**, e non è una svista: sta
//! scritto in `document.rs`, sopra la sua definizione. Un colore validato è una
//! quaterna di canali, e riscriverlo produrrebbe un manifest che quella stessa
//! validazione rifiuta — `rgb(9 9 13)` non è una forma che il formato accetta in
//! ingresso.
//!
//! La conseguenza è l'architettura dello Studio: si edita **il testo**, e la
//! vista a controlli produce modifiche su quel testo. Le due viste di
//! `Rymembra - impalcatura` — clicca o programma — non sono due modelli
//! sincronizzati: sono due tastiere sullo stesso documento. E il documento che
//! esce è il file che si mette in git.

use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rymembra_app::scrittura::scrivi_atomico;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_skin::movimento;
use serde::Serialize;
use tauri::State;

use crate::errore::{ErroreIpc, Esito, errore};
use crate::stato::{Stato, con_libreria};

// ── Il registro ─────────────────────────────────────────────────────────────

/// Un token, come lo mostra l'editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenIpc {
    /// `color.accent`.
    pub id: &'static str,
    /// `--accent`.
    pub css: &'static str,
    /// `color`, `length`, `duration`, `easing`, `number`, `fontStack`, `shadow`.
    pub kind: &'static str,
    /// Il gruppo, per l'albero.
    pub group: &'static str,
    /// La skin di riferimento lo deve dichiarare.
    pub required: bool,
    /// Gli estremi ammessi, dove ci sono: sono i capi del cursore nell'editor.
    ///
    /// Vengono dal registro e non da una tabella qui per la stessa ragione di
    /// `esempio`: il validatore rifiuta quel che esce da questi due numeri, e un
    /// cursore che arrivasse altrove offrirebbe un valore che il documento poi
    /// respinge — cioè un errore che l'interfaccia ha suggerito.
    pub min: Option<f64>,
    /// L'altro capo. Vedi [`Self::min`].
    pub max: Option<f64>,
    /// A cosa serve. È il testo che chi scrive una skin legge nell'editor.
    pub description: &'static str,
}

/// Un preset, come lo mostra l'editor.
///
/// # Perché viaggia come testo e non come valori
///
/// Perché è testo anche all'arrivo. Lo Studio non ha un modello del documento da
/// aggiornare: ha il JSON scritto dall'autore, e applicare un preset vuol dire
/// innestare dei frammenti dentro quel testo con lo stesso `scriviIn` che usa un
/// cursore. Un valore già scritto non ha bisogno che nessuno lo riscriva — e
/// riscriverlo, come dice `document.rs` sopra `SkinDocument`, produrrebbe forme
/// che la validazione rifiuta in ingresso.
///
/// Il gruppo arriva col nome che portano già le righe dei token, dalla stessa
/// funzione: la striscia si costruisce dal gruppo del token selezionato, e due
/// vocabolari diversi vorrebbero dire un confronto fra stringhe che oggi
/// combaciano per caso.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetIpc {
    /// `classico`. Quel che l'interfaccia rimanda indietro.
    pub id: &'static str,
    /// «Classico». Quel che si legge sul bottone.
    pub nome: &'static str,
    /// Il gruppo di token su cui agisce, nel vocabolario di [`TokenIpc::group`].
    pub group: &'static str,
    /// Le scritture: l'id del token, e il suo valore come frammento JSON.
    pub valori: Vec<(&'static str, &'static str)>,
}

/// Una parte, come la mostra l'editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParteIpc {
    /// Il nome, che è anche la classe CSS.
    pub name: &'static str,
    /// Il gruppo, per l'albero.
    pub group: &'static str,
    /// A cosa serve.
    pub description: &'static str,
    /// Ha `::before` libero per un livello, dietro al contenuto.
    pub before: bool,
    /// Ha `::after` libero per un livello, sopra al fondo.
    pub after: bool,
    /// Entra ed esce dal documento come un'unità: ammette `enter` ed `exit`.
    pub presenza: bool,
    /// Può volare fra due schermate: ammette `share`.
    pub condivisa: bool,
}

/// Un effetto, col suo costo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffettoIpc {
    /// `solid`, `dotGrid`, `blurBehind`, …
    pub name: &'static str,
    /// Quanto pesa sul budget della superficie.
    pub cost: u32,
    /// `background`, `clipPath` o `filter`.
    pub target: &'static str,
    /// L'esemplare minimo che il parser accetta, in JSON.
    ///
    /// È quel che «Aggiungi livello» scrive nel documento. Viene da qui e non
    /// dall'editor perché è la **stessa** stringa da cui si ricava il costo: se
    /// un giorno un effetto pretendesse un campo in più, l'editor scriverebbe un
    /// livello valido senza che nessuno se ne ricordi. Un vocabolario chiuso
    /// copiato in due lingue è un vocabolario che diverge.
    pub esempio: &'static str,
    /// Le manopole dell'effetto.
    ///
    /// Senza questa lista, «Aggiungi livello» scriveva l'esemplare e finiva lì:
    /// si otteneva un rettangolo nero e per cambiarne il colore si scendeva nel
    /// JSON. Cioè la vista a controlli non era una seconda tastiera sullo stesso
    /// documento — era una tastiera con meno tasti.
    pub params: Vec<ParametroIpc>,
}

/// Un parametro di un effetto, come lo mostra l'editor.
///
/// Ricalca [`OpzioneIpc`] di proposito: sono due elenchi di manopole, e il
/// pannello che le disegna è lo stesso. La differenza è il vocabolario dei tipi
/// — qui c'è `color` e `length`, là c'è `flag` — perché sono i tipi che i due
/// posti usano davvero.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParametroIpc {
    /// Come si chiama nel documento.
    pub name: &'static str,
    /// `color`, `length`, `angle`, `number`, `stops`, `corners`, `word`,
    /// `point`, `asset`, `flag`.
    pub kind: &'static str,
    /// A cosa serve.
    pub description: &'static str,
    /// Le parole ammesse, per `word`. Vuoto altrimenti.
    pub allowed: Vec<&'static str>,
    /// L'intervallo, per `angle`, `number` e `length`.
    pub min: Option<f64>,
    /// L'altro capo.
    pub max: Option<f64>,
    /// Si può togliere: il crate ha un valore di serie per questo campo.
    pub optional: bool,
}

/// Scorciatoia per tenere la tabella dei parametri leggibile: una riga per manopola.
macro_rules! par {
    ($name:literal, $kind:literal, $optional:literal, $desc:literal) => {
        ParametroIpc {
            name: $name,
            kind: $kind,
            description: $desc,
            allowed: Vec::new(),
            min: None,
            max: None,
            optional: $optional,
        }
    };
    ($name:literal, $kind:literal, $optional:literal, $min:literal, $max:literal, $desc:literal) => {
        ParametroIpc {
            name: $name,
            kind: $kind,
            description: $desc,
            allowed: Vec::new(),
            min: Some($min),
            max: Some($max),
            optional: $optional,
        }
    };
    ($name:literal, $kind:literal, $optional:literal, [$($parola:literal),+], $desc:literal) => {
        ParametroIpc {
            name: $name,
            kind: $kind,
            description: $desc,
            allowed: vec![$($parola),+],
            min: None,
            max: None,
            optional: $optional,
        }
    };
}

/// Le manopole di ogni effetto, nell'ordine in cui si mostrano.
///
/// È una tabella e non una derivazione perché `Effect` è un enum di varianti
/// tipizzate, non un elenco dichiarativo: non c'è niente da cui leggerla a
/// macchina. Sta **qui** e non nell'editor per la stessa ragione di `esempio` —
/// accanto al codice che la deve tenere allineata alle varianti, in Rust, dove
/// aggiungerne una senza aggiornare questa riga è visibile a chi la aggiunge.
fn parametri(nome: &str) -> Vec<ParametroIpc> {
    match nome {
        "solid" => vec![par!("color", "color", false, "La tinta.")],
        "linearGradient" => vec![
            par!(
                "angle",
                "angle",
                true,
                -360.0,
                360.0,
                "Gradi. 180 va dall'alto in basso."
            ),
            par!("stops", "stops", false, "Le fermate, in ordine."),
        ],
        "radialGradient" => vec![
            par!(
                "shape",
                "word",
                true,
                ["circle", "ellipse"],
                "Cerchio o ellisse."
            ),
            par!(
                "at",
                "point",
                true,
                "Dov'è il centro, orizzontale e verticale. Senza, nel mezzo."
            ),
            par!(
                "size",
                "length",
                true,
                0.0,
                400.0,
                "Quanto è grande. Senza, fino all'angolo più lontano."
            ),
            par!("stops", "stops", false, "Le fermate, dal centro."),
        ],
        "conicGradient" => vec![
            par!(
                "from",
                "angle",
                true,
                -360.0,
                360.0,
                "Da che angolo parte il giro."
            ),
            par!(
                "at",
                "point",
                true,
                "Dov'è il centro del giro. Senza, nel mezzo."
            ),
            par!("stops", "stops", false, "Le fermate, in senso orario."),
        ],
        "hairlineGrid" => vec![
            par!("color", "color", false, "Il colore delle linee."),
            par!("cell", "length", false, 1.0, 200.0, "Il lato della cella."),
            par!(
                "cellY",
                "length",
                true,
                1.0,
                200.0,
                "L'altezza, se diversa dal lato."
            ),
            par!(
                "thickness",
                "length",
                true,
                0.0,
                8.0,
                "Lo spessore delle linee."
            ),
        ],
        "scanlines" => vec![
            par!("color", "color", false, "Il colore della riga."),
            par!("line", "length", false, 0.0, 32.0, "Quanto è alta la riga."),
            par!(
                "gap",
                "length",
                false,
                0.0,
                64.0,
                "Quanto la separa dalla prossima."
            ),
        ],
        "stripes" => vec![
            par!("angle", "angle", true, -360.0, 360.0, "L'inclinazione."),
            par!("color", "color", false, "La striscia."),
            par!("background", "color", false, "Quel che sta fra le strisce."),
            par!(
                "width",
                "length",
                false,
                1.0,
                96.0,
                "La larghezza di una striscia."
            ),
        ],
        "dotGrid" => vec![
            par!("color", "color", false, "Il colore dei punti."),
            par!(
                "spacing",
                "length",
                false,
                2.0,
                96.0,
                "Quanto distano fra loro."
            ),
            par!("dot", "length", false, 0.0, 16.0, "Quanto sono grossi."),
        ],
        "vignette" => vec![
            par!(
                "color",
                "color",
                false,
                "Il colore che si chiude sui bordi."
            ),
            par!(
                "start",
                "number",
                true,
                0.0,
                100.0,
                "Da che percentuale del raggio comincia."
            ),
        ],
        "chamfer" => vec![
            par!("size", "length", false, 0.0, 64.0, "Quanto taglia."),
            par!(
                "corners",
                "corners",
                true,
                "Quali angoli. Senza, in alto a destra e in basso a sinistra."
            ),
        ],
        "blurBehind" => vec![
            par!(
                "radius",
                "length",
                false,
                0.0,
                64.0,
                "Il raggio della sfocatura."
            ),
            par!(
                "saturate",
                "number",
                true,
                0.0,
                400.0,
                "Quanto satura quel che sta dietro."
            ),
        ],
        "image" => vec![
            par!(
                "src",
                "asset",
                false,
                "Un'immagine del pacchetto: png, jpg o webp."
            ),
            par!(
                "size",
                "word",
                true,
                ["cover", "contain", "auto"],
                "Come riempie: tutta coperta, tutta dentro, o com'è. Anche una lunghezza, per le mattonelle."
            ),
            par!("repeat", "flag", true, "Si ripete a mattonelle."),
            par!(
                "position",
                "point",
                true,
                "Dove si appoggia. Senza, nel mezzo."
            ),
        ],
        _ => Vec::new(),
    }
}

/// Una manopola di widget, come la mostra l'editor.
///
/// I tre casi non si fondono in uno: `kind` dice quale controllo disegnare, e
/// gli altri campi sono quelli che quel controllo usa. È il dividendo della
/// disciplina «una forma di controllo per tipo del crate» — la vista nuova non
/// ha bisogno di un controllo nuovo, riusa il segmentato, la casella e il
/// cursore che l'ispettore ha già.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpzioneIpc {
    /// Come si chiama nel documento.
    pub name: &'static str,
    /// `flag`, `word` o `count`.
    pub kind: &'static str,
    /// A cosa serve.
    pub description: &'static str,
    /// Il valore di serie, nella forma che JSON già ha.
    pub default: crate::skin::ValoreOpzioneIpc,
    /// Le parole ammesse. Vuoto per gli altri due tipi.
    pub allowed: Vec<&'static str>,
    /// L'intervallo, per un conteggio.
    pub min: Option<u32>,
    /// L'intervallo, per un conteggio.
    pub max: Option<u32>,
}

/// Un widget dello scafale, come lo mostra l'editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetIpc {
    /// Il nome nel documento.
    pub name: &'static str,
    /// Il gruppo, per la tavolozza.
    pub group: &'static str,
    /// A cosa serve.
    pub description: &'static str,
    /// La classe del registro delle parti che questo widget porta.
    pub part: Option<&'static str>,
    /// `no`, `yes`, o il nome del gruppo di cui deve esserci almeno un membro.
    pub essential: &'static str,
    /// Ne esiste al massimo uno.
    pub singleton: bool,
    /// In quali zone ci sta: `row`, `column`, `scroll`.
    pub fits: Vec<&'static str>,
    /// Quanto pesa sul budget dello scafale.
    pub cost: u32,
    /// Le manopole.
    pub options: Vec<OpzioneIpc>,
}

/// Il vocabolario dello scafale: le parole che una zona può usare.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabolarioIpc {
    /// `row`, `column`, `scroll`.
    pub zones: Vec<&'static str>,
    /// `none`, `xs`, `s`, `m`, `l`, `xl`.
    pub gaps: Vec<&'static str>,
    /// `start`, `center`, `end`, `stretch`.
    pub aligns: Vec<&'static str>,
    /// `start`, `center`, `end`, `between`.
    pub spreads: Vec<&'static str>,
}

/// Uno stato del DOM, come lo mostra l'editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoIpc {
    /// Il nome nel documento: `hover`, `current`, `playing`.
    pub name: &'static str,
    /// Il selettore che lo riconosce, per una parte che prende il fuoco da sé.
    pub selector: &'static str,
    /// Il selettore per una parte il cui fuoco lo prende un figlio.
    pub dentro: &'static str,
}

/// Un innesco del movimento, con quel che ammette.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InnescoIpc {
    /// `enter`, `exit`, `share`, `inView`, `change`, o uno stato.
    pub name: &'static str,
    /// Ammette un ciclo infinito: solo gli stati.
    pub ciclo: bool,
    /// Si realizza come transizione di vista: vale sulle presenze e sulle
    /// condivise.
    pub vista: bool,
    /// È uno stato del DOM.
    pub stato: bool,
}

/// Un campo di un fotogramma, coi suoi estremi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampoFotogrammaIpc {
    /// Come si chiama nel documento.
    pub name: &'static str,
    /// `number`, `offset` (px o %), `angle`, `length`, `color`, `shadow`, `clip`.
    pub kind: &'static str,
    /// Il valore di riposo: quello di un elemento che non si anima.
    pub riposo: Option<f64>,
    /// Gli estremi che il validatore ammette.
    pub min: Option<f64>,
    /// L'altro capo.
    pub max: Option<f64>,
}

/// Un'animazione pronta, come la offre l'editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimazioneProntaIpc {
    /// Il nome proposto nel documento.
    pub id: &'static str,
    /// Il nome che si legge.
    pub nome: &'static str,
    /// L'innesco per cui è pensata.
    pub innesco: &'static str,
    /// La dichiarazione, come frammento JSON.
    pub valore: &'static str,
}

/// I campi di un fotogramma, nell'ordine del documento.
///
/// Una tabella qui, come quella dei parametri degli effetti, e per la stessa
/// ragione: `AnimFrame` è una struttura tipizzata e non un elenco da leggere a
/// macchina. Gli estremi sono quelli di `document.rs`, e un test li fa passare
/// dal parser uno per uno: un estremo scritto qui e rifiutato là sarebbe un
/// cursore che offre un valore che il documento respinge.
fn campi_fotogramma() -> Vec<CampoFotogrammaIpc> {
    let campo = |name, kind, riposo, estremi: Option<(f64, f64)>| CampoFotogrammaIpc {
        name,
        kind,
        riposo,
        min: estremi.map(|(min, _)| min),
        max: estremi.map(|(_, max)| max),
    };
    vec![
        campo("opacity", "number", Some(1.0), Some((0.0, 1.0))),
        campo("translateX", "offset", Some(0.0), Some((-400.0, 400.0))),
        campo("translateY", "offset", Some(0.0), Some((-400.0, 400.0))),
        campo("scale", "number", Some(1.0), Some((0.0, 3.0))),
        campo("scaleX", "number", Some(1.0), Some((0.0, 3.0))),
        campo("scaleY", "number", Some(1.0), Some((0.0, 3.0))),
        campo("rotate", "angle", Some(0.0), Some((-720.0, 720.0))),
        campo("skewX", "angle", Some(0.0), Some((-45.0, 45.0))),
        campo("skewY", "angle", Some(0.0), Some((-45.0, 45.0))),
        campo("blur", "length", Some(0.0), Some((0.0, 40.0))),
        campo("brightness", "number", Some(1.0), Some((0.0, 3.0))),
        campo("saturate", "number", Some(1.0), Some((0.0, 3.0))),
        campo("hueRotate", "angle", Some(0.0), Some((-360.0, 360.0))),
        campo("backgroundColor", "color", None, None),
        campo("color", "color", None, None),
        campo("borderColor", "color", None, None),
        campo("shadow", "shadow", None, None),
        campo("clip", "clip", None, None),
    ]
}

/// Il vocabolario completo: token, parti, effetti, widget.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistroIpc {
    /// I token.
    pub tokens: Vec<TokenIpc>,
    /// I blocchi di valori pronti, per gruppo di token.
    pub presets: Vec<PresetIpc>,
    /// Le parti.
    pub parts: Vec<ParteIpc>,
    /// Gli effetti.
    pub effects: Vec<EffettoIpc>,
    /// I widget che uno scafale può montare.
    pub widgets: Vec<WidgetIpc>,
    /// Le parole che una zona può usare.
    pub vocabolario: VocabolarioIpc,
    /// Il budget di costo di una singola superficie.
    pub budget: u32,
    /// Il budget di costo dello scafale, che è un budget diverso.
    pub shell_budget: u32,
    /// La versione del formato che questo binario capisce.
    pub format: u32,
    /// La soglia di contrasto sotto cui parte un avviso.
    pub contrasto_minimo: f64,

    // ── I tetti del movimento ───────────────────────────────────────────────
    //
    // Uno per uno da `rymembra_skin::movimento`, e non da una tabella scritta
    // qui. È la stessa promessa di `TokenIpc::min`/`max`: il validatore rifiuta
    // quel che esce da questi numeri, e un pannello che ne conoscesse di
    // diversi offrirebbe valori che il documento poi respinge — cioè un errore
    // che l'interfaccia ha suggerito. Finché non c'erano, lo Studio li teneva
    // ricopiati a mano in cima a `studio/Movimento.tsx`, con scritto che la
    // strada giusta era questa.
    /// Quante animazioni un documento può dichiarare.
    pub max_animazioni: usize,
    /// Quanti fotogrammi ha un'animazione, al minimo.
    pub min_fotogrammi: usize,
    /// Quanti fotogrammi ha un'animazione, al massimo.
    pub max_fotogrammi: usize,
    /// Quanto può durare un'animazione, in millisecondi.
    pub max_durata_ms: f64,
    /// Quanto può attendere prima di partire, in millisecondi.
    pub max_ritardo_ms: f64,
    /// Quante volte può ripetersi, o «infinite» sugli stati.
    pub max_iterazioni: u32,
    /// Il budget del movimento di una parte, che è un terzo budget ancora.
    pub motion_budget: u32,
    /// Il budget del movimento della finestra intera.
    pub window_motion_budget: u32,
    /// Quanto costa un'animazione prima delle ripetizioni.
    ///
    /// È il peso di [`CostClass::Composited`](rymembra_skin::effects::CostClass),
    /// cioè la prima metà di `animation_cost`. Viaggia perché il pannello del
    /// movimento mostra il costo di ogni animazione mentre la si scrive, e
    /// senza questo numero se lo dovrebbe inventare: il verdetto resta del
    /// nucleo — l'avviso `costBudget` — ma il numero che si legge accanto alla
    /// manopola deve venire dallo stesso posto, o i due si contraddicono.
    pub peso_composito: u32,
    /// I versi ammessi, come li scrive `AnimDirection::as_str`.
    pub versi: Vec<&'static str>,
    /// I trigger ammessi, come li scrive `AnimTrigger::as_str`.
    pub trigger: Vec<&'static str>,
    /// Gli stati del DOM, nell'ordine in cui il compilatore li emette.
    pub stati: Vec<StatoIpc>,
    /// Gli inneschi, con quel che ammettono.
    pub inneschi: Vec<InnescoIpc>,
    /// I campi di un fotogramma, coi loro estremi.
    pub fotogramma: Vec<CampoFotogrammaIpc>,
    /// Le animazioni pronte, da `preset.rs`.
    pub animazioni_pronte: Vec<AnimazioneProntaIpc>,
    /// Quel che un'animazione tiene prima e dopo: `both`, `forwards`, …
    pub riempimenti: Vec<&'static str>,
    /// Le sorgenti dinamiche di un colore: i colori che la copertina offre.
    pub sorgenti: Vec<&'static str>,
    /// Le chiavi che il formato accetta, per contesto: il completamento del
    /// codice le propone, dalle stesse liste con cui il validatore rifiuta.
    pub chiavi: std::collections::BTreeMap<&'static str, Vec<&'static str>>,
    /// Le parole chiuse di un campo, per nome del campo.
    pub parole: std::collections::BTreeMap<&'static str, Vec<&'static str>>,
}

/// Il nome stabile di un gruppo di token.
///
/// **Un nome stabile, non una parola da leggere.** Qui c'erano «Tipografia»,
/// «Superfici», «Profondità»: tredici stringhe italiane che attraversavano
/// l'IPC e finivano tali e quali sullo schermo. Con l'interfaccia in inglese
/// lo Studio si leggeva per metà in una lingua e per metà nell'altra —
/// un'intestazione che dice CORNICE sopra una scheda che dice «The icon-only
/// button», che è quella frase lì tradotta.
///
/// La forma giusta è quella che il resto del programma usa già da sempre:
/// `Fonte::nome` dà la chiave, `Fonte::etichetta` la parola. Di là passa la
/// chiave — che serve anche a **confrontare**, perché la striscia dei preset
/// si costruisce paragonando `preset.group` col gruppo del token aperto — e la
/// parola la sceglie la finestra, con `studio.group.<chiave>`.
const fn nome_gruppo_token(group: rymembra_skin::tokens::TokenGroup) -> &'static str {
    use rymembra_skin::tokens::TokenGroup as G;
    match group {
        G::Typography => "typography",
        G::Surface => "surface",
        G::Text => "text",
        G::Accent => "accent",
        G::Status => "status",
        G::Chrome => "chrome",
        G::Layout => "layout",
        G::Rhythm => "rhythm",
        G::Geometry => "geometry",
        G::Depth => "depth",
        G::Elevation => "elevation",
        G::Motion => "motion",
        G::Canvas => "canvas",
    }
}

const fn nome_tipo(kind: rymembra_skin::tokens::TokenKind) -> &'static str {
    use rymembra_skin::tokens::TokenKind as K;
    match kind {
        K::Color => "color",
        K::Length => "length",
        K::Duration => "duration",
        K::Easing => "easing",
        K::Number => "number",
        K::FontStack => "fontStack",
        K::Shadow => "shadow",
    }
}

/// Il nome stabile di un gruppo di parti.
///
/// Come [`nome_gruppo_token`], e per la stessa ragione: di qui passa la
/// chiave, la parola la sceglie la finestra. Le chiavi sono le stesse che
/// `WidgetGroup::as_str` usa dove i due vocabolari si sovrappongono — `shell`
/// è la cornice tanto per un widget quanto per una parte — così una traduzione
/// sola serve tutti e due.
const fn nome_gruppo_parte(group: rymembra_skin::parts::PartGroup) -> &'static str {
    use rymembra_skin::parts::PartGroup as G;
    match group {
        G::Shell => "shell",
        G::Nav => "nav",
        G::Page => "page",
        G::Controls => "controls",
        G::Lists => "lists",
        G::Cards => "cards",
        G::Player => "player",
        G::Column => "column",
        G::NowPlaying => "nowPlaying",
        G::Overlays => "overlays",
    }
}

/// Il vocabolario che una skin può usare.
///
/// Statico: si chiede una volta all'apertura dello Studio e non cambia più
/// finché il binario è quello. È il contratto, ed è compilato dentro.
#[tauri::command]
#[must_use]
pub fn studio_registro() -> RegistroIpc {
    RegistroIpc {
        tokens: rymembra_skin::TOKENS
            .iter()
            .map(|def| TokenIpc {
                id: def.id,
                css: def.css,
                kind: nome_tipo(def.kind),
                group: nome_gruppo_token(def.group),
                required: def.required,
                min: def.limiti.map(|(min, _)| min),
                max: def.limiti.map(|(_, max)| max),
                description: def.description,
            })
            .collect(),
        presets: rymembra_skin::PRESETS
            .iter()
            .map(|def| PresetIpc {
                id: def.id,
                nome: def.nome,
                group: nome_gruppo_token(def.group),
                valori: def.valori.to_vec(),
            })
            .collect(),
        parts: rymembra_skin::parts::PARTS
            .iter()
            .map(|def| ParteIpc {
                name: def.name,
                group: nome_gruppo_parte(def.group),
                description: def.description,
                before: def.before,
                after: def.after,
                presenza: def.presenza,
                condivisa: def.condivisa,
            })
            .collect(),
        effects: rymembra_skin::Effect::NAMES
            .iter()
            .filter_map(|nome| esempio(nome))
            .collect(),
        widgets: rymembra_skin::WIDGETS.iter().map(widget).collect(),
        vocabolario: vocabolario(),
        budget: rymembra_skin::SURFACE_COST_BUDGET,
        shell_budget: rymembra_skin::SHELL_COST_BUDGET,
        format: rymembra_skin::SKIN_FORMAT_VERSION,
        contrasto_minimo: rymembra_skin::CONTRASTO_MINIMO,
        max_animazioni: movimento::MAX_ANIMAZIONI,
        min_fotogrammi: movimento::MIN_FOTOGRAMMI,
        max_fotogrammi: movimento::MAX_FOTOGRAMMI,
        max_durata_ms: movimento::MAX_DURATA_MS,
        max_ritardo_ms: movimento::MAX_RITARDO_MS,
        max_iterazioni: movimento::MAX_ITERAZIONI,
        motion_budget: movimento::MOTION_COST_BUDGET,
        window_motion_budget: movimento::WINDOW_MOTION_BUDGET,
        peso_composito: rymembra_skin::effects::CostClass::Composited.weight(),
        // Dagli `ALL` degli enum, come il vocabolario dello scafale poco più
        // sotto: una parola nuova nel crate compare nell'editor senza che
        // nessuno se ne ricordi.
        versi: movimento::AnimDirection::ALL
            .iter()
            .map(|verso| verso.as_str())
            .collect(),
        trigger: movimento::AnimTrigger::nomi(),
        stati: rymembra_skin::PartState::ALL
            .iter()
            .map(|stato| StatoIpc {
                name: stato.as_str(),
                selector: stato.selector(rymembra_skin::stati::Fuoco::Proprio),
                dentro: stato.selector(rymembra_skin::stati::Fuoco::Dentro),
            })
            .collect(),
        inneschi: movimento::AnimTrigger::ALL
            .iter()
            .map(|innesco| InnescoIpc {
                name: innesco.as_str(),
                ciclo: innesco.ammette_ciclo(),
                vista: innesco.di_vista(),
                stato: matches!(innesco, movimento::AnimTrigger::State(_)),
            })
            .collect(),
        fotogramma: campi_fotogramma(),
        animazioni_pronte: rymembra_skin::ANIMAZIONI_PRONTE
            .iter()
            .map(|pronta| AnimazioneProntaIpc {
                id: pronta.id,
                nome: pronta.nome,
                innesco: pronta.innesco,
                valore: pronta.valore,
            })
            .collect(),
        riempimenti: movimento::Riempimento::ALL
            .iter()
            .map(|r| r.as_str())
            .collect(),
        sorgenti: rymembra_skin::tokens::DynamicSource::ALL
            .iter()
            .map(|s| s.as_str())
            .collect(),
        chiavi: rymembra_skin::chiavi_schema()
            .into_iter()
            .map(|(contesto, chiavi)| (contesto, chiavi.to_vec()))
            .collect(),
        parole: rymembra_skin::parole_schema().into_iter().collect(),
    }
}

/// Un widget del registro, per la tavolozza dell'editor.
fn widget(def: &'static rymembra_skin::WidgetDef) -> WidgetIpc {
    use rymembra_skin::layout::Essential;

    WidgetIpc {
        name: def.name,
        group: def.group.as_str(),
        description: def.description,
        part: def.part,
        essential: match def.essential {
            Essential::No => "no",
            Essential::Yes => "yes",
            Essential::OneOf(gruppo) => gruppo,
        },
        singleton: def.singleton,
        fits: def.fits.iter().map(|k| k.as_str()).collect(),
        cost: def.cost,
        options: def.options.iter().map(opzione).collect(),
    }
}

fn opzione(def: &'static rymembra_skin::layout::WidgetOption) -> OpzioneIpc {
    use crate::skin::ValoreOpzioneIpc as V;
    use rymembra_skin::layout::OptionKind;

    let (kind, default, allowed, min, max) = match def.kind {
        OptionKind::Flag { default } => ("flag", V::Flag(default), Vec::new(), None, None),
        OptionKind::Word { allowed, default } => {
            ("word", V::Word(default), allowed.to_vec(), None, None)
        }
        OptionKind::Count { min, max, default } => {
            ("count", V::Count(default), Vec::new(), Some(min), Some(max))
        }
    };
    OpzioneIpc {
        name: def.name,
        kind,
        description: def.description,
        default,
        allowed,
        min,
        max,
    }
}

/// Le parole che una zona può usare.
///
/// Vengono dai `ALL` degli enum e non da una lista scritta qui: una parola nuova
/// nel crate compare nell'editor senza che nessuno se ne ricordi, che è la
/// stessa promessa del registro dei token.
fn vocabolario() -> VocabolarioIpc {
    use rymembra_skin::layout::{Align, GapStep, Spread, ZoneKind};
    VocabolarioIpc {
        zones: ZoneKind::ALL.iter().map(|k| k.as_str()).collect(),
        gaps: GapStep::ALL.iter().map(|g| g.as_str()).collect(),
        aligns: Align::ALL.iter().map(|a| a.as_str()).collect(),
        spreads: Spread::ALL.iter().map(|s| s.as_str()).collect(),
    }
}

/// Il costo e la destinazione di un effetto, da un esemplare minimo.
///
/// Costruire l'esemplare passando dal parser invece che dal costruttore tiene
/// una promessa: se un effetto nuovo nasce e questa tabella non lo conosce, la
/// riga sparisce dall'elenco invece di mentire sul suo costo.
fn esempio(nome: &str) -> Option<EffettoIpc> {
    let minimo = match nome {
        "solid" => r##"{"effect":"solid","color":"#000"}"##,
        "linearGradient" => {
            r##"{"effect":"linearGradient","stops":[{"color":"#000"},{"color":"#fff"}]}"##
        }
        "radialGradient" => {
            r##"{"effect":"radialGradient","stops":[{"color":"#000"},{"color":"#fff"}]}"##
        }
        "conicGradient" => {
            r##"{"effect":"conicGradient","stops":[{"color":"#000"},{"color":"#fff"}]}"##
        }
        "hairlineGrid" => r##"{"effect":"hairlineGrid","color":"#000","cell":"36px"}"##,
        "scanlines" => r##"{"effect":"scanlines","color":"#000","line":"1px","gap":"3px"}"##,
        "stripes" => r##"{"effect":"stripes","color":"#000","background":"#fff","width":"8px"}"##,
        "dotGrid" => r##"{"effect":"dotGrid","color":"#000","spacing":"8px","dot":"1px"}"##,
        "vignette" => r##"{"effect":"vignette","color":"#000"}"##,
        "chamfer" => r##"{"effect":"chamfer","size":"10px"}"##,
        "blurBehind" => r##"{"effect":"blurBehind","radius":"24px"}"##,
        "image" => r##"{"effect":"image","src":"trama.png"}"##,
        _ => return None,
    };
    // Un documento minimo che dichiara quell'effetto come motivo: è la via più
    // corta per ottenere un `Effect` senza esporre i suoi costruttori.
    let documento = format!(
        r##"{{"format":2,"id":"esempio","meta":{{"name":"E","author":"A","version":"1.0.0"}},
             "tokens":{{}},"patterns":{{"x":{minimo}}}}}"##
    );
    let skin = rymembra_skin::parse_skin_json(&documento).ok()?;
    let (_, effetto) = skin.patterns.first()?;
    Some(EffettoIpc {
        name: effetto.name(),
        cost: effetto.cost().weight(),
        esempio: minimo,
        params: parametri(effetto.name()),
        target: match effetto.target() {
            rymembra_skin::effects::EffectTarget::Background => "background",
            rymembra_skin::effects::EffectTarget::ClipPath => "clipPath",
            rymembra_skin::effects::EffectTarget::Filter => "filter",
        },
    })
}

// ── La validazione ──────────────────────────────────────────────────────────

/// Un problema che blocca.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemaIpc {
    /// Il codice del catalogo.
    pub code: String,
    /// Dove, in forma di percorso nel documento.
    pub path: String,
    /// Cosa non va, e cosa scrivere al suo posto.
    pub message: String,
    /// Il nome che forse si voleva scrivere, quando ce n'è uno vicino.
    pub forse: Vec<String>,
    /// La riga in cui è scritto, da uno.
    ///
    /// `None` soltanto quando nemmeno la radice del documento esiste, cioè su un
    /// testo vuoto. Prima non c'era affatto, e la finestra la indovinava
    /// cercando l'ultimo pezzo del percorso col primo `indexOf` che
    /// corrispondeva: su `parts.x.background.0.stops.1.color` finiva a
    /// sottolineare la prima riga che nominasse un colore qualunque.
    pub riga: Option<u32>,
    /// La colonna, da uno, in unità UTF-16 come le conta la `<textarea>`.
    pub colonna: Option<u32>,
}

/// Un avviso, che non blocca.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvvisoIpc {
    /// `missingRequiredToken`, `unkeptCapability`, `unusedPattern`,
    /// `unusedPrefab`, `unusedAnimation`, `costBudget`, `contrast`.
    ///
    /// L'elenco è quello di `nome_avviso`, che è un `match` esaustivo: un
    /// genere nuovo nel nucleo non compila finché non passa di là. Qui però
    /// resta prosa, e questa riga era rimasta indietro di due generi — vale la
    /// pena rileggerla insieme all'unione di `Avviso["kind"]` in `ipc.ts`, che
    /// è l'unico posto dove la stessa svista non la vede nessun compilatore.
    pub kind: &'static str,
    /// Dove.
    pub path: String,
    /// Cosa.
    pub message: String,
    /// La riga in cui è scritto, da uno. Come per [`ProblemaIpc`]: un avviso ha
    /// un percorso, quindi ha un posto, e il bottone che ci porta è lo stesso.
    pub riga: Option<u32>,
    /// La colonna, da uno.
    pub colonna: Option<u32>,
}

/// Una coppia di colori misurata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContrastoIpc {
    /// Il token davanti.
    pub davanti: &'static str,
    /// Il token dietro.
    pub dietro: &'static str,
    /// Il rapporto nel tema di base.
    pub base: f64,
    /// Il rapporto nella variante, se c'è.
    pub variante: Option<f64>,
    /// Passa la soglia in tutti i temi misurati.
    pub passa: bool,
}

/// Quanto costa una parte, sui suoi due budget.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostoParteIpc {
    /// La parte.
    pub parte: &'static str,
    /// Motivi, livelli e filtri: il budget della superficie.
    pub superficie: u32,
    /// Le animazioni assegnate: il budget del movimento.
    pub movimento: u32,
}

/// L'esito di una validazione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidazioneIpc {
    /// Vuoto se il documento è valido.
    pub errori: Vec<ProblemaIpc>,
    /// Gli avvisi. Non bloccano: una skin con un avviso è una skin che funziona.
    pub avvisi: Vec<AvvisoIpc>,
    /// La tabella dei contrasti, tutta, non solo le righe che non passano.
    pub contrasti: Vec<ContrastoIpc>,
    /// Quante volte ogni colore della tavolozza è riferito.
    pub tavolozza: Vec<(String, usize)>,
    /// Il foglio compilato. Vuoto quando ci sono errori.
    pub css: String,
    /// L'impaginazione, con lo scafale completo.
    ///
    /// `None` quando ci sono errori: la vista Impagina resta all'ultimo albero
    /// valido, come l'anteprima resta all'ultimo foglio valido. Un editor che si
    /// svuota a metà di una parentesi costringe a scrivere in fretta.
    pub layout: Option<crate::skin::ImpaginazioneIpc>,
    /// Il costo totale di motivi e parti.
    pub costo: u32,
    /// Quanti pezzi dell'app lo scafale monta, sul suo budget separato.
    pub costo_scafale: u32,
    /// Quanto costa il movimento, sommato su tutte le parti animate.
    ///
    /// Terzo numero e terzo budget. Il compilatore lo calcolava da quando le
    /// animazioni nominate esistono e non lo leggeva nessuno: un numero
    /// prodotto e buttato via è una promessa che il crate faceva senza
    /// mantenerla. Il verdetto resta di `check_skin`, che il budget lo applica
    /// **per parte**; questa è la somma, cioè il numero che sta accanto agli
    /// altri due.
    pub costo_movimento: u32,
    /// Quante parti la skin ridisegna.
    pub parti: usize,
    /// Il costo di ogni parte ridisegnata: la superficie e il movimento.
    pub costi: Vec<CostoParteIpc>,
    /// Quanto movimento può essere in corso nello stesso momento, sulla
    /// finestra intera: il numero che il budget della finestra misura.
    pub costo_finestra: u32,
    /// Le risorse che il documento nomina.
    pub risorse: Vec<String>,
    /// I token che seguono la copertina.
    pub dinamici: Vec<String>,
    /// Quanti millisecondi ci ha messo a compilare.
    pub compilato_ms: u128,
    /// L'id sotto cui il foglio è compilato: quel che la tela scrive in
    /// `data-skin`. Vuoto quando ci sono errori, perché non c'è un foglio.
    ///
    /// È quello della bozza e non quello del documento — vedi
    /// [`radice_della_bozza`] — e viaggia **col** foglio perché i due devono
    /// dire la stessa cosa: fino alla 3.0 la tela scriveva l'id della bozza e
    /// il foglio usava quello del documento, e un documento che cambiava `id`
    /// lasciava la tela senza una regola che la trovasse finché la rinomina
    /// non arrivava.
    pub radice: String,
}

const fn nome_avviso(kind: rymembra_skin::WarningKind) -> &'static str {
    use rymembra_skin::WarningKind as W;
    match kind {
        W::MissingRequiredToken => "missingRequiredToken",
        W::UnkeptCapability => "unkeptCapability",
        W::UnusedPattern => "unusedPattern",
        W::UnusedPrefab => "unusedPrefab",
        W::UnusedAnimation => "unusedAnimation",
        W::CostBudget => "costBudget",
        W::Contrast => "contrast",
        W::RestingLoop => "restingLoop",
    }
}

// ── I ricordi: le ultime risposte, per testo ────────────────────────────────

/// Quante risposte si ricordano per comando.
///
/// Poche, e non è avarizia: il caso che le fa servire è l'annullo — Ctrl+Z,
/// Ctrl+Y, Ctrl+Z su tre testi — e il ritorno da «Prova sull'app», che
/// rimanda lo stesso testo di prima. Una risposta porta dentro un foglio da
/// qualche decina di kilobyte, e sei sono già più di quanti passi si rifanno
/// in fila.
const QUANTI_RICORDI: usize = 6;

/// Una risposta ricordata, col testo e la bozza a cui risponde.
struct Ricordo<V> {
    impronta: u64,
    bozza: Option<String>,
    testo: String,
    valore: V,
}

/// Le ultime risposte di un comando dello Studio che non dipende da nient'altro
/// che dal testo e dalla bozza.
///
/// # Perché si ricorda
///
/// `studio_valida` è `parse` + `check` + `compile` su tutto il documento, e
/// girava sul filo della finestra: a ogni passo dell'annullo, a ogni ritorno
/// dalla prova, la finestra aspettava il nucleo per un documento che aveva
/// già visto un secondo prima. Il nucleo è deterministico — stesso testo,
/// stessa bozza, stessa risposta — quindi la risposta si può tenere.
///
/// L'impronta serve a cercare in fretta; il confronto vero è sul testo intero,
/// così due testi con la stessa impronta non si scambiano mai la risposta.
struct Ricordi<V> {
    voci: Mutex<Vec<Ricordo<V>>>,
}

impl<V: Clone> Ricordi<V> {
    const fn nuovi() -> Self {
        Self {
            voci: Mutex::new(Vec::new()),
        }
    }

    /// La risposta ricordata, o quella calcolata adesso — e ricordata.
    ///
    /// Il calcolo sta **fuori** dal lucchetto: due validazioni diverse non si
    /// aspettano a vicenda, e due uguali al peggio calcolano due volte.
    fn o_calcola(&self, bozza: Option<&str>, testo: &str, calcola: impl FnOnce() -> V) -> V {
        let impronta = {
            let mut h = std::hash::DefaultHasher::new();
            bozza.hash(&mut h);
            testo.hash(&mut h);
            h.finish()
        };
        let stesso = |r: &Ricordo<V>| {
            r.impronta == impronta && r.bozza.as_deref() == bozza && r.testo == testo
        };
        {
            let voci = self.voci.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(trovato) = voci.iter().find(|r| stesso(r)) {
                return trovato.valore.clone();
            }
        }
        let valore = calcola();
        let mut voci = self.voci.lock().unwrap_or_else(PoisonError::into_inner);
        voci.retain(|r| !stesso(r));
        voci.insert(
            0,
            Ricordo {
                impronta,
                bozza: bozza.map(ToOwned::to_owned),
                testo: testo.to_owned(),
                valore: valore.clone(),
            },
        );
        voci.truncate(QUANTI_RICORDI);
        valore
    }
}

static VALIDAZIONI: Ricordi<ValidazioneIpc> = Ricordi::nuovi();
static PROVE: Ricordi<Result<crate::skin::SkinIpc, AppError>> = Ricordi::nuovi();
static MIGRAZIONI: Ricordi<MigratoIpc> = Ricordi::nuovi();

/// L'id sotto cui compilare la tela e la prova: quello della bozza, quando si
/// può usare, altrimenti quello del documento.
///
/// Quello della bozza perché è la cartella da cui vengono le risorse, e il
/// documento può dichiarare un `id` che la bozza non ha ancora — l'ha appena
/// cambiato chi scrive, o è di un'altra skin e la rinomina è stata rifiutata.
/// Compilare col documento voleva dire caratteri e immagini chiesti a una
/// cartella che non esiste. Si usa solo se è già nell'alfabeto del foglio
/// (lettere, cifre, trattini): il compilatore riduce l'id a quello, e un id
/// ridotto non nominerebbe più la cartella.
fn radice_della_bozza(bozza: Option<&str>, documento: &str) -> String {
    bozza
        .filter(|b| id_sicuro(b) && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(documento)
        .to_owned()
}

/// Valida e compila un documento skin scritto a mano.
///
/// **Non fallisce mai.** Un documento non valido è il caso normale mentre lo si
/// scrive — a metà di una parentesi il JSON non è JSON — e restituire un errore
/// IPC costringerebbe l'editor a distinguere «il comando è andato storto» da «il
/// documento non è ancora finito». Qui gli errori sono un campo del risultato.
///
/// `bozza` è la bozza aperta: il foglio si compila sotto il suo id, e le
/// risorse vengono dalla sua cartella. Vedi [`radice_della_bozza`].
///
/// Gira fuori dal filo della finestra, e ricorda le ultime risposte: vedi
/// [`Ricordi`].
#[tauri::command(async)]
#[must_use]
pub fn studio_valida(sorgente: String, bozza: Option<String>) -> ValidazioneIpc {
    VALIDAZIONI.o_calcola(bozza.as_deref(), &sorgente, || {
        valida(&sorgente, bozza.as_deref())
    })
}

/// Quel che fa [`studio_valida`], senza i ricordi.
fn valida(sorgente: &str, bozza: Option<&str>) -> ValidazioneIpc {
    let inizio = std::time::Instant::now();
    let vuoto = ValidazioneIpc {
        errori: Vec::new(),
        avvisi: Vec::new(),
        contrasti: Vec::new(),
        tavolozza: Vec::new(),
        css: String::new(),
        layout: None,
        costo: 0,
        costo_scafale: 0,
        costo_movimento: 0,
        parti: 0,
        costi: Vec::new(),
        costo_finestra: 0,
        risorse: Vec::new(),
        dinamici: Vec::new(),
        compilato_ms: 0,
        radice: String::new(),
    };

    // Le posizioni si calcolano una volta per validazione e non una per
    // problema: è una passata sola sul testo, e sono venti token sbagliati che
    // chiedono la stessa mappa.
    let dove = rymembra_skin::posizioni(sorgente);

    let documento = match rymembra_skin::leggi_skin(sorgente) {
        Ok(documento) => documento,
        Err(problemi) => {
            let mut errori: Vec<ProblemaIpc> =
                problemi.iter().map(|p| problema_da(p, &dove)).collect();
            // In ordine di riga, e non è cosmesi. I token e le parti si leggono
            // da una mappa di `serde_json`, che li ordina per nome: un elenco
            // così esce alfabetico, e chi lo scorre accanto al file salta su e
            // giù per il documento a ogni riga. Chi ha scritto tre token
            // sbagliati li corregge dall'alto in basso.
            //
            // Ordinamento stabile: a parità di posizione — due problemi sulla
            // stessa riga — resta l'ordine in cui il nucleo li ha trovati.
            errori.sort_by_key(|e| (e.riga.unwrap_or(u32::MAX), e.colonna.unwrap_or(u32::MAX)));
            return ValidazioneIpc { errori, ..vuoto };
        }
    };

    // Nell'ambito della tela: i selettori della finestra — la tela è un
    // `<iframe>`, un documento suo dove `:root` è la sua radice e la skin
    // attiva dell'app non c'è — con le risorse della bozza e gli stati che la
    // tela può forzare (`data-forza`). Sotto l'id della bozza: vedi
    // [`radice_della_bozza`].
    let ambito = rymembra_skin::Ambito::tela(&radice_della_bozza(bozza, &documento.id));
    let compilata = rymembra_skin::compile::compila(&documento, &ambito);
    ValidazioneIpc {
        errori: Vec::new(),
        avvisi: rymembra_skin::check_skin(&documento)
            .into_iter()
            .map(|a| {
                let punto = dove.di(&a.path);
                AvvisoIpc {
                    kind: nome_avviso(a.kind),
                    path: a.path,
                    message: a.message,
                    riga: punto.map(|p| p.riga),
                    colonna: punto.map(|p| p.colonna),
                }
            })
            .collect::<Vec<_>>(),
        contrasti: rymembra_skin::contrast_pairs(&documento)
            .into_iter()
            .map(|c| ContrastoIpc {
                passa: c.passa(),
                davanti: c.foreground,
                dietro: c.background,
                base: c.base,
                variante: c.variante,
            })
            .collect(),
        tavolozza: rymembra_skin::palette_usage(&documento),
        css: compilata.css,
        layout: Some(crate::skin::impaginazione(&documento)),
        costo: compilata.cost,
        costo_scafale: compilata.shell_cost,
        costo_movimento: compilata.motion_cost,
        parti: documento.parts.len(),
        costi: documento
            .parts
            .iter()
            .map(|stile| CostoParteIpc {
                parte: stile.def.name,
                superficie: stile.costo_superficie(),
                movimento: stile.motion.costo(),
            })
            .collect(),
        costo_finestra: documento
            .parts
            .iter()
            .map(|stile| stile.motion.costo_insieme())
            .sum(),
        risorse: documento.assets(),
        dinamici: compilata
            .dynamic_tokens
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        compilato_ms: inizio.elapsed().as_millis(),
        radice: ambito.id().to_owned(),
    }
}

/// I nomi suggeriti dal nucleo, presi dal messaggio come dati.
///
/// Il suggerimento di `vicini()` esiste già dentro la frase — «Forse intendevi
/// «color.accent»?» — ma una frase non è un bottone. Qui si rileggono le
/// virgolette basse e si tiene quel che è davvero un nome del registro, così
/// l'editor può offrirlo come correzione da premere.
///
/// # Perché tre registri e non solo le parti
///
/// Perché il filtro era `parts::part(nome).is_some()`, e i refusi sui **token**
/// sono i più frequenti di tutti: `color.accents` per `color.accent`. Il
/// suggerimento c'era, il bottone no, e la differenza la vedeva solo chi sapeva
/// già cosa scrivere. `rinominaChiave` — quel che il bottone chiama — lavora sul
/// percorso del problema, quindi non gli importa di che registro sia il nome.
fn suggeriti(messaggio: &str) -> Vec<String> {
    messaggio
        .split('«')
        .skip(1)
        .filter_map(|pezzo| pezzo.split_once('»').map(|(dentro, _)| dentro.to_owned()))
        .filter(|nome| {
            rymembra_skin::parts::part(nome).is_some()
                || rymembra_skin::tokens::token(nome).is_some()
                || rymembra_skin::effects::Effect::NAMES.contains(&nome.as_str())
        })
        .collect()
}

/// Traduce un problema di validazione in qualcosa che l'editor può mostrare.
///
/// # Cosa faceva prima, e perché non poteva funzionare
///
/// Prendeva un `AppError` — cioè i venti problemi già uniti da `in_errore` con
/// dei punti e virgola — e si riprendeva il percorso facendo `split_once(':')`
/// sulla prosa. Ne usciva **un** problema con dentro venti frasi, un percorso
/// che era quello del primo, e un pannello che cresceva finché non aveva mangiato
/// l'editor. Adesso il percorso, il messaggio e il codice arrivano dai campi di
/// [`rymembra_skin::SkinIssue`], che li ha sempre avuti.
fn problema_da(guasto: &rymembra_skin::SkinIssue, dove: &rymembra_skin::Posizioni) -> ProblemaIpc {
    // Il punto scritto nel problema vince su quello dedotto dal percorso: ce
    // l'ha soltanto l'errore di sintassi, che un percorso non ce l'ha affatto.
    let punto = guasto.punto.or_else(|| dove.di(&guasto.path));
    ProblemaIpc {
        code: guasto.code.kind().code().to_owned(),
        path: guasto.path.clone(),
        message: guasto.message.clone(),
        forse: suggeriti(&guasto.message),
        riga: punto.map(|p| p.riga),
        colonna: punto.map(|p| p.colonna),
    }
}

// ── La bozza, il pacchetto, le istantanee ───────────────────────────────────

/// Dove stanno le bozze dello Studio.
fn cartella_bozze(data_dir: &Path) -> PathBuf {
    data_dir.join("skin").join("bozze")
}

/// Le bozze hanno un lucchetto loro.
///
/// Fino alla 3.0 ogni comando dello Studio prendeva quello della libreria —
/// l'unico, quello della connessione al database — per scrivere un file in
/// una cartella che col database non ha niente a che fare. Dietro un
/// salvataggio della bozza si accodava la scansione, e dietro la scansione il
/// salvataggio: su un disco lento, lo Studio che non salva mentre la libreria
/// si aggiorna. Le bozze però un ordine fra loro lo vogliono — una rinomina e
/// un salvataggio sulla stessa cartella non si devono incrociare — e questo è
/// quell'ordine, senza il resto.
static BOZZE: Mutex<()> = Mutex::new(());

/// La cartella dati, senza il lucchetto della libreria.
///
/// Se la libreria non si è aperta non c'è una cartella: allora si chiede a
/// [`con_libreria`], che sa dire il guasto col suo codice. La usa anche
/// `skin.rs`, per i comandi che leggono pacchetti e non il database.
pub(crate) fn cartella_dati(stato: &Stato) -> Result<PathBuf, AppError> {
    match stato.cartella_dati() {
        Some(dati) => Ok(dati.to_path_buf()),
        None => con_libreria(stato, |libreria| Ok(libreria.data_dir.clone())),
    }
}

/// Fa `azione` sulle bozze, col loro lucchetto e senza quello della libreria.
fn con_le_bozze<T>(
    stato: &Stato,
    azione: impl FnOnce(&Path) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let dati = cartella_dati(stato)?;
    let _turno = BOZZE.lock().unwrap_or_else(PoisonError::into_inner);
    azione(&dati)
}

/// L'identificatore è anche un nome di cartella: non deve poter uscire.
///
/// Stesso controllo di `skin.rs`, e ripetuto apposta: quello protegge la
/// cartella delle skin installate, questo quella delle bozze, e sono due porte
/// diverse. Un giorno una delle due potrebbe accettare qualcosa che l'altra no.
fn id_sicuro(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn non_trovata(id: &str) -> AppError {
    AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() })
}

/// Quel su cui lo Studio sta lavorando: il manifest, e le risorse che
/// l'esportazione dovrà rimettere dentro.
///
/// Il manifest può venire da una bozza; le risorse no. Una bozza è un
/// `skin.json` e basta — i caratteri e le immagini restano dentro il `.ryskin`
/// installato mentre si accorda il documento. Tenere le due cose in una
/// funzione sola è ciò che fa combaciare l'albero della vista Documento con quel
/// che l'esportazione scrive davvero: due letture separate divergerebbero il
/// giorno in cui una delle due impara qualcosa che l'altra no.
struct DaLavorare {
    sorgente: String,
    preview: Option<Vec<u8>>,
    assets: Vec<rymembra_skin::package::SkinAsset>,
}

/// Cerca prima fra le bozze, poi fra quelle di serie, poi fra le installate.
///
/// L'ordine è quello: chi ha lasciato una bozza a metà la ritrova, e non riparte
/// dall'originale che stava modificando.
///
/// «Quelle di serie» sono tutte, non solo Plain: fino alla 3.0 qui si
/// guardava soltanto `plain`, e derivare da Vinile o da Neon — che non stanno
/// fra le installate, perché non si installano — diceva «skin non installata».
///
/// # Il pacchetto si legge senza validarlo
///
/// Con le guardie dell'archivio sì, con la validazione del documento no
/// ([`rymembra_skin::package::leggi_pacchetto_grezzo`]). Fino alla 3.0 qui c'era
/// `read_skin_package` con un `?`: una skin installata che non passava più la
/// validazione — una regola nuova del formato, un budget diventato errore —
/// non si apriva nello Studio, cioè nell'unico posto dove la si poteva
/// aggiustare. E il `?` stava **prima** della bozza: bastava un pacchetto
/// rotto accanto a una bozza buona per perdere l'accesso a tutte e due.
/// Adesso il testo si apre com'è, e la validazione dice riga per riga cosa non
/// va. Un archivio che non si legge nemmeno così resta un guasto da dire —
/// ma solo se non c'è una bozza da aprire al suo posto.
fn da_lavorare(data_dir: &Path, id: &str) -> Result<DaLavorare, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }

    // Un file che non si apre vuol dire «non installata»; un file che si apre e
    // non si legge è un guasto, e va detto invece che scambiato per assenza.
    let installata = data_dir.join("skin").join(format!("{id}.ryskin"));
    let pacchetto = std::fs::read(&installata)
        .ok()
        .map(|bytes| rymembra_skin::package::leggi_pacchetto_grezzo(&bytes));
    let letto = pacchetto.as_ref().and_then(|esito| esito.as_ref().ok());

    let bozza = cartella_bozze(data_dir)
        .join(id)
        .join(rymembra_skin::package::MANIFEST_NAME);
    let sorgente = if let Ok(testo) = std::fs::read_to_string(&bozza) {
        testo
    } else if let Some(di_serie) = rymembra_skin::di_serie(id) {
        di_serie.to_owned()
    } else {
        match pacchetto.as_ref() {
            Some(Ok(letto)) => letto.source.clone(),
            Some(Err(guasto)) => return Err(guasto.clone()),
            None => return Err(non_trovata(id)),
        }
    };

    // Le risorse della bozza comandano quando la bozza ha la sua cartella:
    // è lì che lo Studio aggiunge e toglie.
    let risorse = cartella_risorse(data_dir, id);
    let assets = if risorse.exists() {
        risorse_della_bozza(&risorse)
    } else {
        letto.map_or_else(Vec::new, |letto| letto.assets.clone())
    };
    Ok(DaLavorare {
        sorgente,
        preview: letto.and_then(|letto| letto.preview.clone()),
        assets,
    })
}

/// Dove stanno le istantanee di una skin.
///
/// **Fuori** dalla bozza: fino alla 2.x stavano in `bozze/<id>/istantanee`, e
/// «Scarta bozza» — che butta la cartella della bozza — le portava via con
/// sé, compresa quella presa un attimo prima proprio per poter tornare
/// indietro. La rete sotto il bottone irreversibile se ne andava col bottone.
fn cartella_istantanee(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("skin").join("istantanee").join(id)
}

/// Le istantanee di una skin, dopo averle portate fuori dalla bozza se
/// stavano ancora dentro.
///
/// Il trasloco è per voce e non per cartella: se da una versione all'altra ne
/// sono nate anche nella cartella nuova, le due serie si uniscono invece di
/// scavalcarsi. Un file che non si sposta resta dov'era — la prossima volta ci
/// si riprova — e non ferma niente.
fn istantanee_di(data_dir: &Path, id: &str) -> PathBuf {
    let nuova = cartella_istantanee(data_dir, id);
    let vecchia = cartella_bozze(data_dir).join(id).join("istantanee");
    if let Ok(voci) = std::fs::read_dir(&vecchia)
        && std::fs::create_dir_all(&nuova).is_ok()
    {
        for voce in voci.flatten() {
            drop(std::fs::rename(voce.path(), nuova.join(voce.file_name())));
        }
        drop(std::fs::remove_dir(&vecchia));
    }
    nuova
}

/// Dove stanno le risorse della bozza: caratteri e immagini.
///
/// È la cartella che il protocollo `rymembra-skin` serve all'anteprima e alla
/// prova sull'app (`skin::servi_risorsa`), e quella da cui l'esportazione e
/// «Salva e usa» riempiono il pacchetto.
fn cartella_risorse(data_dir: &Path, id: &str) -> PathBuf {
    cartella_bozze(data_dir).join(id).join("assets")
}

/// Le risorse della bozza, in ordine di nome.
///
/// Si rileggono i byte a ogni volta, e quel che non passa il controllo del
/// formato — un nome storto, un `.png` che non è un PNG — si salta: nella
/// cartella qualcuno ci può aver messo le mani, e una risorsa che il pacchetto
/// rifiuterebbe non deve arrivare fino all'esportazione per farla fallire.
fn risorse_della_bozza(cartella: &Path) -> Vec<rymembra_skin::package::SkinAsset> {
    let Ok(voci) = std::fs::read_dir(cartella) else {
        return Vec::new();
    };
    let mut risorse: Vec<rymembra_skin::package::SkinAsset> = voci
        .flatten()
        .filter_map(|voce| {
            let name = voce.file_name().to_str()?.to_owned();
            let bytes = std::fs::read(voce.path()).ok()?;
            let mime = rymembra_skin::riconosci_risorsa(&name, &bytes)?;
            Some(rymembra_skin::package::SkinAsset { name, mime, bytes })
        })
        .collect();
    risorse.sort_by(|a, b| a.name.cmp(&b.name));
    risorse
}

/// Porta nella bozza le risorse del pacchetto.
///
/// Una volta sola: se la cartella c'è già, la bozza ha le sue e quelle
/// comandano. Senza `forza` la cartella nasce solo se c'è qualcosa da
/// metterci — aprire nello Studio una skin senza risorse non deve lasciare
/// cartelle vuote in giro.
fn materializza_risorse(
    data_dir: &Path,
    id: &str,
    del_pacchetto: &[rymembra_skin::package::SkinAsset],
    forza: bool,
) -> Result<(), AppError> {
    let cartella = cartella_risorse(data_dir, id);
    if cartella.exists() || (del_pacchetto.is_empty() && !forza) {
        return Ok(());
    }
    std::fs::create_dir_all(&cartella)
        .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;
    for risorsa in del_pacchetto {
        scrivi_atomico(&cartella.join(&risorsa.name), &risorsa.bytes)?;
    }
    Ok(())
}

/// Il nome che una risorsa prende nel pacchetto, dal nome del file.
///
/// Il formato ammette minuscole, cifre, trattini e trattini bassi, con
/// un'estensione sola: un «Titoli Grassetto.WOFF2» diventa
/// «titoli-grassetto.woff2» invece di essere rifiutato per una maiuscola.
fn nome_risorsa(percorso: &Path) -> String {
    let radice: String = percorso
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let radice = radice.trim_matches(|c: char| c == '-' || c == '_');
    let radice = if radice.is_empty() { "risorsa" } else { radice };
    let estensione = percorso
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let estensione = if estensione == "jpeg" {
        "jpg".to_owned()
    } else {
        estensione
    };
    format!("{radice}.{estensione}")
}

/// Un nome che nella cartella non c'è ancora: `trama.png`, `trama-2.png`, …
fn nome_libero(cartella: &Path, nome: &str) -> String {
    if !cartella.join(nome).exists() {
        return nome.to_owned();
    }
    let (radice, estensione) = nome.rsplit_once('.').unwrap_or((nome, ""));
    (2..)
        .map(|n| format!("{radice}-{n}.{estensione}"))
        .find(|candidato| !cartella.join(candidato).exists())
        .unwrap_or_else(|| nome.to_owned())
}

fn rifiutata(nome: &str, perche: &str) -> AppError {
    AppError::new(ErrorCode::SkinAssetRejected {
        asset: nome.to_owned(),
        reason: perche.to_owned(),
    })
}

/// Aggiunge una risorsa alla bozza, da un file scelto sul disco.
///
/// Il tipo si decide dai byte, come dentro un pacchetto: un file chiamato
/// `.png` che non lo è si ferma qui, col suo nome, invece che all'esportazione.
///
/// E così la misura: la risorsa entra solo se il pacchetto che ne verrebbe sta
/// nei limiti della lettura (`entra_in_un_pacchetto`). Fino alla 3.0 si
/// contavano soltanto le voci, e una bozza poteva raccogliere trenta megabyte
/// di immagini che nessun `.ryskin` avrebbe mai contenuto — lo si scopriva
/// esportando, cioè dopo averci lavorato sopra.
fn aggiungi_risorsa(data_dir: &Path, id: &str, percorso: &Path) -> Result<VoceFileIpc, AppError> {
    use rymembra_skin::package::limits::MAX_ENTRY_BYTES;

    let da = da_lavorare(data_dir, id)?;
    materializza_risorse(data_dir, id, &da.assets, true)?;
    let cartella = cartella_risorse(data_dir, id);

    let nome = nome_risorsa(percorso);
    let byte = std::fs::metadata(percorso)
        .map_err(|err| rymembra_app::files::io_error(&percorso.display().to_string(), &err))?
        .len();
    if byte > MAX_ENTRY_BYTES {
        return Err(AppError::new(ErrorCode::SkinTooLarge {
            bytes: byte,
            limit_bytes: MAX_ENTRY_BYTES,
        }));
    }
    let bytes = std::fs::read(percorso)
        .map_err(|err| rymembra_app::files::io_error(&percorso.display().to_string(), &err))?;
    if rymembra_skin::riconosci_risorsa(&nome, &bytes).is_none() {
        return Err(rifiutata(
            &nome,
            "una risorsa è un'immagine png, jpg o webp, o un carattere woff2, e i byte devono dire lo stesso dell'estensione",
        ));
    }
    let quanto = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    rymembra_skin::package::entra_in_un_pacchetto(
        quanto(da.sorgente.len()),
        da.preview.as_ref().map(|p| quanto(p.len())),
        risorse_della_bozza(&cartella)
            .iter()
            .map(|r| quanto(r.bytes.len()))
            .chain([byte]),
    )?;
    let libero = nome_libero(&cartella, &nome);
    scrivi_atomico(&cartella.join(&libero), &bytes)?;
    Ok(VoceFileIpc {
        nome: format!("assets/{libero}"),
        byte,
        genere: "risorsa",
    })
}

/// Aggiunge una risorsa alla bozza. Restituisce la voce, col nome che ha preso.
#[tauri::command(async)]
pub fn studio_risorsa_aggiungi(
    stato: State<'_, Stato>,
    id: String,
    percorso: String,
) -> Esito<VoceFileIpc> {
    con_le_bozze(&stato, |dati| {
        aggiungi_risorsa(dati, &id, Path::new(&percorso))
    })
    .map_err(errore)
}

/// Toglie una risorsa dalla bozza.
#[tauri::command(async)]
pub fn studio_risorsa_togli(stato: State<'_, Stato>, id: String, nome: String) -> Esito<()> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) || rymembra_skin::tipo_risorsa(&nome).is_none() {
            return Err(non_trovata(&id));
        }
        let da = da_lavorare(dati, &id)?;
        materializza_risorse(dati, &id, &da.assets, true)?;
        let file = cartella_risorse(dati, &id).join(&nome);
        match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(rymembra_app::files::io_error(
                &file.display().to_string(),
                &err,
            )),
        }
    })
    .map_err(errore)
}

/// Quel che serve per mettere la bozza in un pacchetto: la miniatura del
/// pacchetto installato, e le risorse della bozza.
///
/// Lo usa anche «Salva e usa» in `skin.rs`: prima impacchettava il solo
/// manifest, e una skin col suo carattere installata da qui perdeva il
/// carattere e la miniatura. Prende da sé il lucchetto delle bozze.
pub fn corredo_della_bozza(
    data_dir: &Path,
    id: &str,
) -> Result<(Option<Vec<u8>>, Vec<rymembra_skin::package::SkinAsset>), AppError> {
    let _turno = BOZZE.lock().unwrap_or_else(PoisonError::into_inner);
    corredo(data_dir, id)
}

/// [`corredo_della_bozza`], per chi il lucchetto delle bozze l'ha già.
fn corredo(
    data_dir: &Path,
    id: &str,
) -> Result<(Option<Vec<u8>>, Vec<rymembra_skin::package::SkinAsset>), AppError> {
    let da = da_lavorare(data_dir, id)?;
    Ok((da.preview, da.assets))
}

/// Il testo di una skin da aprire nello Studio.
///
/// Una skin del formato 1 si apre **migrata**: il testo che lo Studio mostra
/// è quello del formato di oggi, e la migrazione si scrive subito come bozza —
/// così alla prossima apertura non si rifà, e non si fotografa due volte. Il
/// testo di prima resta in un'istantanea di causa `migrata`, che è da dove si
/// riparte se la migrazione ha tradito qualcosa.
#[tauri::command(async)]
pub fn studio_documento(stato: State<'_, Stato>, id: String) -> Esito<String> {
    con_le_bozze(&stato, |dati| apri_documento(dati, &id, true)).map_err(errore)
}

/// Quel che fa [`studio_documento`], senza lo stato di Tauri attorno.
///
/// `fotografa` dice se la migrazione di un testo del formato 1 si prende la
/// sua istantanea. Sì all'apertura; no dopo «Scarta bozza», che riapre lo
/// stesso pacchetto: la fotografia di quel testo c'è già dalla prima
/// apertura, e ogni «Scarta» ne aggiungeva un'altra uguale — la serie delle
/// istantanee si riempiva di copie dello stesso file, e le venti che si
/// tengono spingevano fuori quelle che contavano.
fn apri_documento(data_dir: &Path, id: &str, fotografa: bool) -> Result<String, AppError> {
    let da = da_lavorare(data_dir, id)?;
    materializza_risorse(data_dir, id, &da.assets, false)?;
    match rymembra_skin::migra_testo(&da.sorgente) {
        Ok(migrato) if migrato.da < u64::from(rymembra_skin::SKIN_FORMAT_VERSION) => {
            if fotografa {
                prendi_istantanea(&istantanee_di(data_dir, id), &da.sorgente, Causa::Migrata)?;
            }
            let file = cartella_bozze(data_dir)
                .join(id)
                .join(rymembra_skin::package::MANIFEST_NAME);
            scrivi_atomico(&file, migrato.sorgente.as_bytes())?;
            Ok(migrato.sorgente)
        }
        // Già del formato di oggi, o non ancora JSON: si apre com'è, e la
        // validazione dirà dove.
        _ => Ok(da.sorgente),
    }
}

/// Una voce del pacchetto, come la mostra la colonna sinistra dello Studio.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceFileIpc {
    /// Il nome dentro l'archivio: `skin.json`, `preview.png`, `assets/x.woff2`.
    pub nome: String,
    /// Quanto pesa, decompresso.
    pub byte: u64,
    /// `manifest`, `miniatura` o `risorsa`.
    pub genere: &'static str,
}

/// Cosa c'è nel pacchetto su cui si sta lavorando.
///
/// Non è l'elenco di una cartella: è l'elenco delle voci che il **formato**
/// ammette, cioè esattamente quel che `studio_esporta` rimetterà nell'archivio.
/// Mostrare un file che l'esportazione butterebbe via sarebbe peggio che non
/// mostrare niente.
#[tauri::command(async)]
pub fn studio_pacchetto(stato: State<'_, Stato>, id: String) -> Esito<Vec<VoceFileIpc>> {
    con_le_bozze(&stato, |dati| {
        let da = da_lavorare(dati, &id)?;
        let quanto = |byte: usize| u64::try_from(byte).unwrap_or(u64::MAX);
        let mut voci = vec![VoceFileIpc {
            nome: rymembra_skin::package::MANIFEST_NAME.to_owned(),
            byte: quanto(da.sorgente.len()),
            genere: "manifest",
        }];
        if let Some(miniatura) = &da.preview {
            voci.push(VoceFileIpc {
                nome: rymembra_skin::package::PREVIEW_NAME.to_owned(),
                byte: quanto(miniatura.len()),
                genere: "miniatura",
            });
        }
        voci.extend(da.assets.iter().map(|risorsa| VoceFileIpc {
            nome: format!("assets/{}", risorsa.name),
            byte: quanto(risorsa.bytes.len()),
            genere: "risorsa",
        }));
        Ok(voci)
    })
    .map_err(errore)
}

/// Com'è andato un salvataggio della bozza.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvataggioIpc {
    /// L'id sotto cui la bozza sta adesso: quello nuovo, se il documento ha
    /// cambiato `id` e la rinomina è passata.
    pub id: String,
    /// Perché la rinomina non è passata, quando il documento dichiara un id
    /// che la bozza non ha potuto prendere. Il testo è salvato lo stesso,
    /// sotto l'id di prima.
    pub rinomina: Option<ErroreIpc>,
}

/// L'`id` che il testo dichiara, se è un nome di cartella possibile.
///
/// Letto dal JSON e non dal documento validato: una bozza si salva anche a
/// metà, e un token sbagliato altrove non toglie niente al suo `id`.
fn id_dichiarato(sorgente: &str) -> Option<String> {
    let valore: serde_json::Value = serde_json::from_str(sorgente).ok()?;
    let id = valore.get("id")?.as_str()?;
    id_sicuro(id).then(|| id.to_owned())
}

/// Salva la bozza, dopo averla spostata sotto l'id che il documento dichiara.
///
/// # Perché la rinomina sta qui
///
/// Fino alla 3.0 erano due comandi, e la finestra li chiamava a un secondo e
/// mezzo l'uno dall'altro: il salvataggio scriveva nella cartella vecchia, la
/// rinomina — più tardi, se il testo era valido, se nessuno aveva annullato —
/// la spostava. In mezzo c'erano la prova sull'app e l'esportazione, che
/// cercavano le risorse sotto l'id del documento in una cartella che ancora
/// non c'era, e un rifiuto della rinomina che nessuno vedeva: lo si buttava
/// con un `catch` vuoto. Adesso è un passo solo, sotto il lucchetto delle
/// bozze, e quel che è successo torna indietro: dove sta la bozza, e perché
/// non si è spostata se non si è spostata.
fn salva_bozza(
    data_dir: &Path,
    id: &str,
    sorgente: &str,
    origine: Option<&str>,
) -> Result<SalvataggioIpc, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let mut dove = id.to_owned();
    let mut rinomina = None;
    if let Some(dichiarato) = id_dichiarato(sorgente).filter(|d| d != id) {
        match rinomina_bozza(data_dir, id, &dichiarato, origine) {
            Ok(()) => dove = dichiarato,
            Err(rifiuto) => rinomina = Some(ErroreIpc::from(rifiuto)),
        }
    }
    let file = cartella_bozze(data_dir)
        .join(&dove)
        .join(rymembra_skin::package::MANIFEST_NAME);
    scrivi_atomico(&file, sorgente.as_bytes())?;
    Ok(SalvataggioIpc { id: dove, rinomina })
}

/// Salva la bozza.
///
/// Si salva **anche se non è valida**: una bozza è un lavoro in corso, e
/// rifiutare di scrivere un documento a metà vorrebbe dire perderlo chiudendo la
/// finestra. È la validazione a dire cosa non va, non il salvataggio.
///
/// Se il documento ha cambiato `id` la bozza lo segue: vedi [`salva_bozza`].
/// `origine` è l'id con cui lo Studio è stato aperto.
#[tauri::command(async)]
pub fn studio_salva(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    origine: Option<String>,
) -> Esito<SalvataggioIpc> {
    let esito = con_le_bozze(&stato, |dati| {
        salva_bozza(dati, &id, &sorgente, origine.as_deref())
    })
    .map_err(errore);
    // Una bozza è lavoro non ancora salvato da nessuna parte: è precisamente la
    // cosa che un backup deve portare via per prima.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Butta la bozza e torna a quel che dice il pacchetto.
///
/// # Perché le istantanee restano
///
/// Sono due cose diverse e non si cancellano insieme. La bozza è «dove sono
/// arrivato»; le istantanee sono «dove sono passato», e sono l'unica rete sotto
/// questo bottone — che è irreversibile per natura. Per questo non stanno più
/// dentro la cartella della bozza ma in `skin/istantanee/<id>/`: la 2.x le
/// teneva lì, e questo comando se le portava via.
///
/// Restituisce la sorgente del pacchetto, cioè quel che l'editor deve mostrare
/// da qui in poi: farsela ridire con una seconda chiamata lascerebbe un istante
/// in cui la finestra mostra un documento che non esiste più da nessuna parte.
#[tauri::command(async)]
pub fn studio_scarta(stato: State<'_, Stato>, id: String) -> Esito<String> {
    con_le_bozze(&stato, |dati| scarta_bozza(dati, &id)).map_err(errore)
}

/// Quel che fa [`studio_scarta`], senza lo stato di Tauri attorno.
fn scarta_bozza(data_dir: &Path, id: &str) -> Result<String, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let cartella = cartella_bozze(data_dir).join(id);
    if cartella.exists() {
        std::fs::remove_dir_all(&cartella)
            .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;
    }
    // Dopo la cancellazione `da_lavorare` non trova più la bozza e ricade
    // sul pacchetto: è la stessa funzione che decide all'apertura, quindi
    // non c'è un secondo posto in cui la regola «la bozza vince» possa
    // divergere. Le istantanee non stanno più qui dentro, e restano — e non
    // se ne aggiunge un'altra per la migrazione: vedi `apri_documento`.
    apri_documento(data_dir, id, false)
}

/// Scrive un `.ryskin` da una sorgente, con dentro le risorse che aveva.
///
/// Gli **errori** bloccano, gli avvisi no: è la regola scritta nel disegno, e
/// qui è per costruzione — `parse_skin_json` fallisce sugli errori e non sa
/// niente degli avvisi.
///
/// Il manifest è quello dell'editor, le risorse quelle del pacchetto: chi aveva
/// una skin con un carattere dentro lo perdeva esportandola da qui, e l'albero
/// della vista Documento avrebbe mostrato file che l'esportazione buttava via.
///
/// Il percorso arriva dalla pagina, e si scrive solo se finisce in `.ryskin`:
/// fino alla 2.4 questo comando scriveva un pacchetto sotto qualunque nome.
/// Vedi [`crate::destinazioni`].
///
/// # Errori
///
/// `fs.extensionRefused` o `fs.pathInvalid` per un percorso che la finestra non
/// può chiedere, **prima** di prendere il lucchetto; quelli della validazione e
/// della scrittura per il resto.
#[tauri::command(async)]
pub fn studio_esporta(
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    percorso: String,
) -> Esito<()> {
    let file =
        crate::destinazioni::file_da_scrivere(&percorso, crate::destinazioni::ESTENSIONI_SKIN)
            .map_err(errore)?;
    con_le_bozze(&stato, |dati| {
        let da = da_lavorare(dati, &id)?;
        // `write_skin_package` valida da sé prima di scrivere — passa dalle
        // stesse guardie della lettura — quindi qui non serve un `parse` in più:
        // sarebbe una seconda validazione che può divergere dalla prima.
        let bytes =
            rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
                source: &sorgente,
                preview: da.preview.as_deref(),
                assets: &da.assets,
            })?;
        // Intero o niente anche qui: un `.ryskin` a metà sul disco di chi lo
        // voleva mandare a qualcuno è un pacchetto che si scopre rotto
        // dall'altra parte.
        scrivi_atomico(&file.percorso, &bytes)
    })
    .map_err(errore)
}

/// Un testo portato al formato di oggi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigratoIpc {
    /// Il testo da aprire.
    pub sorgente: String,
    /// Di quale formato era.
    pub da: u64,
}

/// Porta un testo al formato di oggi: per chi incolla nello Studio una skin
/// della 2.x.
///
/// Un testo che non è JSON torna com'era, col formato di oggi: la migrazione
/// non è il posto in cui dire cosa non va in una parentesi — lo dice la
/// validazione, riga per riga.
///
/// Fuori dal filo della finestra, e coi suoi ricordi: vedi [`Ricordi`].
#[tauri::command(async)]
#[must_use]
pub fn studio_migra(sorgente: String) -> MigratoIpc {
    MIGRAZIONI.o_calcola(None, &sorgente, || {
        match rymembra_skin::migra_testo(&sorgente) {
            Ok(migrato) => MigratoIpc {
                sorgente: migrato.sorgente,
                da: migrato.da,
            },
            Err(_) => MigratoIpc {
                sorgente: sorgente.clone(),
                da: u64::from(rymembra_skin::SKIN_FORMAT_VERSION),
            },
        }
    })
}

/// Compila una bozza per la finestra vera: «Prova sull'app».
///
/// Selettori della finestra, risorse della bozza — e l'id della **bozza**,
/// come la tela ([`radice_della_bozza`]): le risorse stanno nella sua
/// cartella, e la skin che torna porta lo stesso id dei selettori, che è
/// quel che la finestra scrive in `data-skin`. Gli errori fermano: una bozza
/// che non si legge non si prova addosso.
///
/// Fuori dal filo della finestra, e coi suoi ricordi: tornare dalla prova e
/// riprovare lo stesso testo non ricompila niente.
#[tauri::command(async)]
pub fn studio_compila_app(sorgente: String, bozza: String) -> Esito<crate::skin::SkinIpc> {
    PROVE
        .o_calcola(Some(&bozza), &sorgente, || {
            compila_per_la_prova(&sorgente, &bozza)
        })
        .map_err(errore)
}

/// Quel che fa [`studio_compila_app`], senza i ricordi.
fn compila_per_la_prova(sorgente: &str, bozza: &str) -> Result<crate::skin::SkinIpc, AppError> {
    let documento = rymembra_skin::parse_skin_json(sorgente)?;
    let ambito = rymembra_skin::Ambito::prova(&radice_della_bozza(Some(bozza), &documento.id));
    let mut skin = crate::skin::skin_ipc(&documento, &ambito);
    skin.id = ambito.id().to_owned();
    Ok(skin)
}

/// Sposta la bozza e le sue istantanee sotto un altro identificatore.
///
/// Serve quando nel documento cambia `id`: la bozza vive in una cartella col
/// nome dell'id, e un documento che dice «sono nocturne» dentro la cartella di
/// «plain» si ritroverebbe riaprendo plain. Non si scavalca niente: se sotto
/// il nome nuovo c'è già una bozza, una skin di serie o una skin **installata**
/// si dice di no. L'ultima mancava fino alla 3.0: una bozza rinominata come
/// una skin installata di un altro, alla prima «Salva e usa», ne prendeva il
/// posto.
///
/// `origine` è la skin da cui lo Studio è partito: tornare al suo id non è
/// prendere il posto di un'altra, è rimettere a posto il proprio. Senza,
/// chi cambiava l'`id` e ci ripensava restava chiuso fuori dalla sua skin.
fn rinomina_bozza(
    data_dir: &Path,
    da: &str,
    a: &str,
    origine: Option<&str>,
) -> Result<(), AppError> {
    if !id_sicuro(da) || !id_sicuro(a) {
        return Err(non_trovata(a));
    }
    if da == a {
        return Ok(());
    }
    let arrivo = cartella_bozze(data_dir).join(a);
    let installata =
        origine != Some(a) && data_dir.join("skin").join(format!("{a}.ryskin")).exists();
    if rymembra_skin::di_serie(a).is_some() || arrivo.exists() || installata {
        return Err(
            AppError::new(ErrorCode::SkinIdConflict { id: a.to_owned() }).with_cause(
                "sotto questo identificatore c'è già una skin di serie, una skin installata o \
                 un'altra bozza"
                    .to_owned(),
            ),
        );
    }
    let partenza = cartella_bozze(data_dir).join(da);
    if partenza.exists() {
        std::fs::create_dir_all(cartella_bozze(data_dir))
            .map_err(|err| rymembra_app::files::io_error(&arrivo.display().to_string(), &err))?;
        std::fs::rename(&partenza, &arrivo)
            .map_err(|err| rymembra_app::files::io_error(&arrivo.display().to_string(), &err))?;
    }
    let istantanee = istantanee_di(data_dir, da);
    if istantanee.exists() {
        let nuove = cartella_istantanee(data_dir, a);
        if let Some(sopra) = nuove.parent() {
            std::fs::create_dir_all(sopra)
                .map_err(|err| rymembra_app::files::io_error(&sopra.display().to_string(), &err))?;
        }
        if !nuove.exists() {
            std::fs::rename(&istantanee, &nuove)
                .map_err(|err| rymembra_app::files::io_error(&nuove.display().to_string(), &err))?;
        }
    }
    Ok(())
}

// ── Un tema nuovo ───────────────────────────────────────────────────────────

/// Gli id che hanno già una cartella nello Studio: bozze e istantanee.
///
/// Un tema nuovo non li deve prendere anche se nessuna skin li porta più. Una
/// bozza rimasta da una skin disinstallata ha dentro le sue risorse e la sua
/// conversazione, e fino alla 3.0 il tema nuovo che ne ereditava il nome le
/// ereditava tutte e due — la conversazione su un'altra skin, e caratteri che
/// il manifest nuovo non nominava. Le istantanee idem: comparivano nell'elenco
/// del tema nuovo come se fossero sue.
fn occupati(data_dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = [
        cartella_bozze(data_dir),
        data_dir.join("skin").join("istantanee"),
    ]
    .iter()
    .filter_map(|cartella| std::fs::read_dir(cartella).ok())
    .flatten()
    .flatten()
    .filter(|voce| voce.path().is_dir())
    .filter_map(|voce| voce.file_name().to_str().map(ToOwned::to_owned))
    .filter(|id| id_sicuro(id))
    .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Gli id che un tema nuovo non può prendere, oltre a quelli delle skin
/// elencate: vedi [`occupati`].
#[tauri::command(async)]
pub fn studio_occupati(stato: State<'_, Stato>) -> Esito<Vec<String>> {
    con_le_bozze(&stato, |dati| Ok(occupati(dati))).map_err(errore)
}

/// Un tema nuovo: il pacchetto con le risorse della base, installato, e la
/// bozza che parte da lì.
///
/// # Perché un comando solo
///
/// Fino alla 3.0 la finestra lo componeva da tre: installa il manifest,
/// salva la bozza, scegli. L'installazione impacchettava il **solo**
/// manifest, quindi un tema nato da una skin con un carattere o un'immagine
/// si fermava lì — il manifest nominava una risorsa che il pacchetto non
/// aveva — e quello nato da una skin con la miniatura la perdeva. E la bozza
/// finiva nella cartella che c'era già, se c'era: vedi [`occupati`].
///
/// Qui le risorse e la miniatura sono quelle della base — della sua bozza, se
/// la base è aperta nello Studio con qualcosa di aggiunto — e un id già preso
/// da una skin, da una bozza o da una serie di istantanee si rifiuta prima di
/// scrivere qualunque cosa.
fn crea_tema(
    data_dir: &Path,
    base: &str,
    sorgente: &str,
) -> Result<crate::skin::VoceSkin, AppError> {
    let documento = rymembra_skin::parse_skin_json(sorgente)?;
    let id = documento.id;
    if !id_sicuro(&id) {
        return Err(non_trovata(&id));
    }
    let preso = rymembra_skin::di_serie(&id).is_some()
        || data_dir.join("skin").join(format!("{id}.ryskin")).exists()
        || occupati(data_dir).contains(&id);
    if preso {
        return Err(AppError::new(ErrorCode::SkinIdConflict { id }));
    }
    let (preview, assets) = corredo(data_dir, base)?;
    let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
        source: sorgente,
        preview: preview.as_deref(),
        assets: &assets,
    })?;
    let voce = crate::skin::installa(data_dir, &bytes)?;
    // La bozza parte allineata all'installato: riaprendo lo Studio, la
    // pillola in testa non dice «da salvare» su un lavoro che nessuno ha
    // ancora toccato. Le risorse la bozza le prende dal pacchetto alla prima
    // apertura, come per ogni skin installata.
    let file = cartella_bozze(data_dir)
        .join(&id)
        .join(rymembra_skin::package::MANIFEST_NAME);
    scrivi_atomico(&file, sorgente.as_bytes())?;
    Ok(voce)
}

/// Crea un tema nuovo da `base`: vedi [`crea_tema`].
///
/// # Errori
///
/// `skin.idConflict` se l'id è già preso; quelli della validazione, del
/// pacchetto e della scrittura per il resto.
#[tauri::command(async)]
pub fn studio_crea(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    base: String,
    sorgente: String,
) -> Esito<crate::skin::VoceSkin> {
    let esito = con_le_bozze(&stato, |dati| crea_tema(dati, &base, &sorgente)).map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

// ── La conversazione con la chat ────────────────────────────────────────────

/// Il nome del file della conversazione, dentro la cartella della bozza.
const FILE_CHAT: &str = "chat.json";

/// Quanto può pesare una conversazione salvata.
///
/// Due megabyte sono qualche centinaio di battute con i loro blocchi di
/// modifiche: le immagini non ci finiscono — si mandano e basta — e il testo
/// di una chat vera sta molto sotto. Il tetto è per la finestra che si rompe e
/// scrive in un ciclo, non per chi chiacchiera a lungo.
const CHAT_MASSIMA_MB: usize = 2;
const CHAT_MASSIMA: usize = CHAT_MASSIMA_MB * 1024 * 1024;

/// La conversazione salvata di una bozza, o `None` se non ce n'è.
///
/// Sta dentro la cartella della bozza, accanto a `skin.json`: è una
/// conversazione **su quella bozza**, e «Scarta» se la porta via insieme — le
/// modifiche di cui parla non esistono più. Una rinomina la sposta con la
/// cartella. Il backup su Drive (`rymembra_cloud::pacchetti`) prende soltanto il
/// manifest: quel che si è scritto al modello resta su questo computer.
fn leggi_chat(data_dir: &Path, id: &str) -> Result<Option<String>, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let file = cartella_bozze(data_dir).join(id).join(FILE_CHAT);
    match std::fs::read_to_string(&file) {
        Ok(testo) => Ok(Some(testo)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(rymembra_app::files::io_error(
            &file.display().to_string(),
            &err,
        )),
    }
}

/// Scrive la conversazione di una bozza. Vuota, la toglie.
///
/// Il contenuto lo decide la finestra, e qui si controlla solo che sia un
/// oggetto JSON di un peso ragionevole: il file lo rilegge lei, e un formato
/// scritto in due posti sarebbe un formato che diverge.
fn scrivi_chat(data_dir: &Path, id: &str, testo: &str) -> Result<(), AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let cartella = cartella_bozze(data_dir).join(id);
    let file = cartella.join(FILE_CHAT);
    if testo.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(
                rymembra_app::files::io_error(&file.display().to_string(), &err),
            ),
            _ => Ok(()),
        };
    }
    let oggetto =
        serde_json::from_str::<serde_json::Value>(testo).is_ok_and(|valore| valore.is_object());
    if testo.len() > CHAT_MASSIMA || !oggetto {
        return Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "studio_chat_scrivi".to_owned(),
            detail: Some(format!(
                "la conversazione non è un oggetto JSON entro {} MB ({} byte)",
                CHAT_MASSIMA_MB,
                testo.len()
            )),
        }));
    }
    scrivi_atomico(&file, testo.as_bytes())
}

/// La conversazione salvata della bozza `id`, o `null`.
#[tauri::command(async)]
pub fn studio_chat_leggi(stato: State<'_, Stato>, id: String) -> Esito<Option<String>> {
    con_le_bozze(&stato, |dati| leggi_chat(dati, &id)).map_err(errore)
}

/// Salva la conversazione della bozza `id`; un testo vuoto la cancella.
#[tauri::command(async)]
pub fn studio_chat_scrivi(stato: State<'_, Stato>, id: String, testo: String) -> Esito<()> {
    con_le_bozze(&stato, |dati| scrivi_chat(dati, &id, &testo)).map_err(errore)
}

// ── Il generatore di tavolozze ──────────────────────────────────────────────

/// Un colore generato, per un token.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceTavolozzaIpc {
    /// `color.surface.0`.
    pub token: &'static str,
    /// `#rrggbb` o `#rrggbbaa`.
    pub valore: String,
}

/// Una tavolozza intera da una tinta: un colore scritto, o la copertina.
///
/// La copertina è quella di un brano della libreria, per impronta: la tinta
/// la sa già il magazzino delle copertine, che la calcola per l'accento che
/// segue la musica. Niente si decodifica qui.
#[tauri::command(async)]
pub fn studio_tavolozza_da(
    stato: State<'_, Stato>,
    colore: Option<String>,
    copertina: Option<String>,
    scura: bool,
) -> Esito<Vec<VoceTavolozzaIpc>> {
    let tinta = match (colore, copertina) {
        (Some(scritto), _) => rymembra_skin::values::parse_color(&scritto)
            .ok_or_else(|| errore(rifiutata("colore", "non è un colore")))?,
        (None, Some(impronta)) => {
            if !crate::copertine::e_un_impronta(&impronta) {
                return Err(errore(rifiutata("copertina", "non è un'impronta")));
            }
            let copertine =
                con_libreria(&stato, |libreria| Ok(libreria.covers.clone())).map_err(errore)?;
            let [r, g, b] = copertine
                .tinta(&impronta)
                .ok_or_else(|| errore(rifiutata("copertina", "la copertina non ha una tinta")))?;
            rymembra_skin::values::Rgba { r, g, b, a: 1.0 }
        }
        (None, None) => return Err(errore(rifiutata("tinta", "manca da dove partire"))),
    };
    Ok(rymembra_skin::dinamico::tavolozza_da(tinta, scura)
        .into_iter()
        .map(|(token, valore)| VoceTavolozzaIpc { token, valore })
        .collect())
}

// ── Le istantanee ───────────────────────────────────────────────────────────

/// Perché un'istantanea è stata presa.
///
/// «Senza nomi da inventare»: un'istantanea non si battezza, si prende. Il nome
/// che porta dice **perché**, che è l'unica cosa che chi scorre l'elenco vuole
/// sapere — e siccome è un insieme chiuso, è un enum e non una stringa: così a
/// respingere quel che non è previsto è il confine dell'IPC, e non un controllo
/// che qualcuno prima o poi dimentica di scrivere. È la prima regola dello
/// Studio applicata allo Studio stesso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Causa {
    /// La prima apertura di una skin derivata da un'altra.
    Derivata,
    /// Prima di «Salva e usa».
    Salvata,
    /// Prima di un'esportazione.
    Esportata,
    /// Chiesta a mano.
    Manuale,
    /// Il testo del formato 1, prima della migrazione.
    Migrata,
}

impl Causa {
    const fn nome(self) -> &'static str {
        match self {
            Self::Derivata => "derivata",
            Self::Salvata => "salvata",
            Self::Esportata => "esportata",
            Self::Manuale => "manuale",
            Self::Migrata => "migrata",
        }
    }

    fn da(nome: &str) -> Option<Self> {
        [
            Self::Derivata,
            Self::Salvata,
            Self::Esportata,
            Self::Manuale,
            Self::Migrata,
        ]
        .into_iter()
        .find(|causa| causa.nome() == nome)
    }
}

/// Quante istantanee si tengono per bozza.
///
/// Oltre, la cartella di una bozza cresce senza fine e nessuno se ne accorge — e
/// le venti più recenti sono già più di quante se ne guardino.
const QUANTE_ISTANTANEE: usize = 20;

/// Un'istantanea, come la mostra l'elenco.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IstantaneaIpc {
    /// Millisecondi dall'epoca. È anche la sua identità.
    pub quando: u64,
    /// `derivata`, `salvata`, `esportata`, `manuale`.
    pub causa: &'static str,
    /// Quante parti ridisegnava.
    pub parti: usize,
    /// Quanti token dichiarava.
    pub token: usize,
}

/// Il nome di un'istantanea porta tutto quel che l'elenco deve mostrare.
///
/// `<quando>-<causa>-<parti>-<token>.json`, e il contenuto è il manifest e
/// basta. Nessun file d'indice accanto: un indice è una seconda verità che prima
/// o poi non combacia con la cartella, e qui il costo di evitarlo è dividere una
/// stringa in quattro pezzi. Il contenuto resta un `skin.json` vero, quindi si
/// può copiare fuori e aprire senza passare da qui.
fn istantanea_da(nome: &str) -> Option<IstantaneaIpc> {
    let radice = nome.strip_suffix(".json")?;
    let mut pezzi = radice.split('-');
    let quando = pezzi.next()?.parse().ok()?;
    let causa = Causa::da(pezzi.next()?)?.nome();
    let parti = pezzi.next()?.parse().ok()?;
    let token = pezzi.next()?.parse().ok()?;
    if pezzi.next().is_some() {
        return None;
    }
    Some(IstantaneaIpc {
        quando,
        causa,
        parti,
        token,
    })
}

/// Il nome del file di un'istantanea: l'inverso esatto di `istantanea_da`.
fn nome_istantanea(istantanea: &IstantaneaIpc) -> String {
    format!(
        "{}-{}-{}-{}.json",
        istantanea.quando, istantanea.causa, istantanea.parti, istantanea.token
    )
}

/// Le istantanee di una bozza, la più recente per prima.
fn elenco_istantanee(cartella: &Path) -> Vec<IstantaneaIpc> {
    let Ok(voci) = std::fs::read_dir(cartella) else {
        // Nessuna cartella vuol dire nessuna istantanea, non un guasto: è il
        // caso di ogni bozza prima della prima prova.
        return Vec::new();
    };
    let mut elenco: Vec<IstantaneaIpc> = voci
        .flatten()
        .filter_map(|voce| {
            let nome = voce.file_name();
            istantanea_da(nome.to_str()?)
        })
        .collect();
    // Dalla più recente: `std::cmp::Reverse` invece di scambiare i due lati del
    // confronto, che è la forma che clippy chiede e che si legge meglio.
    elenco.sort_unstable_by_key(|istantanea| std::cmp::Reverse(istantanea.quando));
    elenco
}

/// Le istantanee di una bozza.
#[tauri::command(async)]
pub fn studio_istantanee(stato: State<'_, Stato>, id: String) -> Esito<Vec<IstantaneaIpc>> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        Ok(elenco_istantanee(&istantanee_di(dati, &id)))
    })
    .map_err(errore)
}

/// Prende un'istantanea, e pota le più vecchie.
///
/// Sta fuori dal comando perché il comando è un guscio: prende il lucchetto,
/// compone un percorso e chiama questa. Provare l'una senza costruire l'altro è
/// la stessa divisione che `skin.rs` fa con `installa`.
fn prendi_istantanea(cartella: &Path, sorgente: &str, causa: Causa) -> Result<(), AppError> {
    {
        std::fs::create_dir_all(cartella)
            .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;

        // I conteggi si prendono adesso e non a ogni lettura dell'elenco: sono
        // ciò che il documento **era**, e riparsare venti manifest per riempire
        // una colonna sarebbe lavoro rifatto a ogni apertura. Un documento a
        // metà di una parentesi si conta zero e zero — è una bozza salvata
        // apposta anche quando non è valida, e l'elenco lo dice invece di
        // rifiutarsi di prenderla.
        let (parti, token) = rymembra_skin::parse_skin_json(sorgente).map_or((0, 0), |documento| {
            (documento.parts.len(), documento.tokens.iter().count())
        });

        // Due istantanee nello stesso millisecondo si sovrascriverebbero: il
        // salvataggio e l'esportazione possono capitare a un battito di
        // distanza. Si cerca il primo istante libero invece di perderne una.
        let mut presa = IstantaneaIpc {
            quando: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |da_allora| {
                    u64::try_from(da_allora.as_millis()).unwrap_or(u64::MAX)
                }),
            causa: causa.nome(),
            parti,
            token,
        };
        while cartella.join(nome_istantanea(&presa)).exists() {
            presa.quando = presa.quando.saturating_add(1);
        }

        scrivi_atomico(&cartella.join(nome_istantanea(&presa)), sorgente.as_bytes())?;

        for vecchia in elenco_istantanee(cartella)
            .into_iter()
            .skip(QUANTE_ISTANTANEE)
        {
            // Un file che non si cancella non è un motivo per far fallire il
            // salvataggio: il lavoro è già su disco, ed è quello che conta.
            drop(std::fs::remove_file(
                cartella.join(nome_istantanea(&vecchia)),
            ));
        }
        Ok(())
    }
}

/// Prende un'istantanea del documento com'è adesso.
#[tauri::command(async)]
pub fn studio_istantanea(
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    causa: Causa,
) -> Esito<()> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        prendi_istantanea(&istantanee_di(dati, &id), &sorgente, causa)
    })
    .map_err(errore)
}

/// Il testo di un'istantanea.
///
/// `quando` è un numero, non un pezzo di percorso: il file si trova scorrendo la
/// cartella e confrontando, quindi non c'è nessun nome che arriva da fuori e
/// finisce dentro un `join`.
fn ripristina_istantanea(cartella: &Path, quando: u64) -> Result<String, AppError> {
    let trovata = elenco_istantanee(cartella)
        .into_iter()
        .find(|istantanea| istantanea.quando == quando)
        .ok_or_else(|| non_trovata(&quando.to_string()))?;
    let file = cartella.join(nome_istantanea(&trovata));
    std::fs::read_to_string(&file)
        .map_err(|err| rymembra_app::files::io_error(&file.display().to_string(), &err))
}

/// Riporta il documento a com'era in un'istantanea.
#[tauri::command(async)]
pub fn studio_ripristina(stato: State<'_, Stato>, id: String, quando: u64) -> Esito<String> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        ripristina_istantanea(&istantanee_di(dati, &id), quando)
    })
    .map_err(errore)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::DI_SERIE;

    #[test]
    fn il_registro_dice_tutto_quel_che_serve_a_un_editor() {
        let registro = studio_registro();
        assert_eq!(registro.tokens.len(), rymembra_skin::TOKENS.len());
        assert_eq!(registro.parts.len(), rymembra_skin::parts::PARTS.len());
        // Gli undici effetti, ognuno col suo costo vero. Se un effetto nuovo
        // nascesse e `esempio()` non lo conoscesse, questo test lo direbbe
        // invece di lasciarlo sparire in silenzio dall'elenco.
        assert_eq!(registro.effects.len(), rymembra_skin::Effect::NAMES.len());
        let sfoca = registro
            .effects
            .iter()
            .find(|e| e.name == "blurBehind")
            .expect("blurBehind");
        assert_eq!(
            sfoca.cost, registro.budget,
            "una sfocatura da sola è tutto il budget"
        );

        // L'esemplare minimo è quel che «Aggiungi livello» scrive nel documento.
        // Se non si lasciasse riparsare, l'editor comporrebbe un livello che la
        // validazione rifiuta un istante dopo averlo aggiunto.
        for effetto in &registro.effects {
            let documento = format!(
                r##"{{"format":1,"id":"esempio","meta":{{"name":"E","author":"A","version":"1.0.0"}},
                     "tokens":{{}},"patterns":{{"x":{}}}}}"##,
                effetto.esempio
            );
            let riletto = rymembra_skin::parse_skin_json(&documento)
                .unwrap_or_else(|err| panic!("«{}»: {err:?}", effetto.name));
            let (_, quale) = riletto.patterns.first().expect("il motivo");
            assert_eq!(quale.name(), effetto.name);
        }
    }

    #[test]
    fn la_validazione_non_fallisce_mai_su_un_documento_rotto() {
        // A metà di una parentesi il JSON non è JSON, ed è il caso normale
        // mentre si scrive: se questo comando tornasse un errore IPC, l'editor
        // dovrebbe distinguere «il comando è andato storto» da «non ho ancora
        // finito di scrivere».
        let esito = studio_valida("{ \"format\": 1, ".to_owned(), None);
        assert_eq!(esito.errori.len(), 1);
        assert!(esito.css.is_empty(), "niente foglio da un documento rotto");
    }

    /// Il difetto che si vedeva peggio: un errore di sintassi arrivava come
    /// `{ code, path: "", message: "", forse: [] }`, cioè un pannello rosso
    /// permanente che non diceva né dove né cosa. La frase che l'editor mostrava
    /// al suo posto veniva dal catalogo ed era scritta per il toast dell'app:
    /// «Aprila nello Studio: i problemi vengono elencati riga per riga» —
    /// letta da dentro lo Studio, e falsa.
    #[test]
    fn un_errore_di_sintassi_dice_dove_e_cosa() {
        // Una virgola che manca fra due campi, alla riga tre.
        let json = "{
  \"format\": 1
  \"id\": \"prova\"
}";
        let esito = studio_valida(json.to_owned(), None);
        let solo = match esito.errori.as_slice() {
            [solo] => solo,
            altri => panic!("un errore di sintassi è uno solo: {altri:#?}"),
        };
        assert_eq!(solo.code, "skin.manifestInvalid");
        assert_eq!(solo.riga, Some(3), "{solo:#?}");
        assert!(solo.colonna.is_some());
        assert!(
            solo.message.contains("virgola"),
            "il messaggio dice cosa manca, non «il manifest non è JSON»: {solo:#?}"
        );
    }

    /// Venti problemi erano venti frasi dentro **un** messaggio, unite da punti
    /// e virgola, con il percorso del primo. Il piede che conta «{n} errori»,
    /// l'elenco che li mostra e il giro di correzione della chat sono tutti e
    /// tre scritti per una lista: adesso ne ricevono una.
    #[test]
    fn ogni_problema_e_un_problema_e_porta_la_sua_riga() {
        // Solo nel primo `tokens`: dal formato 2 ce n'è un secondo, nella
        // variante, e lì gli stessi tre sbagli sarebbero altri tre errori.
        let json = rymembra_skin::PLAIN_SOURCE.replacen(
            "\"tokens\": {",
            "\"tokens\": {
    \"color.accents\": \"#ff0000\",
    \"radius.cards\": \"4px\",
    \"font.mainly\": [\"X\"],",
            1,
        );
        let esito = studio_valida(json, None);
        assert_eq!(
            esito.errori.len(),
            3,
            "tre token inesistenti sono tre errori: {:#?}",
            esito.errori
        );

        // Nell'ordine del **documento**, non in quello alfabetico in cui li
        // consegna la mappa di `serde_json`: si correggono dall'alto in basso.
        let percorsi: Vec<&str> = esito.errori.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            percorsi,
            [
                "tokens.color.accents",
                "tokens.radius.cards",
                "tokens.font.mainly"
            ]
        );

        // Righe vere, crescenti, e distinte: è quel che l'euristica a `indexOf`
        // non sapeva dare — cercando `"cards"` o `"accents"` prendeva la prima
        // occorrenza del file, che è quasi sempre un'altra riga.
        let righe: Vec<Option<u32>> = esito.errori.iter().map(|e| e.riga).collect();
        assert!(righe.iter().all(Option::is_some), "{righe:?}");
        assert!(righe.windows(2).all(|due| due[0] < due[1]), "{righe:?}");

        // E il suggerimento arriva come dato anche per un token, non solo per
        // una parte: senza, il bottone che corregge non si può costruire.
        let accento = &esito.errori[0];
        assert!(
            accento.forse.contains(&"color.accent".to_owned()),
            "{accento:#?}"
        );
    }

    /// Un avviso ha un percorso come un errore, quindi ha un posto: il bottone
    /// che ci porta il cursore è lo stesso, e non aveva un numero da usare.
    #[test]
    fn anche_un_avviso_sa_su_che_riga_sta() {
        let json = rymembra_skin::PLAIN_SOURCE.replace(
            "\"capabilities\": {",
            "\"patterns\": { \"mai-usato\": { \"effect\": \"vignette\", \"color\": \"#000\" } }, \"capabilities\": {",
        );
        let esito = studio_valida(json, None);
        let avviso = esito
            .avvisi
            .iter()
            .find(|a| a.kind == "unusedPattern")
            .unwrap_or_else(|| panic!("{:#?}", esito.avvisi));
        assert_eq!(avviso.path, "patterns.mai-usato");
        assert!(avviso.riga.is_some(), "{avviso:#?}");
    }

    #[test]
    fn un_nome_di_parte_sbagliato_arriva_col_suo_suggerimento() {
        let json = rymembra_skin::PLAIN_SOURCE.replace(
            "\"layout\": {",
            "\"parts\": { \"section-cards\": { \"radius\": \"10px\" } }, \"layout\": {",
        );
        let esito = studio_valida(json, None);
        let primo = esito.errori.first().expect("un errore");
        assert!(
            primo.forse.contains(&"section-card".to_owned()),
            "il suggerimento di nearest_parts deve arrivare come dato, non solo \
             dentro la frase: {primo:?}"
        );
    }

    #[test]
    fn la_skin_di_serie_passa_dallo_studio_senza_errori() {
        let esito = studio_valida(rymembra_skin::PLAIN_SOURCE.to_owned(), None);
        assert!(esito.errori.is_empty(), "{:#?}", esito.errori);
        assert!(esito.avvisi.is_empty(), "{:#?}", esito.avvisi);
        assert!(!esito.css.is_empty());
        assert_eq!(esito.costo, 0);
        // E la tabella dei contrasti è piena: è quel che lo Studio mostra a
        // destra, e una tabella vuota vorrebbe dire che nessun colore si è
        // lasciato risolvere.
        assert!(esito.contrasti.len() > 10);
        assert!(
            esito.contrasti.iter().all(|c| c.passa),
            "{:#?}",
            esito.contrasti
        );
    }

    #[test]
    fn una_bozza_non_puo_uscire_dalla_sua_cartella() {
        for cattivo in ["../../altrove", "..\\fuori", "c:/assoluto", ""] {
            assert!(!id_sicuro(cattivo), "«{cattivo}» non è stato respinto");
        }
        assert!(id_sicuro("ruggine"));
        assert!(id_sicuro("fork_di_plain-2"));
    }

    // ── le istantanee ───────────────────────────────────────────────────────

    #[test]
    fn il_nome_di_unistantanea_si_legge_e_si_riscrive_uguale() {
        // Il nome del file **è** l'indice: se andata e ritorno non combaciano,
        // un'istantanea si scrive e non si ritrova più, e il difetto si vede
        // solo dopo — quando serve.
        let nome = "1754121234567-salvata-34-41.json";
        let letta = istantanea_da(nome).expect("un nome buono");
        assert_eq!(letta.quando, 1_754_121_234_567);
        assert_eq!(letta.causa, "salvata");
        assert_eq!((letta.parti, letta.token), (34, 41));
        assert_eq!(nome_istantanea(&letta), nome);

        for storto in [
            "1754121234567-salvata-34.json",       // manca un pezzo
            "1754121234567-battezzata-34-41.json", // una causa che non esiste
            "adesso-salvata-34-41.json",           // un istante che non è un numero
            "1754121234567-salvata-34-41-42.json", // un pezzo di troppo
            "1754121234567-salvata-34-41.txt",     // non è un manifest
        ] {
            assert!(istantanea_da(storto).is_none(), "«{storto}» è passato");
        }
    }

    #[test]
    fn unistantanea_si_prende_si_rilegge_e_si_ripristina_identica() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("presa");

        let elenco = elenco_istantanee(dir.path());
        assert_eq!(elenco.len(), 1);
        let sola = elenco.first().expect("una");
        assert_eq!(sola.causa, "salvata");
        // I conteggi sono quelli del documento, presi al momento della scrittura.
        assert_eq!(sola.token, {
            let documento =
                rymembra_skin::parse_skin_json(rymembra_skin::PLAIN_SOURCE).expect("plain");
            documento.tokens.iter().count()
        });

        assert_eq!(
            ripristina_istantanea(dir.path(), sola.quando).expect("ripristinata"),
            rymembra_skin::PLAIN_SOURCE,
            "il file è il manifest e basta: si copia fuori e si apre"
        );
    }

    #[test]
    fn una_bozza_a_meta_si_puo_comunque_fotografare() {
        // Una bozza si salva anche quando non è valida — è un lavoro in corso —
        // e allora anche la sua istantanea deve poter esistere. I conteggi non
        // si sanno, e l'elenco dice zero invece di rifiutare la fotografia.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        prendi_istantanea(dir.path(), "{ \"format\": 1, ", Causa::Manuale).expect("presa");
        let sola = elenco_istantanee(dir.path()).first().copied().expect("una");
        assert_eq!((sola.parti, sola.token), (0, 0));
    }

    #[test]
    fn oltre_il_ventesimo_giro_la_piu_vecchia_sparisce() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for _ in 0..(QUANTE_ISTANTANEE + 5) {
            prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Manuale)
                .expect("presa");
        }
        let elenco = elenco_istantanee(dir.path());
        assert_eq!(
            elenco.len(),
            QUANTE_ISTANTANEE,
            "la potatura non ha tagliato"
        );
        // E quelle rimaste sono le **ultime**, in ordine dalla più recente.
        assert!(
            elenco.windows(2).all(|coppia| {
                coppia.first().is_some_and(|prima| {
                    coppia.get(1).is_some_and(|dopo| prima.quando > dopo.quando)
                })
            }),
            "l'ordine non è dalla più recente: {elenco:#?}"
        );
    }

    #[test]
    fn due_istantanee_nello_stesso_istante_non_si_sovrascrivono() {
        // «Salva e usa» ed «Esporta» possono capitare a un battito di distanza,
        // e due file con lo stesso nome vorrebbero dire perderne una in
        // silenzio.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("una");
        prendi_istantanea(dir.path(), rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("due");
        assert_eq!(elenco_istantanee(dir.path()).len(), 2);
    }

    #[test]
    fn le_istantanee_di_una_bozza_che_non_ne_ha_sono_nessuna() {
        // Nessuna cartella non è un guasto: è il caso di ogni bozza prima della
        // prima prova, ed è quel che la colonna sinistra chiede all'apertura.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert!(elenco_istantanee(&dir.path().join("mai-esistita")).is_empty());
    }

    // ── il pacchetto ────────────────────────────────────────────────────────

    #[test]
    fn lalbero_del_pacchetto_dice_quel_che_lesportazione_scriverebbe() {
        // La skin di serie non ha risorse: una voce sola, il manifest. Se un
        // giorno l'albero mostrasse un file che `write_skin_package` non
        // riscrive, direbbe una bugia a chi poi prova a esportare.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let da = da_lavorare(dir.path(), DI_SERIE).expect("la skin di serie");
        assert_eq!(da.sorgente, rymembra_skin::PLAIN_SOURCE);
        assert!(da.preview.is_none());
        assert!(da.assets.is_empty());
    }

    #[test]
    fn ogni_skin_di_serie_si_apre_e_si_deriva() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for (id, testo) in rymembra_skin::DI_SERIE {
            let da = da_lavorare(dir.path(), id).expect("una skin di serie");
            assert_eq!(da.sorgente, *testo, "{id}");
        }
        assert!(da_lavorare(dir.path(), "non-esiste").is_err());
    }

    #[test]
    fn una_bozza_vince_sul_pacchetto_ma_le_risorse_restano_quelle() {
        // È l'invariante che tiene insieme l'albero e l'esportazione: il
        // manifest può venire da una bozza, le risorse no — una bozza è un
        // `skin.json` e basta.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let bozza = cartella_bozze(dir.path()).join("plain");
        std::fs::create_dir_all(&bozza).expect("cartella");
        let modificata =
            rymembra_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
        std::fs::write(bozza.join("skin.json"), &modificata).expect("scritta");

        let da = da_lavorare(dir.path(), DI_SERIE).expect("la bozza");
        assert_eq!(da.sorgente, modificata, "la bozza deve vincere sul binario");
    }

    #[test]
    fn ogni_effetto_porta_le_sue_manopole() {
        // La tabella dei parametri è scritta a mano perché `Effect` è un enum di
        // varianti tipizzate e non c'è niente da cui leggerla a macchina. Questo
        // test è il prezzo di quella scelta: un effetto nuovo senza la sua riga
        // arriverebbe all'editor come un livello che si aggiunge e non si può
        // aprire, che è il difetto da cui `params` è nato.
        let registro = studio_registro();
        assert_eq!(
            registro.effects.len(),
            rymembra_skin::effects::Effect::NAMES.len()
        );
        for effetto in &registro.effects {
            assert!(
                !effetto.params.is_empty(),
                "«{}» non dichiara nessuna manopola",
                effetto.name
            );
        }
    }

    #[test]
    fn le_manopole_dichiarate_esistono_davvero_nell_esemplare() {
        // Un parametro obbligatorio deve comparire nell'esemplare minimo: se non
        // ci fosse, l'editor mostrerebbe un controllo vuoto per un campo che il
        // parser pretende, e il primo salvataggio fallirebbe.
        for effetto in studio_registro().effects {
            let esempio: serde_json::Value =
                serde_json::from_str(effetto.esempio).expect("l'esemplare è JSON");
            for parametro in effetto.params.iter().filter(|p| !p.optional) {
                assert!(
                    esempio.get(parametro.name).is_some(),
                    "«{}» dichiara «{}» obbligatorio e non lo scrive nell'esemplare",
                    effetto.name,
                    parametro.name
                );
            }
        }
    }

    #[test]
    fn buttata_la_bozza_torna_a_parlare_il_pacchetto() {
        // Il comando vive dietro `con_libreria` e qui non si può chiamare, ma
        // quel che deve garantire sì: tolta la cartella della bozza,
        // `da_lavorare` — la stessa funzione che decide all'apertura — deve
        // tornare a leggere il pacchetto. Senza questo, «butta la bozza»
        // lascerebbe l'editor su un documento che non sta più da nessuna parte.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let bozza = cartella_bozze(dir.path()).join(DI_SERIE);
        std::fs::create_dir_all(&bozza).expect("cartella");
        let modificata =
            rymembra_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
        std::fs::write(bozza.join("skin.json"), &modificata).expect("scritta");
        assert_eq!(
            da_lavorare(dir.path(), DI_SERIE)
                .expect("la bozza")
                .sorgente,
            modificata
        );

        std::fs::remove_dir_all(&bozza).expect("buttata");
        assert_eq!(
            da_lavorare(dir.path(), DI_SERIE)
                .expect("il pacchetto")
                .sorgente,
            rymembra_skin::PLAIN_SOURCE
        );
    }

    #[test]
    fn la_conversazione_sta_con_la_bozza_e_ne_segue_la_sorte() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert_eq!(leggi_chat(dir.path(), "mia").expect("letta"), None);

        scrivi_chat(dir.path(), "mia", r#"{"versione":1,"battute":[]}"#).expect("scritta");
        assert_eq!(
            leggi_chat(dir.path(), "mia").expect("letta").as_deref(),
            Some(r#"{"versione":1,"battute":[]}"#)
        );

        // Una rinomina la porta con sé.
        rinomina_bozza(dir.path(), "mia", "nuova", None).expect("rinominata");
        assert_eq!(leggi_chat(dir.path(), "mia").expect("letta"), None);
        assert!(leggi_chat(dir.path(), "nuova").expect("letta").is_some());

        // Vuota, si toglie; due volte non è un guasto.
        scrivi_chat(dir.path(), "nuova", "").expect("tolta");
        scrivi_chat(dir.path(), "nuova", "").expect("già tolta");
        assert_eq!(leggi_chat(dir.path(), "nuova").expect("letta"), None);
    }

    #[test]
    fn una_conversazione_storta_non_si_scrive() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for cattiva in ["non json", "[1,2]", "\"testo\""] {
            assert!(
                scrivi_chat(dir.path(), "mia", cattiva).is_err(),
                "«{cattiva}»"
            );
        }
        let enorme = format!(r#"{{"x":"{}"}}"#, "a".repeat(CHAT_MASSIMA));
        assert!(scrivi_chat(dir.path(), "mia", &enorme).is_err());
        for cattivo in ["../fuori", ""] {
            assert!(scrivi_chat(dir.path(), cattivo, "{}").is_err());
            assert!(leggi_chat(dir.path(), cattivo).is_err());
        }
    }

    #[test]
    fn un_id_storto_non_raggiunge_ne_il_pacchetto_ne_le_istantanee() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        for cattivo in ["../../altrove", "..\\fuori", "c:/assoluto", ""] {
            assert!(
                da_lavorare(dir.path(), cattivo).is_err(),
                "«{cattivo}» ha raggiunto un pacchetto"
            );
        }
    }

    // ── il registro del movimento ───────────────────────────────────────────

    /// Un documento di prova con un'animazione dal fotogramma dato.
    fn con_fotogramma(campo: &str, valore: &str) -> String {
        rymembra_skin::PLAIN_SOURCE.replacen(
            "\"motion\": {",
            &format!(
                "\"motion\": {{ \"animations\": {{ \"prova\": {{ \"duration\": \"200ms\", \
                 \"frames\": [{{ \"at\": 0, \"{campo}\": {valore} }}, {{ \"at\": 100, \"opacity\": 1 }}] }} }},"
            ),
            1,
        )
    }

    #[test]
    fn gli_estremi_dei_fotogrammi_sono_quelli_del_parser() {
        let registro = studio_registro();
        assert_eq!(registro.stati.len(), rymembra_skin::PartState::ALL.len());
        assert!(
            registro
                .inneschi
                .iter()
                .any(|i| i.name == "hover" && i.ciclo && i.stato)
        );
        assert!(
            registro
                .inneschi
                .iter()
                .any(|i| i.name == "exit" && i.vista && !i.ciclo)
        );
        assert!(!registro.animazioni_pronte.is_empty());
        // Lo schema arriva intero: il completamento del codice ci conta.
        assert!(registro.chiavi["aspetto"].contains(&"focusRing"));
        assert!(registro.chiavi["documento"].contains(&"variant"));
        assert!(registro.parole["easing"].contains(&"ease-out"));
        assert!(registro.parole["borderStyle"].contains(&"dashed"));
        assert!(!registro.sorgenti.is_empty());
        for campo in &registro.fotogramma {
            let (Some(min), Some(max)) = (campo.min, campo.max) else {
                continue;
            };
            let scrivi = |n: f64| match campo.kind {
                "length" => format!("\"{n}px\""),
                _ => n.to_string(),
            };
            for dentro in [min, max] {
                let testo = con_fotogramma(campo.name, &scrivi(dentro));
                assert!(
                    rymembra_skin::parse_skin_json(&testo).is_ok(),
                    "«{}» = {dentro} dovrebbe passare: {:?}",
                    campo.name,
                    rymembra_skin::parse_skin_json(&testo).err()
                );
            }
            let fuori = con_fotogramma(campo.name, &scrivi(max + 1.0));
            assert!(
                rymembra_skin::parse_skin_json(&fuori).is_err(),
                "«{}» = {} dovrebbe essere rifiutato",
                campo.name,
                max + 1.0
            );
        }
    }

    #[test]
    fn la_validazione_scrive_per_la_tela_e_dice_i_costi() {
        let esito = studio_valida(
            rymembra_skin::di_serie("vinile")
                .expect("vinile")
                .to_owned(),
            None,
        );
        assert!(esito.errori.is_empty(), "{:?}", esito.errori);
        assert!(esito.css.contains(":root[data-skin='vinile'] ."));
        assert!(esito.css.contains("[data-forza~='hover']"));
        let colonna = esito
            .costi
            .iter()
            .find(|c| c.parte == "column-art")
            .expect("column-art");
        assert!(colonna.movimento > 0);
        assert!(esito.costo_finestra > 0);
    }

    // ── le risorse della bozza ──────────────────────────────────────────────

    #[test]
    fn il_nome_di_una_risorsa_si_mette_in_regola() {
        assert_eq!(
            nome_risorsa(Path::new("C:/x/Titoli Grassetto.WOFF2")),
            "titoli-grassetto.woff2"
        );
        assert_eq!(nome_risorsa(Path::new("/x/foto.JPEG")), "foto.jpg");
        assert_eq!(nome_risorsa(Path::new("/x/__.png")), "risorsa.png");
    }

    #[test]
    fn una_risorsa_si_aggiunge_si_rinomina_se_occupato_e_si_rifiuta_se_mente() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let fuori = tempfile::tempdir().expect("cartella dei file");
        let buono = fuori.path().join("Trama.png");
        std::fs::write(&buono, b"\x89PNG\r\n\x1a\n resto").expect("scritto");
        let voce = aggiungi_risorsa(dir.path(), DI_SERIE, &buono).expect("aggiunta");
        assert_eq!(voce.nome, "assets/trama.png");
        let seconda = aggiungi_risorsa(dir.path(), DI_SERIE, &buono).expect("aggiunta");
        assert_eq!(seconda.nome, "assets/trama-2.png");

        let bugiardo = fuori.path().join("finta.png");
        std::fs::write(&bugiardo, b"GIF89a").expect("scritto");
        assert!(aggiungi_risorsa(dir.path(), DI_SERIE, &bugiardo).is_err());

        let da = da_lavorare(dir.path(), DI_SERIE).expect("la bozza");
        let nomi: Vec<&str> = da.assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(nomi, ["trama-2.png", "trama.png"]);
    }

    #[test]
    fn le_risorse_del_pacchetto_passano_nella_bozza_all_apertura() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let sorgente = rymembra_skin::PLAIN_SOURCE
            .replace("\"id\": \"plain\"", "\"id\": \"nocturne\"")
            .replacen(
                "\"id\": \"nocturne\",",
                "\"id\": \"nocturne\", \"fonts\": { \"Prova\": { \"src\": \"prova.woff2\" } },",
                1,
            );
        let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[rymembra_skin::package::SkinAsset {
                name: "prova.woff2".to_owned(),
                mime: "font/woff2",
                bytes: b"wOF2 finto".to_vec(),
            }],
        })
        .expect("impacchettata");
        let skin = dir.path().join("skin");
        std::fs::create_dir_all(&skin).expect("cartella");
        std::fs::write(skin.join("nocturne.ryskin"), bytes).expect("installata");

        apri_documento(dir.path(), "nocturne", true).expect("aperta");
        assert!(
            cartella_risorse(dir.path(), "nocturne")
                .join("prova.woff2")
                .is_file()
        );
        // E «Salva e usa» ha di nuovo il carattere da impacchettare.
        let (_, assets) = corredo_della_bozza(dir.path(), "nocturne").expect("corredo");
        assert_eq!(assets.len(), 1);
    }

    // ── le istantanee fuori dalla bozza ─────────────────────────────────────

    #[test]
    fn scartare_la_bozza_non_porta_via_le_istantanee() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        prendi_istantanea(
            &istantanee_di(dir.path(), DI_SERIE),
            rymembra_skin::PLAIN_SOURCE,
            Causa::Manuale,
        )
        .expect("presa");
        let bozza = cartella_bozze(dir.path()).join(DI_SERIE);
        std::fs::create_dir_all(&bozza).expect("bozza");
        std::fs::write(bozza.join("skin.json"), rymembra_skin::PLAIN_SOURCE).expect("scritta");
        std::fs::remove_dir_all(&bozza).expect("scartata");
        assert_eq!(
            elenco_istantanee(&istantanee_di(dir.path(), DI_SERIE)).len(),
            1
        );
    }

    #[test]
    fn le_istantanee_della_2x_traslocano_fuori_dalla_bozza() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let vecchia = cartella_bozze(dir.path()).join(DI_SERIE).join("istantanee");
        prendi_istantanea(&vecchia, rymembra_skin::PLAIN_SOURCE, Causa::Salvata).expect("presa");
        let elenco = elenco_istantanee(&istantanee_di(dir.path(), DI_SERIE));
        assert_eq!(elenco.len(), 1);
        assert!(!vecchia.exists());
    }

    #[test]
    fn una_skin_del_formato_1_si_apre_migrata_e_ricorda_com_era() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let v1 = include_str!("../../../../core/rymembra-skin/tests/oro/scafalatura.v1.json");
        let id = rymembra_skin::parse_skin_json(v1).expect("si legge").id;
        let bozza = cartella_bozze(dir.path()).join(&id);
        std::fs::create_dir_all(&bozza).expect("bozza");
        std::fs::write(bozza.join("skin.json"), v1).expect("scritta");

        let aperta = apri_documento(dir.path(), &id, true).expect("aperta");
        assert!(aperta.contains("\"format\": 2"));
        let istantanee = elenco_istantanee(&istantanee_di(dir.path(), &id));
        assert_eq!(istantanee.first().map(|i| i.causa), Some("migrata"));
        // La seconda apertura trova la bozza già migrata: niente istantanea nuova.
        apri_documento(dir.path(), &id, true).expect("riaperta");
        assert_eq!(elenco_istantanee(&istantanee_di(dir.path(), &id)).len(), 1);
    }

    #[test]
    fn rinominare_la_bozza_porta_con_se_le_istantanee() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let bozza = cartella_bozze(dir.path()).join("vecchio");
        std::fs::create_dir_all(&bozza).expect("bozza");
        std::fs::write(bozza.join("skin.json"), "{}").expect("scritta");
        prendi_istantanea(&istantanee_di(dir.path(), "vecchio"), "{}", Causa::Manuale)
            .expect("presa");

        rinomina_bozza(dir.path(), "vecchio", "nuovo", None).expect("rinominata");
        assert!(
            cartella_bozze(dir.path())
                .join("nuovo")
                .join("skin.json")
                .is_file()
        );
        assert_eq!(
            elenco_istantanee(&istantanee_di(dir.path(), "nuovo")).len(),
            1
        );
        // Sopra una skin di serie non si scrive.
        assert!(rinomina_bozza(dir.path(), "nuovo", "plain", None).is_err());
    }

    // ── WP31: lo Studio che non perde lavoro ────────────────────────────────

    /// Il manifest di serie con un altro id.
    fn con_id(id: &str) -> String {
        rymembra_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""))
    }

    /// Il manifest di serie con un altro id e un carattere del pacchetto.
    fn con_carattere(id: &str) -> String {
        con_id(id).replacen(
            &format!("\"id\": \"{id}\","),
            &format!(
                "\"id\": \"{id}\", \"fonts\": {{ \"Prova\": {{ \"src\": \"prova.woff2\" }} }},"
            ),
            1,
        )
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n resto";
    const WOFF2: &[u8] = b"wOF2 finto con la firma giusta";

    fn installa_qui(dir: &Path, sorgente: &str, assets: &[rymembra_skin::package::SkinAsset]) {
        let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
            source: sorgente,
            preview: Some(PNG),
            assets,
        })
        .expect("impacchettata");
        crate::skin::installa(dir, &bytes).expect("installata");
    }

    fn carattere() -> rymembra_skin::package::SkinAsset {
        rymembra_skin::package::SkinAsset {
            name: "prova.woff2".to_owned(),
            mime: "font/woff2",
            bytes: WOFF2.to_vec(),
        }
    }

    #[test]
    fn la_tela_si_compila_sotto_l_id_della_bozza() {
        // Il documento dice «nuovo-nome», la bozza sta ancora in «vecchio»:
        // i selettori e gli indirizzi delle risorse seguono la cartella.
        let sorgente = con_carattere("nuovo-nome");
        let esito = studio_valida(sorgente.clone(), Some("vecchio".to_owned()));
        assert!(esito.errori.is_empty(), "{:?}", esito.errori);
        assert_eq!(esito.radice, "vecchio");
        assert!(esito.css.contains(":root[data-skin='vecchio']"));
        assert!(
            esito.css.contains("/bozza/vecchio/prova.woff2"),
            "{}",
            esito.css
        );
        assert!(!esito.css.contains("data-skin='nuovo-nome'"));

        // Un id che il foglio non sa scrivere così com'è ricade sul documento.
        let ricaduta = studio_valida(sorgente.clone(), Some("con_trattino".to_owned()));
        assert_eq!(ricaduta.radice, "nuovo-nome");

        // La stessa domanda ha la stessa risposta, dai ricordi: anche i
        // millisecondi sono quelli della prima volta.
        let ancora = studio_valida(sorgente, Some("vecchio".to_owned()));
        assert_eq!(ancora.css, esito.css);
        assert_eq!(ancora.compilato_ms, esito.compilato_ms);
    }

    #[test]
    fn la_prova_sull_app_porta_l_id_della_bozza() {
        let skin = compila_per_la_prova(&con_id("altro"), "mia").expect("compilata");
        assert_eq!(skin.id, "mia");
        assert!(skin.css.contains(":root[data-skin='mia']"));
        assert!(compila_per_la_prova("{ rotto", "mia").is_err());
    }

    #[test]
    fn salvare_segue_l_id_del_documento_e_non_prende_quello_di_un_altro() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let manifest = |id: &str| cartella_bozze(dir.path()).join(id).join("skin.json");

        let primo = salva_bozza(dir.path(), "prima", &con_id("prima"), None).expect("salvata");
        assert_eq!(primo.id, "prima");
        assert!(primo.rinomina.is_none());

        // Il documento cambia id: la bozza lo segue, in un passo solo.
        let seconda = con_id("seconda");
        let esito = salva_bozza(dir.path(), "prima", &seconda, None).expect("salvata");
        assert_eq!(esito.id, "seconda");
        assert_eq!(
            std::fs::read_to_string(manifest("seconda")).expect("letta"),
            seconda
        );
        assert!(!cartella_bozze(dir.path()).join("prima").exists());

        // L'id di una skin installata di un altro: il testo si salva dov'era,
        // e il rifiuto torna indietro col suo codice invece di sparire.
        installa_qui(dir.path(), &con_id("altrui"), &[]);
        let rubata = con_id("altrui");
        let esito = salva_bozza(dir.path(), "seconda", &rubata, None).expect("salvata");
        assert_eq!(esito.id, "seconda");
        assert_eq!(
            esito.rinomina.as_ref().map(|e| e.code.as_str()),
            Some("skin.idConflict")
        );
        assert_eq!(
            std::fs::read_to_string(manifest("seconda")).expect("letta"),
            rubata
        );
        assert!(!cartella_bozze(dir.path()).join("altrui").exists());

        // Ma tornare all'id da cui lo Studio è partito si può.
        let esito = salva_bozza(dir.path(), "seconda", &rubata, Some("altrui")).expect("salvata");
        assert_eq!(esito.id, "altrui");
        assert!(esito.rinomina.is_none());

        // Un testo a metà si salva lo stesso, senza rinomine.
        let esito = salva_bozza(dir.path(), "altrui", "{ \"id\": ", None).expect("salvata");
        assert_eq!(esito.id, "altrui");
    }

    #[test]
    fn una_skin_installata_che_non_passa_piu_si_apre_nello_studio() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let rotta = con_id("vecchia").replacen(
            "\"tokens\": {",
            "\"tokens\": { \"color.accents\": \"#ff0000\",",
            1,
        );
        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        std::fs::write(
            cartella.join("vecchia.ryskin"),
            crate::skin::zip_a_mano(&[("skin.json", rotta.as_bytes()), ("assets/trama.png", PNG)]),
        )
        .expect("scritta");
        assert_eq!(
            apri_documento(dir.path(), "vecchia", true).expect("aperta"),
            rotta
        );
        assert!(
            cartella_risorse(dir.path(), "vecchia")
                .join("trama.png")
                .is_file()
        );
        // E la validazione dice dove.
        assert!(
            !studio_valida(rotta, Some("vecchia".to_owned()))
                .errori
                .is_empty()
        );

        // Un archivio che non si apre, con una bozza accanto: si apre la bozza.
        std::fs::write(cartella.join("guasta.ryskin"), b"non sono uno zip").expect("scritto");
        let bozza = cartella_bozze(dir.path()).join("guasta");
        std::fs::create_dir_all(&bozza).expect("bozza");
        std::fs::write(bozza.join("skin.json"), con_id("guasta")).expect("scritta");
        assert_eq!(
            da_lavorare(dir.path(), "guasta")
                .expect("la bozza")
                .sorgente,
            con_id("guasta")
        );
        // Senza la bozza il guasto si dice, invece di passare per un'assenza.
        std::fs::write(cartella.join("sola.ryskin"), b"non sono uno zip").expect("scritto");
        let guasto = da_lavorare(dir.path(), "sola").err().expect("un guasto");
        assert_eq!(guasto.code().kind().code(), "skin.packageCorrupt");
    }

    #[test]
    fn un_tema_nuovo_porta_le_risorse_della_base_e_non_eredita_bozze_orfane() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        installa_qui(dir.path(), &con_carattere("base"), &[carattere()]);

        let figlia = con_carattere("figlia");
        let voce = crea_tema(dir.path(), "base", &figlia).expect("creato");
        assert_eq!(voce.id, "figlia");
        let letto = rymembra_skin::read_skin_package(
            &std::fs::read(dir.path().join("skin").join("figlia.ryskin")).expect("installato"),
        )
        .expect("si rilegge");
        assert_eq!(letto.assets.len(), 1, "il carattere della base");
        assert_eq!(
            letto.preview.as_deref(),
            Some(PNG),
            "la miniatura della base"
        );
        assert_eq!(
            std::fs::read_to_string(cartella_bozze(dir.path()).join("figlia").join("skin.json"))
                .expect("la bozza"),
            figlia
        );

        // Una bozza rimasta da una skin che non c'è più: il suo id è preso.
        std::fs::create_dir_all(cartella_bozze(dir.path()).join("orfana")).expect("orfana");
        let err = crea_tema(dir.path(), "base", &con_carattere("orfana")).expect_err("rifiutato");
        assert_eq!(err.code().kind().code(), "skin.idConflict");
        assert!(!dir.path().join("skin").join("orfana.ryskin").exists());
        let presi = occupati(dir.path());
        assert!(presi.contains(&"orfana".to_owned()) && presi.contains(&"figlia".to_owned()));
        // E quello di una skin installata, o di serie, pure.
        assert!(crea_tema(dir.path(), "base", &con_carattere("base")).is_err());
        assert!(crea_tema(dir.path(), "base", rymembra_skin::PLAIN_SOURCE).is_err());
    }

    #[test]
    fn scartare_la_bozza_di_una_skin_del_formato_1_non_aggiunge_istantanee() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let v1 = include_str!("../../../../core/rymembra-skin/tests/oro/scafalatura.v1.json");
        let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
            source: v1,
            preview: None,
            assets: &[],
        })
        .expect("impacchettata");
        crate::skin::installa(dir.path(), &bytes).expect("installata");
        let aperta = apri_documento(dir.path(), "scafalatura", true).expect("aperta");
        let serie = || elenco_istantanee(&istantanee_di(dir.path(), "scafalatura")).len();
        assert_eq!(serie(), 1, "la fotografia del testo del formato 1");
        let riaperta = scarta_bozza(dir.path(), "scafalatura").expect("scartata");
        assert_eq!(riaperta, aperta);
        assert_eq!(
            serie(),
            1,
            "lo scarto non fotografa di nuovo lo stesso testo"
        );
    }
}
