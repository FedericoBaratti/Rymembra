//! I servizi di metadati musicali, tutti senza chiave.
//!
//! Quattro fonti, e nessuna vuole una registrazione, un account o un segreto da
//! tenere nel portachiavi: **MusicBrainz** per il catalogo e gli identificativi,
//! il **Cover Art Archive** per le copertine che ne discendono, **iTunes** e
//! **Deezer** come secondo e terzo parere.
//!
//! # La regola di dipendenza
//!
//! ```text
//! aether-meta  ──►  aether-net  ──►  aether-domain
//! ```
//!
//! Questo crate **non vede `rusqlite`**, esattamente come `aether-catalogo` e
//! `aether-archivio`. Non è pulizia: è ciò che rende impossibile — e non solo
//! sconsigliato — scrivere il codice che tiene preso il lucchetto della libreria
//! mentre aspetta una risposta da MusicBrainz. Chi ha bisogno di ricordare le
//! risposte passa dal tratto [`Deposito`], che sta implementato dall'altra parte
//! del confine.
//!
//! E non decide niente. Quale pubblicazione sia quella giusta lo dice
//! [`aether_domain::enrich`], che è puro e si prova senza rete; qui si costruisce
//! una richiesta, si legge una risposta e si tiene il ritmo.
//!
//! # Il terzo parere non è ridondanza
//!
//! Senza impronta acustica, la prova che una corrispondenza è quella giusta è
//! che **due basi dati indipendenti** la nominano allo stesso modo — è la regola
//! di `decide_track`, e vale la pena dire da dove viene la seconda opinione.
//! iTunes e Deezer non sono cloni di MusicBrainz: hanno cataloghi propri,
//! costruiti da distributori invece che da collaboratori, e sbagliano in modi
//! diversi. È precisamente questo che rende il loro accordo un'informazione.
//!
//! # Come ci si presenta
//!
//! Con lo `User-Agent` di serie di [`aether_net::Rete`], che porta nome,
//! versione e indirizzo del progetto. Non è cortesia: la politica di MusicBrainz
//! chiede abbastanza informazioni da poter scrivere a qualcuno invece di
//! limitarsi a bloccare, e un client anonimo viene servito peggio.

//! # Il vicinato è un metadato come gli altri
//!
//! [`listenbrainz`] non identifica niente e non corregge niente: dice quali
//! altri brani ascolta chi ascolta questo. Sta qui e non in un crate nuovo per
//! la stessa ragione di [`lrclib`] — è un servizio senza chiave, senza account
//! e senza segreti, che eredita gratis il deposito, la cadenza, l'interruttore
//! e la distinzione fra «non ce l'ha» e «non si sa». Aether parla già con
//! MusicBrainz e già manda ascolti a ListenBrainz: questo è lo stesso genere di
//! vicino, non un genere nuovo di rapporto.
//!
//! È anche l'unico di questi servizi le cui richieste non seguono più il ritmo
//! «una domanda, una risposta»: i suoi punti accettano molti identificativi per
//! volta, e quel dettaglio si porta dietro tutto il resto del suo progetto —
//! vedi [`listenbrainz`] e [`Fornitori::json_senza_memoria`].

pub mod cadenza;
pub mod copertine;
pub mod deezer;
pub mod deposito;
pub mod itunes;
pub mod listenbrainz;
pub mod lrclib;
pub mod musicbrainz;

use std::time::Duration;

use aether_domain::enrich::{Candidate, RemoteRelease};
use aether_domain::errors::AppError;
use aether_net::http::{Corpo, Metodo, Rete, Richiesta};

pub use cadenza::Cadenza;
pub use copertine::{Copertina, Provenienza, Sorgenti};
pub use deposito::{Deposito, Senza, Voce};

/// La scadenza di una richiesta ai servizi di metadati.
///
/// Venti secondi, e globale come tutte quelle di [`aether_net`]: copre anche il
/// caso che conta davvero qui, cioè un'immagine di mezzo megabyte che scende da
/// un archivio lento un pezzo per volta. Una scadenza di sola lettura lascerebbe
/// vivere per sempre una connessione che consegna un byte al secondo.
const SCADENZA: Duration = Duration::from_secs(20);

