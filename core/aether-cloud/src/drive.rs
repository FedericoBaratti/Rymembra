//! Drive REST v3, ristretto alla cartella privata dell'applicazione.
//!
//! # `appDataFolder`, e cosa comporta
//!
//! Tutto quel che questo modulo scrive finisce in `appDataFolder`: una cartella
//! che Drive tiene **per applicazione e per account**, invisibile
//! nell'interfaccia web, illeggibile da qualunque altra app, e che sparisce
//! quando l'utente rimuove i dati di Aether dal suo account. È il posto giusto
//! per una cronologia d'ascolto: sono dati personali che non hanno nessun motivo
//! di comparire fra i documenti di nessuno.
//!
//! Ha due conseguenze pratiche che si scoprono altrimenti a proprie spese:
//!
//! 1. **I file lì dentro non si cestinano.** `PATCH {trashed: true}` risponde
//!    `notSupportedForAppDataFolderFiles`. Le uniche due operazioni possibili
//!    sono rinominare e cancellare davvero — ed è il motivo per cui un file
//!    remoto corrotto si **rinomina** invece di sostituirlo: quel che è già
//!    successo una volta lo si conserva finché qualcuno non decide.
//! 2. **`parents` si dichiara solo alla creazione.** Metterlo in un
//!    aggiornamento dà un errore, e toglierlo dalla creazione fa finire il file
//!    nella radice del Drive dell'utente — dove è visibile, e dove non deve
//!    essere.
//!
//! # Multipart o ripartibile
//!
//! Il caricamento `multipart` è una richiesta sola e si ferma a **cinque
//! megabyte**: oltre, Google risponde 413. Una skin arriva a venti, quindi
//! sopra la soglia si passa al caricamento *ripartibile*, che costa una
//! richiesta in più per aprire la sessione. La soglia non è una stima nostra: è
//! il limite documentato, e [`carica`](Drive::carica) sceglie da sé.
//!
//! # Cosa questo modulo non sa
//!
//! Come si chiamano i file di Aether, cosa c'è dentro, quando vanno scritti.
//! Riceve un nome, dei byte e un'impronta. Sapere che
//! `aether-libreria.v1.json.gz` contiene i conteggi d'ascolto è compito di chi
//! sta un piano più su.

use aether_domain::errors::{AppError, ErrorCode};

use crate::http::{Corpo, Metodo, Rete, Richiesta, Risposta, percento};
use crate::oauth::base64url;

/// Il punto delle API per i metadati.
const API: &str = "https://www.googleapis.com/drive/v3/files";

/// Il punto separato per i caricamenti.
const CARICAMENTO: &str = "https://www.googleapis.com/upload/drive/v3/files";

/// I campi che si chiedono a Drive.
///
/// Espliciti e non `*`: la risposta completa di Drive per un file è una
/// quarantina di campi, quasi tutti inutili qui, e chiederli tutti significa
/// pagarli in banda a ogni passata.
const CAMPI: &str = "id,name,size,appProperties";

/// La proprietà in cui si scrive l'impronta.
///
/// Sta in `appProperties` e non in `properties`: le prime sono visibili solo a
/// questa applicazione. È un dettaglio senza conseguenze qui — siamo già in una
/// cartella privata — ma è la direzione che non allarga niente.
const IMPRONTA: &str = "impronta";

/// Oltre questa dimensione il caricamento multipart non si può usare.
const LIMITE_MULTIPART: usize = 5 * 1024 * 1024;

/// Un file nella cartella privata dell'applicazione.
///
/// Il tipo vive ora in `aether-sync`, che l'ha ereditato da qui: era già la
/// forma esatta che serve a un deposito qualunque — un identificativo, un nome,
/// quanto pesa, e l'impronta di quel che ci abbiamo messo dentro. Tenerne due
/// copie identiche voleva dire due posti in cui aggiungere un campo, e uno dei
/// due sarebbe rimasto indietro.
///
/// L'impronta è `None` per un file caricato da una versione che non la scriveva,
/// o da qualcosa che non siamo noi. Vale «non si sa», e chi legge ricarica: è la
/// direzione che non salta un caricamento.
pub use aether_sync::FileRemoto;

