//! ListenBrainz: un token, un documento JSON, nessuna firma.
//!
//! È il più semplice dei due protocolli e il primo a essere stato scritto, ma la
//! ragione per cui viene per primo è un'altra: è **l'unico dei due che accetta
//! un'importazione in blocco**. `listen_type: "import"` prende mille ascolti per
//! richiesta con la loro data, e non ha nessun limite su quanto siano vecchi.
//!
//! Questo chiude un cerchio aperto da tre fasi. La cronologia che si importa
//! dall'archivio di Spotify sono anni di ascolti: mandarli a Last.fm è
//! impossibile — rifiuta le date fuori da una finestra ristretta e limita gli
//! scrobble giornalieri — mentre qui entrano tutti, e diventano la propria
//! cronologia d'ascolto su un servizio che non appartiene a nessuna piattaforma.
//!
//! # Perché `playing_now` è a parte
//!
//! Un `playing_now` **non ha data** — mandargliene una è un errore di richiesta,
//! non una svista tollerata — e non si conserva: ListenBrainz lo tiene finché
//! non arriva il successivo e poi lo dimentica. Serve a far comparire «sta
//! ascoltando» sulla pagina dell'utente, non a registrare niente.
//!
//! Ne segue che non entra mai nella coda persistente. Un `playing_now` rimasto
//! indietro perché non c'era rete descrive un brano finito venti minuti fa: è
//! peggio di non mandarlo, perché dice il falso. Si manda subito o non si manda.
//!
//! # I limiti, che sono dichiarati e non indovinati
//!
//! Mille ascolti per richiesta, dieci megabyte per documento, dieci kilobyte per
//! ascolto. Il primo lo si rispetta tagliando i blocchi; gli altri due non si
//! possono raggiungere con i campi che Aether manda — un ascolto con tutti i
//! campi pieni sta in mezzo kilobyte — e non si controllano, perché un controllo
//! che non può scattare è un controllo che non si prova.

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_net::{Corpo, Metodo, Rete, Richiesta, Risposta};

use crate::{Ascolto, CLIENT, Servizio, VERSIONE};

/// Il punto delle API.
pub const PUNTO: &str = "https://api.listenbrainz.org";

/// Quanti ascolti stanno in una richiesta.
///
/// Il limite del servizio, non una prudenza nostra: `MAX_LISTENS_PER_REQUEST`
/// vale mille, e il milleunesimo fa fallire l'intero documento.
pub const MASSIMI_PER_RICHIESTA: usize = 1000;

/// Quanto si aspetta una risposta.
///
/// Generosa rispetto alle altre di Aether perché una richiesta può portare mille
/// ascolti, e il servizio li scrive prima di rispondere.
pub const SCADENZA: Duration = Duration::from_secs(60);

/// Quante volte si riprova una richiesta ritentabile.
const TENTATIVI: u32 = 3;

/// Il client di ListenBrainz.
///
/// Il token è dentro, e non esce: [`Debug`] non è derivato apposta. Un token
/// finito in un log è un token che chiunque legga quel log può usare per
/// scrivere nella cronologia d'ascolto di qualcun altro.
pub struct ListenBrainz {
    rete: Rete,
    token: String,
    punto: String,
}

impl std::fmt::Debug for ListenBrainz {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenBrainz")
            .field("punto", &self.punto)
            .field("token", &"…")
            .finish()
    }
}

/// Chi è il proprietario di un token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proprietario {
    /// Il nome utente su ListenBrainz.
    pub utente: String,
}

impl ListenBrainz {
    /// Un client verso il servizio pubblico.
    #[must_use]
    pub fn nuovo(token: impl Into<String>) -> Self {
        Self::verso(PUNTO, token)
    }

