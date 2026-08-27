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

/// L'alfabeto di base64, quello standard.
///
/// **Non** quello di `aether-oauth`: là è base64url, che sostituisce `+` e `/`
/// con `-` e `_` perché devono attraversare un indirizzo. Un `data:` URI vuole
/// l'alfabeto normale, e usare l'altro produce un'immagine che il browser
/// accetta e disegna vuota — cioè il guasto che non si vede.
const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// I byte come `data:` URI, se sono un'immagine riconosciuta.
///
/// # Perché un `data:` URI e non un indirizzo
///
/// Perché la politica dei contenuti della finestra non ammette domini esterni
/// fra le immagini (`img-src 'self' data:` in `tauri.conf.json`), e allargarla
/// per sempre per una miniatura è esattamente quel che il resto
/// dell'applicazione si rifiuta di fare. Portare dentro i byte costa una
/// richiesta e non apre niente.
///
/// `None` quando i byte non sono un'immagine: vedi [`plausibile`].
#[must_use]
pub fn data_uri(byte: &[u8], massimo: usize) -> Option<String> {
    let tipo = plausibile(byte, massimo).then(|| tipo(byte))??;
    let mut fuori = String::with_capacity(byte.len().saturating_mul(4).saturating_div(3) + 32);
    fuori.push_str("data:");
    fuori.push_str(tipo);
    fuori.push_str(";base64,");

    for gruppo in byte.chunks(3) {
        let (a, b, c) = (
            gruppo.first().copied().unwrap_or(0),
            gruppo.get(1).copied().unwrap_or(0),
            gruppo.get(2).copied().unwrap_or(0),
        );
        let impacchettato = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        let cifra = |spostamento: u32| {
            let indice = ((impacchettato >> spostamento) & 0x3F) as usize;
            char::from(ALFABETO.get(indice).copied().unwrap_or(b'A'))
        };
        fuori.push(cifra(18));
        fuori.push(cifra(12));
        // Il riempimento non è ornamento: un decodificatore che riceve un
        // gruppo tronco senza `=` può leggere un byte in più di spazzatura, e
        // su un JPEG quel byte in più è una riga di pixel storta in fondo.
        fuori.push(if gruppo.len() > 1 { cifra(6) } else { '=' });
        fuori.push(if gruppo.len() > 2 { cifra(0) } else { '=' });
    }
    Some(fuori)
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

    #[test]
    fn il_data_uri_dichiara_il_tipo_e_codifica_i_byte() {
        // Un PNG minimo: firma più abbastanza byte da passare il cancello.
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0_u8; 16]);
        let uri = data_uri(&png, 1024).expect("è un png");
        assert!(uri.starts_with("data:image/png;base64,"));
        // Ventiquattro byte sono otto gruppi pieni: nessun riempimento.
        assert!(!uri.ends_with('='));
    }

    #[test]
    fn il_riempimento_c_e_quando_serve() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF];
        jpeg.extend_from_slice(&[0x11_u8; 13]);
        let uri = data_uri(&jpeg, 1024).expect("è un jpeg");
        let carico = uri.split(",").nth(1).unwrap_or_default();
        // Sedici byte: cinque gruppi pieni più uno da uno, cioè due `=`.
        assert!(carico.ends_with("=="), "carico: {carico}");
    }

    #[test]
    fn quel_che_non_e_unimmagine_non_diventa_un_uri() {
        assert_eq!(data_uri(b"<!DOCTYPE html><html>errore</html>", 1024), None);
        assert_eq!(data_uri(&[], 1024), None);
        // E nemmeno quel che è troppo grande per essere una copertina.
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0_u8; 64]);
        assert_eq!(data_uri(&png, 16), None);
    }
}
