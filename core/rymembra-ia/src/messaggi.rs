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
    /// Chi usa Rymembra.
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
    /// Le immagini che l'accompagnano, come indirizzi `data:`.
    ///
    /// # Perché indirizzi `data:` e non byte
    ///
    /// Perché è la forma che il protocollo vuole (`image_url.url`), e quella in
    /// cui la finestra le ha già: una tela disegnata su un `<canvas>` esce da
    /// `toDataURL`. Tenerle come byte vorrebbe dire decodificarle al confine
    /// per ricodificarle due righe dopo.
    ///
    /// Vuoto nella quasi totalità dei messaggi, e allora il contenuto parte
    /// come testo semplice: un servizio che non conosce le parti — un server
    /// locale vecchio — continua a capire tutto quel che capiva. Il controllo
    /// di forma e peso lo fa chi le riceve dalla finestra (vedi
    /// [`Self::immagine_ammessa`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub immagini: Vec<String>,
}

/// I tipi d'immagine che un messaggio può portare.
///
/// I tre che tutti e quattro i fornitori leggono. Una GIF animata o un SVG a un
/// modello non servono, e un tipo che un server rifiuta costerebbe un errore
/// su una richiesta che senza immagine sarebbe andata.
const TIPI_IMMAGINE: [&str; 3] = [
    "data:image/png;base64,",
    "data:image/jpeg;base64,",
    "data:image/webp;base64,",
];

/// Quanto può pesare un'immagine, codificata.
///
/// Otto megabyte di base64 sono sei di immagine: un'anteprima dello Studio in
/// JPEG ne pesa due o trecento kilobyte. Il tetto non è per quella, è per il
/// messaggio che arriva da una finestra che si è rotta — un indirizzo senza
/// fine che si manderebbe, e si pagherebbe, per intero.
pub const IMMAGINE_MASSIMA_MB: usize = 8;

/// [`IMMAGINE_MASSIMA_MB`] in byte, cioè in caratteri dell'indirizzo.
pub const IMMAGINE_MASSIMA: usize = IMMAGINE_MASSIMA_MB * 1024 * 1024;

impl Messaggio {
    /// Le istruzioni.
    #[must_use]
    pub fn sistema(testo: impl Into<String>) -> Self {
        Self {
            ruolo: Ruolo::Sistema,
            testo: testo.into(),
            immagini: Vec::new(),
        }
    }

    /// Una richiesta di chi usa Rymembra.
    #[must_use]
    pub fn utente(testo: impl Into<String>) -> Self {
        Self {
            ruolo: Ruolo::Utente,
            testo: testo.into(),
            immagini: Vec::new(),
        }
    }

    /// Una risposta del modello.
    #[must_use]
    pub fn modello(testo: impl Into<String>) -> Self {
        Self {
            ruolo: Ruolo::Modello,
            testo: testo.into(),
            immagini: Vec::new(),
        }
    }

    /// Questa battuta, con un'immagine in più.
    #[must_use]
    pub fn con_immagine(mut self, indirizzo: impl Into<String>) -> Self {
        self.immagini.push(indirizzo.into());
        self
    }

    /// L'indirizzo è un'immagine che si può mandare: uno dei tre tipi, in
    /// base64, entro [`IMMAGINE_MASSIMA`].
    ///
    /// Non decodifica: controlla la forma. Un base64 guasto lo rifiuta il
    /// servizio con un errore suo, e quel che qui interessa è che dalla
    /// finestra non esca verso la rete qualcosa che non è un'immagine.
    #[must_use]
    pub fn immagine_ammessa(indirizzo: &str) -> bool {
        indirizzo.len() <= IMMAGINE_MASSIMA
            && TIPI_IMMAGINE.iter().any(|tipo| {
                indirizzo.strip_prefix(tipo).is_some_and(|dati| {
                    !dati.is_empty()
                        && dati
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
                })
            })
    }

    /// Il campo `content` del protocollo.
    ///
    /// Un testo, quando non ci sono immagini — la forma che ogni servizio
    /// compatibile capisce da sempre. Con le immagini, l'elenco delle parti:
    /// prima il testo, poi un `image_url` per ognuna.
    #[must_use]
    pub fn contenuto(&self) -> serde_json::Value {
        if self.immagini.is_empty() {
            return serde_json::Value::String(self.testo.clone());
        }
        let mut parti = vec![serde_json::json!({ "type": "text", "text": self.testo })];
        parti.extend(
            self.immagini
                .iter()
                .map(|url| serde_json::json!({ "type": "image_url", "image_url": { "url": url } })),
        );
        serde_json::Value::Array(parti)
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

    /// Senza immagini il contenuto resta un testo: un server locale che non
    /// conosce le parti deve continuare a capire la conversazione.
    #[test]
    fn senza_immagini_il_contenuto_e_un_testo() {
        assert_eq!(
            Messaggio::utente("ciao").contenuto(),
            serde_json::json!("ciao")
        );
    }

    #[test]
    fn con_unimmagine_il_contenuto_e_un_elenco_di_parti() {
        let m = Messaggio::utente("guarda").con_immagine("data:image/png;base64,AAAA");
        assert_eq!(
            m.contenuto(),
            serde_json::json!([
                {"type": "text", "text": "guarda"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ])
        );
    }

    /// Dalla finestra un messaggio arriva senza il campo: deve leggersi.
    #[test]
    fn un_messaggio_senza_immagini_si_legge_e_si_scrive_come_prima() {
        let m: Messaggio = serde_json::from_str(r#"{"ruolo":"user","testo":"x"}"#).unwrap();
        assert!(m.immagini.is_empty());
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            r#"{"ruolo":"user","testo":"x"}"#
        );
    }

    #[test]
    fn unimmagine_si_ammette_solo_nella_forma_giusta() {
        assert!(Messaggio::immagine_ammessa(
            "data:image/jpeg;base64,/9j/4AAQ=="
        ));
        for cattiva in [
            "",
            "data:image/jpeg;base64,",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:image/png,<svg>",
            "https://esempio.it/a.png",
            "data:image/png;base64,AAA\"}],\"x\":\"",
        ] {
            assert!(!Messaggio::immagine_ammessa(cattiva), "«{cattiva}»");
        }
        let enorme = format!("data:image/png;base64,{}", "A".repeat(IMMAGINE_MASSIMA));
        assert!(!Messaggio::immagine_ammessa(&enorme));
    }

    #[test]
    fn una_fine_senza_conteggi_non_finge_uno_zero() {
        let fine = Fine::per(Motivo::Fermato);
        assert_eq!(fine.gettoni_in, None);
        assert_eq!(fine.gettoni_out, None);
    }
}
