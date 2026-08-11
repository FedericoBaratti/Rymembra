//! Last.fm: una chiave, un segreto, un consenso nel browser e una firma su ogni
//! richiesta.
//!
//! # Il consenso è a due tempi, e in mezzo c'è una persona
//!
//! Non è OAuth e non gli somiglia: non c'è nessun indirizzo di ritorno, nessun
//! socket in ascolto, nessun `state` da confrontare. Si chiede un **token**
//! (`auth.getToken`), si manda l'utente ad approvarlo su `last.fm`, e quando è
//! tornato lo si scambia per una **chiave di sessione** (`auth.getSession`) che
//! non scade mai.
//!
//! Fra il primo e il secondo tempo non succede niente di osservabile da qui:
//! Last.fm non avvisa nessuno che l'utente ha approvato. L'unico segnale è la
//! persona che torna sulla finestra e dice «fatto». È il motivo per cui esiste
//! [`ErrorCode::SettingsLastfmNoPendingToken`](aether_domain::errors::ErrorCode)
//! — il secondo tempo chiesto quando il primo non è mai stato fatto, o è stato
//! fatto prima di una chiusura dell'applicazione.
//!
//! # La firma, e perché è la cosa che si sbaglia
//!
//! Ogni richiesta autenticata porta un `api_sig`: l'MD5 dei parametri ordinati
//! per nome, concatenati come `nomevalore` senza separatori, con il segreto in
//! fondo. Tre trappole, e tutte e tre danno lo stesso errore inutile
//! (`Invalid method signature supplied`, codice 13):
//!
//! 1. **`format` non si firma.** Va mandato, ma sta fuori dalla stringa firmata.
//! 2. **L'ordine è quello dei nomi come stringhe**, non quello logico: con i
//!    parametri indicizzati di `track.scrobble`, `artist[10]` viene prima di
//!    `artist[2]`. Va bene, purché sia lo stesso ordine che usa il servizio — e
//!    lo è, perché anche lui ordina i nomi come stringhe.
//! 3. **Si firmano i valori in chiaro**, non quelli codificati per l'URL. Un
//!    titolo con un `&` firmato dopo la codifica non corrisponde a niente.
//!
//! # I limiti, che sono stretti
//!
//! Cinquanta scrobble per richiesta. E soprattutto: Last.fm **rifiuta le date
//! troppo vecchie** — un ascolto di due anni fa viene «ignorato, codice 3» — e
//! ha un tetto giornaliero. È la ragione tecnica per cui la cronologia importata
//! da Spotify va a ListenBrainz e non qui: non è una preferenza, è che qui non
//! entrerebbe.
//!
//! # Un ascolto ignorato è un ascolto **consegnato**
//!
//! La distinzione che decide il destino di una riga in coda. Se Last.fm risponde
//! `200` dicendo «accettati 3, ignorati 2», quei due non torneranno mai
//! accettati: la data è quella che è, l'artista è quello che è. Rimetterli in
//! coda vorrebbe dire rimandarli per sempre. Si tolgono dalla coda come gli
//! altri, e il motivo dell'ignoro finisce nel rapporto.

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_net::{Corpo, Metodo, Rete, Richiesta, percento};
use md5::{Digest as _, Md5};

use crate::{Ascolto, Servizio, esadecimale};

/// Il punto delle API.
pub const PUNTO: &str = "https://ws.audioscrobbler.com/2.0/";

/// Dove si manda l'utente a dare il consenso.
const CONSENSO: &str = "https://www.last.fm/api/auth/";

/// Quanti ascolti stanno in una richiesta. Il limite del servizio.
pub const MASSIMI_PER_RICHIESTA: usize = 50;

/// Quanto si aspetta una risposta.
pub const SCADENZA: Duration = Duration::from_secs(30);

/// Quante volte si riprova una richiesta ritentabile.
const TENTATIVI: u32 = 3;

/// Il client di Last.fm.
///
/// [`Debug`] scritto a mano, come per ListenBrainz: il segreto e la chiave di
/// sessione stanno qui dentro.
pub struct LastFm {
    rete: Rete,
    api_key: String,
    segreto: String,
    punto: String,
}

