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
//! (`is_stream_gated`, `is_download_gated`, con le loro `…_conditions`). Un
//! brano col cancello sullo streaming non entra nemmeno nell'elenco — non lo
//! si potrebbe sentire — e uno col cancello sullo scarico scende a solo ascolto
//! qualunque cosa dica la sua licenza. Il permesso di un catalogo non è la
//! somma delle sue licenze: è l'intersezione di tutti i suoi no, e un no che la
//! risposta non dice — un cancello assente, scritto in una forma che non si
//! capisce — conta come detto.
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
//! ed è anche l'unica cosa che Rymembra dice di sé a qualcuno: nessun
//! identificativo, nessun conteggio, nessun numero di serie. Vedi `PRIVACY.md`
//! § 1.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::esterno::{
    BranoEsterno, ContenutoEsterno, Disponibilita, Fonte, GenereContenuto, Licenza, Livello,
};
use rymembra_domain::scelta::{Candidato, Natura, natura_dal_titolo, query_larga, query_stretta};
use rymembra_net::{Corpo, Metodo, Rete, Richiesta, percento};
use serde_json::Value;

use crate::riferimento::{Riferimento, indirizzo_pubblico};

/// Dove si chiede l'elenco dei nodi in salute.
const ELENCO_NODI: &str = "https://api.audius.co";

