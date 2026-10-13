//! L'Internet Archive: cercare che cosa c'è, e prenderne i byte.
//!
//! È l'unico catalogo di Aether da cui si **tiene** qualcosa, e la ragione non è
//! tecnica: è che qui il permesso è scritto. Il Live Music Archive esiste perché
//! gli artisti che ci stano dentro hanno acconsentito allo scambio non
//! commerciale delle loro registrazioni dal vivo; le netlabel pubblicano sotto
//! Creative Commons; il pubblico dominio è pubblico dominio. Nessuna di queste
//! tre cose richiede a nessuno di fidarsi di noi.
//!
//! # La cosa da sapere prima di leggere il codice
//!
//! **La ricerca dell'Internet Archive indicizza gli *item*, non i file.** Un
//! item è un concerto intero o un disco intero; il brano singolo è un file
//! dentro l'item, e nell'indice non compare affatto. Cercare «Grateful Dead
//! Sugaree» quindi non restituisce mai un brano: restituisce i concerti in cui
//! quel brano potrebbe esserci.
//!
//! Da lì la forma di questo modulo, che è la cosa che lo distingue da un client
//! HTTP qualunque: si cerca l'item, si chiede la sua scheda, e si guarda **nei
//! file** se il brano c'è. Due richieste invece di una, moltiplicate per quanti
//! item vale la pena aprire — ed è il motivo per cui [`ITEM_DA_APRIRE`] è un
//! numero piccolo e [`primo_sicuro`] esiste per smettere presto.
//!
//! # Che cosa non si prende, e non è una svista
//!
//! Un item marcato come consultabile e basta (`access-restricted-item`, o il
//! vecchio `stream_only` dei soundboard) resta [`Disponibilita::SoloAscolto`]
//! anche se i suoi file si scaricherebbero benissimo. Il fatto che un indirizzo
//! risponda non è un permesso.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{
    BranoEsterno, ContenutoEsterno, Disponibilita, Fonte, GenereContenuto, Licenza, Livello,
};
use aether_domain::scelta::{Candidato, Natura, natura_dal_titolo, query_larga, query_stretta};
use aether_domain::text::fold_text;
use aether_domain::titolo::spezza;
use aether_net::{Corpo, Metodo, Rete, Richiesta, percento};
use serde_json::Value;

/// La radice di tutto.
const BASE: &str = "https://archive.org";

/// Le collezioni in cui si cerca.
///
/// Non «tutto l'audio»: l'Internet Archive ospita anche molto materiale caricato
/// da chi non aveva il diritto di caricarlo, e una ricerca senza questo filtro
/// lo troverebbe insieme al resto. Queste tre hanno ciascuna un patto scritto
/// dietro — il consenso dei tapers, la licenza della netlabel, la scadenza del
/// diritto d'autore.
const COLLEZIONI: &str = "collection:(etree OR netlabels OR audio_music OR 78rpm)";

/// I `mediatype` che contengono musica.
///
/// **Due e non uno, e il secondo è quello che conta.** Qui c'era scritto
/// `mediatype:audio`, che è il modo ovvio di dire «i file sonori» e che
/// all'Internet Archive vuol dire una cosa più stretta: il Live Music Archive
/// — i concerti registrati col permesso di chi li ha suonati, la fonte che
/// questo programma nomina per prima — non è `audio`, è `etree`. Il proprio
/// `mediatype`, per un archivio nato prima che l'audio generico esistesse.
///
/// Il conto, misurato sull'API: `mediatype:audio AND collection:etree`
/// restituisce **quattro** item in tutto l'archivio, e `collection:etree` da
/// solo ne ha oltre duecentomila. Con la riga di prima i concerti non erano
/// pochi o mal ordinati: erano **zero**, e non si vedeva, perché al loro posto
/// arrivavano le cover band di `audio_music` che nominano l'artista nella
/// descrizione. Cercando «Grateful Dead» si trovavano quattro serate di gruppi
/// che suonano i Grateful Dead e nessuna dei Grateful Dead.
const GENERI: &str = "(mediatype:audio OR mediatype:etree)";

/// Come si ordinano gli item trovati.
///
/// Senza questo, l'Archive risponde nell'ordine che gli fa comodo, e non è la
/// rilevanza: è un ordine che a `("Grateful Dead")` metteva in testa una
/// serata del 2021 di un gruppo di Pennsylvania. Conta perché di item se ne
/// apre un numero piccolo — vedi [`ITEM_DA_APRIRE`] — quindi **l'ordine è il
/// filtro**: quel che non entra nei primi quattro non esiste.
///
/// Gli scaricamenti sono la cosa più vicina a un giudizio che l'Archive
/// pubblichi. Non dicono qual è il brano giusto, dicono quale registrazione la
/// gente ha davvero ascoltato, e su un archivio dove lo stesso concerto esiste
/// in otto nastri diversi è esattamente la domanda che chi cerca si sta
/// facendo. Misurato: la stessa ricerca di prima adesso apre con Barton Hall
/// 1977, che è il nastro dei Grateful Dead più ascoltato che ci sia.
const ORDINE: &str = "downloads desc";

/// Quanti item la ricerca restituisce.
const ITEM_TROVATI: u32 = 8;

/// Di quanti item si apre la scheda.
///
/// Quattro e non otto: ogni scheda è una richiesta, e una richiesta a un
/// servizio pubblico gratuito che regge il mondo si paga in cortesia. Con
/// [`primo_sicuro`] la maggior parte delle volte se ne aprono una o due.
const ITEM_DA_APRIRE: usize = 4;

/// Quanto si aspetta **una risposta**.
///
/// Vale per le domande — ricerche, schede, elenchi — e non per il prelievo di
/// un file, che ha una rete sua senza scadenza complessiva: vedi
/// [`Rete::per_prelievo`]. Il nome di questa costante dice «una risposta» da
/// sempre, ed è quel che fa: per un pezzo di tempo ha coperto anche
/// duecento megabyte di FLAC, ed è stato un guasto.
const SCADENZA: Duration = Duration::from_secs(30);

/// I formati che il motore audio sa aprire, dal migliore al peggiore.
///
/// L'ordine è quello di preferenza e conta: l'Internet Archive tiene l'originale
/// senza perdita **e** i derivati compressi che genera lui, e per una libreria
/// che si tiene sul disco l'originale è quel che si vuole. Non c'è `shn` né
/// `ogg`: symphonia non decodifica il primo, e il secondo lo decodifica ma i
/// derivati Ogg dell'Archive sono a bitrate basso, quindi non c'è mai un caso in
/// cui sia la scelta migliore fra quelle presenti.
const FORMATI: [(&str, &str); 4] = [
    ("flac", "flac"),
    ("24bit flac", "flac"),
    ("vbr mp3", "mp3"),
    ("mp3", "mp3"),
];

