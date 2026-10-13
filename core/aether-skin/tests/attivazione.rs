//! Una skin sicura da indossare: il lampeggio, i budget, l'esame.
//!
//! Le skin si scambiano, e chi ne installa una non l'ha scritta. Queste prove
//! stanno sul confine fra «brutta» e «pericolosa»: quel che non deve passare
//! nemmeno a chi lo vuole — una luce che lampeggia — quel che non si installa
//! perché sfora troppo, e quel che si installa ma va detto prima di indossarla.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "una prova che fallisce deve fermarsi dicendo dove: `expect` e `panic!` sono               il modo in cui lo dice"
)]

use aether_skin::document::{
    Esame, MEZZO_CICLO_MINIMO_MS, controlla_installazione, esamina, fuori_misura,
};
use aether_skin::{SkinDocument, SkinIssue, WarningKind, check_skin, leggi_skin};

/// Un documento con le animazioni e le parti date, e l'intensità.
fn documento(intensita: &str, animazioni: &str, parti: &str) -> String {
    format!(
        r##"{{
          "format": 2,
          "id": "prova",
          "meta": {{ "name": "Prova", "author": "Aether", "version": "1.0.0" }},
          "tokens": {{}},
          "motion": {{ "intensity": "{intensita}", "animations": {animazioni} }},
          "parts": {parti}
        }}"##
    )
}

/// Una sola animazione, `lampo`, assegnata a `playing` sulla copertina della
/// colonna: lo stato in cui un ciclo è di casa.
fn col_lampo(lampo: &str) -> String {
    documento(
        "full",
        &format!(r##"{{ "lampo": {lampo} }}"##),
        r##"{ "column-art": { "motion": { "playing": "lampo" } } }"##,
    )
}

fn valida(json: &str) -> SkinDocument {
    leggi_skin(json).unwrap_or_else(|problemi| panic!("respinta: {problemi:#?}"))
}

fn lampeggio(json: &str) -> SkinIssue {
    let problemi = leggi_skin(json).expect_err("doveva essere respinta");
    problemi
        .into_iter()
        .find(|p| p.message.contains("lampeggia"))
        .expect("respinta, ma non per il lampeggio")
}

// ── Il lampeggio ────────────────────────────────────────────────────────────

#[test]
fn una_luminosita_che_va_e_viene_in_quaranta_millisecondi_non_passa() {
    let detto = lampeggio(&col_lampo(
        r##"{ "duration": "40ms", "iterations": "infinite", "direction": "alternate",
              "frames": [{ "at": 0, "brightness": 1 }, { "at": 100, "brightness": 1.8 }] }"##,
    ));
    // Sulla durata, che è la cosa da cambiare, e con le parole per cambiarla.
    assert_eq!(detto.path, "motion.animations.lampo.duration");
    assert!(detto.message.contains("la luminosità"), "{}", detto.message);
    assert!(detto.message.contains("40 ms"), "{}", detto.message);
    assert!(detto.message.contains("WCAG 2.3.1"), "{}", detto.message);
    assert!(detto.message.contains("all'infinito"), "{}", detto.message);
}

#[test]
fn un_colore_che_pulsa_in_due_secondi_passa() {
    let skin = valida(&col_lampo(
        r##"{ "duration": "2s", "iterations": "infinite", "easing": "ease-in-out",
              "frames": [{ "at": 0, "backgroundColor": "#1a1030" },
                         { "at": 50, "backgroundColor": "#ff3fd1" },
                         { "at": 100, "backgroundColor": "#1a1030" }] }"##,
    ));
    assert_eq!(skin.parts.len(), 1);
}

#[test]
fn un_lampeggio_dichiarato_e_mai_usato_non_passa_lo_stesso() {
    // La timeline dello Studio fa scorrere anche un'animazione non assegnata.
    let lampo = documento(
        "full",
        r##"{ "lampo": { "duration": "100ms", "iterations": "infinite", "direction": "alternate",
               "frames": [{ "at": 0, "opacity": 1 }, { "at": 100, "opacity": 0 }] } }"##,
        "{}",
    );
    assert_eq!(lampeggio(&lampo).path, "motion.animations.lampo.duration");
}

#[test]
fn una_trasformazione_veloce_non_e_una_luce() {
    // Un giro rapido si muove, non lampeggia: la luce resta la stessa.
    valida(&col_lampo(
        r##"{ "duration": "200ms", "iterations": "infinite", "easing": "linear",
              "frames": [{ "at": 0, "rotate": 0, "scale": 1 }, { "at": 100, "rotate": 360, "scale": 1.1 }] }"##,
    ));
}

#[test]
fn tre_ripetizioni_non_sono_un_lampeggio_e_quattro_si() {
    let volte = |n: u32| {
        col_lampo(&format!(
            r##"{{ "duration": "60ms", "iterations": {n}, "direction": "alternate",
                  "frames": [{{ "at": 0, "opacity": 1 }}, {{ "at": 100, "opacity": 0.2 }}] }}"##
        ))
    };
    valida(&volte(3));
    let detto = lampeggio(&volte(4));
    assert!(
        detto.message.contains("si ripete 4 volte"),
        "{}",
        detto.message
    );
}

