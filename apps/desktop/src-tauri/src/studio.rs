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
//! `Aether - impalcatura` — clicca o programma — non sono due modelli
//! sincronizzati: sono due tastiere sullo stesso documento. E il documento che
//! esce è il file che si mette in git.

use std::path::{Path, PathBuf};

use aether_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::skin::DI_SERIE;
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
    /// Ha uno pseudo-elemento libero per un livello aggiuntivo.
    pub layers: bool,
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
    /// `color`, `length`, `angle`, `number`, `stops`, `corners`, `word`.
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
                "Quali angoli. Nessuno dichiarato vuol dire tutti."
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
}

const fn nome_gruppo_token(group: aether_skin::tokens::TokenGroup) -> &'static str {
    use aether_skin::tokens::TokenGroup as G;
    match group {
        G::Typography => "Tipografia",
        G::Surface => "Superfici",
        G::Text => "Testo",
        G::Accent => "Accento",
        G::Status => "Stati",
        G::Chrome => "Cornice",
        G::Layout => "Impaginazione",
        G::Rhythm => "Ritmo",
        G::Geometry => "Geometria",
        G::Depth => "Profondità",
        G::Elevation => "Sopraelevazione",
        G::Motion => "Movimento",
        G::Canvas => "Tela",
    }
}

const fn nome_tipo(kind: aether_skin::tokens::TokenKind) -> &'static str {
    use aether_skin::tokens::TokenKind as K;
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

const fn nome_gruppo_parte(group: aether_skin::parts::PartGroup) -> &'static str {
    use aether_skin::parts::PartGroup as G;
    match group {
        G::Shell => "Cornice",
        G::Nav => "Navigazione",
        G::Page => "Pagina",
        G::Controls => "Controlli",
        G::Lists => "Elenchi",
        G::Player => "Lettore",
        G::NowPlaying => "In riproduzione",
        G::Overlays => "Sovrapposizioni",
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
        tokens: aether_skin::TOKENS
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
        presets: aether_skin::PRESETS
            .iter()
            .map(|def| PresetIpc {
                id: def.id,
                nome: def.nome,
                group: nome_gruppo_token(def.group),
                valori: def.valori.to_vec(),
            })
            .collect(),
        parts: aether_skin::parts::PARTS
            .iter()
            .map(|def| ParteIpc {
                name: def.name,
                group: nome_gruppo_parte(def.group),
                description: def.description,
                layers: def.layers,
            })
            .collect(),
        effects: aether_skin::Effect::NAMES
            .iter()
            .filter_map(|nome| esempio(nome))
            .collect(),
        widgets: aether_skin::WIDGETS.iter().map(widget).collect(),
        vocabolario: vocabolario(),
        budget: aether_skin::SURFACE_COST_BUDGET,
        shell_budget: aether_skin::SHELL_COST_BUDGET,
        format: aether_skin::SKIN_FORMAT_VERSION,
        contrasto_minimo: aether_skin::CONTRASTO_MINIMO,
    }
}

