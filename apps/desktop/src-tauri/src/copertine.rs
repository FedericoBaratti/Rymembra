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

fn risposta(status: u16, tipo: &str, corpo: Vec<u8>) -> http::Response<Vec<u8>> {
    // Il contenuto è indirizzato dall'impronta: gli stessi byte daranno
    // sempre lo stesso indirizzo, quindi non c'è niente da rivalidare.
    risposta_con(status, tipo, corpo, "public, max-age=31536000, immutable")
}

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
    tauri::async_runtime::spawn_blocking(move || {
        match leggi(&copertine, &impronta, misura) {
            Some(bytes) => responder.respond(risposta(200, "image/jpeg", bytes)),
            // Una copertina che manca non è un guasto: è un album senza
            // immagine, e l'interfaccia ha già un ripiego da mostrare.
            None => responder.respond(risposta(404, "text/plain", Vec::new())),
        }
    });
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
