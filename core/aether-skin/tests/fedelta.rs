//! Il collaudo di fedeltà: la skin di riferimento contro il CSS che sostituisce.
//!
//! `plain` non è una skin nuova. È il blocco `:root` di `global.css` riga per
//! riga, riscritto come dati, e questo file confronta il foglio compilato con
//! quelle dichiarazioni. Serve perché un formato provato solo su casi costruiti
//! per riuscire è un formato di cui non si sa niente: convertire una skin vera è
//! ciò che ha fatto emergere la lunghezza adattiva di `--content-x`,
//! `color-scheme`, e i riferimenti fra token.
//!
//! I valori a sinistra sono copiati da `global.css` senza toccarli. Dove il
//! confronto non è testuale è perché il compilatore scrive lo stesso valore in
//! un'altra notazione, e allora si confronta il valore — non la scrittura.
//!
//! # Dove la fedeltà finisce, e perché
//!
//! Tre cose non sono più quelle di `global.css`, e non per svista:
//!
//! - `--color-text-3` faceva 3,47:1 sul fondo, sotto la soglia di WCAG. Era un
//!   numero scritto a occhio in un file che nessuno poteva misurare.
//! - Il tema chiaro sovrascriveva otto token su quaranta e lasciava superfici e
//!   testo scuri, mentre `capabilities.light` diceva `true`.
//! - I caratteri: `global.css` diceva Inter e Cascadia Mono, `plain` dice Geist,
//!   Geist Mono e Bricolage Grotesque. È una scelta di disegno, non una
//!   conversione, e sta qui sotto in `i_caratteri_sono_quelli_impacchettati` —
//!   che è una prova più forte di quella di fedeltà, perché lega il token al
//!   file che l'applicazione spedisce davvero.
//! - Le ombre avevano un livello solo, che non basta a sollevare niente. La
//!   prova è `le_ombre_hanno_un_contatto_e_un_ambiente`, ed è dello stesso tipo
//!   di quella dei contrasti: la proprietà che il valore deve avere, invece del
//!   valore.
//!
//! Erano difetti trasportati insieme al resto, e questo file è il posto giusto
//! per dire che sono stati lasciati indietro apposta: la conversione doveva
//! essere fedele **al foglio**, non ai suoi errori. Al posto del valore fissato
//! ora c'è la proprietà che il valore deve avere, misurata con
//! `contrast_ratio()` — che è più forte, perché regge anche se le superfici
//! cambiano.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use aether_skin::values::{contrast_ratio, parse_color};
use aether_skin::{check_skin, compile_skin, plain};

/// La soglia di WCAG 2.1 per il testo normale.
const LEGGIBILE: f64 = 4.5;

/// Il valore di una proprietà dentro un blocco, o niente.
fn valore(css: &str, selettore: &str, proprieta: &str) -> Option<String> {
    let inizio = css.find(&format!("{selettore} {{"))?;
    let corpo = css.get(inizio..)?;
    let fine = corpo.find("\n}")?;
    let blocco = corpo.get(..fine)?;
    blocco
        .lines()
        .find_map(|riga| riga.trim_start().strip_prefix(&format!("{proprieta}: ")))
        .map(|resto| resto.trim_end_matches(';').to_owned())
}

const BASE: &str = ":root[data-skin='plain']";
const CHIARO: &str = ":root[data-skin='plain'][data-theme='light']";

/// I colori di `global.css`, copiati com'erano scritti.
const COLORI: &[(&str, &str)] = &[
    ("--color-surface-0", "#09090d"),
    ("--color-surface-1", "#0e0e14"),
    ("--color-surface-2", "#16161f"),
    ("--color-surface-3", "#1e1e2a"),
    ("--color-text-1", "rgba(255, 255, 255, 0.92)"),
    ("--color-text-2", "rgba(255, 255, 255, 0.6)"),
    // `--color-text-3` non è qui: è l'unica dichiarazione di `global.css` che la
    // conversione **non** ha conservato. Il perché sta in
    // `il_terzo_livello_di_testo_non_e_piu_quello_di_global_css`.
    ("--accent", "#8b7cf6"),
    ("--accent-soft", "rgba(139, 124, 246, 0.16)"),
    ("--accent-glow", "rgba(139, 124, 246, 0.35)"),
    ("--danger", "#e5484d"),
    ("--danger-soft", "rgba(229, 72, 77, 0.14)"),
    ("--success", "#34d399"),
    ("--success-soft", "rgba(52, 211, 153, 0.14)"),
    ("--warning", "#facc15"),
    ("--warning-soft", "rgba(250, 204, 21, 0.14)"),
    ("--ambient-1", "rgba(139, 124, 246, 0.1)"),
    ("--ambient-2", "rgba(76, 60, 180, 0.06)"),
    ("--sidebar-bg", "rgba(255, 255, 255, 0.04)"),
    ("--hairline", "rgba(255, 255, 255, 0.07)"),
    ("--scrubber-rest", "rgba(255, 255, 255, 0.18)"),
];