/// Cosa si è ottenuto interrogando le fonti per un brano.
///
/// # Perché «nessun candidato» e «nessuno ha risposto» sono due cose
///
/// Il primo è un fatto sul brano e si ricorda: quel file non lo riconosce
/// nessuno, e richiederlo fra un'ora darà la stessa risposta. Il secondo è un
/// fatto sulla **rete di adesso**, e ricordarlo vorrebbe dire marchiare come
/// introvabili tutti i brani che una passata ha incontrato mentre il portatile
/// era staccato dal wifi.
///
/// È lo stesso motivo per cui `enrich_status` resta `NULL` quando i servizi non
/// rispondono, invece di diventare `no-match`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Esito {
    /// I candidati, in ordine di interrogazione delle fonti.
    pub candidati: Vec<Candidate>,
    /// Nessuna fonte ha risposto. Non è «non c'è»: è «non si sa».
    pub tutte_giu: bool,
}

/// I servizi di metadati, con il loro ritmo e la loro memoria.
///
/// Si costruisce una volta sola e si tiene viva: dentro ci sono la riserva di
/// connessioni di `ureq` — che è quel che rende una passata di venti richieste
/// un solo saluto TLS invece di venti — e lo stato degli interruttori, che
/// perderebbe senso se rinascesse a ogni brano.
pub struct Fornitori {
    rete: Rete,
    deposito: Box<dyn Deposito>,
    /// Il cancello di MusicBrainz: una richiesta al secondo, e vale per tutte
    /// le sue chiamate insieme — ricerca, lettura di una pubblicazione,
    /// ricerca di una registrazione.
    pub musicbrainz: Cadenza,
    /// Il cancello del Cover Art Archive.
    pub copertine: Cadenza,
    /// Il cancello di iTunes.
    pub itunes: Cadenza,
    /// Il cancello di Deezer.
    pub deezer: Cadenza,
    /// Il cancello del catalogo dei testi.
    pub lrclib: Cadenza,
    /// Il cancello di ListenBrainz Labs, cioè delle affinità.
    ///
    /// Separato da quello dello scrobbling, che vive in `aether-scrobble` e
    /// parla con un altro host (`api.listenbrainz.org`): sono due servizi con
    /// due limiti e due modi di cadere, e un cancello solo per tutti e due
    /// vorrebbe dire che una coda di ascolti da mandare rallenta il calcolo
    /// delle somiglianze, o peggio che l'interruttore di uno spegne l'altro.
    pub listenbrainz: Cadenza,
}

impl std::fmt::Debug for Fornitori {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fornitori")
            .field("musicbrainz", &self.musicbrainz)
            .field("copertine", &self.copertine)
            .finish_non_exhaustive()
    }
}

impl Fornitori {
    /// I servizi, che ricordano nel deposito dato.
    #[must_use]
    pub fn nuovo(deposito: Box<dyn Deposito>) -> Self {
        Self {
            rete: Rete::nuova("metadati", SCADENZA),
            deposito,
            musicbrainz: Cadenza::nuova("musicbrainz", cadenza::RITMO_MUSICBRAINZ),
            copertine: Cadenza::nuova("coverartarchive", cadenza::RITMO_COVERART),
            itunes: Cadenza::nuova("itunes", cadenza::RITMO_ITUNES),
            deezer: Cadenza::nuova("deezer", cadenza::RITMO_DEEZER),
            lrclib: Cadenza::nuova("lrclib", cadenza::RITMO_LRCLIB),
            listenbrainz: Cadenza::nuova("listenbrainz-labs", cadenza::RITMO_LISTENBRAINZ),
        }
    }

    /// Vale la pena cominciare una passata **di arricchimento**.
    ///
    /// MusicBrainz è l'unica fonte da cui arrivano gli identificativi e la
    /// catena delle copertine: con il suo interruttore aperto le altre due
    /// possono al massimo correggere un titolo, e non vale il traffico.
    ///
    /// # Perché non parla per tutto il crate
    ///
    /// Perché guarda **solo** MusicBrainz, e questo era vero senza conseguenze
    /// finché ogni cosa qui dentro dipendeva da MusicBrainz. Le affinità no:
    /// arrivano da un altro host, con un altro interruttore, e chiedono
    /// identificativi che in libreria ci sono già. Usare questa come cancello
    /// anche per loro vorrebbe dire spegnere il vicinato perché il catalogo è
    /// giù — cioè legare due guasti che non hanno niente in comune.
    ///
    /// Chi chiama le affinità usa [`Self::affinita_in_piedi`].
    #[must_use]
    pub fn in_piedi(&self) -> bool {
        !self.musicbrainz.aperta()
    }