#[test]
fn una_salita_scritta_in_piu_fotogrammi_e_un_cambio_solo() {
    // Un secondo per salire, un salto per ricominciare: un lampo al secondo.
    // Contare ogni fotogramma come un cambio direbbe 250 ms, e sbaglierebbe.
    valida(&col_lampo(
        r##"{ "duration": "1s", "iterations": "infinite", "easing": "linear",
              "frames": [{ "at": 0, "opacity": 0.2 }, { "at": 25, "opacity": 0.4 },
                         { "at": 50, "opacity": 0.6 }, { "at": 75, "opacity": 0.8 },
                         { "at": 100, "opacity": 1 }] }"##,
    ));
}

#[test]
fn una_raffica_in_un_giro_lungo_lampeggia_comunque() {
    // Dieci secondi di giro non diluiscono cinque lampi nel primo: conta il
    // lampo più breve, non la media.
    let detto = lampeggio(&col_lampo(
        r##"{ "duration": "10s", "iterations": "infinite",
              "frames": [{ "at": 0, "opacity": 1 }, { "at": 1, "opacity": 0 },
                         { "at": 2, "opacity": 1 }, { "at": 3, "opacity": 0 },
                         { "at": 4, "opacity": 1 }, { "at": 100, "opacity": 1 }] }"##,
    ));
    assert!(detto.message.contains("l'opacità"), "{}", detto.message);
}

#[test]
fn il_salto_di_fine_giro_conta() {
    // Un solo passo di 600 ms, sopra la soglia. Ma `normal` torna al primo
    // fotogramma **di colpo**: nero che schiarisce e scatta nero, un lampo
    // ogni 600 ms, cioè un mezzo ciclo di 300.
    let giro = |verso: &str| {
        col_lampo(&format!(
            r##"{{ "duration": "600ms", "iterations": "infinite", "direction": "{verso}",
                  "frames": [{{ "at": 0, "color": "#000000" }}, {{ "at": 100, "color": "#ffffff" }}] }}"##
        ))
    };
    let detto = lampeggio(&giro("normal"));
    assert!(
        detto.message.contains("il colore del testo"),
        "{}",
        detto.message
    );
    assert!(detto.message.contains("300 ms"), "{}", detto.message);
    // Con `alternate` ridiscende in altri 600 ms invece che di colpo.
    const { assert!(MEZZO_CICLO_MINIMO_MS < 600.0) };
    valida(&giro("alternate"));
}

#[test]
fn l_intensita_essential_dimezza_le_durate_e_il_conto_lo_sa() {
    let lento = r##"{ "lampo": { "duration": "500ms", "iterations": "infinite", "direction": "alternate",
          "frames": [{ "at": 0, "opacity": 1 }, { "at": 100, "opacity": 0.5 }] } }"##;
    let parti = r##"{ "column-art": { "motion": { "playing": "lampo" } } }"##;
    valida(&documento("full", lento, parti));
    let detto = lampeggio(&documento("essential", lento, parti));
    assert!(detto.message.contains("250 ms"), "{}", detto.message);
    assert!(detto.message.contains("essential"), "{}", detto.message);
}

