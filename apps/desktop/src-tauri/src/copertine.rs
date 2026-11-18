//! Servire le copertine alla finestra.
//!
//! Un protocollo dedicato invece di mandare i byte attraverso l'IPC. La
//! differenza non è di stile: una griglia di novecento album chiederebbe
//! novecento immagini, e passarle come JSON in base64 significherebbe
//! serializzarle, gonfiarle di un terzo, e tenerle tutte vive in memoria nel
//! processo della finestra. Con un protocollo le chiede il motore di rendering
//! come chiederebbe qualunque immagine: in parallelo, con la sua cache, e
//! scartando quelle che escono dallo schermo.
//!
//! L'indirizzo è `rymembra-cover://localhost/<impronta>` per la copertina piena,
//! `rymembra-cover://localhost/<impronta>.m` per la media (le schede delle
//! griglie, vedi `MAX_LATO_MEDIA` nel nucleo) e
//! `rymembra-cover://localhost/<impronta>.t` per la miniatura.
//!
//! La media è l'unica che può non esserci ancora: nasce dalla piena alla prima
//! richiesta, ed è per questo che qui si passa da [`CoverStore::media`] invece
//! che da una lettura di file come per le altre due.
//!
//! # Perché l'impronta e non il percorso
//!
//! Un protocollo che accettasse un percorso sarebbe una lettura arbitraria di
//! file: la finestra chiede `../../qualcosa` e il processo nativo glielo legge.
//! Qui l'unico argomento è un'impronta, e il percorso lo compone lo store. Una
//! richiesta con dentro un separatore non è un percorso da normalizzare — è una
//! richiesta che non può essere legittima, e si rifiuta.

use rymembra_app::covers::CoverStore;
use tauri::http;
use tauri::{Manager as _, UriSchemeContext, UriSchemeResponder};

use crate::stato::Stato;

/// Quale delle tre misure chiede un indirizzo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Misura {
    /// `<impronta>.t`: le righe degli elenchi.
    Miniatura,
    /// `<impronta>.m`: le schede delle griglie.
    Media,
    /// `<impronta>`: dove la copertina è il soggetto.
    Piena,
}

/// L'impronta e la misura, dal percorso dell'indirizzo.
///
/// Solo la separazione: se quel che resta sia un'impronta lo dice
/// [`e_un_impronta`], dopo. Un suffisso sconosciuto — `.x` — resta attaccato
/// all'impronta, e il controllo dell'alfabeto lo rifiuta per via del punto.
fn misura_da(percorso: &str) -> (&str, Misura) {
    if let Some(impronta) = percorso.strip_suffix(".t") {
        (impronta, Misura::Miniatura)
    } else if let Some(impronta) = percorso.strip_suffix(".m") {
        (impronta, Misura::Media)
    } else {
        (percorso, Misura::Piena)
    }
}

/// Un'impronta è esadecimale e basta.
///
/// Il controllo è sull'alfabeto e non sui separatori: elencare i caratteri
/// vietati è una lista da tenere aggiornata (`/`, `\`, `..`, `%2e`, gli
/// equivalenti Unicode…), elencare quelli permessi no.
///
/// Sta qui e non è privata perché il protocollo non è l'unica porta d'ingresso:
/// anche [`crate::skin::accento_copertina`] riceve un'impronta dalla finestra e
/// la dà allo store, che con essa compone un percorso. Due controlli scritti
/// due volte sono due controlli che possono divergere, e quello che divergesse
/// sarebbe una lettura di file arbitraria.
pub(crate) fn e_un_impronta(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Il tipo di un'immagine dai suoi primi byte: JPEG, PNG o WebP, le sole che
/// una copertina di un altro dispositivo può essere (decisione D-32). `None`
/// per tutto il resto.
pub(crate) fn tipo_immagine(byte: &[u8]) -> Option<&'static str> {
    if byte.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if byte.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if byte.starts_with(b"RIFF") && byte.get(8..12) == Some(b"WEBP".as_slice()) {
        Some("image/webp")
    } else {
        None
    }
}

fn risposta(status: u16, tipo: &str, corpo: Vec<u8>) -> http::Response<Vec<u8>> {
    risposta_con(status, tipo, corpo, cache_per(status))
}