/// Quel che il compilatore scrive esattamente come stava scritto a mano.
const IDENTICI: &[(&str, &str)] = &[
    // `--font-sans` stava qui, e non ci sta più: il carattere è cambiato apposta,
    // e la sua prova è `i_caratteri_sono_quelli_impacchettati`.
    ("--accent-rgb", "139 124 246"),
    ("--accent-like", "var(--accent)"),
    ("--hero-rgb", "var(--accent-rgb)"),
    ("--rail-w", "68px"),
    ("--rail-w-expanded", "240px"),
    ("--shell-left", "var(--rail-w)"),
    ("--player-h", "92px"),
    ("--player-gap", "14px"),
    (
        "--player-clearance",
        "calc(var(--player-h) + var(--player-gap) * 2)",
    ),
    ("--content-x", "clamp(16px, 3cqw, 48px)"),
    ("--radius-panel", "20px"),
    ("--radius-card", "14px"),
    ("--ease-out-expo", "cubic-bezier(0.16, 1, 0.3, 1)"),
    ("--ease-spring", "cubic-bezier(0.34, 1.56, 0.64, 1)"),
    ("--dur-1", "150ms"),
    ("--dur-2", "280ms"),
    ("--dur-3", "450ms"),
    ("--viz-primary", "var(--accent)"),
    // Le due triple `--viz-primary-rgb` e `--viz-secondary-rgb` stavano qui, e
    // non ci stanno più: non erano soltanto lette da nessuno, erano sbagliate.
    // Il compilatore sa derivare una tripla da un colore letterale e ripiega
    // sull'accento per tutto il resto, quindi la tripla di un colore preso
    // dalla tavolozza descriveva l'accento e non quel colore.
    // `--viz-secondary` stava qui a `var(--accent)`, ed era la prova che le due
    // `mix()` dello shader non facevano niente: adesso è un letterale apposta,
    // e la sua prova è che la scena ha due colori.
    ("--viz-glow", "20"),
    ("--scrubber-glow", "6"),
];

/// Le ombre non sono qui: sono la quarta divergenza voluta, e la loro prova è
/// [`le_ombre_hanno_un_contatto_e_un_ambiente`].

#[test]
fn plain_e_valida_e_non_ha_avvisi() {
    let skin = plain().expect("la skin di riferimento deve essere valida");
    // Nessun avviso: `plain` dichiara tutti i token obbligatori, ed è ciò che la
    // rende il riferimento. Se un token nuovo nasce obbligatorio, questo test
    // fallisce prima che se ne accorga una skin di qualcun altro.
    let avvisi = check_skin(&skin);
    assert!(avvisi.is_empty(), "{avvisi:#?}");
}

#[test]
fn i_colori_sono_quelli_di_global_css() {
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;

    for (proprieta, scritto) in COLORI {
        let compilato = valore(&css, BASE, proprieta)
            .unwrap_or_else(|| panic!("{proprieta} non è nel foglio:\n{css}"));
        let atteso = parse_color(scritto).expect("il valore di global.css è un colore");
        let ottenuto = parse_color(&compilato)
            .unwrap_or_else(|| panic!("{proprieta} non compila a un colore: {compilato}"));
        // Si confronta il colore, non la scrittura: `#09090d` e `rgb(9 9 13)`
        // sono lo stesso colore, e pretendere la stessa stringa vorrebbe dire
        // provare la notazione invece del risultato.
        assert_eq!(ottenuto, atteso, "{proprieta}: {compilato} ≠ {scritto}");
    }
}

#[test]
fn i_valori_non_colore_sono_identici() {
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;

    for (proprieta, atteso) in IDENTICI {
        assert_eq!(
            valore(&css, BASE, proprieta).as_deref(),
            Some(*atteso),
            "{proprieta}"
        );
    }
}

