//! Dal formato 1 al formato 2, e la forma canonica del testo.
//!
//! # La promessa
//!
//! Una skin del formato 1 si migra in una del formato 2 che **si disegna come
//! prima**. Non «si apre»: si disegna. La prova sta in `tests/migrazione.rs`,
//! che congela il foglio che il compilatore del formato 1 produceva per Plain,
//! per Sala e per un documento che usa ogni costrutto del formato vecchio, e lo
//! confronta con quello che il formato 2 produce dalla loro migrazione — regola
//! per regola, dopo aver applicato le sole rinomine elencate qui sotto.
//!
//! # Cosa cambia
//!
//! | formato 1 | formato 2 |
//! |---|---|
//! | `capabilities.light` + `themes.light` | `scheme: "dark"` + `variant.tokens` |
//! | `capabilities.mobile` | sparisce: lo dice `platforms.mobile` |
//! | `layout.player`, `layout.sidebar` | `layout.shell`, scritto per esteso |
//! | `motion.routeTransition` | `motion.route` |
//! | `parts.<p>.filter` (era `backdrop-filter`) | `parts.<p>.backdrop` |
//! | `parts.<p>.layer` | `parts.<p>.after` |
//! | `parts.<p>.states.active` | `parts.<p>.states.current` |
//! | `parts.<p>.animations` (con `active`) | `parts.<p>.motion` (con `current`) |
//!
//! # Le copie
//!
//! Il formato 1 prestava una classe a superfici diverse: `list-row` stava sulle
//! righe e sulle schede, `glass-modal` sulle finestrelle e sulla coda,
//! `menu-pop` sui menù e sull'equalizzatore, `hero-art` sull'hero e sulla
//! copertina della colonna. Il formato 2 dà a ognuna un nome suo, e la
//! migrazione **copia** lo stile del nome vecchio sul nome nuovo: la scheda di
//! un album continua a vestirsi come la riga di un elenco finché qualcuno non
//! decide altrimenti. Una copia non sovrascrive mai una parte già scritta.
//!
//! Un livello su una copertina (`np-art`, `hero-art`) si toglie: la copertina è
//! un `<img>`, e un'immagine non ha pseudo-elementi — quel livello non si è mai
//! visto, e il formato 2 lo rifiuterebbe.

use serde_json::{Map, Value};

use crate::document::{
    CHIAVI_ANELLO, CHIAVI_ANGOLI, CHIAVI_ANIMAZIONE, CHIAVI_ASPETTO, CHIAVI_CAPACITA,
    CHIAVI_DOCUMENTO, CHIAVI_FERMATA, CHIAVI_FONT, CHIAVI_FOTOGRAMMA, CHIAVI_FOTOGRAMMA_ROTTA,
    CHIAVI_IMPAGINAZIONE, CHIAVI_LIVELLO, CHIAVI_LIVELLO_OMBRA, CHIAVI_META, CHIAVI_MOVIMENTO,
    CHIAVI_MOVIMENTO_PARTE, CHIAVI_RICHIAMO, CHIAVI_ROTTA, CHIAVI_STRUTTURA_PARTE,
    CHIAVI_TRANSIZIONE, CHIAVI_VARIANTE, CHIAVI_WIDGET, CHIAVI_ZONA, Esito, SKIN_FORMAT_VERSION,
    SkinIssue, chiavi_effetto, migra, non_e_json, problema, versione_formato,
};
use crate::layout::{
    LayoutNode, LayoutZone, OptionValue, PlayerLayout, SidebarLayout, TrackSize, WidgetInstance,
    default_shell,
};
use crate::movimento::AnimTrigger;
use crate::stati::PartState;
use crate::values::format_length;

/// Le superfici che il formato 1 vestiva con un nome prestato, e il nome
/// nuovo su cui la migrazione ne copia lo stile.
#[rustfmt::skip]
pub const COPIE: &[(&str, &str)] = &[
    ("list-row", "card"),
    ("list-row", "track-card"),
    ("list-row", "collection-card"),
    ("list-row", "home-resume"),
    ("list-row", "artist-card"),
    ("glass-modal", "queue-panel"),
    ("menu-pop", "eq-panel"),
    ("hero-art", "column-art"),
];

