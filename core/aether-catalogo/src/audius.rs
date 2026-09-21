//! Audius: cercare che cosa c'è, e prenderne i byte quando l'artista lo
//! consente.
//!
//! # Perché entra senza toccare il motore audio
//!
//! Perché [`Fonte::puo_consegnare`] dice `true` per Audius, e non è una
//! distrazione: su Audius **è l'artista** a decidere, brano per brano, se il
//! suo pezzo si possa scaricare o solo ascoltare. Quando dice di sì, il brano
//! percorre la stessa strada di un item dell'Internet Archive — `preleva`
//! scrive un file, la scansione lo porta in libreria, e da lì è un brano come
//! gli altri.
//!
//! Quando dice di no, il brano resta [`Disponibilita::SoloAscolto`] e finisce
//! nell'elenco con la sua pastiglia. Riprodurlo in streaming è un'altra
//! faccenda, e non la fa questo modulo.
//!
//! # I due interruttori, e perché contano tutti e due
//!
//! `Disponibilita::decidi` moltiplica quel che la **fonte** consente per quel
//! che la **licenza** consente. Su Audius la fonte consente in generale, quindi
//! il peso cade tutto sulla seconda metà, e la seconda metà è un campo di testo
//! che scrive l'artista. Un brano con `license` assente o incomprensibile vale
//! [`Licenza::Sconosciuta`], che non permette la copia: è il verso giusto in
//! cui sbagliare, e vale la pena dirlo perché la tentazione opposta —
//! «l'indirizzo risponde, quindi si può» — è precisamente l'errore che ha
//! reso indistribuibile la versione precedente di questo programma.
//!
//! Sopra la licenza c'è un terzo veto, che la licenza non conosce: i brani
//! **con cancello**. Audius permette di chiudere lo streaming o lo scarico
//! dietro un seguito, un acquisto o il possesso di un gettone
//! (`is_stream_gated`, `is_download_gated`). Un brano col cancello sullo
//! streaming non entra nemmeno nell'elenco — non lo si potrebbe sentire — e uno
//! col cancello sullo scarico scende a solo ascolto qualunque cosa dica la sua
//! licenza. Il permesso di un catalogo non è la somma delle sue licenze: è
//! l'intersezione di tutti i suoi no.
//!
//! # Il nodo, e perché si sceglie una volta sola
//!
//! Audius non ha un server: ha una rete di *discovery node* equivalenti, e
//! `https://api.audius.co` restituisce l'elenco di quelli in salute. Se ne
//! prende uno e lo si tiene per la sessione, come [`Rete`] tiene la sua riserva
//! di connessioni: risceglierlo a ogni richiesta vorrebbe dire una stretta di
//! mano TLS nuova ogni volta, contro macchine che qualcun altro paga.
//!
//! Se il nodo scelto smette di rispondere lo si dimentica e se ne prende un
//! altro, **una volta**. Non è un ciclo di tentativi: se il secondo nodo tace
//! anche lui, quel che non funziona è la rete di chi sta ascoltando, e
//! insistere su un elenco di venti nodi vorrebbe dire venti timeout in fila
//! prima di poterglielo dire.
//!
//! # `app_name`, che non è facoltativo
//!
//! Ogni richiesta lo porta. È quel che i loro termini chiedono a chi usa l'API,
//! ed è anche l'unica cosa che Aether dice di sé a qualcuno: nessun
//! identificativo, nessun conteggio, nessun numero di serie. Vedi `PRIVACY.md`
//! § 1.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{
    BranoEsterno, ContenutoEsterno, Disponibilita, Fonte, GenereContenuto, Licenza, Livello,
};
use aether_domain::scelta::{Candidato, Natura, natura_dal_titolo, query_larga, query_stretta};
use aether_net::{Corpo, Metodo, Rete, Richiesta, percento};
use serde_json::Value;

use crate::riferimento::Riferimento;

/// Dove si chiede l'elenco dei nodi in salute.
const ELENCO_NODI: &str = "https://api.audius.co";

/// Come Aether si presenta all'API.
///
/// Non è un identificativo: è lo stesso testo per ogni copia del programma nel
/// mondo, e serve al catalogo per sapere quali applicazioni lo interrogano.
const NOME_APP: &str = "Aether";

/// Quanto si aspetta una risposta.
///
/// La stessa dell'Internet Archive: sono due servizi pubblici e gratuiti, e
/// due pazienze diverse verso due servizi con lo stesso ruolo sarebbero due
/// numeri da tenere allineati a mano.
const SCADENZA: Duration = Duration::from_secs(30);

/// Quanti risultati chiede una ricerca.
///
/// Otto come l'Internet Archive, ma qui costano molto meno: là ogni risultato
/// era un item da aprire con una seconda richiesta, qui la ricerca restituisce
/// già i brani con tutto quel che serve per giudicarli.
const RISULTATI: usize = 8;

/// Quanti brani si leggono al massimo da una playlist o da un artista.
///
/// Duecento è più di qualunque album e più della quasi totalità delle playlist.
/// Il tetto esiste perché un elenco senza fondo, letto tutto, è un modo di
/// riempire la memoria con la libreria di qualcun altro; quando morde, la
/// differenza fra `declared_total` e i brani letti lo dice all'utente invece di
/// nasconderlo.
const TETTO_BRANI: usize = 200;