/// Il client di Drive, con un access token già valido.
///
/// Il token si tiene per il tempo di una passata e non si rinfresca da solo:
/// rinfrescarlo qui vorrebbe dire che questo modulo conosce le credenziali del
/// client e il portachiavi, cioè quasi tutto il resto del crate. Chi chiama
/// riconosce un token scaduto con [`e_scaduto`] e ricostruisce il client.
#[derive(Debug, Clone)]
pub struct Drive {
    rete: Rete,
    autorizzazione: String,
}

impl Drive {
    /// Un client con questo access token.
    #[must_use]
    pub fn nuovo(rete: Rete, access_token: &str) -> Self {
        Self {
            rete,
            autorizzazione: format!("Bearer {access_token}"),
        }
    }

    /// L'intestazione di autorizzazione, da mettere in ogni richiesta.
    fn chiave(&self) -> [(&str, &str); 1] {
        [("Authorization", self.autorizzazione.as_str())]
    }

    /// Tutti i file che possediamo, con le loro impronte.
    ///
    /// Una chiamata sola (più le pagine successive, che su una decina di file
    /// non arrivano mai) restituisce l'inventario completo: è ciò che permette a
    /// una passata di decidere in un colpo se c'è qualcosa da caricare, senza
    /// interrogare un file alla volta.
    ///
    /// # Errori
    ///
    /// `net.*` per la rete e per gli stati di errore.
    pub fn elenca(&self) -> Result<Vec<FileRemoto>, AppError> {
        let mut trovati = Vec::new();
        let mut pagina: Option<String> = None;
        loop {
            let url = match &pagina {
                None => format!(
                    "{API}?spaces=appDataFolder&pageSize=100&fields={}",
                    percento(&format!("nextPageToken,files({CAMPI})"))
                ),
                Some(token) => format!(
                    "{API}?spaces=appDataFolder&pageSize=100&fields={}&pageToken={}",
                    percento(&format!("nextPageToken,files({CAMPI})")),
                    percento(token)
                ),
            };
            let risposta = self.rete.ritenta(3, || {
                let risposta = self.rete.esegui(Richiesta {
                    metodo: Metodo::Get,
                    url: &url,
                    intestazioni: &self.chiave(),
                    corpo: Corpo::Niente,
                })?;
                self.buona(risposta, &url)
            })?;

            let corpo = interpreta(&risposta, &url)?;
            if let Some(file) = corpo.get("files").and_then(serde_json::Value::as_array) {
                trovati.extend(file.iter().filter_map(file_remoto));
            }
            pagina = corpo
                .get("nextPageToken")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            if pagina.is_none() {
                return Ok(trovati);
            }
        }
    }