fn rinomina(map: &mut Map<String, Value>, da: &str, a: &str) {
    if let Some(valore) = map.remove(da) {
        map.entry(a.to_owned()).or_insert(valore);
    }
}

/// Il passo 1 → 2. Vedi il `//!` del modulo.
///
/// # Errori
///
/// Solo quando il formato 1 era già invalido in un punto che la migrazione
/// deve leggere per scrivere — un `layout.player` che non è una delle tre
/// parole. Tutto il resto lo dice la validazione del formato 2, sul risultato.
pub(crate) fn da_1_a_2(map: &mut Map<String, Value>) -> Esito<()> {
    // Le capacità: `light` diventa la presenza della variante, `mobile` non
    // lo leggeva nessuno.
    //
    // `light` si legge **prima** di toglierla. Nel formato 1 il tema chiaro
    // esisteva solo se la skin lo dichiarava con `capabilities.light`: un
    // blocco `themes.light` senza quella dichiarazione era un abbozzo che
    // nessuna finestra mostrava. Fino alla 3.0 la migrazione lo trasformava
    // comunque in una variante, e una skin scura e basta si ritrovava un tema
    // chiaro mai finito, che la scelta «Chiaro» delle Impostazioni indossava.
    let con_il_chiaro = map
        .get("capabilities")
        .and_then(|c| c.get("light"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(Value::Object(capacita)) = map.get_mut("capabilities") {
        capacita.remove("light");
        capacita.remove("mobile");
        if capacita.is_empty() {
            map.remove("capabilities");
        }
    }

    // I temi: il formato 1 era sempre scuro di base.
    match map.remove("themes") {
        Some(Value::Object(mut temi)) => {
            if let Some(chiaro) = temi.remove("light").filter(|_| con_il_chiaro) {
                let mut variante = Map::new();
                variante.insert("tokens".to_owned(), chiaro);
                map.insert("variant".to_owned(), Value::Object(variante));
            }
        }
        // Un `themes` che non è un oggetto era già un errore nel formato 1:
        // lo si lascia dov'è, e la validazione del 2 lo dirà col suo nome.
        Some(altro) => {
            map.insert("themes".to_owned(), altro);
        }
        None => {}
    }
    map.entry("scheme".to_owned())
        .or_insert_with(|| Value::from("dark"));

    if let Some(Value::Object(movimento)) = map.get_mut("motion") {
        rinomina(movimento, "routeTransition", "route");
    }

    if let Some(Value::Object(impaginazione)) = map.get_mut("layout") {
        migra_impaginazione(impaginazione)?;
    }

    if let Some(Value::Object(parti)) = map.get_mut("parts") {
        for (nome, parte) in parti.iter_mut() {
            if let Value::Object(parte) = parte {
                migra_parte(nome, parte);
            }
        }
        for (da, a) in COPIE {
            if parti.contains_key(*a) {
                continue;
            }
            let Some(Value::Object(originale)) = parti.get(*da) else {
                continue;
            };
            let mut copia = originale.clone();
            // Il nome nuovo può non avere i livelli del vecchio: una copia che
            // si portasse dietro un livello che non può esistere renderebbe la
            // skin migrata invalida per una riga che nessuno ha scritto.
            if let Some(def) = crate::parts::part(a) {
                if !def.after {
                    copia.remove("after");
                }
                if !def.before {
                    copia.remove("before");
                }
            }
            parti.insert((*a).to_owned(), Value::Object(copia));
        }
    }

    Ok(())
}

fn migra_parte(nome: &str, parte: &mut Map<String, Value>) {
    rinomina(parte, "filter", "backdrop");
    if let Some(livello) = parte.remove("layer") {
        // Una parte ritirata resta com'era: si accetta e non fa niente, e il
        // formato 2 non guarda dentro.
        let conserva = crate::parts::part(nome).is_none_or(|def| def.after);
        if conserva {
            parte.insert("after".to_owned(), livello);
        }
    }
    if let Some(Value::Object(stati)) = parte.get_mut("states") {
        rinomina(stati, "active", "current");
        for stato in stati.values_mut() {
            if let Value::Object(aspetto) = stato {
                rinomina(aspetto, "filter", "backdrop");
            }
        }
    }
    if let Some(animazioni) = parte.remove("animations") {
        let migrate = match animazioni {
            Value::Object(mut inneschi) => {
                rinomina(&mut inneschi, "active", "current");
                Value::Object(inneschi)
            }
            altro => altro,
        };
        parte.insert("motion".to_owned(), migrate);
    }
}

fn migra_impaginazione(impaginazione: &mut Map<String, Value>) -> Esito<()> {
    let player = impaginazione.remove("player");
    let sidebar = impaginazione.remove("sidebar");
    if impaginazione.contains_key("shell") || (player.is_none() && sidebar.is_none()) {
        return Ok(());
    }
    let player = match player {
        None => PlayerLayout::Floating,
        Some(valore) => valore
            .as_str()
            .and_then(PlayerLayout::parse)
            .ok_or_else(|| {
                problema(
                    "layout.player",
                    "ammessi soltanto: bottom-bar, floating, compact",
                )
            })?,
    };
    let sidebar = match sidebar {
        None => SidebarLayout::Rail,
        Some(valore) => valore
            .as_str()
            .and_then(SidebarLayout::parse)
            .ok_or_else(|| {
                problema("layout.sidebar", "ammessi soltanto: rail, expanded, hidden")
            })?,
    };
    impaginazione.insert(
        "shell".to_owned(),
        zona_json(&default_shell(player, sidebar)),
    );
    Ok(())
}

fn misura_json(size: TrackSize) -> Value {
    match size {
        TrackSize::Hug => Value::from("hug"),
        TrackSize::Fill => Value::from("fill"),
        TrackSize::Fixed(lunghezza) => Value::from(format_length(lunghezza)),
    }
}

/// Uno scafale già validato, nella forma in cui lo si scrive.
///
/// Scrive solo quel che si allontana dai difetti: una zona che dispone in
/// riga senza aria e allineata di serie è `{"zone": "row", "children": […]}`,
/// come la scriverebbe una persona.
#[must_use]
pub fn zona_json(zona: &LayoutZone) -> Value {
    use crate::layout::{Align, GapStep, Spread};
    let mut map = Map::new();
    map.insert("zone".to_owned(), Value::from(zona.kind.as_str()));
    if zona.size != TrackSize::Fill {
        map.insert("size".to_owned(), misura_json(zona.size));
    }
    if zona.gap != GapStep::None {
        map.insert("gap".to_owned(), Value::from(zona.gap.as_str()));
    }
    if zona.align != Align::Stretch {
        map.insert("align".to_owned(), Value::from(zona.align.as_str()));
    }
    if zona.spread != Spread::Start {
        map.insert("spread".to_owned(), Value::from(zona.spread.as_str()));
    }
    if let Some(parte) = zona.part {
        map.insert("part".to_owned(), Value::from(parte.name));
    }
    let figli: Vec<Value> = zona
        .children
        .iter()
        .map(|figlio| match figlio {
            LayoutNode::Zone(sotto) => zona_json(sotto),
            LayoutNode::Widget(istanza) => widget_json(istanza),
        })
        .collect();
    map.insert("children".to_owned(), Value::Array(figli));
    Value::Object(map)
}

fn widget_json(istanza: &WidgetInstance) -> Value {
    let mut map = Map::new();
    map.insert("widget".to_owned(), Value::from(istanza.def.name));
    if istanza.size != istanza.def.natural {
        map.insert("size".to_owned(), misura_json(istanza.size));
    }
    let difetti = WidgetInstance::nuovo(istanza.def);
    let mut opzioni = Map::new();
    for (opzione, valore) in &istanza.options {
        if difetti.option(opzione.name) == Some(*valore) {
            continue;
        }
        let scritto = match *valore {
            OptionValue::Flag(acceso) => Value::from(acceso),
            OptionValue::Word(parola) => Value::from(parola),
            OptionValue::Count(quanti) => Value::from(quanti),
        };
        opzioni.insert(opzione.name.to_owned(), scritto);
    }
    if !opzioni.is_empty() {
        map.insert("options".to_owned(), Value::Object(opzioni));
    }
    Value::Object(map)
}

// ── La forma canonica ───────────────────────────────────────────────────────

/// Dove si trova un oggetto nel documento: decide l'ordine delle sue chiavi e
/// quello dei figli.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dove {
    Radice,
    Meta,
    Capacita,
    Caratteri,
    Carattere,
    Tavolozza,
    Motivi,
    Effetto,
    Fermata,
    Tokens,
    Valore,
    LivelloOmbra,
    Variante,
    Piattaforme,
    Movimento,
    Curve,
    Rotta,
    FotogrammaRotta,
    Animazioni,
    Animazione,
    Fotogramma,
    Impaginazione,
    Prefabs,
    Nodo,
    Parti,
    Parte,
    Stati,
    Aspetto,
    Livello,
    MovimentoParte,
    Transizione,
    Anello,
    Angoli,
    Generico,
}

/// L'ordine delle chiavi di un valore di token o di colore.
const CHIAVI_VALORE: &[&str] = &[
    "$token",
    "$palette",
    "$source",
    "$pattern",
    "alpha",
    "kind",
    "keyword",
    "points",
    "count",
    "position",
    "min",
    "preferred",
    "max",
    "layers",
];

/// Le chiavi di un oggetto nell'ordine canonico: prima quelle conosciute, in
/// ordine di schema, poi le altre in ordine alfabetico.
fn ordinate(map: &Map<String, Value>, dove: Dove) -> Vec<&String> {
    let mut chiavi: Vec<&String> = map.keys().collect();
    let posto = |chiave: &str| -> usize {
        match dove {
            Dove::Tokens => crate::tokens::TOKENS
                .iter()
                .position(|d| d.id == chiave)
                .unwrap_or(usize::MAX),
            Dove::Parti => crate::parts::posto(chiave),
            Dove::Stati => PartState::parse(chiave).map_or(usize::MAX, PartState::posto),
            Dove::MovimentoParte => AnimTrigger::ALL
                .iter()
                .position(|t| t.as_str() == chiave)
                .or_else(|| {
                    CHIAVI_MOVIMENTO_PARTE
                        .iter()
                        .position(|c| *c == chiave)
                        .map(|p| p + AnimTrigger::ALL.len())
                })
                .unwrap_or(usize::MAX),
            _ => {
                let elenco: &[&str] = match dove {
                    Dove::Radice => CHIAVI_DOCUMENTO,
                    Dove::Meta => CHIAVI_META,
                    Dove::Capacita => CHIAVI_CAPACITA,
                    Dove::Carattere => CHIAVI_FONT,
                    Dove::Effetto => map
                        .get("effect")
                        .and_then(Value::as_str)
                        .map_or(&["effect"][..], chiavi_effetto),
                    Dove::Fermata => CHIAVI_FERMATA,
                    Dove::Valore => CHIAVI_VALORE,
                    Dove::LivelloOmbra => CHIAVI_LIVELLO_OMBRA,
                    Dove::Variante => CHIAVI_VARIANTE,
                    Dove::Piattaforme => &["mobile"],
                    Dove::Movimento => CHIAVI_MOVIMENTO,
                    Dove::Rotta => CHIAVI_ROTTA,
                    Dove::FotogrammaRotta => CHIAVI_FOTOGRAMMA_ROTTA,
                    Dove::Animazione => CHIAVI_ANIMAZIONE,
                    Dove::Fotogramma => CHIAVI_FOTOGRAMMA,
                    Dove::Impaginazione => CHIAVI_IMPAGINAZIONE,
                    Dove::Nodo => {
                        if map.contains_key("widget") {
                            CHIAVI_WIDGET
                        } else if map.contains_key("prefab") {
                            CHIAVI_RICHIAMO
                        } else {
                            CHIAVI_ZONA
                        }
                    }
                    Dove::Parte | Dove::Aspetto => {
                        return CHIAVI_ASPETTO
                            .iter()
                            .chain(CHIAVI_STRUTTURA_PARTE.iter())
                            .position(|c| *c == chiave)
                            .unwrap_or(usize::MAX);
                    }
                    Dove::Livello => CHIAVI_LIVELLO,
                    Dove::Transizione => CHIAVI_TRANSIZIONE,
                    Dove::Anello => CHIAVI_ANELLO,
                    Dove::Angoli => CHIAVI_ANGOLI,
                    _ => &[],
                };
                elenco
                    .iter()
                    .position(|c| *c == chiave)
                    .unwrap_or(usize::MAX)
            }
        }
    };
    // Stabile, e a parità di posto l'ordine della mappa, che è alfabetico.
    chiavi.sort_by_key(|chiave| posto(chiave));
    chiavi
}

/// Dove sta il valore di una chiave, sapendo dove sta l'oggetto.
fn figlio(dove: Dove, chiave: &str) -> Dove {
    match (dove, chiave) {
        (Dove::Radice, "meta") => Dove::Meta,
        (Dove::Radice, "capabilities") => Dove::Capacita,
        (Dove::Radice, "fonts") => Dove::Caratteri,
        (Dove::Radice | Dove::Variante, "palette") => Dove::Tavolozza,
        (Dove::Radice | Dove::Variante, "patterns") => Dove::Motivi,
        (Dove::Radice | Dove::Variante | Dove::Piattaforme, "tokens" | "mobile") => Dove::Tokens,
        (Dove::Radice, "variant") => Dove::Variante,
        (Dove::Radice, "platforms") => Dove::Piattaforme,
        (Dove::Radice, "motion") => Dove::Movimento,
        (Dove::Radice, "layout") => Dove::Impaginazione,
        (Dove::Radice | Dove::Variante, "parts") => Dove::Parti,
        (Dove::Caratteri, _) => Dove::Carattere,
        (Dove::Motivi, _) => Dove::Effetto,
        (Dove::Effetto, "stops") => Dove::Fermata,
        (Dove::Tokens | Dove::Curve, _) => Dove::Valore,
        (Dove::Valore, "layers") => Dove::LivelloOmbra,
        (Dove::Movimento, "easings") => Dove::Curve,
        (Dove::Movimento, "route") => Dove::Rotta,
        (Dove::Movimento, "animations") => Dove::Animazioni,
        (Dove::Rotta, "out" | "in") => Dove::FotogrammaRotta,
        (Dove::Animazioni, _) => Dove::Animazione,
        (Dove::Animazione, "frames") => Dove::Fotogramma,
        (Dove::Impaginazione, "prefabs") => Dove::Prefabs,
        (Dove::Impaginazione, "shell") | (Dove::Prefabs, _) | (Dove::Nodo, "children") => {
            Dove::Nodo
        }
        (Dove::Parti, _) => Dove::Parte,
        (Dove::Parte, "states") => Dove::Stati,
        (Dove::Stati, _) => Dove::Aspetto,
        (Dove::Parte, "before" | "after") => Dove::Livello,
        (Dove::Parte, "motion") => Dove::MovimentoParte,
        (Dove::Parte | Dove::Aspetto | Dove::Livello, "background" | "clip" | "backdrop") => {
            Dove::Effetto
        }
        (Dove::Parte | Dove::Aspetto, "focusRing") => Dove::Anello,
        (Dove::Parte | Dove::Aspetto, "radius") => Dove::Angoli,
        (Dove::MovimentoParte, "transition") => Dove::Transizione,
        (
            Dove::Parte | Dove::Aspetto | Dove::Fotogramma | Dove::Fermata | Dove::Effetto,
            "shadow" | "textColor" | "borderColor" | "color" | "backgroundColor" | "background",
        )
        | (Dove::Animazione | Dove::Transizione | Dove::Rotta, "easing")
        | (Dove::Anello, "color")
        | (Dove::LivelloOmbra, "color") => Dove::Valore,
        _ => Dove::Generico,
    }
}

/// Un numero come lo scrive `JSON.stringify`: `180` e non `180.0`.
fn numero(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(valore) if n.is_f64() && valore.fract() == 0.0 && valore.abs() < 1e15 => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "il valore è intero e sta sotto 1e15, che un i64 contiene per intero"
            )]
            let intero = valore as i64;
            intero.to_string()
        }
        _ => n.to_string(),
    }
}

