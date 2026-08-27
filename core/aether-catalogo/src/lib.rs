//! I cataloghi liberi: leggere che cosa c'è, e prenderne i byte dove è permesso.
//!
//! Sostituisce `aether-yt`, e la differenza che conta non è quale servizio si
//! interroga. È che **da qui non si prende niente che non si possa prendere**:
//! ogni brano che esce da questo crate porta con sé la sua
//! [`aether_domain::esterno::Licenza`] e la sua
//! [`aether_domain::esterno::Disponibilita`], e [`prelievo::preleva`] rifiuta di
//! scrivere sul disco quel che la seconda non consente — prima di fare la
//! richiesta, non dopo.
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
//! Tutto passa da `aether-net`, cioè da `ureq`. Non c'è nessun eseguibile da
//! impacchettare nell'installer, nessuna cartella `resources/bin`, nessun
//! `std::process::Command`. Non è un'economia: un binario di terze parti dentro
//! un installer è una cosa che si distribuisce, e distribuire ha delle
//! conseguenze che il codice non può prevedere.
//!
//! # La regola di dipendenza
//!
//! ```text
//! aether-catalogo  ──►  aether-net  ──►  aether-domain
//! ```
//!
//! Qui dentro non passa mai una `rusqlite::Connection`, come in `aether-cloud` e
//! `aether-archivio`. È il verso della dipendenza a rendere impossibile — e non
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

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{BranoEsterno, ContenutoEsterno, Fonte};
use aether_domain::scelta::Candidato;

pub use archivio_org::ArchivioOrg;
pub use audius::Audius;
#[cfg(feature = "jamendo")]
pub use jamendo::Jamendo;
pub use prelievo::{Prelevato, Richiesta, attribuzione, preleva};
pub use riferimento::{Riferimento, riconosci};

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

    /// I file che potrebbero essere il brano chiesto, da tutti i cataloghi.
    ///
    /// # Errori
    ///
    /// Solo se **nessun** catalogo ha risposto: finché uno risponde, il guasto
    /// di un altro è un risultato in meno, non un fallimento. Con un catalogo
    /// solo attivo le due cose coincidono; con più di uno la differenza è fra
    /// «non l'ho trovato» e «sono tutti giù», e sono due frasi diverse.
    pub fn cerca(
        &self,
        brano: &BranoEsterno,
        annullato: &dyn Fn() -> bool,
    ) -> Result<Vec<Candidato>, AppError> {
        let mut trovati = Vec::new();
        let mut guasti: Vec<AppError> = Vec::new();

        match self.archivio.cerca(brano, annullato) {
            Ok(suoi) => trovati.extend(suoi),
            Err(err) => guasti.push(err),
        }

        // Audius **dopo** l'Internet Archive, e non è un ordine casuale: solo
        // una parte dei brani di Audius si può tenere, mentre quasi tutto quel
        // che sta nelle tre collezioni dell'Archive sì. `scegli_candidato`
        // giudica poi il mucchio intero — l'ordine non decide il vincitore —
        // ma `annullato()` fra i due sì: chi ferma la coda mentre il primo
        // catalogo sta rispondendo non paga anche il secondo.
        if !annullato() {
            match self.audius.cerca(brano, annullato) {
                Ok(suoi) => trovati.extend(suoi),
                Err(err) => guasti.push(err),
            }
        }

        // Jamendo per ultimo, e senza chiave non chiede niente. I suoi
        // candidati non si potranno mai tenere — `puo_consegnare` è `false` —
        // quindi per la coda che procura sono zavorra; servono a chi cerca
        // qualcosa da **ascoltare**, ed è `scegli_candidato` a scartarli quando
        // la domanda era un'altra.
        #[cfg(feature = "jamendo")]
        if !annullato() && self.jamendo.configurato() {
            match self.jamendo.cerca(brano, annullato) {
                Ok(suoi) => trovati.extend(suoi),
                Err(err) => guasti.push(err),
            }
        }

        if trovati.is_empty()
            && let Some(primo) = guasti.into_iter().next()
        {
            return Err(primo);
        }
        Ok(trovati)
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
                    "{} non è compilato in questa versione di Aether",
                    riferimento.fonte.etichetta()
                ),
            )),
            Fonte::ArchivioSpotify | Fonte::FilePlaylist => {
                Err(AppError::new(ErrorCode::DownloadUnrecognizedUrl)
                    .with_cause("questa non è una fonte che si legga da un indirizzo".to_owned()))
            }
        }
    }

    /// Prende un candidato e lo scrive dove gli si dice.
    ///
    /// # Errori
    ///
    /// Vedi [`prelievo::preleva`]. In particolare `download.notPermitted`,
    /// che non è un guasto: è un no.
    pub fn preleva(
        &self,
        candidato: &Candidato,
        richiesta: &Richiesta<'_>,
        annullato: &dyn Fn() -> bool,
        avanzamento: &mut dyn FnMut(f32),
    ) -> Result<Prelevato, AppError> {
        match candidato.fonte {
            Fonte::InternetArchive => prelievo::preleva(
                self.archivio.rete(),
                candidato,
                richiesta,
                annullato,
                avanzamento,
            ),
            // Audius passa da `prepara`, che è il momento in cui il percorso
            // conservato in tabella incontra un nodo che risponde **adesso**.
            //
            // Ma solo se c'è qualcosa da prendere. `prepara` chiede l'elenco
            // dei nodi — cioè tocca la rete — e farlo prima che `preleva`
            // guardi `si_puo_tenere()` vorrebbe dire una richiesta uscita per
            // un brano che stiamo per rifiutare noi. Il rifiuto viene prima
            // della rete, qui come in ogni altro catalogo.
            Fonte::Audius => {
                let pronto = if candidato.si_puo_tenere() {
                    self.audius.prepara(candidato)?
                } else {
                    candidato.clone()
                };
                prelievo::preleva(
                    self.audius.rete(),
                    &pronto,
                    richiesta,
                    annullato,
                    avanzamento,
                )
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

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::esterno::{Disponibilita, GenereContenuto, Licenza};

    #[test]
    fn da_audius_un_brano_di_solo_ascolto_si_rifiuta_senza_toccare_la_rete() {
        // Le prove girano senza rete, ed è esattamente ciò che rende questa
        // prova capace di dire qualcosa: se `preleva` chiedesse a Audius
        // l'elenco dei nodi prima di guardare il permesso, qui uscirebbe un
        // errore di trasporto invece del rifiuto. Il codice che torna è la
        // dimostrazione che nessuna richiesta è partita.
        let cataloghi = Cataloghi::nuovi();
        let temporanea = std::env::temp_dir().join("aether-prova-audius");
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
        let temporanea = std::env::temp_dir().join("aether-prova-catalogo");
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
