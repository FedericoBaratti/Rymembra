//! I cataloghi liberi: leggere che cosa c'è, e prenderne i byte dove è permesso.
//!
//! Sostituisce `rymembra-yt`, e la differenza che conta non è quale servizio si
//! interroga. È che **da qui non si prende niente che non si possa prendere**:
//! ogni brano che esce da questo crate porta con sé la sua
//! [`rymembra_domain::esterno::Licenza`] e la sua
//! [`rymembra_domain::esterno::Disponibilita`], e [`Cataloghi::preleva`] rifiuta
//! di scrivere sul disco quel che le due insieme non consentono — prima di fare
//! la richiesta, non dopo — e quel che non sta in un indirizzo del catalogo che
//! il candidato dichiara.
//!
//! Il crate di prima non poteva avere quella proprietà, e non per come era
//! scritto: la fonte da cui prendeva non dichiara nessuna licenza, perché non
//! era una fonte da cui prendere.
//!
//! # Le due metà
//!
//! **Cercare** ([`cerca`]) — da un brano che qualcun altro ha nominato a un
//! elenco di file che potrebbero essere quello. È la metà che serve alla coda:
//! l'archivio di Spotify dice *cosa* si vuole e non dà nessun modo di sentirlo.
//!
//! **Leggere** ([`risolvi`], [`riferimento::riconosci`]) — da un link a un
//! elenco di brani. È la metà che serve a chi incolla l'indirizzo di un
//! concerto.
//!
//! Poi c'è [`prelievo`], che è il momento in cui i byte diventano un file.
//!
//! # Niente processi figli, niente binari
//!
//! Tutto passa da `rymembra-net`, cioè da `ureq`. Non c'è nessun eseguibile da
//! impacchettare nell'installer, nessuna cartella `resources/bin`, nessun
//! `std::process::Command`. Non è un'economia: un binario di terze parti dentro
//! un installer è una cosa che si distribuisce, e distribuire ha delle
//! conseguenze che il codice non può prevedere.
//!
//! # La regola di dipendenza
//!
//! ```text
//! rymembra-catalogo  ──►  rymembra-net  ──►  rymembra-domain
//! ```
//!
//! Qui dentro non passa mai una `rusqlite::Connection`, come in `rymembra-cloud` e
//! `rymembra-archivio`. È il verso della dipendenza a rendere impossibile — e non
//! solo sconsigliato — tenere preso il lucchetto della libreria per il minuto
//! che dura un prelievo. La differenza pesa più che altrove: una richiesta di
//! rete dura un secondo, un concerto in FLAC dura minuti.
//!
//! # Tutto bloccante, come il resto
//!
//! Nessun runtime asincrono — la ragione sta in `Cargo.toml:38-44` della radice.
//! L'annullamento non è un segnale ma una chiusura `Fn() -> bool` che chi chiama
//! collega al proprio `AtomicBool`, interrogata fra un blocco e l'altro.

pub mod archivio_org;
pub mod audius;
#[cfg(feature = "jamendo")]
pub mod jamendo;
pub mod prelievo;
pub mod riferimento;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::esterno::{BranoEsterno, ContenutoEsterno, Fonte};
use rymembra_domain::scelta::Candidato;
use rymembra_net::{Cancello, Rete, Rimandi};

pub use archivio_org::ArchivioOrg;
pub use audius::Audius;
#[cfg(feature = "jamendo")]
pub use jamendo::Jamendo;
pub use prelievo::{Prelevato, Richiesta, attribuzione, licenza_leggibile, spazza_parziali};
pub use riferimento::{
    Riferimento, bersaglio_ammesso, indirizzo_pubblico, nome_pubblico, riconosci,
};

/// Quanto può essere lunga una frase da cercare.
///
/// I motori di ricerca dei cataloghi hanno un tetto loro, e lo dicono male:
/// Audius risponde `too_many_clauses: maxClauseCount is set to 1024`, cioè con
/// il guasto interno del suo Elasticsearch, dentro un 500 che sembra un
/// servizio caduto. Una frase più lunga di questo tetto non è una ricerca che
/// non si può fare: è una ricerca che **non si deve spedire**, perché la
/// risposta è già nota e arriverebbe travestita da disservizio.
///
/// Duecentocinquanta caratteri sono otto volte il titolo più lungo fra quelli
/// che una ricerca vera porta — «beethoven symphony no. 5 [live]» ne ha trenta
/// — e restano largamente sotto il tetto di chi riceve.
pub const FRASE_MASSIMA: usize = 250;

/// Quanti rimandi può seguire un prelievo, o una richiesta di [`rimandi_pubblici`].
///
/// Gli stessi dieci che le reti seguivano da sé, ma adesso uno per volta e
/// solo dove il cancello dice di sì: l'Internet Archive rimanda al nodo che
/// tiene il file, Audius al nodo di contenuti, e nessuno dei due ha bisogno di
/// più di un paio di salti.
const RIMANDI_PRELIEVO: u32 = 10;

/// Il file di un item dell'Internet Archive: `https://archive.org/download/<id>/<file>`.
///
/// L'unica forma da cui un prelievo dall'Archive può partire. L'ospite passa
/// da [`riconosci`], lo stesso cancello dei link incollati e dei flussi; il
/// percorso dev'essere quello di un file, perché `details` o `metadata` sono
/// pagine e non brani.
fn file_dellarchivio(url: &str) -> bool {
    let Some(resto) = url.strip_prefix("https://") else {
        return false;
    };
    let percorso = resto
        .find('/')
        .and_then(|inizio| resto.get(inizio..))
        .unwrap_or_default();
    let pezzi: Vec<&str> = percorso
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    matches!(pezzi.as_slice(), ["download", _, _, ..])
        && riconosci(url).is_some_and(|r| r.fonte == Fonte::InternetArchive)
}

