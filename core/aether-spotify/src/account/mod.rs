//! L'account intero, letto dalla Web API dopo un consenso OAuth.
//!
//! # Questo crate non è più solo keyless, ed è una scelta da dichiarare
//!
//! Il preambolo di [`crate`] racconta un lettore che risolve un link pubblico
//! **senza chiavi, senza account, senza registrare niente**. Resta vero: è
//! [`crate::risolvi`], ed è la via che copre il caso di gran lunga più comune.
//!
//! Questo modulo è l'altra cosa, e chiede esattamente ciò che quello evitava.
//! Vale la pena dire perché, invece di lasciarlo scoprire a chi legge il
//! `Cargo.toml` fra sei mesi:
//!
//! - **Il keyless non può leggere un account.** I livelli in cascata leggono
//!   quel che è pubblico, e «le mie playlist private» non lo è per definizione.
//!   Non è un limite dell'implementazione: è la differenza fra guardare una
//!   pagina e essere qualcuno.
//! - **La Web API dà l'ISRC.** Tolto nel febbraio 2026 e rimesso a marzo, e il
//!   lettore keyless non lo vede più da nessun livello. È il gradino zero
//!   dell'abbinamento — stessa registrazione senza guardare i nomi — e oggi è
//!   sempre a secco. Questa è la prima via che glielo fa arrivare davvero.
//! - **E in cambio dipende da un abbonamento.** Dal febbraio 2026
//!   un'applicazione in Development Mode richiede che il **proprietario** abbia
//!   Spotify Premium attivo; quando scade, smette di funzionare e Spotify non
//!   avvisa nessuno. Accetta cinque utenti. È il motivo per cui l'archivio resta
//!   la seconda via e non un ripiego: quella non chiede niente a nessuno.
//!
//! # Cosa produce, e perché è tutto quel che deve produrre
//!
//! Un [`AccountSnapshot`], lo stesso valore che `aether-archivio` ricava da uno
//! zip. Da lì in giù — l'abbinamento, il piano, la scrittura, la coda di
//! scaricamento — il codice è **uno solo**, e già scritto. Quel che resta a
//! questo modulo è riempire una struttura.
//!
//! # La disciplina di sempre
//!
//! Qui dentro non passa nessuna `rusqlite::Connection`. Chi legge un account
//! parla con la rete per minuti — duecento playlist sono duecento richieste — e
//! il verso della dipendenza è quel che rende impossibile, e non solo
//! sconsigliato, tenere preso il lucchetto della libreria per tutto quel tempo.

pub mod oauth;
pub mod portachiavi;
pub mod web;

use aether_domain::errors::AppError;
use aether_domain::spotify_account::{AccountSnapshot, Provenienza};
use aether_net::Rete;

pub use oauth::{ATTESA_CONSENSO, Invito, SCADENZA, SCOPE, SPOTIFY, Token};
pub use web::{Cliente, Contenuto, Letto, Profilo};

/// Le date, in comune con il lettore dell'archivio.
pub use aether_domain::tempo;

/// A che punto è la lettura.
///
/// Esiste perché leggere un account **è lento** — un account da duecento
/// playlist è duecento richieste, e nessuna barra indeterminata rende quel tempo
/// sopportabile. Il piano prevede un evento `account:avanzamento`, e questo è il
/// suo contenuto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avanzamento {
    /// Cosa si sta leggendo: `profilo`, `preferiti`, `album`, `playlist`,
    /// `artisti`, `cronologia`.
    pub fase: &'static str,
    /// Il nome di quel che si sta leggendo adesso, quando ne ha uno.
    pub nome: Option<String>,
    /// Quanti passi fatti dentro questa fase.
    pub fatti: usize,
    /// Su quanti, quando si sa. Le fasi che sono un passo solo dicono `Some(1)`.
    pub totali: Option<usize>,
}