    /// Un client verso un altro punto.
    ///
    /// Esiste per due ragioni vere, non per simmetria: ListenBrainz è software
    /// libero e c'è chi ne ospita un'istanza propria, e le prove di questo
    /// modulo hanno bisogno di un indirizzo che non sia il servizio vero.
    #[must_use]
    pub fn verso(punto: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            rete: Rete::nuova("listenbrainz", SCADENZA),
            token: token.into(),
            punto: punto.into(),
        }
    }

    /// Il token è valido, e di chi è.
    ///
    /// Si chiama **prima** di salvarlo: un token sbagliato incollato nelle
    /// impostazioni non darebbe nessun sintomo finché il primo ascolto non
    /// fallisce, cioè ore dopo e in un posto diverso da dove è stato commesso
    /// l'errore.
    ///
    /// # Errori
    ///
    /// `settings.scrobbleAuthRejected` se il servizio dice che il token non
    /// vale. I guasti di rete arrivano come sono.
    pub fn valida(&self) -> Result<Proprietario, AppError> {
        let url = format!("{}/1/validate-token", self.punto);
        let risposta = self.rete.ritenta(TENTATIVI, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Get,
                url: &url,
                intestazioni: &[("Authorization", &self.autorizzazione())],
                corpo: Corpo::Niente,
            })?;
            if risposta.e_andata() {
                Ok(risposta)
            } else {
                Err(self.tradisci(&risposta, &url))
            }
        })?;

        // Il servizio risponde `200` anche quando il token non vale, dicendolo
        // nel corpo con `valid: false`. È una scelta sua, e va letta: fidarsi
        // dello stato vorrebbe dire accettare come buono qualunque token.
        let documento: serde_json::Value = serde_json::from_slice(&risposta.corpo)
            .map_err(|err| self.illeggibile(&err.to_string()))?;
        if documento.get("valid").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(AppError::new(ErrorCode::SettingsScrobbleAuthRejected {
                service: Servizio::ListenBrainz.chiave().to_owned(),
            })
            .with_cause(
                documento
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("token non valido")
                    .to_owned(),
            ));
        }

        let utente = documento
            .get("user_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(Proprietario { utente })
    }

    /// Dichiara cosa sta suonando adesso.
    ///
    /// Non registra niente e non entra in coda: vedi il preambolo.
    ///
    /// # Errori
    ///
    /// Come [`Self::manda`].
    pub fn adesso(&self, ascolto: &Ascolto) -> Result<(), AppError> {
        self.manda(&documento("playing_now", std::slice::from_ref(ascolto)))
    }

    /// Manda ascolti veri.
    ///
    /// Un blocco solo, che deve stare nei [`MASSIMI_PER_RICHIESTA`]: chi ne ha
    /// di più li taglia — la coda in `aether-app` lo fa già, leggendone quanti
    /// ne stanno in una richiesta.
    ///
    /// # Errori
    ///
    /// `settings.scrobbleAuthRejected` (token revocato),
    /// `settings.scrobbleRejected` (il documento non va bene, e rimandarlo darà
    /// lo stesso esito), o un errore di rete ritentabile.
    pub fn invia(&self, ascolti: &[Ascolto]) -> Result<(), AppError> {
        if ascolti.is_empty() {
            return Ok(());
        }
        if ascolti.len() > MASSIMI_PER_RICHIESTA {
            return Err(AppError::new(ErrorCode::SettingsScrobbleRejected {
                service: Servizio::ListenBrainz.chiave().to_owned(),
                detail: Some(format!(
                    "{} ascolti in una richiesta, il massimo è {MASSIMI_PER_RICHIESTA}",
                    ascolti.len()
                )),
            }));
        }
        self.manda(&documento(tipo_invio(ascolti.len()), ascolti))
    }

    /// La spedizione vera e propria, comune a tutti e tre i tipi di invio.
    fn manda(&self, documento: &serde_json::Value) -> Result<(), AppError> {
        let url = format!("{}/1/submit-listens", self.punto);
        let corpo = serde_json::to_vec(documento)
            .map_err(|err| self.illeggibile(&format!("documento non serializzabile: {err}")))?;

        self.rete.ritenta(TENTATIVI, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Post,
                url: &url,
                intestazioni: &[("Authorization", &self.autorizzazione())],
                corpo: Corpo::Byte {
                    tipo: "application/json",
                    dati: &corpo,
                },
            })?;
            if risposta.e_andata() {
                Ok(())
            } else {
                Err(self.tradisci(&risposta, &url))
            }
        })
    }

    /// L'intestazione di autorizzazione. `Token`, non `Bearer`.
    fn autorizzazione(&self) -> String {
        format!("Token {}", self.token)
    }

    /// Cosa significa uno stato che non è andato.
    ///
    /// Tre esiti diversi, e la differenza è tutta nel destino della riga in
    /// coda: `401` la fa scollegare, `400` la fa buttare, tutto il resto la fa
    /// riprovare.
    fn tradisci(&self, risposta: &Risposta, url: &str) -> AppError {
        let dettaglio = motivo(risposta);
        match risposta.stato {
            401 | 403 => AppError::new(ErrorCode::SettingsScrobbleAuthRejected {
                service: Servizio::ListenBrainz.chiave().to_owned(),
            })
            .with_cause(dettaglio.unwrap_or_else(|| "token rifiutato".to_owned())),
            400 | 413 => AppError::new(ErrorCode::SettingsScrobbleRejected {
                service: Servizio::ListenBrainz.chiave().to_owned(),
                detail: dettaglio,
            }),
            // Tutto il resto passa per la traduzione normale, che è quella che
            // sa già quali stati si ritentano e per quanto: una seconda tabella
            // qui è il modo documentato in `catalog.rs` in cui lo stesso guasto
            // finisce classificato in due modi.
            _ => self.rete.stato_a_errore(risposta, url),
        }
    }

    /// Una risposta che non si riesce a leggere.
    fn illeggibile(&self, causa: &str) -> AppError {
        AppError::new(ErrorCode::SettingsScrobbleRejected {
            service: Servizio::ListenBrainz.chiave().to_owned(),
            detail: None,
        })
        .with_cause(causa.to_owned())
    }
}