impl std::fmt::Debug for LastFm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LastFm")
            .field("punto", &self.punto)
            .field("api_key", &self.api_key)
            .field("segreto", &"…")
            .finish()
    }
}

/// La sessione, che non scade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sessione {
    /// Il nome utente.
    pub utente: String,
    /// La chiave di sessione. Va nel portachiavi, mai in `settings`.
    pub chiave: String,
}

/// Com'è andato un invio.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Esito {
    /// Quanti Last.fm ha accettato.
    pub accettati: u32,
    /// Quanti ha ricevuto e scartato.
    pub ignorati: u32,
    /// Perché li ha scartati, una voce per motivo distinto.
    ///
    /// Distinti e non uno per ascolto: cinquanta ascolti rifiutati per la stessa
    /// ragione sono una riga da mostrare, non cinquanta.
    pub motivi: Vec<String>,
}

impl LastFm {
    /// Un client con le credenziali dell'utente.
    #[must_use]
    pub fn nuovo(api_key: impl Into<String>, segreto: impl Into<String>) -> Self {
        Self::verso(PUNTO, api_key, segreto)
    }

    /// Un client verso un altro punto. Serve alle prove.
    #[must_use]
    pub fn verso(
        punto: impl Into<String>,
        api_key: impl Into<String>,
        segreto: impl Into<String>,
    ) -> Self {
        Self {
            rete: Rete::nuova("lastfm", SCADENZA),
            api_key: api_key.into(),
            segreto: segreto.into(),
            punto: punto.into(),
        }
    }

