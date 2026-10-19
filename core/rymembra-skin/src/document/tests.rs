//! Le prove del documento.

use super::*;
use crate::effects::Corner;
use crate::layout::{PlayerLayout, SidebarLayout, default_shell};
use crate::values::{Length, LengthUnit};

fn minima(extra: &str) -> String {
    format!(
        r##"{{
          "format": 2,
          "id": "prova",
          "meta": {{ "name": "Prova", "author": "Rymembra", "version": "1.0.0" }},
          "tokens": {{ "color.accent": "#8b7cf6" }}
          {extra}
        }}"##
    )
}

fn rifiuta(json: &str) -> AppError {
    match parse_skin_json(json) {
        Ok(_) => panic!("accettato quel che andava rifiutato: {json}"),
        Err(err) => err,
    }
}

fn messaggio(json: &str) -> String {
    rifiuta(json).message().unwrap_or_default().to_owned()
}

#[test]
fn una_skin_minima_passa() {
    let skin = parse_skin_json(&minima("")).expect("valida");
    assert_eq!(skin.id, "prova");
    assert_eq!(skin.tokens.len(), 1);
    assert!(skin.capabilities.dynamic_accent);
    assert_eq!(skin.scheme, Scheme::Dark);
    assert!(skin.variant.is_none());
}

#[test]
fn una_chiave_sconosciuta_e_un_errore() {
    let detto = messaggio(&minima(r##", "colours": {}"##));
    assert!(detto.contains("colours"), "{detto}");
    assert!(detto.contains("sconosciuta"), "{detto}");
}

#[test]
fn un_token_sbagliato_riceve_un_suggerimento() {
    let detto = messaggio(&minima("").replace("color.accent", "color.accents"));
    assert!(detto.contains("color.accents"), "{detto}");
    assert!(detto.contains("«color.accent»"), "{detto}");
}

#[test]
fn un_valore_di_token_sbagliato_nomina_il_token() {
    let err = rifiuta(&minima("").replace("#8b7cf6", "blu"));
    assert_eq!(
        *err.code(),
        ErrorCode::SkinTokenInvalid {
            token: "color.accent".to_owned(),
            value: "\"blu\"".to_owned(),
        }
    );
    assert!(
        err.message()
            .unwrap_or_default()
            .contains("non è un colore")
    );
}

#[test]
fn gli_errori_dei_token_arrivano_tutti_insieme() {
    let detto = messaggio(&minima("").replace(
        r##""color.accent": "#8b7cf6""##,
        r##""color.accent": "blu", "color.danger": "rosso", "color.text.9": "#fff""##,
    ));
    for nome in ["color.accent", "color.danger", "color.text.9"] {
        assert!(detto.contains(nome), "{detto}");
    }
}

#[test]
fn una_versione_futura_non_e_un_manifest_rotto() {
    let err = rifiuta(&minima("").replace("\"format\": 2", "\"format\": 3"));
    assert_eq!(
        *err.code(),
        ErrorCode::SkinFormatUnsupported {
            found: 3,
            supported: 2
        }
    );
}

#[test]
fn il_formato_e_2_e_il_formato_1_si_migra() {
    assert_eq!(SKIN_FORMAT_VERSION, 2);
    assert_eq!(MIGRAZIONI.len(), 1);
    // Un documento del formato 1 con le chiavi del formato 1 si legge.
    let vecchia = minima(
        r##", "capabilities": { "light": true },
             "themes": { "light": { "color.accent": "#6752d6" } },
             "layout": { "player": "floating", "sidebar": "expanded" },
             "parts": { "nav-pill": { "states": { "active": { "fontWeight": 600 } } } }"##,
    )
    .replace("\"format\": 2", "\"format\": 1");
    let skin = parse_skin_json(&vecchia).expect("il formato 1 si migra");
    assert!(skin.variant.is_some());
    assert_eq!(
        skin.layout.expect("layout").shell,
        default_shell(PlayerLayout::Floating, SidebarLayout::Expanded)
    );
    let parte = skin.parts.first().expect("nav-pill");
    assert!(parte.states.get(PartState::Current).is_some());
}

#[test]
fn le_chiavi_del_formato_1_nel_formato_2_dicono_come_si_scrivono_ora() {
    for (scritto, detto) in [
        (r##", "themes": { "light": {} }"##, "variant"),
        (r##", "capabilities": { "light": true }"##, "variant"),
        (
            r##", "motion": { "routeTransition": { "out": { "opacity": 0 } } }"##,
            "route",
        ),
        (r##", "layout": { "player": "floating" }"##, "layout.shell"),
        (
            r##", "parts": { "list-row": { "states": { "active": {} } } }"##,
            "current",
        ),
    ] {
        let fuori = messaggio(&minima(scritto));
        assert!(fuori.contains(detto), "{scritto}: {fuori}");
    }
}

#[test]
fn un_effetto_sconosciuto_ha_il_suo_codice() {
    let err = rifiuta(&minima(
        r##", "patterns": { "griglia": { "effect": "hairlineGrids", "color": "#fff", "cell": "36px" } }"##,
    ));
    assert_eq!(
        *err.code(),
        ErrorCode::SkinUnknownEffect {
            effect_type: "hairlineGrids".to_owned()
        }
    );
    assert!(err.message().unwrap_or_default().contains("«hairlineGrid»"));
}

#[test]
fn un_id_che_romperebbe_il_selettore_non_passa() {
    for cattivo in ["Prova", "pro va", "pro'va", "p", "pro/va", "1prova"] {
        let json = minima("").replace("\"id\": \"prova\"", &format!("\"id\": \"{cattivo}\""));
        rifiuta(&json);
    }
}

// ── Le parti ────────────────────────────────────────────────────────────────

#[test]
fn un_livello_su_una_parte_senza_pseudo_elemento_e_un_errore() {
    let detto = messaggio(&minima(
        r##", "parts": { "nav-pill": { "after": { "background": [{ "effect": "solid", "color": "#fff" }] } } }"##,
    ));
    assert!(detto.contains("::after"), "{detto}");
    // E una copertina, che è un'immagine, non ha né l'uno né l'altro.
    let detto = messaggio(&minima(
        r##", "parts": { "np-art": { "before": { "background": [{ "effect": "solid", "color": "#fff" }] } } }"##,
    ));
    assert!(detto.contains("::before"), "{detto}");
}

#[test]
fn i_due_livelli_si_leggono_dove_sono_liberi() {
    let skin = parse_skin_json(&minima(
        r##", "parts": { "section-card": {
            "before": { "background": [{ "effect": "solid", "color": "#000" }] },
            "after": { "background": [{ "effect": "solid", "color": "#fff" }], "opacity": 0.5 }
        } }"##,
    ))
    .expect("valida");
    let parte = skin.parts.first().expect("la parte");
    assert!(parte.before.is_some());
    assert_eq!(parte.after.as_ref().and_then(|l| l.opacity), Some(0.5));
}

#[test]
fn una_parte_sbagliata_per_assonanza_riceve_un_suggerimento() {
    let detto = messaggio(&minima(
        r##", "parts": { "section-cards": { "opacity": 1 } }"##,
    ));
    assert!(detto.contains("«section-card»"), "{detto}");
}

#[test]
fn una_parte_ritirata_si_legge_e_non_disegna() {
    let skin = parse_skin_json(&minima(
        r##", "parts": {
            "viz-title": { "opacity": 1, "textColor": "#fff" },
            "viz-screen": { "opacity": 1 }
        }"##,
    ))
    .expect("una parte ritirata non ferma la lettura");
    let ordine: Vec<&str> = skin.parts.iter().map(|s| s.def.name).collect();
    assert_eq!(ordine, ["viz-screen"]);
}

#[test]
fn una_parte_ritirata_su_un_nodo_dello_scafale_si_accetta_e_non_veste_niente() {
    let skin = parse_skin_json(&minima(
        r##", "layout": { "shell": { "zone": "row", "children": [
            { "widget": "navigation" },
            { "zone": "column", "part": "viz-title", "children": [
                { "widget": "content" },
                { "widget": "transport" }
            ] }
        ] } }"##,
    ))
    .expect("una parte ritirata non ferma la lettura dello scafale");
    let shell = skin.layout.expect("dichiara layout").shell;
    let LayoutNode::Zone(dentro) = &shell.children[1] else {
        panic!("il secondo figlio non è una zona");
    };
    assert!(dentro.part.is_none());
    assert!(shell.monta("content"));
}

