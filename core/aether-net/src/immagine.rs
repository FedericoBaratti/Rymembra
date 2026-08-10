//! Se dei byte arrivati dalla rete sono davvero un'immagine.
//!
//! Sta qui, accanto al client HTTP, perché è una domanda su **quel che è
//! arrivato dalla connessione** e non sul formato dei file in generale. Chi
//! scarica una copertina la fa sempre, e la fa per un motivo concreto: un CDN
//! che risponde `200 OK` con una pagina d'errore restituisce comunque dei byte,
//! e senza questo cancello quei byte finiscono nello store delle copertine o —
//! peggio — incorporati dentro un file musicale, dove restano per sempre e in
//! griglia si vedono come un rettangolo rotto.
//!
//! # Dai byte, non dall'intestazione
//!
//! `Content-Type` è quel che il servitore **dichiara**; i primi byte sono quel
//! che si ha davvero in mano. Un servitore che sbaglia il tipo è comune, un
//! `Content-Type: image/jpeg` su una pagina di errore è quasi la norma sui CDN
//! con un proxy davanti. Si guarda il contenuto.

/// Il tipo dell'immagine, dai suoi primi byte.
///
/// `None` per tutto ciò che non si riconosce, compreso il vuoto. L'elenco è
/// chiuso di proposito: sono i tre formati che i servizi di copertine
/// restituiscono davvero, e riconoscerne un quarto che poi il decodificatore
/// non sa aprire sposterebbe soltanto il guasto più in là.
#[must_use]
pub fn tipo(byte: &[u8]) -> Option<&'static str> {
    if byte.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if byte.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if byte.starts_with(b"RIFF") && byte.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    None
}

/// I byte sono plausibilmente un'immagine di copertina.
///
/// Tre condizioni: abbastanza byte da contenere qualcosa, non più di `massimo`,
/// e una firma riconosciuta. Il tetto non è una stima generosa di quanto possa
/// crescere una copertina: è il punto oltre il quale conviene non mostrare
/// niente piuttosto che tenere in memoria quel che ha risposto un indirizzo su
/// cui non abbiamo controllo.
#[must_use]
pub fn plausibile(byte: &[u8], massimo: usize) -> bool {
    byte.len() >= 16 && byte.len() <= massimo && tipo(byte).is_some()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_tipo_viene_dai_byte() {
        assert_eq!(tipo(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(
            tipo(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00]),
            Some("image/png")
        );
        assert_eq!(tipo(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    }

    #[test]
    fn una_pagina_derrore_non_e_unimmagine() {
        // Il caso vero: il CDN risponde 200 con dell'HTML.
        assert_eq!(tipo(b"<!DOCTYPE html><html><body>404</body></html>"), None);
        assert_eq!(tipo(b""), None);
        // Un WAV comincia per RIFF come un WebP: i quattro byte a partire
        // dall'ottavo sono l'unica cosa che li distingue.
        assert_eq!(tipo(b"RIFF\0\0\0\0WAVEfmt "), None);
    }

    #[test]
    fn il_tetto_vale_in_tutte_e_due_le_direzioni() {
        let jpeg = |quanti: usize| {
            let mut byte = vec![0xFF_u8, 0xD8, 0xFF];
            byte.resize(quanti, 0);
            byte
        };
        assert!(plausibile(&jpeg(64), 1024));
        assert!(!plausibile(&jpeg(2048), 1024), "troppo grande");
        assert!(
            !plausibile(&jpeg(8), 1024),
            "troppo corta per essere niente"
        );
    }
}
