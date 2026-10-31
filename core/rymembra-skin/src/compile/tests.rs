//! Le prove del compilatore.

use super::*;
use crate::document::parse_skin_json;

const MINIMA: &str = r##"{
  "format": 2,
  "id": "prova",
  "meta": { "name": "Prova", "author": "Rymembra", "version": "1.0.0" },
  "tokens": {}
}"##;

fn con(extra: &str) -> String {
    MINIMA.replace("\"tokens\": {}", &format!("\"tokens\": {{}}{extra}"))
}

fn compila(tokens: &str) -> String {
    let json = MINIMA.replace("\"tokens\": {}", &format!("\"tokens\": {tokens}"));
    compile_skin(&parse_skin_json(&json).expect("valida")).css
}

fn compila_documento(json: &str) -> CompiledSkin {
    compile_skin(&parse_skin_json(json).expect("valida"))
}

fn anteprima(json: &str) -> CompiledSkin {
    compile_skin_anteprima(&parse_skin_json(json).expect("valida"))
}

/// Il corpo del primo blocco che si apre così, o una stringa vuota.
fn corpo(css: &str, apertura: &str) -> String {
    let Some(inizio) = css.find(&format!("{apertura} {{\n")) else {
        return String::new();
    };
    let dopo = css.get(inizio..).unwrap_or_default();
    let fine = dopo.find("\n}").unwrap_or(dopo.len());
    dopo.get(..fine).unwrap_or_default().to_owned()
}

#[test]
fn la_tripla_si_deriva_dal_colore() {
    let css = compila(r##"{ "color.accent": "#8b7cf6" }"##);
    assert!(css.contains("--accent: rgb(139 124 246);"), "{css}");
    assert!(css.contains("--accent-rgb: 139 124 246;"), "{css}");
}

#[test]
fn hero_esce_solo_come_tripla() {
    let css = compila(r##"{ "color.hero": "#8b7cf6" }"##);
    assert!(css.contains("--hero-rgb: 139 124 246;"), "{css}");
    assert!(!css.contains("--hero-rgb: rgb("), "{css}");
}

#[test]
fn il_color_scheme_segue_il_tema_di_base() {
    assert!(compila("{}").contains("color-scheme: dark;"));
    let chiara = compila_documento(&con(
        r##", "scheme": "light", "variant": { "tokens": { "color.accent": "#b9a8ff" } }"##,
    ))
    .css;
    assert!(
        corpo(&chiara, ":root[data-skin='prova']").contains("color-scheme: light;"),
        "{chiara}"
    );
    // La variante di una skin chiara è scura, e si accende con `dark`.
    assert!(
        corpo(&chiara, ":root[data-skin='prova'][data-theme='dark']")
            .contains("color-scheme: dark;"),
        "{chiara}"
    );
}

#[test]
fn niente_del_documento_finisce_nel_foglio_come_testo() {
    let compilata = compila_documento(
        r##"{
          "format": 2,
          "id": "prova",
          "meta": { "name": "Prova", "author": "Rymembra", "version": "1.0.0" },
          "palette": { "teal": "#00f0ff" },
          "fonts": { "Mia Sans": { "src": "mia.woff2" } },
          "tokens": {
            "color.accent": { "$palette": "teal", "alpha": 0.18 },
            "color.accent.like": { "$token": "color.accent" },
            "font.sans": ["Mia Sans", "Geist"]
          },
          "patterns": {
            "griglia": { "effect": "hairlineGrid", "color": { "$palette": "teal" }, "cell": "36px" },
            "grana": { "effect": "image", "src": "grana.webp", "repeat": true, "size": "240px" }
          },
          "parts": {
            "app-shell": { "background": [{ "$pattern": "griglia" }, { "$pattern": "grana" }] }
          }
        }"##,
    );
    for vietato in ["@import", "expression(", "javascript:", "</style"] {
        assert!(!compilata.css.contains(vietato), "«{vietato}» nel foglio");
    }
    // Gli unici `url()` puntano allo schema che serve i pacchetti, con l'id e
    // il nome della voce validati.
    for (indice, _) in compilata.css.match_indices("url(") {
        let dopo = compilata.css.get(indice..).unwrap_or_default();
        assert!(
            dopo.starts_with(&format!("url(\"{ORIGINE_RISORSE}/prova/")),
            "un url che non è del pacchetto: {dopo:.80}"
        );
    }
    assert!(
        compilata
            .css
            .contains("url(\"http://rymembra-skin.localhost/prova/mia.woff2\") format('woff2')"),
        "{}",
        compilata.css
    );
    // Il carattere ha un nome proprio della skin, e i token lo usano.
    assert!(
        compilata
            .css
            .contains("font-family: 'skin-prova-mia-sans';"),
        "{}",
        compilata.css
    );
    assert!(
        compilata
            .css
            .contains("--font-sans: 'skin-prova-mia-sans', 'Geist',"),
        "{}",
        compilata.css
    );
}

#[test]
fn gli_stati_non_scavalcano_i_componenti() {
    let css = compila_documento(&con(
        r##", "parts": { "list-row": { "states": { "current": { "fontWeight": 600 } } } }"##,
    ))
    .css;
    assert!(
        css.contains(":root[data-skin='prova'] .list-row:where([aria-current]:not([aria-current=\"false\"]), [data-active], .is-active) {"),
        "{css}"
    );
}

#[test]
fn l_anello_del_fuoco_sta_sotto_il_fuoco_e_segue_l_involucro() {
    let css = compila_documento(&con(r##", "parts": {
            "btn-accent": { "focusRing": { "color": "#fff", "width": "2px", "offset": "2px" } },
            "search-field": { "focusRing": { "color": "#fff", "width": "2px" } }
        }"##))
    .css;
    assert!(
        css.contains(":root[data-skin='prova'] .btn-accent:where(:focus-visible) {"),
        "{css}"
    );
    assert!(
        css.contains(
            ":root[data-skin='prova'] .search-field:where(:focus-visible, :has(:focus-visible)) {"
        ),
        "{css}"
    );
    assert!(
        css.contains("outline: 2px solid rgb(255 255 255);"),
        "{css}"
    );
}

#[test]
fn i_livelli_non_li_puo_spostare_la_skin_e_seguono_gli_angoli() {
    let css = compila_documento(&con(r##", "parts": { "section-card": {
            "before": { "background": [{ "effect": "solid", "color": "#000" }] },
            "after": { "background": [{ "effect": "solid", "color": "#fff" }] }
        } }"##))
    .css;
    let prima = corpo(&css, ":root[data-skin='prova'] .section-card::before");
    for attesa in [
        "position: absolute;",
        "inset: 0;",
        "pointer-events: none;",
        "border-radius: inherit;",
        "z-index: -1;",
    ] {
        assert!(prima.contains(attesa), "{attesa}: {prima}");
    }
    assert!(css.contains("isolation: isolate;"), "{css}");
    assert!(!corpo(&css, ":root[data-skin='prova'] .section-card::after").contains("z-index"));
}

#[test]
fn una_tinta_che_non_sta_in_fondo_diventa_un_gradiente() {
    // `background: rgb(…), linear-gradient(…)` non è CSS: il colore pieno
    // sta solo nell'ultimo strato, e il browser butterebbe il fondo intero.
    let css = compila_documento(&con(r##", "parts": { "np-screen": { "background": [
            { "effect": "solid", "color": "rgba(0, 0, 0, 0.2)" },
            { "effect": "vignette", "color": "#000000" },
            { "effect": "solid", "color": { "$token": "color.surface.0" } }
        ] } }"##))
    .css;
    assert!(
        css.contains(
            "background: linear-gradient(rgb(0 0 0 / 0.2), rgb(0 0 0 / 0.2)), radial-gradient("
        ),
        "{css}"
    );
    assert!(css.contains("100%), var(--color-surface-0);"), "{css}");
}

#[test]
fn due_compilazioni_danno_lo_stesso_foglio() {
    let plain = crate::plain().expect("plain");
    assert_eq!(compile_skin(&plain).css, compile_skin(&plain).css);
}

// ── Lo scafale ──────────────────────────────────────────────────────────────

#[test]
fn lo_scafale_esce_in_blocchi_propri_con_l_indirizzo_del_nodo() {
    let css = compile_skin(&crate::plain().expect("plain")).css;
    assert!(
        css.contains(":root[data-skin='plain'] [data-nodo='radice'] {"),
        "{css}"
    );
    assert!(
        css.contains(":root[data-skin='plain'] [data-nodo='1-1'] {"),
        "{css}"
    );
    assert!(css.contains("container-type: inline-size;"), "{css}");
}

// ── Il movimento ────────────────────────────────────────────────────────────

const ANIMAZIONI: &str = r##", "motion": { "intensity": "full",
    "easings": { "molla": { "kind": "cubicBezier", "points": [0.34, 1.56, 0.64, 1] } },
    "animations": {
      "sale": { "duration": "240ms", "easing": "molla", "delay": "40ms", "frames": [
        { "at": 0, "opacity": 0, "translateY": 8 }, { "at": 100, "opacity": 1, "translateY": 0 } ] },
      "sbuca": { "duration": "180ms", "frames": [{ "at": 0, "scale": 0.96 }, { "at": 100, "scale": 1 }] },
      "gira": { "duration": "2400ms", "easing": "linear", "iterations": "infinite", "fill": "none",
        "frames": [{ "at": 0, "rotate": 0 }, { "at": 100, "rotate": 360 }] }
    } }"##;

