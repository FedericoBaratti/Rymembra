//! Piegatura del testo: rendere uguali due scritture della stessa cosa.
//!
//! Serve a due mestieri diversi che devono restare d'accordo — la ricerca in
//! libreria («cerco *Bjork*, trovami *Björk*») e le chiavi di identità che
//! attraversano la sincronizzazione. Il secondo è quello che impone la
//! precisione: una chiave finisce nel file su Drive, e se questo codice piega
//! anche solo un carattere in modo diverso da come lo piegava il vecchio albero,
//! ogni brano della libreria risulta «mancante» sull'altro dispositivo e ne
//! parte il ri-scaricamento.
//!
//! Per questo le regole qui sotto sono trascritte alla lettera dall'originale
//! JavaScript, non riscritte «meglio», e provate contro vettori generati
//! eseguendo quell'originale (`tests/golden/text-keys.json`).

use unicode_normalization::UnicodeNormalization;

/// Primo segno combinante che la piegatura rimuove.
const COMBINING_FIRST: char = '\u{0300}';
/// Ultimo segno combinante che la piegatura rimuove.
const COMBINING_LAST: char = '\u{036F}';

/// Piega il testo: NFD, via i segni combinanti latini, minuscolo.
///
/// # L'intervallo è letterale, e va tenuto tale
///
/// Si tolgono **solo** i codepoint da U+0300 a U+036F — il blocco *Combining
/// Diacritical Marks* — e non «i segni combinanti» in generale. La differenza
/// non è teorica:
///
/// | carattere | blocco | esito |
/// |---|---|---|
/// | `a` + U+0300 | Combining Diacritical Marks | → `a` |
/// | `a` + U+1DC0 | …Supplement | → resta `a᷀` |
/// | `a` + U+20D0 | …for Symbols | → resta `a⃐` |
/// | `a` + U+0483 | Cyrillic | → resta `a҃` |
///
/// Un port che usasse la categoria Unicode `Mn` toglierebbe anche gli ultimi
/// tre, produrrebbe chiavi diverse da quelle già scritte nei file di sync, e il
/// guasto si manifesterebbe come brani che ricompaiono da soli.
///
/// # L'ordine conta
///
/// NFD **prima**, minuscolo **dopo**. Con la I turca maiuscola (U+0130) si vede
/// perché: NFD la scompone in `I` + U+0307, il punto sopra cade nell'intervallo
/// e sparisce, e resta `i`. Invertendo i due passi si otterrebbe `i` + U+0307,
/// cioè una chiave diversa per lo stesso nome.
///
/// ```
/// use aether_domain::text::fold_text;
/// assert_eq!(fold_text("Blue Öyster Cult"), "blue oyster cult");
/// assert_eq!(fold_text("İstanbul"), "istanbul");
/// ```
#[must_use]
pub fn fold_text(input: &str) -> String {
    let stripped: String = input
        .nfd()
        .filter(|c| !(COMBINING_FIRST..=COMBINING_LAST).contains(c))
        .collect();
    stripped.to_lowercase()
}

/// Lo spazio secondo `\s` di JavaScript, che **non** è `char::is_whitespace`.
///
/// I due insiemi differiscono in due punti, e in direzioni opposte:
///
/// - **U+0085** (NEL) — spazio per Rust, non per JavaScript;
/// - **U+FEFF** (BOM / spazio unificatore di larghezza zero) — spazio per
///   JavaScript, non per Rust.
///
/// Usare la funzione di libreria sembrerebbe più pulito e cambierebbe due
/// chiavi su un milione: esattamente il genere di differenza che non si nota in
/// prova e si nota fra sei mesi, quando un brano non si allinea più e non c'è
/// modo di collegarlo a questa riga. L'insieme è quindi scritto per esteso,
/// come lo definisce ECMA-262 (`WhiteSpace` ∪ `LineTerminator`).
#[must_use]
pub const fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'          // TAB
        | '\u{000A}'        // LF          — LineTerminator
        | '\u{000B}'        // VT
        | '\u{000C}'        // FF
        | '\u{000D}'        // CR          — LineTerminator
        | '\u{0020}'        // SPACE
        | '\u{00A0}'        // NBSP
        | '\u{1680}'        // OGHAM SPACE MARK
        | '\u{2000}'
            ..='\u{200A}'
        | '\u{2028}'        // LINE SEPARATOR       — LineTerminator
        | '\u{2029}'        // PARAGRAPH SEPARATOR  — LineTerminator
        | '\u{202F}'        // NARROW NBSP
        | '\u{205F}'        // MEDIUM MATHEMATICAL SPACE
        | '\u{3000}'        // IDEOGRAPHIC SPACE
        | '\u{FEFF}' // ZERO WIDTH NO-BREAK SPACE
    )
}