#[test]
fn le_parti_escono_in_ordine_di_registro() {
    let skin = parse_skin_json(&minima(
        r##", "parts": {
            "toast-card": { "opacity": 1 },
            "app-shell": { "opacity": 1 },
            "nav-pill": { "opacity": 1 }
        }"##,
    ))
    .expect("valida");
    let ordine: Vec<&str> = skin.parts.iter().map(|s| s.def.name).collect();
    assert_eq!(ordine, ["app-shell", "nav-pill", "toast-card"]);
}

#[test]
fn l_aspetto_del_formato_2_si_legge_tutto() {
    let skin = parse_skin_json(&minima(
        r##", "parts": { "card": {
            "borderStyle": "dashed",
            "radius": { "topLeft": "20px", "bottomRight": "4px" },
            "shadow": { "$token": "shadow.2" },
            "focusRing": { "color": { "$token": "color.accent" }, "width": "2px", "offset": "3px" },
            "filter": [{ "saturate": 1.2 }, { "blur": "1px" }],
            "font": "display",
            "fontSize": "15px",
            "lineHeight": 1.4
        } }"##,
    ))
    .expect("valida");
    let aspetto = &skin.parts.first().expect("la parte").appearance;
    assert_eq!(
        aspetto.border_style,
        Some(crate::parts::BorderStyle::Dashed)
    );
    assert!(matches!(aspetto.radius, Some(Raggio::Angoli { .. })));
    assert!(matches!(aspetto.shadow, Some(Ombra::Token(def)) if def.id == "shadow.2"));
    assert!(aspetto.focus_ring.is_some());
    assert_eq!(aspetto.filter.len(), 2);
    assert_eq!(aspetto.font, Some(FontKind::Display));
    assert_eq!(aspetto.font_size, Some(Length::px(15.0)));
    assert_eq!(aspetto.line_height, Some(1.4));
}

#[test]
fn la_misura_del_testo_ha_estremi_e_unita_assolute() {
    assert!(
        messaggio(&minima(
            r##", "parts": { "card": { "fontSize": "120px" } }"##
        ))
        .contains("va fra 9px e 72px")
    );
    assert!(
        messaggio(&minima(r##", "parts": { "card": { "fontSize": "2vw" } }"##)).contains("px, rem")
    );
}

#[test]
fn un_filtro_scritto_come_nel_formato_1_dice_dov_e_andato() {
    let detto = messaggio(&minima(
        r##", "parts": { "glass-modal": { "filter": { "effect": "blurBehind", "radius": "12px" } } }"##,
    ));
    assert!(detto.contains("backdrop"), "{detto}");
}

#[test]
fn un_ombra_che_non_e_un_token_d_ombra_si_rifiuta() {
    let detto = messaggio(&minima(
        r##", "parts": { "card": { "shadow": { "$token": "color.accent" } } }"##,
    ));
    assert!(detto.contains("token d'ombra"), "{detto}");
}

#[test]
fn gli_stati_sono_dieci_e_active_si_chiama_current() {
    let skin = parse_skin_json(&minima(
        r##", "parts": { "list-row": { "states": {
            "hover": { "opacity": 0.9 },
            "pressed": { "opacity": 0.8 },
            "selected": { "fontWeight": 600 },
            "current": { "fontWeight": 700 },
            "open": { "opacity": 1 },
            "checked": { "opacity": 1 },
            "playing": { "opacity": 1 },
            "paused": { "opacity": 1 },
            "focus": { "opacity": 1 },
            "disabled": { "opacity": 0.4 }
        } } }"##,
    ))
    .expect("valida");
    let stati: Vec<PartState> = skin
        .parts
        .first()
        .expect("la parte")
        .states
        .iter()
        .map(|(s, _)| s)
        .collect();
    assert_eq!(stati, PartState::ALL);
}

