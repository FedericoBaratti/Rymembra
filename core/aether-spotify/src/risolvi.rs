//! La scala dei livelli, e la sola porta d'ingresso di questo crate.
//!
//! Tre modi di leggere lo stesso contenuto, dal più completo al più magro:
//!
//! 1. [`crate::pathfinder`] — l'API interna del lettore web. Elenchi completi e
//!    paginati, identificativi degli album, durate esatte. Richiede la stretta
//!    di mano. (L'ISRC no: vedi la nota in testa a quel modulo.)
//! 2. [`crate::embed`] — la pagina del riquadro incorporabile. Nessun gettone,
//!    ma gli elenchi lunghi possono arrivare tagliati.
//! 3. [`crate::oembed`] — titolo e copertina. Non si importa niente da qui: si
//!    fa vedere all'utente che cosa ha incollato.
//!
//! Vince il primo che risponde con qualcosa di utile. Gli altri non si provano.
//!
//! # Perché la degradazione è la funzionalità, non un ripiego
//!
//! Tutto quel che sta sotto dipende da punti interni non documentati che Spotify
//! cambia senza preavviso — e a febbraio 2026 ne ha cambiati parecchi in una
//! volta. Un lettore a un livello solo funziona finché non smette, e quando
//! smette non c'è niente da dire all'utente. Un lettore a tre livelli si degrada:
//! prima perde l'ISRC e la lunghezza esatta, poi perde i brani e resta
//! l'anteprima. In ognuno di quei tre stati l'applicazione sa dire dove si trova,
//! ed è quel che [`Diagnostica`] serve a raccontare.

use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::spotify::{SpotifyContent, SpotifySource};

use crate::config::{Configurazione, EsitoFile};
use crate::sessione::Sessione;
use crate::url::Riferimento;
use crate::{embed, oembed, pathfinder};

/// Un livello che non ha risposto, e perché.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallito {
    /// Quale livello.
    pub livello: SpotifySource,
    /// Cosa ha detto, in forma leggibile da chi sviluppa.
    pub perche: String,
}

/// Quel che si è letto, e cosa è costato leggerlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Risultato {
    /// Il contenuto.
    pub contenuto: SpotifyContent,
    /// I livelli provati prima, in ordine, che non hanno risposto.
    ///
    /// Non è decorazione: quando l'utente vede una playlist tagliata, questo
    /// elenco è la differenza fra «Spotify ha cambiato le impronte» e «il
    /// computer è offline».
    pub falliti: Vec<Fallito>,
}

/// Il lettore keyless.
///
/// Tiene la sessione — e quindi i gettoni — fra una lettura e l'altra: importare
/// tre playlist di fila fa una stretta di mano sola.
#[derive(Debug)]
pub struct Lettore {
    sessione: Sessione,
    esito_file: EsitoFile,
}

impl Lettore {
    /// Un lettore con la configurazione data.
    #[must_use]
    pub fn nuovo(configurazione: Configurazione) -> Self {
        Self {
            sessione: Sessione::nuova(configurazione),
            esito_file: EsitoFile::Assente,
        }
    }

    /// Un lettore che legge la configurazione dal file indicato, se c'è.
    ///
    /// Vedi [`crate::config`]: il file esiste per riparare una rotazione di
    /// Spotify senza ricompilare, e la sua assenza è il caso normale.
    #[must_use]
    pub fn dal_file(percorso: &std::path::Path) -> Self {
        let (configurazione, esito_file) = Configurazione::carica_riportando(percorso);
        Self {
            sessione: Sessione::nuova(configurazione),
            esito_file,
        }
    }