/// L'identificativo e il punto di un brano di Audius, da un percorso conservato
/// `/v1/tracks/<id>/download` o `/v1/tracks/<id>/stream`.
///
/// Il percorso senza nodo è la sola forma in cui Audius conserva un brano
/// (vedi `audius::indirizzo`): il nodo lo sceglie [`Audius::prepara`] al
/// momento, dall'elenco del catalogo. Un indirizzo completo, con un ospite già
/// scritto, non è un brano di Audius: è un ospite scelto da chi ha scritto la
/// riga.
fn brano_di_audius(percorso: &str) -> Option<(&str, &str)> {
    let senza_query = percorso.split(['?', '#']).next()?;
    let resto = senza_query.strip_prefix("/v1/tracks/")?;
    let (id, punto) = resto.split_once('/')?;
    (!id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric())
        && matches!(punto, "download" | "stream"))
    .then_some((id, punto))
}

/// Il no a un indirizzo che non è del catalogo dichiarato.
fn fuori_dal_catalogo(fonte: Fonte, titolo: &str) -> AppError {
    AppError::new(ErrorCode::DownloadUnrecognizedUrl).with_cause(format!(
        "l'indirizzo di «{titolo}» non è un file di {}: non si chiede",
        fonte.etichetta()
    ))
}

/// La stessa rete, con i rimandi seguiti a mano e solo verso un
/// [`indirizzo_pubblico`].
///
/// Per le richieste che partono da un indirizzo scelto da una risposta — le
/// domande ai nodi di Audius, le copertine — dove non c'è un'allowlist di
/// domini da far valere sui salti, ma c'è un confine: un rimando non deve poter
/// portare la richiesta al router o al NAS di casa. Scegliere un punto di
/// partenza pubblico non basta, se poi il punto di partenza rimanda dove vuole.
#[must_use]
pub fn rimandi_pubblici(rete: &Rete) -> Rete {
    rete.con_rimandi(Rimandi::Sorvegliati {
        quanti: RIMANDI_PRELIEVO,
        cancello: Cancello::nuovo(indirizzo_pubblico),
    })
}

/// La rete di un prelievo, con il cancello sui rimandi.
///
/// I rimandi si seguono a mano e solo verso dove [`bersaglio_ammesso`] dice di
/// sì, come per i flussi del lettore: un rimando che porta fuori dal catalogo
/// non riceve nemmeno la richiesta.
fn con_cancello(rete: &Rete, fonte: Fonte, partenza: &str) -> Rete {
    let partenza = partenza.to_owned();
    rete.con_rimandi(Rimandi::Sorvegliati {
        quanti: RIMANDI_PRELIEVO,
        cancello: Cancello::nuovo(move |bersaglio| bersaglio_ammesso(fonte, &partenza, bersaglio)),
    })
}

/// L'errore giusto per una ricerca che non è andata.
///
/// # Il difetto che chiude
///
/// C'era un solo esito per ogni ricerca mancata: `download.externalSearchFailed`,
/// che è **sempre ritentabile** e in finestra si legge «la ricerca sui cataloghi
/// non è riuscita, riprova». Ci finivano dentro due cose diverse.
///
/// Una è il catalogo che non risponde — rete giù, nodo caduto, 503 — e lì
/// riprovare è esattamente la cosa da fare.
///
/// L'altra è il catalogo che ha **respinto quella frase**: il guardiano che
/// risponde con una pagina di blocco invece che con la sua API, il motore di
/// ricerca che rifiuta una richiesta che non sa comporre. Lì riprovare le stesse
/// parole dà lo stesso rifiuto, all'infinito, e nel frattempo si accusa di
/// essere giù un servizio che sta benissimo.
///
/// # Come si distinguono
///
/// Non si indovina: lo ha già deciso chi ha letto la risposta. `rymembra-net`
/// classifica gli stati HTTP con `http_retryable` — un 4xx che non sia 408 o
/// 429 vuol dire «la richiesta era sbagliata», e da lì in giù l'errore porta
/// `is_retryable() == false`. Questa funzione non fa altro che **non buttare
/// via** quella distinzione mentre traduce il guasto della rete nel guasto
/// della ricerca, che è precisamente quel che si perdeva.
pub(crate) fn guasto_di_ricerca(catalogo: &str, err: &AppError, se_muto: &str) -> AppError {
    let detto = err.cause().unwrap_or(se_muto);
    // Il nome del catalogo sta nella causa e non in un campo del codice: i
    // campi non attraversano l'IPC, la causa sì — ed è la riga che la finestra
    // mostra sotto il messaggio, cioè l'unico posto in cui «chi» ha respinto
    // arriva a chi sta cercando.
    let causa = format!("{catalogo}: {detto}");
    if err.is_retryable() {
        return AppError::new(ErrorCode::DownloadExternalSearchFailed).with_cause(causa);
    }
    AppError::new(ErrorCode::DownloadSearchRejected).with_cause(causa)
}

/// Che cosa risponde, e che cosa no.
///
/// Serve alla finestra che spiega perché qualcosa non funziona. Un catalogo che
/// non risponde e un brano che non c'è sono due cose diverse, e senza questa
/// struttura si presentano a chi guarda con la stessa faccia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostica {
    /// L'Internet Archive risponde.
    pub internet_archive: bool,
    /// Audius risponde.
    pub audius: bool,
    /// Jamendo risponde.
    ///
    /// `false` anche quando manca il `client_id`, e anche quando la feature è
    /// spenta: dal punto di vista di chi guarda la diagnostica sono la stessa
    /// riga — «adesso non funziona» — e la ragione la dice la schermata che
    /// chiede la chiave, non questa colonna.
    pub jamendo: bool,
}

/// I cataloghi attivi, con la loro riserva di connessioni.
///
/// Uno per applicazione, tenuto vivo fra una passata e l'altra: costruirne uno
/// nuovo a ogni brano vorrebbe dire un saluto TLS per brano invece che uno per
/// sessione, contro un servizio pubblico che ci ospita gratis.
#[derive(Debug, Clone)]
pub struct Cataloghi {
    archivio: ArchivioOrg,
    audius: Audius,
    #[cfg(feature = "jamendo")]
    jamendo: Jamendo,
}

