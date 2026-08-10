//! I link corti del foglio di condivisione: `spotify.link/…`.
//!
//! # Perché non stanno in [`crate::url`]
//!
//! Perché non dicono niente. `spotify.link/2xY9aB` non contiene il genere né
//! l'identificativo: contiene un indirizzo a cui andare a chiedere. Riconoscerlo
//! è una richiesta di rete, e [`crate::url::riconosci`] è pura di proposito — è
//! quel che le permette di essere provata su cinquanta forme di link senza che
//! nessuna prova tocchi Stoccolma.
//!
//! # Perché contano
//!
//! Perché sono la forma che l'utente ha davvero negli appunti. Il tasto
//! «Condividi» dell'app del telefono produce questa, non l'indirizzo lungo, e
//! un'importazione che li rifiuta con «non è un link di Spotify» dà torto a chi
//! ha fatto tutto giusto.
//!
//! # Perché l'elenco degli ospiti è chiuso
//!
//! Perché questo modulo prende un indirizzo scritto da chi usa l'applicazione e
//! ci manda una richiesta. Finché gli ospiti ammessi sono tre e sono di Spotify,
//! è l'espansione di un link di Spotify; con un elenco aperto sarebbe un modo di
//! far bussare Aether dove dice qualcun altro, e non c'è nessun motivo per cui
//! debba poterlo fare.

use aether_net::http::{Corpo, Metodo, Rete, Richiesta};

use crate::url::{Riferimento, riconosci};

/// Gli ospiti che accorciano un link di Spotify.
///
/// `spotify.link` è quello dell'app di oggi; `spotify.app.link` è il servizio
/// che ci sta sotto e compare ancora nei messaggi vecchi; `link.tospotify.com`
/// è quello delle campagne e delle pagine «ascolta qui».
const OSPITI: [&str; 3] = ["spotify.link", "spotify.app.link", "link.tospotify.com"];

/// Quante volte si guarda dentro una pagina prima di lasciar perdere.
///
/// Una pagina di reindirizzamento nomina il suo contenuto una volta o due. Se
/// dopo venti occorrenze di `open.spotify.com` non se n'è riconosciuta nessuna,
/// non è una pagina di reindirizzamento: è un'altra cosa.
const CANDIDATI_MASSIMI: usize = 20;

/// Questo testo contiene un link corto di Spotify?
#[must_use]
pub fn e_scorciatoia(input: &str) -> bool {
    estrai_indirizzo(input).is_some()
}

/// Va a vedere dove porta, e riconosce quel che trova.
///
/// `None` per qualunque intoppo — nessuna rete, un 404, una pagina che non
/// nomina niente — perché chi chiama ha già il messaggio giusto da dare: «questo
/// non è un link di Spotify» è la stessa cosa che direbbe se non ci avessimo
/// nemmeno provato.
#[must_use]
pub fn espandi(rete: &Rete, input: &str) -> Option<Riferimento> {
    let indirizzo = estrai_indirizzo(input)?;
    let risposta = rete
        .esegui(Richiesta {
            metodo: Metodo::Get,
            url: &indirizzo,
            intestazioni: &[("Accept", "text/html")],
            corpo: Corpo::Niente,
        })
        .ok()?;

    // Il caso normale: `ureq` ha già seguito i salti, e `url_finale` è
    // l'indirizzo lungo.
    if let Some(riferimento) = riconosci(&risposta.url_finale) {
        return Some(riferimento);
    }
    // Il caso dell'atterraggio: certi link corti finiscono su una pagina che
    // rimanda avanti in JavaScript, e lì il salto non è nell'intestazione ma nel
    // testo.
    da_corpo(&risposta.testo())
}

/// L'indirizzo da chiedere, estratto dal testo incollato.
///
/// Funzione pura, così la si prova senza rete. Restituisce sempre uno `https://`
/// costruito da noi: quel che arriva può non avere schema, o averlo in chiaro, e
/// [`Rete`] rifiuta le richieste non cifrate.
#[must_use]
pub fn estrai_indirizzo(input: &str) -> Option<String> {
    let testo = input.trim();
    let minuscolo = testo.to_ascii_lowercase();

    for ospite in OSPITI {
        let mut da = 0_usize;
        while let Some(trovato) = minuscolo.get(da..).and_then(|resto| resto.find(ospite)) {
            let inizio = da.checked_add(trovato)?;
            let dopo = inizio.checked_add(ospite.len())?;
            if confine_prima(&minuscolo, inizio) && ha_percorso(&minuscolo, dopo) {
                let fine = testo
                    .get(inizio..)
                    .and_then(|resto| resto.find(char::is_whitespace))
                    .map_or(testo.len(), |quanto| inizio.saturating_add(quanto));
                let pezzo = testo.get(inizio..fine)?;
                return Some(format!("https://{pezzo}"));
            }
            da = dopo;
        }
    }
    None
}

