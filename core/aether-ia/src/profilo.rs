//! Un modello configurato, con tutto quel che serve per chiamarlo — tranne la
//! chiave.
//!
//! # Perché la chiave non è un campo
//!
//! Perché un [`Profilo`] viaggia: verso la finestra a ogni apertura delle
//! impostazioni, dentro `settings` come JSON, e in un `Debug` se un giorno
//! qualcuno ne stampa uno per capire cosa non torna. Un segreto che viaggia in
//! tre posti è un segreto in tre posti, e due di quei tre sono su disco in
//! chiaro.
//!
//! Quel che il profilo porta è [`Profilo::con_chiave`] — una risposta sì/no — e
//! il nome della voce di portachiavi, che si ricava dall'id
//! ([`Profilo::voce_portachiavi`]). Il valore lo va a prendere chi sta per fare
//! la richiesta, un istante prima di farla.
//!
//! # Perché l'id non è il nome
//!
//! Perché il nome si cambia. «Claude» diventa «Claude (buono)» il giorno in cui
//! se ne aggiunge un secondo, e se l'id fosse il nome quel gesto lascerebbe nel
//! Credential Manager una voce intestata a un profilo che non esiste più —
//! senza che niente, da nessuna parte, sappia più a chi apparteneva.

use aether_domain::errors::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

use crate::fornitore::Fornitore;

/// Quanto può essere lungo un id derivato da un nome.
///
/// Trenta caratteri: un id è una chiave, non un titolo, e la parte che
/// distingue due profili sta sempre nelle prime parole. Il taglio serve a non
/// scrivere nel Credential Manager una voce lunga come una frase.
const LUNGHEZZA_ID: usize = 30;

/// Un modello configurato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profilo {
    /// Stabile per tutta la vita del profilo, e mai riusato.
    pub id: String,
    /// Come lo chiama chi l'ha scritto.
    pub nome: String,
    /// Chi serve il modello.
    pub fornitore: Fornitore,
    /// L'indirizzo di base, senza la barra finale e senza il percorso.
    ///
    /// Modificabile anche sui preset: vedi [`Fornitore::url_di_serie`].
    pub url_base: String,
    /// Il nome del modello, come lo scrive il fornitore.
    pub modello: String,
    /// Se una chiave per questo profilo sta nel portachiavi.
    ///
    /// Una risposta, non il valore. Chi la trova `false` su un fornitore che
    /// [`Fornitore::vuole_chiave`] sa già come finirà la richiesta, e può dirlo
    /// prima di farla.
    pub con_chiave: bool,
}

impl Profilo {
    /// La voce di portachiavi che tiene la chiave di questo profilo.
    ///
    /// Il formato è `ia.<id>.chiave`, e l'id è l'unica parte variabile: è per
    /// questo che deve essere ristretto a lettere, cifre e trattini —
    /// vedi [`id_da_nome`].
    #[must_use]
    pub fn voce_portachiavi(&self) -> String {
        voce_portachiavi(&self.id)
    }

    /// L'indirizzo completo di un percorso delle API.
    ///
    /// Il percorso si scrive con la barra iniziale (`/models`), e la barra
    /// finale dell'indirizzo di base si toglie: `http://localhost:1234/v1/` e
    /// `http://localhost:1234/v1` devono chiamare lo stesso posto, perché chi
    /// incolla un indirizzo da una pagina di documentazione porta a casa l'una o
    /// l'altra forma a seconda della pagina.
    #[must_use]
    pub fn punto(&self, percorso: &str) -> String {
        format!("{}{percorso}", self.url_base.trim_end_matches('/'))
    }
}

/// La voce di portachiavi di un id.
///
/// Libera e non solo metodo: chi **elimina** un profilo deve poter cancellare
/// il segreto avendo in mano l'id e nient'altro — il profilo, a quel punto, è
/// già stato tolto dall'elenco.
#[must_use]
pub fn voce_portachiavi(id: &str) -> String {
    format!("ia.{id}.chiave")
}

/// Un id da un nome, unico rispetto a quelli già presi.
///
/// Minuscole, cifre e trattini: tutto il resto diventa un trattino, e i
/// trattini non si accumulano né restano ai bordi. Un nome che non lascia
/// niente — «✦✦✦», o vuoto — diventa `profilo`, che è meglio di una stringa
/// vuota dentro una chiave di portachiavi.
///
/// Se l'id è già preso si aggiunge `-2`, poi `-3`, e così via. Non è eleganza:
/// due profili con lo stesso id condividerebbero la voce di portachiavi, e
/// cancellarne uno porterebbe via la chiave dell'altro.
#[must_use]
pub fn id_da_nome(nome: &str, presi: &[String]) -> String {
    let mut base = String::new();
    for c in nome.chars() {
        if c.is_ascii_alphanumeric() {
            base.extend(c.to_lowercase());
        } else if !base.ends_with('-') {
            base.push('-');
        }
        if base.len() >= LUNGHEZZA_ID {
            break;
        }
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "profilo" } else { base };

    if !presi.iter().any(|p| p == base) {
        return base.to_owned();
    }
    // Da due in su: `-1` sarebbe il primo, e il primo non ha suffisso.
    let mut n: u32 = 2;
    loop {
        let candidato = format!("{base}-{n}");
        if !presi.contains(&candidato) {
            return candidato;
        }
        n = n.saturating_add(1);
    }
}