/// Quel che si è potuto leggere, e quel che no.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lettura {
    /// Il valore che l'importazione consuma.
    pub snapshot: AccountSnapshot,
    /// L'account ha Premium? `None` quando Spotify non l'ha detto.
    ///
    /// Vedi [`Profilo::premium`]: serve a dare **prima** l'avviso che
    /// altrimenti arriverebbe come un guasto senza spiegazione.
    pub premium: Option<bool>,
    /// Le playlist di cui Spotify non dà più il contenuto.
    ///
    /// Dal marzo 2026 i brani si leggono solo per le playlist che l'utente
    /// possiede o in cui collabora: quelle che segue e basta arrivano con nome e
    /// totale, e niente dentro. Vanno **dette** — «importate 40 playlist su 62»
    /// è un'informazione, e una playlist vuota senza spiegazione sembra un
    /// guasto dell'abbinamento.
    pub senza_contenuto: Vec<String>,
    /// Quante voci erano puntate di podcast.
    pub podcast: usize,
    /// Gli elenchi arrivati a metà perché più lunghi di quanto si legga.
    ///
    /// Vuoto su qualunque account vero — il tetto è diecimila voci per elenco —
    /// e non vuoto vuol dire che si sta importando **meno** di quel che c'è.
    /// Un'importazione monca che non lo dice è il guasto peggiore possibile qui,
    /// ed è la stessa ragione per cui `PlaylistSpotify::dichiarati` esiste.
    pub troncati: Vec<&'static str>,
}

/// Legge tutto l'account leggibile.
///
/// `avanzamento` viene chiamato prima di ogni passo. Chi non ha una finestra
/// passi `|_| {}`.
///
/// # Un guasto su una playlist non ferma le altre
///
/// Il `403` su una playlist che l'utente non possiede è la risposta **normale**
/// dal marzo 2026, non un guasto: si annota il nome e si prosegue. Gli altri
/// errori — un token scaduto, la quota finita, la rete che se ne va — fermano
/// tutto, perché riguardano la lettura e non quella playlist.
///
/// # Errori
///
/// `spotify.accountAuthExpired` se il token non vale più,
/// `spotify.accountForbidden` se l'account non è abilitato all'applicazione,
/// `spotify.quotaExceeded` se la quota del giorno è finita, `net.*` per la rete.
pub fn leggi(
    rete: &Rete,
    access_token: &str,
    mut avanzamento: impl FnMut(&Avanzamento),
) -> Result<Lettura, AppError> {
    let cliente = Cliente::nuovo(rete, access_token);
    let mut snapshot = AccountSnapshot::vuoto(Provenienza::Api);
    let mut lettura_podcast = 0_usize;
    let mut senza_contenuto = Vec::new();
    let mut troncati: Vec<&'static str> = Vec::new();

    avanzamento(&passo("profilo", None, 0, Some(1)));
    let profilo = cliente.profilo()?;
    snapshot.profilo = profilo.nome.clone();
    snapshot.spotify_user_id = profilo.id.clone();

    avanzamento(&passo("preferiti", None, 0, Some(1)));
    let preferiti = cliente.preferiti()?;
    snapshot.preferiti = preferiti.voci;
    segna(&mut troncati, "preferiti", preferiti.troncato);

    avanzamento(&passo("album", None, 0, Some(1)));
    let album = cliente.album()?;
    snapshot.album = album.voci;
    segna(&mut troncati, "album", album.troncato);

    avanzamento(&passo("artisti", None, 0, Some(1)));
    let artisti = cliente.artisti_seguiti()?;
    snapshot.artisti = artisti.voci;
    segna(&mut troncati, "artisti", artisti.troncato);

    // Le playlist sono la parte lunga: l'elenco costa una richiesta, i brani ne
    // costano una per playlist. È l'unica fase in cui la barra ha davvero
    // qualcosa da dire.
    let elenco = cliente.playlist()?;
    segna(&mut troncati, "playlist", elenco.troncato);
    let mut playlist = elenco.voci;
    let quante = playlist.len();
    for (indice, voce) in playlist.iter_mut().enumerate() {
        avanzamento(&passo(
            "playlist",
            Some(voce.nome.clone()),
            indice,
            Some(quante),
        ));
        let Some(id) = voce.spotify_id.clone() else {
            // Senza identificativo non c'è niente da chiedere. Non capita da
            // questa via — l'API lo dà sempre — ed è un `continue` invece di un
            // `unwrap` perché la struttura permette di non averlo.
            continue;
        };
        match cliente.brani_di_playlist(&id) {
            Ok(contenuto) => {
                // Il totale va ricalcolato **prima** di sostituire i brani: quel
                // che Spotify dichiara conta anche i podcast e i brani spariti
                // dal catalogo, e confrontarlo con la sola musica farebbe
                // sembrare monca una playlist arrivata intera. Vedi
                // `Contenuto::dichiarati`, e la guardia che quel numero governa
                // in `import_spotify::prepara_playlist`.
                voce.dichiarati = contenuto.dichiarati(voce.dichiarati);
                lettura_podcast = lettura_podcast.saturating_add(contenuto.podcast);
                voce.brani = contenuto.brani;
            }
            Err(err) if e_solo_questa(&err) => senza_contenuto.push(voce.nome.clone()),
            Err(err) => return Err(err),
        }
    }
    snapshot.playlist = playlist;

    avanzamento(&passo("cronologia", None, 0, Some(1)));
    snapshot.cronologia = cliente.ascolti_recenti()?;

    Ok(Lettura {
        snapshot,
        premium: profilo.premium,
        senza_contenuto,
        podcast: lettura_podcast,
        troncati,
    })
}