/// Un client di Audius.
///
/// `Clone` come [`crate::ArchivioOrg`], e il nodo scelto è condiviso fra le
/// copie: due cloni che scegliessero due nodi diversi sarebbero due strette di
/// mano invece di una, e — peggio — due idee di quale sia la rete.
#[derive(Debug, Clone)]
pub struct Audius {
    rete: Rete,
    /// La rete dei prelievi, che ha una scadenza sua: vedi
    /// [`Rete::per_prelievo`].
    prelievo: Rete,
    nodo: Arc<Mutex<Option<String>>>,
}

impl Default for Audius {
    fn default() -> Self {
        Self::nuovo()
    }
}

impl Audius {
    /// Un client con la scadenza di serie.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            rete: Rete::nuova("audius.co", SCADENZA),
            prelievo: Rete::per_prelievo("audius.co"),
            nodo: Arc::new(Mutex::new(None)),
        }
    }

    /// La rete che usa per **chiedere**: ricerche, schede, elenchi.
    ///
    /// Le richieste piccole e la lettura a finestre di un flusso, che piccole
    /// sono anche loro. Per portare giù un file intero c'è
    /// [`Self::rete_prelievo`], e la differenza fra le due è la scadenza.
    #[must_use]
    pub const fn rete(&self) -> &Rete {
        &self.rete
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
    /// Una ricerca vera e piccolissima. Serve alla diagnostica, che deve poter
    /// distinguere «non l'ho trovato» da «non ci arrivo»: sono due frasi
    /// diverse per chi guarda, e senza una richiesta vera si può solo indovinare
    /// quale delle due sia.
    pub fn risponde(&self) -> bool {
        self.chiedi("/v1/tracks/search", &[("query", "a"), ("limit", "1")])
            .is_ok()
    }

    /// I brani di questo catalogo che potrebbero essere quello chiesto.
    ///
    /// Prima con la stringa stretta, e solo se non ha dato niente con quella
    /// larga: la stessa scala dell'Internet Archive, e per la stessa ragione —
    /// la seconda richiesta si paga e quasi sempre non serve.
    ///
    /// # Errori
    ///
    /// `download.externalSearchFailed` quando la ricerca non risponde, `net.*`
    /// per i guasti di trasporto.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        if annullato() {
            return Ok(Vec::new());
        }
        let mut trovati = self.cerca_con(&query_stretta(brano), Some(&brano.title))?;
        if trovati.is_empty()
            && !annullato()
            && let Some(larga) = query_larga(brano)
        {
            trovati = self.cerca_con(&larga, Some(&brano.title))?;
        }
        Ok(trovati)
    }

    /// I brani di questo catalogo che rispondono a una frase.
    ///
    /// Non c'è un brano da ritrovare: c'è quel che qualcuno ha scritto in una
    /// casella. È la stessa passata di [`Self::cerca`] senza il titolo atteso,
    /// perché qui non c'è niente a cui somigliare. I cancelli restano dove
    /// stanno: un brano con il cancello sullo streaming non esce da qui nemmeno
    /// adesso, e la disponibilità la decide sempre `Disponibilita::decidi`.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::cerca`].
    pub fn cerca_libera(&self, testo: &str) -> Result<Vec<Candidato>, AppError> {
        self.cerca_con(testo, None)
    }

    /// Una passata di ricerca con una stringa sola.
    fn cerca_con(&self, testo: &str, atteso: Option<&str>) -> Result<Vec<Candidato>, AppError> {
        if testo.trim().is_empty() {
            return Ok(Vec::new());
        }
        let limite = RISULTATI.to_string();
        let corpo = self
            .chiedi("/v1/tracks/search", &[("query", testo), ("limit", &limite)])
            .map_err(|err| {
                // La ricerca che non risponde ha un codice suo, e non è lo
                // stesso di un brano che non c'è: la coda tratta i due casi in
                // modo diverso — uno si ritenta, l'altro no.
                AppError::new(ErrorCode::DownloadExternalSearchFailed).with_cause(
                    err.cause()
                        .unwrap_or("la ricerca non ha risposto")
                        .to_owned(),
                )
            })?;

        Ok(elenco(&corpo)
            .iter()
            .filter_map(|t| candidato_da(t, atteso))
            .collect())
    }

    /// Un brano, un album o un artista, come contenuto importabile.
    ///
    /// È la strada di chi incolla un link. Non si cerca niente: si legge quel
    /// che c'è, nell'ordine in cui sta lassù.
    ///
    /// # Errori
    ///
    /// `catalogo.notPublic` se il link non porta a niente, `catalogo.resolveFailed`
    /// se ci porta ma non se ne cava niente di ascoltabile.
    pub fn risolvi(&self, riferimento: &Riferimento) -> Result<ContenutoEsterno, AppError> {
        let indirizzo = riferimento.url_pubblico();
        let risolto = self.chiedi("/v1/resolve", &[("url", &indirizzo)])?;
        let dato = risolto.get("data").unwrap_or(&Value::Null);
        if dato.is_null() {
            return Err(AppError::new(ErrorCode::CatalogoNotPublic)
                .with_cause(format!("Audius non conosce «{indirizzo}»")));
        }

        match riferimento.genere {
            GenereContenuto::Track => self.contenuto_da_brano(dato, riferimento),
            GenereContenuto::Artist => self.contenuto_da_artista(dato, riferimento),
            GenereContenuto::Album | GenereContenuto::Playlist | GenereContenuto::Collezione => {
                self.contenuto_da_playlist(dato, riferimento)
            }
        }
    }

    /// Un brano solo, come contenuto di un elemento.
    fn contenuto_da_brano(
        &self,
        dato: &Value,
        riferimento: &Riferimento,
    ) -> Result<ContenutoEsterno, AppError> {
        // Un array anche per il caso singolo: `resolve` restituisce a volte
        // l'oggetto e a volte un elenco di uno, e distinguere i due casi qui è
        // meno fragile che sperare che non cambi.
        let primo = dato
            .as_array()
            .and_then(|a| a.first())
            .unwrap_or(dato)
            .clone();
        let Some(candidato) = candidato_da(&primo, None) else {
            return Err(AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(
                "il brano c'è ma non è ascoltabile: cancello sullo streaming, o non riproducibile"
                    .to_owned(),
            ));
        };
        let autore = candidato.autore.clone();
        let brano = brano_da(candidato, 0);
        Ok(ContenutoEsterno {
            fonte: Fonte::Audius,
            kind: GenereContenuto::Track,
            id: riferimento.id.clone(),
            title: brano.title.clone(),
            author: autore,
            // Dalla risposta del catalogo, non dal brano: `brano_da` non porta
            // mai una copertina, e `artwork` sta sull'oggetto della traccia.
            cover_url: copertina(&primo),
            declared_total: Some(1),
            tracks: vec![brano],
            source: Livello::Audius,
        })
    }

    /// Una playlist o un album, coi suoi brani nell'ordine di lassù.
    fn contenuto_da_playlist(
        &self,
        dato: &Value,
        riferimento: &Riferimento,
    ) -> Result<ContenutoEsterno, AppError> {
        let testa = dato.as_array().and_then(|a| a.first()).unwrap_or(dato);
        let Some(id) = testo(testa, "id") else {
            return Err(AppError::new(ErrorCode::CatalogoNotPublic)
                .with_cause("la playlist non porta un identificativo".to_owned()));
        };
        let corpo = self.chiedi(&format!("/v1/playlists/{}/tracks", percento(&id)), &[])?;
        let brani = elenco(&corpo);
        let dichiarati = testa
            .get("track_count")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok());

        let tracce = tracce_da(&brani);
        if tracce.is_empty() {
            return Err(
                AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(format!(
                    "«{}» non ha nessun brano ascoltabile",
                    riferimento.url_pubblico()
                )),
            );
        }
        Ok(ContenutoEsterno {
            fonte: Fonte::Audius,
            kind: if testa
                .get("is_album")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                GenereContenuto::Album
            } else {
                GenereContenuto::Playlist
            },
            id: riferimento.id.clone(),
            title: testo(testa, "playlist_name").unwrap_or_else(|| riferimento.id.clone()),
            author: testa.get("user").and_then(|u| testo(u, "name")),
            cover_url: copertina(testa),
            declared_total: dichiarati,
            tracks: tracce,
            source: Livello::Audius,
        })
    }

    /// La discografia di un artista, per quanto ne sta nel tetto.
    fn contenuto_da_artista(
        &self,
        dato: &Value,
        riferimento: &Riferimento,
    ) -> Result<ContenutoEsterno, AppError> {
        let testa = dato.as_array().and_then(|a| a.first()).unwrap_or(dato);
        let Some(id) = testo(testa, "id") else {
            return Err(AppError::new(ErrorCode::CatalogoNotPublic)
                .with_cause("l'artista non porta un identificativo".to_owned()));
        };
        let limite = TETTO_BRANI.to_string();
        let corpo = self.chiedi(
            &format!("/v1/users/{}/tracks", percento(&id)),
            &[("limit", &limite)],
        )?;
        let tracce = tracce_da(&elenco(&corpo));
        if tracce.is_empty() {
            return Err(
                AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(format!(
                    "«{}» non ha nessun brano ascoltabile",
                    riferimento.url_pubblico()
                )),
            );
        }
        Ok(ContenutoEsterno {
            fonte: Fonte::Audius,
            kind: GenereContenuto::Artist,
            id: riferimento.id.clone(),
            title: testo(testa, "name").unwrap_or_else(|| riferimento.id.clone()),
            author: testo(testa, "name"),
            cover_url: copertina(testa),
            // Quanti ne abbia in tutto non lo si sa: `track_count` su un utente
            // conta anche quelli che non sono arrivati qui. Dichiarare un totale
            // che non si è potuto verificare farebbe comparire un ammanco dove
            // c'è solo un tetto.
            declared_total: None,
            tracks: tracce,
            source: Livello::Audius,
        })
    }

    /// Da un candidato conservato a uno pronto da andare a prendere.
    ///
    /// Rimette il nodo davanti al percorso e riporta fuori l'estensione che
    /// [`indirizzo`] ci aveva messo dentro. È il passo che permette a
    /// `desiderati.fonte_url` di non invecchiare: quel che sta in tabella non
    /// nomina nessuna macchina, e la macchina la sceglie questa riga, adesso.
    ///
    /// Un indirizzo che è già completo passa intatto. Non capita oggi, ma è la
    /// differenza fra una funzione che si può chiamare due volte e una che al
    /// secondo giro incolla un nodo davanti a un altro nodo.
    ///
    /// # Errori
    ///
    /// `catalogo.notAvailable` se non si riesce a scegliere un nodo, `net.*`
    /// per i guasti di trasporto nel chiederne l'elenco.
    pub fn prepara(&self, candidato: &Candidato) -> Result<Candidato, AppError> {
        if candidato.url.starts_with("http://") || candidato.url.starts_with("https://") {
            return Ok(candidato.clone());
        }
        let (percorso, coda) = candidato
            .url
            .split_once('?')
            .unwrap_or((&candidato.url, ""));
        let estensione = coda
            .split('&')
            .find_map(|p| p.strip_prefix("ext="))
            .filter(|e| {
                !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric())
            })
            .map(str::to_owned);
        let nodo = self.nodo()?;
        Ok(Candidato {
            // `app_name` e nient'altro: il suggerimento dell'estensione serviva
            // a noi, e spedirlo vorrebbe dire raccontare al catalogo qualcosa
            // che riguarda solo il nostro disco.
            url: format!(
                "{}{percorso}?app_name={NOME_APP}",
                nodo.trim_end_matches('/')
            ),
            estensione: estensione.or_else(|| candidato.estensione.clone()),
            ..candidato.clone()
        })
    }

    /// Una richiesta all'API, sul nodo scelto.
    ///
    /// Se il nodo tace lo si dimentica e si riprova **una volta** con un altro.
    /// Il secondo silenzio non è del catalogo: è della rete di chi ascolta, e
    /// va detto invece che nascosto sotto altri diciannove tentativi.
    fn chiedi(&self, percorso: &str, parametri: &[(&str, &str)]) -> Result<Value, AppError> {
        let primo = self.nodo()?;
        match self.chiedi_a(&primo, percorso, parametri) {
            Ok(valore) => Ok(valore),
            Err(primo_guasto) => {
                self.dimentica(&primo);
                let secondo = self.nodo()?;
                if secondo == primo {
                    return Err(primo_guasto);
                }
                self.chiedi_a(&secondo, percorso, parametri)
            }
        }
    }

    /// La richiesta vera, su un nodo preciso.
    fn chiedi_a(
        &self,
        nodo: &str,
        percorso: &str,
        parametri: &[(&str, &str)],
    ) -> Result<Value, AppError> {
        let mut url = format!(
            "{}{percorso}?app_name={NOME_APP}",
            nodo.trim_end_matches('/')
        );
        for (nome, valore) in parametri {
            url.push('&');
            url.push_str(nome);
            url.push('=');
            url.push_str(&percento(valore));
        }
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
                .with_cause(format!("la risposta di {percorso} non è JSON: {err}"))
        })
    }

    /// Il nodo scelto, chiedendone uno se non se n'è ancora scelto nessuno.
    fn nodo(&self) -> Result<String, AppError> {
        if let Some(gia) = self
            .nodo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Ok(gia);
        }
        let risposta = self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url: ELENCO_NODI,
            intestazioni: &[("accept", "application/json")],
            corpo: Corpo::Niente,
        })?;
        if !risposta.e_andata() {
            return Err(self.rete.stato_a_errore(&risposta, ELENCO_NODI));
        }
        let corpo: Value = serde_json::from_slice(&risposta.corpo).map_err(|err| {
            AppError::new(ErrorCode::DownloadBadResponse)
                .with_cause(format!("l'elenco dei nodi non è JSON: {err}"))
        })?;
        let scelto = elenco(&corpo)
            .iter()
            .filter_map(Value::as_str)
            .find(|n| n.starts_with("https://"))
            .map(ToOwned::to_owned);
        let Some(scelto) = scelto else {
            return Err(AppError::new(ErrorCode::CatalogoNotAvailable)
                .with_cause("Audius non ha nessun nodo in salute da offrire".to_owned()));
        };
        *self.nodo.lock().unwrap_or_else(PoisonError::into_inner) = Some(scelto.clone());
        Ok(scelto)
    }

    /// Butta via il nodo scelto, se è ancora quello.
    ///
    /// Il confronto non è pignoleria: fra il guasto e questa riga un altro filo
    /// può averne già scelto un altro, e dimenticare il suo vorrebbe dire far
    /// ricominciare da capo qualcuno che stava andando bene.
    fn dimentica(&self, quale: &str) {
        let mut guardia = self.nodo.lock().unwrap_or_else(PoisonError::into_inner);
        if guardia.as_deref() == Some(quale) {
            *guardia = None;
        }
    }
}