    /// Scarica il contenuto di un file.
    ///
    /// # Errori
    ///
    /// `net.*`; in particolare `net.http` con stato 404 se il file non c'è più —
    /// che capita davvero, quando l'utente ha rimosso i dati dell'app da Drive
    /// mentre Aether era chiuso.
    pub fn scarica(&self, id: &str) -> Result<Vec<u8>, AppError> {
        let url = format!("{API}/{}?alt=media", percento(id));
        let risposta = self.rete.ritenta(3, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Get,
                url: &url,
                intestazioni: &self.chiave(),
                corpo: Corpo::Niente,
            })?;
            self.buona(risposta, &url)
        })?;
        Ok(risposta.corpo)
    }

    /// Carica dei byte, creando il file o sostituendo quello che c'era.
    ///
    /// `esistente` è l'identificativo del file da sostituire, quando lo si
    /// conosce già dall'elenco. Passarlo evita di accumulare venti copie dello
    /// stesso backup: Drive **permette** due file con lo stesso nome nella
    /// stessa cartella, e senza questo parametro è esattamente quel che
    /// succederebbe.
    ///
    /// # Errori
    ///
    /// `net.*`.
    pub fn carica(
        &self,
        nome: &str,
        esistente: Option<&str>,
        tipo: &str,
        dati: &[u8],
        impronta: &str,
    ) -> Result<FileRemoto, AppError> {
        let metadati = metadati(nome, impronta, esistente.is_none());
        if dati.len() <= LIMITE_MULTIPART {
            self.multipart(esistente, &metadati, tipo, dati)
        } else {
            self.ripartibile(esistente, &metadati, tipo, dati)
        }
    }

    /// Il caricamento in una richiesta sola.
    fn multipart(
        &self,
        esistente: Option<&str>,
        metadati: &str,
        tipo: &str,
        dati: &[u8],
    ) -> Result<FileRemoto, AppError> {
        let confine = confine()?;
        let corpo = corpo_multipart(&confine, metadati, tipo, dati);
        let contenuto = format!("multipart/related; boundary={confine}");
        let url = match esistente {
            Some(id) => format!(
                "{CARICAMENTO}/{}?uploadType=multipart&fields={}",
                percento(id),
                percento(CAMPI)
            ),
            None => format!(
                "{CARICAMENTO}?uploadType=multipart&fields={}",
                percento(CAMPI)
            ),
        };

        let risposta = self.rete.ritenta(3, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: if esistente.is_some() {
                    Metodo::Patch
                } else {
                    Metodo::Post
                },
                url: &url,
                intestazioni: &self.chiave(),
                corpo: Corpo::Byte {
                    tipo: &contenuto,
                    dati: &corpo,
                },
            })?;
            self.buona(risposta, &url)
        })?;
        file_remoto(&interpreta(&risposta, &url)?).ok_or_else(|| forma_inattesa(&url))
    }

    /// Il caricamento in due tempi, per i file grossi.
    ///
    /// Prima si apre una sessione — Drive risponde con un indirizzo
    /// nell'intestazione `Location` — poi ci si mandano i byte con un `PUT`.
    /// I byte partono in un colpo solo e non a pezzi: la ripartenza a metà
    /// servirebbe su una connessione che cade durante un caricamento di ore,
    /// mentre qui il caso peggiore è una skin da venti megabyte, che si rifà da
    /// capo in meno tempo di quanto ne costerebbe scrivere la logica di ripresa.
    fn ripartibile(
        &self,
        esistente: Option<&str>,
        metadati: &str,
        tipo: &str,
        dati: &[u8],
    ) -> Result<FileRemoto, AppError> {
        let url = match esistente {
            Some(id) => format!(
                "{CARICAMENTO}/{}?uploadType=resumable&fields={}",
                percento(id),
                percento(CAMPI)
            ),
            None => format!(
                "{CARICAMENTO}?uploadType=resumable&fields={}",
                percento(CAMPI)
            ),
        };
        let lunghezza = dati.len().to_string();
        let apertura = self.rete.ritenta(3, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: if esistente.is_some() {
                    Metodo::Patch
                } else {
                    Metodo::Post
                },
                url: &url,
                intestazioni: &[
                    ("Authorization", self.autorizzazione.as_str()),
                    ("X-Upload-Content-Type", tipo),
                    ("X-Upload-Content-Length", lunghezza.as_str()),
                ],
                corpo: Corpo::Byte {
                    tipo: "application/json; charset=UTF-8",
                    dati: metadati.as_bytes(),
                },
            })?;
            self.buona(risposta, &url)
        })?;

        let Some(sessione) = apertura.posizione else {
            return Err(AppError::new(ErrorCode::NetBadSchema {
                service: Some("drive".to_owned()),
                detail: Some("la sessione di caricamento non ha un indirizzo".to_owned()),
            }));
        };

        // La sessione **non** porta l'intestazione di autorizzazione: l'indirizzo
        // è già un segreto a tempo, e Drive rifiuta la richiesta se ce la si
        // mette. È il genere di dettaglio che fa perdere un pomeriggio.
        let risposta = self.rete.ritenta(3, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Put,
                url: &sessione,
                intestazioni: &[],
                corpo: Corpo::Byte { tipo, dati },
            })?;
            self.buona(risposta, &sessione)
        })?;
        file_remoto(&interpreta(&risposta, &sessione)?).ok_or_else(|| forma_inattesa(&sessione))
    }

    /// Rinomina un file.
    ///
    /// È quel che si fa a un file remoto illeggibile, al posto di sovrascriverlo:
    /// nella cartella privata dell'app il cestino non esiste (vedi la nota in
    /// testa al modulo), e una cancellazione vera toglierebbe a chi indaga
    /// l'unica copia di ciò che è andato storto.
    ///
    /// # Errori
    ///
    /// `net.*`.
    pub fn rinomina(&self, id: &str, nome: &str) -> Result<(), AppError> {
        let url = format!("{API}/{}?fields={}", percento(id), percento(CAMPI));
        let corpo = serde_json::json!({ "name": nome }).to_string();
        self.rete.ritenta(3, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Patch,
                url: &url,
                intestazioni: &self.chiave(),
                corpo: Corpo::Byte {
                    tipo: "application/json; charset=UTF-8",
                    dati: corpo.as_bytes(),
                },
            })?;
            self.buona(risposta, &url)
        })?;
        Ok(())
    }

    /// Cancella un file, davvero.
    ///
    /// Un 404 vale come riuscita: cancellare qualcosa che non c'è più è già la
    /// situazione voluta, e trattarlo come guasto farebbe fallire una pulizia
    /// ripetuta.
    ///
    /// # Errori
    ///
    /// `net.*`.
    pub fn cancella(&self, id: &str) -> Result<(), AppError> {
        let url = format!("{API}/{}", percento(id));
        self.rete.ritenta(3, || {
            let risposta = self.rete.esegui(Richiesta {
                metodo: Metodo::Delete,
                url: &url,
                intestazioni: &self.chiave(),
                corpo: Corpo::Niente,
            })?;
            if risposta.stato == 404 {
                return Ok(risposta);
            }
            self.buona(risposta, &url)
        })?;
        Ok(())
    }

    /// Lascia passare una risposta riuscita, traduce le altre.
    fn buona(&self, risposta: Risposta, url: &str) -> Result<Risposta, AppError> {
        if risposta.e_andata() {
            Ok(risposta)
        } else {
            Err(self.rete.stato_a_errore(&risposta, url))
        }
    }
}