/// Quel che sta prima dell'ospite non ne fa parte.
///
/// Senza, `nonspotify.link/x` passerebbe per un link corto di Spotify.
fn confine_prima(testo: &str, inizio: usize) -> bool {
    let Some(precedente) = inizio
        .checked_sub(1)
        .and_then(|dove| testo.as_bytes().get(dove))
    else {
        return true;
    };
    !(precedente.is_ascii_alphanumeric() || *precedente == b'.' || *precedente == b'-')
}

/// Dopo l'ospite c'è una barra e qualcosa: senza, non è un link corto.
///
/// È anche quel che tiene fuori `spotify.linkedin.example`, dove l'ospite
/// compare come prefisso di un nome più lungo.
fn ha_percorso(testo: &str, dopo: usize) -> bool {
    testo
        .get(dopo..)
        .and_then(|resto| resto.strip_prefix('/'))
        .is_some_and(|percorso| {
            percorso
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace() && c != '/')
        })
}

/// Il primo link lungo nominato dentro una pagina.
///
/// Funzione pura, per lo stesso motivo di [`estrai_indirizzo`].
#[must_use]
pub fn da_corpo(testo: &str) -> Option<Riferimento> {
    const ANCORA: &str = "open.spotify.com";
    let mut da = 0_usize;
    for _ in 0..CANDIDATI_MASSIMI {
        let trovato = testo.get(da..).and_then(|resto| resto.find(ANCORA))?;
        let inizio = da.checked_add(trovato)?;
        if let Some(riferimento) = testo.get(inizio..).and_then(riconosci) {
            return Some(riferimento);
        }
        da = inizio.checked_add(ANCORA.len())?;
    }
    None
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::spotify::SpotifyKind;

    #[test]
    fn i_link_corti_si_riconoscono_in_tutte_le_forme() {
        assert_eq!(
            estrai_indirizzo("https://spotify.link/2xY9aBcDeF"),
            Some("https://spotify.link/2xY9aBcDeF".to_owned())
        );
        assert_eq!(
            estrai_indirizzo("  spotify.link/2xY9aBcDeF\n"),
            Some("https://spotify.link/2xY9aBcDeF".to_owned()),
            "senza schema: è quel che si ottiene copiando a mano"
        );
        assert_eq!(
            estrai_indirizzo("http://spotify.app.link/abc"),
            Some("https://spotify.app.link/abc".to_owned()),
            "in chiaro non si esce: lo schema lo rimettiamo noi"
        );
        assert_eq!(
            estrai_indirizzo("Ascolta questa https://spotify.link/abc123 ciao"),
            Some("https://spotify.link/abc123".to_owned()),
            "dentro una frase, e si ferma al primo spazio"
        );
    }

    #[test]
    fn quel_che_non_e_un_link_corto_non_lo_diventa() {
        assert_eq!(estrai_indirizzo(""), None);
        assert_eq!(estrai_indirizzo("https://open.spotify.com/track/x"), None);
        assert_eq!(
            estrai_indirizzo("https://spotify.link"),
            None,
            "un ospite senza percorso non nomina niente"
        );
        assert_eq!(
            estrai_indirizzo("https://spotify.link/"),
            None,
            "e nemmeno una barra vuota"
        );
        assert_eq!(
            estrai_indirizzo("https://nonspotify.link/abc"),
            None,
            "l'ospite deve cominciare dove diciamo noi"
        );
        assert_eq!(
            estrai_indirizzo("https://spotify.linkedin.example/abc"),
            None,
            "e finire dove diciamo noi"
        );
        assert!(!e_scorciatoia(
            "https://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3"
        ));
        assert!(e_scorciatoia("spotify.link/2xY9aBcDeF"));
    }

    #[test]
    fn una_pagina_di_atterraggio_nomina_il_link_lungo() {
        let html = r#"<html><head><meta property="og:url"
            content="https://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3?si=x">
            </head><body>Apertura…</body></html>"#;
        assert_eq!(
            da_corpo(html),
            Some(Riferimento {
                genere: SpotifyKind::Album,
                id: "1DFixLWuPkv3KT3TnV35m3".to_owned()
            })
        );
    }

    #[test]
    fn le_occorrenze_che_non_nominano_niente_si_scavalcano() {
        // La prima è il nome del sito, la seconda è il contenuto. Fermarsi alla
        // prima vorrebbe dire non trovare mai niente su questa pagina.
        let html = r#"<a href="https://open.spotify.com/">Spotify</a>
            <link rel="canonical" href="https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT">"#;
        assert_eq!(
            da_corpo(html),
            Some(Riferimento {
                genere: SpotifyKind::Track,
                id: "4cOdK2wGLETKBW3PvgPWqT".to_owned()
            })
        );
        assert_eq!(da_corpo("<html>niente</html>"), None);
    }
}
