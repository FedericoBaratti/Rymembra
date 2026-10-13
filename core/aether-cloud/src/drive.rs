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
//! # I tempi, fase per fase
//!
//! Fino alla 2.4.0 ogni richiesta aveva la stessa scadenza complessiva — due
//! minuti — qualunque cosa portasse: l'elenco da due kilobyte e la skin da
//! venti megabyte in un `PUT` solo. Su una linea da un megabit la skin non ci
//! stava, il caricamento moriva a metà e ripartiva da zero al giro dopo, per
//! sempre. Adesso i tempi sono della fase, non del file:
//!
//! - le **domande** (elenco, metadati, apertura della sessione, rinomina) hanno
//!   la scadenza della rete che si riceve;
//! - un caricamento ripartibile va a **pezzi** ([`PEZZO`]), ognuno con la sua
//!   scadenza: due minuti per otto megabyte vogliono mezzo megabit al secondo,
//!   non un megabit e mezzo;
//! - lo **scarico** di un file passa da una rete senza scadenza complessiva
//!   ([`Rete::per_prelievo`]), che limita il silenzio e non la durata.
//!
//! Un pezzo che cade non fa ripartire il file: si chiede a Drive fin dove è
//! arrivato, si aspetta una pausa che cresce a ogni guasto di fila, e si
//! riprende da lì. Vedi [`Drive::carica`].
//!
//! # Quanto pesa quel che torna
//!
//! Il tetto di [`Rete::esegui`] era quello di `ureq`, dieci megabyte, e un
//! ripristino con una skin da quindici falliva per intero. Drive dichiara il
//! suo: [`TETTO_DRIVE`].
//!
//! # Cosa questo modulo non sa
//!
//! Come si chiamano i file di Aether, cosa c'è dentro, quando vanno scritti.
//! Riceve un nome, dei byte e un'impronta. Sapere che
//! `aether-libreria.v1.json.gz` contiene i conteggi d'ascolto è compito di chi
//! sta un piano più su.

use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};

use crate::http::{Cancello, Corpo, Metodo, Rete, Richiesta, Rimandi, Risposta, percento};
use crate::oauth::base64url;

/// Il punto delle API per i metadati.
const API: &str = "https://www.googleapis.com/drive/v3/files";

/// Il punto separato per i caricamenti.
const CARICAMENTO: &str = "https://www.googleapis.com/upload/drive/v3/files";

/// Quanto può pesare un file che si riporta giù da Drive.
///
/// Venticinque mebibyte: il pacchetto di skin più grande che lo Studio accetta
/// ne pesa venti, e il margine copre un pacchetto scritto da una versione che
/// ne accettasse un po' di più. Il tetto di serie di [`Rete`] — dieci, lo
/// stesso di `ureq` — faceva fallire **l'intero** ripristino alla prima skin
/// oltre quella soglia.
pub const TETTO_DRIVE: u64 = 25 * 1024 * 1024;

/// Quanto è grande un pezzo di un caricamento ripartibile.
///
/// Otto mebibyte: un multiplo di 256 KiB, come Google chiede per ogni pezzo
/// tranne l'ultimo, e abbastanza piccolo perché la scadenza di una richiesta
/// basti anche su una linea lenta. Una skin da venti megabyte sono tre pezzi.
pub const PEZZO: usize = 8 * 1024 * 1024;

/// Quanti guasti di fila si accettano sullo stesso caricamento a pezzi.
///
/// Guasti **di fila**: un pezzo che arriva azzera il conto. Quattro, con la
/// pausa che raddoppia da un secondo, sono quindici secondi di attesa per un
/// file solo prima di arrendersi — e arrendersi vuol dire solo rimandare alla
/// passata dopo, che riprenderà da capo.
const TENTATIVI_PEZZO: u32 = 4;

/// La prima pausa dopo un pezzo caduto. Le successive raddoppiano.
const PAUSA_PEZZO: Duration = Duration::from_secs(1);

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