    /// Vale la pena chiedere delle affinità.
    ///
    /// Guarda l'interruttore di ListenBrainz Labs e nient'altro: vedi la nota
    /// in [`Self::in_piedi`] sul perché i due cancelli sono due.
    #[must_use]
    pub fn affinita_in_piedi(&self) -> bool {
        !self.listenbrainz.aperta()
    }

    /// Chiede un JSON a un servizio, passando dal deposito e dalla cadenza.
    ///
    /// `Ok(None)` è la risposta «non ce l'ho» — un `404`, o un no già ricordato.
    /// `Err` è «non si sa»: trasporto caduto, servizio in errore, interruttore
    /// aperto. La differenza fra i due attraversa tutto il resto del sistema, e
    /// nasce qui.
    fn json(
        &self,
        cadenza: &Cadenza,
        servizio: &str,
        chiave: &str,
        url: &str,
        vive_ms: i64,
    ) -> Result<Option<Vec<u8>>, AppError> {
        match self.deposito.leggi(servizio, chiave) {
            Some(Voce::Corpo(corpo)) => return Ok(Some(corpo)),
            Some(Voce::Niente) => return Ok(None),
            None => {}
        }
        match self.json_senza_memoria(cadenza, url)? {
            Some(corpo) => {
                self.deposito
                    .scrivi(servizio, chiave, &Voce::Corpo(corpo.clone()), vive_ms);
                Ok(Some(corpo))
            }
            None => {
                self.deposito
                    .scrivi(servizio, chiave, &Voce::Niente, deposito::VIVE_NIENTE_MS);
                Ok(None)
            }
        }
    }

    /// Come [`Self::json`], ma senza toccare il deposito.
    ///
    /// # Perché esiste
    ///
    /// Perché [`Self::json`] scrive **una voce di cache per chiamata**, e quella
    /// è la forma giusta finché una richiesta corrisponde a una domanda. I punti
    /// delle affinità non funzionano così: una richiesta porta gli
    /// identificativi di venticinque brani e torna con una risposta sola, da
    /// dividere e ricordare **per brano**.
    ///
    /// Ricordarla intera sotto una chiave che descrive il lotto sarebbe la cosa
    /// peggiore che si possa fare a una cache: la chiave dipenderebbe da *come*
    /// i brani sono stati raggruppati, e un lotto composto anche solo in un
    /// ordine diverso — cioè quasi sempre, perché i lotti li compone chi chiama
    /// scorrendo una libreria che intanto cambia — sarebbe un errore in cache a
    /// ogni passata. Il deposito si riempirebbe di risposte che non verranno
    /// mai più rilette.
    ///
    /// Quindi il cancello di ritmo e l'interruttore restano qui — sono
    /// proprietà del **servizio**, e vanno rispettati da chiunque gli parli — e
    /// la memoria se la gestisce chi sa dividere la risposta, con la chiave che
    /// deve avere: il singolo identificativo. Vedi [`listenbrainz`].
    ///
    /// # Errori
    ///
    /// Gli stessi di [`Self::json`], con la stessa distinzione: `Ok(None)` è un
    /// `404`, cioè «non ce l'ho», ed `Err` è «non si sa».
    fn json_senza_memoria(
        &self,
        cadenza: &Cadenza,
        url: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        if cadenza.aperta() {
            return Err(AppError::new(
                aether_domain::errors::ErrorCode::NetCircuitOpen {
                    service: cadenza.nome().to_owned(),
                    retry_after_ms: None,
                },
            ));
        }

        cadenza.attendi();
        let risposta = match self.rete.esegui(Richiesta {
            metodo: Metodo::Get,
            url,
            intestazioni: &[("Accept", "application/json")],
            corpo: Corpo::Niente,
        }) {
            Ok(risposta) => risposta,
            Err(err) => {
                cadenza.guasto();
                return Err(err);
            }
        };

        // Un `404` è una risposta, e una risposta veloce: contarla come un
        // guasto spegnerebbe l'interruttore proprio sul servizio che sta
        // funzionando meglio di tutti, cioè quello che risponde subito di no.
        if risposta.stato == 404 {
            cadenza.riuscita();
            return Ok(None);
        }
        if !risposta.e_andata() {
            let errore = self.rete.stato_a_errore(&risposta, url);
            // Solo quel che si può ritentare conta come guasto del servizio: un
            // `400` è una query che abbiamo scritto male noi, e aprire
            // l'interruttore nasconderebbe il nostro difetto dietro «il
            // servizio è giù».
            if errore.is_retryable() {
                cadenza.guasto();
            } else {
                cadenza.riuscita();
            }
            return Err(errore);
        }

        cadenza.riuscita();
        Ok(Some(risposta.corpo))
    }

