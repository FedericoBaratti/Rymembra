//! La migrazione dal formato 1 non cambia l'aspetto di nessuna skin.
//!
//! In `tests/oro/` stanno quattro skin del formato 1 e il foglio che il
//! compilatore della 2.x ne ricavava, congelati *prima* di toccare il nucleo.
//! Qui la stessa sorgente passa dalla migrazione al compilatore di oggi, e i
//! due fogli si confrontano come terne (regola, proprietà, valore): l'ordine
//! delle regole di parti diverse non conta, quello delle regole di una stessa
//! parte sì.
//!
//! Le sole differenze ammesse sono quelle scritte nel CHANGELOG della 3.0.0:
//!
//! - la rotta si anima sulla presenza della pagina, non su `(root)`;
//! - «attivo» diventa «corrente» e riconosce anche `aria-current`;
//! - un livello eredita il raggio del suo ospite;
//! - ogni parte animata porta le sue regole di arresto;
//! - le parti nate dalla divisione di una classe prestata ricevono una copia
//!   dello stile della parte da cui prendevano in prestito la classe;
//! - una tinta piena che non sta in fondo a un `background` si scrive come un
//!   gradiente di un colore solo. Prima era CSS non valido, e il browser
//!   buttava l'intero fondo: è l'unico caso in cui l'aspetto cambia, perché
//!   prima non c'era.

use std::collections::BTreeSet;

use aether_skin::migrazione::COPIE;
use aether_skin::{compile_skin, migra_testo, parse_skin_json};

const ORO: &[(&str, &str, &str)] = &[
    (
        "plain",
        include_str!("oro/plain.v1.json"),
        include_str!("oro/plain.v1.css"),
    ),
    (
        "sala",
        include_str!("oro/sala.v1.json"),
        include_str!("oro/sala.v1.css"),
    ),
    (
        "scafalatura",
        include_str!("oro/scafalatura.v1.json"),
        include_str!("oro/scafalatura.v1.css"),
    ),
    (
        "completa",
        include_str!("oro/completa.v1.json"),
        include_str!("oro/completa.v1.css"),
    ),
];

/// Una dichiarazione con la catena di blocchi che la contiene.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Terna {
    regola: String,
    proprieta: String,
    valore: String,
}

/// Il foglio letto riga per riga, com'è scritto dal compilatore: un
/// selettore per riga, `{` in fondo all'ultima, una dichiarazione per riga.
/// Una lista di selettori diventa una regola per selettore.
fn terne(css: &str) -> (Vec<Terna>, Vec<String>) {
    let mut pila: Vec<Vec<String>> = Vec::new();
    let mut in_attesa: Vec<String> = Vec::new();
    let mut terne = Vec::new();
    let mut ordine = Vec::new();

    for riga in css.lines().map(str::trim) {
        if riga.is_empty() || riga.starts_with("/*") {
            continue;
        }
        if riga == "}" {
            pila.pop();
            continue;
        }
        if let Some(testa) = riga.strip_suffix('{') {
            in_attesa.push(testa.trim().to_owned());
            let selettori: Vec<String> = std::mem::take(&mut in_attesa)
                .into_iter()
                .map(|s| s.trim_end_matches(',').trim().to_owned())
                .collect();
            // Il compilatore annida solo dentro `@media`, `@supports` e
            // `@keyframes`, che hanno una testa sola.
            let fuori = pila.last().and_then(|fuori| fuori.first());
            let percorsi: Vec<String> = selettori
                .iter()
                .map(|selettore| match fuori {
                    Some(fuori) => format!("{fuori} » {selettore}"),
                    None => selettore.clone(),
                })
                .collect();
            for percorso in &percorsi {
                if !percorso.contains("@keyframes") {
                    ordine.push(percorso.clone());
                }
            }
            pila.push(percorsi);
            continue;
        }
        if riga.ends_with(',') {
            in_attesa.push(riga.to_owned());
            continue;
        }
        let coppia = riga.trim_end_matches(';').split_once(':');
        assert!(coppia.is_some(), "riga che non so leggere: {riga}");
        let Some((proprieta, valore)) = coppia else {
            continue;
        };
        for regola in pila.last().into_iter().flatten() {
            terne.push(Terna {
                regola: regola.clone(),
                proprieta: proprieta.trim().to_owned(),
                valore: valore.trim().to_owned(),
            });
        }
    }
    (terne, ordine)
}

/// Le rinomine del formato 2 applicate a una regola del formato 1.
fn rinomina(regola: &str) -> String {
    regola
        .replace(
            "::view-transition-old(root)",
            "::view-transition-old(.vt-page-content-exit)",
        )
        .replace(
            "::view-transition-new(root)",
            "::view-transition-new(.vt-page-content-enter)",
        )
        .replace(
            ":where([data-active], .is-active)",
            ":where([aria-current]:not([aria-current=\"false\"]), [data-active], .is-active)",
        )
}

/// I pezzi di una lista CSS separati dalle virgole di primo livello.
fn pezzi(valore: &str) -> Vec<&str> {
    let mut fuori = Vec::new();
    let mut profondita = 0_u32;
    let mut inizio = 0;
    for (posto, c) in valore.char_indices() {
        match c {
            '(' => profondita += 1,
            ')' => profondita = profondita.saturating_sub(1),
            ',' if profondita == 0 => {
                fuori.push(valore[inizio..posto].trim());
                inizio = posto + 1;
            }
            _ => {}
        }
    }
    fuori.push(valore[inizio..].trim());
    fuori
}

