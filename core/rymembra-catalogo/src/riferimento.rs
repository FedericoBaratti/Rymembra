//! Che cosa nomina il link che qualcuno ha incollato.
//!
//! Si legge senza rete, perché è una decisione su una stringa e le decisioni su
//! stringhe si provano senza rete. Il crate che parla col mondo la usa; se
//! l'indirizzo non si riconosce, nessuna richiesta parte.
//!
//! # Perché non si accetta qualunque cosa
//!
//! Perché «non riconosco questo link» è una risposta utile, mentre «l'ho chiesto
//! e ha detto di no» non lo è: la seconda manda a controllare la connessione per
//! un indirizzo di un servizio che non c'entra niente. E soprattutto: un link
//! che non si riconosce è un link a cui **non si va**, ed è la garanzia che
//! Rymembra non vada mai a bussare dove non è invitato.

use rymembra_domain::esterno::{Fonte, GenereContenuto};

/// Che cosa un indirizzo nomina, e presso chi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Riferimento {
    /// Quale catalogo.
    pub fonte: Fonte,
    /// Che genere di cosa.
    pub genere: GenereContenuto,
    /// L'identificativo presso quel catalogo.
    pub id: String,
}

impl Riferimento {
    /// L'indirizzo pubblico da cui questo riferimento si è ricavato.
    ///
    /// Ricostruito e non conservato: quel che si è incollato può avere
    /// parametri di tracciamento, un dominio abbreviato, un `?si=` di troppo.
    /// Quel che si mostra e si salva dev'essere la forma canonica.
    #[must_use]
    pub fn url_pubblico(&self) -> String {
        match self.fonte {
            Fonte::InternetArchive => format!("https://archive.org/details/{}", self.id),
            Fonte::Jamendo => format!(
                "https://www.jamendo.com/{}/{}",
                self.genere.path_word(),
                self.id
            ),
            Fonte::Audius => format!("https://audius.co/{}", self.id),
            Fonte::ArchivioSpotify | Fonte::FilePlaylist => self.id.clone(),
        }
    }
}

/// Riconosce un indirizzo di un catalogo che Rymembra sa leggere.
///
/// `None` per tutto il resto, compresi i servizi che Rymembra non interroga: non
/// è una lacuna da colmare, è il confine.
#[must_use]
pub fn riconosci(input: &str) -> Option<Riferimento> {
    let pulito = input.trim();
    if pulito.is_empty() {
        return None;
    }
    let (dominio, percorso) = spezza_indirizzo(pulito)?;

    if e_dominio(&dominio, "archive.org") {
        return archivio_org(&percorso);
    }
    // Senza la feature, un link di Jamendo **non si riconosce affatto**, e non è
    // una svista da correggere. Riconoscerlo vorrebbe dire accettarlo e poi
    // fallire con «questo catalogo non risponde» — che è falso: Jamendo
    // risponde benissimo, è Rymembra che non lo porta. Non riconoscendolo,
    // l'errore che esce è `download.unrecognizedUrl`, che elenca i cataloghi
    // che ci sono davvero ed è l'unica frase vera delle due.
    #[cfg(feature = "jamendo")]
    if e_dominio(&dominio, "jamendo.com") {
        return jamendo(&percorso);
    }
    if e_dominio(&dominio, "audius.co") || e_dominio(&dominio, "audius.org") {
        return audius(&percorso);
    }
    None
}