    /// Primo tempo: chiede un token da far approvare.
    ///
    /// # Errori
    ///
    /// `settings.scrobbleAuthRejected` se la chiave o il segreto non vanno, un
    /// errore di rete altrimenti.
    pub fn chiedi_token(&self) -> Result<String, AppError> {
        let documento = self.chiama(&[
            ("method".to_owned(), "auth.getToken".to_owned()),
            ("api_key".to_owned(), self.api_key.clone()),
        ])?;
        documento
            .get("token")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AppError::new(ErrorCode::SettingsScrobbleAuthRejected {
                    service: Servizio::LastFm.chiave().to_owned(),
                })
                .with_cause("Last.fm non ha mandato nessun token")
            })
    }

    /// L'indirizzo a cui mandare l'utente perché approvi il token.
    ///
    /// Si apre nel **browser di sistema**, come il consenso di Google e quello
    /// di Spotify: una pagina di accesso dentro una finestra dell'applicazione
    /// è una pagina in cui l'utente non può verificare a chi sta dando la
    /// password.
    #[must_use]
    pub fn url_consenso(&self, token: &str) -> String {
        format!(
            "{CONSENSO}?api_key={}&token={}",
            percento(&self.api_key),
            percento(token)
        )
    }

    /// Secondo tempo: scambia il token approvato per una sessione.
    ///
    /// # Errori
    ///
    /// `settings.scrobbleAuthRejected` se il token non è stato approvato, è
    /// scaduto (un'ora) o è già stato usato.
    pub fn sessione(&self, token: &str) -> Result<Sessione, AppError> {
        let documento = self.chiama(&[
            ("method".to_owned(), "auth.getSession".to_owned()),
            ("api_key".to_owned(), self.api_key.clone()),
            ("token".to_owned(), token.to_owned()),
        ])?;
        let sessione = documento.get("session");
        let chiave = sessione
            .and_then(|s| s.get("key"))
            .and_then(serde_json::Value::as_str);
        let utente = sessione
            .and_then(|s| s.get("name"))
            .and_then(serde_json::Value::as_str);
        match (chiave, utente) {
            (Some(chiave), Some(utente)) => Ok(Sessione {
                utente: utente.to_owned(),
                chiave: chiave.to_owned(),
            }),
            _ => Err(AppError::new(ErrorCode::SettingsScrobbleAuthRejected {
                service: Servizio::LastFm.chiave().to_owned(),
            })
            .with_cause("Last.fm non ha mandato nessuna sessione")),
        }
    }

    /// Dichiara cosa sta suonando adesso.
    ///
    /// Come il `playing_now` di ListenBrainz: non registra niente, non entra in
    /// coda, si manda subito o non si manda.
    ///
    /// # Errori
    ///
    /// Come [`Self::invia`].
    pub fn adesso(&self, sessione: &str, ascolto: &Ascolto) -> Result<(), AppError> {
        let mut parametri = vec![
            ("method".to_owned(), "track.updateNowPlaying".to_owned()),
            ("api_key".to_owned(), self.api_key.clone()),
            ("sk".to_owned(), sessione.to_owned()),
        ];
        parametri.extend(campi_brano(ascolto, None));
        self.chiama(&parametri).map(|_| ())
    }

    /// Manda ascolti veri.
    ///
    /// # Errori
    ///
    /// `settings.scrobbleAuthRejected` (sessione revocata: l'utente ha tolto
    /// l'accesso dalla propria pagina), `settings.scrobbleRejected` (firma o
    /// parametri sbagliati), o un errore di rete ritentabile.
    ///
    /// Gli ascolti **ignorati** non sono un errore: tornano dentro [`Esito`].
    pub fn invia(&self, sessione: &str, ascolti: &[Ascolto]) -> Result<Esito, AppError> {
        if ascolti.is_empty() {
            return Ok(Esito::default());
        }
        if ascolti.len() > MASSIMI_PER_RICHIESTA {
            return Err(AppError::new(ErrorCode::SettingsScrobbleRejected {
                service: Servizio::LastFm.chiave().to_owned(),
                detail: Some(format!(
                    "{} ascolti in una richiesta, il massimo è {MASSIMI_PER_RICHIESTA}",
                    ascolti.len()
                )),
            }));
        }

        let mut parametri = vec![
            ("method".to_owned(), "track.scrobble".to_owned()),
            ("api_key".to_owned(), self.api_key.clone()),
            ("sk".to_owned(), sessione.to_owned()),
        ];
        for (i, ascolto) in ascolti.iter().enumerate() {
            parametri.extend(campi_brano(ascolto, Some(i)));
        }
        let documento = self.chiama(&parametri)?;
        Ok(leggi_esito(&documento))
    }

    /// Firma, manda, e traduce quel che torna.
    ///
    /// Tutti i metodi passano di qui: la firma non è un passo che si possa
    /// dimenticare in un punto solo.
    fn chiama(&self, parametri: &[(String, String)]) -> Result<serde_json::Value, AppError> {
        let mut da_mandare = parametri.to_vec();
        da_mandare.push(("api_sig".to_owned(), firma(parametri, &self.segreto)));
        // Dopo la firma, sempre: `format` non si firma. Vedi il preambolo.
        da_mandare.push(("format".to_owned(), "json".to_owned()));

        let campi: Vec<(&str, String)> = da_mandare
            .iter()
            .map(|(nome, valore)| (nome.as_str(), valore.clone()))
            .collect();

        let risposta = self.rete.ritenta(TENTATIVI, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Post,
                url: &self.punto,
                intestazioni: &[],
                corpo: Corpo::Modulo(&campi),
            })?;

            // Il corpo si legge **sempre**, andata o no: Last.fm dichiara i suoi
            // guasti nel documento, e a volte con uno stato `200`. Fidarsi dello
            // stato vorrebbe dire prendere un «sessione non valida» per un
            // invio riuscito, e cancellare dalla coda ascolti mai arrivati.
            let documento: serde_json::Value = match serde_json::from_slice(&risposta.corpo) {
                Ok(d) => d,
                Err(err) => {
                    return if risposta.e_andata() {
                        Err(self.rifiuto(Some(format!("risposta illeggibile: {err}"))))
                    } else {
                        Err(self.rete.stato_a_errore(&risposta, &self.punto))
                    };
                }
            };

            if let Some(err) = self.guasto_dichiarato(&documento) {
                return Err(err);
            }
            if risposta.e_andata() {
                Ok(documento)
            } else {
                Err(self.rete.stato_a_errore(&risposta, &self.punto))
            }
        })?;
        Ok(risposta)
    }

    /// Il guasto che il documento dichiara, se ne dichiara uno.
    ///
    /// La tabella dei codici di Last.fm, ridotta a quel che cambia il
    /// comportamento: chi va rifatto da capo, chi si riprova e chi si butta.
    fn guasto_dichiarato(&self, documento: &serde_json::Value) -> Option<AppError> {
        let codice = documento.get("error").and_then(serde_json::Value::as_i64)?;
        let messaggio = documento
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Last.fm ha rifiutato la richiesta")
            .to_owned();
        let servizio = Servizio::LastFm.chiave().to_owned();

        Some(match codice {
            // 4 autenticazione fallita, 9 sessione non valida, 14 token non
            // autorizzato, 15 token scaduto. Tutti e quattro vogliono che
            // l'utente rifaccia il consenso: ritentarli è tempo perso.
            4 | 9 | 14 | 15 => {
                AppError::new(ErrorCode::SettingsScrobbleAuthRejected { service: servizio })
                    .with_cause(format!("{codice}: {messaggio}"))
            }
            // 11 e 16 il servizio è temporaneamente giù, 29 troppe richieste.
            // Questi sì, e con l'attesa che raddoppia.
            11 | 16 | 29 => AppError::new(ErrorCode::NetRateLimited {
                service: Some(Servizio::LastFm.nome().to_owned()),
                retry_after_ms: None,
            })
            .with_cause(format!("{codice}: {messaggio}")),
            // Tutto il resto — parametri mancanti (6), firma sbagliata (13),
            // chiave sospesa (26) — è colpa di come è stata composta la
            // richiesta, e riproporla identica dà lo stesso esito.
            _ => self.rifiuto(Some(format!("{codice}: {messaggio}"))),
        })
    }

    /// Un rifiuto non ritentabile.
    fn rifiuto(&self, dettaglio: Option<String>) -> AppError {
        AppError::new(ErrorCode::SettingsScrobbleRejected {
            service: Servizio::LastFm.chiave().to_owned(),
            detail: dettaglio,
        })
    }
}

