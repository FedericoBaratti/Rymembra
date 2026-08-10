//! Da quel che l'utente incolla a quel che si va a chiedere.
//!
//! # Perché a mano e non con un'espressione regolare
//!
//! Il vecchio albero lo faceva con una riga sola
//! (`legacy/.../download/urlDetect.ts`), e in TypeScript era la scelta giusta:
//! le espressioni regolari sono nel linguaggio. Qui vorrebbero dire aggiungere
//! `regex` al workspace — un albero di dipendenze e qualche centinaio di
//! kilobyte nell'`.so` di Android — per riconoscere quattro parole in un
//! percorso. Il resto del nucleo non ne usa nessuna, e questa non è la ragione
//! per cominciare.
//!
//! # Cosa si accetta
//!
//! Tutto quel che un utente può ragionevolmente avere negli appunti:
//!
//! ```text
//! https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT
//! https://open.spotify.com/intl-it/album/1DFixLWuPkv3KT3TnV35m3?si=abc
//! https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M
//! https://open.spotify.com/user/tizio/playlist/37i9dQZF1DXcBWIGoYBM5M
//! open.spotify.com/artist/0OdUWJ0sBjDrqHygGUXeCF
//! Senti questa: open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT bella eh
//! spotify:track:4cOdK2wGLETKBW3PvgPWqT
//! spotify:user:tizio:playlist:37i9dQZF1DXcBWIGoYBM5M
//! ```
//!
//! I link corti che l'app del telefono mette nel foglio di condivisione —
//! `spotify.link/…` — **non** si riconoscono qui: dicono dove andare a
//! guardare, non che cosa nominano, e per saperlo serve una richiesta di rete.
//! Se ne occupa [`crate::scorciatoia`].
//!
//! Il parametro `?si=` — l'etichetta di condivisione che Spotify appiccica a
//! ogni link copiato dall'app — si butta via insieme al resto della query. Non
//! serve a niente di quel che facciamo, e portarselo dietro vorrebbe dire
//! spedire a Spotify l'identificativo di chi ha condiviso il link.

use aether_domain::spotify::SpotifyKind;

/// Un link riconosciuto, ridotto alle due cose che contano.
///
/// Il genere è [`SpotifyKind`], che vive nel dominio: è lo stesso valore che
/// finirà dentro un [`aether_domain::spotify::SpotifyContent`], e averne due
/// definizioni — una qui per il protocollo, una là per i dati — vorrebbe dire
/// scrivere una conversione fra due enumerazioni identiche e aspettare che
/// divergano.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Riferimento {
    /// Cosa nomina.
    pub genere: SpotifyKind,
    /// L'identificativo, in base62.
    pub id: String,
}

impl Riferimento {
    /// L'URI che Pathfinder si aspetta come parametro delle sue interrogazioni.
    #[must_use]
    pub fn uri(&self) -> String {
        format!("spotify:{}:{}", self.genere.path_word(), self.id)
    }

    /// L'indirizzo pubblico, ricostruito in forma canonica.
    ///
    /// Ricostruito e non conservato: quel che l'utente ha incollato può avere
    /// una lingua, un `?si=`, o nessuno schema. Serve a oEmbed, che vuole un
    /// indirizzo vero.
    #[must_use]
    pub fn url_pubblico(&self) -> String {
        format!(
            "https://open.spotify.com/{}/{}",
            self.genere.path_word(),
            self.id
        )
    }

    /// L'indirizzo della pagina del riquadro incorporabile.
    #[must_use]
    pub fn url_embed(&self) -> String {
        format!(
            "https://open.spotify.com/embed/{}/{}",
            self.genere.path_word(),
            self.id
        )
    }
}

/// L'ospite che si riconosce. Solo questo: un link a `spotify.com` senza
/// `open.` è una pagina di marketing, non un contenuto.
const OSPITE: &str = "open.spotify.com";