impl Default for Cataloghi {
    fn default() -> Self {
        Self::nuovi()
    }
}

impl Cataloghi {
    /// I cataloghi di serie.
    #[must_use]
    pub fn nuovi() -> Self {
        Self {
            archivio: ArchivioOrg::nuovo(),
            audius: Audius::nuovo(),
            #[cfg(feature = "jamendo")]
            jamendo: Jamendo::nuovo(),
        }
    }

    /// L'Internet Archive, per chi deve prelevare con la stessa rete.
    #[must_use]
    pub const fn archivio(&self) -> &ArchivioOrg {
        &self.archivio
    }

    /// Audius, per chi deve prelevare con la stessa rete.
    #[must_use]
    pub const fn audius(&self) -> &Audius {
        &self.audius
    }

    /// Jamendo, per chi deve leggerne il flusso o dargli la chiave.
    #[cfg(feature = "jamendo")]
    #[must_use]
    pub const fn jamendo(&self) -> &Jamendo {
        &self.jamendo
    }

    /// Che cosa risponde adesso.
    ///
    /// Fa richieste vere: è l'unico modo di distinguere «è giù» da «non ce l'ha»,
    /// e una diagnostica che non chiede niente non diagnostica niente.
    #[must_use]
    pub fn diagnostica(&self) -> Diagnostica {
        Diagnostica {
            internet_archive: self.archivio.risponde(),
            audius: self.audius.risponde(),
            #[cfg(feature = "jamendo")]
            jamendo: self.jamendo.risponde(),
            #[cfg(not(feature = "jamendo"))]
            jamendo: false,
        }
    }

    /// Il candidato da portare sul disco, a partire da quello ascoltato.
    ///
    /// Per l'Internet Archive è un'altra riga della stessa scheda: la ricerca
    /// libera sceglie il formato per l'ascolto, e la copia vuole l'originale —
    /// vedi [`ArchivioOrg::per_tenere`]. Per gli altri cataloghi è lo stesso
    /// candidato: Audius consegna un file solo per brano, e da Jamendo non si
    /// tiene niente.
    ///
    /// # Errori
    ///
    /// Quelli di [`ArchivioOrg::per_tenere`], che va in rete.
    pub fn per_tenere(&self, candidato: &Candidato) -> Result<Candidato, AppError> {
        match candidato.fonte {
            Fonte::InternetArchive => self.archivio.per_tenere(candidato),
            _ => Ok(candidato.clone()),
        }
    }

    /// I file che potrebbero essere il brano chiesto, da tutti i cataloghi.
    ///
    /// # Il guasto di uno, e i risultati dell'altro
    ///
    /// Finché un catalogo risponde, il guasto di un altro è un risultato in
    /// meno, non un fallimento — ma **non è nemmeno niente**: esce in
    /// [`Ricerca::guasti`]. Fino alla 3.2 si buttava via, e con l'Archive giù e
    /// Audius che rispondeva con otto brani da non tenere la coda dichiarava
    /// «introvabile», cioè terminale, un brano che stava solo sull'Archive.
    ///
    /// # Errori
    ///
    /// `internal.aborted` se chi ha chiesto ha annullato, anche a metà: un
    /// elenco parziale dopo un «Annulla» non è una risposta, e per la coda un
    /// elenco vuoto è «introvabile». Poi il primo guasto se **nessun** catalogo
    /// ha dato niente.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Ricerca, AppError> {
        let mut esiti = vec![self.archivio.cerca(brano, annullato)];

        // Audius **dopo** l'Internet Archive, e non è un ordine casuale: solo
        // una parte dei brani di Audius si può tenere, mentre quasi tutto quel
        // che sta nelle tre collezioni dell'Archive sì. `scegli_candidato`
        // giudica poi il mucchio intero — l'ordine non decide il vincitore —
        // ma l'annullamento fra i due sì: `Audius::cerca` lo guarda prima di
        // chiedere, e chi ferma la coda mentre il primo catalogo sta
        // rispondendo non paga anche il secondo.
        esiti.push(self.audius.cerca(brano, annullato).map(Ricerca::da));

        // Jamendo per ultimo, e senza chiave non chiede niente. I suoi
        // candidati non si potranno mai tenere — `puo_consegnare` è `false` —
        // quindi per la coda che procura sono zavorra; servono a chi cerca
        // qualcosa da **ascoltare**, ed è `scegli_candidato` a scartarli quando
        // la domanda era un'altra.
        #[cfg(feature = "jamendo")]
        if annullato() {
            esiti.push(Err(ricerca_annullata("Jamendo")));
        } else if self.jamendo.configurato() {
            esiti.push(self.jamendo.cerca(brano, annullato).map(Ricerca::da));
        }

