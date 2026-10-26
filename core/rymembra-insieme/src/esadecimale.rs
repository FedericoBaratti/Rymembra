//! Byte ↔ esadecimale minuscolo, per gettoni, chiavi e impronte nei messaggi.
//!
//! Venti righe invece di una dipendenza: il formato è uno, e non cambia.

/// I byte in esadecimale minuscolo.
pub(crate) fn scrivi(byte: &[u8]) -> String {
    const CIFRE: &[u8; 16] = b"0123456789abcdef";
    let mut testo = String::with_capacity(byte.len() * 2);
    for b in byte {
        // Un nibble sta sempre fra 0 e 15: il ripiego non scatta mai, ed è lì
        // solo perché l'indicizzazione diretta qui è vietata.
        for nibble in [b >> 4, b & 0x0f] {
            let cifra = CIFRE.get(usize::from(nibble)).copied().unwrap_or(b'0');
            testo.push(char::from(cifra));
        }
    }
    testo
}

/// Esattamente `N` byte da `2·N` cifre esadecimali; `None` per tutto il resto.
pub(crate) fn leggi<const N: usize>(testo: &str) -> Option<[u8; N]> {
    let cifre = testo.as_bytes();
    if cifre.len() != N * 2 {
        return None;
    }
    let mut byte = [0_u8; N];
    let (coppie, _) = cifre.as_chunks::<2>();
    for (posto, [alta, bassa]) in byte.iter_mut().zip(coppie) {
        *posto = (cifra(*alta)? << 4) | cifra(*bassa)?;
    }
    Some(byte)
}

fn cifra(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn andata_e_ritorno() {
        let byte = [0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(scrivi(&byte), "000fa5ff");
        assert_eq!(leggi::<4>("000fa5ff"), Some(byte));
        assert_eq!(
            leggi::<4>("000FA5FF"),
            Some(byte),
            "le maiuscole si leggono"
        );
    }

    #[test]
    fn la_lunghezza_sbagliata_e_le_cifre_sbagliate_non_passano() {
        assert_eq!(leggi::<4>("000fa5f"), None);
        assert_eq!(leggi::<4>("000fa5fff0"), None);
        assert_eq!(leggi::<4>("000fa5fg"), None);
        assert_eq!(
            leggi::<2>("éé"),
            None,
            "un carattere di due byte non è una cifra"
        );
    }
}