/// Un nome che si risolve nel DNS pubblico, e non un indirizzo di casa.
///
/// Si guarda l'ultima etichetta: un nome pubblico finisce con un dominio di
/// primo livello fatto di lettere (o di punycode, `xn--…`). È la regola che
/// esclude in un colpo solo `127.0.0.1`, `10.0.0.1`, `[::1]`, le forme
/// esadecimali o decimali di un IPv4 che alcuni risolutori accettano
/// (`0x7f.1`, `2130706433`) e i nomi senza punto; i suffissi da rete di casa si
/// escludono per nome.
///
/// # Perché sta qui
///
/// Stava nel lettore (`riproduzione/flusso.rs`), e valeva soltanto per i
/// rimandi dei flussi. Ma le altre richieste verso un indirizzo che arriva da
/// una risposta — il nodo che `api.audius.co` elenca, le copertine, i rimandi
/// di un prelievo — partono da questo crate, e un secondo controllo scritto qui
/// sarebbe stato il secondo: quello che un giorno dice una cosa diversa. Sta
/// accanto a [`riconosci`], che è l'altra metà dello stesso confine.
#[must_use]
pub fn nome_pubblico(ospite: &str) -> bool {
    let ospite = ospite.trim_end_matches('.').to_ascii_lowercase();
    let Some((_, ultima)) = ospite.rsplit_once('.') else {
        return false;
    };
    let lettere = ultima.len() >= 2 && ultima.chars().all(|c| c.is_ascii_alphabetic());
    let punycode = ultima.starts_with("xn--");
    let di_casa = [
        "localhost",
        "local",
        "internal",
        "lan",
        "home",
        "arpa",
        "test",
        "invalid",
    ]
    .contains(&ultima);
    (lettere || punycode) && !di_casa
}

/// Un indirizzo cifrato verso un [`nome_pubblico`].
///
/// La forma che si esige da ogni indirizzo che non si conosce in anticipo: il
/// nodo di Audius, una copertina dichiarata da una risposta.
#[must_use]
pub fn indirizzo_pubblico(url: &str) -> bool {
    url.starts_with("https://") && rymembra_net::http::ospite(url).is_some_and(nome_pubblico)
}

/// Se un rimando di una richiesta a `fonte`, partita da `partenza`, può andare
/// a `bersaglio`.
///
/// # La stessa allowlist dell'indirizzo di partenza
///
/// L'indirizzo di partenza è passato da [`riconosci`], e il bersaglio deve
/// passare **dallo stesso posto**: un secondo elenco di domini sarebbe il
/// secondo, cioè quello che un giorno dice una cosa diversa. Ma `riconosci`
/// legge link pubblici, e il bersaglio di un rimando non lo è:
/// `archive.org/download/…` rimanda a `ia800305.us.archive.org/27/items/…`, un
/// percorso che `riconosci` non ha motivo di conoscere. Quel che del bersaglio
/// conta è l'**ospite**, e allora si chiede a `riconosci` esattamente quello: lo
/// stesso indirizzo di partenza, con l'ospite del bersaglio al posto del suo.
/// Se è ancora un link di quel catalogo, l'ospite è di casa; `evilarchive.org`
/// e `archive.org.esempio.com` no, per le stesse ragioni per cui non lo sono
/// come link.
///
/// # Audius, che un'allowlist di domini non ce l'ha
///
/// Audius non ha un server ma una rete di nodi gestiti da chi vuole, ognuno col
/// suo dominio: il nodo di partenza viene dall'elenco che `api.audius.co`
/// consegna, non da `riconosci`, e rimanda a un nodo di contenuti che può stare
/// sotto qualunque nome. Un elenco di domini per Audius sarebbe un elenco che
/// smette di suonare metà del catalogo il giorno in cui entra un nodo nuovo.
/// Quel che si può esigere da un salto di Audius, e si esige, è che resti
/// cifrato e che vada verso un [`nome_pubblico`]: mai un indirizzo IP scritto a
/// cifre, mai `localhost`, mai un nome da rete di casa. Un nodo ostile non può
/// così usare Rymembra per bussare al router o a un servizio di questa
/// macchina.
///
/// Vale per i flussi del lettore e per i prelievi di [`crate::Cataloghi::preleva`].
#[must_use]
pub fn bersaglio_ammesso(fonte: Fonte, partenza: &str, bersaglio: &str) -> bool {
    if !bersaglio.starts_with("https://") {
        return false;
    }
    let Some(ospite) = rymembra_net::http::ospite(bersaglio) else {
        return false;
    };
    if fonte == Fonte::Audius {
        return nome_pubblico(ospite);
    }
    // Il percorso della partenza, query compresa: `riconosci` guarda il
    // percorso per capire *che cosa* il link nomina, e deve trovarci quel che
    // ci ha trovato la prima volta.
    let percorso = partenza
        .strip_prefix("https://")
        .and_then(|resto| resto.find('/').and_then(|inizio| resto.get(inizio..)))
        .unwrap_or("/");
    riconosci(&format!("https://{ospite}{percorso}"))
        .is_some_and(|riferimento| riferimento.fonte == fonte)
}