/// L'array che sta in `data`, o niente.
fn elenco(corpo: &Value) -> Vec<Value> {
    corpo
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Un campo di testo, non vuoto.
fn testo(valore: &Value, campo: &str) -> Option<String> {
    let grezzo = valore.get(campo)?.as_str()?.trim();
    (!grezzo.is_empty()).then(|| grezzo.to_owned())
}

/// Un booleano, che vale `false` quando manca.
fn vero(valore: &Value, campo: &str) -> bool {
    valore.get(campo).and_then(Value::as_bool).unwrap_or(false)
}

/// La copertina, nella misura più grande che il catalogo dichiara.
///
/// Dalla più grande alla più piccola: quel che se ne fa Aether è incorporarla
/// nei tag di un file che resterà sul disco per anni, e una miniatura da 150
/// pixel dentro un FLAC è un difetto che non si corregge più senza riscrivere
/// il file.
fn copertina(valore: &Value) -> Option<String> {
    let arte = valore.get("artwork")?;
    for misura in ["1000x1000", "480x480", "150x150"] {
        if let Some(url) = testo(arte, misura) {
            return Some(url);
        }
    }
    None
}

/// La licenza, dal campo di testo che scrive l'artista.
///
/// # Perché si legge una frase invece di un codice
///
/// Perché è una frase: Audius mostra all'artista un menu a tendina e ne salva
/// l'etichetta, non un identificatore. «Attribution ShareAlike CC BY-SA» è quel
/// che arriva, e il codice `by-sa` va estratto da lì.
///
/// Tutto ciò che non si riconosce vale [`Licenza::Sconosciuta`], che **non**
/// permette la copia. Non è prudenza esagerata: il campo è libero, e un giorno
/// ci sarà dentro qualcosa che nessuno ha previsto. Quel giorno il brano si
/// ascolterà e non si scaricherà, che è il modo giusto di non sapere.
fn licenza_da(traccia: &Value) -> Licenza {
    let Some(grezza) = testo(traccia, "license") else {
        return Licenza::Sconosciuta;
    };
    let piatta = grezza.to_ascii_lowercase();

    if piatta.contains("cc0") || piatta.contains("public domain") {
        return Licenza::PubblicoDominio;
    }
    if piatta.contains("all rights reserved") {
        return Licenza::TutteRiservate;
    }
    if piatta.contains("open music license") {
        return Licenza::OpenMusicLicense;
    }
    // `cc by-nc-sa`, `cc by-sa`, `cc by`: si prende quel che segue «cc by» e si
    // tiene finché sono lettere e trattini. L'ordine dei confronti qui sopra
    // conta — «CC0» contiene «cc» ma non è una licenza con attribuzione.
    if let Some(coda) = piatta.split("cc by").nth(1) {
        let codice: String = coda
            .trim_start_matches(['-', ' '])
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '-')
            .collect();
        let codice = codice.trim_end_matches('-');
        return if codice.is_empty() {
            Licenza::CreativeCommons("by".to_owned())
        } else {
            Licenza::CreativeCommons(format!("by-{codice}"))
        };
    }
    Licenza::Sconosciuta
}