/// I campi di un brano, con o senza l'indice del lotto.
///
/// Con l'indice per `track.scrobble` (`artist[0]`, `track[0]`, …), senza per
/// `track.updateNowPlaying`, che ne prende uno solo. Una funzione sola per tutti
/// e due perché i nomi dei campi sono gli stessi, e due elenchi che divergono
/// sono due firme che non corrispondono.
fn campi_brano(ascolto: &Ascolto, indice: Option<usize>) -> Vec<(String, String)> {
    let nome = |base: &str| match indice {
        Some(i) => format!("{base}[{i}]"),
        None => base.to_owned(),
    };

    let mut campi = vec![
        (nome("artist"), ascolto.artista.clone()),
        (nome("track"), ascolto.titolo.clone()),
    ];
    if let Some(i) = indice {
        // La data solo negli scrobble veri: `updateNowPlaying` non la vuole, e
        // mandargliela è un parametro sconosciuto.
        let _ = i;
        campi.push((nome("timestamp"), ascolto.quando_s.to_string()));
    }
    if let Some(album) = &ascolto.album {
        campi.push((nome("album"), album.clone()));
    }
    if let Some(artista) = &ascolto.artista_album {
        campi.push((nome("albumArtist"), artista.clone()));
    }
    if let Some(n) = &ascolto.numero_traccia {
        campi.push((nome("trackNumber"), n.to_string()));
    }
    if let Some(mbid) = &ascolto.mbid_registrazione {
        campi.push((nome("mbid"), mbid.clone()));
    }
    if let Some(ms) = ascolto.durata_ms {
        campi.push((nome("duration"), secondi_di(ms).to_string()));
    }
    campi
}