/// Il dominio è esattamente `base`, o un suo sottodominio.
///
/// Un semplice `ends_with` accetterebbe `evilarchive.org`: il modulo esiste per
/// dire «un link che non si riconosce è un link a cui non si bussa», e un
/// suffisso non è un riconoscimento.
fn e_dominio(dominio: &str, base: &str) -> bool {
    dominio == base
        || dominio
            .strip_suffix(base)
            .is_some_and(|resto| resto.ends_with('.'))
}

/// Dominio e percorso, senza schema, senza `www.`, senza query.
///
/// Lo schema è `https://`, `http://` o nessuno, e l'ospite è un nome di
/// dominio: lettere, cifre, punti e trattini, nient'altro. Prima l'ospite era
/// tutto quel che stava prima della prima `/`, e [`e_dominio`] guardava solo
/// come finiva: `\\host\cartella\x.archive.org` o `search-ms:…x.archive.org`
/// passavano per sottodomini di `archive.org`, mentre per Windows sono un
/// percorso di rete e un protocollo di sistema. Niente porte, niente
/// credenziali, niente barre rovesciate: nessun link vero di un catalogo li
/// porta.
fn spezza_indirizzo(input: &str) -> Option<(String, Vec<String>)> {
    let senza_schema = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let senza_query = senza_schema.split(['?', '#']).next()?;
    let (host, resto) = senza_query.split_once('/').unwrap_or((senza_query, ""));
    let dominio = host.trim_start_matches("www.").to_ascii_lowercase();
    let un_nome = dominio
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
    if dominio.is_empty() || !dominio.contains('.') || !un_nome {
        return None;
    }
    let pezzi: Vec<String> = resto
        .split('/')
        .filter(|p| !p.is_empty())
        .map(decodifica)
        .collect();
    Some((dominio, pezzi))
}

/// `archive.org/details/<id>` e `archive.org/download/<id>/<file>`.
///
/// Il secondo si riduce al primo di proposito: chi incolla il link diretto a un
/// file vuole quel concerto, non quel file soltanto — e se volesse solo quello,
/// prenderlo insieme agli altri non gli toglie niente.
fn archivio_org(percorso: &[String]) -> Option<Riferimento> {
    let primo = percorso.first()?.as_str();
    if !matches!(primo, "details" | "download" | "metadata" | "embed") {
        return None;
    }
    let id = percorso.get(1)?.trim();
    (!id.is_empty()).then(|| Riferimento {
        fonte: Fonte::InternetArchive,
        genere: GenereContenuto::Collezione,
        id: id.to_owned(),
    })
}

/// `jamendo.com/track/<id>[/<slug>]`, `/album/<id>`, `/artist/<id>`.
#[cfg(feature = "jamendo")]
fn jamendo(percorso: &[String]) -> Option<Riferimento> {
    // Le pagine di Jamendo hanno una lingua davanti su certi mercati:
    // `jamendo.com/it/track/…`. Saltarla quando c'è costa due righe; non
    // saltarla vuol dire non riconoscere i link di metà Europa.
    let inizio = percorso
        .first()
        .filter(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map_or(0, |_| 1);
    let genere = match percorso.get(inizio)?.as_str() {
        "track" => GenereContenuto::Track,
        "album" => GenereContenuto::Album,
        "artist" => GenereContenuto::Artist,
        "playlist" => GenereContenuto::Playlist,
        _ => return None,
    };
    let id = percorso.get(inizio.saturating_add(1))?.trim();
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| Riferimento {
        fonte: Fonte::Jamendo,
        genere,
        id: id.to_owned(),
    })
}