        unisci(esiti)
    }

    /// I brani che rispondono a una frase scritta da una persona.
    ///
    /// L'altra metà di [`Self::cerca`]: là c'è un brano che qualcun altro ha
    /// nominato e si cerca *quello*, qui c'è quel che è stato scritto in una
    /// casella e non c'è niente con cui filtrare.
    ///
    /// # La scala, e perché non basta una passata sola
    ///
    /// Misurato sui due cataloghi veri, su tre frasi di natura diversa: né la
    /// frase esatta né i termini sciolti bastano da soli. «Broke For Free» dà
    /// zero risultati con le virgolette e sessantotto senza; «grateful dead
    /// 1977» ne dà dieci pertinenti con le virgolette e rumore senza. Si fa
    /// quindi quel che [`Self::cerca`] fa già per l'abbinamento — prima
    /// stretta, e la larga **solo** se la prima non ha dato niente — che è
    /// anche la scala che costa meno: la seconda richiesta si paga soltanto
    /// quando la prima è andata a vuoto.
    ///
    /// Su Audius la distinzione non esiste: il suo punto di ricerca prende una
    /// stringa e la tratta a modo suo, quindi una passata sola.
    ///
    /// # Quanto costa
    ///
    /// Da una a cinque richieste all'Archive — una ricerca più fino a quattro
    /// schede di item, il tetto che quel modulo si dà — e una ad Audius, raddoppiate
    /// nel caso peggiore in cui la passata stretta torni vuota. È il motivo per
    /// cui questa funzione va chiamata a **Invio** e non a ogni carattere
    /// digitato: gli archivi che ci ospitano sono gratuiti, e una ricerca per
    /// tasto premuto è il modo di farsi chiudere la porta.
    ///
    /// # Errori
    ///
    /// Come [`Self::cerca`]: solo se **nessun** catalogo ha risposto. Finché
    /// uno risponde, il guasto di un altro è un risultato in meno. La
    /// distinzione serve alla schermata, che deve poter dire «il catalogo non
    /// risponde» invece di «nessun risultato»: sono due frasi diverse, e chi
    /// legge la seconda al posto della prima smette di cercare.
    ///
    /// # E il catalogo che è giù mentre l'altro risponde
    ///
    /// Non è un errore, ma **non è nemmeno niente**: chi cerca un brano che sta
    /// solo su Audius, con Audius giù e l'Internet Archive che risponde, vede
    /// un elenco che sembra completo e non lo è. Per questo il guasto degli
    /// altri non si butta via: esce in [`RicercaLibera::muti`], e la schermata
    /// lo dice sopra l'elenco.
    pub fn cerca_libera(
        &self,
        testo: &str,
        annullato: &dyn Fn() -> bool,
    ) -> Result<RicercaLibera, AppError> {
        let testo = testo.trim();
        if testo.is_empty() {
            return Ok(RicercaLibera::default());
        }
        // Prima di spedire, non dopo aver ricevuto: una frase oltre il tetto
        // torna respinta da tutti e tre, ognuno con parole sue, e il risultato
        // è tre guasti di rete per una cosa che si sapeva di qua. Vedi
        // [`FRASE_MASSIMA`].
        let quanti = testo.chars().count();
        if quanti > FRASE_MASSIMA {
            return Err(
                AppError::new(ErrorCode::DownloadSearchRejected).with_cause(format!(
                    "la frase da cercare è di {quanti} caratteri, e il tetto è {FRASE_MASSIMA}"
                )),
            );
        }

        let mut trovati = Vec::new();
        let mut guasti: Vec<AppError> = Vec::new();
        let mut muti: Vec<Fonte> = Vec::new();

        match self.archivio.cerca_libera(testo, true, annullato) {
            Ok(suoi) if suoi.is_empty() && !annullato() => {
                match self.archivio.cerca_libera(testo, false, annullato) {
                    Ok(larghi) => trovati.extend(larghi),
                    Err(err) => {
                        guasti.push(err);
                        muti.push(Fonte::InternetArchive);
                    }
                }
            }
            Ok(suoi) => trovati.extend(suoi),
            Err(err) => {
                guasti.push(err);
                muti.push(Fonte::InternetArchive);
            }
        }

        if !annullato() {
            match self.audius.cerca_libera(testo) {
                Ok(suoi) => trovati.extend(suoi),
                Err(err) => {
                    guasti.push(err);
                    muti.push(Fonte::Audius);
                }
            }
        }

        #[cfg(feature = "jamendo")]
        if !annullato() && self.jamendo.configurato() {
            match self.jamendo.cerca_libera(testo) {
                Ok(suoi) => trovati.extend(suoi),
                Err(err) => {
                    guasti.push(err);
                    muti.push(Fonte::Jamendo);
                }
            }
        }

        if trovati.is_empty()
            && let Some(primo) = guasti.into_iter().next()
        {
            return Err(primo);
        }
        senza_doppioni(&mut trovati);
        Ok(RicercaLibera { trovati, muti })
    }

    /// Da un riferimento a un contenuto con i suoi brani.
    ///
    /// # Errori
    ///
    /// `catalogo.notPublic` se non c'è, `catalogo.resolveFailed` se c'è ma non
    /// se ne cava niente di ascoltabile, `download.unrecognizedUrl` per una
    /// fonte che non si legge da un link.
    pub fn risolvi(&self, riferimento: &Riferimento) -> Result<ContenutoEsterno, AppError> {
        match riferimento.fonte {
            Fonte::InternetArchive => self.archivio.risolvi(&riferimento.id),
            Fonte::Audius => self.audius.risolvi(riferimento),
            #[cfg(feature = "jamendo")]
            Fonte::Jamendo => self.jamendo.risolvi(riferimento),
            // Senza la feature Jamendo non c'è: il codice non è compilato, e
            // l'unica risposta onesta è che questo catalogo qui non funziona.
            #[cfg(not(feature = "jamendo"))]
            Fonte::Jamendo => Err(AppError::new(ErrorCode::CatalogoNotAvailable).with_cause(
                format!(
                    "{} non è compilato in questa versione di Rymembra",
                    riferimento.fonte.etichetta()
                ),
            )),
            Fonte::ArchivioSpotify | Fonte::FilePlaylist => {
                Err(AppError::new(ErrorCode::DownloadUnrecognizedUrl)
                    .with_cause("questa non è una fonte che si legga da un indirizzo".to_owned()))
            }
        }
    }

    /// Il candidato da prelevare per una riga che il file l'aveva già scelto,
    /// riletto **adesso** dal catalogo.
    ///
    /// # Perché si rilegge
    ///
    /// Perché di quel che sta in `desiderati` ci si può fidare per sapere
    /// *quale* file, e non per sapere *se* prenderlo. L'importazione da un link
    /// non ne salvava la licenza né la pagina, quindi il brano arrivava sul
    /// disco come «sconosciuta» e senza rimando; un profilo importato scrive
    /// indirizzo, licenza e disponibilità come vuole; e un artista di Audius
    /// può aver spento lo scarico dopo l'importazione. La scheda dell'item —
    /// o la traccia di Audius — dice oggi licenza, permesso e pagina, e sono
    /// quelli che viaggiano fino al file e alla riga della coda.
    ///
    /// L'indirizzo passa dallo stesso controllo di [`Self::preleva`] prima di
    /// qualunque richiesta.
    ///
    /// `Ok(None)` quando il file non c'è più — l'item non lo elenca, la traccia
    /// non esiste — e per le fonti da cui non si tiene niente: in tutti e due i
    /// casi il brano resta da comprare.
    ///
    /// # Errori
    ///
    /// `download.unrecognizedUrl` per un indirizzo che non è del catalogo
    /// dichiarato, senza nessuna richiesta; poi quelli della scheda o della
    /// traccia (`net.*`, `download.badResponse`).
    pub fn da_prelevare(&self, fonte: Fonte, url: &str) -> Result<Option<Candidato>, AppError> {
        match fonte {
            Fonte::InternetArchive if file_dellarchivio(url) => self.archivio.riletto(url),
            Fonte::InternetArchive => Err(fuori_dal_catalogo(fonte, "il brano")),
            Fonte::Audius => match brano_di_audius(url) {
                Some((id, _)) => self.audius.riletto(id),
                None => Err(fuori_dal_catalogo(fonte, "il brano")),
            },
            Fonte::Jamendo | Fonte::ArchivioSpotify | Fonte::FilePlaylist => Ok(None),
        }
    }

    /// Prende un candidato e lo scrive dove gli si dice.
    ///
    /// # Tre controlli, e tutti prima della rete
    ///
    /// Il permesso ([`Candidato::si_puo_tenere`]: disponibilità **e** licenza),
    /// poi l'indirizzo, poi i rimandi. L'indirizzo dev'essere del catalogo che
    /// il candidato dichiara: per l'Internet Archive un file sotto
    /// `archive.org/download/`, riconosciuto da [`riconosci`]; per Audius il
    /// percorso `/v1/tracks/<id>/download`, a cui il nodo lo mette
    /// [`Audius::prepara`]. Un indirizzo completo scritto in una riga di
    /// `desiderati` — da un profilo importato, per esempio — non parte: fino
    /// alla 3.2 bastava un `https://` qualunque con la disponibilità
    /// «scaricabile» per far scrivere nella cartella musica un file di un host
    /// qualunque. I rimandi si seguono solo dove [`bersaglio_ammesso`] dice di
    /// sì, come per i flussi.
    ///
    /// # Errori
    ///
    /// `download.notPermitted`, che non è un guasto: è un no.
    /// `download.unrecognizedUrl` per un indirizzo fuori dal catalogo. Poi
    /// quelli del trasferimento e del disco, e `download.invalidFiles` per un
    /// contenuto che non è audio.
    pub fn preleva(
        &self,
        candidato: &Candidato,
        richiesta: &Richiesta<'_>,
        annullato: &dyn Fn() -> bool,
        avanzamento: &mut dyn FnMut(f32),
    ) -> Result<Prelevato, AppError> {
        // Il rifiuto viene prima della rete, qui come in ogni altro catalogo:
        // `prepara` chiede l'elenco dei nodi, e farlo per un brano che stiamo
        // per rifiutare noi vorrebbe dire una richiesta uscita per niente.
        if !candidato.si_puo_tenere() {
            return Err(prelievo::non_si_tiene(candidato));
        }
        match candidato.fonte {
            Fonte::InternetArchive => {
                if !file_dellarchivio(&candidato.url) {
                    return Err(fuori_dal_catalogo(candidato.fonte, &candidato.titolo));
                }
                let rete = con_cancello(
                    self.archivio.rete_prelievo(),
                    Fonte::InternetArchive,
                    &candidato.url,
                );
                prelievo::preleva(&rete, candidato, richiesta, annullato, avanzamento)
            }
            // Audius passa da `prepara`, che è il momento in cui il percorso
            // conservato in tabella incontra un nodo che risponde **adesso**.
            Fonte::Audius => {
                if brano_di_audius(&candidato.url).is_none_or(|(_, punto)| punto != "download") {
                    return Err(fuori_dal_catalogo(candidato.fonte, &candidato.titolo));
                }
                let pronto = self.audius.prepara(candidato)?;
                let rete = con_cancello(self.audius.rete_prelievo(), Fonte::Audius, &pronto.url);
                prelievo::preleva(&rete, &pronto, richiesta, annullato, avanzamento)
            }
            altra => Err(AppError::new(ErrorCode::DownloadNotPermitted {
                licenza: Some(candidato.licenza.nome()),
            })
            .with_cause(format!(
                "da {} non si tiene niente sul disco",
                altra.etichetta()
            ))),
        }
    }
}