/// Gli stessi formati, nell'ordine di chi **ascolta** e basta.
///
/// Rovesciato, e il conto è quello che la prova sul campo ha fatto: un FLAC
/// dell'Archive vuole circa centodieci kilobyte al secondo per suonare senza
/// fermarsi, e il nodo che lo serviva ne dava cinquanta. L'MP3 che l'Archive
/// genera dallo stesso nastro ne vuole un quarto, e per un file che nessuno
/// terrà sul disco la differenza di qualità non compra niente: compra solo
/// silenzi in mezzo alla canzone.
const FORMATI_PER_ASCOLTO: [(&str, &str); 4] = [
    ("vbr mp3", "mp3"),
    ("mp3", "mp3"),
    ("flac", "flac"),
    ("24bit flac", "flac"),
];

/// Che cosa si farà del file scelto, che decide quale formato vince.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Uso {
    /// Portarlo sul disco: vince l'originale. Vedi [`FORMATI`].
    Tenere,
    /// Suonarlo mentre arriva: vince il più leggero. Vedi
    /// [`FORMATI_PER_ASCOLTO`].
    Ascoltare,
}

/// Un client dell'Internet Archive.
#[derive(Debug, Clone)]
pub struct ArchivioOrg {
    rete: Rete,
    /// La rete dei prelievi, che ha una scadenza sua: vedi
    /// [`Rete::per_prelievo`].
    prelievo: Rete,
    /// La rete dei flussi: vedi [`Rete::per_flusso`].
    flusso: Rete,
    /// Per ogni FLAC già chiesto, il file che si suona al suo posto.
    ///
    /// Vedi [`Self::da_ascoltare`]. Condivisa fra le copie, come il nodo di
    /// Audius: il lettore clona i cataloghi a ogni apertura, e una memoria che
    /// non sopravvivesse al clone rileggerebbe la scheda a ogni brano.
    ascolto: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for ArchivioOrg {
    fn default() -> Self {
        Self::nuovo()
    }
}

impl ArchivioOrg {
    /// Un client con la scadenza di serie.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            rete: Rete::nuova("archive.org", SCADENZA),
            prelievo: Rete::per_prelievo("archive.org"),
            flusso: Rete::per_flusso("archive.org"),
            ascolto: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// La rete che usa per **chiedere**: ricerche, schede, elenchi.
    ///
    /// Le richieste piccole, con una scadenza complessiva. Per portare giù un
    /// file intero c'è [`Self::rete_prelievo`], per suonarlo mentre arriva
    /// [`Self::rete_flusso`], e la differenza fra le tre è la scadenza.
    #[must_use]
    pub const fn rete(&self) -> &Rete {
        &self.rete
    }

    /// La rete che usa per **suonare un file mentre arriva**.
    ///
    /// Fino alla 2.4.0 i flussi leggevano con [`Self::rete`], e le sue finestre
    /// crescono fino a due megabyte: con trenta secondi complessivi, su una
    /// linea lenta una finestra moriva a metà e con lei la canzone. Vedi
    /// [`Rete::per_flusso`].
    #[must_use]
    pub const fn rete_flusso(&self) -> &Rete {
        &self.flusso
    }

    /// La rete che usa per **portare giù un file**.
    ///
    /// Una seconda riserva di connessioni, e non è uno spreco: è l'unico modo
    /// di avere due politiche di scadenza nello stesso catalogo, perché in
    /// `ureq` la scadenza sta nell'agente e non nella richiesta. Vedi
    /// [`Rete::per_prelievo`] per il conto che ha reso necessaria la
    /// separazione.
    #[must_use]
    pub const fn rete_prelievo(&self) -> &Rete {
        &self.prelievo
    }

    /// Il catalogo risponde.
    ///
    /// Una richiesta vera e piccolissima: la ricerca di un item solo. Serve alla
    /// diagnostica, che deve poter distinguere «non l'ho trovato» da «non ci
    /// arrivo».
    pub fn risponde(&self) -> bool {
        self.cerca_item(GENERI, 1).is_ok()
    }

    /// I brani di questo catalogo che potrebbero essere quello chiesto.
    ///
    /// Prima con la stringa stretta, e solo se non ha dato niente con quella
    /// larga: la seconda richiesta si paga, e nella maggior parte dei casi la
    /// prima basta.
    ///
    /// # Errori
    ///
    /// `download.externalSearchFailed` quando la ricerca non risponde,
    /// `net.*` per i guasti di trasporto.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        let mut trovati = self.cerca_con(&query_stretta(brano), Some(brano), true, annullato)?;
        if trovati.is_empty()
            && let Some(larga) = query_larga(brano)
        {
            trovati = self.cerca_con(&larga, Some(brano), true, annullato)?;
        }
        Ok(trovati)
    }

    /// Gli item di questo catalogo che rispondono a una frase.
    ///
    /// Non c'è un brano da ritrovare: c'è quel che qualcuno ha scritto in una
    /// casella, e nessun titolo atteso con cui filtrare i file dentro l'item.
    /// Vale quindi il tetto di sempre — [`ITEM_DA_APRIRE`] schede, non una di
    /// più — e la frenata di [`primo_sicuro`] qui non può scattare: senza una
    /// durata attesa non si è sicuri di niente.
    ///
    /// `esatta` decide se i termini restano legati in una frase. Per un titolo
    /// sì, ed è quel che fa [`Self::cerca`]; per «lo-fi piano», che non è il
    /// nome di niente, una frase esatta trova quasi sempre zero.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::cerca`].
    pub fn cerca_libera(
        &self,
        testo: &str,
        esatta: bool,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        self.cerca_con(testo, None, esatta, annullato)
    }

    /// Una passata di ricerca con una stringa sola.
    fn cerca_con(
        &self,
        testo: &str,
        atteso: Option<&BranoEsterno>,
        esatta: bool,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        if testo.trim().is_empty() {
            return Ok(Vec::new());
        }
        let item = self.cerca_item(&query_item(testo, esatta), ITEM_TROVATI)?;

        let mut candidati = Vec::new();
        // Un item che non risponde è un risultato in meno, non un fallimento:
        // buttare i candidati già raccolti per un 503 su una scheda qualunque
        // sarebbe il contrario della disciplina di `Cataloghi::cerca`. Il primo
        // guasto si conserva, e si riporta solo se non è rimasto niente.
        let mut primo_guasto: Option<AppError> = None;
        for doc in item.iter().take(ITEM_DA_APRIRE) {
            if annullato() {
                return Ok(candidati);
            }
            let Some(identificativo) = testo_di(doc, "identifier") else {
                continue;
            };
            let scheda = match self.scheda(&identificativo) {
                Ok(scheda) => scheda,
                Err(guasto) => {
                    primo_guasto.get_or_insert(guasto);
                    continue;
                }
            };
            // Con un brano atteso si sta cercando un file da portare giù — è la
            // strada della coda di prelievo; senza, qualcuno ha scritto una
            // frase in Esplora, e quel che troverà lo ascolterà mentre arriva.
            // Chi da lì vuole tenerne una copia passa da [`Self::per_tenere`].
            let uso = if atteso.is_some() {
                Uso::Tenere
            } else {
                Uso::Ascoltare
            };
            candidati.extend(brani_dalla_scheda(&identificativo, &scheda, atteso, uso));
            if atteso.is_some_and(|b| primo_sicuro(&candidati, b)) {
                break;
            }
        }
        if candidati.is_empty()
            && let Some(guasto) = primo_guasto
        {
            return Err(guasto);
        }
        Ok(candidati)
    }

