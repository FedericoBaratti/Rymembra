//! La stretta di mano che trasforma «nessuna credenziale» in due gettoni.
//!
//! Per interrogare Pathfinder — l'API interna che il lettore web usa davvero —
//! servono due cose, e nessuna delle due è una chiave di sviluppatore:
//!
//! 1. un **gettone di accesso** anonimo, che `open.spotify.com` rilascia a chi
//!    dimostra di essere il lettore web presentando un TOTP valido;
//! 2. un **`client-token`**, che `clienttoken.spotify.com` rilascia a chi
//!    dichiara versione del lettore, identificativo dell'applicazione e
//!    identificativo di dispositivo.
//!
//! Sono quattro richieste in fila, ognuna delle quali può fallire per conto suo.
//! Falliscono spesso: è la ragione per cui chi chiama ha sempre un livello sotto
//! a cui scendere.
//!
//! # Perché si chiede l'ora al server
//!
//! Il TOTP dipende dall'orologio. Se quello del computer è avanti o indietro di
//! più di trenta secondi — succede, sui portatili che dormono a lungo e sulle
//! macchine senza sincronizzazione — il codice generato è giusto per una
//! finestra che il server ha già chiuso, e la risposta è un rifiuto che non
//! spiega niente. Chiedere l'ora a Spotify prima di calcolare il codice toglie
//! di mezzo un'intera classe di guasti irriproducibili. Il vecchio albero lo
//! faceva già, ed è una delle poche cose di quel codice da portare intatte.
//!
//! # Cosa NON si conserva
//!
//! I gettoni stanno in memoria e basta. Non nel portachiavi, non nella tabella
//! `settings`, non in un file: durano un'ora, non identificano nessuno — sono
//! **anonimi**, è tutto il punto — e scriverli da qualche parte vorrebbe dire
//! creare un segreto da custodire dove non ce n'era uno.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aether_domain::errors::{AppError, ErrorCode};
use aether_net::http::{Corpo, Metodo, Rete, Richiesta};

use crate::config::{AGENTE, Configurazione};
use crate::totp;

/// Quanto si aspetta una risposta dai punti del lettore web.
const SCADENZA: Duration = Duration::from_secs(15);

/// Quanto prima della scadenza vera si considera scaduto un gettone.
///
/// Trenta secondi di margine: una richiesta partita con un gettone che scade fra
/// due secondi arriva scaduta, e il guasto che ne esce è un 401 che sembra un
/// problema di autorizzazione invece che di orologio.
const MARGINE_MS: u64 = 30_000;

/// I gettoni di una sessione anonima, con la loro scadenza.
#[derive(Debug, Clone)]
struct Gettoni {
    accesso: String,
    client_token: String,
    scade_a_ms: u64,
}

/// Una sessione anonima col lettore web.
///
/// Tiene i gettoni e li rinnova quando servono. Non è `Sync`: chi la usa da più
/// fili la mette dietro un lucchetto suo, che è anche il modo per non fare
/// quattro strette di mano in parallelo.
#[derive(Debug)]
pub struct Sessione {
    rete: Rete,
    configurazione: Configurazione,
    dispositivo: String,
    gettoni: Option<Gettoni>,
    /// La versione del lettore letta dalla pagina, quando ci si è riusciti.
    versione_letta: Option<String>,
}

impl Sessione {
    /// Una sessione nuova, che non ha ancora parlato con nessuno.
    #[must_use]
    pub fn nuova(configurazione: Configurazione) -> Self {
        Self {
            rete: Rete::nuova_con_agente("spotify", SCADENZA, AGENTE),
            configurazione,
            dispositivo: identificativo_dispositivo(),
            gettoni: None,
            versione_letta: None,
        }
    }

    /// Il client HTTP, condiviso con chi fa le interrogazioni.
    #[must_use]
    pub fn rete(&self) -> &Rete {
        &self.rete
    }

    /// La configurazione in uso.
    #[must_use]
    pub const fn configurazione(&self) -> &Configurazione {
        &self.configurazione
    }

    /// I gettoni buoni adesso, rinnovandoli se serve.
    ///
    /// # Errori
    ///
    /// `spotify.tokenUnavailable` se nessun cifrario mint-a un gettone, o se lo
    /// scambio del `client-token` fallisce.
    pub fn gettoni(&mut self) -> Result<(String, String), AppError> {
        if let Some(g) = &self.gettoni
            && g.scade_a_ms > adesso_ms().saturating_add(MARGINE_MS)
        {
            return Ok((g.accesso.clone(), g.client_token.clone()));
        }
        self.rinnova()
    }

    /// Butta via i gettoni: la prossima richiesta ne prende di nuovi.
    ///
    /// Si chiama su un 401, che è l'unico modo che Spotify ha di dirci che un
    /// gettone non vale più prima della sua scadenza dichiarata.
    pub fn invalida(&mut self) {
        self.gettoni = None;
    }