#[test]
fn le_ombre_hanno_un_contatto_e_un_ambiente() {
    // In `global.css` ogni ombra aveva **un livello solo** — `--shadow-2` era
    // `0 8px 28px rgba(0,0,0,.45)` — e un livello solo non può dire le due cose
    // che un oggetto sopraelevato dice insieme: dove tocca (scostamento corto,
    // sfocatura corta, quasi opaca) e quanto sta in alto (scostamento lungo,
    // sfocatura larga, trasparente). Con uno solo si sceglie: o una macchia
    // sfocata che non poggia da nessuna parte, o un contorno duro che non
    // solleva. Erano quattro pannelli appoggiati su niente.
    //
    // Non si fissano i numeri, e non è pigrizia: fissarli rifarebbe qui il
    // difetto che tutto questo file esiste per togliere — un valore scritto a
    // occhio che nessuno può misurare. Si fissa la proprietà, che è quella che
    // il disegno chiede e che regge anche se i pixel cambiano.
    let skin = plain().expect("valida");
    let ombre = ["shadow.1", "shadow.2", "shadow.3", "shadow.player"];

    // E vale anche per la variante chiara: alla luce le ombre sono più tenui,
    // non più povere. Il tema chiaro le ridichiara tutte e quattro, quindi
    // tutte e quattro devono passare di qui.
    let chiaro = skin.light.as_ref().expect("plain ha la variante chiara");
    for insieme in [&skin.tokens, chiaro] {
        for id in ombre {
            let Some(aether_skin::tokens::TokenValue::Shadow(ombra)) = insieme.get(id) else {
                panic!("{id} non è un'ombra dichiarata");
            };
            assert!(
                ombra.layers.len() >= 2,
                "{id}: un livello solo non poggia e non solleva",
            );
            // Il primo livello è il contatto, l'ultimo l'ambiente: il secondo
            // deve essere il più sfocato, altrimenti l'ordine è invertito e
            // l'ombra si legge al contrario.
            let primo = ombra.layers.first().expect("almeno due");
            let ultimo = ombra.layers.last().expect("almeno due");
            assert!(
                ultimo.blur.value > primo.blur.value,
                "{id}: l'ambiente ({}) non è più largo del contatto ({})",
                ultimo.blur.value,
                primo.blur.value,
            );
        }
    }
}

#[test]
fn i_caratteri_sono_quelli_impacchettati() {
    // I tre token dei caratteri non si confrontano con `global.css` — Geist non
    // c'era — ma con i file che `stile.css` dichiara in `@font-face`. È il
    // legame che conta: un token che nomina una famiglia senza il `woff2`
    // accanto è la stessa dichiarazione a vuoto che Inter è stata per mesi, e si
    // nota solo su una macchina che quel carattere non ce l'ha installato.
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;
    for (proprieta, famiglia) in [
        ("--font-sans", "'Geist'"),
        ("--font-mono", "'Geist Mono'"),
        ("--font-dot", "'Bricolage Grotesque'"),
    ] {
        let scritto = valore(&css, BASE, proprieta).unwrap_or_else(|| panic!("{proprieta}"));
        assert!(
            scritto.starts_with(famiglia),
            "{proprieta} deve cominciare da {famiglia}, non da «{scritto}»"
        );
    }
}

#[test]
fn le_transizioni_ora_seguono_la_curva_della_skin() {
    // È l'unica differenza di comportamento della conversione, e vale scriverla:
    // in `global.css` le due transizioni erano `150ms cubic-bezier(0.4, 0, 0.2,
    // 1)` e `250ms cubic-bezier(0.4, 0, 0.2, 1)` — durate e curva scritte a
    // mano, che non c'entravano con `--dur-1`, `--dur-2` e `--ease-out-expo`
    // dichiarati venti righe sopra. Erano quattro valori da tenere allineati e
    // non lo erano: 250ms contro 280ms, e la curva di Material contro quella
    // della skin. Ora sono conseguenze, quindi non possono più divergere.
    //
    // Il `* var(--motion-scale, 1)` è arrivato dopo, ed è il lettore che mancava
    // a `--motion-intensity`: il compilatore scriveva la scala e nessuno la
    // usava. Il fallback serve alle skin che dichiarano le durate ma non
    // l'intensità — senza, otterrebbero un `calc()` invalido e zero transizioni.
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;
    assert_eq!(
        valore(&css, BASE, "--transition-fast").as_deref(),
        Some("calc(var(--dur-1) * var(--motion-scale, 1)) var(--ease-out-expo)")
    );
    assert_eq!(
        valore(&css, BASE, "--transition-med").as_deref(),
        Some("calc(var(--dur-2) * var(--motion-scale, 1)) var(--ease-out-expo)")
    );
}