/// L'intestazione di cache di una risposta del protocollo.
///
/// Un'immagine è indirizzata dall'impronta: gli stessi byte daranno sempre lo
/// stesso indirizzo, quindi non c'è niente da rivalidare. Un errore invece no:
/// il 503 di una libreria non ancora aperta o il 404 di una lettura caduta (la
/// cartella dati su una share che sparisce per qualche secondo) sono guasti
/// passeggeri, e la finestra l'indirizzo di una copertina non lo cambia mai.
/// Con «immutable» WebView2 li terrebbe per un anno, e quell'album resterebbe
/// senza copertina anche col file di nuovo leggibile: è lo stesso «non c'è»
/// ricordato che le copertine di sessione evitano con `no-store` (vedi
/// [`servi`]).
fn cache_per(status: u16) -> &'static str {
    if status == 200 {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    }
}

/// La risposta per una copertina di un altro dispositivo collegato.
///
/// Mai in cache nella finestra, nemmeno quando c'è: la copertina si serve solo
/// dietro il cancello del collegamento (vedi `lontano::copertina`), e una
/// WebView che la tenesse per un anno («immutable», come le copertine di qui)
/// continuerebbe a mostrarla dopo «Dimentica», senza ripassare dal cancello.
/// La cache che conta sta nel nucleo, per dispositivo e dietro il cancello:
/// rifare la domanda costa una lettura dal disco.
fn risposta_lontana(esito: Result<Option<Vec<u8>>, ()>) -> http::Response<Vec<u8>> {
    let senza_cache = "no-store";
    match esito {
        // Il tipo dai byte, che `lontano::copertina` ha già riconosciuto come
        // un'immagine: con `nosniff` la finestra crede a quel che si dice.
        Ok(Some(byte)) => {
            let tipo = tipo_immagine(&byte).unwrap_or("application/octet-stream");
            risposta_con(200, tipo, byte, senza_cache)
        }
        Ok(None) => risposta_con(404, "text/plain", Vec::new(), senza_cache),
        Err(()) => risposta_con(503, "text/plain", Vec::new(), senza_cache),
    }
}