/// La durata in secondi, che è l'unità che Last.fm chiede.
///
/// Mandare millisecondi darebbe un brano lungo sessantotto ore, e **nessun
/// errore**: il campo è facoltativo e il servizio lo prende com'è.
///
/// La divisione è troncata di proposito — un brano di 245,5 secondi ne dura 245
/// per Last.fm, e mezzo secondo non cambia niente per nessuno.
#[expect(
    clippy::integer_division,
    reason = "da millisecondi a secondi: il resto è quel che si vuole buttare"
)]
const fn secondi_di(ms: u64) -> u64 {
    ms / 1000
}

/// La stringa che si firma.
///
/// Separata dalla firma apposta: l'MD5 è una funzione che non sbaglia, mentre
/// **questa** è la parte in cui si sbaglia — l'ordine, l'esclusione di `format`,
/// i valori in chiaro. È l'unica delle due che vale la pena provare.
fn stringa_da_firmare(parametri: &[(String, String)]) -> String {
    let mut ordinati: Vec<&(String, String)> = parametri
        .iter()
        // `callback` non c'è mai in un client desktop, ma sta nella stessa
        // riga della documentazione di `format` e costa una condizione.
        .filter(|(nome, _)| nome != "format" && nome != "callback" && nome != "api_sig")
        .collect();
    ordinati.sort_by(|(a, _), (b, _)| a.cmp(b));

    ordinati
        .into_iter()
        .fold(String::new(), |mut testo, (nome, valore)| {
            testo.push_str(nome);
            testo.push_str(valore);
            testo
        })
}

/// L'`api_sig` di una richiesta.
///
/// # Esempio
///
/// Senza parametri resta il solo segreto, il che rende la funzione verificabile
/// con un vettore di prova pubblico di MD5:
///
/// ```
/// # use aether_scrobble::lastfm::firma;
/// assert_eq!(firma(&[], "abc"), "900150983cd24fb0d6963f7d28e17f72");
/// ```
#[must_use]
pub fn firma(parametri: &[(String, String)], segreto: &str) -> String {
    let mut hash = Md5::new();
    hash.update(stringa_da_firmare(parametri).as_bytes());
    hash.update(segreto.as_bytes());
    esadecimale(&hash.finalize())
}

/// Quanti ne ha presi, quanti ne ha scartati e perché.
///
/// # La forma che cambia con il numero
///
/// Con un solo scrobble `scrobble` è un oggetto; con due o più è un elenco. È la
/// classica trappola delle API che serializzano da XML, e leggerne solo una
/// forma vuol dire perdere i motivi degli ignorati esattamente nel caso in cui
/// se ne manda uno alla volta — cioè quando si ascolta normalmente.
fn leggi_esito(documento: &serde_json::Value) -> Esito {
    let scrobbles = documento.get("scrobbles");
    let attributi = scrobbles.and_then(|s| s.get("@attr"));
    let numero = |campo: &str| -> u32 {
        attributi
            .and_then(|a| a.get(campo))
            .and_then(|v| {
                // Last.fm manda questi due a volte come numeri e a volte come
                // stringhe, nello stesso documento.
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|t| t.parse().ok()))
            })
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };

    let voci: Vec<&serde_json::Value> = match scrobbles.and_then(|s| s.get("scrobble")) {
        Some(serde_json::Value::Array(elenco)) => elenco.iter().collect(),
        Some(oggetto) => vec![oggetto],
        None => Vec::new(),
    };

    let mut motivi: Vec<String> = Vec::new();
    for voce in voci {
        let Some(ignorato) = voce.get("ignoredMessage") else {
            continue;
        };
        let codice = ignorato
            .get("code")
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|t| t.parse().ok()))
            })
            .unwrap_or(0);
        if codice == 0 {
            continue;
        }
        let motivo = motivo_ignorato(codice);
        if !motivi.iter().any(|m| m == motivo) {
            motivi.push(motivo.to_owned());
        }
    }

    Esito {
        accettati: numero("accepted"),
        ignorati: numero("ignored"),
        motivi,
    }
}