#[test]
fn le_skin_di_serie_e_le_animazioni_pronte_non_lampeggiano() {
    for (id, testo) in aether_skin::DI_SERIE {
        leggi_skin(testo).unwrap_or_else(|p| panic!("{id}: {p:#?}"));
    }
    for pronta in aether_skin::ANIMAZIONI_PRONTE {
        // Come la striscia dello Studio le scrive: dichiarate così come sono.
        // Il lampeggio si misura sulla dichiarazione, quindi basta questo.
        let json = documento(
            "full",
            &format!(r##"{{ "pronta": {} }}"##, pronta.valore),
            "{}",
        );
        if let Err(problemi) = leggi_skin(&json) {
            let lampi: Vec<&SkinIssue> = problemi
                .iter()
                .filter(|p| p.message.contains("lampeggia"))
                .collect();
            assert!(lampi.is_empty(), "{}: {lampi:#?}", pronta.id);
        }
    }
}

// ── I budget all'installazione ──────────────────────────────────────────────

/// Un'animazione che costa 23 — un colore, sedici ripetizioni — assegnata a
/// quanti stati si vuole della stessa scheda.
fn cara(stati: &[&str]) -> String {
    let assegnate: Vec<String> = stati
        .iter()
        .map(|s| format!(r##""{s}": "tinta""##))
        .collect();
    documento(
        "full",
        r##"{ "tinta": { "duration": "2s", "iterations": 16,
              "frames": [{ "at": 0, "backgroundColor": "#101014" },
                         { "at": 50, "backgroundColor": "#20202a" },
                         { "at": 100, "backgroundColor": "#101014" }] } }"##,
        &format!(
            r##"{{ "card": {{ "motion": {{ {} }} }} }}"##,
            assegnate.join(", ")
        ),
    )
}

#[test]
fn oltre_il_doppio_del_budget_la_skin_non_si_installa() {
    let skin = valida(&cara(&["hover", "current"]));
    // Nello Studio resta un avviso, e dice già che non si installerà.
    let avviso = check_skin(&skin)
        .into_iter()
        .find(|a| a.kind == WarningKind::CostBudget && a.path == "parts.card.motion")
        .expect("l'avviso del movimento");
    assert!(
        avviso.message.contains("non si installa"),
        "{}",
        avviso.message
    );
    // All'installazione è un errore, al suo posto.
    let fuori = fuori_misura(&skin);
    assert_eq!(fuori.len(), 1, "{fuori:#?}");
    assert_eq!(
        fuori.first().map(|p| p.path.as_str()),
        Some("parts.card.motion")
    );
    // Col codice suo: il manifest è valido, e «non si legge» sarebbe falso.
    let err = controlla_installazione(&skin).expect_err("respinta");
    assert_eq!(err.code().kind().code(), "skin.overBudget");
    assert_eq!(
        err.code(),
        &aether_domain::errors::ErrorCode::SkinOverBudget {
            costo: 46,
            budget: aether_skin::MOTION_COST_BUDGET,
        }
    );
}

#[test]
fn sopra_il_budget_ma_sotto_il_doppio_si_installa_e_si_chiede() {
    let skin = valida(&cara(&["hover"]));
    let avviso = check_skin(&skin)
        .into_iter()
        .find(|a| a.kind == WarningKind::CostBudget)
        .expect("l'avviso del movimento");
    assert!(
        !avviso.message.contains("non si installa"),
        "{}",
        avviso.message
    );
    controlla_installazione(&skin).expect("si installa");
    let esame = esamina(&skin);
    assert!(esame.movimento_parte.sfora());
    assert!(!esame.movimento_parte.fuori_misura());
    assert!(esame.da_confermare());
}

#[test]
fn le_skin_del_formato_1_si_installano_ancora() {
    // La soglia è nuova, le skin in giro no: quelle scritte per la 2.x — qui le
    // quattro che la prova di migrazione tiene da parte, `completa` compresa —
    // devono continuare a entrare.
    for (nome, testo) in [
        ("plain", include_str!("oro/plain.v1.json")),
        ("sala", include_str!("oro/sala.v1.json")),
        ("scafalatura", include_str!("oro/scafalatura.v1.json")),
        ("completa", include_str!("oro/completa.v1.json")),
    ] {
        let skin = valida(testo);
        controlla_installazione(&skin).unwrap_or_else(|e| panic!("{nome}: {e:?}"));
    }
}

#[test]
fn le_skin_di_serie_si_installerebbero_senza_domande() {
    for (id, testo) in aether_skin::DI_SERIE {
        let skin = valida(testo);
        controlla_installazione(&skin).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        let esame = esamina(&skin);
        assert!(!esame.da_confermare(), "{id}: {esame:#?}");
    }
}

// ── L'esame ─────────────────────────────────────────────────────────────────

fn esame_di(id: &str) -> Esame {
    let testo = aether_skin::di_serie(id).expect("di serie");
    esamina(&valida(testo))
}

#[test]
fn l_esame_dice_cosa_gira_per_sempre() {
    // Plain non ha cicli, ma si muove: il cambio di pagina è una rotta.
    let plain = esame_di("plain");
    assert!(plain.anima);
    assert!(plain.cicli.is_empty());

    // Il disco gira finché suona, sulla colonna e nella schermata grande.
    let vinile = esame_di("vinile");
    assert!(vinile.anima);
    let giri: Vec<(&str, &str, &str)> = vinile
        .cicli
        .iter()
        .map(|c| (c.parte, c.stato, c.animazione.as_str()))
        .collect();
    assert!(
        giri.contains(&("column-art", "playing", "gira")),
        "{giri:?}"
    );
    assert!(giri.contains(&("np-art", "playing", "gira")), "{giri:?}");
}

#[test]
fn una_skin_ferma_non_anima_niente_anche_se_lo_dichiara() {
    let skin = valida(&documento(
        "none",
        r##"{ "gira": { "duration": "2s", "iterations": "infinite", "easing": "linear",
              "frames": [{ "at": 0, "rotate": 0 }, { "at": 100, "rotate": 360 }] } }"##,
        r##"{ "column-art": { "motion": { "playing": "gira" } } }"##,
    ));
    let esame = esamina(&skin);
    assert!(!esame.anima);
    assert!(esame.cicli.is_empty());
}

#[test]
fn un_testo_che_non_si_legge_si_chiede_prima_di_indossarla() {
    let skin = valida(
        r##"{
          "format": 2,
          "id": "grigia",
          "meta": { "name": "Grigia", "author": "Aether", "version": "1.0.0" },
          "tokens": {
            "color.surface.0": "#777777",
            "color.surface.1": "#777777",
            "color.surface.2": "#777777",
            "color.text.1": "#808080"
          }
        }"##,
    );
    let esame = esamina(&skin);
    assert!(esame.contrasti >= 3, "{esame:#?}");
    assert!(esame.da_confermare());
    // Il contrasto è un avviso, non un budget: si installa.
    controlla_installazione(&skin).expect("si installa");
}