/// Quanti rimandi segue Drive, e solo verso Google.
///
/// # Perché non zero
///
/// Perché uno Drive lo fa davvero, e sulla strada che conta di più: lo scarico
/// di un file con `files.get?alt=media` — cioè il ripristino — può rispondere
/// `307` verso `…-apidata.googleusercontent.com/download/drive/v3/files/…`,
/// l'indirizzo da cui Google serve i byte (è documentato nelle segnalazioni dei
/// client ufficiali, per esempio `google-api-javascript-client` #600). Con zero
/// rimandi quel ripristino fallirebbe il giorno in cui Google decidesse di
/// servirlo da lì.
///
/// Il resto di Drive non ne fa: l'apertura di un caricamento ripartibile
/// risponde `200` con l'indirizzo di sessione in `Location` — un'intestazione,
/// non un rimando — e il `PUT` dei byte risponde `200`, o `308 Resume
/// Incomplete` che non è un rimando (non ha `Location`) e arriva come stato.
/// Metadati, elenco, rinomina e cancellazione rispondono sul posto.
///
/// # Perché due, e perché sorvegliati
///
/// Due: il salto verso chi serve i byte, e uno di margine. Sorvegliati perché la
/// richiesta porta un token d'accesso: `Authorization` non attraversa comunque
/// un rimando (vedi `aether_net::http::Rete`), ma un servizio che rimandasse
/// fuori da Google porterebbe fuori da Google la domanda — e il corpo che
/// torna finirebbe nel ripristino. Solo i `GET` seguono rimandi: un
/// caricamento che ricevesse un 3xx lo riceve come errore, perché rimandare
/// altrove i byte di un backup è proprio la cosa da non fare.
const RIMANDI_DRIVE: u32 = 2;

/// Se un rimando di Drive porta a casa di Google.
///
/// `https://` e un ospite che è `googleapis.com` o `googleusercontent.com`, o
/// un loro sottodominio. Il confronto è sul suffisso **con il punto**:
/// `evilgoogleapis.com` finisce per `googleapis.com` ma non per
/// `.googleapis.com`, e l'ospite si legge dopo l'ultima chiocciola
/// (`aether_net::http::ospite`), così `googleapis.com@esempio.com` è
/// `esempio.com`.
fn verso_google(bersaglio: &str) -> bool {
    if !bersaglio.starts_with("https://") {
        return false;
    }
    let Some(ospite) = crate::http::ospite(bersaglio) else {
        return false;
    };
    let ospite = ospite.trim_end_matches('.').to_ascii_lowercase();
    ["googleapis.com", "googleusercontent.com"]
        .iter()
        .any(|casa| ospite == *casa || ospite.ends_with(&format!(".{casa}")))
}

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
///
/// [`Debug`] scritto a mano: dentro c'è l'intestazione con il token.
#[derive(Clone)]
pub struct Drive {
    /// Le domande e i pezzi dei caricamenti: una scadenza per richiesta.
    rete: Rete,
    /// Gli scarichi: nessuna scadenza complessiva, vedi il `//!`.
    trasferimenti: Rete,
    autorizzazione: String,
    /// Il punto dei metadati. Una costante, tranne che nelle prove.
    api: String,
    /// Il punto dei caricamenti. Una costante, tranne che nelle prove.
    caricamento: String,
    /// Quanto è grande un pezzo: [`PEZZO`], tranne che nelle prove.
    pezzo: usize,
    /// Oltre quanto si passa al ripartibile: [`LIMITE_MULTIPART`], tranne che
    /// nelle prove.
    limite_multipart: usize,
    /// La prima pausa dopo un pezzo caduto: [`PAUSA_PEZZO`], tranne che nelle
    /// prove.
    pausa: Duration,
}

impl Drive {
    /// Un client puntato su un Drive finto di questa macchina, per le prove:
    /// pezzi da otto byte e sempre ripartibile, così un caricamento di venti
    /// byte passa da tutte le fasi.
    #[cfg(test)]
    fn verso_prova(rete: Rete, access_token: &str, base: &str) -> Self {
        Self {
            rete: rete.clone().con_tetto_corpo(TETTO_DRIVE),
            trasferimenti: rete.con_tetto_corpo(TETTO_DRIVE),
            autorizzazione: format!("Bearer {access_token}"),
            api: format!("{base}/drive/v3/files"),
            caricamento: format!("{base}/upload/drive/v3/files"),
            pezzo: 8,
            limite_multipart: 0,
            pausa: Duration::from_millis(10),
        }
    }
}

impl std::fmt::Debug for Drive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drive")
            .field("api", &self.api)
            .field("autorizzazione", &"Bearer …")
            .finish_non_exhaustive()
    }
}