    /// Rifà la stretta di mano da capo.
    fn rinnova(&mut self) -> Result<(String, String), AppError> {
        // La versione del lettore si legge una volta sola: cambia a ogni
        // distribuzione di Spotify, non a ogni gettone.
        if self.versione_letta.is_none() {
            self.versione_letta = self.leggi_versione_client();
        }
        let (accesso, scade_a_ms) = self.gettone_di_accesso()?;
        let client_token = self.client_token()?;
        self.gettoni = Some(Gettoni {
            accesso: accesso.clone(),
            client_token: client_token.clone(),
            scade_a_ms,
        });
        Ok((accesso, client_token))
    }

    /// La versione del lettore, estratta dalla pagina.
    ///
    /// Restituisce `None` senza lamentarsi: c'è un valore predefinito, e una
    /// versione un po' vecchia viene accettata mentre l'assenza di versione no.
    fn leggi_versione_client(&self) -> Option<String> {
        let risposta = self
            .rete
            .esegui(Richiesta {
                metodo: Metodo::Get,
                url: "https://open.spotify.com/",
                intestazioni: &[("Accept", "text/html")],
                corpo: Corpo::Niente,
            })
            .ok()?;
        if !risposta.e_andata() {
            return None;
        }
        versione_da_pagina(&risposta.testo())
    }

    /// Il gettone anonimo, provando i cifrari in ordine.
    #[expect(
        clippy::integer_division,
        reason = "l'endpoint dei gettoni vuole un timestamp in secondi interi: il \
                  resto in millisecondi non ha posto nel parametro"
    )]
    fn gettone_di_accesso(&self) -> Result<(String, u64), AppError> {
        let server_ms = self.ora_del_server();
        let secondi = server_ms / 1000;

        for cifrario in &self.configurazione.cifrari {
            let Some(codice) = totp::genera(cifrario, server_ms) else {
                continue;
            };
            // Due punti di scambio: il primo è quello attuale, il secondo è
            // quello storico. Provarli entrambi costa una richiesta in più solo
            // quando il primo ha già fallito.
            for base in [
                "https://open.spotify.com/api/token",
                "https://open.spotify.com/get_access_token",
            ] {
                let url = format!(
                    "{base}?reason=init&productType=web-player&totp={codice}&totpServer={codice}\
                     &totpVer={}&ts={secondi}",
                    cifrario.versione
                );
                let Ok(risposta) = self.rete.esegui(Richiesta {
                    metodo: Metodo::Get,
                    url: &url,
                    intestazioni: &[
                        ("Origin", "https://open.spotify.com"),
                        ("Referer", "https://open.spotify.com/"),
                        ("App-Platform", "WebPlayer"),
                        ("Accept", "application/json"),
                    ],
                    corpo: Corpo::Niente,
                }) else {
                    continue;
                };
                if !risposta.e_andata() {
                    continue;
                }
                let Ok(corpo) = serde_json::from_slice::<serde_json::Value>(&risposta.corpo) else {
                    continue;
                };
                let Some(gettone) = corpo.get("accessToken").and_then(|v| v.as_str()) else {
                    continue;
                };
                let scade = corpo
                    .get("accessTokenExpirationTimestampMs")
                    .and_then(serde_json::Value::as_u64)
                    // Un'ora, che è la durata che Spotify dà di fatto. Serve solo
                    // quando il campo manca del tutto.
                    .unwrap_or_else(|| adesso_ms().saturating_add(3_600_000));
                return Ok((gettone.to_owned(), scade));
            }
        }

        Err(
            AppError::new(ErrorCode::SpotifyTokenUnavailable).with_message(
                "nessun cifrario TOTP ha ottenuto un gettone: probabile rotazione dei segreti",
            ),
        )
    }

    /// Lo scambio del `client-token`.
    fn client_token(&self) -> Result<String, AppError> {
        let versione = self
            .versione_letta
            .as_deref()
            .unwrap_or(&self.configurazione.versione_client);
        let corpo = serde_json::json!({
            "client_data": {
                "client_version": versione,
                "client_id": self.configurazione.client_id,
                "js_sdk_data": {
                    "device_brand": "unknown",
                    "device_model": "unknown",
                    "os": "windows",
                    "os_version": "NT 10.0",
                    "device_id": self.dispositivo,
                    "device_type": "computer"
                }
            }
        });
        let Ok(byte) = serde_json::to_vec(&corpo) else {
            return Err(AppError::new(ErrorCode::SpotifyTokenUnavailable)
                .with_message("non si è riusciti a comporre la richiesta del client-token"));
        };

        let url = "https://clienttoken.spotify.com/v1/clienttoken";
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Post,
            url,
            intestazioni: &[
                ("Accept", "application/json"),
                ("Origin", "https://open.spotify.com"),
                ("Referer", "https://open.spotify.com/"),
            ],
            corpo: Corpo::Byte {
                tipo: "application/json",
                dati: &byte,
            },
        })?;
        if !risposta.e_andata() {
            return Err(self.rete.stato_a_errore(&risposta, url));
        }
        let Ok(letto) = serde_json::from_slice::<serde_json::Value>(&risposta.corpo) else {
            return Err(AppError::new(ErrorCode::SpotifyTokenUnavailable)
                .with_message("il client-token è arrivato in una forma incomprensibile"));
        };
        letto
            .get("granted_token")
            .and_then(|g| g.get("token"))
            .and_then(|t| t.as_str())
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                AppError::new(ErrorCode::SpotifyTokenUnavailable)
                    .with_message("la risposta non conteneva un client-token")
                    .with_cause(risposta.testo())
            })
    }

    /// L'ora del server, o la nostra se non risponde.
    ///
    /// Vedi la nota in testa al modulo: è il rimedio a un orologio locale
    /// sfasato, non una precisione fine a sé stessa.
    fn ora_del_server(&self) -> u64 {
        let esito = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: "https://open.spotify.com/api/server-time",
            intestazioni: &[
                ("Origin", "https://open.spotify.com"),
                ("Accept", "application/json"),
            ],
            corpo: Corpo::Niente,
        });
        if let Ok(risposta) = esito
            && risposta.e_andata()
            && let Ok(corpo) = serde_json::from_slice::<serde_json::Value>(&risposta.corpo)
            && let Some(secondi) = corpo.get("serverTime").and_then(serde_json::Value::as_u64)
        {
            return secondi.saturating_mul(1000);
        }
        adesso_ms()
    }
}