// ── I motivi ────────────────────────────────────────────────────────────────

fn con_motivo(parti: &str) -> String {
    minima(&format!(
        r##", "patterns": {{
            "griglia": {{ "effect": "hairlineGrid", "color": "#fff", "cell": "36px" }},
            "taglio": {{ "effect": "chamfer", "size": "12px", "corners": ["topLeft"] }}
        }}, "parts": {{ {parti} }}"##
    ))
}

#[test]
fn un_motivo_si_richiama_per_nome() {
    let skin = parse_skin_json(&con_motivo(
        r##""app-shell": { "background": [{ "$pattern": "griglia" }], "clip": { "$pattern": "taglio" } }"##,
    ))
    .expect("valida");
    let stile = &skin.parts[0];
    assert_eq!(
        stile.appearance.background[0].pattern_name(),
        Some("griglia")
    );
    assert_eq!(
        stile.appearance.clip.as_ref().and_then(Paint::pattern_name),
        Some("taglio")
    );
    assert_eq!(stile.effects().len(), 2);
}

#[test]
fn un_motivo_inesistente_riceve_un_suggerimento() {
    let detto = messaggio(&con_motivo(
        r##""app-shell": { "background": [{ "$pattern": "griglias" }] }"##,
    ));
    assert!(detto.contains("motivo inesistente"), "{detto}");
    assert!(detto.contains("«griglia»"), "{detto}");
}