fn scrivi(valore: &Value, dove: Dove, livello: usize, fuori: &mut String) {
    let rientro = "  ".repeat(livello);
    let dentro = "  ".repeat(livello + 1);
    match valore {
        Value::Null => fuori.push_str("null"),
        Value::Bool(b) => fuori.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => fuori.push_str(&numero(n)),
        Value::String(s) => {
            fuori.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned()));
        }
        Value::Array(voci) => {
            if voci.is_empty() {
                fuori.push_str("[]");
                return;
            }
            fuori.push_str("[\n");
            for (indice, voce) in voci.iter().enumerate() {
                if indice > 0 {
                    fuori.push_str(",\n");
                }
                fuori.push_str(&dentro);
                scrivi(voce, dove, livello + 1, fuori);
            }
            fuori.push('\n');
            fuori.push_str(&rientro);
            fuori.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                fuori.push_str("{}");
                return;
            }
            fuori.push_str("{\n");
            for (indice, chiave) in ordinate(map, dove).into_iter().enumerate() {
                if indice > 0 {
                    fuori.push_str(",\n");
                }
                fuori.push_str(&dentro);
                fuori
                    .push_str(&serde_json::to_string(chiave).unwrap_or_else(|_| "\"\"".to_owned()));
                fuori.push_str(": ");
                if let Some(sotto) = map.get(chiave) {
                    scrivi(sotto, figlio(dove, chiave), livello + 1, fuori);
                }
            }
            fuori.push('\n');
            fuori.push_str(&rientro);
            fuori.push('}');
        }
    }
}