/// Riduce ogni corsa di spazi a uno solo e toglie quelli ai bordi.
///
/// Equivale a `.replace(/\s+/g, ' ').trim()` del JavaScript originale, con lo
/// stesso insieme di spazi di [`is_js_whitespace`] anche per la rifilatura —
/// `str::trim` userebbe il predicato di Rust e tratterebbe U+0085 e U+FEFF al
/// contrario.
#[must_use]
pub fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    for c in input.chars() {
        if is_js_whitespace(c) {
            // Lo spazio si annota, non si scrive: così quelli finali non
            // arrivano mai nella stringa e la rifilatura di coda è gratis.
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toglie_i_diacritici_dentro_l_intervallo() {
        assert_eq!(fold_text("Blue Öyster Cult"), "blue oyster cult");
        assert_eq!(fold_text("Café"), "cafe");
        // …anche quando arrivano già scomposti dal tagger.
        assert_eq!(fold_text("Cafe\u{0301}"), "cafe");
    }

    #[test]
    fn lascia_i_combinanti_fuori_dall_intervallo() {
        // Se un giorno qualcuno «semplifica» usando la categoria Mn, è questo
        // test a cadere, non un utente che vede i brani duplicarsi.
        assert_eq!(fold_text("a\u{1DC0}"), "a\u{1DC0}");
        assert_eq!(fold_text("a\u{20D0}"), "a\u{20D0}");
        assert_eq!(fold_text("a\u{0483}"), "a\u{0483}");
        assert_eq!(fold_text("a\u{FE20}"), "a\u{FE20}");
    }

    #[test]
    fn nfd_prima_del_minuscolo() {
        // U+0130 si scompone in I + U+0307: il punto cade nell'intervallo.
        // Con l'ordine invertito resterebbe attaccato e la chiave cambierebbe.
        assert_eq!(fold_text("İstanbul"), "istanbul");
    }

    #[test]
    fn le_scritture_non_latine_sopravvivono() {
        // Piegare non deve voler dire cancellare: una chiave vuota farebbe
        // collassare tutti i brani giapponesi in uno solo.
        assert_eq!(fold_text("坂本龍一"), "坂本龍一");
        assert_eq!(fold_text("Кино"), "кино");
    }

    #[test]
    fn spazi_come_javascript_non_come_rust() {
        assert!(is_js_whitespace('\u{FEFF}'), "il BOM è spazio per JS");
        assert!(!is_js_whitespace('\u{0085}'), "NEL non lo è");
        // …e i due sono esattamente il contrario per la libreria standard.
        assert!(!'\u{FEFF}'.is_whitespace());
        assert!('\u{0085}'.is_whitespace());
    }

    #[test]
    fn collassa_e_rifila() {
        assert_eq!(collapse_whitespace("  a  b  "), "a b");
        assert_eq!(collapse_whitespace("\t\na\r\nb\t"), "a b");
        assert_eq!(collapse_whitespace("   "), "");
        assert_eq!(collapse_whitespace(""), "");
        // Lo spazio a larghezza zero non è spazio: separa senza unire.
        assert_eq!(collapse_whitespace("a\u{200B}b"), "a\u{200B}b");
    }
}