/// L'ora locale in millisecondi dall'epoca.
fn adesso_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Un identificativo di dispositivo: trentadue esadecimali casuali.
///
/// Casuale e non derivato dalla macchina, di proposito. Un identificativo
/// stabile legherebbe fra loro tutte le importazioni fatte da questo computer e
/// le renderebbe un profilo; qui non serve a niente di più che far accettare la
/// richiesta, e un valore nuovo a ogni avvio la fa accettare uguale.
fn identificativo_dispositivo() -> String {
    let mut byte = [0_u8; 16];
    if getrandom::fill(&mut byte).is_err() {
        // Senza casualità si va avanti lo stesso: questo valore non protegge
        // niente, e un'importazione che non parte è peggio di un identificativo
        // prevedibile.
        let semente = adesso_ms().to_le_bytes();
        for (posizione, cella) in byte.iter_mut().enumerate() {
            *cella = semente.get(posizione % semente.len()).copied().unwrap_or(0);
        }
    }
    let mut fuori = String::with_capacity(32);
    for b in byte {
        fuori.push_str(&format!("{b:02x}"));
    }
    fuori
}

/// La versione del lettore, cercata nella pagina.
///
/// Sta dentro `appServerConfig`, che è un JSON codificato in base64 dentro un
/// tag `<script>`. Decodificarlo per estrarre un campo vorrebbe dire aggiungere
/// un decodificatore base64 al workspace per una stringa sola: si cerca invece
/// la forma `harmony:x.y.z-hash`, che nel testo compare comunque e che è
/// esattamente il valore da rispedire.
///
/// Funzione pura, così la si prova senza rete.
#[must_use]
pub fn versione_da_pagina(html: &str) -> Option<String> {
    const ANCORA: &str = "harmony:";
    let inizio = html.find(ANCORA)?;
    let resto = html.get(inizio..)?;
    let fine = resto
        .char_indices()
        // `>=` e non `>`: la posizione pari alla lunghezza dell'ancora è il
        // **primo** carattere della versione, e saltarlo faceva finire le
        // virgolette di chiusura dentro il valore restituito.
        .find(|(posizione, c)| {
            *posizione >= ANCORA.len() && !(c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        })
        .map_or(resto.len(), |(posizione, _)| posizione);
    let versione = resto.get(..fine)?;
    // `harmony:` e basta non è una versione: vuol dire che l'ancora è capitata
    // in un punto che non c'entra.
    (versione.len() > ANCORA.len()).then(|| versione.to_owned())
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_versione_si_trova_dentro_la_pagina() {
        let html = r#"<script>{"clientVersion":"harmony:4.42.0-2780565d","altro":1}</script>"#;
        assert_eq!(
            versione_da_pagina(html),
            Some("harmony:4.42.0-2780565d".to_owned())
        );
    }

    #[test]
    fn una_pagina_senza_versione_non_ne_inventa_una() {
        assert_eq!(versione_da_pagina("<html><body>ciao</body></html>"), None);
        assert_eq!(
            versione_da_pagina(r#"{"x":"harmony:"}"#),
            None,
            "l'ancora senza niente dietro non è una versione"
        );
    }

    #[test]
    fn la_versione_si_ferma_dove_finisce() {
        // Il tranello: senza un limite si porterebbe dentro il resto del JSON.
        let html = r#"a"harmony:4.42.0-2780565d","b":2"#;
        assert_eq!(
            versione_da_pagina(html),
            Some("harmony:4.42.0-2780565d".to_owned())
        );
    }

    #[test]
    fn ogni_dispositivo_e_diverso_e_ben_formato() {
        let uno = identificativo_dispositivo();
        let due = identificativo_dispositivo();
        assert_eq!(uno.len(), 32);
        assert!(uno.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(uno, due, "un identificativo fisso sarebbe un profilo");
    }
}