impl Drive {
    /// Un client con questo access token.
    ///
    /// La politica dei rimandi la decide Drive, non chi gli passa la rete: vedi
    /// [`RIMANDI_DRIVE`]. La rete che si riceve resta quella — scadenze e
    /// riserva di connessioni comprese — con i rimandi rimessi a quelli di
    /// Drive e il tetto del corpo alzato a [`TETTO_DRIVE`], così nessuno dei
    /// posti che costruiscono un `Drive` può dimenticarsene. Gli scarichi
    /// passano da una seconda rete, senza scadenza complessiva: vedi il `//!`.
    #[must_use]
    pub fn nuovo(rete: Rete, access_token: &str) -> Self {
        let rimandi = || Rimandi::Sorvegliati {
            quanti: RIMANDI_DRIVE,
            cancello: Cancello::nuovo(verso_google),
        };
        Self {
            rete: rete.con_rimandi(rimandi()).con_tetto_corpo(TETTO_DRIVE),
            trasferimenti: Rete::per_prelievo("drive")
                .con_rimandi(rimandi())
                .con_tetto_corpo(TETTO_DRIVE),
            autorizzazione: format!("Bearer {access_token}"),
            api: API.to_owned(),
            caricamento: CARICAMENTO.to_owned(),
            pezzo: PEZZO,
            limite_multipart: LIMITE_MULTIPART,
            pausa: PAUSA_PEZZO,
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
            let api = &self.api;
            let url = match &pagina {
                None => format!(
                    "{api}?spaces=appDataFolder&pageSize=100&fields={}",
                    percento(&format!("nextPageToken,files({CAMPI})"))
                ),
                Some(token) => format!(
                    "{api}?spaces=appDataFolder&pageSize=100&fields={}&pageToken={}",
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
    /// mentre Aether era chiuso. `net.badSchema` per un file oltre
    /// [`TETTO_DRIVE`].
    pub fn scarica(&self, id: &str) -> Result<Vec<u8>, AppError> {
        let url = format!("{}/{}?alt=media", self.api, percento(id));
        let risposta = self.trasferimenti.ritenta(3, || {
            let risposta = self.trasferimenti.esegui(Richiesta {
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
        // Il limite di Google è sul CORPO della richiesta, non sul payload:
        // separatori, intestazioni di parte e metadati JSON si aggiungono ai
        // dati, e un payload esattamente al limite sforerebbe con un 413 senza
        // mai provare la strada ripartibile. Il margine copre tutto quel
        // contorno con spazio d'avanzo.
        let contorno = metadati.len().saturating_add(1024);
        if dati.len().saturating_add(contorno) <= self.limite_multipart {
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
        let caricamento = &self.caricamento;
        let url = match esistente {
            Some(id) => format!(
                "{caricamento}/{}?uploadType=multipart&fields={}",
                percento(id),
                percento(CAMPI)
            ),
            None => format!(
                "{caricamento}?uploadType=multipart&fields={}",
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
    /// nell'intestazione `Location` — poi ci si mandano i byte con dei `PUT`,
    /// un [`PEZZO`] per volta.
    ///
    /// # Perché a pezzi, adesso
    ///
    /// Fino alla 2.4.0 i byte partivano in un colpo solo, e il commento che
    /// stava qui diceva che una skin da venti megabyte si rifà da capo in meno
    /// tempo di quanto costi la logica di ripresa. Era vero su una linea buona.
    /// Su una da un megabit quel `PUT` non stava nei due minuti della scadenza,
    /// moriva ogni volta a metà, e la passata dopo lo rifaceva da zero: un
    /// backup che non finiva mai, e banda spesa a vuoto ogni quarto d'ora.
    ///
    /// # Come si riprende
    ///
    /// Drive risponde a ogni pezzo intermedio `308` con `Range: bytes=0-N`,
    /// cioè quel che ha davvero ricevuto, e da lì si continua. Un pezzo che
    /// cade per un guasto ritentabile non fa ripartire il file: si aspetta una
    /// pausa che raddoppia ([`PAUSA_PEZZO`]), si chiede a Drive a che punto è
    /// (`Content-Range: bytes */totale`) e si riprende dal byte che dice lui.
    /// Dopo [`TENTATIVI_PEZZO`] guasti di fila si rinuncia a **questo** file,
    /// e l'errore torna a chi chiama.
    fn ripartibile(
        &self,
        esistente: Option<&str>,
        metadati: &str,
        tipo: &str,
        dati: &[u8],
    ) -> Result<FileRemoto, AppError> {
        let caricamento = &self.caricamento;
        let url = match esistente {
            Some(id) => format!(
                "{caricamento}/{}?uploadType=resumable&fields={}",
                percento(id),
                percento(CAMPI)
            ),
            None => format!(
                "{caricamento}?uploadType=resumable&fields={}",
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
        let totale = dati.len();
        let mut inviati = 0_usize;
        let mut guasti = 0_u32;
        loop {
            let esito = self.manda_pezzo(&sessione, tipo, dati, inviati);
            let guasto = match esito {
                Ok(Passo::Finito(file)) => return Ok(file),
                Ok(Passo::Ricevuti(fin_qui)) if fin_qui > inviati => {
                    inviati = fin_qui.min(totale);
                    guasti = 0;
                    continue;
                }
                // Un `308` che non va avanti: Drive dice di non aver preso
                // niente del pezzo. Vale un guasto, o un servizio che risponde
                // sempre così terrebbe questo filo in un giro senza fine.
                Ok(Passo::Ricevuti(fin_qui)) => {
                    inviati = fin_qui.min(totale);
                    AppError::new(ErrorCode::NetBadSchema {
                        service: Some("drive".to_owned()),
                        detail: Some("il caricamento non va avanti".to_owned()),
                    })
                }
                Err(err) if err.is_retryable() => err,
                Err(err) => return Err(err),
            };
            guasti = guasti.saturating_add(1);
            if guasti >= TENTATIVI_PEZZO {
                return Err(guasto.with_cause(format!(
                    "caricamento ripartibile fermo a {inviati} byte su {totale} dopo {guasti} guasti di fila"
                )));
            }
            std::thread::sleep(self.pausa_dopo(guasti, &guasto));
            // Dove è arrivato davvero: il pezzo caduto può essere arrivato
            // tutto, in parte o per niente, e solo Drive lo sa.
            match self.a_che_punto(&sessione, totale) {
                Ok(Passo::Finito(file)) => return Ok(file),
                Ok(Passo::Ricevuti(fin_qui)) => inviati = fin_qui.min(totale),
                // Anche la domanda è caduta: si riprova il pezzo da dove si
                // pensava di essere, e il prossimo `308` rimetterà a posto.
                Err(err) if err.is_retryable() => {}
                Err(err) => return Err(err),
            }
        }
    }

    /// Un pezzo di un caricamento ripartibile, da `da` in poi.
    fn manda_pezzo(
        &self,
        sessione: &str,
        tipo: &str,
        dati: &[u8],
        da: usize,
    ) -> Result<Passo, AppError> {
        let totale = dati.len();
        let fino = da.saturating_add(self.pezzo.max(1)).min(totale);
        let pezzo = dati.get(da..fino).unwrap_or(&[]);
        let intervallo = if totale == 0 {
            "bytes */0".to_owned()
        } else {
            format!("bytes {da}-{}/{totale}", fino.saturating_sub(1))
        };
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Put,
            url: sessione,
            intestazioni: &[("Content-Range", intervallo.as_str())],
            corpo: Corpo::Byte { tipo, dati: pezzo },
        })?;
        self.passo_da(risposta, sessione)
    }

    /// Chiede a Drive fin dove è arrivato un caricamento ripartibile.
    ///
    /// È la forma documentata: un `PUT` vuoto con `Content-Range: bytes
    /// */totale`. Risponde come a un pezzo — `308` con quel che ha, o `200` se
    /// aveva già tutto.
    fn a_che_punto(&self, sessione: &str, totale: usize) -> Result<Passo, AppError> {
        let intervallo = format!("bytes */{totale}");
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Put,
            url: sessione,
            intestazioni: &[("Content-Range", intervallo.as_str())],
            corpo: Corpo::Niente,
        })?;
        self.passo_da(risposta, sessione)
    }

    /// Cosa vuol dire la risposta a un pezzo.
    ///
    /// `308` è «continua», e **non è un rimando**: non ha `Location`, e i
    /// rimandi di Drive si seguono solo sui `GET`. `404` e `410` vogliono dire
    /// che la sessione non c'è più — scade dopo una settimana, o Google l'ha
    /// chiusa — e allora il file va ricominciato, dalla passata dopo.
    fn passo_da(&self, risposta: Risposta, sessione: &str) -> Result<Passo, AppError> {
        match risposta.stato {
            308 => Ok(Passo::Ricevuti(ricevuti_fin_qui(
                risposta.intervallo.as_deref(),
            ))),
            200 | 201 => file_remoto(&interpreta(&risposta, sessione)?)
                .map(Passo::Finito)
                .ok_or_else(|| forma_inattesa(sessione)),
            _ => Err(self.guasto(&risposta, sessione)),
        }
    }

    /// Quanto aspettare dopo `guasti` pezzi caduti di fila.
    ///
    /// Il tempo che Drive ha chiesto, se l'ha chiesto; altrimenti la pausa che
    /// raddoppia. Mai più di mezzo minuto: oltre, la passata dopo arriva comunque.
    fn pausa_dopo(&self, guasti: u32, guasto: &AppError) -> Duration {
        let chiesto = match guasto.code() {
            ErrorCode::NetRateLimited {
                retry_after_ms: Some(ms),
                ..
            } => Some(Duration::from_millis(*ms)),
            _ => None,
        };
        let raddoppi = guasti.saturating_sub(1).min(8);
        chiesto
            .unwrap_or_else(|| self.pausa.saturating_mul(1_u32 << raddoppi))
            .min(Duration::from_secs(30))
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
        let url = format!("{}/{}?fields={}", self.api, percento(id), percento(CAMPI));
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
        let url = format!("{}/{}", self.api, percento(id));
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
            Err(self.guasto(&risposta, url))
        }
    }

    /// Il codice di una risposta di errore di Drive.
    ///
    /// # I `403` che non sono «non hai il permesso»
    ///
    /// Drive risponde `403` a tre cose diverse, e la differenza sta nel campo
    /// `reason` del corpo:
    ///
    /// - `storageQuotaExceeded`: lo spazio del Drive è pieno. Non passa da sé e
    ///   non è un guasto di Aether: ha un codice suo, `sync.quotaExceeded`, con
    ///   una frase che dice dove liberare spazio. Fino alla 2.4.0 arrivava come
    ///   «il server ha risposto con un errore», ogni quarto d'ora.
    /// - `userRateLimitExceeded` e `rateLimitExceeded`: troppe richieste. Passa
    ///   aspettando, ed è `net.rateLimited`, che si ritenta.
    /// - tutto il resto resta un `net.http` con il corpo nella causa.
    fn guasto(&self, risposta: &Risposta, url: &str) -> AppError {
        match (risposta.stato, motivo_di_drive(&risposta.corpo).as_deref()) {
            (403, Some("storageQuotaExceeded")) => AppError::new(ErrorCode::SyncQuotaExceeded)
                .with_cause("Drive: storageQuotaExceeded".to_owned()),
            (403, Some("userRateLimitExceeded" | "rateLimitExceeded")) => {
                AppError::new(ErrorCode::NetRateLimited {
                    service: Some("drive".to_owned()),
                    retry_after_ms: risposta.riprova_fra_ms,
                })
                .with_cause(format!(
                    "Drive: {}",
                    motivo_di_drive(&risposta.corpo).unwrap_or_default()
                ))
            }
            _ => self.rete.stato_a_errore(risposta, url),
        }
    }
}

/// Com'è andato un pezzo di un caricamento ripartibile.
enum Passo {
    /// Drive ha tutto il file, e questo è quel che ne dice.
    Finito(FileRemoto),
    /// Drive ha i byte fino a questo, escluso: si continua da qui.
    Ricevuti(usize),
}

/// Quanti byte Drive dice di avere, da `Range: bytes=0-N` di un `308`.
///
/// `N` è l'ultimo byte **compreso**, quindi si riparte da `N + 1`. Senza
/// intestazione Drive non ha niente, e si riparte da zero; un valore che non
/// si legge vale lo stesso, perché è la direzione che non salta byte.
fn ricevuti_fin_qui(intervallo: Option<&str>) -> usize {
    intervallo
        .and_then(|valore| valore.trim().strip_prefix("bytes="))
        .and_then(|valore| valore.split_once('-'))
        .and_then(|(_, ultimo)| ultimo.trim().parse::<usize>().ok())
        .map_or(0, |ultimo| ultimo.saturating_add(1))
}

/// Il `reason` del primo errore dichiarato nel corpo di una risposta di Drive.
///
/// La forma documentata è `{"error": {"errors": [{"reason": "…"}]}}`; quella
/// più nuova delle API di Google porta lo stesso nome in
/// `error.details[].reason`. Si guardano tutte e due, perché Drive le manda a
/// seconda del punto che risponde.
fn motivo_di_drive(corpo: &[u8]) -> Option<String> {
    let documento: serde_json::Value = serde_json::from_slice(corpo).ok()?;
    let errore = documento.get("error")?;
    ["errors", "details"]
        .iter()
        .filter_map(|campo| errore.get(*campo).and_then(serde_json::Value::as_array))
        .flatten()
        .find_map(|voce| voce.get("reason").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
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

    /// Drive segue i suoi rimandi solo verso Google, e ne segue pochi.
    #[test]
    fn drive_segue_i_rimandi_solo_verso_google() {
        let drive = Drive::nuovo(
            Rete::nuova("drive", std::time::Duration::from_secs(1)),
            "token",
        );
        assert!(
            matches!(
                drive.rete.rimandi(),
                Rimandi::Sorvegliati {
                    quanti: RIMANDI_DRIVE,
                    ..
                }
            ),
            "la politica di Drive vale qualunque rete gli si passi"
        );

        for buono in [
            "https://www.googleapis.com/drive/v3/files/x?alt=media",
            "https://doc-0s-9c-apidata.googleusercontent.com/download/drive/v3/files/x?qk=abc",
            "https://WWW.GOOGLEAPIS.COM/upload/drive/v3/files",
        ] {
            assert!(verso_google(buono), "«{buono}» è Google");
        }
        for cattivo in [
            "http://www.googleapis.com/drive/v3/files/x",
            "https://evilgoogleapis.com/x",
            "https://googleapis.com.esempio.com/x",
            "https://www.googleapis.com@esempio.com/x",
            "https://esempio.com/www.googleapis.com/x",
            "https://127.0.0.1/x",
        ] {
            assert!(!verso_google(cattivo), "«{cattivo}» non è Google");
        }
    }

    /// Una richiesta come la vede il Drive finto.
    #[derive(Debug, Clone)]
    struct Vista {
        metodo: String,
        percorso: String,
        intestazioni: Vec<(String, String)>,
        corpo: Vec<u8>,
    }

    impl Vista {
        fn intestazione(&self, nome: &str) -> Option<&str> {
            self.intestazioni
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(nome))
                .map(|(_, v)| v.as_str())
        }
    }

    /// Un Drive finto su questa macchina: una connessione per richiesta, e una
    /// chiusura che decide cosa rispondere guardando la richiesta intera.
    ///
    /// Le prove non escono mai da questo computer: il client vero si punta qui
    /// con `Drive::verso_prova`, e quel che si prova è proprio chi ha ricevuto
    /// cosa.
    struct Finto {
        base: String,
        viste: std::sync::Arc<std::sync::Mutex<Vec<Vista>>>,
    }

    impl Finto {
        fn avvia(risponde: impl Fn(&Vista) -> Vec<u8> + Send + 'static) -> Self {
            use std::io::{BufRead as _, Read as _, Write as _};
            let ascolto =
                std::net::TcpListener::bind("127.0.0.1:0").expect("una porta su questa macchina");
            let porta = ascolto.local_addr().expect("la porta").port();
            let viste = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let registro = std::sync::Arc::clone(&viste);
            std::thread::spawn(move || {
                for connessione in ascolto.incoming() {
                    let Ok(mut connessione) = connessione else {
                        continue;
                    };
                    let mut lettore =
                        std::io::BufReader::new(connessione.try_clone().expect("clone"));
                    let mut prima = String::new();
                    if lettore.read_line(&mut prima).is_err() {
                        continue;
                    }
                    let mut intestazioni = Vec::new();
                    loop {
                        let mut riga = String::new();
                        if lettore.read_line(&mut riga).is_err() || riga.trim().is_empty() {
                            break;
                        }
                        if let Some((n, v)) = riga.trim_end().split_once(':') {
                            intestazioni.push((n.trim().to_owned(), v.trim().to_owned()));
                        }
                    }
                    let lunghezza = intestazioni
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, v)| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    let mut corpo = vec![0_u8; lunghezza];
                    if lettore.read_exact(&mut corpo).is_err() {
                        continue;
                    }
                    let mut parti = prima.split_whitespace();
                    let vista = Vista {
                        metodo: parti.next().unwrap_or_default().to_owned(),
                        percorso: parti.next().unwrap_or_default().to_owned(),
                        intestazioni,
                        corpo,
                    };
                    let risposta = risponde(&vista);
                    registro.lock().expect("registro").push(vista);
                    let _ = connessione.write_all(&risposta);
                }
            });
            Self {
                base: format!("http://127.0.0.1:{porta}"),
                viste,
            }
        }

        fn viste(&self) -> Vec<Vista> {
            self.viste.lock().expect("registro").clone()
        }

        fn drive(&self) -> Drive {
            let rete = Rete::verso("drive", std::time::Duration::from_secs(10), &self.base)
                .expect("un indirizzo di casa");
            Drive::verso_prova(rete, "token-di-prova", &self.base)
        }
    }

    /// Una risposta HTTP con il suo corpo e le intestazioni dette.
    fn risposta(stato: &str, intestazioni: &[(&str, &str)], corpo: &[u8]) -> Vec<u8> {
        let mut testa = format!("HTTP/1.1 {stato}\r\n");
        for (nome, valore) in intestazioni {
            testa.push_str(&format!("{nome}: {valore}\r\n"));
        }
        testa.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            corpo.len()
        ));
        let mut tutto = testa.into_bytes();
        tutto.extend_from_slice(corpo);
        tutto
    }

    /// Il ripristino con una skin da undici megabyte: col tetto di `ureq`
    /// falliva per intero, adesso scende.
    #[test]
    fn un_file_oltre_i_dieci_megabyte_scende_intero() {
        let finto = Finto::avvia(|vista| {
            assert!(vista.percorso.contains("alt=media"), "{}", vista.percorso);
            risposta("200 OK", &[], &vec![7_u8; 11 * 1024 * 1024])
        });
        let byte = finto
            .drive()
            .scarica("skin")
            .expect("sotto il tetto di Drive");
        assert_eq!(byte.len(), 11 * 1024 * 1024);
    }

    /// Un pezzo che cade non fa ripartire il file: si chiede a Drive dove è
    /// arrivato, e si riprende da lì.
    #[test]
    fn un_caricamento_a_pezzi_riprende_da_dove_drive_e_arrivato() {
        let ricevuti = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let caduto = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (suoi, sua_caduta) = (
            std::sync::Arc::clone(&ricevuti),
            std::sync::Arc::clone(&caduto),
        );
        let finto = Finto::avvia(move |vista| {
            if vista.percorso.contains("uploadType=resumable") {
                // L'indirizzo di sessione è intero, come lo manda Google.
                let sessione = format!(
                    "http://{}/sessione",
                    vista.intestazione("host").unwrap_or_default()
                );
                return risposta("200 OK", &[("Location", &sessione)], b"");
            }
            assert_eq!(vista.metodo, "PUT");
            assert!(
                vista.intestazione("authorization").is_none(),
                "la sessione non porta il token"
            );
            let intervallo = vista
                .intestazione("content-range")
                .unwrap_or_default()
                .to_owned();
            let mut presi = suoi.lock().expect("ricevuti");
            if let Some(totale) = intervallo.strip_prefix("bytes */") {
                // La domanda «a che punto sei».
                let _ = totale;
                let fin_qui = format!("bytes=0-{}", presi.len().saturating_sub(1));
                return risposta("308 Resume Incomplete", &[("Range", &fin_qui)], b"");
            }
            // Il secondo pezzo cade una volta, senza arrivare.
            if presi.len() == 8 && !sua_caduta.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return risposta("503 Service Unavailable", &[], b"");
            }
            presi.extend_from_slice(&vista.corpo);
            if presi.len() >= 20 {
                return risposta(
                    "200 OK",
                    &[("Content-Type", "application/json")],
                    br#"{"id":"nuovo","name":"aether-skin-x.aeskin","size":"20"}"#,
                );
            }
            let fin_qui = format!("bytes=0-{}", presi.len() - 1);
            risposta("308 Resume Incomplete", &[("Range", &fin_qui)], b"")
        });

        let dati: Vec<u8> = (0..20_u8).collect();
        let file = finto
            .drive()
            .carica(
                "aether-skin-x.aeskin",
                None,
                "application/zip",
                &dati,
                "impronta",
            )
            .expect("il caricamento finisce");
        assert_eq!(file.id, "nuovo");
        assert_eq!(
            *ricevuti.lock().expect("ricevuti"),
            dati,
            "i byte sono quelli, una volta sola"
        );
        assert!(
            caduto.load(std::sync::atomic::Ordering::SeqCst),
            "il guasto c'è stato"
        );

        let pezzi: Vec<String> = finto
            .viste()
            .iter()
            .filter_map(|v| v.intestazione("content-range").map(ToOwned::to_owned))
            .collect();
        assert_eq!(
            pezzi,
            [
                "bytes 0-7/20",
                "bytes 8-15/20",
                "bytes */20",
                "bytes 8-15/20",
                "bytes 16-19/20"
            ],
            "dopo il guasto si chiede dove si è, e si riprende dal pezzo caduto, non da capo"
        );
    }

    /// Lo spazio pieno ha una frase sua, e non si ritenta.
    #[test]
    fn lo_spazio_pieno_di_drive_ha_il_suo_codice() {
        let finto = Finto::avvia(|_| {
            risposta(
                "403 Forbidden",
                &[("Content-Type", "application/json")],
                br#"{"error":{"errors":[{"domain":"usageLimits","reason":"storageQuotaExceeded","message":"The user's Drive storage quota has been exceeded."}],"code":403}}"#,
            )
        });
        let mut drive = finto.drive();
        drive.limite_multipart = LIMITE_MULTIPART;
        let err = drive
            .carica(
                "aether-libreria.v1.json.gz",
                None,
                "application/gzip",
                b"x",
                "i",
            )
            .expect_err("Drive è pieno");
        assert_eq!(err.code().kind().code(), "sync.quotaExceeded");
        assert!(!err.is_retryable(), "lo spazio non si libera riprovando");
        assert_eq!(finto.viste().len(), 1, "una richiesta sola");
    }

    /// Un limite di frequenza di Drive arriva come `403`, e si aspetta.
    #[test]
    fn un_limite_di_frequenza_di_drive_si_ritenta() {
        let volte = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sue = std::sync::Arc::clone(&volte);
        let finto = Finto::avvia(move |_| {
            if sue.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                return risposta(
                    "403 Forbidden",
                    &[("Retry-After", "0")],
                    br#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}],"code":403}}"#,
                );
            }
            risposta("200 OK", &[], br#"{"files":[]}"#)
        });
        assert_eq!(finto.drive().elenca(), Ok(Vec::new()));
        assert_eq!(volte.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn quel_che_drive_ha_ricevuto_si_legge_dal_range() {
        assert_eq!(ricevuti_fin_qui(Some("bytes=0-524287")), 524_288);
        assert_eq!(ricevuti_fin_qui(Some(" bytes=0-0 ")), 1);
        assert_eq!(
            ricevuti_fin_qui(None),
            0,
            "senza intestazione non ha niente"
        );
        assert_eq!(ricevuti_fin_qui(Some("sciocchezze")), 0);
    }

    #[test]
    fn il_token_non_compare_nel_debug() {
        let drive = Drive::nuovo(
            Rete::nuova("drive", std::time::Duration::from_secs(1)),
            "ya29.segretissimo",
        );
        assert!(!format!("{drive:?}").contains("segretissimo"));
    }

    #[test]
    fn drive_alza_il_tetto_del_corpo() {
        let drive = Drive::nuovo(
            Rete::nuova("drive", std::time::Duration::from_secs(1)),
            "token",
        );
        assert_eq!(drive.rete.tetto_corpo(), TETTO_DRIVE);
        assert_eq!(drive.trasferimenti.tetto_corpo(), TETTO_DRIVE);
        assert_eq!(
            drive.trasferimenti.scadenza_complessiva(),
            None,
            "uno scarico ha i tempi di un prelievo, non di una domanda"
        );
        assert_eq!(PEZZO % (256 * 1024), 0, "Google vuole multipli di 256 KiB");
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
