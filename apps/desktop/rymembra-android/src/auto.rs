//! Le domande dell'auto sulla libreria: che cosa c'è dentro un album, una
//! playlist, una ricerca.
//!
//! Android Auto sfoglia la libreria dal suo schermo, e lo chiede al servizio
//! della sessione (`MediaLibraryService`), in Kotlin. Kotlin non la conosce: la
//! libreria è il database di Rust. Le domande fanno quindi la strada dei gesti
//! — Kotlin le mette in una coda, un filo di qui le prende
//! ([`aspetta_richiesta`]) — e la risposta torna con [`rispondi`], che
//! completa la promessa che Kotlin ha consegnato a Media3.
//!
//! Il primo livello dell'albero — le quattro schede: recenti, playlist, album,
//! artisti — Kotlin lo costruisce da sé, con i nomi tradotti dalle risorse di
//! Android: l'auto lo chiede appena si collega, anche prima che il nucleo sia
//! partito. Da lì in giù è tutto di qui.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Errore, chiama};

/// Una domanda dell'auto.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub enum Richiesta {
    /// I figli di una voce, a pagine.
    #[serde(rename_all = "camelCase")]
    Figli {
        /// Il numero con cui rispondere.
        id: u64,
        /// La voce di cui si vogliono i figli.
        genitore: String,
        /// Quale pagina, da zero.
        pagina: u32,
        /// Quante voci per pagina.
        quante: u32,
    },
    /// Una voce sola, per identificativo.
    #[serde(rename_all = "camelCase")]
    Voce {
        /// Il numero con cui rispondere.
        id: u64,
        /// L'identificativo della voce.
        voce: String,
    },
    /// I risultati di una ricerca scritta sullo schermo dell'auto.
    #[serde(rename_all = "camelCase")]
    Cerca {
        /// Il numero con cui rispondere.
        id: u64,
        /// Quel che si cerca.
        testo: String,
        /// Quale pagina, da zero.
        pagina: u32,
        /// Quante voci per pagina.
        quante: u32,
    },
}

impl Richiesta {
    /// Il numero con cui rispondere.
    #[must_use]
    pub fn id(&self) -> u64 {
        match self {
            Self::Figli { id, .. } | Self::Voce { id, .. } | Self::Cerca { id, .. } => *id,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Attesa {
    entro_ms: u64,
}

/// Aspetta una domanda, al massimo per `entro`. `Ok(None)` se non ne arriva
/// nessuna.
///
/// Va chiamata da un filo che non ha altro da fare: blocca.
///
/// # Errori
///
/// Se il ponte non risponde, o se la domanda non ha la forma attesa.
pub fn aspetta_richiesta(entro: Duration) -> Result<Option<Richiesta>, Errore> {
    let entro_ms = u64::try_from(entro.as_millis()).unwrap_or(u64::MAX);
    chiama("auto.richiesta", &Attesa { entro_ms })
}

#[derive(Serialize)]
struct Risposta<'a, T> {
    id: u64,
    voci: &'a T,
}

/// Risponde alla domanda `id` con queste voci.
///
/// # Errori
///
/// Se il ponte non risponde. Una domanda a cui Kotlin ha già smesso di
/// aspettare — l'auto si è staccata — non è un errore: la risposta cade lì.
pub fn rispondi<T: Serialize>(id: u64, voci: &T) -> Result<(), Errore> {
    chiama("auto.risposta", &Risposta { id, voci })
}

#[derive(Serialize)]
struct Rifiuto<'a> {
    id: u64,
    errore: &'a str,
}

/// Risponde alla domanda `id` con un rifiuto: una voce che non esiste più, una
/// libreria che non si legge.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn rifiuta(id: u64, perche: &str) -> Result<(), Errore> {
    chiama("auto.risposta", &Rifiuto { id, errore: perche })
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn le_domande_si_leggono_come_le_scrive_kotlin() {
        let figli: Result<Richiesta, _> = serde_json::from_str(
            r#"{"tipo":"figli","id":4,"genitore":"album:x","pagina":1,"quante":50}"#,
        );
        assert_eq!(
            figli.as_ref().ok(),
            Some(&Richiesta::Figli {
                id: 4,
                genitore: "album:x".to_owned(),
                pagina: 1,
                quante: 50,
            })
        );
        assert_eq!(figli.map(|domanda| domanda.id()).ok(), Some(4));
        let cerca: Result<Richiesta, _> = serde_json::from_str(
            r#"{"tipo":"cerca","id":9,"testo":"queen","pagina":0,"quante":20}"#,
        );
        assert!(matches!(cerca, Ok(Richiesta::Cerca { id: 9, .. })));
    }
}