#[test]
fn un_motivo_nel_posto_sbagliato_e_un_errore() {
    for (dove, motivo) in [
        (r##""background": [{ "$pattern": "taglio" }]"##, "taglio"),
        (r##""clip": { "$pattern": "griglia" }"##, "griglia"),
    ] {
        let detto = messaggio(&con_motivo(&format!(r##""app-shell": {{ {dove} }}"##)));
        assert!(detto.contains(motivo), "{detto}");
        assert!(detto.contains("qui ci va"), "{detto}");
    }
}

#[test]
fn un_motivo_mai_richiamato_e_un_avviso() {
    let skin = parse_skin_json(&con_motivo(
        r##""app-shell": { "background": [{ "$pattern": "griglia" }] }"##,
    ))
    .expect("valida");
    let motivi: Vec<String> = check_skin(&skin)
        .into_iter()
        .filter(|a| a.kind == WarningKind::UnusedPattern)
        .map(|a| a.path)
        .collect();
    assert_eq!(motivi, ["patterns.taglio"]);
}

#[test]
fn un_motivo_richiamato_solo_in_uno_stato_e_richiamato() {
    let skin = parse_skin_json(&con_motivo(
        r##""app-shell": { "clip": { "$pattern": "taglio" },
            "states": { "hover": { "background": [{ "$pattern": "griglia" }] } } }"##,
    ))
    .expect("valida");
    assert!(
        !check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::UnusedPattern)
    );
}

// ── Gli effetti ─────────────────────────────────────────────────────────────

fn effetto_letto(scritto: &str) -> Effect {
    let grezzo: Value = serde_json::from_str(scritto).expect("json ben formato");
    effetto(&grezzo, "prova").expect("effetto valido")
}

fn effetto_storto(scritto: &str) -> SkinIssue {
    let grezzo: Value = serde_json::from_str(scritto).expect("json ben formato");
    match effetto(&grezzo, "prova") {
        Ok(_) => panic!("accettato quel che andava rifiutato: {scritto}"),
        Err(guaio) => guaio,
    }
}

fn tinta(r: u8, g: u8, b: u8) -> ColorValue {
    ColorValue::Literal(Rgba { r, g, b, a: 1.0 })
}

#[test]
fn una_tinta_piatta_torna_col_colore_che_e_stato_scritto() {
    assert_eq!(
        effetto_letto(r##"{ "effect": "solid", "color": "#8b7cf6" }"##),
        Effect::Solid {
            color: tinta(0x8b, 0x7c, 0xf6),
        }
    );
}

#[test]
fn un_gradiente_lineare_senza_angolo_scende_dall_alto() {
    assert_eq!(
        effetto_letto(
            r##"{ "effect": "linearGradient",
                  "stops": [{ "color": "#000" }, { "color": "#fff", "at": "80%" }] }"##
        ),
        Effect::LinearGradient {
            angle: 180.0,
            stops: vec![
                Stop {
                    color: tinta(0, 0, 0),
                    at: None,
                },
                Stop {
                    color: tinta(255, 255, 255),
                    at: Some(Length {
                        value: 80.0,
                        unit: LengthUnit::Percent,
                    }),
                },
            ],
        }
    );
}

#[test]
fn un_angolo_smussato_senza_elenco_taglia_i_due_di_serie() {
    assert_eq!(
        effetto_letto(r##"{ "effect": "chamfer", "size": "12px" }"##),
        Effect::Chamfer {
            size: Length::px(12.0),
            corners: vec![Corner::TopRight, Corner::BottomLeft],
        }
    );
}

#[test]
fn un_immagine_nomina_una_voce_del_pacchetto_e_niente_altro() {
    assert_eq!(
        effetto_letto(
            r##"{ "effect": "image", "src": "grana.webp", "size": "240px", "repeat": true }"##
        ),
        Effect::Image {
            src: "grana.webp".to_owned(),
            size: ImageSize::Misura(Length::px(240.0)),
            repeat: true,
            position: None,
        }
    );
    // Un indirizzo, una cartella, un tipo che non è un'immagine: no.
    for cattivo in [
        "https://esempio.invalid/x.png",
        "../x.png",
        "assets/x.png",
        "carattere.woff2",
        "x.svg",
    ] {
        let guaio = effetto_storto(&format!(r##"{{ "effect": "image", "src": "{cattivo}" }}"##));
        assert_eq!(guaio.path, "prova.src", "{cattivo}");
    }
}

#[test]
fn una_chiave_di_troppo_dentro_un_effetto_riceve_un_suggerimento() {
    let coda = effetto_storto(r##"{ "effect": "solid", "color": "#fff", "colors": "#000" }"##);
    assert_eq!(coda.path, "prova.colors");
    assert!(coda.message.contains("«color»"), "{}", coda.message);
}

#[test]
fn quel_che_manca_a_un_effetto_si_dice_col_percorso() {
    assert_eq!(
        effetto_storto(r##""solid""##).message,
        "qui ci va un oggetto"
    );
    let vuoto = effetto_storto("{}");
    assert_eq!(vuoto.path, "prova.effect");
    let senza_passo =
        effetto_storto(r##"{ "effect": "scanlines", "color": "#fff", "line": "1px" }"##);
    assert_eq!(senza_passo.path, "prova.gap");
}

#[test]
fn un_numero_fuori_scala_dice_fra_quali_estremi_sta() {
    let guaio = effetto_storto(r##"{ "effect": "vignette", "color": "#000", "start": 120 }"##);
    assert_eq!(guaio.message, "va fra 0 e 100, non 120");
}

// ── I caratteri ─────────────────────────────────────────────────────────────

#[test]
fn un_carattere_del_pacchetto_si_dichiara_e_si_nomina_nei_token() {
    let skin = parse_skin_json(&minima(
        r##", "fonts": { "Fraunces": { "src": "fraunces.woff2", "weight": [100, 900] } }"##,
    ))
    .expect("valida");
    assert_eq!(
        skin.fonts,
        vec![FontFace {
            family: "Fraunces".to_owned(),
            src: "fraunces.woff2".to_owned(),
            weight: (100, 900),
            italic: false,
        }]
    );
    assert_eq!(skin.assets(), ["fraunces.woff2"]);
    // Un'immagine non è un carattere.
    assert!(
        messaggio(&minima(
            r##", "fonts": { "Fraunces": { "src": "fraunces.png" } }"##
        ))
        .contains("tipo")
    );
}

#[test]
fn due_famiglie_che_nel_foglio_si_chiamano_uguale_si_rifiutano() {
    // «Titoli A» e «titoli-a» sono due chiavi diverse e un nome solo nel
    // foglio: due `@font-face` con la stessa famiglia e due file.
    let doppie = messaggio(&minima(
        r##", "fonts": { "Titoli A": { "src": "a.woff2" }, "titoli-a": { "src": "b.woff2" } }"##,
    ));
    assert!(doppie.contains("stesso nome"), "{doppie}");
    // Due famiglie davvero diverse passano.
    let skin = parse_skin_json(&minima(
        r##", "fonts": { "Titoli": { "src": "a.woff2" }, "Testo": { "src": "b.woff2" } }"##,
    ))
    .expect("valida");
    assert_eq!(skin.fonts.len(), 2);
}

// ── I temi ──────────────────────────────────────────────────────────────────

#[test]
fn una_skin_chiara_di_base_ha_una_variante_scura() {
    let skin = parse_skin_json(&minima(
        r##", "scheme": "light",
             "palette": { "inchiostro": "#101016" },
             "variant": {
               "tokens": { "color.accent": "#b9a8ff" },
               "palette": { "inchiostro": "#f0f0f6" },
               "parts": { "card": { "radius": "8px" } }
             }"##,
    ))
    .expect("valida");
    assert_eq!(skin.scheme, Scheme::Light);
    let variante = skin.variant.as_ref().expect("la variante");
    assert_eq!(variante.parts.len(), 1);
    // La tavolozza della variante ridefinisce, non aggiunge.
    let rifiutata = messaggio(&minima(
        r##", "variant": { "palette": { "nuovo": "#fff" } }"##,
    ));
    assert!(rifiutata.contains("non ne aggiunge"), "{rifiutata}");
}

#[test]
fn il_movimento_non_cambia_col_tema() {
    let detto = messaggio(&minima(
        r##", "variant": { "parts": { "card": { "motion": { "enter": "x" } } } }"##,
    ));
    assert!(detto.contains("non cambia col tema"), "{detto}");
}

#[test]
fn una_superficie_dipinta_a_mano_che_sparisce_nella_variante_e_un_avviso() {
    let skin = parse_skin_json(&minima(
        r##", "variant": { "tokens": { "color.text.1": "rgba(10, 10, 15, 0.95)" } },
             "parts": {
               "section-card": {
                 "background": [
                   { "effect": "linearGradient",
                     "stops": [{ "color": "#030305" }, { "color": "#0a0a12" }] }
                 ]
               }
             }"##,
    ))
    .expect("valida");
    let suoi: Vec<SkinWarning> = check_skin(&skin)
        .into_iter()
        .filter(|a| a.path == "parts.section-card.background")
        .collect();
    assert_eq!(suoi.len(), 1, "uno solo, nel tema del guasto: {suoi:?}");
    assert!(suoi[0].message.contains("chiaro"), "{}", suoi[0].message);
}

#[test]
fn la_stessa_superficie_ridipinta_nella_variante_non_avvisa() {
    let skin = parse_skin_json(&minima(
        r##", "variant": {
               "tokens": { "color.text.1": "rgba(10, 10, 15, 0.95)" },
               "parts": { "section-card": { "background": [{ "effect": "solid", "color": "#f4f4f8" }] } }
             },
             "parts": {
               "section-card": { "background": [{ "effect": "solid", "color": "#0a0a12" }] }
             }"##,
    ))
    .expect("valida");
    assert!(
        !check_skin(&skin)
            .iter()
            .any(|a| a.path.starts_with("parts.")),
        "{:?}",
        check_skin(&skin)
    );
}

#[test]
fn un_velo_trasparente_si_misura_sopra_il_fondo_dell_app() {
    // Un bagliore al 14% sopra una superficie chiara resta una superficie
    // chiara: misurato come se fosse pieno, il testo scuro sopra sembrerebbe
    // illeggibile, e l'avviso sarebbe falso.
    let skin = parse_skin_json(&minima(
        r##", "scheme": "light",
             "variant": { "tokens": { "color.text.1": "rgba(246, 238, 226, 0.92)", "color.surface.0": "#15110d" } },
             "parts": {
               "ambient-backdrop": {
                 "background": [
                   { "effect": "radialGradient",
                     "stops": [{ "color": "rgba(185, 138, 62, 0.14)" }, { "color": "rgba(185, 138, 62, 0)" }] }
                 ]
               }
             }"##,
    ))
    .expect("valida");
    let suoi: Vec<SkinWarning> = check_skin(&skin)
        .into_iter()
        .filter(|a| a.path == "parts.ambient-backdrop.background")
        .collect();
    assert!(suoi.is_empty(), "{suoi:?}");
}

#[test]
fn si_misura_lo_strato_in_fondo_che_in_css_e_l_ultimo() {
    // La tinta chiara sta sopra, ed è mezza trasparente; sotto c'è il nero
    // pieno. Il testo chiaro di serie ci sta bene: l'avviso sarebbe falso se
    // si guardasse lo strato di sopra come fosse il fondo.
    let skin = parse_skin_json(&minima(
        r##", "tokens": { "color.accent": "#8b7cf6", "color.text.1": "#ffffff", "color.surface.0": "#000000" },
             "parts": {
               "section-card": {
                 "background": [
                   { "effect": "solid", "color": "rgba(255, 255, 255, 0.04)" },
                   { "effect": "solid", "color": "#000000" }
                 ]
               }
             }"##,
    ))
    .expect("valida");
    assert!(
        !check_skin(&skin)
            .iter()
            .any(|a| a.path == "parts.section-card.background"),
        "{:?}",
        check_skin(&skin)
    );
}

#[test]
fn una_variante_vuota_e_una_promessa_non_mantenuta() {
    let skin = parse_skin_json(&minima(r##", "variant": {}"##)).expect("valida");
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::UnkeptCapability && a.path == "variant")
    );
}

// ── Lo scafale ──────────────────────────────────────────────────────────────

const SCAFALATURA: &str = include_str!("../../tests/scafalatura.json");

fn con_scafale(shell: &str) -> String {
    minima(&format!(r##", "layout": {{ "shell": {shell} }}"##))
}

const SANO: &str = r##"{
    "zone": "row",
    "children": [
        { "widget": "navigation" },
        { "widget": "content" },
        { "widget": "player" }
    ]
}"##;

#[test]
fn la_skin_di_riferimento_scrive_lo_scafale_che_la_finestra_ha_sempre_avuto() {
    let skin = parse_skin_json(crate::PLAIN_SOURCE).expect("plain è valida");
    assert_eq!(
        skin.layout.expect("plain dichiara layout").shell,
        default_shell(PlayerLayout::Floating, SidebarLayout::Expanded)
    );
}

#[test]
fn chi_non_dichiara_uno_scafale_riceve_quello_di_serie() {
    let skin =
        parse_skin_json(&minima(r##", "layout": { "density": "compact" }"##)).expect("valida");
    assert_eq!(skin.layout.expect("layout").shell, shell_di_serie());
}

#[test]
fn uno_scafale_scritto_per_esteso_si_legge() {
    let skin = parse_skin_json(SCAFALATURA).expect("valida");
    let layout = skin.layout.expect("dichiara layout");
    assert_eq!(layout.shell.kind, ZoneKind::Column);
    assert_eq!(layout.density, Density::Compact);
    assert!(layout.shell.monta("queue"));
}

#[test]
fn togliere_un_widget_essenziale_e_un_errore_col_suo_codice() {
    let err = rifiuta(&con_scafale(
        r##"{ "zone": "row", "children": [
            { "widget": "navigation" },
            { "widget": "player", "size": "fill" }
        ] }"##,
    ));
    assert_eq!(
        *err.code(),
        ErrorCode::SkinLayoutIncomplete {
            widget: "content".to_owned()
        }
    );
}

#[test]
fn un_widget_sconosciuto_riceve_un_suggerimento() {
    let detto = messaggio(&con_scafale(&SANO.replace("\"content\"", "\"contents\"")));
    assert!(detto.contains("«content»"), "{detto}");
}

#[test]
fn gli_errori_dello_scafale_arrivano_tutti_insieme() {
    let detto = messaggio(&con_scafale(
        r##"{ "zone": "row", "children": [
            { "widget": "navigazione" },
            { "widget": "contenuto" },
            { "widget": "player" }
        ] }"##,
    ));
    assert!(detto.contains("navigazione"), "{detto}");
    assert!(detto.contains("contenuto"), "{detto}");
}

#[test]
fn l_espansione_di_un_prefab_si_ferma_al_tetto_invece_che_alla_memoria() {
    let foglia = r##"{"zone":"row","children":[]}"##;
    let corpo: Vec<&str> = std::iter::repeat_n(foglia, 1500).collect();
    let usi: Vec<&str> = std::iter::repeat_n(r##"{"prefab":"barretta"}"##, 1500).collect();
    let detto = messaggio(&minima(&format!(
        r##", "layout": {{
            "prefabs": {{ "barretta": {{"zone":"row","children":[{}]}} }},
            "shell": {{ "zone": "row", "children": [{}] }}
        }}"##,
        corpo.join(","),
        usi.join(",")
    )));
    assert!(
        detto.contains(&format!("supera i {MAX_SHELL_NODES} nodi")),
        "{detto}"
    );
}

// ── I token ─────────────────────────────────────────────────────────────────

fn con_token(id: &str, valore: &str) -> String {
    format!(
        r##"{{
          "format": 2,
          "id": "prova",
          "meta": {{ "name": "Prova", "author": "Rymembra", "version": "1.0.0" }},
          "tokens": {{ "{id}": {valore} }}
        }}"##
    )
}

#[test]
fn il_ritmo_oltre_gli_estremi_e_un_errore() {
    assert!(
        rifiuta(&con_token("space.5", r#""400px""#))
            .to_string()
            .contains("va fra 0 e 64")
    );
    assert!(
        rifiuta(&con_token("space.3", r#""30vh""#))
            .to_string()
            .contains("px o rem")
    );
}

#[test]
fn i_token_obbligatori_mancanti_sono_un_avviso_non_un_errore() {
    let avvisi = check_skin(&parse_skin_json(&minima("")).expect("valida"));
    assert!(avvisi.len() > 10, "{avvisi:?}");
    assert!(
        avvisi
            .iter()
            .all(|a| a.kind == WarningKind::MissingRequiredToken)
    );
}

// ── Il movimento ────────────────────────────────────────────────────────────

fn con_animazioni(animazioni: &str, parti: &str) -> String {
    minima(&format!(
        r##", "motion": {{ "intensity": "full", "easings": {{ "molla": {{ "kind": "cubicBezier", "points": [0.34, 1.56, 0.64, 1] }} }}, "animations": {animazioni} }},
             "parts": {parti}"##
    ))
}

const SALE: &str = r##"{
  "duration": "240ms",
  "easing": { "kind": "keyword", "keyword": "ease-out" },
  "delay": "60ms",
  "frames": [
    { "at": 0, "opacity": 0, "translateY": 8 },
    { "at": 100, "opacity": 1, "translateY": 0 }
  ]
}"##;

const GIRA: &str = r##"{
  "duration": "2400ms", "easing": "linear", "iterations": "infinite",
  "frames": [{ "at": 0, "rotate": 0 }, { "at": 100, "rotate": 360 }]
}"##;

#[test]
fn un_animazione_nominata_si_dichiara_una_volta_e_si_richiama() {
    let skin = parse_skin_json(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "section-card": { "motion": { "enter": "sale", "hover": "sale" } } }"##,
    ))
    .expect("valida");
    let (nome, def) = skin
        .motion
        .as_ref()
        .and_then(|m| m.animations.first())
        .expect("l'animazione");
    assert_eq!(nome, "sale");
    assert_eq!(def.iterations, Iterazioni::Volte(1));
    assert_eq!(
        def.frames.first().and_then(|f| f.translate_y),
        Some(Length::px(8.0))
    );
    let parte = skin.parts.first().expect("la parte");
    assert_eq!(parte.animations_used(), ["sale", "sale"]);
    assert_eq!(parte.motion_cost(), 8);
    assert!(parte.appearance.is_empty());
}

#[test]
fn una_curva_si_richiama_per_nome_o_per_parola_chiave() {
    let skin = parse_skin_json(&con_animazioni(
        r##"{ "a": { "duration": "200ms", "easing": "molla", "frames": [{ "at": 0, "opacity": 0 }, { "at": 100, "opacity": 1 }] },
              "b": { "duration": "200ms", "easing": "ease-in", "frames": [{ "at": 0, "opacity": 0 }, { "at": 100, "opacity": 1 }] } }"##,
        "{}",
    ))
    .expect("valida");
    let animazioni = &skin.motion.expect("movimento").animations;
    assert_eq!(animazioni[0].1.easing, Curva::Nome("molla".to_owned()));
    assert!(matches!(
        animazioni[1].1.easing,
        Curva::Valore(Easing::Keyword(_))
    ));
    let detto = messaggio(&con_animazioni(
        r##"{ "a": { "duration": "200ms", "easing": "mola", "frames": [{ "at": 0, "opacity": 0 }, { "at": 100, "opacity": 1 }] } }"##,
        "{}",
    ));
    assert!(detto.contains("«molla»"), "{detto}");
}

#[test]
fn i_fotogrammi_fuori_ordine_si_rifiutano_invece_di_essere_riordinati() {
    let detto = messaggio(&con_animazioni(
        r##"{ "sale": { "duration": "240ms", "frames": [
            { "at": 0, "opacity": 0 },
            { "at": 80, "opacity": 1 },
            { "at": 40, "opacity": 0.5 }
        ] } }"##,
        "{}",
    ));
    assert!(detto.contains("ordine crescente"), "{detto}");
}

#[test]
fn un_ciclo_si_ammette_sugli_stati_e_non_sulle_entrate() {
    parse_skin_json(&con_animazioni(
        &format!(r##"{{ "gira": {GIRA} }}"##),
        r##"{ "column-art": { "motion": { "playing": "gira" } } }"##,
    ))
    .expect("un ciclo su playing è ammesso");
    let detto = messaggio(&con_animazioni(
        &format!(r##"{{ "gira": {GIRA} }}"##),
        r##"{ "column-art": { "motion": { "enter": "gira" } } }"##,
    ));
    assert!(detto.contains("non è uno stato"), "{detto}");
}

#[test]
fn l_uscita_esiste_solo_sulle_presenze_e_il_volo_solo_sulle_copertine() {
    parse_skin_json(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "menu-pop": { "motion": { "enter": "sale", "exit": "sale" } },
             "card-cover": { "motion": { "share": "sale" } } }"##,
    ))
    .expect("uscita su una presenza e volo su una copertina");
    let detto = messaggio(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "section-card": { "motion": { "exit": "sale" } } }"##,
    ));
    assert!(detto.contains("presenze"), "{detto}");
    let detto = messaggio(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "section-card": { "motion": { "share": "sale" } } }"##,
    ));
    assert!(detto.contains("copertine"), "{detto}");
}