    /// Un item intero, come contenuto importabile.
    ///
    /// È la strada di chi incolla il link di un concerto: non si cerca niente,
    /// si legge quel che c'è dentro nell'ordine in cui sta.
    ///
    /// # Errori
    ///
    /// `catalogo.notPublic` se l'item non esiste, `catalogo.resolveFailed` se
    /// non ha nessun file leggibile.
    pub fn risolvi(&self, identificativo: &str) -> Result<ContenutoEsterno, AppError> {
        let scheda = self.scheda(identificativo)?;
        let metadati = scheda.get("metadata").unwrap_or(&Value::Null);
        if metadati.is_null() {
            return Err(AppError::new(ErrorCode::CatalogoNotPublic)
                .with_cause(format!("l'item «{identificativo}» non esiste")));
        }

        let candidati = brani_dalla_scheda(identificativo, &scheda, None, Uso::Tenere);
        if candidati.is_empty() {
            return Err(
                AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(format!(
                    "l'item «{identificativo}» non ha nessun file in un formato leggibile"
                )),
            );
        }

        let autore = testo_di(metadati, "creator");
        let album = testo_di(metadati, "title");
        let licenza = licenza_da(metadati);
        let disponibilita = disponibilita_da(metadati, &licenza);
        let tracce: Vec<BranoEsterno> = candidati
            .into_iter()
            .enumerate()
            .map(|(n, c)| brano_da_candidato(c, autore.as_deref(), album.as_deref(), n))
            .collect();

        Ok(ContenutoEsterno {
            fonte: Fonte::InternetArchive,
            kind: GenereContenuto::Collezione,
            id: identificativo.to_owned(),
            title: testo_di(metadati, "title").unwrap_or_else(|| identificativo.to_owned()),
            author: autore,
            cover_url: Some(format!("{BASE}/services/img/{}", percento(identificativo))),
            // Il numero di brani è quello che si è letto: la scheda arriva tutta
            // in una risposta, non c'è una pagina dopo, e dichiarare un totale
            // diverso da quello letto sarebbe inventarsi un ammanco.
            declared_total: None,
            tracks: {
                let mut t = tracce;
                for (n, brano) in t.iter_mut().enumerate() {
                    brano.disponibilita = disponibilita;
                    brano.licenza = licenza.clone();
                    brano.track_number = brano
                        .track_number
                        .or_else(|| u32::try_from(n.saturating_add(1)).ok());
                }
                t
            },
            source: Livello::ArchivioOrg,
        })
    }

    /// Lo stesso brano, nel formato da tenere sul disco.
    ///
    /// # Perché serve
    ///
    /// Perché Esplora sceglie per l'ascolto — vedi [`FORMATI_PER_ASCOLTO`] — e
    /// «Tieni una copia» parte da lì: senza questo passo la copia sul disco
    /// sarebbe l'MP3 che l'Archive ha generato, quando accanto c'è l'originale
    /// senza perdita che [`FORMATI`] esiste per preferire.
    ///
    /// Si rilegge la scheda dell'item e si prende il file che ha la **stessa
    /// radice** del nome: è quel che i derivati dell'Archive condividono con
    /// l'originale da cui vengono, ed è la stessa chiave con cui
    /// [`brani_dalla_scheda`] li raggruppa. Se non se ne trova uno migliore,
    /// resta quello di partenza, che è comunque il brano che si è ascoltato.
    ///
    /// # Errori
    ///
    /// Quelli di una scheda: `net.*` per i guasti di trasporto, `download.*`
    /// per una risposta che non si legge.
    pub fn per_tenere(&self, candidato: &Candidato) -> Result<Candidato, AppError> {
        let Some(riferimento) = crate::riconosci(&candidato.url) else {
            return Ok(candidato.clone());
        };
        let scheda = self.scheda(&riferimento.id)?;
        Ok(
            lo_stesso_brano(&riferimento.id, &scheda, &candidato.url, Uso::Tenere)
                .unwrap_or_else(|| candidato.clone()),
        )
    }

    /// L'indirizzo da suonare al posto di un FLAC di questo catalogo.
    ///
    /// # Per chi
    ///
    /// Per i brani entrati in libreria con la 2.4.0, che sceglieva il FLAC anche
    /// per l'ascolto. Sono di due specie, e tutte e due si sentivano male: quelli
    /// dei concerti `stream_only`, il cui FLAC è **privato** e risponde `401` a
    /// ogni tentativo, e quelli il cui FLAC si scarica ma vuole il quadruplo
    /// della linea di un MP3 e su un nodo lento si ferma a metà. La riga in
    /// libreria resta com'è — l'indirizzo è la sua identità, e cambiarlo
    /// vorrebbe dire un'altra riga — e al momento di suonarla si prende l'MP3
    /// con la stessa radice, come fa oggi la ricerca (vedi
    /// [`FORMATI_PER_ASCOLTO`]).
    ///
    /// # Quando non cambia niente
    ///
    /// Un indirizzo che non è un FLAC, e uno per cui la scheda non risponde o
    /// non ha un fratello più leggero: si suona quello che c'è, che è quel che
    /// si faceva prima. La risposta si ricorda per la sessione, così un brano
    /// ripreso o risentito non paga di nuovo la scheda.
    #[must_use]
    pub fn da_ascoltare(&self, url: &str) -> String {
        if aether_domain::indirizzo::estensione_da_url(url).as_deref() != Some("flac") {
            return url.to_owned();
        }
        if let Some(noto) = self
            .ascolto
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(url)
        {
            return noto.clone();
        }
        let Some(riferimento) = crate::riconosci(url) else {
            return url.to_owned();
        };
        // Un guasto qui non si racconta: la scheda serve a migliorare la scelta,
        // non a farla, e se non arriva resta l'indirizzo della riga. Non si
        // ricorda nemmeno, perché la prossima volta potrebbe arrivare.
        let Ok(scheda) = self.scheda(&riferimento.id) else {
            return url.to_owned();
        };
        let scelto = lo_stesso_brano(&riferimento.id, &scheda, url, Uso::Ascoltare)
            .map_or_else(|| url.to_owned(), |c| c.url);
        self.ascolto
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(url.to_owned(), scelto.clone());
        scelto
    }