/// Il colore compilato di una proprietà, in un blocco.
fn colore(css: &str, selettore: &str, proprieta: &str) -> aether_skin::values::Rgba {
    let scritto = valore(css, selettore, proprieta)
        .unwrap_or_else(|| panic!("{proprieta} non è in {selettore}:\n{css}"));
    parse_color(&scritto)
        .unwrap_or_else(|| panic!("{proprieta} non compila a un colore: {scritto}"))
}

#[test]
fn una_coppia_illeggibile_diventa_un_avviso() {
    // Il ramo che `check_skin` non aveva. `contrast_ratio()` esisteva da prima,
    // con un test che provava che `text.3` di `plain` stava sotto la soglia — e
    // nessuno la chiamava, quindi quel test provava un difetto invece di
    // impedirlo.
    //
    // Qui si rimette il valore vecchio, quello di `global.css`, e si controlla
    // che adesso qualcuno se ne accorga.
    let json = aether_skin::PLAIN_SOURCE.replace(
        "\"color.text.3\": \"rgba(255, 255, 255, 0.46)\"",
        "\"color.text.3\": \"rgba(255, 255, 255, 0.38)\"",
    );
    let skin =
        aether_skin::parse_skin_json(&json).expect("resta valida: è un avviso, non un errore");

    let avvisi = check_skin(&skin);
    let contrasto: Vec<&aether_skin::SkinWarning> = avvisi
        .iter()
        .filter(|a| a.kind == aether_skin::WarningKind::Contrast)
        .collect();
    assert!(
        !contrasto.is_empty(),
        "0.38 su un fondo scuro fa 3,47:1 e nessuno l'ha detto: {avvisi:#?}"
    );
    assert!(
        contrasto.iter().any(|a| a.path == "tokens.color.text.3"),
        "l'avviso deve nominare il token: {contrasto:#?}"
    );

    // E la tabella completa c'è, non solo le righe che non passano: è quella
    // che dice quanto margine resta prima che un ritocco al fondo rompa
    // qualcosa.
    let coppie = aether_skin::contrast_pairs(&skin);
    assert!(coppie.len() > 10, "{} coppie misurate", coppie.len());
    assert!(
        coppie.iter().any(aether_skin::ContrastPair::passa),
        "non tutte devono fallire"
    );
    // La coppia inversa — il testo **sopra** l'accento — è misurata: nessun'altra
    // riga la copre, perché lì `surface.0` è sempre un fondo.
    assert!(
        coppie
            .iter()
            .any(|c| c.foreground == "color.surface.0" && c.background == "color.accent"),
        "manca la coppia del bottone primario"
    );
}

#[test]
fn un_colore_che_segue_la_copertina_non_si_misura() {
    // Un rapporto inventato sarebbe peggio di un rapporto assente: al momento
    // della validazione il disco che suonerà non esiste, e misurarne il
    // contrasto vorrebbe dire sceglierne uno a caso.
    let json = r##"{
      "format": 1,
      "id": "prova",
      "meta": { "name": "Prova", "author": "Aether", "version": "1.0.0" },
      "tokens": {
        "color.surface.0": "#09090d",
        "color.text.1": { "$source": "albumArt.vibrant" }
      }
    }"##;
    let skin = aether_skin::parse_skin_json(json).expect("valida");
    let coppie = aether_skin::contrast_pairs(&skin);
    assert!(
        !coppie.iter().any(|c| c.foreground == "color.text.1"),
        "un colore dalla copertina non ha un rapporto da misurare"
    );
}

#[test]
fn la_tavolozza_sa_quante_volte_e_usata() {
    // «Usato 14×» accanto a un colore è il numero che trasforma una tavolozza in
    // un sistema. `plain` dichiara `ciano` e non lo usa in nessun token: lo
    // disegnano lo spinner e la barra di avanzamento, che sono CSS
    // dell'applicazione e non della skin. Il conteggio lo dice, e non è un
    // avviso — è un'informazione.
    let skin = plain().expect("valida");
    let uso = aether_skin::palette_usage(&skin);
    assert_eq!(uso, vec![("ciano".to_owned(), 0)]);

    // E quando è usato davvero, lo conta.
    let json = r##"{
      "format": 1,
      "id": "prova",
      "meta": { "name": "Prova", "author": "Aether", "version": "1.0.0" },
      "palette": { "ruggine": "#c96a2e" },
      "tokens": {
        "color.accent": { "$palette": "ruggine" },
        "color.hero": { "$palette": "ruggine", "alpha": 0.2 }
      },
      "parts": {
        "section-card": { "background": [{ "effect": "solid", "color": { "$palette": "ruggine" } }] }
      }
    }"##;
    let usata = aether_skin::parse_skin_json(json).expect("valida");
    assert_eq!(
        aether_skin::palette_usage(&usata),
        vec![("ruggine".to_owned(), 3)]
    );
}