/// Perché Last.fm ha scartato uno scrobble.
///
/// I codici sono suoi; le frasi sono nostre, e dicono cosa può farci l'utente.
const fn motivo_ignorato(codice: u64) -> &'static str {
    match codice {
        1 => "artista ignorato da Last.fm",
        2 => "brano ignorato da Last.fm",
        3 => "l'ascolto è troppo vecchio",
        4 => "l'ascolto è nel futuro",
        5 => "limite giornaliero di scrobble superato",
        _ => "scartato da Last.fm senza un motivo noto",
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn param(coppie: &[(&str, &str)]) -> Vec<(String, String)> {
        coppie
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn la_stringa_firmata_e_ordinata_per_nome() {
        let p = param(&[
            ("method", "auth.getSession"),
            ("api_key", "chiave"),
            ("token", "gettone"),
        ]);
        assert_eq!(
            stringa_da_firmare(&p),
            "api_keychiavemethodauth.getSessiontokengettone"
        );
    }

    #[test]
    fn il_formato_non_si_firma() {
        let p = param(&[
            ("api_key", "k"),
            ("format", "json"),
            ("method", "m"),
            ("api_sig", "gia-fatta"),
            ("callback", "boh"),
        ]);
        assert_eq!(
            stringa_da_firmare(&p),
            "api_keykmethodm",
            "format, callback e la firma stessa stanno fuori: dentro darebbero un codice 13"
        );
    }

    #[test]
    fn i_valori_si_firmano_in_chiaro() {
        let p = param(&[("track", "Sympathy for the Devil & Co"), ("api_key", "k")]);
        assert_eq!(
            stringa_da_firmare(&p),
            "api_keyktrackSympathy for the Devil & Co",
            "firmare il valore già codificato per l'URL non corrisponde a niente"
        );
    }

    #[test]
    fn la_firma_e_un_md5_vero() {
        // Vettori di prova pubblici di MD5: senza parametri la stringa firmata è
        // il solo segreto. Prova il collegamento con il digest, non il nostro
        // ordinamento — quello ha le sue prove sopra.
        assert_eq!(firma(&[], ""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(firma(&[], "abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            firma(&param(&[("a", "b")]), "c"),
            // md5("abc")
            "900150983cd24fb0d6963f7d28e17f72",
            "nome + valore + segreto, senza separatori"
        );
    }

    #[test]
    fn i_campi_indicizzati_hanno_la_forma_del_lotto() {
        let mut a = Ascolto::nuovo("Verdena", "Luna", 1_700_000_000);
        a.album = Some("Solo un grande sasso".to_owned());
        a.durata_ms = Some(245_500);
        a.numero_traccia = Some(4);

        let campi = campi_brano(&a, Some(2));
        let trova = |nome: &str| {
            campi
                .iter()
                .find(|(n, _)| n == nome)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(trova("artist[2]"), Some("Verdena"));
        assert_eq!(trova("track[2]"), Some("Luna"));
        assert_eq!(trova("timestamp[2]"), Some("1700000000"));
        assert_eq!(trova("album[2]"), Some("Solo un grande sasso"));
        assert_eq!(trova("trackNumber[2]"), Some("4"));
        assert_eq!(
            trova("duration[2]"),
            Some("245"),
            "Last.fm vuole secondi: in millisecondi sarebbe un brano di sessantotto ore"
        );
    }

    #[test]
    fn il_now_playing_non_porta_la_data() {
        let a = Ascolto::nuovo("Verdena", "Luna", 1_700_000_000);
        let campi = campi_brano(&a, None);
        assert!(campi.iter().any(|(n, _)| n == "artist"));
        assert!(
            !campi.iter().any(|(n, _)| n.starts_with("timestamp")),
            "updateNowPlaying non ha un parametro timestamp"
        );
    }

    #[test]
    fn l_url_del_consenso_codifica_quel_che_ci_mette() {
        let client = LastFm::nuovo("chiave/strana", "segreto");
        let url = client.url_consenso("gettone+con spazi");
        assert!(url.starts_with("https://www.last.fm/api/auth/?api_key="));
        assert!(url.contains("chiave%2Fstrana"));
        assert!(url.contains("gettone%2Bcon%20spazi"));
        assert!(
            !url.contains("segreto"),
            "il segreto non esce mai, tantomeno in un indirizzo che finisce nella cronologia del browser"
        );
    }

    #[test]
    fn un_esito_con_un_solo_scrobble_si_legge_lo_stesso() {
        // Con uno solo `scrobble` è un oggetto, non un elenco.
        let d: serde_json::Value = serde_json::from_str(
            r##"{"scrobbles":{"@attr":{"accepted":0,"ignored":1},
                  "scrobble":{"ignoredMessage":{"code":"3","#text":"Timestamp too old"},
                              "track":{"#text":"Luna"}}}}"##,
        )
        .expect("json di prova valido");
        let esito = leggi_esito(&d);
        assert_eq!(esito.accettati, 0);
        assert_eq!(esito.ignorati, 1);
        assert_eq!(esito.motivi, vec!["l'ascolto è troppo vecchio".to_owned()]);
    }

    #[test]
    fn un_esito_con_molti_scrobble_raggruppa_i_motivi() {
        let d: serde_json::Value = serde_json::from_str(
            r#"{"scrobbles":{"@attr":{"accepted":"1","ignored":"2"},
                 "scrobble":[
                   {"ignoredMessage":{"code":"0"}},
                   {"ignoredMessage":{"code":"3"}},
                   {"ignoredMessage":{"code":"3"}}]}}"#,
        )
        .expect("json di prova valido");
        let esito = leggi_esito(&d);
        assert_eq!(esito.accettati, 1);
        assert_eq!(esito.ignorati, 2);
        assert_eq!(
            esito.motivi.len(),
            1,
            "due ignorati per lo stesso motivo sono una riga da mostrare, non due"
        );
    }

    #[test]
    fn una_sessione_revocata_non_si_ritenta_e_una_manutenzione_si() {
        let client = LastFm::nuovo("k", "s");
        let dichiarato = |codice: i64| {
            client
                .guasto_dichiarato(&serde_json::json!({ "error": codice, "message": "boh" }))
                .expect("un codice d'errore produce sempre un guasto")
        };

        let revocata = dichiarato(9);
        assert_eq!(
            revocata.code().kind().code(),
            "settings.scrobbleAuthRejected"
        );
        assert!(!revocata.is_retryable());

        let manutenzione = dichiarato(16);
        assert!(
            manutenzione.is_retryable(),
            "«temporaneamente non disponibile» vuol dire riprova, ed è l'unico modo di non perdere l'ascolto"
        );

        let firma_storta = dichiarato(13);
        assert_eq!(
            firma_storta.code().kind().code(),
            "settings.scrobbleRejected"
        );
        assert!(
            !firma_storta.is_retryable(),
            "la stessa firma sbagliata resta sbagliata"
        );

        assert!(
            client
                .guasto_dichiarato(&serde_json::json!({ "scrobbles": {} }))
                .is_none(),
            "un documento senza `error` non è un guasto"
        );
    }

    #[test]
    fn un_invio_vuoto_non_esce_in_rete() {
        let client = LastFm::verso("https://esempio.invalido/", "k", "s");
        assert_eq!(client.invia("sk", &[]), Ok(Esito::default()));
    }

    #[test]
    fn un_blocco_troppo_grande_e_un_rifiuto() {
        let client = LastFm::verso("https://esempio.invalido/", "k", "s");
        let troppi: Vec<Ascolto> = (0..=MASSIMI_PER_RICHIESTA)
            .map(|i| Ascolto::nuovo("A", "B", i64::try_from(i).unwrap_or(0)))
            .collect();
        let err = client.invia("sk", &troppi).expect_err("deve rifiutare");
        assert_eq!(err.code().kind().code(), "settings.scrobbleRejected");
    }

    #[test]
    fn il_segreto_non_compare_nel_debug() {
        let client = LastFm::nuovo("chiave-pubblica", "segretissimo");
        let scritto = format!("{client:?}");
        assert!(
            scritto.contains("chiave-pubblica"),
            "la chiave non è un segreto"
        );
        assert!(!scritto.contains("segretissimo"));
    }
}
