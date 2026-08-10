//! La copertina dell'anteprima, portata dentro come `data:`.
//!
//! # Perché non basta passare l'indirizzo
//!
//! La politica dei contenuti della finestra (`tauri.conf.json`) ammette per le
//! immagini soltanto `data:` e il protocollo locale delle copertine. Un indirizzo
//! di `i.scdn.co` verrebbe bloccato, e l'alternativa — allargare la politica per
//! sempre a un dominio esterno — è esattamente quel che il resto
//! dell'applicazione si rifiuta di fare per un'operazione rara: la stessa
//! ragione per cui il consenso di Google si apre nel browser di sistema invece
//! che nella webview.
//!
//! Portarla dentro costa una richiesta in più su un percorso che già ne fa tre o
//! quattro, e in cambio la finestra non parla mai con Spotify: nessuna richiesta
//! parte dal motore di rendering, quindi l'indirizzo di rete dell'utente resta
//! dove sta.
//!
//! Non è un archivio: quel che finisce qui vive quanto una schermata. Le
//! copertine vere della libreria stanno nel loro store, con la loro impronta.

use aether_net::http::{Corpo, Metodo, Rete, Richiesta};
// Il riconoscimento del formato **dai byte** sta in `aether-net` e non qui: il
// recupero delle copertine dai servizi di metadati fa lo stesso identico
// controllo, per lo stesso identico motivo. Due copie avrebbero finito per
// riconoscere due elenchi diversi di formati, e il disaccordo si vedrebbe come
// un'immagine che l'anteprima mostra e la libreria rifiuta.
use aether_net::immagine::tipo as tipo_immagine;

/// Oltre questa dimensione l'immagine si lascia perdere.
///
/// Una copertina di Spotify sta in poche decine di kilobyte. Il limite non è una
/// stima generosa di quanto possano crescere: è il punto oltre il quale ha più
/// senso non mostrare niente che tenere in memoria — e in una stringa base64,
/// quindi di un terzo più grande — quel che ha risposto un indirizzo su cui non
/// abbiamo controllo.
const BYTE_MASSIMI: usize = 512 * 1024;

/// Scarica una copertina e la restituisce come `data:` URI.
///
/// `None` per qualunque intoppo — indirizzo non raggiungibile, risposta non
/// riuscita, immagine troppo grande, formato non riconosciuto. Non è un errore:
/// un'anteprima senza immagine è un'anteprima, e far fallire la lettura di una
/// playlist perché la sua miniatura non si scarica sarebbe assurdo.
pub fn scarica(rete: &Rete, url: &str) -> Option<String> {
    let byte = scarica_byte(rete, url)?;
    let tipo = tipo_immagine(&byte)?;
    Some(format!("data:{tipo};base64,{}", codifica(&byte)))
}

/// Scarica una copertina e restituisce i byte così come sono.
///
/// Serve a chi la deve **incorporare** in un file invece che mostrarla: i tag di
/// un brano scaricato vogliono l'immagine, non una stringa base64 da
/// ridecodificare. Stessi cancelli di [`scarica`] — solo `https`, un tetto alla
/// dimensione, e i byte devono somigliare a un'immagine — perché sono gli stessi
/// pericoli: un CDN che risponde con una pagina d'errore restituisce comunque
/// dei byte, e senza il controllo quelli finirebbero dentro il file per sempre.
pub fn scarica_byte(rete: &Rete, url: &str) -> Option<Vec<u8>> {
    // Solo `https`. Un indirizzo arriva da una risposta di Spotify, non
    // dall'utente, ma è pur sempre una stringa presa dalla rete che finisce in
    // una richiesta: `file:` o `http:` non hanno motivo di essere seguiti.
    if !url.starts_with("https://") {
        return None;
    }

    let risposta = rete
        .esegui(Richiesta {
            metodo: Metodo::Get,
            url,
            intestazioni: &[("Accept", "image/*")],
            corpo: Corpo::Niente,
        })
        .ok()?;
    if !risposta.e_andata() || risposta.corpo.len() > BYTE_MASSIMI {
        return None;
    }
    tipo_immagine(&risposta.corpo)?;
    Some(risposta.corpo)
}