#[test]
fn il_terzo_livello_di_testo_non_e_piu_quello_di_global_css() {
    // L'unica dichiarazione che la conversione non conserva, e il motivo per cui
    // vale la pena non conservarla: `rgba(255, 255, 255, 0.38)` su `#09090d` fa
    // 3,47:1, cioè sotto la soglia di WCAG per il testo normale. In
    // `global.css` era un numero scritto a occhio, e nessuno poteva accorgersene
    // perché nel vecchio albero i colori erano CSS e il CSS non si misura.
    //
    // Sull'albero mobile qualcuno l'aveva già alzato a mano a 0.5 dopo averlo
    // visto su un telefono — cioè lo stesso difetto scoperto due volte, sul
    // dispositivo, invece che una volta qui.
    //
    // Il valore non si fissa: si fissa la proprietà che deve avere. Se un giorno
    // le superfici cambiano, questo test chiede che `text.3` le segua.
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;

    for (tema, selettore) in [("scuro", BASE), ("chiaro", CHIARO)] {
        let testo = colore(&css, selettore, "--color-text-3");
        for superficie in [
            "--color-surface-0",
            "--color-surface-1",
            "--color-surface-2",
        ] {
            let fondo = colore(&css, selettore, superficie);
            let rapporto = contrast_ratio(testo, fondo);
            assert!(
                rapporto >= LEGGIBILE,
                "tema {tema}: text.3 su {superficie} fa {rapporto:.2}:1, sotto {LEGGIBILE}"
            );
        }
    }
}

#[test]
fn il_tema_chiaro_esiste_davvero() {
    // Questo test diceva un'altra cosa, e la cosa che diceva era il difetto: «il
    // blocco chiaro sovrascrive solo quel che cambia — otto token», e verificava
    // che `--accent` e `--color-surface-0` **non** ci fossero. Ma una variante
    // chiara che lascia le quattro superfici e i tre livelli di testo a quelli
    // scuri non è una variante chiara: è la stessa skin con i rossi diversi. E
    // `capabilities.light` era `true`, quindi l'interruttore del tema appariva e
    // non faceva quasi niente — che è esattamente l'avviso `UnkeptCapability`,
    // solo troppo debole per accorgersene.
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;

    // Le otto di `global.css` restano, con le tre semantiche riportate sopra la
    // soglia di contrasto (erano 4,27 e 3,90 su un fondo chiaro).
    for (proprieta, scritto) in [
        ("--sidebar-bg", "rgba(0, 0, 0, 0.04)"),
        ("--hairline", "rgba(0, 0, 0, 0.08)"),
        ("--danger", "#c41f26"),
        ("--danger-soft", "rgba(196, 31, 38, 0.12)"),
        ("--success", "#0a7040"),
        ("--success-soft", "rgba(10, 112, 64, 0.12)"),
        ("--warning", "#7d6200"),
        ("--warning-soft", "rgba(125, 98, 0, 0.14)"),
    ] {
        assert_eq!(
            Some(colore(&css, CHIARO, proprieta)),
            parse_color(scritto),
            "{proprieta}"
        );
    }
    assert_eq!(
        valore(&css, CHIARO, "color-scheme").as_deref(),
        Some("light")
    );

    // E le dieci che mancavano: senza queste, «chiaro» era una promessa.
    for proprieta in [
        "--color-surface-0",
        "--color-surface-1",
        "--color-surface-2",
        "--color-surface-3",
        "--color-text-1",
        "--color-text-2",
        "--color-text-3",
        "--accent",
        "--accent-soft",
        "--accent-glow",
    ] {
        assert!(
            valore(&css, CHIARO, proprieta).is_some(),
            "{proprieta} manca nel tema chiaro"
        );
    }

    // La superficie di fondo è chiara sul serio, non «meno scura». Serve anche a
    // `--surface-0-rgb`, che il velo di `np-screen` usa per scurire l'ambiente:
    // con un fondo scuro dichiarato chiaro, quel velo diventerebbe una macchia.
    let fondo = colore(&css, CHIARO, "--color-surface-0");
    assert!(
        aether_skin::values::relative_luminance(fondo) > 0.7,
        "il fondo del tema chiaro non è chiaro: {fondo:?}"
    );
}

