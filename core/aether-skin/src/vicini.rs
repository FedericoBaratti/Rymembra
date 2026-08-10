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

/// I nomi più vicini a quello scritto, al massimo tre.
pub(crate) fn vicini<'a>(ago: &str, candidati: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let ago = ago.to_lowercase();
    // La testa è il primo segmento: chi sbaglia `np-titles` sbaglia la coda, chi
    // sbaglia `player-bar` sbaglia la coda, e la testa resta buona in entrambi.
    let testa = ago.split(['-', '.']).next().unwrap_or_default().to_owned();

    let mut punteggi: Vec<(u8, &'a str)> = candidati
        .filter_map(|nome| {
            let minuscolo = nome.to_lowercase();
            let punteggio = if minuscolo.starts_with(&ago) || ago.starts_with(&minuscolo) {
                3
            } else if minuscolo.contains(&ago) || ago.contains(&minuscolo) {
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

/// I candidati, scritti come li leggerà chi ha sbagliato.
pub(crate) fn forse(candidati: &[&str]) -> String {
    let elenco: Vec<String> = candidati.iter().map(|nome| format!("«{nome}»")).collect();
    match elenco.len() {
        0 => String::new(),
        _ => format!(" Forse intendevi {}?", elenco.join(", ")),
    }
}
