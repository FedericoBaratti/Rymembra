//! Il suggerimento dopo un nome sbagliato.
//!
//! Chi crea una skin sbaglia un nome per assonanza — `section-cards`,
//! `color.accents`, `radius.cards` — e ricevere «non esiste» senza un
//! suggerimento significa aprire il registro e leggerlo tutto. Con due o tre
//! candidati la correzione è immediata.
//!
//! Sta in un modulo suo perché serve identico ai token e alle parti, e nel
//! vecchio albero esisteva solo per le parti: chi sbagliava il nome di un token
//! riceveva l'elenco completo delle voci del registro, che è il modo educato di non
//! dire niente.

/// Quanti caratteri del nome scritto si confrontano. Vedi [`vicini`].
const MAX_CARATTERI: usize = 64;

/// I nomi più vicini a quello scritto, al massimo tre.
pub(crate) fn vicini<'a>(ago: &str, candidati: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    // Si guarda la testa del nome scritto, non tutto: [`distanza`] costa una
    // riga della tabella per ogni carattere, contro ogni candidato, e una
    // chiave lunga quanto il documento ne costava milioni (R09-1). Nessun nome
    // del registro supera i cinquanta caratteri; una testa di sessantaquattro
    // basta a riconoscere un refuso e un prefisso giusto.
    let ago: String = ago
        .chars()
        .take(MAX_CARATTERI)
        .collect::<String>()
        .to_lowercase();
    // La testa è il primo segmento: chi sbaglia `np-titles` sbaglia la coda, chi
    // sbaglia `player-bar` sbaglia la coda, e la testa resta buona in entrambi.
    let testa = ago.split(['-', '.']).next().unwrap_or_default().to_owned();

    let mut punteggi: Vec<(u8, &'a str)> = candidati
        .filter_map(|nome| {
            let minuscolo = nome.to_lowercase();
            let refusi = distanza(&ago, &minuscolo);
            let punteggio = if minuscolo.starts_with(&ago) || ago.starts_with(&minuscolo) {
                3
            } else if refusi <= 1 {
                // Una lettera saltata o raddoppiata — `mola`, `pulssa` — è il
                // refuso più comune, e non somiglia al nome per nessun pezzo.
                3
            } else if minuscolo.contains(&ago)
                || ago.contains(&minuscolo)
                || (refusi == 2 && ago.chars().count() >= 5)
            {
                2
            } else if testa.len() > 2 && minuscolo.contains(&testa) {
                1
            } else {
                0
            };
            (punteggio > 0).then_some((punteggio, nome))
        })
        .collect();

    // Ordinamento stabile: a parità di punteggio vince chi viene prima nel
    // registro, così due esecuzioni danno lo stesso messaggio e un test che lo
    // confronta non diventa capriccioso.
    punteggi.sort_by_key(|(punteggio, _)| std::cmp::Reverse(*punteggio));
    punteggi.into_iter().take(3).map(|(_, nome)| nome).collect()
}

/// Quante lettere tolte, aggiunte o cambiate separano due nomi (Levenshtein).
fn distanza(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    // Una riga della tabella alla volta: `prima[j]` è la distanza fra il
    // pezzo di `a` letto finora e i primi `j` caratteri di `b`.
    let mut prima: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut riga = Vec::with_capacity(prima.len());
        riga.push(i + 1);
        for ((cb, diagonale), sopra) in b.iter().zip(&prima).zip(prima.iter().skip(1)) {
            let sinistra = riga.last().copied().unwrap_or(0);
            riga.push(if ca == *cb {
                *diagonale
            } else {
                diagonale.min(sopra).min(&sinistra).saturating_add(1)
            });
        }
        prima = riga;
    }
    prima.last().copied().unwrap_or(0)
}

/// I candidati, scritti come li leggerà chi ha sbagliato.
pub(crate) fn forse(candidati: &[&str]) -> String {
    let elenco: Vec<String> = candidati.iter().map(|nome| format!("«{nome}»")).collect();
    match elenco.len() {
        0 => String::new(),
        _ => format!(" Forse intendevi {}?", elenco.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_distanza_conta_le_lettere_da_cambiare() {
        assert_eq!(distanza("molla", "molla"), 0);
        assert_eq!(distanza("mola", "molla"), 1);
        assert_eq!(distanza("pulssa", "pulsa"), 1);
        assert_eq!(distanza("sale", "sela"), 2);
        assert_eq!(distanza("", "abc"), 3);
        assert_eq!(distanza("abc", ""), 3);
    }

    #[test]
    fn un_refuso_di_una_lettera_trova_il_nome() {
        assert_eq!(vicini("mola", ["molla", "scatto"].into_iter()), ["molla"]);
    }

    /// Una chiave sconosciuta lunga quanto il documento costava a `distanza` un
    /// `Vec` nuovo per ogni suo carattere, contro ognuno dei settantatré token
    /// (R09-1): minuti di processore per un testo che non è nemmeno valido.
    #[test]
    fn una_chiave_enorme_non_costa_minuti() {
        let token = || crate::tokens::TOKENS.iter().map(|t| t.id);
        let ago = "x".repeat(200_000);
        let inizio = std::time::Instant::now();
        let trovati = vicini(&ago, token());
        assert!(
            inizio.elapsed() < std::time::Duration::from_millis(500),
            "{:?}",
            inizio.elapsed()
        );
        assert!(trovati.is_empty(), "{trovati:?}");
        // La testa resta buona anche in un nome lungo: un prefisso noto si
        // suggerisce ancora.
        let lungo = format!("color.accent{}", "x".repeat(200_000));
        assert!(vicini(&lungo, token()).contains(&"color.accent"));
    }
}