    /// Che cosa nomina quel che l'utente ha incollato.
    ///
    /// Prima [`crate::url::riconosci`], che non tocca niente e copre tutte le
    /// forme che si leggono da sole. Solo se quella non riconosce nulla **e** il
    /// testo contiene un link corto si va a chiedere alla rete dove porta: il
    /// caso normale non paga una richiesta per scoprire quel che sapeva già.
    ///
    /// `None` quando non è un link di Spotify — che è una risposta, non un
    /// guasto: chi chiama può dirlo all'utente invece di partire e fallire più
    /// avanti con un 404.
    #[must_use]
    pub fn riferimento(&self, input: &str) -> Option<Riferimento> {
        crate::url::riconosci(input)
            .or_else(|| crate::scorciatoia::espandi(self.sessione.rete(), input))
    }

    /// Legge un contenuto, scendendo di livello finché serve.
    ///
    /// # Errori
    ///
    /// `spotify.notPublic` se il contenuto non è pubblico — è l'unico caso in
    /// cui l'utente può fare qualcosa — e `spotify.resolveFailed` se nessuno dei
    /// tre livelli ha risposto.
    pub fn risolvi(&mut self, riferimento: &Riferimento) -> Result<Risultato, AppError> {
        let mut falliti = Vec::new();
        // Un «non è pubblico» detto da un livello qualsiasi è una risposta, non
        // un guasto: gli altri livelli direbbero la stessa cosa, e insistere
        // trasformerebbe un messaggio utile in «non si è riusciti a leggere».
        let mut non_pubblico = false;

        match pathfinder::risolvi(&mut self.sessione, riferimento) {
            Ok(contenuto) if utile(&contenuto) => {
                return Ok(Risultato { contenuto, falliti });
            }
            Ok(_) => falliti.push(Fallito {
                livello: SpotifySource::Pathfinder,
                perche: "ha risposto senza brani".to_owned(),
            }),
            Err(err) => {
                non_pubblico |= e_non_pubblico(&err);
                falliti.push(Fallito {
                    livello: SpotifySource::Pathfinder,
                    perche: err.to_string(),
                });
            }
        }

        match embed::risolvi(self.sessione.rete(), riferimento) {
            Ok(contenuto) if utile(&contenuto) => {
                return Ok(Risultato { contenuto, falliti });
            }
            Ok(_) => falliti.push(Fallito {
                livello: SpotifySource::Embed,
                perche: "ha risposto senza brani".to_owned(),
            }),
            Err(err) => {
                non_pubblico |= e_non_pubblico(&err);
                falliti.push(Fallito {
                    livello: SpotifySource::Embed,
                    perche: err.to_string(),
                });
            }
        }

        match oembed::risolvi(self.sessione.rete(), riferimento) {
            Ok(contenuto) => {
                return Ok(Risultato { contenuto, falliti });
            }
            Err(err) => {
                non_pubblico |= e_non_pubblico(&err);
                falliti.push(Fallito {
                    livello: SpotifySource::OEmbed,
                    perche: err.to_string(),
                });
            }
        }

        if non_pubblico {
            return Err(AppError::new(ErrorCode::SpotifyNotPublic));
        }
        Err(AppError::new(ErrorCode::SpotifyResolveFailed)
            .with_message("nessuno dei tre livelli ha risposto")
            .with_cause(riassunto(&falliti)))
    }

    /// La risposta di Pathfinder così com'è, senza interpretarla.
    ///
    /// Strumento di manutenzione, non parte del flusso: vedi
    /// [`crate::pathfinder::grezzo`]. Quando un campo diventa vuoto, questo è
    /// l'unico modo di sapere se è sparito o se si è solo spostato.
    ///
    /// # Errori
    ///
    /// Quelli di [`Self::risolvi`], senza però scendere agli altri due livelli:
    /// qui si sta guardando Pathfinder, e un ripiego nasconderebbe la risposta
    /// che si voleva vedere.
    pub fn grezzo(&mut self, riferimento: &Riferimento) -> Result<serde_json::Value, AppError> {
        pathfinder::grezzo(&mut self.sessione, riferimento)
    }