/// Che cosa si può fare di questo brano.
///
/// I due cancelli di Audius stanno **sopra** la licenza, e vanno guardati
/// prima: un brano il cui scarico è chiuso dietro un seguito o un gettone non
/// diventa scaricabile perché la sua licenza è una CC BY. La licenza dice cosa
/// l'autore consente in astratto; il cancello dice cosa il catalogo consegna a
/// noi, adesso.
fn disponibilita_da(traccia: &Value, licenza: &Licenza) -> Disponibilita {
    let scaricabile = (vero(traccia, "is_downloadable") || vero(traccia, "downloadable"))
        && !vero(traccia, "is_download_gated");
    Disponibilita::decidi(Fonte::Audius, licenza, scaricabile)
}

/// Da una traccia dell'API a un candidato.
///
/// `None` quando il brano non si può nemmeno sentire: senza titolo, non
/// riproducibile, o con il cancello sullo streaming. Un elenco che li
/// contenesse darebbe una playlist di brani che non partono, che è peggio di
/// una playlist più corta.
///
/// `atteso` è il titolo che si stava cercando, quando lo si stava cercando: da
/// lì si decide se il risultato è la registrazione chiesta o un'altra. Chi
/// risolve un link non sta cercando niente e passa `None`.
fn candidato_da(traccia: &Value, atteso: Option<&str>) -> Option<Candidato> {
    let titolo = testo(traccia, "title")?;
    let id = testo(traccia, "id")?;
    if vero(traccia, "is_stream_gated") {
        return None;
    }
    // `is_streamable` manca sulle risposte più vecchie: assente vale «sì», o
    // un campo aggiunto dopo farebbe sparire tutto il catalogo.
    if traccia
        .get("is_streamable")
        .and_then(Value::as_bool)
        .is_some_and(|s| !s)
    {
        return None;
    }

    let licenza = licenza_da(traccia);
    let disponibilita = disponibilita_da(traccia, &licenza);
    let utente = traccia.get("user");
    let autore = utente.and_then(|u| testo(u, "name").or_else(|| testo(u, "handle")));
    let pagina = testo(traccia, "permalink").map(|p| {
        format!(
            "https://audius.co{}",
            if p.starts_with('/') {
                p
            } else {
                format!("/{p}")
            }
        )
    });

    // Dal nome del file che l'artista ha caricato, quando c'è: il punto di
    // scarico restituisce **quello**, che può essere un wav o un flac, e
    // scriverlo con estensione `.mp3` darebbe una libreria in cui il formato
    // dichiarato non è il formato vero.
    let estensione = testo(traccia, "orig_filename")
        .and_then(|n| n.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()))
        .filter(|e| !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()));

    Some(Candidato {
        url: indirizzo(&id, disponibilita, estensione.as_deref()),
        natura: atteso.map_or_else(Natura::default, |a| natura_dal_titolo(&titolo, a)),
        titolo,
        autore,
        autore_verificato: utente.is_some_and(|u| vero(u, "is_verified")),
        durata_sec: traccia
            .get("duration")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok()),
        fonte: Fonte::Audius,
        licenza,
        disponibilita,
        // Volutamente vuota: l'estensione viaggia dentro l'indirizzo, e la
        // rimette [`Audius::prepara`]. Vedi il commento di [`indirizzo`].
        estensione: None,
        pagina,
        // Su Audius quasi tutto è un singolo, e quando fa parte di una
        // raccolta il brano porta il nome di quella. Niente ripiego: un album
        // inventato sarebbe peggio di un album assente.
        album: testo(traccia, "album_backlink")
            .or_else(|| traccia.get("album").and_then(|a| testo(a, "playlist_name"))),
    })
}