/// Quel che la ricerca di un brano ha trovato, e quel che non ha potuto
/// guardare.
///
/// La gemella di [`RicercaLibera`] per la coda, e per la stessa ragione: «non
/// c'è» e «non tutti hanno risposto» sono due esiti diversi. Qui il secondo
/// porta il guasto e non solo il nome del catalogo, perché la coda deve sapere
/// se riprovare: è [`AppError::is_retryable`] a dirlo.
#[derive(Debug, Clone, Default)]
pub struct Ricerca {
    /// I candidati, dai cataloghi che hanno risposto.
    pub trovati: Vec<Candidato>,
    /// I guasti di chi non ha risposto: un catalogo intero, o la scheda di un
    /// item dell'Internet Archive. Vuoto è il caso normale.
    pub guasti: Vec<AppError>,
}

impl Ricerca {
    /// Una risposta completa: questi candidati, e nessun guasto.
    #[must_use]
    pub const fn da(trovati: Vec<Candidato>) -> Self {
        Self {
            trovati,
            guasti: Vec::new(),
        }
    }
}

/// L'annullamento di una ricerca, che non è un guasto e non è un elenco vuoto.
pub(crate) fn ricerca_annullata(catalogo: &str) -> AppError {
    AppError::new(ErrorCode::InternalAborted {
        what: Some(format!("ricerca su {catalogo}")),
    })
}

