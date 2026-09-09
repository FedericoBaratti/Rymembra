//! Chi serve il modello, e le tre cose per cui i quattro differiscono.
//!
//! # Perché sono quattro nomi e non quattro protocolli
//!
//! Perché il protocollo è uno solo. OpenRouter, Ollama e il server locale di LM
//! Studio parlano tutti e tre lo stesso dialetto — `POST /chat/completions` con
//! `stream: true`, `GET /models` — e chi ne scrive un quarto lo scrive
//! compatibile con quello, perché è l'unico che le librerie sanno già parlare.
//!
//! Quel che cambia fra loro sta tutto in tre righe: **dove** rispondere,
//! **se** vogliono una chiave, e se stanno **su questa macchina**. È una
//! costante ciascuno, non un tratto con quattro implementazioni: un tratto qui
//! sarebbe quattro file per non scrivere tre `match`.
//!
//! # «Bionic» è LM Studio, e va detto qui
//!
//! Bionic è l'applicazione agent che LM Studio ha pubblicato nel luglio 2026.
//! Non sostituisce LM Studio e non espone un'API HTTP propria: quel che si
//! chiama da Aether è il **server locale di LM Studio** — scheda Developer, poi
//! Start server — che risponde su `http://localhost:1234/v1` parlando come
//! OpenAI. Il nome è quello che la gente cerca; l'indirizzo è quello che
//! funziona, ed è per costruzione lo stesso ramo di
//! [`Fornitore::Personalizzato`] con un valore iniziale diverso.

use serde::{Deserialize, Serialize};

/// Chi serve il modello.
/// I nomi serializzati sono scritti a mano e uguali a [`Fornitore::chiave`]:
/// `rename_all` darebbe `personalizzato` da una parte e `custom` dall'altra,
/// cioè due nomi per la stessa cosa a seconda di chi la scrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fornitore {
    /// Il rivenditore: una chiave sola per i modelli di tutti.
    #[serde(rename = "openrouter")]
    OpenRouter,
    /// Su questa macchina, porta 11434.
    #[serde(rename = "ollama")]
    Ollama,
    /// Su questa macchina, porta 1234: il server locale di LM Studio.
    #[serde(rename = "bionic")]
    Bionic,
    /// Qualunque altro punto OpenAI-compatibile.
    #[serde(rename = "custom")]
    Personalizzato,
}

impl Fornitore {
    /// Tutti, nell'ordine in cui si mostrano.
    ///
    /// OpenRouter per primo perché è l'unico che funziona senza installare
    /// niente; `Personalizzato` per ultimo perché è la casella «nessuno dei
    /// precedenti», e una casella così in cima invita a sceglierla per non
    /// leggere le altre.
    pub const ALL: &'static [Self] = &[
        Self::OpenRouter,
        Self::Ollama,
        Self::Bionic,
        Self::Personalizzato,
    ];

    /// Il nome che viaggia nei documenti e nelle chiavi.
    ///
    /// Stabile: finisce dentro `ia.profili` in `settings`, e cambiarlo
    /// renderebbe illeggibili i profili già salvati.
    #[must_use]
    pub const fn chiave(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Ollama => "ollama",
            Self::Bionic => "bionic",
            Self::Personalizzato => "custom",
        }
    }

    /// L'indirizzo che si propone quando si sceglie questo fornitore.
    ///
    /// Di serie, non definitivo: resta modificabile anche sui tre preset,
    /// perché Ollama si sposta di porta con una variabile d'ambiente e chi lo
    /// ha fatto sa perché.
    #[must_use]
    pub const fn url_di_serie(self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1",
            Self::Ollama => "http://localhost:11434/v1",
            Self::Bionic => "http://localhost:1234/v1",
            // Vuoto di proposito: un indirizzo suggerito qui sarebbe un
            // indirizzo che qualcuno lascia com'è senza guardarlo.
            Self::Personalizzato => "",
        }
    }

    /// Se senza una chiave non risponde.
    ///
    /// Solo OpenRouter. I due locali non ne vogliono una, e un campo chiave
    /// obbligatorio davanti a un servizio che non la guarda è un modo di far
    /// credere a qualcuno di aver sbagliato a incollarla.
    ///
    /// Un endpoint personalizzato **può** volerla, e infatti il campo si mostra
    /// lo stesso: quel che questa funzione dice è se il salvataggio si rifiuta
    /// di procedere senza, non se il campo esiste.
    #[must_use]
    pub const fn vuole_chiave(self) -> bool {
        matches!(self, Self::OpenRouter)
    }

    /// Se gira su questa macchina.
    ///
    /// Serve a due cose, e sono la ragione per cui è una domanda del fornitore
    /// e non dell'indirizzo: dire nella schermata che con questo profilo non
    /// esce niente da qui, e distinguere «il servizio non c'è» da «non c'è
    /// rete» quando la connessione viene rifiutata.
    #[must_use]
    pub const fn locale(self) -> bool {
        matches!(self, Self::Ollama | Self::Bionic)
    }

    /// Il fornitore che porta questo nome.
    ///
    /// Restituisce [`Self::Personalizzato`] per un nome sconosciuto invece di
    /// `None`: un profilo salvato da una versione futura non deve sparire
    /// dall'elenco — l'indirizzo e il modello ce li ha comunque scritti dentro,
    /// ed è tutto quello che serve per chiamarlo.
    #[must_use]
    pub fn da_chiave(nome: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|f| f.chiave() == nome)
            .unwrap_or(Self::Personalizzato)
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn ogni_nome_torna_al_suo_fornitore() {
        for atteso in Fornitore::ALL {
            assert_eq!(Fornitore::da_chiave(atteso.chiave()), *atteso);
        }
    }

    #[test]
    fn un_nome_sconosciuto_diventa_personalizzato() {
        assert_eq!(
            Fornitore::da_chiave("qualcosa-che-arriva-domani"),
            Fornitore::Personalizzato
        );
    }

    /// La riga che tiene in piedi la frase di `PRIVACY.md`: i due locali
    /// parlano in chiaro, e allora devono parlare con questo computer.
    #[test]
    fn in_chiaro_solo_i_locali_e_solo_verso_qui() {
        for fornitore in Fornitore::ALL {
            let url = fornitore.url_di_serie();
            if url.starts_with("http://") {
                assert!(
                    fornitore.locale(),
                    "{} propone un indirizzo in chiaro senza dichiararsi locale",
                    fornitore.chiave()
                );
                assert!(
                    aether_net::http::in_chiaro_ammesso(url),
                    "{url} non è questa macchina"
                );
            }
        }
    }

    #[test]
    fn la_chiave_la_vuole_solo_chi_sta_fuori() {
        assert!(Fornitore::OpenRouter.vuole_chiave());
        assert!(!Fornitore::Ollama.vuole_chiave());
        assert!(!Fornitore::Bionic.vuole_chiave());
        assert!(!Fornitore::Personalizzato.vuole_chiave());
    }
}