/// Drive come deposito qualunque per la sincronia.
///
/// Non c'è nessun adattamento: le cinque operazioni che `aether-sync` chiede
/// sono, una per una, i cinque metodi che questo modulo aveva già. Non è una
/// coincidenza — il tratto è stato ricavato da qui, dopo aver constatato che la
/// forma che il backup usava da mesi era già quella giusta per un deposito
/// qualunque.
///
/// Quel che il tratto aggiunge davvero è la possibilità di sostituire Drive con
/// una cartella condivisa senza che il motore della sincronia se ne accorga. È
/// la ragione per cui la fusione si prova senza rete e senza account.
impl aether_sync::Magazzino for Drive {
    fn elenca(&self) -> Result<Vec<FileRemoto>, AppError> {
        Self::elenca(self)
    }

    fn leggi(&self, id: &str) -> Result<Vec<u8>, AppError> {
        self.scarica(id)
    }

    fn scrivi(
        &self,
        nome: &str,
        esistente: Option<&str>,
        tipo: &str,
        dati: &[u8],
        impronta: &str,
    ) -> Result<FileRemoto, AppError> {
        self.carica(nome, esistente, tipo, dati, impronta)
    }

    fn rinomina(&self, id: &str, nome: &str) -> Result<(), AppError> {
        Self::rinomina(self, id, nome)
    }

    fn cancella(&self, id: &str) -> Result<(), AppError> {
        Self::cancella(self, id)
    }
}

/// L'errore dice che il token non vale più?
///
/// Un 401 a metà passata non è un guasto da riportare all'utente: è il momento
/// in cui si rinfresca l'access token e si rifà la passata, **una volta sola**.
/// Al secondo 401 è davvero un problema di autorizzazione.
#[must_use]
pub fn e_scaduto(err: &AppError) -> bool {
    matches!(err.code(), ErrorCode::NetHttp { status: 401, .. })
}

/// I metadati JSON di un file.
///
/// `parents` **solo** alla creazione: in un aggiornamento Drive lo rifiuta, e
/// senza di lui alla creazione il file finirebbe nella radice del Drive
/// dell'utente, dove è visibile e non deve essere.
fn metadati(nome: &str, impronta: &str, e_nuovo: bool) -> String {
    let mut campi = serde_json::Map::new();
    campi.insert("name".to_owned(), serde_json::Value::from(nome));
    campi.insert(
        "appProperties".to_owned(),
        serde_json::json!({ IMPRONTA: impronta }),
    );
    if e_nuovo {
        campi.insert(
            "parents".to_owned(),
            serde_json::Value::from(vec!["appDataFolder"]),
        );
    }
    serde_json::Value::Object(campi).to_string()
}