/// Mette insieme quel che i cataloghi hanno risposto.
///
/// Un annullamento vince su tutto e torna com'è; un guasto si conserva accanto
/// ai risultati degli altri; e solo se non è arrivato niente da nessuno il
/// primo guasto diventa l'errore della ricerca.
fn unisci(esiti: Vec<Result<Ricerca, AppError>>) -> Result<Ricerca, AppError> {
    let mut tutto = Ricerca::default();
    for esito in esiti {
        match esito {
            Ok(parte) => {
                tutto.trovati.extend(parte.trovati);
                tutto.guasti.extend(parte.guasti);
            }
            Err(annullata) if matches!(annullata.code(), ErrorCode::InternalAborted { .. }) => {
                return Err(annullata);
            }
            Err(guasto) => tutto.guasti.push(guasto),
        }
    }
    if tutto.trovati.is_empty()
        && let Some(primo) = tutto.guasti.first()
    {
        return Err(primo.clone());
    }
    Ok(tutto)
}

/// Quel che una ricerca libera ha trovato, e chi non ha risposto.
///
/// Due campi e non un `Vec` perché la seconda informazione esisteva già e si
/// buttava via: [`Cataloghi::cerca_libera`] interroga più cataloghi, tiene
/// buono quel che arriva e fallisce solo se non arriva niente da nessuno. Nel
/// mezzo — uno risponde, l'altro è giù — la schermata mostrava un elenco che
/// sembrava completo senza modo di sapere che non lo era.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RicercaLibera {
    /// I risultati, deduplicati, nell'ordine in cui vanno mostrati.
    pub trovati: Vec<Candidato>,
    /// I cataloghi che non hanno risposto. Vuoto è il caso normale.
    pub muti: Vec<Fonte>,
}

/// Toglie dall'elenco lo stesso brano comparso più volte.
///
/// Serve **solo** alla ricerca libera, e la ragione è precisa: senza un titolo
/// atteso con cui filtrare, un item dell'Internet Archive restituisce lo stesso
/// pezzo in FLAC, in MP3 e nella sua versione a bitrate variabile. La deduplica
/// che il modulo dell'Archive fa già lavora sulla radice del nome del file, e
/// `gd77d1t01.flac` e `gd77d1t01_vbr.mp3` radici diverse ce le hanno — quindi
/// passano tutti e tre, e in una schermata di ricerca diventano tre righe
/// identiche.
///
/// La chiave comprende la **durata** e non solo il titolo, e non è un di più:
/// dentro un concerto due improvvisazioni si chiamano tutte e due «Jam», e
/// collassarle per solo titolo farebbe sparire della musica. Due file che sono
/// lo stesso brano durano invece lo stesso.
///
/// Si tiene il primo, che è il migliore: l'Archive ordina i suoi per qualità di
/// formato, e il primo di un gruppo è quello con la resa più alta.
fn senza_doppioni(candidati: &mut Vec<Candidato>) {
    let mut visti: std::collections::HashSet<(Fonte, String, String, Option<u32>)> =
        std::collections::HashSet::new();
    candidati.retain(|c| {
        visti.insert((
            c.fonte,
            rymembra_domain::text::fold_text(c.album.as_deref().unwrap_or_default()),
            rymembra_domain::text::fold_text(&c.titolo),
            c.durata_sec,
        ))
    });
}

#[cfg(test)]
mod prove {
    use super::*;
    use rymembra_domain::esterno::{Disponibilita, GenereContenuto, Licenza};

    #[test]
    fn da_audius_un_brano_di_solo_ascolto_si_rifiuta_senza_toccare_la_rete() {
        // Le prove girano senza rete, ed è esattamente ciò che rende questa
        // prova capace di dire qualcosa: se `preleva` chiedesse a Audius
        // l'elenco dei nodi prima di guardare il permesso, qui uscirebbe un
        // errore di trasporto invece del rifiuto. Il codice che torna è la
        // dimostrazione che nessuna richiesta è partita.
        let cataloghi = Cataloghi::nuovi();
        let temporanea = std::env::temp_dir().join("rymembra-prova-audius");
        let esito = cataloghi.preleva(
            &Candidato {
                fonte: Fonte::Audius,
                licenza: Licenza::TutteRiservate,
                disponibilita: Disponibilita::SoloAscolto,
                url: "/v1/tracks/aB3/stream".to_owned(),
                titolo: "una canzone".to_owned(),
                ..Candidato::default()
            },
            &Richiesta {
                cartella_download: &temporanea,
                cartella_temporanea: &temporanea,
                base_relativa: "A/B/01 - C",
            },
            &|| false,
            &mut |_| {},
        );
        assert!(
            matches!(
                esito.as_ref().map_err(AppError::code),
                Err(ErrorCode::DownloadNotPermitted { .. })
            ),
            "invece di rifiutare ha risposto: {:?}",
            esito.map(|_| ()).map_err(|e| e.code().kind().code())
        );
    }

    #[test]
    fn da_un_catalogo_di_solo_ascolto_non_si_preleva() {
        let cataloghi = Cataloghi::nuovi();
        let temporanea = std::env::temp_dir().join("rymembra-prova-catalogo");
        let esito = cataloghi.preleva(
            &Candidato {
                fonte: Fonte::Jamendo,
                licenza: Licenza::CreativeCommons("by".to_owned()),
                disponibilita: Disponibilita::SoloAscolto,
                url: "https://prod-1.storage.jamendo.com/x.mp3".to_owned(),
                ..Candidato::default()
            },
            &Richiesta {
                cartella_download: &temporanea,
                cartella_temporanea: &temporanea,
                base_relativa: "A/B/01 - C",
            },
            &|| false,
            &mut |_| {},
        );
        assert!(matches!(
            esito.as_ref().map_err(AppError::code),
            Err(ErrorCode::DownloadNotPermitted { .. })
        ));
    }