/// Come Rymembra si presenta all'API.
///
/// Non è un identificativo: è lo stesso testo per ogni copia del programma nel
/// mondo, e serve al catalogo per sapere quali applicazioni lo interrogano.
const NOME_APP: &str = "Rymembra";

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
    /// La rete dei flussi: vedi [`Rete::per_flusso`].
    flusso: Rete,
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
            flusso: Rete::per_flusso("audius.co"),
            nodo: Arc::new(Mutex::new(None)),
        }
    }

    /// Un client con un nodo già scelto, per le prove: niente elenco da
    /// chiedere, quindi niente rete prima del prelievo vero.
    #[cfg(test)]
    pub(crate) fn con_nodo(quale: &str) -> Self {
        let audius = Self::nuovo();
        *audius.nodo.lock().unwrap_or_else(PoisonError::into_inner) = Some(quale.to_owned());
        audius
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

    /// La rete che usa per **suonare un file mentre arriva**: vedi
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
    /// per i guasti di trasporto, `internal.aborted` se chi ha chiesto ha
    /// annullato. Un annullamento non è mai un elenco vuoto: per la coda un
    /// elenco vuoto vuol dire «introvabile», che è terminale.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        if annullato() {
            return Err(crate::ricerca_annullata("Audius"));
        }
        let mut trovati = self.cerca_con(&query_stretta(brano), Some(&brano.title))?;
        if annullato() {
            return Err(crate::ricerca_annullata("Audius"));
        }
        if trovati.is_empty()
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
                // modo diverso — uno si ritenta, l'altro no. E fra le ricerche
                // che non riescono, quella respinta si distingue da quella
                // caduta: vedi `guasto_di_ricerca`.
                crate::guasto_di_ricerca("Audius", &err, "la ricerca non ha risposto")
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

    /// Un brano, con la licenza, i cancelli e la pagina di **adesso**.
    ///
    /// Per la coda che preleva un brano il cui file era già scelto: vedi
    /// [`crate::Cataloghi::da_prelevare`]. L'indirizzo del candidato si rifà
    /// dai dati di adesso, quindi è `/download` solo se oggi l'artista lo
    /// consente. `None` se il catalogo non conosce più quel brano, o se lo
    /// consegna sotto un altro identificativo.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::chiedi`]: `net.*`, `download.badResponse`,
    /// `catalogo.notAvailable`.
    pub fn riletto(&self, id: &str) -> Result<Option<Candidato>, AppError> {
        let corpo = self.chiedi(&format!("/v1/tracks/{}", percento(id)), &[])?;
        Ok(traccia_riletta(&corpo, id))
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
        let Some(scelto) = nodo_dall_elenco(&corpo) else {
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

/// Il brano `id` dalla risposta di `/v1/tracks/<id>`, come candidato.
///
/// L'oggetto o un elenco di uno, come per `resolve`. Un identificativo diverso
/// da quello chiesto non è il brano chiesto.
fn traccia_riletta(corpo: &Value, id: &str) -> Option<Candidato> {
    let dato = corpo.get("data")?;
    let traccia = dato.as_array().and_then(|a| a.first()).unwrap_or(dato);
    if testo(traccia, "id").as_deref() != Some(id) {
        return None;
    }
    candidato_da(traccia, None)
}

/// Il nodo da usare, dall'elenco che `api.audius.co` consegna.
///
/// Il primo che sia un [`indirizzo_pubblico`]: da quel nodo partono ricerche,
/// flussi e prelievi, e un elenco alterato — una risposta manomessa, un
/// servizio compromesso — non deve poterle mandare al router o al NAS di casa.
/// Lo stesso controllo che i rimandi dei flussi facevano già sui salti, adesso
/// anche sul punto di partenza.
fn nodo_dall_elenco(corpo: &Value) -> Option<String> {
    elenco(corpo)
        .iter()
        .filter_map(Value::as_str)
        .find(|n| indirizzo_pubblico(n))
        .map(ToOwned::to_owned)
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
///
/// Solo per quel che **non** concede niente — la spunta di verifica di un
/// autore. I permessi passano da [`esattamente`], e i cancelli da
/// [`cancello_chiuso`].
fn vero(valore: &Value, campo: &str) -> bool {
    valore.get(campo).and_then(Value::as_bool).unwrap_or(false)
}

/// Il campo c'è, ed è esattamente il booleano `atteso`.
///
/// Un `"true"` scritto come testo, un numero, un campo assente: niente di
/// questo è la risposta attesa, e chi decide un permesso deve poterlo
/// distinguere da un sì.
fn esattamente(valore: &Value, campo: &str, atteso: bool) -> bool {
    valore.get(campo).and_then(Value::as_bool) == Some(atteso)
}

/// Un cancello di Audius che può essere chiuso: la bandiera `is_<…>_gated` o
/// le condizioni `<…>_conditions`.
///
/// Chiuso è tutto ciò che non dice «aperto» per esteso: la bandiera deve essere
/// `false` scritto, e le condizioni assenti o `null`. Un oggetto, anche vuoto,
/// è una condizione; una bandiera che manca è una domanda senza risposta.
/// Vedi [`disponibilita_da`] per perché si sbaglia da questa parte.
fn cancello_chiuso(traccia: &Value, bandiera: &str, condizioni: &str) -> bool {
    let condizioni_assenti = traccia.get(condizioni).is_none_or(Value::is_null);
    !(esattamente(traccia, bandiera, false) && condizioni_assenti)
}

/// Il cancello sullo streaming è dichiarato chiuso.
///
/// Il gemello largo di [`cancello_chiuso`], per una domanda diversa: qui non
/// si decide se tenere, ma se il brano entra nell'elenco di quel che si
/// ascolta. Un campo **assente** lascia entrare — le risposte più vecchie non
/// lo portano, e farle sparire vorrebbe dire svuotare il catalogo — mentre un
/// valore che non sia `false`, una condizione o un accesso negato dal catalogo
/// tengono fuori. Per tenerne una copia vale comunque la regola stretta.
fn streaming_chiuso(traccia: &Value) -> bool {
    let bandiera = traccia
        .get("is_stream_gated")
        .is_some_and(|v| v.as_bool() != Some(false));
    let condizioni = traccia
        .get("stream_conditions")
        .is_some_and(|v| !v.is_null());
    let accesso = traccia
        .get("access")
        .is_some_and(|a| a.get("stream").and_then(Value::as_bool) != Some(true));
    bandiera || condizioni || accesso
}

/// La copertina, nella misura più grande che il catalogo dichiara.
///
/// Dalla più grande alla più piccola: quel che se ne fa Rymembra è incorporarla
/// nei tag di un file che resterà sul disco per anni, e una miniatura da 150
/// pixel dentro un FLAC è un difetto che non si corregge più senza riscrivere
/// il file.
///
/// Solo un [`indirizzo_pubblico`]: l'indirizzo arriva dalla risposta e chi lo
/// riceve ci fa una richiesta, quindi vale lo stesso controllo del nodo. Una
/// misura che punta a casa si salta, e si prova la successiva.
fn copertina(valore: &Value) -> Option<String> {
    let arte = valore.get("artwork")?;
    for misura in ["1000x1000", "480x480", "150x150"] {
        if let Some(url) = testo(arte, misura).filter(|u| indirizzo_pubblico(u)) {
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
///
/// # Si chiude in sicurezza
///
/// Fino alla 3.2 il divieto si deduceva dalla presenza di un `true`: un
/// `is_download_gated` assente, scritto come testo o accompagnato da una
/// `download_conditions` valeva «nessun cancello», e un brano col seguito
/// obbligatorio diventava scaricabile. Adesso il permesso vuole **ogni no
/// detto**: l'interruttore acceso, i due cancelli a `false` scritti — chi ha
/// il cancello sullo streaming ce l'ha anche sullo scarico — nessuna
/// condizione, e l'accesso che il catalogo calcola, quando lo manda, aperto.
/// Lo schema attuale dell'API porta sempre i due cancelli e l'accesso, quindi
/// un dato che manca è una risposta che non si capisce, e su quella non si
/// scrive niente sul disco.
fn disponibilita_da(traccia: &Value, licenza: &Licenza) -> Disponibilita {
    let interruttore =
        esattamente(traccia, "is_downloadable", true) || esattamente(traccia, "downloadable", true);
    let accesso = traccia
        .get("access")
        .is_none_or(|a| a.get("download").and_then(Value::as_bool) == Some(true));
    let scaricabile = interruttore
        && !cancello_chiuso(traccia, "is_download_gated", "download_conditions")
        && !cancello_chiuso(traccia, "is_stream_gated", "stream_conditions")
        && accesso;
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
    if streaming_chiuso(traccia) {
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
            r#"{"title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true,"is_download_gated":false,"is_stream_gated":false}"#,
        );
        let chiuso = traccia(
            r#"{"title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true,"is_download_gated":true,"is_stream_gated":false}"#,
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
    fn un_cancello_che_manca_o_non_si_capisce_chiude_lo_scarico() {
        // Il permesso si concede solo quando ogni no è stato guardato e
        // detto: interruttore acceso, i due cancelli a `false` scritti, nessuna
        // condizione, e l'accesso che il catalogo calcola — se c'è — aperto.
        let base = r#""title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true"#;
        let aperto = r#""is_download_gated":false,"is_stream_gated":false"#;
        for (resto, perche) in [
            ("", "cancelli assenti"),
            (
                r#","is_download_gated":false"#,
                "cancello sullo streaming assente",
            ),
            (
                r#","is_stream_gated":false"#,
                "cancello sullo scarico assente",
            ),
            (
                r#","is_download_gated":"false","is_stream_gated":false"#,
                "un falso scritto come testo",
            ),
            (
                &format!(r#",{aperto},"download_conditions":{{"follow_user_id":1}}"#),
                "una condizione di scarico",
            ),
            (
                &format!(r#",{aperto},"download_conditions":{{}}"#),
                "una condizione vuota",
            ),
            (
                &format!(r#",{aperto},"stream_conditions":{{"usdc_purchase":{{}}}}"#),
                "una condizione sullo streaming",
            ),
            (
                &format!(r#",{aperto},"access":{{"stream":true,"download":false}}"#),
                "l'accesso calcolato dal catalogo",
            ),
        ] {
            let t = traccia(&format!("{{{base}{resto}}}"));
            assert_eq!(
                disponibilita_da(&t, &licenza_da(&t)),
                Disponibilita::SoloAscolto,
                "con {perche}"
            );
        }
        // Tutto detto, tutto aperto: si tiene.
        let t = traccia(&format!(
            r#"{{{base},{aperto},"download_conditions":null,"access":{{"stream":true,"download":true}}}}"#
        ));
        assert_eq!(
            disponibilita_da(&t, &licenza_da(&t)),
            Disponibilita::Scaricabile
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

        // Un cancello scritto in un'altra forma resta un cancello.
        for altro in [
            r#"{"title":"t","id":"a1","is_stream_gated":"true"}"#,
            r#"{"title":"t","id":"a1","stream_conditions":{"follow_user_id":1}}"#,
            r#"{"title":"t","id":"a1","access":{"stream":false,"download":false}}"#,
        ] {
            assert!(candidato_da(&traccia(altro), None).is_none(), "su {altro}");
        }

        // Ma un campo assente non deve far sparire il catalogo intero.
        let vecchio = traccia(r#"{"title":"t","id":"a1"}"#);
        assert!(candidato_da(&vecchio, None).is_some());
    }

    #[test]
    fn chi_non_puo_tenere_niente_chiede_lo_streaming_e_non_lo_scarico() {
        let libero = traccia(
            r#"{"title":"t","id":"aB3","license":"Attribution CC BY","is_downloadable":true,"is_download_gated":false,"is_stream_gated":false}"#,
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
        Audius::con_nodo(quale)
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
            r#"{"title":"t","id":"a1","license":"Attribution CC BY","is_downloadable":true,"is_download_gated":false,"is_stream_gated":false,"orig_filename":"la mia canzone.wav"}"#,
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
            pronto.url, "https://nodo.esempio/v1/tracks/a1/download?app_name=Rymembra",
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
            "https://nodo.esempio/v1/tracks/a1/stream?app_name=Rymembra"
        );
    }

    #[test]
    fn un_indirizzo_gia_completo_non_prende_un_secondo_nodo() {
        // `prepara` deve poter essere chiamata due volte senza incollare un
        // nodo davanti a un altro nodo.
        let gia = Candidato {
            url: "https://nodo.esempio/v1/tracks/a1/stream?app_name=Rymembra".to_owned(),
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
    fn una_traccia_riletta_porta_licenza_pagina_e_permesso_di_adesso() {
        let corpo = |scaricabile: bool| {
            traccia(&format!(
                r#"{{"data":{{"id":"aB3","title":"Una canzone","license":"Attribution ShareAlike CC BY-SA",
                   "is_downloadable":{scaricabile},"is_download_gated":false,"is_stream_gated":false,
                   "permalink":"/tizio/una-canzone","user":{{"name":"Tizio"}}}}}}"#
            ))
        };
        let oggi = traccia_riletta(&corpo(true), "aB3").expect("il brano c'è");
        assert_eq!(oggi.licenza, Licenza::CreativeCommons("by-sa".to_owned()));
        assert_eq!(
            oggi.pagina.as_deref(),
            Some("https://audius.co/tizio/una-canzone")
        );
        assert!(oggi.si_puo_tenere());
        assert!(oggi.url.ends_with("/download"), "{}", oggi.url);
        // L'artista ha spento lo scarico dopo l'importazione: si rilegge come
        // solo ascolto, e l'indirizzo torna allo streaming.
        let spento = traccia_riletta(&corpo(false), "aB3").expect("il brano c'è");
        assert!(!spento.si_puo_tenere());
        assert!(spento.url.ends_with("/stream"), "{}", spento.url);
        // Un altro brano, o nessuno, non è il brano chiesto.
        assert!(traccia_riletta(&corpo(true), "zZ9").is_none());
        assert!(traccia_riletta(&traccia(r#"{"data":null}"#), "aB3").is_none());
    }

    #[test]
    fn una_ricerca_annullata_non_e_un_risultato_vuoto() {
        // «Annulla» mentre la coda cerca: un elenco vuoto qui diventava
        // «introvabile», che è terminale, e il brano finiva nella lista della
        // spesa senza che nessuno l'avesse cercato davvero. L'annullamento si
        // guarda prima di qualunque richiesta: la prova gira senza rete.
        let brano = BranoEsterno {
            title: "Karma Police".to_owned(),
            artist: Some("Radiohead".to_owned()),
            ..BranoEsterno::default()
        };
        let esito = con_nodo("https://127.0.0.1:9").cerca(&brano, &|| true);
        assert!(
            matches!(
                esito.as_ref().map_err(AppError::code),
                Err(ErrorCode::InternalAborted { .. })
            ),
            "invece dell'annullamento: {esito:?}"
        );
    }

    #[test]
    fn un_nodo_di_casa_non_si_sceglie() {
        // Un elenco alterato non deve poter mandare ricerche, flussi e
        // prelievi al router o al NAS: si salta tutto ciò che non è un nome
        // pubblico, e si prende il primo che lo è.
        let corpo = traccia(
            r#"{"data":["https://192.168.1.1","https://[::1]:8443","https://nas.local",
                        "http://nodo-in-chiaro.esempio.com","https://nodo.esempio.com"]}"#,
        );
        assert_eq!(
            nodo_dall_elenco(&corpo).as_deref(),
            Some("https://nodo.esempio.com")
        );
        assert_eq!(
            nodo_dall_elenco(&traccia(r#"{"data":["https://10.0.0.1"]}"#)),
            None
        );
    }

    #[test]
    fn una_copertina_di_casa_non_si_prende() {
        // L'indirizzo d'immagine arriva dalla risposta e finisce in una GET:
        // lo stesso controllo del nodo.
        let arte = traccia(
            r#"{"artwork":{"1000x1000":"https://192.168.1.1/l.jpg","480x480":"https://p.esempio.com/m.jpg"}}"#,
        );
        assert_eq!(
            copertina(&arte),
            Some("https://p.esempio.com/m.jpg".to_owned())
        );
        let in_chiaro = traccia(r#"{"artwork":{"1000x1000":"http://p.esempio.com/l.jpg"}}"#);
        assert_eq!(copertina(&in_chiaro), None);
    }

    #[test]
    fn la_copertina_si_prende_grande() {
        // Finisce dentro i tag di un file che resta sul disco per anni: una
        // miniatura da 150 pixel là dentro non si corregge più.
        let tutte = traccia(
            r#"{"artwork":{"150x150":"https://p.esempio.com/s.jpg","480x480":"https://p.esempio.com/m.jpg","1000x1000":"https://p.esempio.com/l.jpg"}}"#,
        );
        assert_eq!(
            copertina(&tutte),
            Some("https://p.esempio.com/l.jpg".to_owned())
        );
        let poche = traccia(r#"{"artwork":{"150x150":"https://p.esempio.com/s.jpg"}}"#);
        assert_eq!(
            copertina(&poche),
            Some("https://p.esempio.com/s.jpg".to_owned())
        );
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