/// Riconosce un link o un URI di Spotify.
///
/// Restituisce `None` per tutto il resto — un link di YouTube, una frase, una
/// stringa vuota — perché chi chiama possa dire all'utente «questo non è un
/// link di Spotify» invece di partire e fallire più avanti con un 404.
#[must_use]
pub fn riconosci(input: &str) -> Option<Riferimento> {
    let testo = input.trim();
    if testo.is_empty() {
        return None;
    }
    riconosci_uri(testo).or_else(|| riconosci_indirizzo(testo))
}

/// La forma `spotify:genere:id`, e la vecchia `spotify:user:tizio:playlist:id`.
fn riconosci_uri(testo: &str) -> Option<Riferimento> {
    if !inizia_con_ignorando_maiuscole(testo, "spotify:") {
        return None;
    }
    let pezzi: Vec<&str> = testo.split(':').collect();
    // Si scorre a coppie invece di prendere la seconda e la terza: la forma
    // vecchia con l'utente in mezzo mette il genere in quarta posizione, e
    // cercare la coppia «genere valido seguito da identificativo valido» le
    // gestisce entrambe senza contare i due punti.
    for coppia in pezzi.windows(2) {
        if let [parola, pezzo] = coppia
            && let Some(genere) = SpotifyKind::from_path_word(parola)
            && let Some(id) = id_da_segmento(pezzo)
        {
            return Some(Riferimento { genere, id });
        }
    }
    None
}

/// La forma con l'indirizzo, con o senza schema, lingua o `embed`.
fn riconosci_indirizzo(testo: &str) -> Option<Riferimento> {
    let dopo = dopo_ospite(testo)?;
    // La query e il frammento non ci interessano — vedi la nota in testa al
    // modulo sul perché `?si=` si butta via invece di conservarlo.
    let percorso = dopo.split(['?', '#']).next().unwrap_or(dopo);

    let mut segmenti = percorso.split('/').filter(|s| !s.is_empty()).peekable();
    // Davanti al genere Spotify infila la lingua (`intl-it`) e, nei link del
    // riquadro incorporabile, `embed`. Sono prefissi, non generi, e possono
    // esserci entrambi.
    //
    // `user/<nome>` è il terzo: la forma vecchia degli indirizzi delle playlist
    // (`/user/tizio/playlist/…`) che sta ancora in mezzo ai vecchi messaggi e
    // nei segnalibri. Si salta a coppie perché il nome dell'utente è un
    // segmento suo, e saltarne uno solo lascerebbe quello al posto del genere.
    loop {
        match segmenti.peek() {
            Some(s) if s.starts_with("intl-") || *s == "embed" => {
                segmenti.next();
            }
            Some(s) if *s == "user" => {
                segmenti.next();
                segmenti.next();
            }
            _ => break,
        }
    }

    let genere = SpotifyKind::from_path_word(segmenti.next()?)?;
    let id = id_da_segmento(segmenti.next()?)?;
    Some(Riferimento { genere, id })
}

/// Quel che segue l'ospite nell'indirizzo, se l'ospite c'è.
fn dopo_ospite(testo: &str) -> Option<&str> {
    // `to_ascii_lowercase` e non `to_lowercase`: il secondo può cambiare la
    // **lunghezza** in byte (la I turca ne è l'esempio classico), e la
    // posizione trovata nella copia minuscola non varrebbe più nell'originale.
    let minuscolo = testo.to_ascii_lowercase();
    let dove = minuscolo.find(OSPITE)?;
    testo.get(dove.checked_add(OSPITE.len())?..)
}

/// La lunghezza di un identificativo di Spotify. Sono sempre ventidue.
const LUNGHEZZA_ID: usize = 22;