    /// Le pubblicazioni che potrebbero essere questo gruppo d'album.
    ///
    /// Si veda [`musicbrainz::pubblicazioni`] per come si restringe il campo
    /// prima di spendere una lettura per candidato.
    ///
    /// # Errori
    ///
    /// `metadata.musicbrainzUnavailable` quando MusicBrainz non risponde. Un
    /// catalogo che non ha quel disco restituisce invece un elenco vuoto: sono
    /// due esiti diversi e chi chiama li tratta in modo diverso.
    pub fn pubblicazioni(
        &self,
        titolo: &str,
        artista: &str,
        tracce: usize,
    ) -> Result<Vec<RemoteRelease>, AppError> {
        musicbrainz::pubblicazioni(self, titolo, artista, tracce)
    }

    /// I candidati per un singolo brano, da tutte e tre le fonti testuali.
    ///
    /// Non fallisce: una fonte caduta è un parere in meno, e i pareri in meno si
    /// contano in [`Esito::tutte_giu`] invece di far cadere la ricerca. Chi
    /// decide ha comunque bisogno del consenso fra due fonti, quindi un elenco
    /// costruito da una sola non porterà mai ad applicare niente.
    #[must_use]
    pub fn candidati(&self, titolo: &str, artista: &str) -> Esito {
        let mut candidati = Vec::new();
        let mut giu = 0_usize;
        let mut interrogate = 0_usize;

        for esito in [
            musicbrainz::registrazioni(self, titolo, artista),
            itunes::cerca(self, titolo, artista),
            deezer::cerca(self, titolo, artista),
        ] {
            interrogate = interrogate.saturating_add(1);
            match esito {
                Ok(trovati) => candidati.extend(trovati),
                Err(_) => giu = giu.saturating_add(1),
            }
        }

        Esito {
            candidati,
            tutte_giu: interrogate > 0 && giu == interrogate,
        }
    }

    /// La copertina, per la catena di ripiego descritta in [`copertine`].
    #[must_use]
    pub fn copertina(&self, sorgenti: &Sorgenti<'_>) -> Option<Copertina> {
        copertine::risolvi(self, sorgenti)
    }
}

/// Toglie da un termine i caratteri che Lucene interpreta dentro una frase.
///
/// MusicBrainz cerca con Lucene, e le uniche due cose che rompono una frase fra
/// virgolette sono le virgolette stesse e la barra rovesciata. Non si sfugge
/// niente altro di proposito: sfuggire l'intera sintassi di Lucene su un titolo
/// che ne contiene un pezzo per caso — un `+`, un `-`, delle parentesi —
/// produrrebbe query più letterali di quel che serve, e la ricerca deve essere
/// generosa perché a restringere ci pensa il punteggio.
#[must_use]
pub fn termine_lucene(grezzo: &str) -> String {
    grezzo
        .chars()
        .filter(|c| *c != '"' && *c != '\\')
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn un_termine_non_puo_spezzare_la_frase() {
        assert_eq!(termine_lucene(r#"Say "Hello""#), "Say Hello");
        assert_eq!(termine_lucene(r"AC\DC"), "ACDC");
        // Il resto della sintassi di Lucene resta: restringere tocca al
        // punteggio, non alla query.
        assert_eq!(
            termine_lucene("Sgt. Pepper's (Remix) + 1"),
            "Sgt. Pepper's (Remix) + 1"
        );
    }
}