/// `single` per uno, `import` per molti.
///
/// Non è cosmetica: `single` con più di un ascolto nel carico è un errore di
/// richiesta, e `import` con uno solo funziona ma dichiara il falso — quel che
/// il servizio ne fa è mostrarlo o no fra gli ascolti recenti.
const fn tipo_invio(quanti: usize) -> &'static str {
    if quanti == 1 { "single" } else { "import" }
}

/// Il documento da mandare.
///
/// Una funzione pura, e separata dalla spedizione apposta: è la parte che si può
/// sbagliare in silenzio — un campo con il nome storto viene ignorato dal
/// servizio senza nessun errore — ed è quindi l'unica che vale davvero la pena
/// provare senza rete.
fn documento(tipo: &str, ascolti: &[Ascolto]) -> serde_json::Value {
    let carico: Vec<serde_json::Value> = ascolti
        .iter()
        .map(|a| {
            let mut voce = serde_json::Map::new();
            // Il `playing_now` **non** porta la data. Non è tollerato: il
            // servizio rifiuta il documento.
            if tipo != "playing_now" {
                voce.insert("listened_at".to_owned(), a.quando_s.into());
            }

            let mut extra = serde_json::Map::new();
            extra.insert("submission_client".to_owned(), CLIENT.into());
            extra.insert("submission_client_version".to_owned(), VERSIONE.into());
            extra.insert("media_player".to_owned(), CLIENT.into());
            if let Some(ms) = a.durata_ms {
                extra.insert("duration_ms".to_owned(), ms.into());
            }
            if let Some(n) = a.numero_traccia {
                extra.insert("tracknumber".to_owned(), n.into());
            }
            if let Some(mbid) = &a.mbid_registrazione {
                extra.insert("recording_mbid".to_owned(), mbid.as_str().into());
            }

            let mut tag = serde_json::Map::new();
            tag.insert("artist_name".to_owned(), a.artista.as_str().into());
            tag.insert("track_name".to_owned(), a.titolo.as_str().into());
            if let Some(album) = &a.album {
                tag.insert("release_name".to_owned(), album.as_str().into());
            }
            tag.insert("additional_info".to_owned(), extra.into());

            voce.insert("track_metadata".to_owned(), tag.into());
            voce.into()
        })
        .collect();

    serde_json::json!({ "listen_type": tipo, "payload": carico })
}

