//! Lo scafale: `layout`, cioè densità, prefab e l'albero di zone e widget,
//! letto con un tetto sui nodi che si spende mentre si legge.

use rymembra_domain::errors::ErrorCode;
use serde_json::{Map, Value};

use super::{
    CHIAVI_IMPAGINAZIONE, CHIAVI_RICHIAMO, CHIAVI_WIDGET, CHIAVI_ZONA, Density, Esito, SkinIssue,
    SkinLayout, booleano, campo, giu, lista, oggetto, parola, problema, richiesto, solo_chiavi,
    testo,
};
use crate::layout::{
    Align, GapStep, LayoutNode, LayoutZone, MAX_SHELL_NODES, OptionKind, OptionValue, Spread,
    TrackSize, WidgetDef, WidgetInstance, WidgetOption, ZoneKind, shell_di_serie,
};
use crate::tokens::is_local_name;
use crate::values::parse_length;
use crate::vicini::{forse, vicini};

pub(super) fn impaginazione(
    value: &Value,
    path: &str,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<SkinLayout> {
    let map = oggetto(value, path)?;
    for chiave in ["player", "sidebar"] {
        if map.contains_key(chiave) {
            return Err(problema(
                &giu(path, chiave),
                "nel formato 2 non c'è più: dove stanno il lettore e la barra lo dice lo \
                 scafale, `layout.shell`, scritto per esteso",
            ));
        }
    }
    solo_chiavi(map, path, CHIAVI_IMPAGINAZIONE)?;
    let density = campo(map, "density")
        .map(|v| {
            parola(
                v,
                &giu(path, "density"),
                &[
                    ("compact", Density::Compact),
                    ("comfortable", Density::Comfortable),
                    ("spacious", Density::Spacious),
                ],
            )
        })
        .transpose()?
        .unwrap_or(Density::Comfortable);

    // I prefab si leggono **prima** dello scafale, come i motivi prima delle
    // parti: un riferimento si risolve solo se quel che riferisce esiste già.
    let prefabs = match campo(map, "prefabs") {
        None => Vec::new(),
        Some(grezzo) => modelli(grezzo, &giu(path, "prefabs"), problemi)?,
    };

    let Some(grezzo) = campo(map, "shell") else {
        return Ok(SkinLayout {
            density,
            shell: shell_di_serie(),
            prefabs: prefabs.iter().map(|(n, _)| n.clone()).collect(),
        });
    };

    let dentro = giu(path, "shell");
    let prima = problemi.len();
    // La radice è già un nodo: il budget parte da uno in meno.
    let mut restanti = MAX_SHELL_NODES.saturating_sub(1);
    let albero = zona_scafale(grezzo, &dentro, Some(&prefabs), &mut restanti, problemi)?;

    // Le regole si applicano soltanto se l'albero è arrivato intero: un ramo
    // che non si è letto lascia un buco, e un buco produce errori veri
    // dell'albero mutilato e falsi di quello scritto.
    if problemi.len() == prima {
        for guasto in crate::layout::verifica(&albero) {
            problemi.push(problema_scafale(&dentro, &guasto));
        }
    }

    Ok(SkinLayout {
        density,
        shell: albero,
        prefabs: prefabs.into_iter().map(|(n, _)| n).collect(),
    })
}

/// I sottoalberi nominati che lo scafale può richiamare.
///
/// Un prefab è **un sottoalbero di widget già registrati**, e nient'altro:
/// niente parametri, niente condizioni, niente prefab dentro prefab.
fn modelli(
    value: &Value,
    path: &str,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<Vec<(String, LayoutZone)>> {
    let map = oggetto(value, path)?;
    let mut elenco = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        if !is_local_name(nome) {
            return Err(problema(
                &percorso,
                "il nome di un prefab ammette minuscole, cifre e trattini",
            ));
        }
        let mut restanti = MAX_SHELL_NODES.saturating_sub(1);
        let mut corpo = zona_scafale(grezzo, &percorso, None, &mut restanti, problemi)?;
        corpo.from_prefab = Some(nome.clone());
        elenco.push((nome.clone(), corpo));
    }
    elenco.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(elenco)
}

// ── Lo scafale ──────────────────────────────────────────────────────────────

/// Un problema dello scafale, col prefisso e il codice del catalogo.
///
/// Il percorso è **quello del JSON** e mai un indirizzo `data-nodo`: è ciò che
/// lo Studio consuma per portare il cursore sull'errore.
fn problema_scafale(prefisso: &str, guasto: &crate::layout::ShellIssue) -> SkinIssue {
    // Un problema dentro un prefab si riporta al sito di **dichiarazione**.
    let base = match guasto.prefab.as_ref() {
        None => prefisso.to_owned(),
        Some(nome) => format!(
            "{}.prefabs.{nome}",
            prefisso.strip_suffix(".shell").unwrap_or(prefisso)
        ),
    };
    let percorso = if guasto.path.is_empty() {
        base
    } else {
        format!("{base}.{}", guasto.path)
    };
    let Some(widget) = guasto.missing.as_ref() else {
        return problema(&percorso, guasto.message.clone());
    };
    SkinIssue {
        code: ErrorCode::SkinLayoutIncomplete {
            widget: widget.clone(),
        },
        path: percorso,
        message: guasto.message.clone(),
        punto: None,
    }
}

fn misura_traccia(value: &Value, path: &str) -> Esito<TrackSize> {
    let scritto = testo(value, path)?;
    match scritto {
        "hug" => Ok(TrackSize::Hug),
        "fill" => Ok(TrackSize::Fill),
        _ => parse_length(scritto).map(TrackSize::Fixed).ok_or_else(|| {
            problema(
                path,
                format!("«{scritto}» non è una misura: «hug», «fill», o una lunghezza in px o rem"),
            )
        }),
    }
}

fn opzioni_widget(
    def: &'static WidgetDef,
    value: &Value,
    path: &str,
) -> Esito<Vec<(&'static WidgetOption, OptionValue)>> {
    let map = oggetto(value, path)?;
    // Si parte dai difetti e si sovrascrive: chi legge la struttura a valle
    // trova sempre tutte le manopole, dichiarate o no.
    let mut valori = WidgetInstance::nuovo(def).options;

    for (nome, grezzo) in map {
        let percorso = giu(path, nome);
        let Some(opzione) = def.option(nome) else {
            let candidati = def.nearest_options(nome);
            return Err(problema(
                &percorso,
                format!(
                    "«{}» non ha una manopola «{nome}».{}",
                    def.name,
                    forse(&candidati)
                ),
            ));
        };
        let valore = match opzione.kind {
            OptionKind::Flag { .. } => OptionValue::Flag(booleano(grezzo, &percorso)?),
            OptionKind::Word { allowed, .. } => {
                let ammesse: Vec<(&str, &'static str)> =
                    allowed.iter().map(|parola| (*parola, *parola)).collect();
                OptionValue::Word(parola(grezzo, &percorso, &ammesse)?)
            }
            OptionKind::Count { min, max, .. } => {
                let quanti = grezzo
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| {
                        problema(&percorso, format!("qui ci va un intero da {min} a {max}"))
                    })?;
                OptionValue::Count(quanti)
            }
        };
        if let Some(voce) = valori.iter_mut().find(|(o, _)| o.name == opzione.name) {
            voce.1 = valore;
        }
    }

    Ok(valori)
}

fn foglia_scafale(map: &Map<String, Value>, path: &str) -> Esito<WidgetInstance> {
    solo_chiavi(map, path, CHIAVI_WIDGET)?;
    let percorso = giu(path, "widget");
    let nome = testo(richiesto(map, path, "widget")?, &percorso)?;
    let Some(def) = crate::layout::widget(nome) else {
        let candidati = crate::layout::nearest_widgets(nome);
        return Err(problema(
            &percorso,
            format!("widget inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };

    Ok(WidgetInstance {
        def,
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size")))
            .transpose()?
            .unwrap_or(def.natural),
        options: match campo(map, "options") {
            None => WidgetInstance::nuovo(def).options,
            Some(grezzo) => opzioni_widget(def, grezzo, &giu(path, "options"))?,
        },
    })
}

fn troppi_nodi(path: &str) -> SkinIssue {
    problema(
        path,
        format!("lo scafale supera i {MAX_SHELL_NODES} nodi, che è il massimo"),
    )
}

/// Prende dei nodi dal budget, o dice che l'albero non ci sta.
///
/// Il tetto si spende invece di contarlo alla fine perché un prefab si espande
/// dove è richiamato: l'albero cresce come il **prodotto** fra la larghezza di
/// un corpo e il numero dei richiami, e contare alla fine vuol dire contare
/// qualcosa che è già stato allocato.
fn prendi(restanti: &mut usize, quanti: usize, path: &str) -> Esito<()> {
    match restanti.checked_sub(quanti) {
        Some(rimasti) => {
            *restanti = rimasti;
            Ok(())
        }
        None => {
            *restanti = 0;
            Err(troppi_nodi(path))
        }
    }
}

/// `prefabs` è `None` dentro il corpo di un prefab, dove richiamarne un altro
/// è vietato, e l'elenco dichiarato — vuoto compreso — nello scafale.
fn zona_scafale(
    value: &Value,
    path: &str,
    prefabs: Option<&[(String, LayoutZone)]>,
    restanti: &mut usize,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<LayoutZone> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_ZONA)?;

    let kind = {
        let ammesse: Vec<(&str, ZoneKind)> =
            ZoneKind::ALL.iter().map(|k| (k.as_str(), *k)).collect();
        parola(richiesto(map, path, "zone")?, &giu(path, "zone"), &ammesse)?
    };

    let parte = match campo(map, "part") {
        None => None,
        Some(grezzo) => {
            let percorso = giu(path, "part");
            let nome = testo(grezzo, &percorso)?;
            match crate::parts::part(nome) {
                Some(def) => Some(def),
                // La stessa decisione di `stile_parte`, sulla seconda porta: una
                // parte ritirata su un nodo si accetta, e il nodo resta senza.
                None if crate::parts::ritirata(nome).is_some() => None,
                None => {
                    let candidati = crate::parts::nearest_parts(nome);
                    return Err(problema(
                        &percorso,
                        format!("parte inesistente: «{nome}».{}", forse(&candidati)),
                    ));
                }
            }
        }
    };

    let dentro = giu(path, "children");
    let voci = lista(richiesto(map, path, "children")?, &dentro)?;
    let mut figli = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let percorso = giu(&dentro, &indice.to_string());
        if *restanti == 0 {
            return Err(troppi_nodi(&dentro));
        }
        // Un figlio malformato smette di scendere in **quel** ramo; i fratelli
        // proseguono.
        match nodo_scafale(grezzo, &percorso, prefabs, restanti, problemi) {
            Ok(figlio) => figli.push(figlio),
            Err(guasto) => problemi.push(guasto),
        }
    }

    Ok(LayoutZone {
        kind,
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size")))
            .transpose()?
            .unwrap_or(TrackSize::Fill),
        gap: campo(map, "gap")
            .map(|v| {
                let ammesse: Vec<(&str, GapStep)> =
                    GapStep::ALL.iter().map(|g| (g.as_str(), *g)).collect();
                parola(v, &giu(path, "gap"), &ammesse)
            })
            .transpose()?
            .unwrap_or(GapStep::None),
        align: campo(map, "align")
            .map(|v| {
                let ammesse: Vec<(&str, Align)> =
                    Align::ALL.iter().map(|a| (a.as_str(), *a)).collect();
                parola(v, &giu(path, "align"), &ammesse)
            })
            .transpose()?
            .unwrap_or(Align::Stretch),
        spread: campo(map, "spread")
            .map(|v| {
                let ammesse: Vec<(&str, Spread)> =
                    Spread::ALL.iter().map(|s| (s.as_str(), *s)).collect();
                parola(v, &giu(path, "spread"), &ammesse)
            })
            .transpose()?
            .unwrap_or(Spread::Start),
        part: parte,
        from_prefab: None,
        children: figli,
    })
}

fn nodo_scafale(
    value: &Value,
    path: &str,
    prefabs: Option<&[(String, LayoutZone)]>,
    restanti: &mut usize,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<LayoutNode> {
    // Un nodo costa uno prima ancora che si sappia cos'è.
    prendi(restanti, 1, path)?;

    let map = oggetto(value, path)?;
    if campo(map, "widget").is_some() {
        return foglia_scafale(map, path).map(LayoutNode::Widget);
    }
    if campo(map, "zone").is_some() {
        return zona_scafale(value, path, prefabs, restanti, problemi).map(LayoutNode::Zone);
    }
    if campo(map, "prefab").is_some() {
        return modello_richiamato(map, path, prefabs, restanti).map(LayoutNode::Zone);
    }
    Err(problema(
        path,
        "un nodo dello scafale è una zona («zone»), un widget («widget») o un \
         prefab («prefab»), e questo non dichiara nessuno dei tre",
    ))
}

/// Un prefab richiamato: si espande qui, e da qui in giù non esiste più.
fn modello_richiamato(
    map: &Map<String, Value>,
    path: &str,
    prefabs: Option<&[(String, LayoutZone)]>,
    restanti: &mut usize,
) -> Esito<LayoutZone> {
    solo_chiavi(map, path, CHIAVI_RICHIAMO)?;
    let percorso = giu(path, "prefab");
    let nome = testo(richiesto(map, path, "prefab")?, &percorso)?;

    // Il corpo di un prefab si riconosce dal `None`, non dall'elenco vuoto:
    // vuoto è anche l'elenco di una skin che non dichiara prefab, e fino alla
    // 3.2 un richiamo dallo scafale di quella skin si sentiva dire che un
    // prefab non può contenerne un altro — un annidamento che non c'era.
    let Some(prefabs) = prefabs else {
        return Err(problema(
            &percorso,
            "un prefab non può contenerne un altro: sarebbe un linguaggio, \
             e i cicli non si riconoscerebbero senza visitare il grafo",
        ));
    };
    let Some((_, corpo)) = prefabs.iter().find(|(n, _)| n == nome) else {
        if prefabs.is_empty() {
            return Err(problema(
                &percorso,
                format!(
                    "prefab inesistente: «{nome}». I prefab si dichiarano in `layout.prefabs`, \
                     e questa skin non ne dichiara"
                ),
            ));
        }
        let candidati = vicini(nome, prefabs.iter().map(|(n, _)| n.as_str()));
        return Err(problema(
            &percorso,
            format!("prefab inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };

    // Il corpo intero, meno il nodo che `nodo_scafale` ha già pagato.
    prendi(restanti, corpo.conta_nodi().saturating_sub(1), path)?;

    Ok(LayoutZone {
        // La misura è l'unica cosa che il sito d'uso può dire.
        size: campo(map, "size")
            .map(|v| misura_traccia(v, &giu(path, "size")))
            .transpose()?
            .unwrap_or(corpo.size),
        ..corpo.clone()
    })
}