fn animata(parti: &str) -> CompiledSkin {
    compila_documento(&con(&format!("{ANIMAZIONI}, \"parts\": {parti}")))
}

#[test]
fn i_keyframes_portano_il_nome_della_skin_e_dell_animazione() {
    let css = animata("{}").css;
    assert!(css.contains("@keyframes skin-prova-anim-sale {"), "{css}");
    assert!(css.contains("transform: translateY(8px);"), "{css}");
    assert!(css.contains("--skin-ease-molla: cubic-bezier(0.34, 1.56, 0.64, 1);"));
}

#[test]
fn una_curva_richiamata_per_nome_diventa_la_sua_variabile() {
    let css = animata(r##"{ "section-card": { "motion": { "enter": "sale" } } }"##).css;
    assert!(
        css.contains("animation: skin-prova-anim-sale calc(240ms * var(--motion-scale, 1)) var(--skin-ease-molla)"),
        "{css}"
    );
}

#[test]
fn ogni_regola_che_tocca_animation_scrive_la_lista_intera() {
    // Il vinile gira sulla base; il passaggio del mouse aggiunge la sua, e la
    // regola dello stato deve ripetere il giro, o lo spegnerebbe.
    let css =
        animata(r##"{ "column-art": { "motion": { "playing": "gira", "hover": "sbuca" } } }"##).css;
    let base = corpo(&css, ":root[data-skin='prova'] .column-art");
    assert!(base.contains("skin-prova-anim-gira"), "{base}");
    assert!(
        base.contains("animation-play-state: var(--skin-suona, paused);"),
        "{base}"
    );
    let passaggio = corpo(&css, ":root[data-skin='prova'] .column-art:where(:hover)");
    assert!(passaggio.contains("skin-prova-anim-gira"), "{passaggio}");
    assert!(passaggio.contains("skin-prova-anim-sbuca"), "{passaggio}");
    assert!(
        passaggio.contains("animation-play-state: var(--skin-suona, paused), running;"),
        "{passaggio}"
    );
    // E lo stato che suona accende la variabile.
    assert!(
        corpo(
            &css,
            ":root[data-skin='prova'] .column-art:where([data-playing])"
        )
        .contains("--skin-suona: running;"),
        "{css}"
    );
}

#[test]
fn ogni_parte_animata_ha_le_sue_regole_d_arresto() {
    // È la regola che rende ammissibile un ciclo infinito: nessuna parte che
    // si muove resta senza la sua porta chiusa, in nessuno dei tre modi.
    let css = animata(
        r##"{ "column-art": { "motion": { "playing": "gira" } },
             "section-card": { "motion": { "enter": "sale" } } }"##,
    )
    .css;
    for parte in ["column-art", "section-card"] {
        assert!(
            css.contains(&format!(
                "@media (prefers-reduced-motion: reduce) {{\n:root[data-skin='prova'] .{parte} {{\n  animation: none !important;"
            )),
            "{parte}: {css}"
        );
        assert!(
            css.contains(&format!(
                ":root[data-skin='prova'][data-motion-utente='ridotto'] .{parte} {{\n  animation: none !important;"
            )),
            "{parte}"
        );
        assert!(
            css.contains(&format!(
                ":root[data-skin='prova'][data-motion='none'] .{parte} {{\n  animation: none !important;"
            )),
            "{parte}"
        );
    }
}

#[test]
fn la_cascata_legge_l_indice_e_si_ferma_a_dodici() {
    let css = animata(r##"{ "card": { "motion": { "enter": "sale", "stagger": "40ms" } } }"##).css;
    assert!(
        css.contains("calc((40ms + min(var(--indice, 0), 12) * 40ms) * var(--motion-scale, 1))"),
        "{css}"
    );
}

#[test]
fn una_riga_riciclata_non_rientra_mentre_si_scorre() {
    let css =
        animata(r##"{ "list-row": { "motion": { "enter": "sale", "hover": "sbuca" } } }"##).css;
    let scorsa = corpo(
        &css,
        ":root[data-skin='prova'] :where([data-scorso]) .list-row",
    );
    assert!(scorsa.contains("animation: none;"), "{css}");
    let scorsa_sopra = corpo(
        &css,
        ":root[data-skin='prova'] :where([data-scorso]) .list-row:where(:hover)",
    );
    assert!(
        scorsa_sopra.contains("skin-prova-anim-sbuca"),
        "{scorsa_sopra}"
    );
    assert!(
        !scorsa_sopra.contains("skin-prova-anim-sale"),
        "{scorsa_sopra}"
    );
}

#[test]
fn lo_scorrimento_usa_la_linea_della_vista() {
    let css = animata(r##"{ "card": { "motion": { "inView": "sale" } } }"##).css;
    let base = corpo(&css, ":root[data-skin='prova'] .card");
    assert!(base.contains("skin-prova-anim-sale auto"), "{base}");
    assert!(base.contains("animation-timeline: view();"), "{base}");
    assert!(
        base.contains("animation-range: entry 0% cover 30%;"),
        "{base}"
    );
}

#[test]
fn il_cambio_alterna_due_gemelli() {
    let css = animata(r##"{ "column-art": { "motion": { "change": "sbuca" } } }"##).css;
    assert!(
        css.contains("@keyframes skin-prova-anim-sbuca-a {"),
        "{css}"
    );
    assert!(
        css.contains("@keyframes skin-prova-anim-sbuca-b {"),
        "{css}"
    );
    assert!(
        corpo(
            &css,
            ":root[data-skin='prova'] .column-art:where([data-cambio='a'])"
        )
        .contains("skin-prova-anim-sbuca-a"),
        "{css}"
    );
}

#[test]
fn il_cambio_non_riparte_alla_fine_di_un_passaggio() {
    // Sopra una parte appena cambiata, il passaggio del mouse deve tenere il
    // gemello del cambio nella lista, allo stesso posto: toglierlo e
    // rimetterlo all'uscita lo faceva ripartire.
    let css =
        animata(r##"{ "column-art": { "motion": { "change": "sale", "hover": "sbuca" } } }"##).css;
    let selettore = ":root[data-skin='prova'] .column-art:where([data-cambio='a']):where(:hover)";
    let insieme = corpo(&css, selettore);
    let cambio = insieme.find("skin-prova-anim-sale-a");
    let passaggio = insieme.find("skin-prova-anim-sbuca ");
    assert!(
        cambio.is_some() && passaggio.is_some() && cambio < passaggio,
        "il gemello prima, lo stato dopo: {insieme}"
    );
    // E vince: viene dopo la regola dello stato, a pari specificità.
    let dove_stato = css
        .find(":root[data-skin='prova'] .column-art:where(:hover) {")
        .expect("la regola dello stato");
    let dove_insieme = css.find(selettore).expect("la regola d'insieme");
    assert!(dove_stato < dove_insieme, "{css}");
    // La regola del cambio da sola mette il gemello allo stesso posto.
    let solo = corpo(
        &css,
        ":root[data-skin='prova'] .column-art:where([data-cambio='a'])",
    );
    assert!(
        solo.contains("animation: skin-prova-anim-sale-a "),
        "{solo}"
    );
}

#[test]
fn la_pausa_del_ciclo_non_si_eredita() {
    // Il ciclo «finché suona» parte fermo per dichiarazione della parte
    // stessa, non per assenza: un genitore che suona non lo accende.
    let css = animata(r##"{ "column-art": { "motion": { "playing": "gira" } } }"##).css;
    let base = corpo(&css, ":root[data-skin='prova'] .column-art");
    assert!(base.contains("--skin-suona: paused;"), "{base}");
    let suona = css
        .find(":root[data-skin='prova'] .column-art:where([data-playing]) {")
        .expect("lo stato che suona");
    let dichiarata = css.find("--skin-suona: paused;").expect("la pausa");
    assert!(dichiarata < suona, "la pausa prima, lo stato dopo: {css}");
    // Una parte senza ciclo non dichiara niente: non c'è niente da fermare.
    let senza = animata(r##"{ "card": { "motion": { "hover": "sbuca" } } }"##).css;
    assert!(!senza.contains("--skin-suona"), "{senza}");
}

#[test]
fn le_presenze_escono_come_transizioni_di_vista_con_una_classe_per_innesco() {
    let compilata = animata(
        r##"{ "menu-pop": { "motion": { "enter": "sbuca", "exit": "sale" } },
             "card-cover": { "motion": { "share": "sale" } } }"##,
    );
    let css = &compilata.css;
    assert!(
        css.contains(":root[data-skin='prova']::view-transition-new(.vt-menu-pop-enter) {"),
        "{css}"
    );
    assert!(
        css.contains(":root[data-skin='prova']::view-transition-old(.vt-menu-pop-exit) {"),
        "{css}"
    );
    assert!(
        css.contains(":root[data-skin='prova']::view-transition-group(.vt-card-cover-share) {"),
        "{css}"
    );
    // L'entrata della presenza non è nella regola base: è una transizione.
    assert!(!corpo(css, ":root[data-skin='prova'] .menu-pop").contains("animation:"));
    // E spegne l'entrata di serie del motore.
    assert!(corpo(css, ":root[data-skin='prova'] .menu-pop").contains("transition: none;"));
    let menu = compilata
        .presenze
        .iter()
        .find(|p| p.parte == "menu-pop")
        .expect("la presenza");
    assert!(menu.enter && menu.exit && !menu.share);
    assert!(
        compilata
            .presenze
            .iter()
            .any(|p| p.parte == "card-cover" && p.share)
    );
}

#[test]
fn la_rotta_passa_dalla_scala_e_dalla_classe_della_pagina() {
    let css = compila_documento(&con(
        r##", "motion": { "route": { "duration": "320ms", "out": { "opacity": 0 }, "in": { "opacity": 0 } } }"##,
    ))
    .css;
    assert!(
        css.contains(":root[data-skin='prova']::view-transition-old(.vt-page-content-exit) {\n  animation: skin-prova-out calc(320ms * var(--motion-scale, 1)) var(--ease-out-expo) both;"),
        "{css}"
    );
    assert!(!css.contains("(root)"), "{css}");
}

/// I pezzi di un valore che stanno **fuori** da ogni `calc(…)`.
fn fuori_dai_calc(valore: &str) -> String {
    let mut fuori = String::new();
    let mut resto = valore;
    while let Some(inizio) = resto.find("calc(") {
        fuori.push_str(resto.get(..inizio).unwrap_or_default());
        let mut profondita = 0_i32;
        let mut fine = resto.len();
        for (indice, carattere) in resto.char_indices().skip(inizio) {
            match carattere {
                '(' => profondita += 1,
                ')' => {
                    profondita -= 1;
                    if profondita == 0 {
                        fine = indice + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        resto = resto.get(fine..).unwrap_or_default();
    }
    fuori.push_str(resto);
    fuori
}

#[test]
fn nessuna_durata_esce_da_qui_fuori_da_var_motion_scale() {
    // **La prova che difende la regola**, sull'output: qualunque ramo del
    // compilatore che emetta un tempo fuori da un `calc()` cade qui.
    let fogli = [
        compile_skin(&crate::plain().expect("plain")).css,
        animata(
            r##"{ "column-art": { "motion": { "playing": "gira", "hover": "sbuca", "change": "sale" } },
                 "card": { "motion": { "enter": "sale", "stagger": "30ms", "transition": { "duration": "200ms" } } },
                 "list-row": { "motion": { "enter": "sale" } },
                 "menu-pop": { "motion": { "enter": "sbuca", "exit": "sale" } },
                 "card-cover": { "motion": { "share": "sale" } } }"##,
        )
        .css,
    ];
    let mut viste = 0_usize;
    for css in &fogli {
        for riga in css.lines() {
            let riga = riga.trim();
            let Some((proprieta, valore)) = riga.split_once(": ") else {
                continue;
            };
            if !matches!(
                proprieta,
                "animation"
                    | "transition"
                    | "animation-duration"
                    | "animation-delay"
                    | "transition-duration"
            ) && !proprieta.starts_with("--transition")
            {
                continue;
            }
            viste += 1;
            let valore = valore.trim_end_matches(';');
            let nudo = fuori_dai_calc(valore);
            assert!(
                !nudo.contains("ms") && !nudo.contains("var(--dur"),
                "«{proprieta}: {valore}»: c'è una durata fuori dal calc"
            );
            assert!(
                !nudo
                    .as_bytes()
                    .windows(2)
                    .any(|c| c.first().is_some_and(u8::is_ascii_digit) && c.get(1) == Some(&b's')),
                "«{proprieta}: {valore}»: c'è una durata in secondi fuori dal calc"
            );
        }
    }
    assert!(viste >= 12, "solo {viste} dichiarazioni esaminate");
}

// ── L'anteprima ─────────────────────────────────────────────────────────────

#[test]
fn l_anteprima_parte_dal_riquadro_e_non_dalla_radice() {
    let css = anteprima(&con(r##", "parts": { "card": { "radius": "4px" } }"##)).css;
    assert!(
        css.contains(".anteprima[data-skin='prova'] .card {"),
        "{css}"
    );
    assert!(!css.contains(":root[data-skin='prova'] .card"), "{css}");
}

#[test]
fn l_anteprima_colma_i_token_che_la_bozza_non_dichiara() {
    let css = anteprima(MINIMA).css;
    let serie = corpo(&css, ":where(.anteprima[data-skin='prova'])");
    // Un token di Plain, e uno che nemmeno Plain dichiara.
    assert!(serie.contains("--color-surface-0: rgb(9 9 13);"), "{serie}");
    assert!(serie.contains("--ritmo-3: 14px;"), "{serie}");
    assert!(serie.contains("--motion-scale: 1;"), "{serie}");
    // E la variante chiara di serie, a specificità zero anche lei.
    assert!(
        corpo(
            &css,
            ":where(.anteprima[data-skin='prova'][data-theme='light'])"
        )
        .contains("color-scheme: light;"),
        "{css}"
    );
}

#[test]
fn nell_anteprima_gli_stati_si_possono_forzare() {
    let css = anteprima(&con(
        r##", "parts": { "btn-accent": { "states": { "hover": { "opacity": 0.8 } } } }"##,
    ))
    .css;
    assert!(
        css.contains(
            ".anteprima[data-skin='prova'] .btn-accent:where(:hover, [data-forza~='hover']) {"
        ),
        "{css}"
    );
}

#[test]
fn nell_anteprima_i_nomi_globali_hanno_un_suffisso_e_le_viste_non_ci_sono() {
    let compilata = compile_skin_anteprima(
        &parse_skin_json(&con(&format!(
            "{ANIMAZIONI}, \"parts\": {{ \"menu-pop\": {{ \"motion\": {{ \"enter\": \"sbuca\" }} }} }}"
        )))
        .expect("valida"),
    );
    assert!(
        compilata
            .css
            .contains("@keyframes skin-prova-anim-sale-anteprima {"),
        "{}",
        compilata.css
    );
    assert!(
        !compilata.css.contains("view-transition"),
        "{}",
        compilata.css
    );
    assert!(compilata.presenze.is_empty());
    // Le regole d'arresto si compongono dall'ambito.
    assert!(
        compilata.css.contains(
            ":root[data-motion-utente='ridotto'] .anteprima[data-skin='prova'] .menu-pop {"
        ),
        "{}",
        compilata.css
    );
}

#[test]
fn il_foglio_della_finestra_non_ha_il_blocco_di_serie() {
    // Il blocco di serie è solo dell'anteprima: nella finestra lo colma
    // `stile.css`, e il blocco di base resta quello confrontato riga per riga.
    let css = compile_skin(&parse_skin_json(MINIMA).expect("valida")).css;
    assert!(!css.contains(":where(:root"), "{css}");
    assert!(!css.contains(":where(.anteprima"), "{css}");
}

// ── La variante ─────────────────────────────────────────────────────────────

#[test]
fn la_variante_ridipinge_tavolozza_motivi_e_parti() {
    let css = compila_documento(&con(
        r##", "palette": { "carta": "#f4efe6" },
             "patterns": { "grana": { "effect": "dotGrid", "color": { "$palette": "carta" }, "spacing": "4px", "dot": "1px" } },
             "parts": { "app-shell": { "background": [{ "$pattern": "grana" }] } },
             "variant": {
               "palette": { "carta": "#1a1712" },
               "patterns": { "grana": { "effect": "dotGrid", "color": "#000", "spacing": "6px", "dot": "1px" } },
               "parts": { "card": { "radius": "2px" } }
             }"##,
    ))
    .css;
    let variante = corpo(&css, ":root[data-skin='prova'][data-theme='light']");
    assert!(
        variante.contains("--skin-color-carta: rgb(26 23 18);"),
        "{variante}"
    );
    assert!(
        variante.contains("--skin-grana: radial-gradient("),
        "{variante}"
    );
    assert!(
        css.contains(":root[data-skin='prova'][data-theme='light'] .card {\n  border-radius: 2px;"),
        "{css}"
    );
}

// ── Un documento ostile non cambia la forma del foglio ──────────────────────
//
// Le skin si scambiano: chi ne indossa una sta eseguendo il documento di uno
// sconosciuto. Le prove qui sotto guardano il foglio **emesso**, non il codice
// che lo scrive, per la stessa ragione della prova sulle durate: una regola
// difesa solo dal codice cade al primo ramo nuovo che la dimentica.

/// Le at-rule che il compilatore scrive, e nessun'altra.
///
/// Derivate dal codice: `@font-face` per i caratteri del pacchetto,
/// `@keyframes` per le animazioni e per la rotta, `@media` per le regole
/// d'arresto sotto `prefers-reduced-motion`. Un'at-rule che non sta qui è un
/// ramo nuovo del compilatore — e allora va aggiunta con la sua ragione — o un
/// pezzo di documento che ha trovato la strada del foglio.
const AT_RULE_NOTE: &[&str] = &["@font-face", "@keyframes", "@media"];

/// Le origini che un `url()` del foglio può nominare.
const ORIGINI_AMMESSE: &[&str] = &["\"http://rymembra-skin.localhost/", "\"data:"];

/// Il carattere può stare fra apici nel foglio?
///
/// Fra apici il compilatore scrive valori di attributo (`[data-skin='prova']`),
/// famiglie di caratteri, indirizzi del protocollo delle skin e `content: ""`.
/// Niente di tutto questo ha bisogno di un apice, di una barra inversa o di un
/// a capo, che sono i tre modi di uscire da una stringa CSS.
fn sta_fra_apici(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | ':' | '/')
}

/// Il nome di una proprietà, o di una variabile.
fn e_una_proprieta(nome: &str) -> bool {
    let corpo = nome.strip_prefix("--").unwrap_or(nome);
    corpo.starts_with(|c: char| c.is_ascii_alphabetic())
        && corpo.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Un selettore di fotogramma dentro `@keyframes`.
fn e_un_fotogramma(prelude: &str) -> bool {
    prelude == "from"
        || prelude == "to"
        || prelude
            .strip_suffix('%')
            .is_some_and(|n| n.parse::<f64>().is_ok())
}

/// Che cosa si sta aprendo, per sapere cosa ci può stare dentro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocco {
    /// `@media`: dentro, solo regole.
    Media,
    /// `@keyframes`: dentro, solo fotogrammi.
    Fotogrammi,
    /// Una regola, un fotogramma o `@font-face`: dentro, solo dichiarazioni.
    Dichiarazioni,
}

/// Legge un foglio emesso come lo leggerebbe il motore — commenti, stringhe,
/// blocchi, dichiarazioni — quanto basta a dire che nessun pezzo di documento
/// ne ha cambiato la forma.
///
/// È un lettore **severo**: pretende la forma che il compilatore scrive, non
/// quella che il CSS ammette. Un commento solo, in testa. Graffe bilanciate.
/// Solo le at-rule di [`AT_RULE_NOTE`]. Stringhe su una riga, con dentro
/// soltanto [`sta_fra_apici`]. Ogni dichiarazione chiusa dal suo `;`, con un
/// nome di proprietà per nome. Nessun `<`, nessuna barra inversa, niente fuori
/// dall'ASCII dopo l'intestazione. `url(` solo verso [`ORIGINI_AMMESSE`].
fn foglio_sano(css: &str) -> Result<(), String> {
    let (testa, corpo) = css
        .split_once('\n')
        .ok_or_else(|| "il foglio non ha nemmeno una riga".to_owned())?;
    let dentro = testa
        .strip_prefix("/* skin ")
        .and_then(|t| t.strip_suffix(" — generato, non modificare a mano */"))
        .ok_or_else(|| format!("intestazione inattesa: {testa}"))?;
    let (id, versione) = dentro
        .split_once(' ')
        .ok_or_else(|| format!("intestazione senza versione: {testa}"))?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!(
            "l'id dell'intestazione esce dal suo alfabeto: {id}"
        ));
    }
    if !versione.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(format!(
            "la versione dell'intestazione esce dal suo alfabeto: {versione}"
        ));
    }

    for vietato in [
        "/*",
        "*/",
        "<",
        "\\",
        "@import",
        "expression(",
        "javascript:",
    ] {
        if corpo.contains(vietato) {
            return Err(format!("«{vietato}» nel foglio, dopo l'intestazione"));
        }
    }
    if let Some(c) = corpo
        .chars()
        .find(|c| !(c.is_ascii_graphic() || *c == ' ' || *c == '\n'))
    {
        return Err(format!("carattere fuori posto nel foglio: {c:?}"));
    }
    for (indice, _) in corpo.match_indices("url(") {
        let dopo = corpo.get(indice + 4..).unwrap_or_default();
        if !ORIGINI_AMMESSE.iter().any(|o| dopo.starts_with(o)) {
            return Err(format!("un url() che non è del pacchetto: {dopo:.60}"));
        }
    }

    let mut pila: Vec<Blocco> = Vec::new();
    let mut attesa = String::new();
    let mut caratteri = corpo.chars();
    while let Some(c) = caratteri.next() {
        match c {
            '\'' | '"' => {
                attesa.push(c);
                loop {
                    let Some(d) = caratteri.next() else {
                        return Err("una stringa che non si chiude".to_owned());
                    };
                    attesa.push(d);
                    if d == c {
                        break;
                    }
                    if !sta_fra_apici(d) {
                        return Err(format!("{d:?} dentro una stringa: {attesa}"));
                    }
                }
            }
            '{' => {
                let prelude = attesa.trim().to_owned();
                attesa.clear();
                if prelude.is_empty() {
                    return Err("un blocco senza selettore".to_owned());
                }
                let nuovo = if prelude.starts_with('@') {
                    let nome = prelude
                        .split(|c: char| c.is_whitespace() || c == '(')
                        .next()
                        .unwrap_or_default();
                    if !AT_RULE_NOTE.contains(&nome) {
                        return Err(format!("at-rule sconosciuta: {prelude}"));
                    }
                    if pila.last() == Some(&Blocco::Dichiarazioni) {
                        return Err(format!("at-rule dentro una regola: {prelude}"));
                    }
                    match nome {
                        "@media" => Blocco::Media,
                        "@keyframes" => Blocco::Fotogrammi,
                        _ => Blocco::Dichiarazioni,
                    }
                } else {
                    match pila.last() {
                        Some(Blocco::Dichiarazioni) => {
                            return Err(format!("una regola dentro una regola: {prelude}"));
                        }
                        Some(Blocco::Fotogrammi) if !e_un_fotogramma(&prelude) => {
                            return Err(format!(
                                "un fotogramma che non è un fotogramma: {prelude}"
                            ));
                        }
                        _ => Blocco::Dichiarazioni,
                    }
                };
                pila.push(nuovo);
            }
            ';' => {
                let dichiarazione = attesa.trim().to_owned();
                attesa.clear();
                if pila.last() != Some(&Blocco::Dichiarazioni) {
                    return Err(format!(
                        "una dichiarazione fuori da una regola: {dichiarazione}"
                    ));
                }
                let Some((nome, valore)) = dichiarazione.split_once(':') else {
                    return Err(format!("una dichiarazione senza valore: {dichiarazione}"));
                };
                if !e_una_proprieta(nome.trim()) || valore.trim().is_empty() {
                    return Err(format!("dichiarazione malformata: {dichiarazione}"));
                }
            }
            '}' => {
                if !attesa.trim().is_empty() {
                    return Err(format!("testo prima di una graffa chiusa: {attesa}"));
                }
                attesa.clear();
                if pila.pop().is_none() {
                    return Err("una graffa chiusa di troppo".to_owned());
                }
            }
            altro => attesa.push(altro),
        }
    }
    if !pila.is_empty() {
        return Err(format!("{} blocchi che non si chiudono", pila.len()));
    }
    if !attesa.trim().is_empty() {
        return Err(format!("testo in coda al foglio: {attesa}"));
    }
    Ok(())
}

/// Un documento che usa ogni campo di stringa che il formato conosce: nomi
/// della tavolozza, dei motivi, delle curve, delle animazioni e dei prefab,
/// caratteri e immagini del pacchetto, la variante, il telefono, la rotta.
const COMPLETA: &str = r##"{
  "format": 2,
  "id": "completa",
  "meta": {
    "name": "Completa",
    "author": "Rymembra",
    "version": "2.1.0",
    "description": "Ogni campo di stringa che il formato conosce.",
    "license": "MIT",
    "basedOn": "plain",
    "preview": ["#101014", "#1c1c24", "#8b7cf6"]
  },
  "scheme": "dark",
  "capabilities": { "dynamicAccent": true },
  "fonts": {
    "Mia Sans": { "src": "mia-sans.woff2", "weight": [300, 700] },
    "Mia_Mono": { "src": "mia_mono.woff2", "style": "italic" }
  },
  "palette": { "carta": "#f4efe6", "teal": "#00f0ff" },
  "patterns": {
    "griglia": { "effect": "hairlineGrid", "color": { "$palette": "teal", "alpha": 0.1 }, "cell": "36px" },
    "grana": { "effect": "image", "src": "grana.webp", "repeat": true, "size": "240px" },
    "taglio": { "effect": "chamfer", "size": "8px", "corners": ["topLeft", "bottomRight"] }
  },
  "tokens": {
    "color.accent": { "$palette": "teal" },
    "color.accent.like": { "$token": "color.accent" },
    "font.sans": ["Mia Sans", "Geist"],
    "font.mono": ["Mia_Mono", "Geist Mono"]
  },
  "variant": {
    "tokens": { "color.surface.0": "#f4efe6" },
    "palette": { "carta": "#1a1712" },
    "patterns": { "griglia": { "effect": "hairlineGrid", "color": "#000000", "cell": "40px" } },
    "parts": { "card": { "radius": "2px" } }
  },
  "platforms": { "mobile": { "layout.rail": "56px" } },
  "motion": {
    "intensity": "full",
    "easings": { "molla": { "kind": "cubicBezier", "points": [0.34, 1.56, 0.64, 1] } },
    "route": { "duration": "320ms", "easing": "molla", "out": { "opacity": 0 }, "in": { "opacity": 0, "translateY": 8 } },
    "animations": {
      "sale": { "duration": "240ms", "easing": "molla", "frames": [
        { "at": 0, "opacity": 0, "translateY": 8 }, { "at": 100, "opacity": 1, "translateY": 0 } ] },
      "gira": { "duration": "2400ms", "easing": "linear", "iterations": "infinite", "fill": "none",
        "frames": [{ "at": 0, "rotate": 0 }, { "at": 100, "rotate": 360 }] },
      "sbuca": { "duration": "180ms", "origin": "top left", "frames": [
        { "at": 0, "scale": 0.96 }, { "at": 100, "scale": 1 } ] }
    }
  },
  "layout": {
    "density": "compact",
    "prefabs": {
      "centro": { "zone": "column", "children": [{ "widget": "page-header" }, { "widget": "content" }] }
    },
    "shell": {
      "zone": "row",
      "part": "app-shell",
      "children": [
        { "widget": "navigation", "options": { "wide": true } },
        { "prefab": "centro" },
        { "widget": "column" },
        { "widget": "player" },
        { "widget": "queue", "size": "hug" },
        { "widget": "selection-bar" }
      ]
    }
  },
  "parts": {
    "app-shell": { "background": [{ "$pattern": "griglia" }, { "$pattern": "grana" }] },
    "card": { "clip": { "$pattern": "taglio" }, "motion": { "enter": "sale", "stagger": "30ms" } },
    "column-art": { "motion": { "playing": "gira", "hover": "sbuca" } },
    "menu-pop": { "motion": { "enter": "sbuca", "exit": "sale" } },
    "section-card": {
      "before": { "background": [{ "effect": "solid", "color": { "$palette": "teal", "alpha": 0.2 } }] }
    }
  }
}"##;

/// I documenti su cui si prova: le tre skin di serie, e quella che usa tutto.
fn documenti_veri() -> Vec<(&'static str, &'static str)> {
    let mut tutti: Vec<(&str, &str)> = crate::DI_SERIE.to_vec();
    tutti.push(("completa", COMPLETA));
    tutti
}

/// I quattro ambiti in cui un documento si compila.
fn ambiti(id: &str) -> [Ambito; 4] {
    [
        Ambito::app(id),
        Ambito::prova(id),
        Ambito::tela(id),
        Ambito::anteprima(id),
    ]
}

#[test]
fn il_foglio_delle_skin_vere_e_sano_in_ogni_ambito() {
    for (nome, testo) in documenti_veri() {
        let skin = crate::leggi_skin(testo)
            .unwrap_or_else(|problemi| panic!("{nome} non è valida: {problemi:?}"));
        for ambito in ambiti(&skin.id) {
            let css = super::compila(&skin, &ambito).css;
            if let Err(guasto) = foglio_sano(&css) {
                panic!("{nome}, {ambito:?}: {guasto}\n{css}");
            }
        }
    }
}

/// Le dichiarazioni di un foglio che nominano sé stesse: `--x: …var(--x)…`.
///
/// Una proprietà personalizzata che rimanda a sé stessa è un ciclo, e al
/// valore calcolato non vale niente — né il suo valore né quello di serie di
/// `stile.css`, perché la dichiarazione della skin vince la cascata.
fn cicli(css: &str) -> Vec<String> {
    css.lines()
        .map(str::trim)
        .filter_map(|riga| {
            let (nome, valore) = riga.strip_suffix(';')?.split_once(':')?;
            let nome = nome.trim();
            (nome.starts_with("--") && valore.contains(&format!("var({nome})")))
                .then(|| riga.to_owned())
        })
        .collect()
}

#[test]
fn nessuna_variabile_si_riferisce_a_se_stessa() {
    // Neon e Vinile scrivono l'accento con la tavolozza: fino alla 3.2.0 di prova la sua
    // tripla diventava `--accent-rgb: var(--accent-rgb)`, e con lei cadevano
    // `--hero-rgb`, l'alone senza copertina e l'anello del giro guidato.
    for (nome, testo) in documenti_veri() {
        let skin = crate::leggi_skin(testo)
            .unwrap_or_else(|problemi| panic!("{nome} non è valida: {problemi:?}"));
        for ambito in ambiti(&skin.id) {
            let css = super::compila(&skin, &ambito).css;
            let trovati = cicli(&css);
            assert!(trovati.is_empty(), "{nome}, {ambito:?}: {trovati:#?}");
        }
    }
}

#[test]
fn la_tripla_della_tavolozza_segue_la_tavolozza() {
    let css = compila_documento(&con(
        r##", "palette": { "magenta": "#ff2bd6", "ciano": "#00f0ff" }"##,
    ))
    .css;
    assert!(
        css.contains("--skin-color-magenta-rgb: 255 43 214;"),
        "{css}"
    );
    let css = compila_documento(&MINIMA.replace(
        "\"tokens\": {}",
        r##""palette": { "magenta": "#ff2bd6", "ciano": "#00f0ff" },
                "tokens": { "color.accent": { "$palette": "ciano" },
                            "color.hero": { "$palette": "magenta" } }"##,
    ))
    .css;
    assert!(
        css.contains("--accent-rgb: var(--skin-color-ciano-rgb);"),
        "{css}"
    );
    assert!(
        css.contains("--hero-rgb: var(--skin-color-magenta-rgb);"),
        "{css}"
    );
}

#[test]
fn la_tripla_di_un_riferimento_segue_il_riferimento() {
    // Un token senza tripla si segue fino al colore: un letterale dà la sua
    // tripla, un colore della tavolozza la variabile della tavolozza, che la
    // variante può ridefinire.
    let css =
        compila(r##"{ "color.danger": "#ff0000", "color.accent": { "$token": "color.danger" } }"##);
    assert!(css.contains("--accent: var(--danger);"), "{css}");
    assert!(css.contains("--accent-rgb: 255 0 0;"), "{css}");

    // Una tripla che esiste si richiama, com'è sempre stato: `plain`.
    let css =
        compila(r##"{ "color.accent": "#8b7cf6", "color.hero": { "$token": "color.accent" } }"##);
    assert!(css.contains("--hero-rgb: var(--accent-rgb);"), "{css}");

    // Nella variante si segue prima la variante e poi la base.
    let css = compila_documento(&MINIMA.replace(
        "\"tokens\": {}",
        r##""tokens": { "color.danger": "#ff0000", "color.accent": { "$token": "color.danger" } },
            "variant": { "tokens": { "color.danger": "#00ff00" } }"##,
    ))
    .css;
    let chiaro = corpo(&css, ":root[data-skin='prova'][data-theme='light']");
    assert!(chiaro.contains("--accent-rgb: 0 255 0;"), "{css}");
}

#[test]
fn hero_richiamato_in_un_colore_si_scrive_come_colore() {
    // `--hero-rgb` è una tripla: `var(--hero-rgb)` dove ci va un colore non è
    // un colore, e il fondo cadeva senza un avviso.
    let css = compila_documento(&con(
        r##", "parts": { "card": { "background": [{ "effect": "solid", "color": { "$token": "color.hero" } }],
                                    "textColor": { "$token": "color.hero" } } }"##,
    ))
    .css;
    assert!(css.contains("background: rgb(var(--hero-rgb));"), "{css}");
    assert!(css.contains("color: rgb(var(--hero-rgb));"), "{css}");
    assert!(!css.contains(": var(--hero-rgb);"), "{css}");
}

#[test]
fn un_accento_che_rimanda_a_se_stesso_non_esce() {
    // La validazione rifiuta `$source` sull'accento; un documento costruito a
    // mano passa lo stesso dal compilatore, e il ciclo non deve uscire.
    let mut skin = parse_skin_json(MINIMA).expect("valida");
    let accento = crate::tokens::token("color.accent").expect("nel registro");
    skin.tokens = TokenSet::new(vec![(
        accento,
        TokenValue::Color(ColorValue::Source {
            source: crate::tokens::DynamicSource::Vibrant,
            alpha: None,
        }),
    )]);
    let css = compile_skin(&skin).css;
    assert!(cicli(&css).is_empty(), "{css}");
}

/// I blocchi di un foglio: selettore e corpo.
fn blocchi(css: &str) -> Vec<(String, String)> {
    css.split("}\n")
        .filter_map(|pezzo| {
            let (selettore, corpo) = pezzo.split_once(" {\n")?;
            Some((selettore.trim().to_owned(), corpo.to_owned()))
        })
        .collect()
}

#[test]
fn l_anello_di_uno_stato_sta_sotto_il_fuoco() {
    // Un `focusRing` scritto in `current` finiva nella regola di `current`: la
    // riga in ascolto portava sempre un anello, anche senza fuoco, e chi
    // naviga con la tastiera ne vedeva due senza sapere quale fosse il fuoco.
    let json = con(r##", "parts": { "list-row": {
              "focusRing": { "color": "#ffffff", "width": "2px" },
              "states": {
                "current": { "focusRing": { "color": "#ffcc00", "width": "3px" } },
                "focus": { "focusRing": { "color": "#00ffcc", "width": "2px" } }
              } } }"##);
    let skin = parse_skin_json(&json).expect("valida");
    for ambito in ambiti(&skin.id) {
        let css = super::compila(&skin, &ambito).css;
        for (selettore, corpo) in blocchi(&css) {
            if corpo.contains("outline:") {
                assert!(
                    selettore.contains(":focus-visible"),
                    "{ambito:?}: «{selettore}» disegna un anello senza fuoco"
                );
            }
        }
        // L'anello di `current` c'è ancora: sotto `current` **e** il fuoco.
        assert!(css.contains("3px solid rgb(255 204 0)"), "{css}");
    }
    // Nell'anteprima i due stati forzati si combinano: `current` e il fuoco
    // forzati insieme accendono l'anello, uno solo no.
    let css = super::compila(&skin, &Ambito::anteprima(&skin.id)).css;
    assert!(
        css.contains("[data-forza~='current']):where(:focus-visible, [data-forza~='focus'])"),
        "{css}"
    );
}

#[test]
fn il_lettore_del_foglio_non_e_di_manica_larga() {
    // Un controllo che non boccia niente non controlla niente: qui ogni
    // guasto che la prova deve saper vedere, a cominciare da quello vero.
    let buono = compile_skin(&crate::plain().expect("plain")).css;
    assert_eq!(foglio_sano(&buono), Ok(()));
    let dopo = buono.split_once('\n').map(|(_, c)| c).unwrap_or_default();
    for guasto in [
        // Il nome nell'intestazione, com'era fino alla 3.0.
        format!(
            "/* x */ :root{{display:none}} /* 1.0.0 — generato, non modificare a mano */\n{dopo}"
        ),
        format!("{buono}@import url(\"https://esempio.invalid/x.css\");\n"),
        format!("{buono}:root {{ background: url(\"https://esempio.invalid/x.png\"); }}\n"),
        format!("{buono}:root {{\n"),
        format!("{buono}}}\n"),
        format!("{buono}/* un commento in più */\n"),
        format!("{buono}:root {{ --x: '</style>'; }}\n"),
        format!("{buono}@supports (display: grid) {{ :root {{ --x: 1; }} }}\n"),
        format!("{buono}:root {{ --x: 'a\\'b'; }}\n"),
        format!("{buono}:root {{ color red; }}\n"),
    ] {
        assert!(foglio_sano(&guasto).is_err(), "passato: {guasto:.200}");
    }
}

/// Un passo di un percorso dentro il JSON.
#[derive(Debug, Clone)]
enum Passo {
    Chiave(String),
    Indice(usize),
}

/// Un posto del documento in cui c'è una stringa: un valore, o una chiave.
#[derive(Debug, Clone)]
struct Posto {
    via: Vec<Passo>,
    chiave: bool,
}

impl Posto {
    fn percorso(&self) -> String {
        self.via
            .iter()
            .map(|p| match p {
                Passo::Chiave(k) => k.clone(),
                Passo::Indice(i) => i.to_string(),
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// Ogni stringa del documento, chiavi comprese.
fn posti(valore: &serde_json::Value, via: &mut Vec<Passo>, fuori: &mut Vec<Posto>) {
    match valore {
        serde_json::Value::String(_) => fuori.push(Posto {
            via: via.clone(),
            chiave: false,
        }),
        serde_json::Value::Array(voci) => {
            for (indice, voce) in voci.iter().enumerate() {
                via.push(Passo::Indice(indice));
                posti(voce, via, fuori);
                via.pop();
            }
        }
        serde_json::Value::Object(mappa) => {
            for (chiave, voce) in mappa {
                via.push(Passo::Chiave(chiave.clone()));
                fuori.push(Posto {
                    via: via.clone(),
                    chiave: true,
                });
                posti(voce, via, fuori);
                via.pop();
            }
        }
        _ => {}
    }
}

/// Il documento con il veleno nel posto dato: accodato alla stringa, o al
/// suo posto.
fn avvelena(radice: &serde_json::Value, posto: &Posto, veleno: &str, accoda: bool) -> String {
    let mut copia = radice.clone();
    let Some((ultimo, sopra)) = posto.via.split_last() else {
        return copia.to_string();
    };
    let mut qui = &mut copia;
    for passo in sopra {
        qui = match passo {
            Passo::Chiave(k) => qui.get_mut(k.as_str()),
            Passo::Indice(i) => qui.get_mut(*i),
        }
        .expect("il percorso esiste");
    }
    let nuovo = |vecchio: &str| {
        if accoda {
            format!("{vecchio}{veleno}")
        } else {
            veleno.to_owned()
        }
    };
    match (ultimo, posto.chiave) {
        (Passo::Chiave(k), true) => {
            let mappa = qui.as_object_mut().expect("un oggetto");
            let valore = mappa.remove(k.as_str()).expect("la chiave esiste");
            mappa.insert(nuovo(k), valore);
        }
        (Passo::Chiave(k), false) => {
            let dentro = qui.get_mut(k.as_str()).expect("il valore esiste");
            let vecchio = dentro.as_str().unwrap_or_default().to_owned();
            *dentro = serde_json::Value::String(nuovo(&vecchio));
        }
        (Passo::Indice(i), _) => {
            let dentro = qui.get_mut(*i).expect("la voce esiste");
            let vecchio = dentro.as_str().unwrap_or_default().to_owned();
            *dentro = serde_json::Value::String(nuovo(&vecchio));
        }
    }
    copia.to_string()
}

/// Quel che un autore ostile metterebbe in una stringa. Insieme contengono
/// `*/`, graffe, `<`, `;`, `@`, la barra inversa, i due apici e gli a capo.
const VELENI: &[&str] = &[
    "x */ :root{display:none} /*",
    "';}</style><script>alert(1)</script>",
    "\"\\\n\r;@import url(//esempio.invalid);{",
];

#[test]
fn un_veleno_in_qualunque_stringa_non_cambia_il_foglio_di_un_byte() {
    // Ogni stringa di ogni documento — valori **e** chiavi — riceve ogni
    // veleno, in coda e al posto del testo. Le vie d'uscita sono due soltanto:
    // la validazione lo rifiuta, oppure il documento passa e il foglio è
    // identico a quello di prima in tutti e quattro gli ambiti. Un documento
    // che passa e cambia il foglio vuol dire che una stringa dell'autore è
    // arrivata nel CSS.
    let mut provati = 0_usize;
    for (nome, testo) in documenti_veri() {
        let radice: serde_json::Value = serde_json::from_str(testo).expect("JSON");
        let skin = crate::leggi_skin(testo).expect("valida");
        let fogli: Vec<String> = ambiti(&skin.id)
            .iter()
            .map(|a| super::compila(&skin, a).css)
            .collect();
        let mut dove = Vec::new();
        posti(&radice, &mut Vec::new(), &mut dove);
        let mut entrati: Vec<String> = Vec::new();
        for posto in &dove {
            for veleno in VELENI {
                // Una chiave avvelenata è una chiave sconosciuta sia in coda sia
                // al posto del nome: basta una delle due.
                let modi: &[bool] = if posto.chiave {
                    &[true]
                } else {
                    &[true, false]
                };
                for &accoda in modi {
                    provati += 1;
                    let avvelenato = avvelena(&radice, posto, veleno, accoda);
                    let Ok(letta) = crate::leggi_skin(&avvelenato) else {
                        continue;
                    };
                    for (ambito, prima) in ambiti(&letta.id).iter().zip(&fogli) {
                        let dopo = super::compila(&letta, ambito).css;
                        assert!(
                            dopo == *prima,
                            "{nome}: «{veleno}» in {} ha cambiato il foglio ({ambito:?})",
                            posto.percorso()
                        );
                    }
                    entrati.push(posto.percorso());
                }
            }
        }
        // La prova non è vuota: il testo libero accetta il veleno — è testo
        // libero — e proprio lì il foglio non deve accorgersene.
        entrati.sort();
        entrati.dedup();
        for campo in [
            "meta.author",
            "meta.description",
            "meta.license",
            "meta.name",
        ] {
            assert!(
                entrati.iter().any(|e| e == campo),
                "{nome}: {campo} non ha mai accettato il veleno: {entrati:?}"
            );
        }
        // E niente fuori dal testo libero passa.
        for entrato in &entrati {
            assert!(
                matches!(
                    entrato.as_str(),
                    "meta.author" | "meta.description" | "meta.license" | "meta.name"
                ),
                "{nome}: {entrato} ha accettato un veleno"
            );
        }
    }
    assert!(provati > 5_000, "solo {provati} documenti provati");
}

#[test]
fn il_nome_della_skin_non_arriva_piu_nell_intestazione() {
    // Il guasto vero, come l'avrebbe scritto chi lo sfruttava.
    let json = MINIMA.replace(
        "\"name\": \"Prova\"",
        "\"name\": \"x */ :root{display:none} /*\"",
    );
    let css = compile_skin(&parse_skin_json(&json).expect("il nome è testo libero")).css;
    assert!(
        css.starts_with("/* skin prova 1.0.0 — generato, non modificare a mano */\n"),
        "{css}"
    );
    assert!(!css.contains("display:none"), "{css}");
    assert_eq!(css.matches("*/").count(), 1, "{css}");
    assert_eq!(foglio_sano(&css), Ok(()));
}

#[test]
fn un_documento_costruito_a_mano_non_esce_comunque_dai_suoi_nomi() {
    // I campi di `SkinDocument` sono pubblici: niente obbliga a passare dalla
    // validazione. Il compilatore riduce ogni nome al suo alfabeto nel punto in
    // cui lo scrive, e il foglio resta un foglio.
    let mut skin = crate::leggi_skin(COMPLETA).expect("valida");
    skin.id = "x'] *{display:none} [a='".to_owned();
    skin.meta.version = "1.0.0 */ @import url(//esempio.invalid); /*".to_owned();
    for (nome, _) in &mut skin.palette {
        nome.push_str(";} :root{display:none} .x{");
    }
    for (nome, _) in &mut skin.patterns {
        nome.push_str("\"</style>");
    }
    for font in &mut skin.fonts {
        font.family.push_str("'; } * { color: red; } x { a: '");
        font.src
            .push_str("\") , url(\"https://esempio.invalid/x.woff2");
    }
    if let Some(movimento) = skin.motion.as_mut() {
        for (nome, _) in &mut movimento.easings {
            nome.push_str(") } @import x; {");
        }
        for (nome, _) in &mut movimento.animations {
            nome.push_str(" { } @keyframes x");
        }
    }
    for ambito in ambiti(&skin.id) {
        let css = super::compila(&skin, &ambito).css;
        if let Err(guasto) = foglio_sano(&css) {
            panic!("{ambito:?}: {guasto}\n{css}");
        }
    }
}