/// L'alfabeto base64 standard.
const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Codifica in base64 standard, con il riempimento.
///
/// Scritta a mano per la stessa ragione del decodificatore in
/// [`crate::embed`]: sono venti righe usate in un punto solo, e il workspace non
/// ha altri motivi per portarsi una dipendenza.
fn codifica(byte: &[u8]) -> String {
    let mut fuori = String::with_capacity(byte.len().div_ceil(3) * 4);
    let simbolo = |sei: u32| -> char {
        // L'indice sta in 0..64 per costruzione: `sei` è mascherato a sei bit.
        char::from(*ALFABETO.get(sei as usize % 64).unwrap_or(&b'A'))
    };

    for blocco in byte.chunks(3) {
        let a = u32::from(*blocco.first().unwrap_or(&0));
        let b = u32::from(*blocco.get(1).unwrap_or(&0));
        let c = u32::from(*blocco.get(2).unwrap_or(&0));
        let unito = (a << 16) | (b << 8) | c;

        fuori.push(simbolo((unito >> 18) & 0x3f));
        fuori.push(simbolo((unito >> 12) & 0x3f));
        fuori.push(if blocco.len() > 1 {
            simbolo((unito >> 6) & 0x3f)
        } else {
            '='
        });
        fuori.push(if blocco.len() > 2 {
            simbolo(unito & 0x3f)
        } else {
            '='
        });
    }
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I vettori della RFC 4648, sezione 10. Provano tutti e tre i resti.
    #[test]
    fn i_vettori_canonici_della_rfc_4648() {
        assert_eq!(codifica(b""), "");
        assert_eq!(codifica(b"f"), "Zg==");
        assert_eq!(codifica(b"fo"), "Zm8=");
        assert_eq!(codifica(b"foo"), "Zm9v");
        assert_eq!(codifica(b"foob"), "Zm9vYg==");
        assert_eq!(codifica(b"fooba"), "Zm9vYmE=");
        assert_eq!(codifica(b"foobar"), "Zm9vYmFy");
    }

    /// I byte alti sono quelli su cui uno spostamento sbagliato si nota: se
    /// `codifica` trattasse i byte come `i8` o perdesse il bit più alto, questi
    /// tre non tornerebbero.
    #[test]
    fn i_byte_alti_non_si_perdono() {
        assert_eq!(codifica(&[0xff, 0xff, 0xff]), "////");
        assert_eq!(codifica(&[0x00, 0x00, 0x00]), "AAAA");
        assert_eq!(codifica(&[0xfb, 0xff, 0xbf]), "+/+/");
    }

    #[test]
    fn il_tipo_viene_dai_byte() {
        assert_eq!(tipo_immagine(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(
            tipo_immagine(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00]),
            Some("image/png")
        );
        assert_eq!(tipo_immagine(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    }

    /// Un `data:` con un tipo inventato è un `data:` che la finestra proverà a
    /// disegnare: se non si riconosce il formato non si dichiara niente.
    #[test]
    fn quel_che_non_e_unimmagine_non_diventa_una_data_uri() {
        assert_eq!(tipo_immagine(b"<!DOCTYPE html>"), None);
        assert_eq!(tipo_immagine(b""), None);
        assert_eq!(tipo_immagine(b"RIFF\0\0\0\0WAVEfmt "), None);
    }

    /// Un indirizzo non cifrato non si segue nemmeno per una miniatura.
    ///
    /// Non tocca la rete: i due indirizzi sono respinti prima della richiesta, e
    /// una prova che dipendesse da un guasto di collegamento proverebbe soltanto
    /// di essere offline.
    #[test]
    fn solo_https() {
        let rete = Rete::nuova("prova", std::time::Duration::from_secs(1));
        assert_eq!(scarica(&rete, "http://esempio.invalido/a.jpg"), None);
        assert_eq!(scarica(&rete, "file:///C:/segreto.jpg"), None);
    }
}
