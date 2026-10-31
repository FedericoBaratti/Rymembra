//! Un server WebDAV: Nextcloud, ownCloud, pCloud, Koofr, kDrive, un Synology
//! con un certificato vero.
//!
//! # Solo `https`
//!
//! Il permesso qui è una password (di solito una «password per app»), che a
//! ogni richiesta viaggia nell'intestazione `Authorization: Basic`. Su `http`
//! la leggerebbe chiunque stia fra il PC e il server. Un NAS di casa senza
//! certificato si aggiunge come cartella di rete (`\\nas\Musica`), che
//! Rymembra sorveglia già.
//!
//! # L'identificativo è il percorso
//!
//! WebDAV non ha identificativi: una risorsa è il suo indirizzo. Qui
//! l'identificativo è il percorso decodificato (`/remote.php/dav/files/mario/
//! Musica/01.flac`), e la cima è l'indirizzo che l'utente ha scritto. Uno
//! spostamento è una voce tolta e una nuova, e la scansione lo riconosce dal
//! contenuto, come per un file spostato sul disco.
//!
//! # I cambiamenti
//!
//! WebDAV non ha un flusso dei cambiamenti. Nextcloud e ownCloud però
//! cambiano l'`ETag` di una cartella quando cambia qualcosa sotto di lei, fino
//! alla cima: il cursore è l'`ETag` della cima, e quando cambia si rielenca
//! tutto e lo specchio confronta. Un server che gli `ETag` delle cartelle non
//! li propaga si rielenca comunque ogni [`RIPASSO`].

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::nuvola::{Cambio, Servizio, VoceRemota, interessa};
use rymembra_net::flusso::Sorgente;
use rymembra_net::http::{Corpo, Metodo, Pezzo, Rete, Richiesta, Rimandi, Risposta, percento};

use super::{Deposito, Identita, errore_cursore_scaduto, ripasso_periodico};

const CHIAVE: &str = "webdav";

const SCADENZA_DOMANDE: Duration = Duration::from_secs(30);
const TENTATIVI: u32 = 3;

/// Quante cartelle si chiedono insieme durante l'elenco.
const INSIEME: usize = 4;

/// Ogni quanto si rielenca tutto anche senza segni di cambiamento: per i
/// server che non propagano gli `ETag` delle cartelle.
const RIPASSO: Duration = Duration::from_secs(30 * 60);

/// Le proprietà che si chiedono.
const PROPRIETA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:displayname/><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/></d:prop></d:propfind>"#;

/// Una risorsa di una risposta `multistatus`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Risorsa {
    /// Il percorso decodificato, senza la barra finale.
    percorso: String,
    cartella: bool,
    dimensione: u64,
    modificata: Option<String>,
    etag: Option<String>,
}

/// Un server WebDAV. Vedi il `//!`.
#[derive(Debug)]
pub struct WebDav {
    rete: Rete,
    flusso: Rete,
    /// `https://cloud.esempio.it`.
    origine: String,
    /// Il percorso della cima, decodificato e senza barra finale.
    base: String,
    utente: String,
    /// `Basic …`.
    autorizzazione: String,
    /// Il conto è stato scollegato: nessuna domanda parte più. Un elenco di un
    /// server grande dura minuti, e senza questo continuava a riempire lo
    /// specchio di un conto che non c'era più.
    spento: AtomicBool,
}