/// Cosa ha detto il servizio, quando l'ha detto.
///
/// ListenBrainz mette il motivo in `error` dentro un JSON. Quando non è JSON —
/// un proxy di mezzo, una pagina di manutenzione — si ripiega sul corpo tagliato:
/// una causa brutta è meglio di nessuna causa.
fn motivo(risposta: &Risposta) -> Option<String> {
    let testo = risposta.testo();
    if testo.trim().is_empty() {
        return None;
    }
    let messaggio = serde_json::from_str::<serde_json::Value>(&testo)
        .ok()
        .and_then(|d| {
            d.get("error")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        });
    Some(messaggio.unwrap_or_else(|| testo.chars().take(200).collect()))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn ascolto(titolo: &str, quando: i64) -> Ascolto {
        let mut a = Ascolto::nuovo("Massimo Volume", titolo, quando);
        a.album = Some("Lungo i bordi".to_owned());
        a.durata_ms = Some(312_000);
        a.numero_traccia = Some(3);
        a
    }

    #[test]
    fn un_ascolto_solo_e_un_single_e_ha_la_data() {
        let d = documento("single", &[ascolto("Stanze", 1_700_000_000)]);
        assert_eq!(d["listen_type"], "single");
        assert_eq!(d["payload"][0]["listened_at"], 1_700_000_000_i64);
        assert_eq!(
            d["payload"][0]["track_metadata"]["artist_name"],
            "Massimo Volume"
        );
        assert_eq!(d["payload"][0]["track_metadata"]["track_name"], "Stanze");
        assert_eq!(
            d["payload"][0]["track_metadata"]["release_name"],
            "Lungo i bordi"
        );
    }

    #[test]
    fn il_playing_now_non_porta_la_data() {
        let d = documento("playing_now", &[ascolto("Stanze", 1_700_000_000)]);
        assert_eq!(d["listen_type"], "playing_now");
        assert!(
            d["payload"][0].get("listened_at").is_none(),
            "una data su un playing_now fa rifiutare il documento intero"
        );
        // Il resto c'è comunque: è quel che il servizio mostra come «sta
        // ascoltando».
        assert_eq!(d["payload"][0]["track_metadata"]["track_name"], "Stanze");
    }

    #[test]
    fn molti_ascolti_sono_un_import() {
        assert_eq!(tipo_invio(1), "single");
        assert_eq!(tipo_invio(2), "import");
        assert_eq!(tipo_invio(1000), "import");
        // Zero non arriva mai qui: `invia` esce prima. Se ci arrivasse, `import`
        // con un carico vuoto è comunque un documento valido.
        assert_eq!(tipo_invio(0), "import");
    }

    #[test]
    fn il_client_si_dichiara() {
        let d = documento("single", &[ascolto("Stanze", 1)]);
        let extra = &d["payload"][0]["track_metadata"]["additional_info"];
        assert_eq!(extra["submission_client"], "Aether");
        assert_eq!(extra["submission_client_version"], VERSIONE);
        assert_eq!(extra["duration_ms"], 312_000);
        assert_eq!(extra["tracknumber"], 3);
    }

    #[test]
    fn i_campi_assenti_non_finiscono_nel_documento_come_null() {
        // Un `release_name: null` non è la stessa cosa di un `release_name`
        // assente: il primo è un campo dichiarato vuoto, e il servizio lo
        // scriverebbe come tale sulla riga dell'ascolto.
        let d = documento("single", &[Ascolto::nuovo("CCCP", "Emilia paranoica", 42)]);
        let tag = &d["payload"][0]["track_metadata"];
        assert!(tag.get("release_name").is_none());
        assert!(tag["additional_info"].get("duration_ms").is_none());
        assert!(tag["additional_info"].get("recording_mbid").is_none());
    }

    #[test]
    fn il_mbid_passa_quando_c_e() {
        let mut a = ascolto("Stanze", 1);
        a.mbid_registrazione = Some("2b3f8f4a-0000-4000-8000-abcdefabcdef".to_owned());
        let d = documento("single", &[a]);
        assert_eq!(
            d["payload"][0]["track_metadata"]["additional_info"]["recording_mbid"],
            "2b3f8f4a-0000-4000-8000-abcdefabcdef"
        );
    }

    #[test]
    fn un_blocco_troppo_grande_e_un_rifiuto_non_un_tentativo() {
        let cliente = ListenBrainz::verso("https://esempio.invalido", "token");
        let troppi: Vec<Ascolto> = (0..=MASSIMI_PER_RICHIESTA)
            .map(|i| ascolto("Stanze", i64::try_from(i).unwrap_or(0)))
            .collect();
        let err = cliente.invia(&troppi).expect_err("deve rifiutare");
        assert_eq!(err.code().kind().code(), "settings.scrobbleRejected");
        assert!(!err.is_retryable(), "riprovare darebbe lo stesso esito");
    }

    #[test]
    fn un_invio_vuoto_non_esce_in_rete() {
        // L'indirizzo non esiste: se uscisse davvero, questa prova fallirebbe
        // con un errore di rete invece che passare.
        let cliente = ListenBrainz::verso("https://esempio.invalido", "token");
        assert!(cliente.invia(&[]).is_ok());
    }

    #[test]
    fn il_motivo_si_legge_dal_json_e_altrimenti_dal_corpo() {
        let con = |corpo: &str| Risposta {
            stato: 400,
            corpo: corpo.as_bytes().to_vec(),
            riprova_fra_ms: None,
            posizione: None,
            url_finale: "https://x".to_owned(),
        };
        assert_eq!(
            motivo(&con(
                r#"{"code":400,"error":"Value for key listened_at is invalid"}"#
            )),
            Some("Value for key listened_at is invalid".to_owned())
        );
        assert_eq!(
            motivo(&con("<html>502 Bad Gateway</html>")),
            Some("<html>502 Bad Gateway</html>".to_owned()),
            "una causa brutta è meglio di nessuna causa"
        );
        assert_eq!(motivo(&con("   ")), None);
    }

    #[test]
    fn il_token_non_compare_nel_debug() {
        let cliente = ListenBrainz::nuovo("lbz-segretissimo-000");
        let scritto = format!("{cliente:?}");
        assert!(
            !scritto.contains("segretissimo"),
            "un token in un log è un token utilizzabile da chi legge quel log"
        );
    }
}
