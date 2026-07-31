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

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use aether_skin::values::parse_color;
use aether_skin::{check_skin, compile_skin, plain};

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
    ("--color-text-3", "rgba(255, 255, 255, 0.38)"),
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
    (
        "--font-sans",
        "'Inter Variable', system-ui, -apple-system, sans-serif",
    ),
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
    ("--viz-primary-rgb", "var(--accent-rgb)"),
    ("--viz-secondary", "var(--accent)"),
    ("--viz-secondary-rgb", "var(--accent-rgb)"),
    ("--viz-glow", "20"),
    ("--scrubber-glow", "6"),
];

/// Le riscritture note: stesso valore, notazione del compilatore.
///
/// `0` diventa `0px` perché una lunghezza è sempre un numero più un'unità, e
/// `rgba(a, b, c, d)` diventa `rgb(a b c / d)` perché è la forma che il
/// compilatore emette per ogni colore. Sono due differenze di scrittura, non di
/// risultato.
const RISCRITTI: &[(&str, &str)] = &[
    ("--shadow-1", "0px 2px 12px rgb(0 0 0 / 0.3)"),
    ("--shadow-2", "0px 8px 28px rgb(0 0 0 / 0.45)"),
    ("--shadow-3", "0px 16px 56px rgb(0 0 0 / 0.55)"),
    (
        "--shadow-player",
        "inset 0px 1px 0px rgb(255 255 255 / 0.06), 0px 8px 40px rgb(0 0 0 / 0.5)",
    ),
    ("--glow-accent", "0px 0px 24px var(--accent-glow)"),
];

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
    for (proprieta, atteso) in RISCRITTI {
        assert_eq!(
            valore(&css, BASE, proprieta).as_deref(),
            Some(*atteso),
            "{proprieta}"
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
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;
    assert_eq!(
        valore(&css, BASE, "--transition-fast").as_deref(),
        Some("var(--dur-1) var(--ease-out-expo)")
    );
    assert_eq!(
        valore(&css, BASE, "--transition-med").as_deref(),
        Some("var(--dur-2) var(--ease-out-expo)")
    );
}

#[test]
fn il_tema_chiaro_e_quello_di_global_css() {
    let skin = plain().expect("valida");
    let css = compile_skin(&skin).css;

    for (proprieta, scritto) in [
        ("--sidebar-bg", "rgba(0, 0, 0, 0.04)"),
        ("--hairline", "rgba(0, 0, 0, 0.08)"),
        ("--danger", "#d1242b"),
        ("--danger-soft", "rgba(209, 36, 43, 0.12)"),
        ("--success", "#0f8a4d"),
        ("--success-soft", "rgba(15, 138, 77, 0.12)"),
        ("--warning", "#9a7b00"),
        ("--warning-soft", "rgba(154, 123, 0, 0.14)"),
    ] {
        let compilato = valore(&css, CHIARO, proprieta)
            .unwrap_or_else(|| panic!("{proprieta} non è nel tema chiaro:\n{css}"));
        assert_eq!(parse_color(&compilato), parse_color(scritto), "{proprieta}");
    }
    assert_eq!(
        valore(&css, CHIARO, "color-scheme").as_deref(),
        Some("light")
    );
    // E il blocco chiaro sovrascrive **solo** quel che cambia: otto token più
    // `color-scheme`. Nel vecchio albero era la stessa cosa, ed è la proprietà
    // che rende un tema una variante invece di una seconda skin.
    assert_eq!(valore(&css, CHIARO, "--accent"), None);
    assert_eq!(valore(&css, CHIARO, "--color-surface-0"), None);
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