    /// I cataloghi di serie, con Audius già su un nodo che non risponde: una
    /// porta chiusa su questa macchina. Se una richiesta partisse comunque, si
    /// vedrebbe come un errore di trasporto, senza uscire di casa.
    fn cataloghi_senza_rete() -> Cataloghi {
        Cataloghi {
            audius: Audius::con_nodo("https://127.0.0.1:9"),
            ..Cataloghi::nuovi()
        }
    }

    fn tenibile(fonte: Fonte, url: &str) -> Candidato {
        Candidato {
            fonte,
            licenza: Licenza::CreativeCommons("by".to_owned()),
            disponibilita: Disponibilita::Scaricabile,
            url: url.to_owned(),
            titolo: "Intro".to_owned(),
            ..Candidato::default()
        }
    }

    /// Un `.ryprofile` può scrivere in `desiderati` un indirizzo qualunque con
    /// la disponibilità «scaricabile»: il prelievo deve rifiutarlo **prima**
    /// della rete se l'ospite non è del catalogo che la riga dichiara.
    #[test]
    fn un_indirizzo_fuori_dal_catalogo_non_si_preleva() {
        let cataloghi = cataloghi_senza_rete();
        let temporanea = std::env::temp_dir().join("rymembra-prova-fuori-catalogo");
        let _ = std::fs::remove_dir_all(&temporanea);
        for (fonte, url) in [
            (
                Fonte::InternetArchive,
                "https://127.0.0.1:9/download/x/x.url",
            ),
            (
                Fonte::InternetArchive,
                "http://archive.org/download/x/t01.flac",
            ),
            (Fonte::Audius, "https://127.0.0.1:9/v1/tracks/aB3/download"),
            (Fonte::Audius, "/v1/tracks/aB3/stream"),
            (Fonte::Audius, "/v1/users/aB3/download"),
            (Fonte::Audius, "/v1/tracks/aB3/download/../../x"),
        ] {
            let esito = cataloghi.preleva(
                &tenibile(fonte, url),
                &Richiesta {
                    cartella_download: &temporanea,
                    cartella_temporanea: &temporanea,
                    base_relativa: "A/B/01 - Intro",
                },
                &|| false,
                &mut |_| {},
            );
            assert!(
                matches!(
                    esito.as_ref().map_err(AppError::code),
                    Err(ErrorCode::DownloadUnrecognizedUrl)
                ),
                "«{url}»: invece di rifiutare ha risposto {:?}",
                esito.map(|_| ()).map_err(|e| e.code().kind().code())
            );
        }
        assert!(
            !temporanea.exists(),
            "non doveva nemmeno creare la cartella"
        );
    }

    fn candidato_di(fonte: Fonte, titolo: &str) -> Candidato {
        Candidato {
            fonte,
            titolo: titolo.to_owned(),
            url: format!("/v1/tracks/{titolo}/stream"),
            ..Candidato::default()
        }
    }

    #[test]
    fn un_catalogo_muto_non_rende_completo_quel_che_dice_laltro() {
        // L'Archive risponde 503, Audius risponde con un brano che non si può
        // tenere: il risultato porta quel brano **e** il guasto, perché per la
        // coda «non c'è» sarebbe terminale e qui non lo si sa.
        let caduto = AppError::new(ErrorCode::NetHttp {
            status: 503,
            url: None,
        });
        let esito = unisci(vec![
            Err(caduto),
            Ok(Ricerca::da(vec![candidato_di(Fonte::Audius, "x")])),
        ])
        .expect("Audius ha risposto");
        assert_eq!(esito.trovati.len(), 1);
        assert_eq!(esito.guasti.len(), 1);
        assert!(esito.guasti.iter().all(AppError::is_retryable));

        // La scheda di un item che non risponde vale lo stesso.
        let parziale = Ricerca {
            trovati: vec![candidato_di(Fonte::InternetArchive, "y")],
            guasti: vec![AppError::new(ErrorCode::NetHttp {
                status: 502,
                url: None,
            })],
        };
        let esito = unisci(vec![Ok(parziale), Ok(Ricerca::default())]).expect("risposte");
        assert_eq!(esito.guasti.len(), 1);

        // Tutti giù: il primo guasto è l'errore.
        let tutti = unisci(vec![
            Err(AppError::new(ErrorCode::NetHttp {
                status: 503,
                url: None,
            })),
            Err(AppError::new(ErrorCode::DownloadExternalSearchFailed)),
        ]);
        assert!(matches!(
            tutti.as_ref().map_err(AppError::code),
            Err(ErrorCode::NetHttp { status: 503, .. })
        ));

        // E tutti che rispondono «niente» è un niente vero.
        let niente =
            unisci(vec![Ok(Ricerca::default()), Ok(Ricerca::default())]).expect("nessun guasto");
        assert!(niente.trovati.is_empty() && niente.guasti.is_empty());
    }

    #[test]
    fn un_annullamento_vince_sui_risultati() {
        let esito = unisci(vec![
            Ok(Ricerca::da(vec![candidato_di(Fonte::InternetArchive, "y")])),
            Err(ricerca_annullata("Audius")),
        ]);
        assert!(matches!(
            esito.as_ref().map_err(AppError::code),
            Err(ErrorCode::InternalAborted { .. })
        ));
        // E senza rete: l'annullamento si guarda prima di ogni richiesta.
        let brano = BranoEsterno {
            title: "Karma Police".to_owned(),
            artist: Some("Radiohead".to_owned()),
            ..BranoEsterno::default()
        };
        let esito = cataloghi_senza_rete().cerca(&brano, &|| true);
        assert!(matches!(
            esito.as_ref().map_err(AppError::code),
            Err(ErrorCode::InternalAborted { .. })
        ));
    }