/// Un widget del registro, per la tavolozza dell'editor.
fn widget(def: &'static aether_skin::WidgetDef) -> WidgetIpc {
    use aether_skin::layout::Essential;

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

fn opzione(def: &'static aether_skin::layout::WidgetOption) -> OpzioneIpc {
    use crate::skin::ValoreOpzioneIpc as V;
    use aether_skin::layout::OptionKind;

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
    use aether_skin::layout::{Align, GapStep, Spread, ZoneKind};
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
        _ => return None,
    };
    // Un documento minimo che dichiara quell'effetto come motivo: è la via più
    // corta per ottenere un `Effect` senza esporre i suoi costruttori.
    let documento = format!(
        r##"{{"format":1,"id":"esempio","meta":{{"name":"E","author":"A","version":"1.0.0"}},
             "tokens":{{}},"patterns":{{"x":{minimo}}}}}"##
    );
    let skin = aether_skin::parse_skin_json(&documento).ok()?;
    let (_, effetto) = skin.patterns.first()?;
    Some(EffettoIpc {
        name: effetto.name(),
        cost: effetto.cost().weight(),
        esempio: minimo,
        params: parametri(effetto.name()),
        target: match effetto.target() {
            aether_skin::effects::EffectTarget::Background => "background",
            aether_skin::effects::EffectTarget::ClipPath => "clipPath",
            aether_skin::effects::EffectTarget::Filter => "filter",
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
    /// `costBudget`, `contrast`.
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
    /// Il rapporto nel tema scuro.
    pub scuro: f64,
    /// Il rapporto nel tema chiaro, se c'è.
    pub chiaro: Option<f64>,
    /// Passa la soglia in tutti i temi misurati.
    pub passa: bool,
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
    /// Quante parti la skin ridisegna.
    pub parti: usize,
    /// I token che seguono la copertina.
    pub dinamici: Vec<String>,
    /// Quanti millisecondi ci ha messo a compilare.
    pub compilato_ms: u128,
}

const fn nome_avviso(kind: aether_skin::WarningKind) -> &'static str {
    use aether_skin::WarningKind as W;
    match kind {
        W::MissingRequiredToken => "missingRequiredToken",
        W::UnkeptCapability => "unkeptCapability",
        W::UnusedPattern => "unusedPattern",
        W::UnusedPrefab => "unusedPrefab",
        W::CostBudget => "costBudget",
        W::Contrast => "contrast",
    }
}

/// Valida e compila un documento skin scritto a mano.
///
/// **Non fallisce mai.** Un documento non valido è il caso normale mentre lo si
/// scrive — a metà di una parentesi il JSON non è JSON — e restituire un errore
/// IPC costringerebbe l'editor a distinguere «il comando è andato storto» da «il
/// documento non è ancora finito». Qui gli errori sono un campo del risultato.
#[tauri::command]
#[must_use]
pub fn studio_valida(sorgente: String) -> ValidazioneIpc {
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
        parti: 0,
        dinamici: Vec::new(),
        compilato_ms: 0,
    };

    // Le posizioni si calcolano una volta per validazione e non una per
    // problema: è una passata sola sul testo, e sono venti token sbagliati che
    // chiedono la stessa mappa.
    let dove = aether_skin::posizioni(&sorgente);

    let documento = match aether_skin::leggi_skin(&sorgente) {
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

    let compilata = aether_skin::compile_skin(&documento);
    ValidazioneIpc {
        errori: Vec::new(),
        avvisi: aether_skin::check_skin(&documento)
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
        contrasti: aether_skin::contrast_pairs(&documento)
            .into_iter()
            .map(|c| ContrastoIpc {
                passa: c.passa(),
                davanti: c.foreground,
                dietro: c.background,
                scuro: c.dark,
                chiaro: c.light,
            })
            .collect(),
        tavolozza: aether_skin::palette_usage(&documento),
        css: compilata.css,
        layout: Some(crate::skin::impaginazione(&documento)),
        costo: compilata.cost,
        costo_scafale: compilata.shell_cost,
        parti: documento.parts.len(),
        dinamici: compilata
            .dynamic_tokens
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        compilato_ms: inizio.elapsed().as_millis(),
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
            aether_skin::parts::part(nome).is_some()
                || aether_skin::tokens::token(nome).is_some()
                || aether_skin::effects::Effect::NAMES.contains(&nome.as_str())
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
/// [`aether_skin::SkinIssue`], che li ha sempre avuti.
fn problema_da(guasto: &aether_skin::SkinIssue, dove: &aether_skin::Posizioni) -> ProblemaIpc {
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

/// Dove stanno le istantanee di una bozza.
fn cartella_istantanee(data_dir: &Path, id: &str) -> PathBuf {
    cartella_bozze(data_dir).join(id).join("istantanee")
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
/// `skin.json` e basta — i caratteri e le immagini restano dentro l'`.aeskin`
/// installato mentre si accorda il documento. Tenere le due cose in una
/// funzione sola è ciò che fa combaciare l'albero della vista Documento con quel
/// che l'esportazione scrive davvero: due letture separate divergerebbero il
/// giorno in cui una delle due impara qualcosa che l'altra no.
struct DaLavorare {
    sorgente: String,
    preview: Option<Vec<u8>>,
    assets: Vec<aether_skin::package::SkinAsset>,
}

/// Cerca prima fra le bozze, poi in quella di serie, poi fra le installate.
///
/// L'ordine è quello: chi ha lasciato una bozza a metà la ritrova, e non riparte
/// dall'originale che stava modificando.
fn da_lavorare(data_dir: &Path, id: &str) -> Result<DaLavorare, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }

    // Un file che non si apre vuol dire «non installata»; un file che si apre e
    // non si legge è un guasto, e va detto invece che scambiato per assenza.
    let installata = data_dir.join("skin").join(format!("{id}.aeskin"));
    let pacchetto = match std::fs::read(&installata) {
        Ok(bytes) => Some(aether_skin::read_skin_package(&bytes)?),
        Err(_) => None,
    };

    let bozza = cartella_bozze(data_dir)
        .join(id)
        .join(aether_skin::package::MANIFEST_NAME);
    let sorgente = if let Ok(testo) = std::fs::read_to_string(&bozza) {
        testo
    } else if id == DI_SERIE {
        aether_skin::PLAIN_SOURCE.to_owned()
    } else if let Some(letto) = &pacchetto {
        letto.source.clone()
    } else {
        return Err(non_trovata(id));
    };

    Ok(DaLavorare {
        sorgente,
        preview: pacchetto.as_ref().and_then(|letto| letto.preview.clone()),
        assets: pacchetto.map_or_else(Vec::new, |letto| letto.assets),
    })
}

/// Il testo di una skin da aprire nello Studio.
#[tauri::command(async)]
pub fn studio_documento(stato: State<'_, Stato>, id: String) -> Esito<String> {
    con_libreria(&stato, |libreria| {
        da_lavorare(&libreria.data_dir, &id).map(|da| da.sorgente)
    })
    .map_err(errore)
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
    con_libreria(&stato, |libreria| {
        let da = da_lavorare(&libreria.data_dir, &id)?;
        let quanto = |byte: usize| u64::try_from(byte).unwrap_or(u64::MAX);
        let mut voci = vec![VoceFileIpc {
            nome: aether_skin::package::MANIFEST_NAME.to_owned(),
            byte: quanto(da.sorgente.len()),
            genere: "manifest",
        }];
        if let Some(miniatura) = &da.preview {
            voci.push(VoceFileIpc {
                nome: aether_skin::package::PREVIEW_NAME.to_owned(),
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

/// Salva la bozza.
///
/// Si salva **anche se non è valida**: una bozza è un lavoro in corso, e
/// rifiutare di scrivere un documento a metà vorrebbe dire perderlo chiudendo la
/// finestra. È la validazione a dire cosa non va, non il salvataggio.
#[tauri::command(async)]
pub fn studio_salva(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
) -> Esito<()> {
    let esito = con_libreria(&stato, |libreria| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        let cartella = cartella_bozze(&libreria.data_dir).join(&id);
        std::fs::create_dir_all(&cartella)
            .map_err(|err| aether_app::files::io_error(&cartella.display().to_string(), &err))?;
        let file = cartella.join("skin.json");
        std::fs::write(&file, sorgente.as_bytes())
            .map_err(|err| aether_app::files::io_error(&file.display().to_string(), &err))
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
/// questo bottone — che è irreversibile per natura. Chi scarta una bozza e si
/// pente ha ancora l'ultima istantanea da cui ripartire; se questo comando
/// portasse via anche quelle, non avrebbe più niente.
///
/// Restituisce la sorgente del pacchetto, cioè quel che l'editor deve mostrare
/// da qui in poi: farsela ridire con una seconda chiamata lascerebbe un istante
/// in cui la finestra mostra un documento che non esiste più da nessuna parte.
#[tauri::command(async)]
pub fn studio_scarta(stato: State<'_, Stato>, id: String) -> Esito<String> {
    con_libreria(&stato, |libreria| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        let cartella = cartella_bozze(&libreria.data_dir).join(&id);
        if cartella.exists() {
            std::fs::remove_dir_all(&cartella).map_err(|err| {
                aether_app::files::io_error(&cartella.display().to_string(), &err)
            })?;
        }
        // Dopo la cancellazione `da_lavorare` non trova più la bozza e ricade
        // sul pacchetto: è la stessa funzione che decide all'apertura, quindi
        // non c'è un secondo posto in cui la regola «la bozza vince» possa
        // divergere.
        Ok(da_lavorare(&libreria.data_dir, &id)?.sorgente)
    })
    .map_err(errore)
}

/// Scrive un `.aeskin` da una sorgente, con dentro le risorse che aveva.
///
/// Gli **errori** bloccano, gli avvisi no: è la regola scritta nel disegno, e
/// qui è per costruzione — `parse_skin_json` fallisce sugli errori e non sa
/// niente degli avvisi.
///
/// Il manifest è quello dell'editor, le risorse quelle del pacchetto: chi aveva
/// una skin con un carattere dentro lo perdeva esportandola da qui, e l'albero
/// della vista Documento avrebbe mostrato file che l'esportazione buttava via.
#[tauri::command(async)]
pub fn studio_esporta(
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    percorso: String,
) -> Esito<()> {
    con_libreria(&stato, |libreria| {
        let da = da_lavorare(&libreria.data_dir, &id)?;
        // `write_skin_package` valida da sé prima di scrivere — passa dalle
        // stesse guardie della lettura — quindi qui non serve un `parse` in più:
        // sarebbe una seconda validazione che può divergere dalla prima.
        let bytes = aether_skin::write_skin_package(&aether_skin::package::WritePackageInput {
            source: &sorgente,
            preview: da.preview.as_deref(),
            assets: &da.assets,
        })?;
        std::fs::write(&percorso, &bytes)
            .map_err(|err| aether_app::files::io_error(&percorso, &err))
    })
    .map_err(errore)
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
}

impl Causa {
    const fn nome(self) -> &'static str {
        match self {
            Self::Derivata => "derivata",
            Self::Salvata => "salvata",
            Self::Esportata => "esportata",
            Self::Manuale => "manuale",
        }
    }

    fn da(nome: &str) -> Option<Self> {
        [
            Self::Derivata,
            Self::Salvata,
            Self::Esportata,
            Self::Manuale,
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
    con_libreria(&stato, |libreria| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        Ok(elenco_istantanee(&cartella_istantanee(
            &libreria.data_dir,
            &id,
        )))
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
            .map_err(|err| aether_app::files::io_error(&cartella.display().to_string(), &err))?;

        // I conteggi si prendono adesso e non a ogni lettura dell'elenco: sono
        // ciò che il documento **era**, e riparsare venti manifest per riempire
        // una colonna sarebbe lavoro rifatto a ogni apertura. Un documento a
        // metà di una parentesi si conta zero e zero — è una bozza salvata
        // apposta anche quando non è valida, e l'elenco lo dice invece di
        // rifiutarsi di prenderla.
        let (parti, token) = aether_skin::parse_skin_json(sorgente).map_or((0, 0), |documento| {
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

        let file = cartella.join(nome_istantanea(&presa));
        std::fs::write(&file, sorgente.as_bytes())
            .map_err(|err| aether_app::files::io_error(&file.display().to_string(), &err))?;

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
    con_libreria(&stato, |libreria| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        prendi_istantanea(
            &cartella_istantanee(&libreria.data_dir, &id),
            &sorgente,
            causa,
        )
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
        .map_err(|err| aether_app::files::io_error(&file.display().to_string(), &err))
}

/// Riporta il documento a com'era in un'istantanea.
#[tauri::command(async)]
pub fn studio_ripristina(stato: State<'_, Stato>, id: String, quando: u64) -> Esito<String> {
    con_libreria(&stato, |libreria| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        ripristina_istantanea(&cartella_istantanee(&libreria.data_dir, &id), quando)
    })
    .map_err(errore)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_registro_dice_tutto_quel_che_serve_a_un_editor() {
        let registro = studio_registro();
        assert_eq!(registro.tokens.len(), aether_skin::TOKENS.len());
        assert_eq!(registro.parts.len(), aether_skin::parts::PARTS.len());
        // Gli undici effetti, ognuno col suo costo vero. Se un effetto nuovo
        // nascesse e `esempio()` non lo conoscesse, questo test lo direbbe
        // invece di lasciarlo sparire in silenzio dall'elenco.
        assert_eq!(registro.effects.len(), aether_skin::Effect::NAMES.len());
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
            let riletto = aether_skin::parse_skin_json(&documento)
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
        let esito = studio_valida("{ \"format\": 1, ".to_owned());
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
        let esito = studio_valida(json.to_owned());
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
        let json = aether_skin::PLAIN_SOURCE.replace(
            "\"tokens\": {",
            "\"tokens\": {
    \"color.accents\": \"#ff0000\",
    \"radius.cards\": \"4px\",
    \"font.mainly\": [\"X\"],",
        );
        let esito = studio_valida(json);
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
        let json = aether_skin::PLAIN_SOURCE.replace(
            "\"capabilities\": {",
            "\"patterns\": { \"mai-usato\": { \"effect\": \"vignette\", \"color\": \"#000\" } }, \"capabilities\": {",
        );
        let esito = studio_valida(json);
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
        let json = aether_skin::PLAIN_SOURCE.replace(
            "\"layout\": {",
            "\"parts\": { \"section-cards\": { \"radius\": \"10px\" } }, \"layout\": {",
        );
        let esito = studio_valida(json);
        let primo = esito.errori.first().expect("un errore");
        assert!(
            primo.forse.contains(&"section-card".to_owned()),
            "il suggerimento di nearest_parts deve arrivare come dato, non solo \
             dentro la frase: {primo:?}"
        );
    }

    #[test]
    fn la_skin_di_serie_passa_dallo_studio_senza_errori() {
        let esito = studio_valida(aether_skin::PLAIN_SOURCE.to_owned());
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
        prendi_istantanea(dir.path(), aether_skin::PLAIN_SOURCE, Causa::Salvata).expect("presa");

        let elenco = elenco_istantanee(dir.path());
        assert_eq!(elenco.len(), 1);
        let sola = elenco.first().expect("una");
        assert_eq!(sola.causa, "salvata");
        // I conteggi sono quelli del documento, presi al momento della scrittura.
        assert_eq!(sola.token, {
            let documento = aether_skin::parse_skin_json(aether_skin::PLAIN_SOURCE).expect("plain");
            documento.tokens.iter().count()
        });

        assert_eq!(
            ripristina_istantanea(dir.path(), sola.quando).expect("ripristinata"),
            aether_skin::PLAIN_SOURCE,
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
            prendi_istantanea(dir.path(), aether_skin::PLAIN_SOURCE, Causa::Manuale)
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
        prendi_istantanea(dir.path(), aether_skin::PLAIN_SOURCE, Causa::Salvata).expect("una");
        prendi_istantanea(dir.path(), aether_skin::PLAIN_SOURCE, Causa::Salvata).expect("due");
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
        assert_eq!(da.sorgente, aether_skin::PLAIN_SOURCE);
        assert!(da.preview.is_none());
        assert!(da.assets.is_empty());
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
            aether_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
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
            aether_skin::effects::Effect::NAMES.len()
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
            aether_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
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
            aether_skin::PLAIN_SOURCE
        );
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
}