/// L'indirizzo conservato di un brano: un percorso, non un URL.
///
/// # Perché non l'indirizzo completo
///
/// Perché questo testo non serve solo adesso: finisce in `desiderati.fonte_url`
/// e ci resta finché quel brano non viene procurato, che può essere fra un mese.
/// Audius non ha un server ma una rete di nodi che entrano ed escono, e un
/// indirizzo con dentro il nodo di oggi è un indirizzo che fra un mese punta a
/// una macchina che non c'è più. Il percorso invece non invecchia: il nodo lo
/// rimette [`Audius::prepara`] al momento di andarlo a prendere, e sarà uno che
/// risponde adesso.
///
/// # Perché l'estensione sta qui dentro
///
/// Perché `Candidato::estensione` **non sopravvive** al viaggio: la coda
/// ricostruisce il candidato da `desiderati` con `..Candidato::default()`, e
/// quel campo torna vuoto. Il punto di scarico non ha un'estensione nel
/// percorso — finisce per `/download` — quindi senza questo suggerimento un wav
/// originale finirebbe sul disco chiamato `.mp3`. Audius ignora i parametri che
/// non conosce, come ogni API di questo tipo; `prepara` lo toglie comunque
/// prima di spedire la richiesta, così quel che parte è solo quel che serve.
fn indirizzo(id: &str, disponibilita: Disponibilita, estensione: Option<&str>) -> String {
    // `/download` dà il file originale che l'artista ha caricato, `/stream` un
    // mp3 transcodificato: quale dei due si scriva qui **è** la differenza fra
    // tenere e non tenere. Chi non può tenere niente non ha bisogno del primo,
    // e chiederglielo sarebbe chiedere un permesso che sappiamo di non avere.
    let punto = if disponibilita == Disponibilita::Scaricabile {
        "download"
    } else {
        "stream"
    };
    let base = format!("/v1/tracks/{}/{punto}", percento(id));
    match estensione {
        Some(ext) => format!("{base}?ext={ext}"),
        None => base,
    }
}