#[test]
fn un_nome_di_animazione_inesistente_riceve_un_suggerimento() {
    let detto = messaggio(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "section-card": { "motion": { "enter": "sales" } } }"##,
    ));
    assert!(detto.contains("animazione inesistente: «sales»"), "{detto}");
    assert!(detto.contains("Forse intendevi «sale»?"), "{detto}");
}

#[test]
fn il_movimento_di_una_parte_ha_un_budget_suo() {
    let pulsa = r##"{
      "duration": "160ms", "iterations": 2, "direction": "alternate",
      "frames": [{ "at": 0, "scale": 1 }, { "at": 100, "scale": 1.08 }]
    }"##;
    let skin = parse_skin_json(&con_animazioni(
        &format!(r##"{{ "sale": {SALE}, "pulsa": {pulsa} }}"##),
        r##"{ "section-card": { "motion": {
            "enter": "sale", "hover": "sale", "current": "pulsa", "focus": "sale"
        } } }"##,
    ))
    .expect("valida");
    assert_eq!(skin.parts.first().map(PartStyle::motion_cost), Some(17));
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::CostBudget && a.path == "parts.section-card.motion"),
    );
}

#[test]
fn la_finestra_intera_ha_un_budget_del_movimento() {
    let parti: Vec<String> = crate::parts::PARTS
        .iter()
        .filter(|p| !p.presenza)
        .take(26)
        .map(|p| format!(r##""{}": {{ "motion": {{ "enter": "sale" }} }}"##, p.name))
        .collect();
    let skin = parse_skin_json(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        &format!("{{ {} }}", parti.join(",")),
    ))
    .expect("valida");
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::CostBudget && a.path == "parts"),
    );
}