    /// La ricerca avanzata: da una query a un elenco di item.
    fn cerca_item(&self, query: &str, righe: u32) -> Result<Vec<Value>, AppError> {
        let url = format!(
            "{BASE}/advancedsearch.php?q={}&sort%5B%5D={}\
             &fl%5B%5D=identifier&rows={righe}&page=1&output=json",
            percento(query),
            percento(ORDINE)
        );
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: &url,
            intestazioni: &[("accept", "application/json")],
            corpo: Corpo::Niente,
        })?;
        if !risposta.e_andata() {
            // Si passa da `stato_a_errore` e non si scrive qui lo stato: quella
            // sa già distinguere un 503 — l'archivio è sotto carico, si ritenta
            // — da un 400 che dice che la richiesta non andava bene, e allega
            // quel che il servizio ha risposto invece del solo numero.
            let guasto = self.rete.stato_a_errore(&risposta, &url);
            return Err(crate::guasto_di_ricerca(
                "Internet Archive",
                &guasto,
                "la ricerca non ha risposto",
            ));
        }
        let corpo: Value = serde_json::from_slice(&risposta.corpo).map_err(|err| {
            AppError::new(ErrorCode::DownloadBadResponse)
                .with_cause(format!("la ricerca non è JSON: {err}"))
        })?;
        Ok(corpo
            .get("response")
            .and_then(|r| r.get("docs"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// La scheda di un item: i metadati e l'elenco dei file.
    fn scheda(&self, identificativo: &str) -> Result<Value, AppError> {
        let url = format!("{BASE}/metadata/{}", percento(identificativo));
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: &url,
            intestazioni: &[("accept", "application/json")],
            corpo: Corpo::Niente,
        })?;
        if !risposta.e_andata() {
            return Err(self.rete.stato_a_errore(&risposta, &url));
        }
        serde_json::from_slice(&risposta.corpo).map_err(|err| {
            AppError::new(ErrorCode::DownloadBadResponse)
                .with_cause(format!("la scheda di «{identificativo}» non è JSON: {err}"))
        })
    }
}