impl WebDav {
    /// Un server, dall'indirizzo che l'utente ha scritto.
    ///
    /// # Errori
    ///
    /// `cloud.insecureServer` per `http://`; `cloud.credentialsInAddress` per
    /// un indirizzo con `utente:password@` prima del server; `net.badSchema`
    /// per un indirizzo che non si legge.
    pub fn nuovo(indirizzo: &str, utente: &str, password: &str) -> Result<Self, AppError> {
        let (origine, base) = scomponi(indirizzo)?;
        if !origine.starts_with("https://") && !cfg!(test) {
            return Err(AppError::new(ErrorCode::CloudInsecureServer));
        }
        Ok(Self {
            rete: Rete::nuova(CHIAVE, SCADENZA_DOMANDE).con_rimandi(Rimandi::Nessuno),
            flusso: Rete::per_flusso(CHIAVE).con_rimandi(Rimandi::Nessuno),
            origine,
            base,
            utente: utente.to_owned(),
            autorizzazione: format!(
                "Basic {}",
                base64(format!("{utente}:{password}").as_bytes())
            ),
            spento: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    fn per_prova(rete: Rete, indirizzo: &str, utente: &str, password: &str) -> Self {
        let mut io = Self::nuovo(indirizzo, utente, password).expect("indirizzo di prova");
        io.rete = rete.clone();
        io.flusso = rete;
        io
    }

    /// L'indirizzo di un percorso, coi segmenti codificati.
    ///
    /// La cima di un server che serve WebDAV dalla radice (`https://nas/`) non
    /// ha segmenti: una barra sola, non `https://nas//`, che più di un server
    /// non riconosce.
    fn url(&self, percorso: &str) -> String {
        let segmenti: Vec<String> = percorso
            .split('/')
            .filter(|s| !s.is_empty())
            .map(percento)
            .collect();
        if segmenti.is_empty() {
            return format!("{}/", self.origine);
        }
        format!("{}/{}/", self.origine, segmenti.join("/"))
    }

    /// Un `PROPFIND` con la sua profondità.
    fn propfind(&self, percorso: &str, profondita: &str) -> Result<Vec<Risorsa>, AppError> {
        if self.spento.load(Ordering::Acquire) {
            return Err(AppError::new(ErrorCode::CloudNotConnected {
                servizio: Servizio::WebDav.nome().to_owned(),
            })
            .with_cause("server scollegato"));
        }
        let url = self.url(percorso);
        let risposta = self.rete.ritenta(TENTATIVI, || {
            self.rete.esegui(Richiesta {
                metodo: Metodo::Propfind,
                url: &url,
                intestazioni: &[
                    ("authorization", &self.autorizzazione),
                    ("depth", profondita),
                ],
                corpo: Corpo::Byte {
                    tipo: "application/xml; charset=utf-8",
                    dati: PROPRIETA.as_bytes(),
                },
            })
        })?;
        if risposta.stato != 207 {
            return Err(self.errore(&risposta, &url));
        }
        let testo = String::from_utf8_lossy(&risposta.corpo);
        leggi_multistatus(&testo)
            .map_err(|dettaglio| forma(&format!("multistatus illeggibile: {dettaglio}")))
    }

    fn errore(&self, risposta: &Risposta, url: &str) -> AppError {
        match risposta.stato {
            401 => AppError::new(ErrorCode::CloudAuthExpired {
                servizio: Servizio::WebDav.nome().to_owned(),
            }),
            404 => AppError::new(ErrorCode::CloudNotFound),
            _ => self.rete.stato_a_errore(risposta, url),
        }
    }

    /// Le voci figlie di una cartella (la cartella stessa esclusa).
    fn figli(&self, cartella: &str) -> Result<Vec<VoceRemota>, AppError> {
        let risorse = self.propfind(cartella, "1")?;
        Ok(risorse
            .into_iter()
            .filter(|r| r.percorso != cartella)
            .filter_map(|r| self.voce(r))
            .collect())
    }

    fn voce(&self, risorsa: Risorsa) -> Option<VoceRemota> {
        let (madre, nome) = risorsa.percorso.rsplit_once('/')?;
        Some(VoceRemota {
            genitore: Some(if madre.is_empty() {
                "/".to_owned()
            } else {
                madre.to_owned()
            }),
            nome: nome.to_owned(),
            cartella: risorsa.cartella,
            dimensione: risorsa.dimensione,
            modificato_ms: risorsa
                .modificata
                .as_deref()
                .and_then(data_http)
                .unwrap_or(0),
            versione: risorsa.etag,
            tag: None,
            id: risorsa.percorso,
        })
    }

    /// La catena dalla cima a `cartella`: la cima senza nome, poi un antenato
    /// per segmento.
    fn catena(&self, cartella: &str) -> Vec<VoceRemota> {
        let mut voci = vec![VoceRemota {
            id: self.base.clone(),
            genitore: None,
            nome: String::new(),
            cartella: true,
            dimensione: 0,
            modificato_ms: 0,
            versione: None,
            tag: None,
        }];
        let resto = cartella.strip_prefix(&self.base).unwrap_or_default();
        let mut finora = self.base.clone();
        for segmento in resto.split('/').filter(|s| !s.is_empty()) {
            let madre = finora.clone();
            finora = unisci(&finora, segmento);
            voci.push(VoceRemota {
                id: finora.clone(),
                genitore: Some(madre),
                nome: segmento.to_owned(),
                cartella: true,
                dimensione: 0,
                modificato_ms: 0,
                versione: None,
                tag: None,
            });
        }
        voci
    }

    /// L'`ETag` della cima.
    fn etag_della_cima(&self) -> Result<String, AppError> {
        let risorse = self.propfind(&self.base, "0")?;
        Ok(risorse
            .into_iter()
            .next()
            .and_then(|r| r.etag)
            .unwrap_or_default())
    }
}

impl Deposito for WebDav {
    fn servizio(&self) -> Servizio {
        Servizio::WebDav
    }

    fn spegni(&self) {
        self.spento.store(true, Ordering::Release);
    }

    /// Chi è: l'indirizzo e l'utente. Una domanda vera al server, perché è il
    /// momento in cui si scopre una password sbagliata.
    fn chi(&self) -> Result<Identita, AppError> {
        match self.propfind(&self.base, "0") {
            Err(err) if matches!(err.code(), ErrorCode::CloudAuthExpired { .. }) => {
                return Err(AppError::new(ErrorCode::CloudWrongCredentials));
            }
            altro => {
                altro?;
            }
        }
        let ospite = self
            .origine
            .split("://")
            .nth(1)
            .unwrap_or(&self.origine)
            .to_ascii_lowercase();
        Ok(Identita {
            id_stabile: format!(
                "{}{}|{}",
                self.origine.to_ascii_lowercase(),
                self.base,
                self.utente
            ),
            nome: format!("{}@{ospite}", self.utente),
        })
    }

    fn cartelle(&self, dentro: Option<&str>) -> Result<Vec<VoceRemota>, AppError> {
        let cartella = dentro.filter(|d| *d != "root").unwrap_or(&self.base);
        Ok(self
            .figli(cartella)?
            .into_iter()
            .filter(|v| v.cartella)
            .collect())
    }

    fn cursore_adesso(&self) -> Result<String, AppError> {
        Ok(format!(
            "{}|{}",
            self.etag_della_cima()?,
            rymembra_oauth::adesso_ms()
        ))
    }

    fn elenca(
        &self,
        cima: &str,
        pagina: &mut dyn FnMut(Vec<VoceRemota>) -> ControlFlow<()>,
    ) -> Result<bool, AppError> {
        let cima = if cima == "root" {
            self.base.as_str()
        } else {
            cima
        };
        let mut in_testa = self.catena(cima);
        let mut livello = vec![cima.to_owned()];
        while !livello.is_empty() {
            let mut prossimo = Vec::new();
            for gruppo in livello.chunks(INSIEME) {
                let risultati: Vec<Result<Vec<VoceRemota>, AppError>> =
                    std::thread::scope(|ambito| {
                        let fili: Vec<_> = gruppo
                            .iter()
                            .map(|cartella| ambito.spawn(move || self.figli(cartella)))
                            .collect();
                        fili.into_iter()
                            .map(|filo| {
                                filo.join()
                                    .unwrap_or_else(|_| Err(forma("un filo dell'elenco è caduto")))
                            })
                            .collect()
                    });
                for risultato in risultati {
                    let mut voci = std::mem::take(&mut in_testa);
                    for voce in risultato? {
                        if voce.cartella {
                            prossimo.push(voce.id.clone());
                            voci.push(voce);
                        } else if interessa(&voce.nome) {
                            voci.push(voce);
                        }
                    }
                    if pagina(voci).is_break() {
                        return Ok(false);
                    }
                }
            }
            livello = prossimo;
        }
        Ok(true)
    }

    /// L'`ETag` della cima uguale, e il ripasso non ancora scaduto: niente da
    /// dire. Altrimenti il cursore è scaduto, e chi chiama rielenca tutto.
    fn cambiamenti(
        &self,
        cursore: &str,
        attesa: Duration,
    ) -> Result<(Vec<Cambio>, String), AppError> {
        let _ = attesa;
        let (etag, quando) = cursore.rsplit_once('|').unwrap_or((cursore, "0"));
        let quando: i64 = quando.parse().unwrap_or(0);
        let passati = rymembra_oauth::adesso_ms().saturating_sub(quando);
        if u128::try_from(passati).unwrap_or(0) > RIPASSO.as_millis() {
            return Err(ripasso_periodico());
        }
        // Due `ETag` vuoti sono uguali: un server che non lo dà per le
        // cartelle si rielenca al ripasso, non a ogni giro. Fino alla 3.2.0 di
        // prova un `ETag` vuoto valeva «cambiato», e un server così si
        // rielencava per intero ogni quarantacinque secondi.
        if self.etag_della_cima()? != etag {
            return Err(errore_cursore_scaduto("la cima ha cambiato ETag"));
        }
        Ok((Vec::new(), cursore.to_owned()))
    }

    fn sorgente(&self, voce: &VoceRemota) -> Result<Box<dyn Sorgente>, AppError> {
        let mut url = self.url(&voce.id);
        // Un file non ha la barra finale delle cartelle.
        url.pop();
        Ok(Box::new(DaWebDav {
            flusso: self.flusso.clone(),
            url,
            autorizzazione: self.autorizzazione.clone(),
        }))
    }

    /// Per Nextcloud e ownCloud, la cartella nella loro interfaccia web.
    fn indirizzo_web(&self, voce: &VoceRemota) -> Option<String> {
        let cartella = if voce.cartella {
            voce.id.as_str()
        } else {
            voce.genitore.as_deref()?
        };
        let radice = ["/remote.php/dav/files/", "/remote.php/webdav"]
            .iter()
            .find_map(|segno| cartella.find(segno).map(|i| (i, *segno)))?;
        let (inizio, segno) = radice;
        let dopo = cartella.get(inizio + segno.len()..).unwrap_or_default();
        // Per `dav/files/<utente>/…` l'utente non fa parte della cartella.
        let dentro = if segno.ends_with("files/") {
            dopo.split_once('/').map_or("", |(_, resto)| resto)
        } else {
            dopo.trim_start_matches('/')
        };
        let prima = cartella.get(..inizio).unwrap_or_default();
        Some(format!(
            "{}{prima}/index.php/apps/files/?dir={}",
            self.origine,
            percento(&format!("/{dentro}"))
        ))
    }
}

/// I byte di un file WebDAV: `GET` con `Range` e la password.
#[derive(Debug)]
pub struct DaWebDav {
    flusso: Rete,
    url: String,
    autorizzazione: String,
}

impl Sorgente for DaWebDav {
    fn pezzo(&self, da: u64, quanti: u64) -> Result<Pezzo, AppError> {
        self.pezzo_finche(da, quanti, &|| true)
    }

    fn pezzo_finche(
        &self,
        da: u64,
        quanti: u64,
        avanti: &dyn Fn() -> bool,
    ) -> Result<Pezzo, AppError> {
        self.flusso
            .intervallo_con(
                &self.url,
                da,
                quanti,
                avanti,
                &[("authorization", &self.autorizzazione)],
            )
            .map_err(|err| match err.code() {
                // Un `200` a una richiesta da metà file: il server non sa
                // servire un pezzo, e ogni salto sarebbe il file da capo.
                ErrorCode::DownloadBadResponse if da > 0 => {
                    AppError::new(ErrorCode::CloudRangeUnsupported)
                }
                ErrorCode::NetHttp { status: 401, .. } => {
                    AppError::new(ErrorCode::CloudAuthExpired {
                        servizio: Servizio::WebDav.nome().to_owned(),
                    })
                }
                _ => err,
            })
    }
}

/// Un percorso e un segmento: `/` + `Musica` è `/Musica`, non `//Musica`.
fn unisci(madre: &str, segmento: &str) -> String {
    format!("{}/{segmento}", madre.trim_end_matches('/'))
}

/// Origine e percorso della cima, da un indirizzo.
fn scomponi(indirizzo: &str) -> Result<(String, String), AppError> {
    let pulito = indirizzo.trim();
    let (schema, resto) = pulito
        .split_once("://")
        .ok_or_else(|| forma("l'indirizzo non ha uno schema"))?;
    let schema = schema.to_ascii_lowercase();
    if schema != "https" && schema != "http" {
        return Err(forma("lo schema non è https"));
    }
    let (ospite, percorso) = resto.split_once('/').unwrap_or((resto, ""));
    if ospite.is_empty() {
        return Err(forma("l'indirizzo non ha un server"));
    }
    // `utente:password@` prima del server: la forma che molte guide mostrano.
    // Tenuta, l'autorità intera diventava l'origine, e la password finiva nel
    // nome e nell'identificativo del conto, nei dettagli salvati nel database
    // e in ogni errore di rete scritto nel diario. Utente e password hanno i
    // loro campi, e la password il portachiavi. Solo l'autorità: una
    // chiocciola nel percorso (`…/dav/files/mario@esempio.it/`) è un nome
    // utente fatto da un'email, e va bene.
    if ospite.contains('@') {
        return Err(AppError::new(ErrorCode::CloudCredentialsInAddress)
            .with_cause("utente e password vanno nei campi, non nell'indirizzo"));
    }
    let percorso = decodifica(percorso);
    // La cima del server è `/`; ogni altra cartella senza la barra finale.
    let base = format!("/{}", percorso.trim_matches('/'));
    Ok((format!("{schema}://{ospite}"), base))
}

/// Decodifica `%XX`.
fn decodifica(testo: &str) -> String {
    let mut byte = Vec::with_capacity(testo.len());
    let mut resto = testo.bytes();
    while let Some(b) = resto.next() {
        if b == b'%' {
            let alto = resto.next();
            let basso = resto.next();
            let valore = alto
                .zip(basso)
                .and_then(|(a, b)| u8::from_str_radix(std::str::from_utf8(&[a, b]).ok()?, 16).ok());
            match valore {
                Some(v) => byte.push(v),
                None => {
                    byte.push(b'%');
                    byte.extend(alto);
                    byte.extend(basso);
                }
            }
        } else {
            byte.push(b);
        }
    }
    String::from_utf8_lossy(&byte).into_owned()
}

/// Il percorso di un `href`, assoluto o relativo, decodificato e senza la
/// barra finale.
fn percorso_di_href(href: &str) -> String {
    let senza_origine = match href.split_once("://") {
        Some((_, resto)) => resto.find('/').and_then(|i| resto.get(i..)).unwrap_or("/"),
        None => href,
    };
    let decodificato = decodifica(senza_origine);
    let pulito = decodificato.trim_end_matches('/');
    if pulito.starts_with('/') {
        pulito.to_owned()
    } else {
        format!("/{pulito}")
    }
}

/// Le risorse di una risposta `multistatus`. Le proprietà contano solo da un
/// `propstat` con stato 200.
fn leggi_multistatus(xml: &str) -> Result<Vec<Risorsa>, String> {
    let mut lettore = Reader::from_str(xml);
    lettore.config_mut().trim_text(true);
    let mut risorse = Vec::new();
    let mut attuale: Option<Risorsa> = None;
    let mut proprieta = Risorsa::default();
    let mut stato_buono = false;
    let mut testo = String::new();
    let mut in_resourcetype = false;
    loop {
        match lettore.read_event().map_err(|err| err.to_string())? {
            Event::Start(elemento) => {
                testo.clear();
                match elemento.local_name().as_ref() {
                    "response" => attuale = Some(Risorsa::default()),
                    "propstat" => {
                        proprieta = Risorsa::default();
                        stato_buono = false;
                    }
                    "resourcetype" => in_resourcetype = true,
                    "collection" if in_resourcetype => proprieta.cartella = true,
                    _ => {}
                }
            }
            Event::Empty(elemento) => {
                if elemento.local_name().as_ref() == "collection" && in_resourcetype {
                    proprieta.cartella = true;
                }
            }
            Event::Text(pezzo) => testo.push_str(&pezzo.xml10_content()),
            Event::CData(pezzo) => testo.push_str(&pezzo.xml10_content()),
            Event::GeneralRef(riferimento) => {
                let nome: &str = AsRef::<str>::as_ref(&riferimento);
                match nome {
                    "amp" => testo.push('&'),
                    "lt" => testo.push('<'),
                    "gt" => testo.push('>'),
                    "quot" => testo.push('"'),
                    "apos" => testo.push('\''),
                    _ => {
                        if let Ok(Some(carattere)) = riferimento.resolve_char_ref() {
                            testo.push(carattere);
                        }
                    }
                }
            }
            Event::End(elemento) => {
                let valore = std::mem::take(&mut testo);
                match elemento.local_name().as_ref() {
                    "href" => {
                        if let Some(r) = attuale.as_mut() {
                            r.percorso = percorso_di_href(valore.trim());
                        }
                    }
                    "status" => {
                        stato_buono = valore.split_whitespace().nth(1) == Some("200");
                    }
                    "getcontentlength" => proprieta.dimensione = valore.trim().parse().unwrap_or(0),
                    "getlastmodified" => proprieta.modificata = Some(valore.trim().to_owned()),
                    "getetag" => proprieta.etag = Some(valore.trim().trim_matches('"').to_owned()),
                    "resourcetype" => in_resourcetype = false,
                    "propstat" => {
                        if stato_buono && let Some(r) = attuale.as_mut() {
                            r.cartella |= proprieta.cartella;
                            if proprieta.dimensione > 0 {
                                r.dimensione = proprieta.dimensione;
                            }
                            if proprieta.modificata.is_some() {
                                r.modificata = proprieta.modificata.take();
                            }
                            if proprieta.etag.is_some() {
                                r.etag = proprieta.etag.take();
                            }
                        }
                    }
                    "response" => {
                        if let Some(r) = attuale.take()
                            && !r.percorso.is_empty()
                        {
                            risorse.push(r);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(risorse)
}

/// Una data HTTP (RFC 1123: `Tue, 01 Oct 2026 10:00:00 GMT`) in millisecondi.
fn data_http(testo: &str) -> Option<i64> {
    let mut parti = testo.split_whitespace().skip(1);
    let giorno: u32 = parti.next()?.parse().ok()?;
    let mese = match parti.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let anno: i32 = parti.next()?.parse().ok()?;
    let ora = parti.next()?;
    rymembra_domain::tempo::istante_ms(&format!("{anno:04}-{mese:02}-{giorno:02}T{ora}Z"))
}

/// Base64 con l'alfabeto standard e il riempimento: quello di `Basic`.
fn base64(byte: &[u8]) -> String {
    const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let carattere = |indice: u32| {
        ALFABETO
            .get(usize::try_from(indice & 63).unwrap_or(0))
            .copied()
            .map_or('=', char::from)
    };
    let mut fuori = String::with_capacity(byte.len().div_ceil(3) * 4);
    for gruppo in byte.chunks(3) {
        let a = u32::from(gruppo.first().copied().unwrap_or(0));
        let b = u32::from(gruppo.get(1).copied().unwrap_or(0));
        let c = u32::from(gruppo.get(2).copied().unwrap_or(0));
        let tre = (a << 16) | (b << 8) | c;
        fuori.push(carattere(tre >> 18));
        fuori.push(carattere(tre >> 12));
        fuori.push(if gruppo.len() > 1 {
            carattere(tre >> 6)
        } else {
            '='
        });
        fuori.push(if gruppo.len() > 2 {
            carattere(tre)
        } else {
            '='
        });
    }
    fuori
}

/// Una risposta del server che non ha la forma attesa.
fn forma(dettaglio: &str) -> AppError {
    AppError::new(ErrorCode::NetBadSchema {
        service: Some(CHIAVE.to_owned()),
        detail: Some(dettaglio.to_owned()),
    })
}

#[cfg(test)]
mod prove;