#[test]
fn un_ciclo_che_non_parte_dalla_posa_di_riposo_e_un_avviso() {
    let storto = r##"{
      "duration": "2s", "iterations": "infinite",
      "frames": [{ "at": 0, "rotate": 30 }, { "at": 100, "rotate": 390 }]
    }"##;
    let skin = parse_skin_json(&con_animazioni(
        &format!(r##"{{ "storto": {storto} }}"##),
        r##"{ "column-art": { "motion": { "playing": "storto" } } }"##,
    ))
    .expect("valida");
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::RestingLoop)
    );
}

#[test]
fn una_cascata_senza_entrata_e_un_avviso() {
    let skin = parse_skin_json(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "card": { "motion": { "hover": "sale", "stagger": "40ms" } } }"##,
    ))
    .expect("valida");
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.path == "parts.card.motion.stagger")
    );
}

#[test]
fn una_cascata_su_uno_scorrimento_e_un_avviso_e_su_un_entrata_no() {
    // Sullo scorrimento la cascata non fa niente: ogni voce entra quando
    // arriva in vista. L'avviso lo dice, invece di suggerirlo come rimedio.
    let avvisi = |parti: &str| {
        let skin = parse_skin_json(&con_animazioni(
            &format!(r##"{{ "sale": {SALE} }}"##),
            parti,
        ))
        .expect("valida");
        check_skin(&skin)
            .into_iter()
            .filter(|a| a.path == "parts.card.motion.stagger")
            .collect::<Vec<_>>()
    };
    let scorre = avvisi(r##"{ "card": { "motion": { "inView": "sale", "stagger": "40ms" } } }"##);
    assert_eq!(scorre.len(), 1, "{scorre:#?}");
    assert!(
        scorre.iter().all(|a| a.message.contains("scorrimento")),
        "{scorre:#?}"
    );
    let entra = avvisi(r##"{ "card": { "motion": { "enter": "sale", "stagger": "40ms" } } }"##);
    assert!(entra.is_empty(), "{entra:#?}");
}

#[test]
fn un_animazione_dichiarata_e_mai_assegnata_e_un_avviso_non_un_costo() {
    let skin = parse_skin_json(&con_animazioni(
        &format!(r##"{{ "sale": {SALE} }}"##),
        r##"{ "section-card": { "opacity": 0.9 } }"##,
    ))
    .expect("valida");
    assert_eq!(skin.parts.first().map(PartStyle::motion_cost), Some(0));
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::UnusedAnimation && a.path == "motion.animations.sale")
    );
}

#[test]
fn i_fotogrammi_del_formato_2_si_leggono() {
    let skin = parse_skin_json(&con_animazioni(
        r##"{ "tutto": { "duration": "400ms", "origin": "top left", "fill": "forwards", "frames": [
            { "at": 0, "translateX": "-100%", "scaleX": 0.5, "skewY": 10, "blur": "4px",
              "brightness": 1.2, "hueRotate": 90, "backgroundColor": "#000", "color": { "$token": "color.accent" },
              "shadow": { "$token": "shadow.1" }, "clip": { "circle": { "radius": "0%" } } },
            { "at": 100, "translateX": 0, "scaleX": 1, "clip": { "polygon": [["0%", "0%"], ["100%", "0%"], ["50%", "100%"]] } }
        ] } }"##,
        "{}",
    ))
    .expect("valida");
    let def = &skin.motion.expect("movimento").animations[0].1;
    assert_eq!(def.fill, Riempimento::Forwards);
    assert!(def.origin.is_some());
    let primo = &def.frames[0];
    assert!(primo.ha_filtri() && primo.ha_pittura());
    assert!(matches!(primo.clip, Some(Ritaglio::Circle { .. })));
}