/// Un file, letto dalla risposta di Drive.
///
/// `size` arriva come **stringa**: è la convenzione di Drive per i numeri a
/// sessantaquattro bit, che in JavaScript non ci starebbero. Leggerlo come
/// numero dà sempre zero, e uno zero qui significherebbe «ricarica tutto a ogni
/// passata».
fn file_remoto(valore: &serde_json::Value) -> Option<FileRemoto> {
    Some(FileRemoto {
        id: valore.get("id")?.as_str()?.to_owned(),
        nome: valore
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        byte: valore
            .get("size")
            .and_then(|size| {
                size.as_str()
                    .and_then(|testo| testo.parse().ok())
                    .or_else(|| size.as_u64())
            })
            .unwrap_or(0),
        impronta: valore
            .pointer(&format!("/appProperties/{IMPRONTA}"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    })
}

/// Il JSON di una risposta.
fn interpreta(risposta: &Risposta, url: &str) -> Result<serde_json::Value, AppError> {
    serde_json::from_slice(&risposta.corpo)
        .map_err(|err| forma_inattesa(url).with_cause(err.to_string()))
}

/// La risposta non aveva la forma che ci aspettavamo.
fn forma_inattesa(url: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some("drive".to_owned()),
        detail: Some(format!("risposta inattesa da {url}")),
    })
}