/// L'unico costruttore delle risposte del protocollo. `nosniff` su ognuna,
/// come `skin::risposta_risorsa`: la finestra non indovina un tipo diverso da
/// quello detto, nemmeno il giorno in cui un ramo restituisse un tipo che
/// Chromium fiuta (reperto R10-1, il 09-4 annunciato e mai scritto).
fn risposta_con(
    status: u16,
    tipo: &str,
    corpo: Vec<u8>,
    cache: &'static str,
) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, tipo)
        .header(http::header::CACHE_CONTROL, cache)
        .header(http::header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(corpo)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// Il prefisso delle chiavi delle copertine di sessione: `insieme-<uid>`.
///
/// Vedi `riproduzione::ospiti::SchedaVoce::copertina`.
const PREFISSO_SESSIONE: &str = "insieme-";

/// Una chiave di copertina di sessione: il prefisso e un numero, nient'altro.
///
/// Lo stesso principio di [`e_un_impronta`] — l'alfabeto permesso, non i
/// caratteri vietati — con un alfabeto ancora più stretto: solo cifre. La
/// chiave non diventa mai un percorso (le copertine di sessione stanno in
/// memoria, vedi `crate::insieme::copertina`), ma una regola stretta costa
/// niente e toglie la domanda.
fn e_una_chiave_di_sessione(chiave: &str) -> bool {
    chiave
        .strip_prefix(PREFISSO_SESSIONE)
        .is_some_and(|numero| {
            !numero.is_empty() && numero.len() <= 20 && numero.bytes().all(|b| b.is_ascii_digit())
        })
}

/// Risponde a una richiesta di copertina.
///
/// # Le copertine di sessione
///
/// Una voce d'«Ascolta insieme» che arriva da un altro PC ha la copertina in
/// memoria, scaricata e decodificata dalla sessione, con la chiave
/// `insieme-<uid>`. Si guardano **prima** del controllo dell'impronta, che il
/// trattino lo rifiuterebbe, e non passano mai dallo store delle copertine:
/// lì la pulizia delle orfane le cancellerebbe, e una copertina di un altro PC
/// sporcherebbe la libreria di questo. Le tre misure sono la stessa immagine:
/// è una sola per voce, e al più un mebibyte.
pub fn servi(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let percorso = request.uri().path().trim_start_matches('/').to_owned();
    let (impronta, misura) = misura_da(&percorso);
    let impronta = impronta.to_owned();

    if e_una_chiave_di_sessione(&impronta) {
        // In memoria: nessun disco da aspettare, nessun filo da prestare.
        // Mai in cache: la chiave è un numero di voce, e i numeri ricominciano
        // a ogni sessione. Con «immutable» la voce 5 della sessione dopo
        // mostrava la copertina della voce 5 di quella prima — o il «non c'è»
        // di quando la copertina non era ancora arrivata.
        let senza_cache = "no-store";
        match crate::insieme::copertina(ctx.app_handle(), &impronta) {
            Some(byte) => {
                responder.respond(risposta_con(200, "image/jpeg", byte.to_vec(), senza_cache))
            }
            None => responder.respond(risposta_con(404, "text/plain", Vec::new(), senza_cache)),
        }
        return;
    }

    if crate::lontano::e_una_copertina(&impronta) {
        // Da un altro dispositivo collegato: la rete, quindi il pool
        // bloccante, mai il filo della finestra. Le tre misure sono la media
        // dell'altro. Niente cache nella finestra: vedi [`risposta_lontana`].
        let app = ctx.app_handle().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let esito = crate::lontano::copertina(&app, &impronta).map_err(|_| ());
            responder.respond(risposta_lontana(esito));
        });
        return;
    }

    if !e_un_impronta(&impronta) {
        responder.respond(risposta(400, "text/plain", b"impronta non valida".to_vec()));
        return;
    }

    let app = ctx.app_handle();
    let Some(stato) = app.try_state::<Stato>() else {
        responder.respond(risposta(503, "text/plain", b"libreria non aperta".to_vec()));
        return;
    };

    let Ok(copertine) = stato.copertine() else {
        responder.respond(risposta(503, "text/plain", b"libreria non aperta".to_vec()));
        return;
    };

    // Nessun lucchetto è stato preso, e la lettura va comunque su un altro
    // thread. Le due cose sono separate e servono a due guasti diversi.
    //
    // Il lucchetto: WebView2 dispatcha questo gestore sul filo della finestra, e
    // una griglia di novecento album lo chiama novecento volte. Chiederlo qui
    // vorrebbe dire che ogni immagine si mette in coda dietro chiunque stia
    // usando il database — e mentre una scansione legge una condivisione di rete
    // vorrebbe dire una finestra che il sistema dichiara «non risponde», per
    // calcolare un nome di file da una cartella che non cambia mai. Da qui in
    // giù non c'è più niente che il mutex debba proteggere: [`CoverStore`] è un
    // percorso, e il percorso è già stato copiato.
    //
    // Il thread: la lettura è un `read` su disco, e questo resta il filo della
    // finestra. Non deve tenere fermo né lo stato né chi ha mandato la richiesta.
    // Per la media vale il doppio: la prima volta è una decodifica e una
    // ricodifica, qualche millisecondo per copertina e trenta copertine per
    // schermata.
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        match leggi(&copertine, &impronta, misura)
            .or_else(|| dal_dispositivo(&app, &copertine, &impronta, misura))
        {
            Some(bytes) => responder.respond(risposta(200, "image/jpeg", bytes)),
            // Una copertina che manca non è un guasto: è un album senza
            // immagine, e l'interfaccia ha già un ripiego da mostrare.
            None => responder.respond(risposta(404, "text/plain", Vec::new())),
        }
    });
}

/// La copertina di una riga della libreria unica che qui non c'è ancora: la si
/// chiede a un dispositivo che risponde e la si tiene nello store sotto la sua
/// impronta (`lontano::specchio::copertina_di_una_riga`); poi si rilegge nella
/// misura chiesta. `None` se nessun dispositivo che risponde ce l'ha.
fn dal_dispositivo(
    app: &tauri::AppHandle,
    copertine: &CoverStore,
    impronta: &str,
    misura: Misura,
) -> Option<Vec<u8>> {
    match crate::lontano::copertina_di_una_riga(app, impronta) {
        Ok(Some(_)) => leggi(copertine, impronta, misura),
        Ok(None) => None,
        Err(guasto) => {
            crate::nota!(
                "[copertine] una copertina di un altro dispositivo non arriva codice={}",
                guasto.code().kind().code()
            );
            None
        }
    }
}