#[test]
fn le_parole_dei_fotogrammi_fuori_scala_si_rifiutano() {
    for (campo, valore) in [
        ("rotate", "900"),
        ("translateY", "900"),
        ("translateX", "\"300%\""),
        ("scale", "5"),
        ("skewX", "60"),
    ] {
        let detto = messaggio(&con_animazioni(
            &format!(
                r##"{{ "x": {{ "duration": "200ms", "frames": [{{ "at": 0, "{campo}": {valore} }}, {{ "at": 100, "opacity": 1 }}] }} }}"##
            ),
            "{}",
        ));
        assert!(detto.contains("va fra"), "{campo}: {detto}");
    }
}

#[test]
fn una_superficie_troppo_costosa_e_un_avviso() {
    let skin = parse_skin_json(&minima(
        r##", "parts": { "glass-modal": {
            "background": [
                { "effect": "blurBehind", "radius": "24px" },
                { "effect": "blurBehind", "radius": "12px" }
            ]
        } }"##,
    ))
    .expect("valida");
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.kind == WarningKind::CostBudget && a.path == "parts.glass-modal")
    );
}

// ── l'anello del fuoco ──────────────────────────────────────────────────────

#[test]
fn un_anello_che_non_si_vede_e_un_avviso_e_va_chiesto() {
    // L'accento di serie sul nero si vede; l'anello grigio scuro che la scheda
    // si disegna da sé, sullo stesso nero, no.
    let skin = parse_skin_json(&minima(
        r##", "tokens": { "color.accent": "#8b7cf6", "color.surface.0": "#000000" },
             "parts": { "card": { "focusRing": { "color": "#222222", "width": "2px" } } }"##,
    ))
    .expect("valida");
    let anelli = focus_ring_pairs(&skin);
    let di_serie = anelli
        .iter()
        .find(|a| a.path == "tokens.color.accent")
        .expect("l'anello di serie si misura");
    assert!(di_serie.passa(), "{di_serie:?}");
    let della_scheda = anelli
        .iter()
        .find(|a| a.path == "parts.card.focusRing")
        .expect("l'anello della parte si misura");
    assert!(!della_scheda.passa(), "{della_scheda:?}");
    assert_eq!(della_scheda.background, "color.surface.0");

    let avvisi: Vec<SkinWarning> = check_skin(&skin)
        .into_iter()
        .filter(|a| a.path == "parts.card.focusRing")
        .collect();
    assert_eq!(avvisi.len(), 1, "{avvisi:?}");
    assert_eq!(avvisi[0].kind, WarningKind::Contrast);
    assert!(avvisi[0].message.contains("3:1"), "{}", avvisi[0].message);

    // Un avviso e una domanda, non un rifiuto: la skin è valida, e l'esame lo dice.
    let esame = esamina(&skin);
    assert_eq!(esame.anelli, 1);
    assert!(esame.da_confermare());
}

