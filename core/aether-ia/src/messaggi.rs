//! Cosa si manda a un modello, e cosa si ottiene quando ha finito.
//!
//! # Perché tre ruoli e non i quattro del protocollo
//!
//! Il protocollo ne conosce anche un quarto — `tool` — e qui non c'è, perché
//! qui non ci sono strumenti: le modifiche viaggiano dentro un blocco di testo
//! (vedi [`crate::operazioni`]), scelta presa perché i modelli locali piccoli
//! sbagliano il tool-calling molto più spesso di quanto sbaglino un blocco di
//! codice. Un ruolo che nessuno può produrre sarebbe una casella da riempire di
//! `unreachable!()`.
//!
//! # Perché la conversazione non sta qui
//!
//! Perché la tiene chi la mostra. Questo modulo definisce **un** messaggio; la
//! sequenza, i turni, quel che si taglia quando la finestra di contesto finisce
//! sono decisioni della schermata, e una struttura `Conversazione` in un crate
//! di nucleo sarebbe un secondo posto in cui esiste la stessa lista.

use serde::{Deserialize, Serialize};

/// Chi parla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ruolo {
    /// Le istruzioni, in testa e una volta sola.
    #[serde(rename = "system")]
    Sistema,
    /// Chi usa Aether.
    #[serde(rename = "user")]
    Utente,
    /// Il modello.
    #[serde(rename = "assistant")]
    Modello,
}

impl Ruolo {
    /// Il nome nel protocollo.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Sistema => "system",
            Self::Utente => "user",
            Self::Modello => "assistant",
        }
    }
}

/// Una battuta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Messaggio {
    /// Chi l'ha detta.
    pub ruolo: Ruolo,
    /// Cosa.
    pub testo: String,
}

impl Messaggio {
    /// Le istruzioni.
    #[must_use]
    pub fn sistema(testo: impl Into<String>) -> Self {
        Self {
            ruolo: Ruolo::Sistema,
            testo: testo.into(),
        }
    }

    /// Una richiesta di chi usa Aether.
    #[must_use]
    pub fn utente(testo: impl Into<String>) -> Self {
        Self {
            ruolo: Ruolo::Utente,
            testo: testo.into(),
        }
    }

    /// Una risposta del modello.
    #[must_use]
    pub fn modello(testo: impl Into<String>) -> Self {
        Self {
            ruolo: Ruolo::Modello,
            testo: testo.into(),
        }
    }
}

/// Da quale voce del modello arriva un pezzo di risposta.
///
/// # Perché due e non una
///
/// Perché i modelli che ragionano scrivono in due posti. Il protocollo di
/// OpenAI ha `delta.content`, e basta finché il modello risponde e basta; quelli
/// che pensano prima aggiungono `delta.reasoning` — OpenRouter, vLLM — oppure
/// `delta.reasoning_content` — DeepSeek, LM Studio, alcune build di Ollama — e
/// ci mettono dentro **tutto** il ragionamento, che su un modello grande dura
/// anche un minuto prima che in `content` arrivi il primo carattere.
///
/// Chi legge solo `content` ottiene un pannello che tace per un minuto: chi
/// guarda lo dà per rotto e preme «Ferma» molto prima della risposta. Chi le
/// mescola ottiene un blocco di modifiche con dentro il ragionamento su come
/// scriverlo, che non si applica. Servono distinte, e restano distinte fino alla
/// finestra, che le disegna in due posti diversi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voce {
    /// La risposta: quella che si legge, e da cui si estraggono le modifiche.
    Risposta,
    /// Il ragionamento, quando il modello lo mostra. Si guarda; non si
    /// interpreta, e non torna indietro al modello nel giro dopo.
    Pensiero,
}

/// Perché il modello ha smesso di parlare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Motivo {
    /// Aveva finito.
    Finito,
    /// Ha raggiunto il tetto di gettoni e si è fermato dov'era.
    ///
    /// Distinto da [`Self::Finito`] perché cambia cosa si può fare della
    /// risposta: un blocco di operazioni troncato a metà non si applica, e chi
    /// legge deve sapere che la colpa è della lunghezza e non del modello.
    Tagliato,
    /// Chi guarda ha premuto «Ferma».
    Fermato,
    /// Il flusso è finito senza dirlo.
    ///
    /// La connessione è caduta, o il servizio ha chiuso a metà frase. Quel che
    /// era arrivato resta leggibile; quel che manca non si sa quanto fosse.
    Troncato,
}

/// Com'è andata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fine {
    /// Perché ha smesso.
    pub motivo: Motivo,
    /// Quanti gettoni sono stati letti, se il servizio l'ha detto.
    ///
    /// `None` non è zero: Ollama e LM Studio non sempre mandano il conteggio,
    /// e mostrare uno zero al posto di «non l'ha detto» farebbe credere che una
    /// richiesta a pagamento sia stata gratis.
    pub gettoni_in: Option<u32>,
    /// Quanti ne ha scritti, se il servizio l'ha detto.
    pub gettoni_out: Option<u32>,
}

impl Fine {
    /// Una fine senza conteggi.
    #[must_use]
    pub const fn per(motivo: Motivo) -> Self {
        Self {
            motivo,
            gettoni_in: None,
            gettoni_out: None,
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I nomi dei ruoli sono protocollo, non stile: cambiarli vuol dire che il
    /// servizio non capisce più chi ha detto cosa.
    #[test]
    fn i_ruoli_si_scrivono_come_li_scrive_il_protocollo() {
        for (ruolo, atteso) in [
            (Ruolo::Sistema, "system"),
            (Ruolo::Utente, "user"),
            (Ruolo::Modello, "assistant"),
        ] {
            assert_eq!(ruolo.nome(), atteso);
            assert_eq!(
                serde_json::to_string(&ruolo).unwrap(),
                format!("\"{atteso}\"")
            );
        }
    }

    #[test]
    fn una_fine_senza_conteggi_non_finge_uno_zero() {
        let fine = Fine::per(Motivo::Fermato);
        assert_eq!(fine.gettoni_in, None);
        assert_eq!(fine.gettoni_out, None);
    }
}