/// I byte di una misura, o `None` se non ci sono.
///
/// Una media che non si riesce a fare — la piena illeggibile, la cartella in
/// sola lettura — vale come una copertina che manca: la finestra mostra il
/// segnaposto, e il motivo va nel diario invece che in un avviso rosso per
/// un quadratino.
fn leggi(copertine: &CoverStore, impronta: &str, misura: Misura) -> Option<Vec<u8>> {
    match misura {
        Misura::Miniatura => std::fs::read(copertine.thumbnail_path_for(impronta)).ok(),
        Misura::Piena => std::fs::read(copertine.path_for(impronta)).ok(),
        Misura::Media => match copertine.media(impronta) {
            Ok(byte) => byte,
            Err(guasto) => {
                crate::nota!("[copertine] la media di {impronta} non si fa: {guasto}");
                None
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_le_impronte_passano() {
        assert!(e_un_impronta("a3f9"));
        assert!(e_un_impronta(&"0".repeat(64)));
        // Tutto il resto è una richiesta che non può essere legittima.
        assert!(!e_un_impronta(""));
        assert!(!e_un_impronta("../../etc/passwd"));
        assert!(!e_un_impronta("a3f9/../x"));
        assert!(!e_un_impronta("a3f9.jpg"));
        assert!(!e_un_impronta("C:\\Windows"));
        assert!(!e_un_impronta(&"a".repeat(129)));
    }

    #[test]
    fn solo_le_chiavi_di_sessione_vere_vanno_alla_sessione() {
        assert!(e_una_chiave_di_sessione("insieme-1"));
        assert!(e_una_chiave_di_sessione("insieme-18446744073709551615"));
        // Le misure si staccano prima: una chiave con il suffisso arriva qui
        // senza.
        let (chiave, misura) = misura_da("insieme-42.t");
        assert_eq!(misura, Misura::Miniatura);
        assert!(e_una_chiave_di_sessione(chiave));
        // Tutto il resto no, e non diventa nemmeno un'impronta.
        for falsa in [
            "insieme-",
            "insieme-x1",
            "insieme-1/../2",
            "insieme--1",
            "insieme-1.jpg",
            "insieme-123456789012345678901",
            "Insieme-1",
            "a3f9",
        ] {
            assert!(!e_una_chiave_di_sessione(falsa), "{falsa}");
        }
        assert!(!e_un_impronta("insieme-1"));
        // Le copertine degli altri dispositivi hanno la loro porta, e il
        // controllo dell'impronta non le lascerebbe passare.
        let lontana = format!("lontano-{}-0123456789abcdef", "a".repeat(64));
        assert!(crate::lontano::e_una_copertina(&lontana));
        assert!(!e_un_impronta(&lontana));
    }

    #[test]
    fn la_copertina_di_un_altro_dispositivo_non_resta_nella_finestra() {
        // Si serve solo dietro il cancello del collegamento: una WebView che la
        // tenesse per un anno la mostrerebbe anche dopo «Dimentica».
        let cache = |r: &http::Response<Vec<u8>>| {
            r.headers()
                .get(http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        for esito in [Ok(Some(vec![1, 2, 3])), Ok(None), Err(())] {
            let r = risposta_lontana(esito);
            assert_eq!(cache(&r), "no-store", "stato {}", r.status());
        }
    }

    #[test]
    fn ogni_risposta_dice_nosniff() {
        // Reperto R10-1 (il 09-4 annunciato e mai scritto): nessuna risposta
        // di `rymembra-cover` diceva `X-Content-Type-Options: nosniff`.
        let nosniff = |r: &http::Response<Vec<u8>>| {
            r.headers()
                .get(http::header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok())
                == Some("nosniff")
        };
        let mut risposte = Vec::new();
        for status in [200, 400, 404, 503] {
            risposte.push(risposta(status, "text/plain", Vec::new()));
        }
        for esito in [Ok(Some(vec![0xFF, 0xD8, 0xFF, 0xE0])), Ok(None), Err(())] {
            risposte.push(risposta_lontana(esito));
        }
        risposte.push(risposta_con(200, "image/jpeg", vec![1], "no-store"));
        for r in &risposte {
            assert!(nosniff(r), "stato {}", r.status());
        }
    }

    #[test]
    fn le_copertine_degli_altri_sono_solo_immagini() {
        // Decisione D-32: JPEG, PNG e WebP, riconosciuti dai byte.
        assert_eq!(
            tipo_immagine(&[0xFF, 0xD8, 0xFF, 0xE0, 0]),
            Some("image/jpeg")
        );
        assert_eq!(tipo_immagine(b"\x89PNG\r\n\x1a\n\0\0"), Some("image/png"));
        assert_eq!(tipo_immagine(b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
        for altro in [
            b"".as_slice(),
            b"\xFF\xD8",
            b"<html><script>x</script></html>",
            b"GIF89a",
            b"RIFF\x10\0\0\0WAVEfmt ",
            b"RIFF",
            b"%PDF-1.7",
        ] {
            assert_eq!(tipo_immagine(altro), None, "{altro:?}");
        }
        // E la risposta dice il tipo vero.
        let png = risposta_lontana(Ok(Some(b"\x89PNG\r\n\x1a\n".to_vec())));
        assert_eq!(
            png.headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("image/png")
        );
    }

    #[test]
    fn gli_errori_del_protocollo_non_vanno_in_cache() {
        let cache = |r: &http::Response<Vec<u8>>| {
            r.headers()
                .get(http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        let buona = risposta(200, "image/jpeg", vec![1, 2, 3]);
        assert!(cache(&buona).contains("immutable"), "{}", cache(&buona));
        // Un guasto passeggero (la libreria non ancora aperta, la cartella dati
        // su una share caduta) non deve restare per un anno: la finestra non
        // cambia mai l'indirizzo di una copertina.
        for status in [400, 404, 503] {
            let guasto = risposta(status, "text/plain", Vec::new());
            assert_eq!(cache(&guasto), "no-store", "stato {status}");
        }
    }

    #[test]
    fn il_suffisso_dice_la_misura() {
        assert_eq!(misura_da("a3f9"), ("a3f9", Misura::Piena));
        assert_eq!(misura_da("a3f9.t"), ("a3f9", Misura::Miniatura));
        assert_eq!(misura_da("a3f9.m"), ("a3f9", Misura::Media));
        // Un suffisso che non è nostro resta attaccato, e il punto lo fa
        // rifiutare dal controllo dell'impronta.
        let (impronta, misura) = misura_da("a3f9.x");
        assert_eq!(misura, Misura::Piena);
        assert!(!e_un_impronta(impronta));
        // Due suffissi non sono una misura.
        let (impronta, _) = misura_da("a3f9.m.t");
        assert!(!e_un_impronta(impronta));
    }

    #[test]
    fn le_tre_misure_si_leggono_dallo_store() {
        let dir = tempfile::tempdir().expect("cartella");
        let store = CoverStore::open(dir.path()).expect("store");
        // Un PNG vero da due per due, scritto a byte: questo crate non dipende
        // da `image`, e per provare che le tre misure si leggono non serve
        // un'immagine grande — serve che lo store la sappia decodificare.
        const PNG: [u8; 75] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00,
            0x00, 0xFD, 0xD4, 0x9A, 0x73, 0x00, 0x00, 0x00, 0x12, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x38, 0x91, 0x62, 0x74, 0x22, 0xC5, 0x88, 0x01, 0x42, 0x01, 0x00, 0x28,
            0xAE, 0x05, 0x79, 0xA2, 0x98, 0x47, 0x51, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
            0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let salvata = store
            .store(&PNG, rymembra_app::covers::CoverSource::Tag)
            .expect("salvataggio");
        for misura in [Misura::Miniatura, Misura::Media, Misura::Piena] {
            assert!(leggi(&store, &salvata.hash, misura).is_some(), "{misura:?}");
        }
        // La media è nata alla richiesta, accanto alle altre due.
        assert!(store.percorso_media(&salvata.hash).exists());
        // Un'impronta senza file: niente, per tutte e tre.
        let assente = "cd".repeat(32);
        for misura in [Misura::Miniatura, Misura::Media, Misura::Piena] {
            assert!(leggi(&store, &assente, misura).is_none(), "{misura:?}");
        }
    }
}