/// Annota un elenco arrivato a metà.
fn segna(troncati: &mut Vec<&'static str>, quale: &'static str, troncato: bool) {
    if troncato {
        troncati.push(quale);
    }
}

/// L'errore riguarda **questa** playlist e non la lettura?
///
/// Un caso solo, dichiarato invece che riconosciuto per esclusione: dal marzo
/// 2026 il contenuto di una playlist che l'utente non possiede non si legge, e
/// Spotify risponde `403`. Tutto il resto — token scaduto, quota finita, rete —
/// riguarda la lettura intera e deve fermarla.
fn e_solo_questa(err: &AppError) -> bool {
    matches!(
        err.code(),
        aether_domain::errors::ErrorCode::SpotifyAccountForbidden
    )
}

/// Un passo, scritto una volta sola.
fn passo(
    fase: &'static str,
    nome: Option<String>,
    fatti: usize,
    totali: Option<usize>,
) -> Avanzamento {
    Avanzamento {
        fase,
        nome,
        fatti,
        totali,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn le_fasi_dell_avanzamento_hanno_nomi_stabili() {
        // Attraversano l'IPC e finiscono in un `switch` nella finestra: un nome
        // cambiato qui è una barra che smette di avanzare, senza errori.
        let p = passo("playlist", Some("Corsa".to_owned()), 3, Some(62));
        assert_eq!(p.fase, "playlist");
        assert_eq!(p.nome.as_deref(), Some("Corsa"));
        assert_eq!(p.fatti, 3);
        assert_eq!(p.totali, Some(62));
    }

    #[test]
    fn solo_il_403_riguarda_una_playlist_sola() {
        use aether_domain::errors::ErrorCode;
        assert!(e_solo_questa(&AppError::new(
            ErrorCode::SpotifyAccountForbidden
        )));
        // Questi fermano tutto: riguardano la lettura, non la playlist.
        assert!(!e_solo_questa(&AppError::new(
            ErrorCode::SpotifyAccountAuthExpired
        )));
        assert!(!e_solo_questa(&AppError::new(
            ErrorCode::SpotifyQuotaExceeded {
                retry_after_ms: None
            }
        )));
        assert!(!e_solo_questa(&AppError::new(ErrorCode::NetHttp {
            status: 500,
            url: None
        })));
    }

    #[test]
    fn quel_che_esce_e_lo_stesso_valore_che_esce_dall_archivio() {
        // Il perno di tutta la funzione: due lettori che non si somigliano per
        // niente producono lo stesso tipo, e da lì in giù il codice è uno solo.
        let vuoto = AccountSnapshot::vuoto(Provenienza::Api);
        assert_eq!(vuoto.provenienza.nome(), "api");
        assert!(
            !vuoto.provenienza.ha_cronologia_completa(),
            "cinquanta righe non sono «la cronologia», e il rapporto deve dirlo"
        );
    }
}