/// Il testo di un campo, che l'Archive dà come stringa o come elenco.
///
/// `creator` e `collection` sono l'uno o l'altro a seconda dell'item, e non c'è
/// modo di saperlo prima: un item con due autori li dà come array, uno con un
/// autore solo come stringa. Trattarne uno solo dei due casi vuol dire perdere
/// l'autore su metà del catalogo, in silenzio.
fn testo_di(valore: &Value, campo: &str) -> Option<String> {
    match valore.get(campo)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Array(elenco) => {
            let uniti: Vec<&str> = elenco.iter().filter_map(Value::as_str).collect();
            (!uniti.is_empty()).then(|| uniti.join(", "))
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Le collezioni di un item, in minuscolo.
fn collezioni_di(metadati: &Value) -> Vec<String> {
    testo_di(metadati, "collection")
        .map(|c| {
            c.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Un campo che vale «sì» in una delle forme che l'Archive usa.
fn vero(metadati: &Value, campo: &str) -> bool {
    match metadati.get(campo) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
        }
        Some(Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

/// Sotto che licenza sta un item.
///
/// L'ordine dei tentativi è quello dell'autorevolezza: `licenseurl` è una
/// dichiarazione esplicita di chi ha caricato, la collezione è un'inferenza dal
/// patto che quella collezione ha con i suoi artisti. La seconda si usa solo
/// quando la prima tace, e mai per contraddirla.
fn licenza_da(metadati: &Value) -> Licenza {
    if let Some(url) = testo_di(metadati, "licenseurl") {
        let piatto = url.to_ascii_lowercase();
        if piatto.contains("publicdomain") || piatto.contains("/zero/") {
            return Licenza::PubblicoDominio;
        }
        if let Some(coda) = piatto.split("/licenses/").nth(1)
            && let Some(codice) = coda.split('/').next()
            && !codice.is_empty()
        {
            return Licenza::CreativeCommons(codice.to_owned());
        }
    }

    let collezioni = collezioni_di(metadati);
    if collezioni.iter().any(|c| c == "etree") {
        // Il patto del Live Music Archive: gli artisti *trade-friendly*
        // acconsentono allo scambio non commerciale, e a quello soltanto.
        return Licenza::LiberaNonCommerciale;
    }
    if collezioni.iter().any(|c| c == "78rpm") {
        return Licenza::PubblicoDominio;
    }
    Licenza::Sconosciuta
}

/// Che cosa si può fare di un item.
fn disponibilita_da(metadati: &Value, licenza: &Licenza) -> Disponibilita {
    // Il fatto che un indirizzo risponda non è un permesso: un item marcato
    // come consultabile e basta resta tale anche se i byte si prenderebbero.
    let solo_ascolto = vero(metadati, "access-restricted-item")
        || vero(metadati, "access-restricted")
        || vero(metadati, "stream_only");
    Disponibilita::decidi(Fonte::InternetArchive, licenza, !solo_ascolto)
}

/// I formati nell'ordine in cui vincono, per quel che si farà del file.
const fn formati_per(uso: Uso) -> &'static [(&'static str, &'static str); 4] {
    match uso {
        Uso::Tenere => &FORMATI,
        Uso::Ascoltare => &FORMATI_PER_ASCOLTO,
    }
}

/// Il formato di un file, tradotto in estensione, se lo sappiamo aprire.
///
/// Restituisce anche quanto è preferito, perché la scelta fra due file dello
/// stesso brano nello stesso item è esattamente questa: per tenerlo il FLAC
/// originale batte l'MP3 che l'Archive ha generato da lui, per ascoltarlo
/// mentre arriva il contrario. Vedi [`Uso`].
fn formato_di(file: &Value, uso: Uso) -> Option<(usize, &'static str)> {
    let dichiarato = file.get("format")?.as_str()?.to_ascii_lowercase();
    formati_per(uso)
        .iter()
        .enumerate()
        .find(|(_, (nome, _))| dichiarato == *nome)
        .map(|(rango, (_, estensione))| (rango, *estensione))
}

/// La durata di un file in secondi, dalle due forme che l'Archive usa.
///
/// `"312.45"` e `"5:12"` compaiono tutte e due, e nello stesso item: la prima
/// sui derivati che genera lui, la seconda su quel che ha scritto chi ha
/// caricato.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "i secondi sono già vincolati a finiti e non negativi, e un brano che durasse più di 4 miliardi di secondi non è un brano"
)]
fn durata_di(file: &Value) -> Option<u32> {
    let grezza = file.get("length")?.as_str()?.trim();
    if grezza.is_empty() {
        return None;
    }
    if grezza.contains(':') {
        // `MM:SS` oppure `H:MM:SS`: i file «concerto intero in un file»
        // superano l'ora, e scartarli qui toglierebbe proprio a loro il veto
        // di durata che questo campo esiste per applicare.
        let pezzi: Vec<&str> = grezza.split(':').collect();
        let (ore, minuti, secondi) = match pezzi[..] {
            [minuti, secondi] => ("0", minuti, secondi),
            [ore, minuti, secondi] => (ore, minuti, secondi),
            _ => return None,
        };
        let h: u32 = ore.trim().parse().ok()?;
        let m: u32 = minuti.trim().parse().ok()?;
        let s: f64 = secondi.trim().parse().ok()?;
        return Some(
            h.saturating_mul(3600)
                .saturating_add(m.saturating_mul(60))
                .saturating_add(s.round() as u32),
        );
    }
    let secondi: f64 = grezza.parse().ok()?;
    (secondi.is_finite() && secondi >= 0.0).then(|| secondi.round() as u32)
}

/// I brani di un item, come candidati.
///
/// Con `atteso` a `Some`, tiene solo quelli che potrebbero essere quel brano;
/// con `None` li tiene tutti, che è il caso di chi ha incollato il link
/// dell'item e vuole quel che c'è dentro. `uso` decide quale formato vince fra
/// quelli dello stesso brano.
///
/// # I file privati non ci sono
///
/// Un file con `"private": "true"` sta nell'elenco e non si scarica: risponde
/// `401` a chiunque. È la forma dei concerti `stream_only` — tutti i Grateful
/// Dead da tavolo, per esempio — dove i FLAC sono privati e l'MP3 accanto no.
/// Prima di questo controllo il FLAC vinceva sempre, e «Ascolta» su quei
/// concerti rispondeva «il server ha risposto con un errore» a ogni brano.
fn brani_dalla_scheda(
    identificativo: &str,
    scheda: &Value,
    atteso: Option<&BranoEsterno>,
    uso: Uso,
) -> Vec<Candidato> {
    let metadati = scheda.get("metadata").unwrap_or(&Value::Null);
    let licenza = licenza_da(metadati);
    let disponibilita = disponibilita_da(metadati, &licenza);
    let autore = testo_di(metadati, "creator");
    let titolo_item = testo_di(metadati, "title");
    let dal_vivo = collezioni_di(metadati).iter().any(|c| c == "etree");

    let Some(file) = scheda.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };

    // Un brano può stare nello stesso item in FLAC e in MP3. Si tiene il
    // migliore per titolo, non tutti e due: due candidati identici tranne che
    // per il formato farebbero perdere tempo alla scelta e, a pari merito,
    // deciderebbe l'ordine di arrivo, cioè il caso. La chiave comprende anche
    // la radice del nome del file: i derivati dello stesso brano la
    // condividono, mentre due «Jam» diversi dello stesso concerto no — e
    // collassarli per solo titolo farebbe sparire tracce senza che niente lo
    // dica.
    let mut migliori: Vec<((String, String), usize, Candidato)> = Vec::new();

    for voce in file {
        if vero(voce, "private") {
            continue;
        }
        let Some((rango, estensione)) = formato_di(voce, uso) else {
            continue;
        };
        let Some(nome) = voce.get("name").and_then(Value::as_str) else {
            continue;
        };
        let titolo_grezzo = testo_di(voce, "title").unwrap_or_else(|| nome_pulito(nome));
        let (interprete, titolo) = spezza(&titolo_grezzo, autore.as_deref());
        if titolo.trim().is_empty() {
            continue;
        }

        if let Some(atteso) = atteso
            && !somiglia(&titolo, &atteso.title)
        {
            continue;
        }

        let chiave = (fold_text(&titolo), fold_text(&nome_pulito(nome)));
        let natura = if dal_vivo {
            Natura::DalVivo
        } else {
            atteso.map_or(Natura::Studio, |a| natura_dal_titolo(&titolo, &a.title))
        };

        let candidato = Candidato {
            url: format!(
                "{BASE}/download/{}/{}",
                percento(identificativo),
                percento(nome)
            ),
            titolo,
            autore: testo_di(voce, "artist").or_else(|| interprete.clone()),
            // L'Archive non ha una spunta blu. Quel che ha è l'appartenenza a
            // una collezione curata, che è la cosa più vicina a «qualcuno ha
            // guardato»: `etree` accetta solo artisti che hanno acconsentito.
            autore_verificato: dal_vivo,
            durata_sec: durata_di(voce),
            natura,
            fonte: Fonte::InternetArchive,
            licenza: licenza.clone(),
            disponibilita,
            estensione: Some(estensione.to_owned()),
            pagina: Some(format!("{BASE}/details/{}", percento(identificativo))),
            // Il titolo dell'item, che per l'Archive è il concerto o il disco:
            // è l'unica cosa che distingue dieci file dello stesso spettacolo
            // da dieci brani che si chiamano allo stesso modo. Se manca resta
            // l'identificativo, che è brutto da leggere ma è comunque vero.
            album: Some(
                titolo_item
                    .clone()
                    .unwrap_or_else(|| identificativo.to_owned()),
            ),
        };

        match migliori.iter_mut().find(|(c, _, _)| *c == chiave) {
            Some((_, rango_ora, ferma)) if rango < *rango_ora => {
                *rango_ora = rango;
                *ferma = candidato;
            }
            Some(_) => {}
            None => migliori.push((chiave, rango, candidato)),
        }
    }

    migliori.into_iter().map(|(_, _, c)| c).collect()
}

/// Lo stesso brano di `url`, nel formato che vince per `uso`.
///
/// La radice del nome è la chiave: `gd77d1t02.mp3` è il derivato di
/// `gd77d1t02.flac`, e l'Archive lo nomina così apposta. Fra i file con la
/// stessa radice vince il primo dell'ordine di `uso` — per tenerlo l'originale
/// senza perdita, per ascoltarlo l'MP3 — e mai un file privato. `None` se l'item
/// non ne ha nessuno: il brano è sparito, o è stato rinominato.
fn lo_stesso_brano(identificativo: &str, scheda: &Value, url: &str, uso: Uso) -> Option<Candidato> {
    let radice = radice_del_file(url);
    let ordine = formati_per(uso);
    brani_dalla_scheda(identificativo, scheda, None, uso)
        .into_iter()
        .filter(|c| radice_del_file(&c.url) == radice)
        .min_by_key(|c| {
            ordine
                .iter()
                .position(|(_, estensione)| c.estensione.as_deref() == Some(*estensione))
                .unwrap_or(usize::MAX)
        })
}

/// L'ultimo pezzo di un indirizzo, senza estensione.
fn radice_del_file(url: &str) -> &str {
    let ultimo = url.rsplit('/').next().unwrap_or(url);
    ultimo.rsplit_once('.').map_or(ultimo, |(radice, _)| radice)
}

/// Il nome del file senza cartella e senza estensione, come ripiego di un titolo.
fn nome_pulito(nome: &str) -> String {
    let ultimo = nome.rsplit('/').next().unwrap_or(nome);
    ultimo
        .rsplit_once('.')
        .map_or(ultimo, |(radice, _)| radice)
        .replace(['_', '-'], " ")
        .trim()
        .to_owned()
}

/// I due titoli potrebbero essere lo stesso brano.
///
/// Deliberatamente largo: qui non si decide niente, si sfoltisce. La decisione è
/// di [`aether_domain::scelta::scegli_candidato`], che guarda anche la durata, e
/// togliere qui un candidato che quella avrebbe scelto è un brano perduto in
/// silenzio.
fn somiglia(titolo: &str, atteso: &str) -> bool {
    let a = fold_text(titolo);
    let b = fold_text(atteso);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// Fra i candidati ce n'è uno che chiude la questione.
///
/// Serve a smettere di aprire schede: ogni scheda è una richiesta a un servizio
/// pubblico gratuito, e continuare a chiederne dopo aver già trovato il brano
/// giusto è tempo di chi aspetta e banda di chi ospita, per niente.
fn primo_sicuro(candidati: &[Candidato], brano: &BranoEsterno) -> bool {
    let Some(attesa) = brano.duration_ms else {
        // Senza una durata da confrontare non si può essere sicuri di niente:
        // si aprono tutte le schede previste e decide chi sceglie.
        return false;
    };
    candidati.iter().any(|c| {
        c.durata_sec
            .is_some_and(|sec| attesa.abs_diff(u64::from(sec).saturating_mul(1000)) <= 3_000)
            && fold_text(&c.titolo) == fold_text(&brano.title)
    })
}

/// Da un candidato al brano che finirà in libreria.
///
/// `album` è il titolo dell'item: senza, ogni concerto finirebbe in `Singoli/`
/// mescolato a tutto il resto preso dallo stesso artista, e il raggruppamento
/// per album non avrebbe niente su cui lavorare.
fn brano_da_candidato(
    candidato: Candidato,
    autore: Option<&str>,
    album: Option<&str>,
    posizione: usize,
) -> BranoEsterno {
    BranoEsterno {
        title: candidato.titolo,
        artist: candidato.autore.or_else(|| autore.map(ToOwned::to_owned)),
        album_artist: autore.map(ToOwned::to_owned),
        album: album.map(ToOwned::to_owned),
        track_number: u32::try_from(posizione.saturating_add(1)).ok(),
        duration_ms: candidato
            .durata_sec
            .map(|s| u64::from(s).saturating_mul(1000)),
        fonte_url: Some(candidato.url),
        licenza: candidato.licenza,
        disponibilita: candidato.disponibilita,
        pagina_url: candidato.pagina,
        ..BranoEsterno::default()
    }
}

/// La domanda che si fa all'indice degli item.
///
/// Tre pezzi, e ognuno risponde a una domanda diversa: [`GENERI`] dice **che
/// cosa** è musica per l'Archive, [`COLLEZIONI`] dice quale musica abbiamo il
/// permesso di toccare, e il terzo è quel che si sta cercando.
///
/// È una funzione e non un `format!` in mezzo a `cerca_con` per una ragione
/// sola: così una prova può leggerla senza rete. Il guasto che ha reso
/// necessario `GENERI` stava esattamente qui, era una parola, ed è rimasto
/// invisibile finché nessuno ha potuto guardare la stringa.
fn query_item(testo: &str, esatta: bool) -> String {
    format!(
        "{GENERI} AND {COLLEZIONI} AND ({})",
        if esatta { frase(testo) } else { termini(testo) }
    )
}

/// La frase da mettere nella query, con le virgolette al posto giusto.
///
/// Senza virgolette, «Karma Police» cerca i due termini separatamente e trova
/// ogni item che contenga «police». Con le virgolette cerca la frase.
fn frase(testo: &str) -> String {
    let ripulito: String = testo
        .chars()
        .map(|c| if c == '"' || c == '\\' { ' ' } else { c })
        .collect();
    format!("\"{}\"", ripulito.trim())
}

/// Gli stessi termini, sciolti: senza le virgolette che li legano in una frase.
///
/// La sorella di [`frase`], e la differenza è tutta in chi ha scritto il testo.
/// Un titolo che arriva da una playlist è il nome di una cosa, e va cercato
/// come tale; una frase digitata in una casella descrive quel che si vorrebbe
/// sentire, e legarla intera vorrebbe dire chiedere all'archivio un item che
/// si chiami così.
///
/// La ripulitura è la stessa, e per la stessa ragione: una virgoletta o una
/// barra rovesciata nel mezzo romperebbero la query invece di restringerla.
fn termini(testo: &str) -> String {
    testo
        .chars()
        .map(|c| if c == '"' || c == '\\' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod prove {
    #[test]
    fn il_prelievo_non_usa_la_rete_delle_domande() {
        // Due reti e non una: la domanda ha una scadenza complessiva perché
        // un servizio muto è un servizio rotto, il prelievo no perché un file
        // grosso su una linea normale non è rotto — è grosso.
        let archivio = super::ArchivioOrg::nuovo();
        assert_eq!(
            archivio.rete().scadenza_complessiva(),
            Some(super::SCADENZA)
        );
        assert_eq!(archivio.rete_prelievo().scadenza_complessiva(), None);
    }

    use super::*;

    fn scheda(json: &str) -> Value {
        serde_json::from_str(json).unwrap_or(Value::Null)
    }

    #[test]
    fn un_campo_e_stringa_o_elenco_e_va_letto_in_tutti_e_due_i_modi() {
        let uno = scheda(r#"{"creator":"Grateful Dead"}"#);
        let due = scheda(r#"{"creator":["Jerry Garcia","Bob Weir"]}"#);
        assert_eq!(testo_di(&uno, "creator").as_deref(), Some("Grateful Dead"));
        assert_eq!(
            testo_di(&due, "creator").as_deref(),
            Some("Jerry Garcia, Bob Weir")
        );
        assert_eq!(testo_di(&uno, "assente"), None);
    }

    #[test]
    fn la_licenza_dichiarata_batte_quella_dedotta_dalla_collezione() {
        let m = scheda(
            r#"{"collection":["etree"],
                "licenseurl":"https://creativecommons.org/licenses/by-nc-sa/3.0/"}"#,
        );
        assert_eq!(
            licenza_da(&m),
            Licenza::CreativeCommons("by-nc-sa".to_owned())
        );
    }

    #[test]
    fn senza_licenza_dichiarata_il_patto_dei_tapers_vale_come_licenza() {
        let m = scheda(r#"{"collection":["etree","GratefulDead"]}"#);
        assert_eq!(licenza_da(&m), Licenza::LiberaNonCommerciale);
    }

    #[test]
    fn il_pubblico_dominio_si_riconosce_in_tutte_e_due_le_forme() {
        assert_eq!(
            licenza_da(&scheda(
                r#"{"licenseurl":"http://creativecommons.org/publicdomain/mark/1.0/"}"#
            )),
            Licenza::PubblicoDominio
        );
        assert_eq!(
            licenza_da(&scheda(
                r#"{"licenseurl":"https://creativecommons.org/publicdomain/zero/1.0/"}"#
            )),
            Licenza::PubblicoDominio
        );
        assert_eq!(
            licenza_da(&scheda(r#"{"collection":"78rpm"}"#)),
            Licenza::PubblicoDominio
        );
    }

    #[test]
    fn una_licenza_sconosciuta_non_diventa_scaricabile() {
        let m = scheda(r#"{"collection":["opensource_audio"]}"#);
        assert_eq!(licenza_da(&m), Licenza::Sconosciuta);
        assert_eq!(
            disponibilita_da(&m, &licenza_da(&m)),
            Disponibilita::SoloAscolto
        );
    }

    #[test]
    fn un_item_consultabile_e_basta_resta_solo_ascolto() {
        // Il fatto che i byte si prenderebbero non è un permesso.
        let m = scheda(r#"{"collection":["etree"],"access-restricted-item":"true"}"#);
        assert_eq!(
            disponibilita_da(&m, &licenza_da(&m)),
            Disponibilita::SoloAscolto
        );
        let libero = scheda(r#"{"collection":["etree"]}"#);
        assert_eq!(
            disponibilita_da(&libero, &licenza_da(&libero)),
            Disponibilita::Scaricabile
        );
    }

    #[test]
    fn la_durata_si_legge_nelle_due_forme_che_larchive_usa() {
        assert_eq!(durata_di(&scheda(r#"{"length":"312.45"}"#)), Some(312));
        assert_eq!(durata_di(&scheda(r#"{"length":"5:12"}"#)), Some(312));
        assert_eq!(durata_di(&scheda(r#"{"length":""}"#)), None);
        assert_eq!(durata_di(&scheda("{}")), None);
    }

    #[test]
    fn la_durata_in_ore_dei_concerti_interi_si_legge() {
        // I file «concerto intero in un file» superano l'ora: senza questa
        // forma il veto di durata non si applicherebbe proprio a loro.
        assert_eq!(durata_di(&scheda(r#"{"length":"1:02:33"}"#)), Some(3753));
        assert_eq!(durata_di(&scheda(r#"{"length":"1:00:00"}"#)), Some(3600));
        assert_eq!(durata_di(&scheda(r#"{"length":"1:2:3:4"}"#)), None);
    }

    #[test]
    fn due_tracce_omonime_nello_stesso_item_restano_due() {
        // «Jam» nel primo set e «Jam» nel secondo sono due brani: la deduplica
        // dei formati non deve mangiarsene uno.
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"gd77d1t05.flac","format":"Flac","title":"Jam","length":"300"},
                {"name":"gd77d2t03.flac","format":"Flac","title":"Jam","length":"420"}
              ]
            }"#,
        );
        assert_eq!(brani_dalla_scheda("gd77", &s, None, Uso::Tenere).len(), 2);
    }

    #[test]
    fn i_brani_di_un_item_portano_lalbum() {
        // Senza album finirebbero tutti in `Singoli/`, mescolati a tutto il
        // resto preso dallo stesso artista.
        let brano = brano_da_candidato(
            Candidato {
                titolo: "Sugaree".to_owned(),
                ..Candidato::default()
            },
            Some("Grateful Dead"),
            Some("Barton Hall 1977-05-08"),
            0,
        );
        assert_eq!(brano.album.as_deref(), Some("Barton Hall 1977-05-08"));
    }

    #[test]
    fn dei_due_formati_dello_stesso_brano_resta_il_migliore() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"gd77t01.mp3","format":"VBR MP3","title":"Sugaree","length":"300"},
                {"name":"gd77t01.flac","format":"Flac","title":"Sugaree","length":"300"}
              ]
            }"#,
        );
        let trovati = brani_dalla_scheda("gd77", &s, None, Uso::Tenere);
        assert_eq!(trovati.len(), 1);
        assert!(trovati.first().is_some_and(|c| c.url.ends_with(".flac")));
        assert_eq!(
            trovati.first().and_then(|c| c.estensione.clone()),
            Some("flac".to_owned())
        );
    }

    #[test]
    fn per_ascoltare_dei_due_formati_vince_il_piu_leggero() {
        // Quel che si suona mentre arriva: il FLAC vuole il quadruplo della
        // linea, e sul nodo dell'Archive misurato non c'era.
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"gd77t01.flac","format":"Flac","title":"Sugaree","length":"300"},
                {"name":"gd77t01.mp3","format":"VBR MP3","title":"Sugaree","length":"300"}
              ]
            }"#,
        );
        let trovati = brani_dalla_scheda("gd77", &s, None, Uso::Ascoltare);
        assert_eq!(trovati.len(), 1);
        assert!(
            trovati.first().is_some_and(|c| c.url.ends_with(".mp3")),
            "{:?}",
            trovati.first().map(|c| &c.url)
        );
        assert_eq!(
            trovati.first().and_then(|c| c.estensione.clone()),
            Some("mp3".to_owned())
        );
    }

    #[test]
    fn un_file_privato_non_diventa_un_candidato() {
        // La forma dei concerti `stream_only`, presa dal 5 maggio 1977: i FLAC
        // sono privati e rispondono 401, l'MP3 accanto si ascolta. Prima vinceva
        // il FLAC anche per chi voleva tenerlo, e ogni «Ascolta» era un errore.
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree","stream_only"],
                           "creator":"Grateful Dead","stream_only":"true"},
              "files": [
                {"name":"gd77-05-05d1t02.flac","format":"Flac","title":"Promised Land",
                 "length":"260","private":"true"},
                {"name":"gd77-05-05d1t02.mp3","format":"VBR MP3","title":"Promised Land",
                 "length":"260"}
              ]
            }"#,
        );
        for uso in [Uso::Tenere, Uso::Ascoltare] {
            let trovati = brani_dalla_scheda("gd77", &s, None, uso);
            assert_eq!(trovati.len(), 1, "{uso:?}");
            assert!(
                trovati.first().is_some_and(|c| c.url.ends_with(".mp3")),
                "{uso:?}: ha scelto un file privato"
            );
        }
        // E un item fatto solo di file privati non dà niente, invece di dare
        // dei brani che non suoneranno.
        let tutto_privato = scheda(
            r#"{
              "metadata": {"identifier":"x","collection":["etree"]},
              "files": [{"name":"t01.flac","format":"Flac","title":"Jam","private":true}]
            }"#,
        );
        assert!(brani_dalla_scheda("x", &tutto_privato, None, Uso::Ascoltare).is_empty());
    }

    #[test]
    fn da_quel_che_si_e_ascoltato_si_tiene_loriginale() {
        // «Tieni una copia» parte dall'MP3 scelto per l'ascolto e deve portare
        // giù il FLAC dello stesso brano: stessa radice del nome, non stesso
        // titolo, perché due «Jam» dello stesso concerto sono due brani.
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"gd77d1t05.flac","format":"Flac","title":"Jam","length":"300"},
                {"name":"gd77d1t05.mp3","format":"VBR MP3","title":"Jam","length":"300"},
                {"name":"gd77d2t03.flac","format":"Flac","title":"Jam","length":"420"},
                {"name":"gd77d2t03.mp3","format":"VBR MP3","title":"Jam","length":"420"}
              ]
            }"#,
        );
        let ascoltato = "https://archive.org/download/gd77/gd77d2t03.mp3";
        let da_tenere = lo_stesso_brano("gd77", &s, ascoltato, Uso::Tenere);
        assert_eq!(
            da_tenere.as_ref().map(|c| c.url.as_str()),
            Some("https://archive.org/download/gd77/gd77d2t03.flac")
        );
        assert_eq!(
            da_tenere.and_then(|c| c.estensione),
            Some("flac".to_owned())
        );
        // Un file che nell'item non c'è più non diventa un altro brano.
        assert!(
            lo_stesso_brano(
                "gd77",
                &s,
                "https://archive.org/download/gd77/sparito.mp3",
                Uso::Tenere
            )
            .is_none()
        );
    }

    #[test]
    fn un_flac_gia_in_libreria_si_ascolta_dal_suo_mp3() {
        // Le righe entrate con la 2.4.0 portano il FLAC. Il caso peggiore è il
        // concerto `stream_only`, dove il FLAC è privato e risponde 401: al
        // momento di suonarlo si prende l'MP3 con la stessa radice.
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"stream_only":"true"},
              "files": [
                {"name":"gd77-05-05d1t02.flac","format":"Flac","title":"Promised Land",
                 "private":"true"},
                {"name":"gd77-05-05d1t02.mp3","format":"VBR MP3","title":"Promised Land"},
                {"name":"gd77-05-05d1t03.mp3","format":"VBR MP3","title":"Sugaree"}
              ]
            }"#,
        );
        let in_libreria = "https://archive.org/download/gd77/gd77-05-05d1t02.flac";
        assert_eq!(
            lo_stesso_brano("gd77", &s, in_libreria, Uso::Ascoltare).map(|c| c.url),
            Some("https://archive.org/download/gd77/gd77-05-05d1t02.mp3".to_owned())
        );
    }

    #[test]
    fn solo_un_flac_va_a_cercare_un_fratello() {
        // Un MP3 si suona com'è, senza chiedere niente all'Archive: la prova
        // gira senza rete, e se partisse una richiesta qui tornerebbe un altro
        // indirizzo o un errore.
        let archivio = ArchivioOrg::nuovo();
        let mp3 = "https://archive.org/download/ItsAllAGame/02%20I%27m%20All%20Good.mp3";
        assert_eq!(archivio.da_ascoltare(mp3), mp3);
    }

    #[test]
    fn i_file_che_non_sappiamo_aprire_non_diventano_candidati() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"x","collection":["etree"]},
              "files": [
                {"name":"cover.jpg","format":"JPEG","title":"copertina"},
                {"name":"x_meta.xml","format":"Metadata"},
                {"name":"t01.shn","format":"Shorten","title":"Sugaree"}
              ]
            }"#,
        );
        assert!(brani_dalla_scheda("x", &s, None, Uso::Ascoltare).is_empty());
    }

    #[test]
    fn dentro_una_collezione_di_concerti_tutto_e_dal_vivo() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [{"name":"t01.flac","format":"Flac","title":"Sugaree","length":"300"}]
            }"#,
        );
        let trovati = brani_dalla_scheda("gd77", &s, None, Uso::Ascoltare);
        assert_eq!(trovati.first().map(|c| c.natura), Some(Natura::DalVivo));
    }

    #[test]
    fn cercando_un_brano_gli_altri_dellitem_non_arrivano() {
        let s = scheda(
            r#"{
              "metadata": {"identifier":"gd77","collection":["etree"],"creator":"Grateful Dead"},
              "files": [
                {"name":"t01.flac","format":"Flac","title":"Sugaree","length":"300"},
                {"name":"t02.flac","format":"Flac","title":"Jack Straw","length":"280"}
              ]
            }"#,
        );
        let atteso = BranoEsterno {
            title: "Sugaree".to_owned(),
            ..BranoEsterno::default()
        };
        let trovati = brani_dalla_scheda("gd77", &s, Some(&atteso), Uso::Tenere);
        assert_eq!(trovati.len(), 1);
        assert_eq!(trovati.first().map(|c| c.titolo.as_str()), Some("Sugaree"));
    }

    #[test]
    fn senza_titolo_si_ripiega_sul_nome_del_file() {
        assert_eq!(
            nome_pulito("dir/gd1977-05-08_d1t04.flac"),
            "gd1977 05 08 d1t04"
        );
        assert_eq!(nome_pulito("Sugaree.mp3"), "Sugaree");
    }

    #[test]
    fn la_frase_non_puo_rompere_la_query() {
        assert_eq!(frase(r#"Say "Hello""#), "\"Say  Hello\"");
        assert_eq!(frase("Karma Police"), "\"Karma Police\"");
    }

    #[test]
    fn smettere_presto_vuole_una_durata_da_confrontare() {
        let senza = BranoEsterno {
            title: "Sugaree".to_owned(),
            ..BranoEsterno::default()
        };
        let con = BranoEsterno {
            duration_ms: Some(300_000),
            ..senza.clone()
        };
        let candidati = [Candidato {
            titolo: "Sugaree".to_owned(),
            durata_sec: Some(300),
            ..Candidato::default()
        }];
        assert!(!primo_sicuro(&candidati, &senza));
        assert!(primo_sicuro(&candidati, &con));
    }
    /// La ricerca chiede tutti e due i `mediatype` della musica.
    ///
    /// # Cos'è questa prova
    ///
    /// Il guardiano di una parola. Fino alla 2.4.0 la query diceva
    /// `mediatype:audio`, e quella parola da sola teneva fuori il Live Music
    /// Archive intero — duecentomila concerti che all'Archive sono
    /// `mediatype:etree` — mentre la ricerca continuava a rispondere con
    /// qualcosa, cioè senza mai sembrare rotta. Chi cercava i Grateful Dead
    /// riceveva quattro serate di gruppi che li suonano.
    ///
    /// Una prova che guarda la stringa è l'unica possibile senza rete, ed è
    /// quella che mancava: il guasto stava in un `format!` in mezzo a una
    /// funzione che parla con Internet, dove nessuna prova poteva arrivare.
    #[test]
    fn la_ricerca_non_dimentica_i_concerti() {
        let q = query_item("Grateful Dead", true);
        assert!(
            q.contains("mediatype:etree"),
            "senza `etree` il Live Music Archive non esiste: {q}"
        );
        assert!(
            q.contains("mediatype:audio"),
            "e senza `audio` sparisce il resto: {q}"
        );
        assert!(
            q.contains("collection:(etree OR netlabels OR audio_music OR 78rpm)"),
            "il cancello delle collezioni non si allarga con i generi: {q}"
        );
        assert!(
            q.contains("\"Grateful Dead\""),
            "la frase resta legata: {q}"
        );
    }

    /// La passata larga scioglie i termini e tiene tutto il resto.
    #[test]
    fn la_passata_larga_cambia_solo_la_frase() {
        let larga = query_item("lo-fi piano", false);
        assert!(!larga.contains('"'), "senza virgolette: {larga}");
        assert!(larga.contains("mediatype:etree"), "{larga}");
    }
}