#[test]
fn un_anello_acceso_non_avvisa_e_non_chiede() {
    let skin = parse_skin_json(&minima(
        r##", "tokens": { "color.accent": "#8b7cf6", "color.surface.0": "#000000" },
             "parts": { "card": { "focusRing": { "color": "#ffffff", "width": "2px" } } }"##,
    ))
    .expect("valida");
    assert!(focus_ring_pairs(&skin).iter().all(AnelloMisurato::passa));
    assert!(
        !check_skin(&skin)
            .iter()
            .any(|a| a.path.ends_with("focusRing"))
    );
    assert_eq!(esamina(&skin).anelli, 0);
}

#[test]
fn l_anello_si_misura_negli_stati_e_in_ogni_tema() {
    // Un anello quasi bianco nello stato del fuoco: sul nero del tema scuro si
    // vede, sul bianco della variante chiara sparisce. L'avviso è uno, e dice
    // il tema in cui sparisce.
    let skin = parse_skin_json(&minima(
        r##", "tokens": { "color.accent": "#8b7cf6", "color.surface.0": "#000000" },
             "variant": { "tokens": { "color.surface.0": "#ffffff", "color.accent": "#5b4bd6" } },
             "parts": { "card": { "states": { "focus": {
               "focusRing": { "color": "#f4f4f4", "width": "2px" } } } } }"##,
    ))
    .expect("valida");
    let suoi: Vec<AnelloMisurato> = focus_ring_pairs(&skin)
        .into_iter()
        .filter(|a| a.path == "parts.card.states.focus.focusRing")
        .collect();
    assert_eq!(suoi.len(), 2, "uno per tema: {suoi:?}");
    assert!(suoi.iter().any(|a| a.tema == "scuro" && a.passa()));
    assert!(suoi.iter().any(|a| a.tema == "chiaro" && !a.passa()));
    let avvisi: Vec<SkinWarning> = check_skin(&skin)
        .into_iter()
        .filter(|a| a.path == "parts.card.states.focus.focusRing")
        .collect();
    assert_eq!(avvisi.len(), 1, "{avvisi:?}");
    assert!(
        avvisi[0].message.contains("chiaro"),
        "{}",
        avvisi[0].message
    );
}

#[test]
fn un_accento_spento_e_anche_un_anello_che_non_si_vede() {
    // L'anello di serie è l'accento: un accento quasi nero sul nero non regge
    // nemmeno i tre a uno.
    let skin = parse_skin_json(&minima(
        r##", "tokens": { "color.accent": "#1a1a1a", "color.surface.0": "#000000" }"##,
    ))
    .expect("valida");
    assert!(
        check_skin(&skin)
            .iter()
            .any(|a| a.path == "tokens.color.accent" && a.message.contains("anello")),
        "{:?}",
        check_skin(&skin)
    );
    assert_eq!(esamina(&skin).anelli, 1);
}