/// Il fondo del formato 1 com'è scritto oggi: una tinta piena prima
/// dell'ultimo strato diventa un gradiente di un colore solo.
fn ripara_sfondo(terna: Terna) -> Terna {
    if terna.proprieta != "background" {
        return terna;
    }
    let strati = pezzi(&terna.valore);
    let ultimo = strati.len() - 1;
    let tinta = |strato: &str| {
        strato.starts_with("rgb(")
            || strato.starts_with("var(--color-")
            || strato.starts_with("var(--skin-color-")
            || strato.starts_with("var(--accent")
    };
    let valore = strati
        .iter()
        .enumerate()
        .map(|(posto, strato)| {
            if posto < ultimo && tinta(strato) {
                format!("linear-gradient({strato}, {strato})")
            } else {
                (*strato).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    Terna { valore, ..terna }
}

/// `.da` → `.a` dove `.da` è una classe intera, non l'inizio di un'altra.
fn cambia_classe(regola: &str, da: &str, a: &str) -> Option<String> {
    let cercato = format!(".{da}");
    let mut fuori = String::new();
    let mut resto = regola;
    let mut trovata = false;
    while let Some(posto) = resto.find(&cercato) {
        let dopo = &resto[posto + cercato.len()..];
        let intera = !dopo
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        fuori.push_str(&resto[..posto]);
        if intera {
            fuori.push('.');
            fuori.push_str(a);
            trovata = true;
        } else {
            fuori.push_str(&cercato);
        }
        resto = dopo;
    }
    fuori.push_str(resto);
    trovata.then_some(fuori)
}

fn ammessa(terna: &Terna, prima: &BTreeSet<Terna>) -> bool {
    if terna.valore == "none !important" && terna.proprieta == "animation" {
        return true;
    }
    if terna.proprieta == "border-radius"
        && terna.valore == "inherit"
        && (terna.regola.ends_with("::before") || terna.regola.ends_with("::after"))
    {
        return true;
    }
    COPIE.iter().any(|(da, a)| {
        cambia_classe(&terna.regola, a, da).is_some_and(|regola| {
            prima.contains(&Terna {
                regola: rinomina(&regola),
                ..terna.clone()
            }) || prima.contains(&Terna {
                regola,
                ..terna.clone()
            })
        })
    })
}

#[test]
fn la_migrazione_non_cambia_l_aspetto() {
    for (id, sorgente, oro) in ORO {
        let skin = parse_skin_json(sorgente)
            .unwrap_or_else(|err| panic!("«{id}» non si legge più: {err}"));
        let oggi = compile_skin(&skin).css;

        let (prima, ordine_prima) = terne(oro);
        let prima: BTreeSet<Terna> = prima
            .into_iter()
            .map(|terna| {
                ripara_sfondo(Terna {
                    regola: rinomina(&terna.regola),
                    ..terna
                })
            })
            .collect();
        let (dopo, ordine_dopo) = terne(&oggi);
        let dopo: BTreeSet<Terna> = dopo.into_iter().collect();

        let perse: Vec<&Terna> = prima.difference(&dopo).collect();
        assert!(
            perse.is_empty(),
            "«{id}» ha perso delle dichiarazioni: {perse:#?}"
        );

        let nuove: Vec<&Terna> = dopo
            .difference(&prima)
            .filter(|terna| !ammessa(terna, &prima))
            .collect();
        assert!(
            nuove.is_empty(),
            "«{id}» ha dichiarazioni che non c'erano: {nuove:#?}"
        );

        // L'ordine delle regole di una stessa parte è la cascata fra i suoi
        // stati: le regole del formato 1 devono comparire nello stesso ordine.
        let ordine_prima: Vec<String> = ordine_prima.iter().map(|r| rinomina(r)).collect();
        let mut cursore = ordine_dopo.iter();
        for regola in &ordine_prima {
            assert!(
                cursore.any(|dopo| dopo == regola),
                "«{id}»: la regola {regola} è fuori posto"
            );
        }
    }
}

#[test]
fn il_testo_migrato_si_rilegge_uguale() {
    for (id, sorgente, _) in ORO {
        let migrato = migra_testo(sorgente).unwrap_or_else(|guasti| panic!("«{id}»: {guasti:#?}"));
        assert_eq!(migrato.da, 1, "«{id}»");

        // Il testo che lo Studio mostra dice la stessa skin che l'app legge.
        let dal_testo = parse_skin_json(&migrato.sorgente)
            .unwrap_or_else(|err| panic!("«{id}» migrato non si legge: {err}"));
        let diretta = parse_skin_json(sorgente).expect("letta sopra");
        assert_eq!(
            compile_skin(&dal_testo).css,
            compile_skin(&diretta).css,
            "«{id}»"
        );

        // E migrare di nuovo non cambia niente.
        let di_nuovo = migra_testo(&migrato.sorgente).expect("già al formato 2");
        assert_eq!(di_nuovo.da, 2, "«{id}»");
        assert_eq!(di_nuovo.sorgente, migrato.sorgente, "«{id}»");
    }
}