    /// La copertina di un contenuto, portata dentro come `data:` URI.
    ///
    /// `None` per qualunque intoppo: vedi [`crate::copertina`] sul perché
    /// un'anteprima senza immagine resta un'anteprima, e sul perché l'indirizzo
    /// non si passa alla finestra così com'è.
    #[must_use]
    pub fn copertina(&self, url: &str) -> Option<String> {
        crate::copertina::scarica(self.sessione.rete(), url)
    }

    /// Che aria tira, adesso.
    ///
    /// Prova solo la stretta di mano — non serve un contenuto vero — e riporta
    /// quel che sa della configurazione. È il comando che rende leggibile un
    /// guasto invece di lasciare all'utente «non funziona».
    pub fn diagnostica(&mut self) -> Diagnostica {
        let stretta = self.sessione.gettoni().err().map(|e| e.to_string());
        Diagnostica {
            stretta_di_mano: stretta,
            cifrari: self.sessione.configurazione().cifrari.len(),
            versioni_cifrari: self
                .sessione
                .configurazione()
                .cifrari
                .iter()
                .map(|c| c.versione)
                .collect(),
            file_configurazione: self.esito_file.clone(),
        }
    }
}

/// Lo stato del lettore, per la diagnostica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostica {
    /// `None` se la stretta di mano riesce, altrimenti perché no.
    pub stretta_di_mano: Option<String>,
    /// Quanti cifrari TOTP sono disponibili.
    pub cifrari: usize,
    /// Le loro versioni, in ordine di tentativo.
    pub versioni_cifrari: Vec<u32>,
    /// Cosa è successo al file di soprascrittura.
    pub file_configurazione: EsitoFile,
}

/// Un contenuto è utile se ha brani, o se è dichiaratamente vuoto.
///
/// La seconda parte non è un cavillo: una playlist appena creata ha davvero zero
/// brani, e trattarla come un guasto manderebbe il lettore a provare gli altri
/// due livelli per poi fallire su una risposta che era corretta.
fn utile(contenuto: &SpotifyContent) -> bool {
    !contenuto.tracks.is_empty() || contenuto.declared_total == Some(0)
}

fn e_non_pubblico(err: &AppError) -> bool {
    err.code().kind() == aether_domain::errors::ErrorCodeKind::SpotifyNotPublic
}

fn riassunto(falliti: &[Fallito]) -> String {
    falliti
        .iter()
        .map(|f| format!("{}: {}", f.livello.nome(), f.perche))
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::spotify::{SpotifyKind, SpotifyTrack};

    fn contenuto(quanti: usize, totale: Option<u32>) -> SpotifyContent {
        SpotifyContent {
            kind: SpotifyKind::Playlist,
            id: "x".to_owned(),
            title: "t".to_owned(),
            author: None,
            cover_url: None,
            tracks: vec![SpotifyTrack::default(); quanti],
            declared_total: totale,
            source: SpotifySource::Pathfinder,
        }
    }

    #[test]
    fn una_playlist_con_brani_e_utile() {
        assert!(utile(&contenuto(3, Some(3))));
    }

    #[test]
    fn una_playlist_dichiaratamente_vuota_e_una_risposta_non_un_guasto() {
        assert!(utile(&contenuto(0, Some(0))));
    }

    #[test]
    fn zero_brani_senza_un_totale_non_basta() {
        // Qui non si sa se la playlist è vuota o se il livello ha fallito in
        // silenzio: vale la pena scendere e chiedere a qualcun altro.
        assert!(!utile(&contenuto(0, None)));
        assert!(!utile(&contenuto(0, Some(120))));
    }

    #[test]
    fn il_riassunto_dice_quale_livello_ha_detto_cosa() {
        let falliti = vec![
            Fallito {
                livello: SpotifySource::Pathfinder,
                perche: "401".to_owned(),
            },
            Fallito {
                livello: SpotifySource::Embed,
                perche: "404".to_owned(),
            },
        ];
        assert_eq!(riassunto(&falliti), "pathfinder: 401 | embed: 404");
    }
}