/// `audius.co/<handle>/<slug>` per un brano, `audius.co/<handle>` per un artista.
fn audius(percorso: &[String]) -> Option<Riferimento> {
    let handle = percorso.first()?.trim();
    if handle.is_empty() {
        return None;
    }
    match percorso.get(1).map(String::as_str) {
        Some("album") | Some("playlist") => {
            let coda = percorso.get(2)?.trim();
            (!coda.is_empty()).then(|| Riferimento {
                fonte: Fonte::Audius,
                genere: GenereContenuto::Playlist,
                id: format!("{handle}/{}/{coda}", percorso.get(1).map_or("", |s| s)),
            })
        }
        Some(slug) if !slug.is_empty() => Some(Riferimento {
            fonte: Fonte::Audius,
            genere: GenereContenuto::Track,
            id: format!("{handle}/{slug}"),
        }),
        _ => Some(Riferimento {
            fonte: Fonte::Audius,
            genere: GenereContenuto::Artist,
            id: handle.to_owned(),
        }),
    }
}

/// La decodifica percentuale, per gli identificativi con caratteri speciali.
fn decodifica(pezzo: &str) -> String {
    let byte = pezzo.as_bytes();
    let mut fuori: Vec<u8> = Vec::with_capacity(byte.len());
    let mut n = 0_usize;
    while let Some(&b) = byte.get(n) {
        if b == b'%'
            && let (Some(&alto), Some(&basso)) = (byte.get(n + 1), byte.get(n + 2))
            && let (Some(a), Some(z)) = (cifra(alto), cifra(basso))
        {
            fuori.push(a.saturating_mul(16).saturating_add(z));
            n = n.saturating_add(3);
            continue;
        }
        fuori.push(b);
        n = n.saturating_add(1);
    }
    String::from_utf8(fuori).unwrap_or_else(|_| pezzo.to_owned())
}

