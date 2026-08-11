//! Lo scrobbling: dire a un servizio esterno cosa si è ascoltato.
//!
//! Due protocolli, e sono diversi fin nella filosofia. ListenBrainz vuole un
//! token e un documento JSON; Last.fm vuole una chiave, un segreto, un consenso
//! nel browser e una firma MD5 su ogni richiesta. Provare a nasconderli dietro
//! un'unica astrazione «servizio di scrobbling» costerebbe più di quel che
//! renderebbe: quel che davvero hanno in comune è **l'ascolto da mandare**, che
//! infatti non è definito qui — sta in [`aether_domain::scrobble`], dove i due
//! crate che non devono conoscersi si incontrano.
//!
//! # La regola di cosa conta non sta qui
//!
//! Sta in [`aether_domain::listen::counts_as_play`], dove sta da sempre: metà
//! brano o quattro minuti, quel che viene prima, mai sotto i trenta secondi. È
//! la regola di Last.fm, ed è la stessa che ListenBrainz raccomanda parola per
//! parola nella sua documentazione — il che è comodo, ma non è il motivo per
//! cui si riusa. Il motivo è che `play_count`, la cronologia e lo scrobble
//! devono contare **la stessa cosa**: due misure dello stesso ascolto che non
//! concordano sono esattamente il difetto che `listen.rs` è nato per correggere,
//! e riscriverne una terza qui lo reintrodurrebbe dalla porta di servizio.
//!
//! Questo crate quindi non decide mai se un ascolto conta. Riceve ascolti che
//! contano già.
//!
//! # Perché non conosce il database
//!
//! Stessa regola di `aether-net`, `aether-spotify` e `aether-cloud`: chi parla
//! col mondo non vede `rusqlite`. Qui però la regola paga un dividendo
//! specifico, e vale la pena dirlo: la coda dello scrobbling si svuota mille
//! ascolti alla volta, con richieste che possono durare minuti se il servizio è
//! lento. Se questo crate potesse toccare la connessione, la cosa naturale da
//! scrivere sarebbe «leggi una riga, mandala, cancellala» — cioè tenere il
//! lucchetto della libreria per tutta la durata della rete, con la riproduzione
//! ferma dietro. Non potendo, la forma che resta è l'unica giusta: si legge un
//! blocco, si chiude, si manda, si riapre per cancellare.
//!
//! # Cosa non finisce mai in un errore
//!
//! Il token di ListenBrainz, il segreto di Last.fm, la chiave di sessione e il
//! token in attesa del consenso. Nelle cause finiscono il codice numerico che
//! Last.fm restituisce e il suo messaggio — che servono a capire cosa è
//! successo e non sono segreti. Una [`firma`](lastfm::firma) non è ricostruibile
//! senza il segreto, ma non c'è ragione di scriverla in un log e non ci finisce.

pub mod lastfm;
pub mod listenbrainz;

pub use aether_domain::scrobble::{Ascolto, Servizio};
pub use lastfm::LastFm;
pub use listenbrainz::ListenBrainz;

/// Quanti ascolti stanno in una sola richiesta a questo servizio.
///
/// Una funzione qui e non un metodo su [`Servizio`]: è un limite di protocollo,
/// e il dominio non conosce i protocolli. Mille contro cinquanta non è una
/// differenza di stile — su una cronologia di Spotify da quarantamila righe sono
/// quaranta richieste contro ottocento, cioè la differenza fra un'importazione
/// di un minuto e una di mezz'ora.
#[must_use]
pub const fn per_richiesta(servizio: Servizio) -> usize {
    match servizio {
        Servizio::ListenBrainz => listenbrainz::MASSIMI_PER_RICHIESTA,
        Servizio::LastFm => lastfm::MASSIMI_PER_RICHIESTA,
    }
}

/// Come Aether si presenta ai servizi di scrobbling.
///
/// ListenBrainz lo chiede esplicitamente (`submission_client`) e lo mostra
/// accanto agli ascolti: serve a chi guarda la propria pagina a capire da quale
/// programma sia arrivata una riga, e a chi gestisce il servizio a scrivere a
/// qualcuno quando un client si comporta male.
pub const CLIENT: &str = "Aether";

/// La versione dichiarata, presa dal manifesto.
pub const VERSIONE: &str = env!("CARGO_PKG_VERSION");

/// Esadecimale minuscolo, senza separatori.
///
/// Serve alla firma di Last.fm, che è un MD5 scritto così. Sta qui e non in
/// `lastfm.rs` perché è una conversione di byte, non una regola di protocollo.
fn esadecimale(byte: &[u8]) -> String {
    use std::fmt::Write as _;
    byte.iter().fold(String::new(), |mut testo, b| {
        // `write!` su una `String` non può fallire: l'unica via d'errore di
        // `fmt::Write` è quella dell'implementazione, e quella di `String`
        // restituisce sempre `Ok`. Si ignora invece di `unwrap`, che qui è
        // vietato.
        let _ = write!(testo, "{b:02x}");
        testo
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn i_limiti_per_richiesta_sono_quelli_dei_due_protocolli() {
        assert_eq!(per_richiesta(Servizio::ListenBrainz), 1000);
        assert_eq!(per_richiesta(Servizio::LastFm), 50);
    }

    #[test]
    fn l_esadecimale_e_minuscolo_e_a_due_cifre() {
        assert_eq!(esadecimale(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
        assert_eq!(esadecimale(&[]), "");
    }
}