/// L'identificativo dentro un segmento del percorso.
///
/// # Perché non basta «il segmento è base62»
///
/// Un link incollato da una chat si porta dietro quel che c'era scritto dopo —
/// «…/track/4cOdK2wGLETKBW3PvgPWqT bella eh», o una parentesi di chiusura — e
/// pretendere che il segmento sia base62 dal primo carattere all'ultimo fa
/// cadere un link perfettamente leggibile. Si prende quindi la **testa** in
/// base62.
///
/// # Perché tagliare non basta a sua volta
///
/// Perché allora `track/non-valido!` diventerebbe l'identificativo `non`, cioè
/// una richiesta vera a Spotify, e l'utente vedrebbe un 404 al posto di «questo
/// link non è valido». Quindi: se la testa è tutto il segmento vale la regola di
/// prima, larga sulla lunghezza; se invece si è tagliato qualcosa, la testa deve
/// essere lunga esattamente quanto un identificativo vero — è l'unico modo di
/// sapere che si è tagliato nel punto giusto e non in mezzo a una parola.
fn id_da_segmento(segmento: &str) -> Option<String> {
    let fine = segmento
        .bytes()
        .position(|b| !b.is_ascii_alphanumeric())
        .unwrap_or(segmento.len());
    let id = segmento.get(..fine)?;
    if id.is_empty() {
        return None;
    }
    let intero = fine == segmento.len();
    let buono = if intero {
        id.len() <= 64
    } else {
        id.len() == LUNGHEZZA_ID
    };
    buono.then(|| id.to_owned())
}