/// Da un candidato a un brano, col suo posto nell'elenco.
fn brano_da(candidato: Candidato, posizione: usize) -> BranoEsterno {
    BranoEsterno {
        title: candidato.titolo,
        artist: candidato.autore,
        duration_ms: candidato
            .durata_sec
            .map(|s| u64::from(s).saturating_mul(1_000)),
        fonte_url: Some(candidato.url),
        licenza: candidato.licenza,
        disponibilita: candidato.disponibilita,
        pagina_url: candidato.pagina,
        track_number: u32::try_from(posizione.saturating_add(1)).ok(),
        ..BranoEsterno::default()
    }
}

/// Le tracce leggibili di un elenco, numerate come stanno lassù.
///
/// Quelle che non si possono sentire spariscono, e la numerazione si stringe
/// dietro di loro: un album con un buco al posto della traccia 4 sarebbe un
/// album che dichiara di avere un brano che non ha.
fn tracce_da(brani: &[Value]) -> Vec<BranoEsterno> {
    brani
        .iter()
        .filter_map(|t| candidato_da(t, None))
        .take(TETTO_BRANI)
        .enumerate()
        .map(|(n, c)| {
            let mut brano = brano_da(c, n);
            brano.cover_url = None;
            brano
        })
        .collect()
}

#[cfg(test)]
mod prove {
    #[test]
    fn il_prelievo_non_usa_la_rete_delle_domande() {
        let audius = super::Audius::nuovo();
        assert_eq!(audius.rete().scadenza_complessiva(), Some(super::SCADENZA));
        assert_eq!(audius.rete_prelievo().scadenza_complessiva(), None);
    }