/// Se con questo indirizzo si può parlare, e perché no quando no.
///
/// # Perché sta qui e non solo dentro il client
///
/// Perché la stessa domanda se la fa chi **salva** un profilo, per dire di no
/// mentre l'indirizzo si scrive invece che al primo messaggio. Una risposta
/// data solo al momento della richiesta lascerebbe salvare una configurazione
/// che non potrà mai funzionare, e la scoperta arriverebbe dopo aver scritto la
/// prima domanda.
///
/// La decisione vera non è qui: è
/// [`aether_net::http::in_chiaro_ammesso`], che è l'unico posto in cui esiste.
/// Qui si aggiunge solo il **nome** dell'ospite, che serve al messaggio e non
/// alla decisione — e per questo si estrae alla buona, senza pretendere di
/// essere un analizzatore di indirizzi.
///
/// # Errori
///
/// `ia.notLoopback` per un `http://` che non è questa macchina. Un indirizzo
/// che non è né `http://` né `https://` cade nello stesso codice: non è
/// loopback, e non lo diventerà.
pub fn controlla_indirizzo(url: &str) -> Result<(), AppError> {
    let url = url.trim();
    if url.starts_with("https://") || aether_net::http::in_chiaro_ammesso(url) {
        return Ok(());
    }
    Err(AppError::new(ErrorCode::IaNotLoopback {
        host: ospite(url).to_owned(),
    }))
}

/// L'ospite di un indirizzo, alla buona, per scriverlo in un messaggio.
///
/// Non decide niente — vedi [`controlla_indirizzo`] — quindi può permettersi di
/// restituire l'indirizzo intero quando non riconosce niente: in un messaggio
/// «non posso parlare con X» va bene qualunque X che chi legge riconosca.
fn ospite(url: &str) -> &str {
    let resto = url
        .split_once("://")
        .map_or(url, |(_, dopo_schema)| dopo_schema);
    let autorita = resto
        .find(['/', '?', '#'])
        .map_or(resto, |fine| resto.get(..fine).unwrap_or(resto));
    // L'**ultima** chiocciola, come in `aether_net`: in `localhost@esempio.com`
    // l'ospite è il secondo, e nominare il primo direbbe una bugia proprio nel
    // caso costruito per ingannare.
    let dopo_utente = autorita.rsplit('@').next().unwrap_or(autorita);
    if dopo_utente.is_empty() {
        return url;
    }
    dopo_utente
        .strip_prefix('[')
        .and_then(|dentro| dentro.split(']').next())
        .unwrap_or_else(|| dopo_utente.split(':').next().unwrap_or(dopo_utente))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn profilo(url: &str) -> Profilo {
        Profilo {
            id: "prova".to_owned(),
            nome: "Prova".to_owned(),
            fornitore: Fornitore::Ollama,
            url_base: url.to_owned(),
            modello: "qwen3".to_owned(),
            con_chiave: false,
        }
    }

    #[test]
    fn la_barra_finale_non_raddoppia() {
        assert_eq!(
            profilo("http://localhost:1234/v1/").punto("/models"),
            "http://localhost:1234/v1/models"
        );
        assert_eq!(
            profilo("http://localhost:1234/v1").punto("/models"),
            "http://localhost:1234/v1/models"
        );
    }

    #[test]
    fn un_nome_diventa_un_id_che_sta_in_una_chiave() {
        assert_eq!(
            id_da_nome("Claude via OpenRouter", &[]),
            "claude-via-openrouter"
        );
        assert_eq!(id_da_nome("  Qwen 3 — 30B  ", &[]), "qwen-3-30b");
        assert_eq!(id_da_nome("ÀÉÎ", &[]), "profilo");
        assert_eq!(id_da_nome("", &[]), "profilo");
    }

    #[test]
    fn un_id_lungo_si_taglia_senza_lasciare_trattini_ai_bordi() {
        let id = id_da_nome(&"a b ".repeat(40), &[]);
        assert!(id.len() <= LUNGHEZZA_ID, "{id}");
        assert!(!id.starts_with('-') && !id.ends_with('-'), "{id}");
    }

    /// Due profili con lo stesso id condividerebbero la voce di portachiavi, e
    /// cancellarne uno porterebbe via la chiave dell'altro.
    #[test]
    fn due_nomi_uguali_non_fanno_due_id_uguali() {
        let presi = vec!["claude".to_owned()];
        let secondo = id_da_nome("Claude", &presi);
        assert_eq!(secondo, "claude-2");
        let presi = vec!["claude".to_owned(), secondo];
        assert_eq!(id_da_nome("Claude", &presi), "claude-3");
    }

    #[test]
    fn con_questa_macchina_si_parla_anche_in_chiaro() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:1234/v1",
            "https://openrouter.ai/api/v1",
        ] {
            assert!(controlla_indirizzo(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn in_chiaro_con_chiunque_altro_no_e_lo_dice_col_nome() {
        for (url, atteso) in [
            ("http://esempio.com/v1", "esempio.com"),
            // La chiocciola che serve a ingannare: l'ospite è il secondo.
            ("http://localhost@esempio.com/v1", "esempio.com"),
            ("http://192.168.1.9:1234/v1", "192.168.1.9"),
        ] {
            let err = controlla_indirizzo(url).unwrap_err();
            assert!(
                matches!(err.code(), ErrorCode::IaNotLoopback { host } if host == atteso),
                "{url} → {err:?}"
            );
        }
    }

    #[test]
    fn quel_che_non_e_un_indirizzo_non_passa() {
        for url in [
            "",
            "localhost:11434",
            "ftp://localhost/v1",
            "ws://localhost",
        ] {
            assert!(controlla_indirizzo(url).is_err(), "{url}");
        }
    }

    #[test]
    fn la_chiave_non_sta_nel_profilo() {
        let scritto =
            serde_json::to_string(&profilo("http://localhost:11434/v1")).unwrap_or_default();
        assert!(scritto.contains("\"conChiave\":false"), "{scritto}");
        assert!(!scritto.contains("chiave\":\""), "{scritto}");
    }
}