/// Un separatore di parti che non può comparire nel contenuto.
///
/// Casuale e non fisso. Un separatore costante andrebbe bene per un JSON
/// compresso — la probabilità che quei byte capitino dentro è remota — ma
/// «remota» su un archivio di skin che qualcuno può costruire a mano non è una
/// garanzia, e un separatore che compare nel contenuto tronca il caricamento a
/// metà senza che nessuno se ne accorga.
///
/// # Errori
///
/// `internal.unexpected` se il sistema non fornisce casualità.
fn confine() -> Result<String, AppError> {
    let mut byte = [0u8; 16];
    getrandom::fill(&mut byte).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("il sistema non fornisce casualità".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    Ok(format!("aether-{}", base64url(&byte)))
}

/// Compone il corpo `multipart/related` di un caricamento.
///
/// Separata dal resto perché è l'unico pezzo di Drive che si può provare byte
/// per byte senza rete, ed è anche quello dove un `\r\n` di troppo o di meno
/// produce un errore che Google descrive come «Invalid multipart request» e
/// basta.
fn corpo_multipart(confine: &str, metadati: &str, tipo: &str, dati: &[u8]) -> Vec<u8> {
    let mut corpo = Vec::with_capacity(dati.len() + metadati.len() + 256);
    corpo.extend_from_slice(
        format!(
            "--{confine}\r\n\
             Content-Type: application/json; charset=UTF-8\r\n\
             \r\n\
             {metadati}\r\n\
             --{confine}\r\n\
             Content-Type: {tipo}\r\n\
             \r\n"
        )
        .as_bytes(),
    );
    corpo.extend_from_slice(dati);
    corpo.extend_from_slice(format!("\r\n--{confine}--\r\n").as_bytes());
    corpo
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_corpo_multipart_e_esattamente_questo() {
        // Byte per byte contro una forma scritta a mano: Google descrive
        // qualunque sbaglio qui come «Invalid multipart request», che non dice
        // se manca un ritorno a capo, se il separatore è sbagliato o se il
        // contenuto è finito nella parte dei metadati.
        // Contenuto ASCII solo perché l'atteso possa stare in un letterale: che
        // i byte veri passino intatti lo prova la funzione qui sotto.
        let corpo = corpo_multipart(
            "CONFINE",
            r#"{"name":"x"}"#,
            "application/gzip",
            b"CONTENUTO",
        );
        let atteso = concat!(
            "--CONFINE\r\n",
            "Content-Type: application/json; charset=UTF-8\r\n",
            "\r\n",
            "{\"name\":\"x\"}\r\n",
            "--CONFINE\r\n",
            "Content-Type: application/gzip\r\n",
            "\r\n",
            "CONTENUTO\r\n",
            "--CONFINE--\r\n",
        );
        assert_eq!(String::from_utf8_lossy(&corpo), atteso);
    }

    #[test]
    fn il_corpo_multipart_regge_i_byte_che_non_sono_testo() {
        // Il contenuto vero è gzip: se qualcuno rifacesse questa funzione con
        // una `String` invece di un `Vec<u8>`, ogni byte non valido in UTF-8
        // diventerebbe un punto interrogativo e il file arriverebbe rotto.
        let dati: Vec<u8> = (0..=255u8).collect();
        let corpo = corpo_multipart("C", "{}", "application/octet-stream", &dati);
        assert!(
            corpo.windows(dati.len()).any(|finestra| finestra == dati),
            "i byte devono attraversare intatti"
        );
    }

    #[test]
    fn i_metadati_dichiarano_il_genitore_solo_alla_creazione() {
        let nuovo: serde_json::Value =
            serde_json::from_str(&metadati("f.gz", "abc", true)).expect("json");
        assert_eq!(
            nuovo.pointer("/parents/0").and_then(|v| v.as_str()),
            Some("appDataFolder")
        );
        assert_eq!(
            nuovo
                .pointer("/appProperties/impronta")
                .and_then(|v| v.as_str()),
            Some("abc")
        );

        // In un aggiornamento `parents` è un errore di Drive, non un dettaglio.
        let aggiornato: serde_json::Value =
            serde_json::from_str(&metadati("f.gz", "abc", false)).expect("json");
        assert!(aggiornato.get("parents").is_none());
    }

    #[test]
    fn la_dimensione_si_legge_anche_quando_e_una_stringa() {
        // È la convenzione di Drive per i numeri a 64 bit. Letta come numero
        // darebbe zero, e uno zero qui vorrebbe dire ricaricare tutto a ogni
        // passata.
        let valore = serde_json::json!({
            "id": "1a2b",
            "name": "aether-libreria.v1.json.gz",
            "size": "71234",
            "appProperties": {"impronta": "deadbeef"}
        });
        assert_eq!(
            file_remoto(&valore),
            Some(FileRemoto {
                id: "1a2b".to_owned(),
                nome: "aether-libreria.v1.json.gz".to_owned(),
                byte: 71234,
                impronta: Some("deadbeef".to_owned()),
            })
        );
    }

    #[test]
    fn un_file_senza_impronta_vale_non_si_sa() {
        // Caricato da una versione precedente, o da qualcosa che non siamo noi.
        // `None` fa ricaricare, che è la direzione che non salta un backup.
        let valore = serde_json::json!({"id": "x", "name": "vecchio.gz"});
        let file = file_remoto(&valore).expect("un file");
        assert_eq!(file.impronta, None);
        assert_eq!(file.byte, 0);
    }

    #[test]
    fn una_risposta_senza_id_non_e_un_file() {
        assert_eq!(file_remoto(&serde_json::json!({"name": "senza id"})), None);
        assert_eq!(file_remoto(&serde_json::json!([])), None);
    }

    #[test]
    fn l_elenco_vuoto_di_un_account_nuovo_si_legge() {
        // La prima passata di chi ha appena collegato l'account: `files: []`.
        // Un lettore che si aspettasse almeno un elemento fallirebbe proprio lì.
        let corpo = serde_json::json!({"files": []});
        let file = corpo
            .get("files")
            .and_then(|v| v.as_array())
            .expect("array");
        assert!(file.iter().filter_map(file_remoto).next().is_none());
    }

    #[test]
    fn un_token_scaduto_si_riconosce_e_non_si_confonde() {
        assert!(e_scaduto(&AppError::new(ErrorCode::NetHttp {
            status: 401,
            url: None
        })));
        // Un 403 è «non hai il permesso», e rinfrescare il token non lo cambia.
        assert!(!e_scaduto(&AppError::new(ErrorCode::NetHttp {
            status: 403,
            url: None
        })));
        assert!(!e_scaduto(&AppError::new(ErrorCode::SyncAuthExpired)));
    }

    #[test]
    fn due_separatori_di_fila_non_sono_uguali() {
        // Un separatore fisso troncherebbe in silenzio un caricamento il cui
        // contenuto lo contenga.
        let uno = confine().expect("casualità");
        let due = confine().expect("casualità");
        assert_ne!(uno, due);
        assert!(uno.starts_with("aether-"));
        assert!(
            uno.bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
            "un separatore con caratteri strani rompe l'intestazione"
        );
    }
}