/// Confronto di prefisso che ignora maiuscole e minuscole, senza allocare.
fn inizia_con_ignorando_maiuscole(testo: &str, prefisso: &str) -> bool {
    testo
        .get(..prefisso.len())
        .is_some_and(|inizio| inizio.eq_ignore_ascii_case(prefisso))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn rif(genere: SpotifyKind, id: &str) -> Option<Riferimento> {
        Some(Riferimento {
            genere,
            id: id.to_owned(),
        })
    }

    #[test]
    fn i_link_normali_si_riconoscono() {
        assert_eq!(
            riconosci("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
        assert_eq!(
            riconosci("http://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3"),
            rif(SpotifyKind::Album, "1DFixLWuPkv3KT3TnV35m3")
        );
        assert_eq!(
            riconosci("open.spotify.com/artist/0OdUWJ0sBjDrqHygGUXeCF"),
            rif(SpotifyKind::Artist, "0OdUWJ0sBjDrqHygGUXeCF"),
            "senza schema: è quel che si ottiene copiando dalla barra di Chrome"
        );
    }

    #[test]
    fn la_lingua_e_il_riquadro_sono_prefissi_non_generi() {
        assert_eq!(
            riconosci("https://open.spotify.com/intl-it/track/4cOdK2wGLETKBW3PvgPWqT"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
        assert_eq!(
            riconosci("https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M"),
            rif(SpotifyKind::Playlist, "37i9dQZF1DXcBWIGoYBM5M")
        );
        assert_eq!(
            riconosci("https://open.spotify.com/embed/intl-fr/album/1DFixLWuPkv3KT3TnV35m3"),
            rif(SpotifyKind::Album, "1DFixLWuPkv3KT3TnV35m3"),
            "e possono esserci tutti e due"
        );
    }

    #[test]
    fn letichetta_di_condivisione_si_butta_via() {
        // È l'unico caso davvero universale: ogni link copiato dall'app di
        // Spotify ce l'ha attaccato.
        assert_eq!(
            riconosci("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=b1f9&pt=x"),
            rif(SpotifyKind::Playlist, "37i9dQZF1DXcBWIGoYBM5M")
        );
        assert_eq!(
            riconosci("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT#t=30"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
    }

    #[test]
    fn la_forma_vecchia_con_lutente_nel_percorso_si_riconosce() {
        // `/user/<nome>/playlist/<id>` è com'erano gli indirizzi delle playlist
        // prima, e sta ancora nei messaggi e nei segnalibri di chi ce l'ha da
        // allora.
        assert_eq!(
            riconosci("https://open.spotify.com/user/spotify/playlist/37i9dQZF1DXcBWIGoYBM5M"),
            rif(SpotifyKind::Playlist, "37i9dQZF1DXcBWIGoYBM5M")
        );
        assert_eq!(
            riconosci(
                "https://open.spotify.com/user/tizio-99/playlist/37i9dQZF1DXcBWIGoYBM5M?si=x"
            ),
            rif(SpotifyKind::Playlist, "37i9dQZF1DXcBWIGoYBM5M"),
            "il nome dell'utente può contenere di tutto: è un segmento da saltare, non da leggere"
        );
    }

    #[test]
    fn un_link_dentro_una_frase_resta_un_link() {
        // Il caso di chi incolla il messaggio intero invece del solo indirizzo.
        // Senza il `?si=` a chiudere il percorso, quel che segue finisce dentro
        // il segmento dell'identificativo.
        assert_eq!(
            riconosci("Senti questa: open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT bella eh"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
        assert_eq!(
            riconosci("(https://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3)"),
            rif(SpotifyKind::Album, "1DFixLWuPkv3KT3TnV35m3")
        );
    }

    #[test]
    fn tagliare_la_coda_non_vuol_dire_accettare_qualunque_cosa() {
        // Il tranello del taglio: senza la lunghezza, `non-valido!` diventerebbe
        // l'identificativo «non» e quindi una richiesta vera.
        assert_eq!(
            riconosci("https://open.spotify.com/track/non-valido!"),
            None
        );
        assert_eq!(
            riconosci("https://open.spotify.com/track/4cOd bella"),
            None,
            "una testa più corta di un identificativo è una parola tagliata a metà"
        );
    }

    #[test]
    fn gli_uri_si_riconoscono_in_tutte_e_due_le_forme() {
        assert_eq!(
            riconosci("spotify:track:4cOdK2wGLETKBW3PvgPWqT"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
        assert_eq!(
            riconosci("spotify:user:tizio:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            rif(SpotifyKind::Playlist, "37i9dQZF1DXcBWIGoYBM5M"),
            "la forma vecchia con l'utente in mezzo"
        );
        assert_eq!(
            riconosci("SPOTIFY:TRACK:4cOdK2wGLETKBW3PvgPWqT"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
    }

    #[test]
    fn gli_spazi_intorno_non_contano() {
        // Incollare da una chat porta dentro spazi e a capo.
        assert_eq!(
            riconosci("  https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT \n"),
            rif(SpotifyKind::Track, "4cOdK2wGLETKBW3PvgPWqT")
        );
    }

    #[test]
    fn quel_che_non_e_spotify_non_si_riconosce() {
        assert_eq!(riconosci(""), None);
        assert_eq!(riconosci("   "), None);
        assert_eq!(riconosci("ciao come stai"), None);
        assert_eq!(riconosci("https://youtube.com/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(
            riconosci("https://spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"),
            None,
            "senza `open.` è una pagina di marketing"
        );
        assert_eq!(
            riconosci("https://open.spotify.com/track/"),
            None,
            "un genere senza identificativo non è un link"
        );
        assert_eq!(
            riconosci("https://open.spotify.com/episode/512ojhOuo1ktJprKbVcKyQ"),
            None,
            "i podcast non si importano: non sono musica in libreria"
        );
        assert_eq!(
            riconosci("https://open.spotify.com/track/non-valido!"),
            None,
            "un identificativo che non è base62 è spazzatura, non una richiesta"
        );
    }

    #[test]
    fn un_riferimento_sa_ricostruire_i_suoi_indirizzi() {
        let Some(r) =
            riconosci("https://open.spotify.com/intl-it/album/1DFixLWuPkv3KT3TnV35m3?si=x")
        else {
            panic!("il link è valido");
        };
        assert_eq!(r.uri(), "spotify:album:1DFixLWuPkv3KT3TnV35m3");
        assert_eq!(
            r.url_pubblico(),
            "https://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3",
            "canonico: la lingua e l'etichetta di condivisione sono sparite"
        );
        assert_eq!(
            r.url_embed(),
            "https://open.spotify.com/embed/album/1DFixLWuPkv3KT3TnV35m3"
        );
    }
}