/// Il documento scritto nella forma canonica: due spazi, e le chiavi
/// nell'ordine dello schema.
///
/// # Perché un ordine di schema e non quello di scrittura
///
/// Perché il testo migrato non ha un ordine di scrittura: la migrazione passa
/// per un `serde_json::Map`, che qui è ordinata alfabeticamente, e un manifest
/// con `author` prima di `name` e `color.accent` prima di `color.surface.0` è
/// un manifest che nessuno ha scritto così. L'ordine dello schema è quello in
/// cui i pannelli dello Studio mostrano le cose, e quello del registro per
/// token e parti: chi apre il file dopo la migrazione lo ritrova leggibile.
///
/// La forma è quella di `JSON.stringify(documento, null, 2)`, che è quella
/// dello Studio: un documento migrato e poi toccato da un cursore non cambia
/// di formato a ogni gesto.
#[must_use]
pub fn forma_canonica(documento: &Value) -> String {
    let mut fuori = String::new();
    scrivi(documento, Dove::Radice, 0, &mut fuori);
    fuori.push('\n');
    fuori
}

/// Com'è andata una lettura per lo Studio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migrato {
    /// Il testo da aprire.
    pub sorgente: String,
    /// Di quale formato era il testo di partenza.
    pub da: u64,
}

/// Porta il testo di una skin al formato di oggi, per aprirlo nello Studio.
///
/// Un documento già del formato di oggi torna **com'era**, carattere per
/// carattere: la forma canonica serve al testo che la migrazione riscrive, non
/// a quello che qualcuno ha formattato a modo suo. Uno più vecchio torna
/// migrato e scritto in forma canonica.
///
/// # Errori
///
/// Un problema solo — la sintassi, con la posizione — se il testo non è JSON;
/// quelli della migrazione se il formato vecchio non si sa leggere. Un
/// documento migrato ma non valido **non** è un errore qui: lo Studio lo apre e
/// la validazione dice cosa non va, come per qualunque bozza.
pub fn migra_testo(sorgente: &str) -> Result<Migrato, Vec<SkinIssue>> {
    let raw: Value = serde_json::from_str(sorgente).map_err(|err| vec![non_e_json(&err)])?;
    let Some(da) = versione_formato(&raw) else {
        return Ok(Migrato {
            sorgente: sorgente.to_owned(),
            da: u64::from(SKIN_FORMAT_VERSION),
        });
    };
    if da >= u64::from(SKIN_FORMAT_VERSION) {
        return Ok(Migrato {
            sorgente: sorgente.to_owned(),
            da,
        });
    }
    let migrato = migra(&raw, da)?;
    Ok(Migrato {
        sorgente: forma_canonica(&migrato),
        da,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn migrato(v1: Value) -> Value {
        let Value::Object(mut map) = v1 else {
            panic!("non è un oggetto");
        };
        da_1_a_2(&mut map).expect("migra");
        Value::Object(map)
    }

    #[test]
    fn il_tema_chiaro_diventa_la_variante() {
        let fuori = migrato(json!({
            "format": 1,
            "capabilities": { "light": true, "mobile": false, "dynamicAccent": false },
            "themes": { "light": { "color.accent": "#6752d6" } }
        }));
        assert_eq!(fuori["scheme"], "dark");
        assert_eq!(fuori["variant"]["tokens"]["color.accent"], "#6752d6");
        assert!(fuori.get("themes").is_none());
        assert_eq!(fuori["capabilities"], json!({ "dynamicAccent": false }));
    }

    #[test]
    fn un_tema_chiaro_non_dichiarato_non_diventa_una_variante() {
        // Nel formato 1 il chiaro esisteva solo con `capabilities.light`: un
        // blocco senza la dichiarazione non si vedeva, e migrato non deve
        // cominciare a vedersi.
        for capacita in [
            json!({ "light": false, "dynamicAccent": true }),
            json!({ "dynamicAccent": true }),
        ] {
            let fuori = migrato(json!({
                "format": 1,
                "capabilities": capacita,
                "themes": { "light": { "color.accent": "#6752d6" } }
            }));
            assert!(fuori.get("variant").is_none(), "{fuori}");
            assert!(fuori.get("themes").is_none(), "{fuori}");
            assert_eq!(fuori["scheme"], "dark");
            assert_eq!(fuori["capabilities"], json!({ "dynamicAccent": true }));
        }
        // E senza `capabilities` del tutto, idem.
        let fuori = migrato(json!({
            "format": 1,
            "themes": { "light": { "color.accent": "#6752d6" } }
        }));
        assert!(fuori.get("variant").is_none(), "{fuori}");
    }

    #[test]
    fn le_capacita_vuote_spariscono() {
        let fuori = migrato(json!({ "capabilities": { "light": true } }));
        assert!(fuori.get("capabilities").is_none());
    }

    #[test]
    fn le_parti_rinominano_le_chiavi_del_formato_1() {
        let fuori = migrato(json!({
            "parts": {
                "section-card": {
                    "filter": { "$pattern": "vetro" },
                    "layer": { "background": [] },
                    "states": { "active": { "filter": { "$pattern": "vetro" } } },
                    "animations": { "enter": "sale", "active": "pulsa" }
                }
            }
        }));
        let parte = &fuori["parts"]["section-card"];
        assert_eq!(parte["backdrop"], json!({ "$pattern": "vetro" }));
        assert!(parte.get("filter").is_none());
        assert!(parte.get("layer").is_none());
        assert!(parte.get("after").is_some());
        assert!(parte["states"].get("active").is_none());
        assert_eq!(
            parte["states"]["current"]["backdrop"],
            json!({ "$pattern": "vetro" })
        );
        assert_eq!(
            parte["motion"],
            json!({ "enter": "sale", "current": "pulsa" })
        );
    }

    #[test]
    fn un_livello_su_una_copertina_si_toglie() {
        let fuori = migrato(json!({
            "parts": { "np-art": { "radius": "14px", "layer": { "background": [] } } }
        }));
        assert_eq!(fuori["parts"]["np-art"], json!({ "radius": "14px" }));
    }

    #[test]
    fn le_classi_prestate_si_copiano_senza_sovrascrivere() {
        let fuori = migrato(json!({
            "parts": {
                "list-row": { "radius": "6px" },
                "card": { "radius": "20px" },
                "glass-modal": { "radius": "18px" }
            }
        }));
        let parti = &fuori["parts"];
        assert_eq!(parti["card"]["radius"], "20px", "una copia non sovrascrive");
        assert_eq!(parti["track-card"]["radius"], "6px");
        assert_eq!(parti["artist-card"]["radius"], "6px");
        assert_eq!(parti["queue-panel"]["radius"], "18px");
        assert!(parti.get("eq-panel").is_none(), "menu-pop non c'era");
    }

    #[test]
    fn player_e_sidebar_diventano_uno_scafale_scritto() {
        let fuori = migrato(json!({
            "layout": { "player": "bottom-bar", "sidebar": "hidden", "density": "compact" }
        }));
        let layout = &fuori["layout"];
        assert!(layout.get("player").is_none());
        assert!(layout.get("sidebar").is_none());
        assert_eq!(layout["density"], "compact");
        assert_eq!(layout["shell"]["zone"], "column");
        assert_eq!(layout["shell"]["part"], "app-shell");
    }

    #[test]
    fn uno_scafale_gia_scritto_vince_sulle_manopole() {
        let fuori = migrato(json!({
            "layout": { "player": "floating", "shell": { "zone": "row", "children": [] } }
        }));
        assert_eq!(
            fuori["layout"],
            json!({ "shell": { "zone": "row", "children": [] } })
        );
    }

    #[test]
    fn una_manopola_sbagliata_del_formato_1_e_un_errore_col_percorso() {
        let Value::Object(mut map) = json!({ "layout": { "player": "laterale" } }) else {
            panic!();
        };
        let guasto = da_1_a_2(&mut map).expect_err("rifiutata");
        assert_eq!(guasto.path, "layout.player");
    }

    #[test]
    fn la_forma_canonica_mette_le_chiavi_nell_ordine_dello_schema() {
        let testo = forma_canonica(&json!({
            "tokens": { "color.accent": "#fff", "color.surface.0": "#000" },
            "meta": { "version": "1.0.0", "name": "X", "author": "Y" },
            "id": "x",
            "format": 2,
            "parts": { "toast-card": { "opacity": 1.0 }, "app-shell": { "radius": "4px" } }
        }));
        let posto = |s: &str| testo.find(s).unwrap_or(usize::MAX);
        assert!(posto("\"format\"") < posto("\"id\""));
        assert!(posto("\"id\"") < posto("\"meta\""));
        assert!(posto("\"name\"") < posto("\"author\""));
        assert!(posto("\"author\"") < posto("\"version\""));
        assert!(posto("\"color.surface.0\"") < posto("\"color.accent\""));
        assert!(posto("\"app-shell\"") < posto("\"toast-card\""));
        // `1.0` si scrive `1`, come `JSON.stringify`.
        assert!(testo.contains("\"opacity\": 1\n"), "{testo}");
        assert!(testo.ends_with("}\n"));
    }

    #[test]
    fn un_testo_gia_del_formato_di_oggi_torna_com_era() {
        let scritto = "{ \"format\": 2,   \"id\": \"x\" }";
        let fuori = migra_testo(scritto).expect("letto");
        assert_eq!(fuori.sorgente, scritto);
        assert_eq!(fuori.da, 2);
    }

    #[test]
    fn un_testo_del_formato_1_torna_migrato_e_canonico() {
        let fuori = migra_testo(include_str!("../tests/oro/plain.v1.json")).expect("migrato");
        assert_eq!(fuori.da, 1);
        assert!(fuori.sorgente.starts_with("{\n  \"format\": 2,\n"));
        crate::document::parse_skin_json(&fuori.sorgente).expect("Plain migrata è valida");
    }

    #[test]
    fn un_testo_che_non_e_json_da_un_errore_con_la_posizione() {
        let guasti = migra_testo("{ \"format\": 1, ").expect_err("rotto");
        assert_eq!(guasti.len(), 1);
        assert!(guasti.first().is_some_and(|g| g.punto.is_some()));
    }
}