/// Una cifra esadecimale, o niente.
const fn cifra(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_concerto_dellarchive_si_riconosce_in_tutte_le_sue_forme() {
        let atteso = Riferimento {
            fonte: Fonte::InternetArchive,
            genere: GenereContenuto::Collezione,
            id: "gd1977-05-08.sbd.hicks.4982.sbeok.shnf".to_owned(),
        };
        for url in [
            "https://archive.org/details/gd1977-05-08.sbd.hicks.4982.sbeok.shnf",
            "http://www.archive.org/details/gd1977-05-08.sbd.hicks.4982.sbeok.shnf/",
            "archive.org/download/gd1977-05-08.sbd.hicks.4982.sbeok.shnf/gd77t01.flac",
            "https://archive.org/metadata/gd1977-05-08.sbd.hicks.4982.sbeok.shnf?x=1",
        ] {
            assert_eq!(riconosci(url).as_ref(), Some(&atteso), "su «{url}»");
        }
    }

    #[test]
    fn lindirizzo_pubblico_e_la_forma_canonica() {
        let r = riconosci("archive.org/download/gd77/t01.flac?utm_source=x")
            .expect("è un link dell'Archive");
        assert_eq!(r.url_pubblico(), "https://archive.org/details/gd77");
    }

    #[test]
    fn quel_che_non_si_riconosce_non_si_va_a_chiedere() {
        // Il confine, e la ragione per cui è scritto: un link che non si
        // riconosce è un link a cui non si bussa.
        for url in [
            "https://www.youtube.com/watch?v=abcdefghijk",
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
            "https://music.apple.com/it/album/x/123",
            "https://archive.org/",
            "non un indirizzo",
            "",
            "   ",
            // Un suffisso non è un riconoscimento: questi somigliano ai
            // cataloghi veri ma non lo sono.
            "https://evilarchive.org/details/x",
            "https://notaudius.co/artista/brano",
        ] {
            assert_eq!(riconosci(url), None, "su «{url}»");
        }
    }

    #[test]
    fn i_sottodomini_veri_si_riconoscono_ancora() {
        assert!(riconosci("https://web.archive.org/details/gd77").is_some());
    }

    /// L'ospite è un nome di dominio vero, preceduto da uno schema web o da
    /// niente. Prima `spezza_indirizzo` prendeva per ospite tutto quel che
    /// stava prima della prima `/`, e un ospite così finiva col dominio di un
    /// catalogo anche quando era un percorso UNC o un protocollo di Windows:
    /// `riconosci` diceva di sì, e la pagina di un brano si apriva.
    #[test]
    fn un_prefisso_non_web_non_si_riconosce() {
        for url in [
            "\\\\evil.example\\share\\x.archive.org/details/gd77",
            "search-ms:query=x.archive.org/details/gd77",
            "file:x.archive.org/details/gd77",
            "ms-settings:x.audius.co/deadmau5",
            "https://utente@x.archive.org/details/gd77",
            "https://evil.example\\x.archive.org/details/gd77",
            "https://x_y.archive.org/details/gd77",
            "https://archive.org:8080/details/gd77",
        ] {
            assert_eq!(riconosci(url), None, "su «{url}»");
        }
        // Le forme buone di prima restano buone, maiuscole comprese.
        assert!(riconosci("https://WEB.Archive.org/details/gd77").is_some());
        assert!(riconosci("archive.org/details/gd77").is_some());
        assert!(riconosci("http://www.archive.org/details/gd77").is_some());
    }

    #[test]
    #[cfg(feature = "jamendo")]
    fn jamendo_si_riconosce_anche_con_la_lingua_davanti() {
        let atteso = Riferimento {
            fonte: Fonte::Jamendo,
            genere: GenereContenuto::Track,
            id: "1884527".to_owned(),
        };
        assert_eq!(
            riconosci("https://www.jamendo.com/track/1884527/mon-ami").as_ref(),
            Some(&atteso)
        );
        assert_eq!(
            riconosci("https://www.jamendo.com/it/track/1884527/mon-ami").as_ref(),
            Some(&atteso)
        );
        // Un identificativo che non è un numero non è un identificativo Jamendo.
        assert_eq!(riconosci("https://www.jamendo.com/track/mon-ami"), None);
    }

    #[test]
    fn audius_distingue_un_brano_da_un_artista() {
        assert_eq!(
            riconosci("https://audius.co/deadmau5").map(|r| r.genere),
            Some(GenereContenuto::Artist)
        );
        assert_eq!(
            riconosci("https://audius.co/deadmau5/strobe-123").map(|r| r.genere),
            Some(GenereContenuto::Track)
        );
        assert_eq!(
            riconosci("https://audius.co/deadmau5/album/random-album-title-99").map(|r| r.genere),
            Some(GenereContenuto::Playlist)
        );
    }

    #[test]
    fn un_indirizzo_pubblico_e_cifrato_e_ha_un_nome() {
        for buono in [
            "https://creatornode2.audius.co/tracks/x",
            "https://nodo.xn--p1ai/",
            "https://Archive.ORG./details/x",
        ] {
            assert!(indirizzo_pubblico(buono), "su «{buono}»");
        }
        for cattivo in [
            "https://127.0.0.1/",
            "https://192.168.1.1",
            "https://[::1]:8443/",
            "https://0x7f.1/",
            "https://2130706433/",
            "https://localhost/",
            "https://router.lan/",
            "https://stampante.local/",
            "https://p/s.jpg",
            "http://creatornode2.audius.co/",
            "https://utente@192.168.1.1/",
        ] {
            assert!(!indirizzo_pubblico(cattivo), "su «{cattivo}»");
        }
    }

    #[test]
    fn gli_identificativi_codificati_si_leggono() {
        assert_eq!(decodifica("gd77%20live"), "gd77 live");
        assert_eq!(decodifica("Bj%C3%B6rk"), "Björk");
        // Una percentuale che non introduce niente resta com'è.
        assert_eq!(decodifica("100%"), "100%");
    }
}