    #[test]
    fn i_rimandi_pubblici_non_portano_a_casa() {
        // La rete con cui `procura` scarica una copertina: partire da un nome
        // pubblico non basta se un rimando può portare la richiesta al router.
        let rete = rimandi_pubblici(&Rete::nuova("prova", std::time::Duration::from_secs(1)));
        let Rimandi::Sorvegliati { cancello, .. } = rete.rimandi() else {
            panic!("i rimandi non sono sorvegliati");
        };
        assert!(cancello.ammette("https://ia800305.us.archive.org/services/img/x"));
        assert!(!cancello.ammette("https://192.168.1.1/x.jpg"));
        assert!(!cancello.ammette("http://ia800305.us.archive.org/x.jpg"));
    }

    #[test]
    fn la_forma_degli_indirizzi_del_catalogo() {
        assert!(file_dellarchivio(
            "https://archive.org/download/gd77/gd77d1t01.flac"
        ));
        assert!(file_dellarchivio(
            "https://archive.org/download/gd77/sotto/cartella.mp3?x=1"
        ));
        for no in [
            "https://archive.org/details/gd77",
            "https://archive.org/download/gd77",
            "http://archive.org/download/gd77/t01.flac",
            "https://evilarchive.org/download/gd77/t01.flac",
            "https://archive.org@esempio.com/download/gd77/t01.flac",
            "https://127.0.0.1/download/gd77/t01.flac",
        ] {
            assert!(!file_dellarchivio(no), "su «{no}»");
        }
        assert_eq!(
            brano_di_audius("/v1/tracks/aB3/download?ext=wav"),
            Some(("aB3", "download"))
        );
        assert_eq!(
            brano_di_audius("/v1/tracks/aB3/stream"),
            Some(("aB3", "stream"))
        );
        for no in [
            "https://nodo.esempio.com/v1/tracks/aB3/download",
            "/v1/users/aB3/download",
            "/v1/tracks//download",
            "/v1/tracks/a%2Fb/download",
            "/v1/tracks/aB3/download/altro",
        ] {
            assert_eq!(brano_di_audius(no), None, "su «{no}»");
        }
    }

    #[test]
    fn una_riga_da_riverificare_fuori_dal_catalogo_non_si_chiede() {
        // La rilettura passa dallo stesso controllo del prelievo, prima di
        // qualunque richiesta.
        let cataloghi = cataloghi_senza_rete();
        for (fonte, url) in [
            (
                Fonte::InternetArchive,
                "https://127.0.0.1:9/download/x/x.url",
            ),
            (Fonte::Audius, "https://127.0.0.1:9/v1/tracks/aB3/download"),
        ] {
            assert!(
                matches!(
                    cataloghi
                        .da_prelevare(fonte, url)
                        .as_ref()
                        .map_err(AppError::code),
                    Err(ErrorCode::DownloadUnrecognizedUrl)
                ),
                "su «{url}»"
            );
        }
        // Da una fonte che non consegna niente non c'è niente da rileggere: il
        // brano resta da comprare.
        assert!(matches!(
            cataloghi.da_prelevare(Fonte::ArchivioSpotify, "https://127.0.0.1:9/x"),
            Ok(None)
        ));
    }

    #[test]
    fn una_ricerca_caduta_si_ritenta_e_una_respinta_no() {
        // Il servizio che non risponde: un 503, che `rymembra-net` dichiara
        // ritentabile. Chi cerca deve vedere «riprova».
        let caduto = AppError::new(ErrorCode::NetHttp {
            status: 503,
            url: None,
        });
        let guasto = guasto_di_ricerca("Audius", &caduto, "muto");
        assert_eq!(guasto.code().kind().code(), "download.externalSearchFailed");
        assert!(guasto.is_retryable());

        // Il guardiano che respinge: un 403. Le stesse parole daranno lo stesso
        // rifiuto, quindi «riprova» è un tasto che non può funzionare.
        let respinto = AppError::new(ErrorCode::NetHttp {
            status: 403,
            url: None,
        })
        .with_cause("<html><title>Attention Required! | Cloudflare</title>");
        let guasto = guasto_di_ricerca("Audius", &respinto, "muto");
        assert_eq!(guasto.code().kind().code(), "download.searchRejected");
        assert!(!guasto.is_retryable());
        // E chi ha respinto si sa, perché la causa attraversa l'IPC e i campi
        // del codice no.
        assert!(guasto.cause().is_some_and(|c| c.starts_with("Audius: ")));
    }

    #[test]
    fn una_frase_piu_lunga_del_tetto_non_parte_nemmeno() {
        // Senza rete: il tetto si applica prima di qualunque richiesta, ed è
        // per questo che questa prova può girare senza toccare niente.
        let cataloghi = Cataloghi::nuovi();
        let esito = cataloghi.cerca_libera(&"a".repeat(FRASE_MASSIMA + 1), &|| false);
        let guasto = esito.expect_err("respinta");
        assert_eq!(guasto.code().kind().code(), "download.searchRejected");
        assert!(!guasto.is_retryable());
        // E il tetto è un tetto, non una soglia di sospetto: una ricerca vera
        // ci sta dentro con otto volte di margine.
        assert!("grateful dead 1977 cornell".chars().count() * 8 <= FRASE_MASSIMA);
    }

    #[test]
    fn una_fonte_che_non_si_legge_da_un_link_lo_dice() {
        let cataloghi = Cataloghi::nuovi();
        let esito = cataloghi.risolvi(&Riferimento {
            fonte: Fonte::ArchivioSpotify,
            genere: GenereContenuto::Playlist,
            id: "x".to_owned(),
        });
        assert!(matches!(
            esito.as_ref().map_err(AppError::code),
            Err(ErrorCode::DownloadUnrecognizedUrl)
        ));
    }
}
