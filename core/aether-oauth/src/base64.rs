//! base64url, l'unica codifica che attraversa OAuth senza farsi male.
//!
//! Non è base64. La differenza sono due caratteri e un riempimento, e sono
//! esattamente i tre punti in cui base64 normale si rompe dentro un indirizzo:
//! `/` spezzerebbe il percorso, `+` diventerebbe uno spazio in una `query`, e un
//! `=` finale andrebbe codificato a sua volta.

/// L'alfabeto base64url: `-` e `_` al posto di `+` e `/`.
const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Codifica in base64url **senza riempimento**.
///
/// Senza `=` finali perché è quel che la RFC 7636 impone per PKCE, e perché un
/// `=` dentro una `query` va poi codificato a sua volta: due modi di sbagliare
/// tolti insieme.
///
/// Scritta con iteratori e `get`, non con indici: `indexing_slicing` è `deny` in
/// questo workspace, e in una funzione che maneggia byte crittografici quella
/// regola vale il doppio — un indice fuori posto qui non darebbe un errore, ma
/// una stringa sbagliata che fallisce solo dalla parte del fornitore.
#[must_use]
pub fn base64url(byte: &[u8]) -> String {
    let mut fuori = String::new();
    for gruppo in byte.chunks(3) {
        let mut pezzi = gruppo.iter().copied();
        let primo = pezzi.next().unwrap_or(0);
        let secondo = pezzi.next();
        let terzo = pezzi.next();
        let impacchettato = (u32::from(primo) << 16)
            | (u32::from(secondo.unwrap_or(0)) << 8)
            | u32::from(terzo.unwrap_or(0));
        // Tre byte fanno quattro caratteri; due ne fanno tre, uno ne fa due. È
        // il conto che rende la codifica reversibile senza riempimento.
        let quanti = match (secondo, terzo) {
            (None, _) => 2,
            (Some(_), None) => 3,
            (Some(_), Some(_)) => 4,
        };
        for spostamento in [18u32, 12, 6, 0].into_iter().take(quanti) {
            let sestina = usize::try_from((impacchettato >> spostamento) & 63).unwrap_or(0);
            if let Some(carattere) = ALFABETO.get(sestina) {
                fuori.push(char::from(*carattere));
            }
        }
    }
    fuori
}

/// Decodifica base64url, con o senza riempimento.
///
/// `None` se compare un carattere che non appartiene all'alfabeto. Serve a
/// leggere il carico di un id token, che Google manda senza `=`.
#[must_use]
pub fn da_base64url(testo: &str) -> Option<Vec<u8>> {
    let mut fuori = Vec::new();
    let mut accumulatore: u32 = 0;
    let mut bit: u32 = 0;
    for carattere in testo.bytes() {
        if carattere == b'=' {
            break;
        }
        let valore = ALFABETO.iter().position(|c| *c == carattere)?;
        accumulatore = (accumulatore << 6) | u32::try_from(valore).unwrap_or(0);
        bit += 6;
        if bit >= 8 {
            bit -= 8;
            fuori.push(u8::try_from((accumulatore >> bit) & 0xFF).unwrap_or(0));
        }
    }
    Some(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn base64url_non_usa_ne_piu_ne_barre_ne_riempimento() {
        // I tre byte che producono `+` e `/` in base64 normale. In un indirizzo
        // una barra spezzerebbe il percorso e un più diventerebbe uno spazio.
        let byte = [0xFBu8, 0xFF, 0xBF];
        let codificato = base64url(&byte);
        assert!(!codificato.contains('+'));
        assert!(!codificato.contains('/'));
        assert!(!codificato.contains('='));
        assert_eq!(da_base64url(&codificato), Some(byte.to_vec()));
    }

    #[test]
    fn base64url_fa_andata_e_ritorno_su_ogni_lunghezza() {
        // Le tre code possibili: zero, uno o due byte oltre il gruppo da tre.
        for quanti in 0..12usize {
            let byte: Vec<u8> = (0..quanti)
                .map(|i| u8::try_from(i * 7 % 256).unwrap_or(0))
                .collect();
            assert_eq!(
                da_base64url(&base64url(&byte)),
                Some(byte.clone()),
                "lunghezza {quanti}"
            );
        }
    }

    #[test]
    fn un_carattere_estraneo_non_si_decodifica() {
        assert_eq!(da_base64url("abc!"), None);
        assert_eq!(
            da_base64url("ab+c"),
            None,
            "il più non è di questo alfabeto"
        );
    }
}