#[test]
fn nel_tema_chiaro_il_testo_e_le_semantiche_si_leggono() {
    // La prova che il tema chiaro non ripete l'errore di `text.3`: ogni colore
    // che finisce **sul testo** deve stare sopra 4,5:1 su tutte e tre le
    // superfici su cui il testo può stare. La quarta (`surface.3`) è
    // deliberatamente fuori: è un riempimento di sopraelevazione — il fondo di
    // un elemento di menù al passaggio del mouse — e non è un letto di testo
    // semantico. La regola sta scritta qui perché è il posto in cui si scopre se
    // qualcuno la cambia.
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;

    for (tema, selettore) in [("scuro", BASE), ("chiaro", CHIARO)] {
        for proprieta in [
            "--color-text-1",
            "--color-text-2",
            "--color-text-3",
            "--accent",
            "--danger",
            "--success",
            "--warning",
        ] {
            let davanti = colore(&css, selettore, proprieta);
            for superficie in [
                "--color-surface-0",
                "--color-surface-1",
                "--color-surface-2",
            ] {
                let fondo = colore(&css, selettore, superficie);
                let rapporto = contrast_ratio(davanti, fondo);
                assert!(
                    rapporto >= LEGGIBILE,
                    "tema {tema}: {proprieta} su {superficie} fa {rapporto:.2}:1"
                );
            }
        }

        // E la regola inversa, quella di §7 del brief: il testo che sta **sopra**
        // l'accento è `--color-surface-0`. Se l'accento si schiarisce, l'etichetta
        // di un bottone primario sparisce, e nessun altro test se ne accorge.
        let accento = colore(&css, selettore, "--accent");
        let sopra = colore(&css, selettore, "--color-surface-0");
        let rapporto = contrast_ratio(sopra, accento);
        assert!(
            rapporto >= LEGGIBILE,
            "tema {tema}: surface.0 sopra l'accento fa {rapporto:.2}:1"
        );
    }
}

#[test]
fn le_transizioni_di_rotta_sono_quelle_scritte_a_mano() {
    // In `global.css` erano due `@keyframes` a mano, `vt-out` e `vt-in`. Qui
    // sono quattro numeri, e il compilatore rifà i fotogrammi.
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;
    assert!(css.contains("@keyframes skin-plain-out"), "{css}");
    assert!(css.contains("transform: scale(0.992);"), "{css}");
    assert!(css.contains("@keyframes skin-plain-in"), "{css}");
    assert!(css.contains("transform: translateY(8px);"), "{css}");
}

#[test]
fn plain_non_costa_niente() {
    // È il riferimento: nessun motivo, nessuna superficie ridisegnata, quindi
    // costo zero. Una skin che parte da qui ha tutti e dieci i punti di budget.
    let skin = plain().expect("valida");
    let compilata = compile_skin(&skin);
    assert_eq!(compilata.cost, 0);
    assert_eq!(compilata.id, "plain");
    // Nessun token segue la copertina: in `plain` l'accento è un colore fisso e
    // il legame con la copertina lo fa il runtime scrivendo `--accent`, che è il
    // comportamento storico di questa skin.
    assert!(compilata.dynamic_tokens.is_empty());
}

#[test]
fn plain_sopravvive_a_un_giro_nel_pacchetto() {
    // Il percorso che un trasferimento PC→telefono fa davvero: si impacchetta,
    // si spedisce, si rilegge. Se la sorgente non tornasse indietro identica, il
    // pacchetto sarebbe illeggibile dallo stesso codice che l'ha scritto.
    let archivio = aether_skin::write_skin_package(&aether_skin::package::WritePackageInput {
        source: aether_skin::PLAIN_SOURCE,
        preview: None,
        assets: &[],
    })
    .expect("scritto");
    let letto = aether_skin::read_skin_package(&archivio).expect("riletto");
    assert_eq!(letto.source, aether_skin::PLAIN_SOURCE);
    assert_eq!(
        compile_skin(&letto.document).css,
        compile_skin(&plain().expect("valida")).css
    );
}
