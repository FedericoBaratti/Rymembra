//! Il registro dello Studio: il vocabolario che una skin può usare, letto dai
//! registri del nucleo e messo nella forma che l'editor mostra (`studio_registro`).

use rymembra_skin::movimento;
use serde::Serialize;

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