    use super::*;

    fn traccia(json: &str) -> Value {
        serde_json::from_str(json).unwrap_or(Value::Null)
    }

    #[test]
    fn le_etichette_di_licenza_di_audius_si_leggono_tutte() {
        // Sono le voci del menu che Audius mostra all'artista, copiate come le
        // salva: se una di queste smettesse di essere riconosciuta, i brani che
        // la portano diventerebbero silenziosamente non scaricabili.
        for (etichetta, atteso) in [
            (
                "Attribution CC BY",
                Licenza::CreativeCommons("by".to_owned()),
            ),
            (
                "Attribution ShareAlike CC BY-SA",
                Licenza::CreativeCommons("by-sa".to_owned()),
            ),
            (
                "Attribution NonCommercial CC BY-NC",
                Licenza::CreativeCommons("by-nc".to_owned()),
            ),
            (
                "Attribution NonCommercial NoDerivatives CC BY-NC-ND",
                Licenza::CreativeCommons("by-nc-nd".to_owned()),
            ),
            ("CC0 / Public Domain", Licenza::PubblicoDominio),
            ("All rights reserved", Licenza::TutteRiservate),
        ] {
            let valore = traccia(&format!(r#"{{"license":"{etichetta}"}}"#));
            assert_eq!(licenza_da(&valore), atteso, "«{etichetta}»");
        }
    }

    #[test]
    fn una_licenza_che_non_si_capisce_non_diventa_scaricabile() {
        // Il campo è libero e un giorno ci sarà dentro qualcosa di nuovo. Quel
        // giorno il brano dev'essere ascoltabile e non copiabile, non il
        // contrario.
        let valore = traccia(r#"{"license":"qualcosa che nessuno ha previsto"}"#);
        assert_eq!(licenza_da(&valore), Licenza::Sconosciuta);
        assert!(!licenza_da(&valore).permette_copia());

        // E senza il campo affatto.
        assert_eq!(licenza_da(&traccia("{}")), Licenza::Sconosciuta);
    }

    #[test]
    fn il_cancello_sullo_scarico_batte_la_licenza() {
        // Una CC BY con il cancello: la licenza direbbe di sì, il catalogo dice
        // di no, e vince il catalogo. È l'intersezione dei no, non la somma dei
        // sì.
        let aperto = traccia(
            r#"{"title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true}"#,
        );
        let chiuso = traccia(
            r#"{"title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true,"is_download_gated":true}"#,
        );
        assert_eq!(
            disponibilita_da(&aperto, &licenza_da(&aperto)),
            Disponibilita::Scaricabile
        );
        assert_eq!(
            disponibilita_da(&chiuso, &licenza_da(&chiuso)),
            Disponibilita::SoloAscolto
        );
    }

    #[test]
    fn un_brano_col_cancello_sullo_streaming_non_entra_nell_elenco() {
        // Non è una scelta di gusto: un elenco che lo contenesse darebbe una
        // playlist con dentro un brano che non parte.
        let chiuso = traccia(r#"{"title":"t","id":"a1","is_stream_gated":true}"#);
        assert!(candidato_da(&chiuso, None).is_none());

        let muto = traccia(r#"{"title":"t","id":"a1","is_streamable":false}"#);
        assert!(candidato_da(&muto, None).is_none());

        // Ma un campo assente non deve far sparire il catalogo intero.
        let vecchio = traccia(r#"{"title":"t","id":"a1"}"#);
        assert!(candidato_da(&vecchio, None).is_some());
    }

    #[test]
    fn chi_non_puo_tenere_niente_chiede_lo_streaming_e_non_lo_scarico() {
        let libero = traccia(
            r#"{"title":"t","id":"aB3","license":"Attribution CC BY","is_downloadable":true}"#,
        );
        let riservato = traccia(r#"{"title":"t","id":"aB3","license":"All rights reserved"}"#);
        let libero = candidato_da(&libero, None).unwrap_or_default();
        let riservato = candidato_da(&riservato, None).unwrap_or_default();
        assert!(libero.url.ends_with("/download"), "{}", libero.url);
        assert!(riservato.url.ends_with("/stream"), "{}", riservato.url);
        assert!(libero.si_puo_tenere());
        assert!(!riservato.si_puo_tenere());
    }

    /// Un client con un nodo già scelto, per provare tutto senza rete.
    fn con_nodo(quale: &str) -> Audius {
        let audius = Audius::nuovo();
        *audius.nodo.lock().unwrap_or_else(PoisonError::into_inner) = Some(quale.to_owned());
        audius
    }

    #[test]
    fn l_estensione_sopravvive_al_viaggio_in_tabella() {
        // Il punto di scarico restituisce quel che l'artista ha caricato: un
        // wav scritto con estensione `.mp3` sarebbe una libreria che dichiara
        // un formato che non ha. E `Candidato::estensione` non sopravvive al
        // giro in `desiderati` — la coda ricostruisce il candidato con
        // `..Default::default()` — quindi l'estensione viaggia dentro
        // l'indirizzo, che invece sopravvive.
        let wav = traccia(
            r#"{"title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true,"orig_filename":"la mia canzone.wav"}"#,
        );
        let candidato = candidato_da(&wav, None).unwrap_or_default();
        assert!(
            candidato.url.ends_with("/download?ext=wav"),
            "il suggerimento non è nell'indirizzo: {}",
            candidato.url
        );

        // Il giro completo: quel che la coda rilegge dalla tabella è
        // l'indirizzo e nient'altro, e `prepara` ne ricava le due cose che
        // servono — il nodo davanti, e l'estensione fuori.
        let dalla_tabella = Candidato {
            url: candidato.url.clone(),
            ..Candidato::default()
        };
        let pronto = con_nodo("https://nodo.esempio")
            .prepara(&dalla_tabella)
            .unwrap_or_default();
        assert_eq!(pronto.estensione, Some("wav".to_owned()));
        assert_eq!(
            pronto.url, "https://nodo.esempio/v1/tracks/a1/download?app_name=Aether",
            "il suggerimento non doveva essere spedito al catalogo"
        );
    }

    #[test]
    fn senza_suggerimento_lestensione_la_decide_prelievo() {
        let senza = traccia(r#"{"title":"t","id":"a1"}"#);
        let candidato = candidato_da(&senza, None).unwrap_or_default();
        assert!(!candidato.url.contains("ext="), "{}", candidato.url);
        let pronto = con_nodo("https://nodo.esempio")
            .prepara(&candidato)
            .unwrap_or_default();
        assert_eq!(pronto.estensione, None);
        assert_eq!(
            pronto.url,
            "https://nodo.esempio/v1/tracks/a1/stream?app_name=Aether"
        );
    }

    #[test]
    fn un_indirizzo_gia_completo_non_prende_un_secondo_nodo() {
        // `prepara` deve poter essere chiamata due volte senza incollare un
        // nodo davanti a un altro nodo.
        let gia = Candidato {
            url: "https://nodo.esempio/v1/tracks/a1/stream?app_name=Aether".to_owned(),
            ..Candidato::default()
        };
        let pronto = con_nodo("https://un.altro.nodo")
            .prepara(&gia)
            .unwrap_or_default();
        assert_eq!(pronto.url, gia.url);
    }

    #[test]
    fn il_nodo_che_tace_si_dimentica_ma_solo_se_e_ancora_il_suo() {
        // Fra il guasto e la dimenticanza un altro filo può aver già scelto:
        // buttare via il nodo di qualcun altro vorrebbe dire far ricominciare
        // da capo chi stava andando bene.
        let audius = con_nodo("https://primo.esempio");
        audius.dimentica("https://un.nodo.mai.scelto");
        assert_eq!(
            audius
                .nodo
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_deref(),
            Some("https://primo.esempio"),
            "ha dimenticato un nodo che non era il suo"
        );
        audius.dimentica("https://primo.esempio");
        assert!(
            audius
                .nodo
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_none(),
            "il nodo che tace è rimasto scelto"
        );
    }

    #[test]
    fn la_copertina_si_prende_grande() {
        // Finisce dentro i tag di un file che resta sul disco per anni: una
        // miniatura da 150 pixel là dentro non si corregge più.
        let tutte = traccia(
            r#"{"artwork":{"150x150":"https://p/s.jpg","480x480":"https://p/m.jpg","1000x1000":"https://p/l.jpg"}}"#,
        );
        assert_eq!(copertina(&tutte), Some("https://p/l.jpg".to_owned()));
        let poche = traccia(r#"{"artwork":{"150x150":"https://p/s.jpg"}}"#);
        assert_eq!(copertina(&poche), Some("https://p/s.jpg".to_owned()));
        assert_eq!(copertina(&traccia("{}")), None);
    }

    #[test]
    fn le_tracce_illeggibili_spariscono_e_la_numerazione_si_stringe() {
        let brani = vec![
            traccia(r#"{"title":"uno","id":"a1"}"#),
            traccia(r#"{"title":"due","id":"a2","is_stream_gated":true}"#),
            traccia(r#"{"title":"tre","id":"a3"}"#),
        ];
        let tracce = tracce_da(&brani);
        assert_eq!(tracce.len(), 2, "il brano col cancello è passato");
        assert_eq!(tracce.first().map(|b| b.track_number), Some(Some(1)));
        // «tre» prende il 2 e non il 3: un album non dichiara un brano che non ha.
        assert_eq!(tracce.get(1).map(|b| b.track_number), Some(Some(2)));
        assert_eq!(tracce.get(1).map(|b| b.title.as_str()), Some("tre"));
    }

    #[test]
    fn la_pagina_pubblica_si_ricompone_dal_permalink() {
        // I termini di Audius obbligano a un rimando visibile accanto al brano,
        // e `attribuzione` lo scrive dentro il tag del file.
        let con = traccia(r#"{"title":"t","id":"a1","permalink":"/tizio/canzone"}"#);
        assert_eq!(
            candidato_da(&con, None).and_then(|c| c.pagina),
            Some("https://audius.co/tizio/canzone".to_owned())
        );
    }
}
