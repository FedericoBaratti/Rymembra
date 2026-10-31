//! Deezer: il terzo parere, e la copertina più grande di tutte.
//!
//! Anche questo punto di ricerca è pubblico e senza chiave. Rispetto a iTunes
//! porta due cose in più — `cover_xl` è a mille pixel, e il catalogo è europeo,
//! quindi copre meglio la musica che una libreria italiana contiene davvero —
//! e una in meno: non dà il genere del brano.
//!
//! # Perché tre fonti e non due
//!
//! Perché il consenso serve fra fonti **diverse**, e due sole vuol dire che una
//! qualunque delle due, cadendo, spegne la possibilità di applicare qualcosa.
//! Con tre, ne basta una che risponda insieme a MusicBrainz.

use rymembra_domain::enrich::{Candidate, FonteMeta, normalize_for_match};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_net::percento;
use serde_json::Value;

use crate::deposito::VIVE_RICERCA_MS;
use crate::{Fornitori, itunes};

/// Interpreta la risposta di una ricerca. Funzione pura.
#[must_use]
pub fn interpreta(corpo: &[u8]) -> Vec<Candidate> {
    let Ok(letto) = serde_json::from_slice::<Value>(corpo) else {
        return Vec::new();
    };
    let Some(elenco) = letto.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    elenco
        .iter()
        .filter_map(|voce| {
            let titolo = voce.get("title").and_then(Value::as_str)?;
            let artista = voce
                .get("artist")
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)?;
            let album = voce.get("album");
            Some(Candidate {
                fonte: Some(FonteMeta::Deezer),
                title: titolo.to_owned(),
                artist: artista.to_owned(),
                album: album
                    .and_then(|a| a.get("title"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                // Deezer non dà l'anno sul brano: sta sull'album, che questa
                // ricerca non restituisce per esteso. Meglio `None` che l'anno
                // di qualcos'altro.
                year: None,
                genre: None,
                // I secondi, non i millisecondi: è l'unica fonte delle tre a
                // darli così, ed è un errore che si fa una volta sola perché
                // produce scarti di durata di tre ordini di grandezza — cioè
                // nessuna corrispondenza, mai, senza nessun messaggio.
                duration_ms: voce
                    .get("duration")
                    .and_then(Value::as_u64)
                    .filter(|d| *d > 0)
                    .map(|secondi| secondi.saturating_mul(1000)),
                cover_url: album.and_then(copertina),
                mb_recording_id: None,
                mb_release_id: None,
                mb_release_group_id: None,
            })
        })
        .collect()
}

/// Il guasto che Deezer dichiara dentro una risposta riuscita, se ne dichiara
/// uno. Funzione pura: è il [`crate::cadenza::Vaglio`] della cadenza di Deezer.
///
/// # Perché serve
///
/// Deezer risponde agli errori con stato `200` e un oggetto `error` —
/// `{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}`.
/// Guardando solo lo stato, quel corpo era una risposta riuscita: si ricordava
/// per una settimana ([`VIVE_RICERCA_MS`]), [`interpreta`] ci trovava un elenco
/// vuoto, e Deezer contava come una fonte che aveva risposto «non ho niente».
/// Con MusicBrainz e iTunes giù, il brano diventava `no-match` invece di
/// restare da rifare, e l'interruttore non si apriva mai.
///
/// La quota superata (4) e il servizio occupato (700) sono un limite:
/// `net.rateLimited`, che si ritenta e conta come guasto per l'interruttore.
/// Gli altri codici sono un no che non si ritenta: una fonte in meno, senza
/// memoria. Le parole di Deezer vanno nella causa.
#[must_use]
pub fn guasto_nel_corpo(corpo: &[u8]) -> Option<AppError> {
    let letto = serde_json::from_slice::<Value>(corpo).ok()?;
    let errore = letto.get("error").filter(|e| e.is_object())?;
    let codice = errore.get("code").and_then(Value::as_i64);
    let causa = format!(
        "Deezer ha risposto con un errore: {} {}: {}",
        errore.get("type").and_then(Value::as_str).unwrap_or("?"),
        codice.map_or_else(|| "?".to_owned(), |c| c.to_string()),
        errore
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    Some(match codice {
        Some(4 | 700) => AppError::new(ErrorCode::NetRateLimited {
            service: Some("deezer".to_owned()),
            retry_after_ms: None,
        })
        .with_cause(causa),
        _ => AppError::new(ErrorCode::NetBadSchema {
            service: Some("deezer".to_owned()),
            detail: Some(causa.clone()),
        })
        .with_cause(causa),
    })
}

/// La copertina più grande fra quelle che l'album dichiara.
fn copertina(album: &Value) -> Option<String> {
    for campo in ["cover_xl", "cover_big", "cover_medium", "cover"] {
        if let Some(url) = album
            .get(campo)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|u| u.starts_with("https://"))
        {
            return Some(url.to_owned());
        }
    }
    None
}

/// I candidati che Deezer propone per questo brano.
///
/// # Errori
///
/// L'errore di rete così com'è, e il guasto che Deezer scrive dentro un `200`
/// ([`guasto_nel_corpo`]): chi chiama lo conta come «una fonte in meno».
pub fn cerca(
    fornitori: &Fornitori,
    titolo: &str,
    artista: &str,
) -> Result<Vec<Candidate>, AppError> {
    // Lo stesso termine di iTunes, e non uno costruito qui: due fonti
    // interrogate con due domande diverse darebbero un disaccordo che parla
    // delle domande invece che dei cataloghi — e il consenso fra loro è
    // esattamente la prova su cui si decide di scrivere.
    let Some(termine) = itunes::termine(titolo, artista) else {
        return Ok(Vec::new());
    };
    let url = format!(
        "https://api.deezer.com/search?q={}&limit=10",
        percento(&termine)
    );
    let corpo = fornitori.json(
        &fornitori.deezer,
        "deezer-ricerca",
        &normalize_for_match(&termine),
        &url,
        VIVE_RICERCA_MS,
    )?;
    Ok(corpo.as_deref().map(interpreta).unwrap_or_default())
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::deposito::Voce;

    #[test]
    fn i_secondi_diventano_millisecondi() {
        // L'errore che si fa una volta sola: senza la moltiplicazione, ogni
        // scarto di durata è di tre ordini di grandezza e nessun brano
        // corrisponde mai, senza nessun messaggio che dica perché.
        let corpo = br#"{"data":[{"title":"Poetica","duration":297,
            "artist":{"name":"Cesare Cremonini"},
            "album":{"title":"Possibili scenari","cover_xl":"https://c/xl.jpg",
                     "cover_big":"https://c/big.jpg"}}]}"#;
        let candidati = interpreta(corpo);
        let primo = candidati.first().expect("un candidato");
        assert_eq!(primo.duration_ms, Some(297_000));
        assert_eq!(primo.fonte, Some(FonteMeta::Deezer));
        assert_eq!(primo.cover_url.as_deref(), Some("https://c/xl.jpg"));
    }

    #[test]
    fn si_prende_la_copertina_piu_grande_disponibile() {
        let corpo = br#"{"data":[{"title":"X","artist":{"name":"Y"},
            "album":{"cover_medium":"https://c/m.jpg","cover":"https://c/s.jpg"}}]}"#;
        assert_eq!(
            interpreta(corpo)
                .first()
                .and_then(|c| c.cover_url.as_deref()),
            Some("https://c/m.jpg")
        );
    }

    #[test]
    fn una_copertina_non_cifrata_non_si_segue() {
        // L'indirizzo arriva da una risposta di rete e finisce in una
        // richiesta: `http:` non ha motivo di essere seguito.
        let corpo = br#"{"data":[{"title":"X","artist":{"name":"Y"},
            "album":{"cover_xl":"http://c/xl.jpg"}}]}"#;
        assert_eq!(
            interpreta(corpo)
                .first()
                .and_then(|c| c.cover_url.as_deref()),
            None
        );
    }

    #[test]
    fn una_voce_senza_interprete_si_salta() {
        let corpo = br#"{"data":[{"title":"Solo il titolo"},
            {"title":"Buono","artist":{"name":"Buono"}}]}"#;
        assert_eq!(interpreta(corpo).len(), 1);
    }

    #[test]
    fn un_corpo_storto_non_fa_cadere_niente() {
        assert!(interpreta(b"non json").is_empty());
        assert!(interpreta(b"{}").is_empty());
        assert!(interpreta(br#"{"data":{}}"#).is_empty());
    }

    /// Deezer manda i suoi errori con stato `200`, dentro un oggetto `error`.
    /// La quota superata (4) e il servizio occupato (700) sono un limite da
    /// rispettare; gli altri un no. Nessuno dei due è «non c'è niente».
    #[test]
    fn un_errore_di_deezer_dentro_un_200_non_e_un_elenco_vuoto() {
        let quota = guasto_nel_corpo(
            br#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#,
        )
        .expect("la quota superata è un guasto");
        assert!(quota.is_retryable());
        assert!(
            quota.cause().is_some_and(|c| c.contains("Quota")),
            "{quota:?}"
        );
        let occupato =
            guasto_nel_corpo(br#"{"error":{"type":"Exception","message":"busy","code":700}}"#)
                .expect("il servizio occupato è un guasto");
        assert!(occupato.is_retryable());
        let rifiuto = guasto_nel_corpo(
            br#"{"error":{"type":"DataException","message":"no data","code":800}}"#,
        )
        .expect("un errore dichiarato è un guasto anche se non si ritenta");
        assert!(!rifiuto.is_retryable());

        assert!(guasto_nel_corpo(br#"{"data":[],"total":0}"#).is_none());
        assert!(guasto_nel_corpo(b"non json").is_none());
        assert!(guasto_nel_corpo(br#"{"data":[{"title":"error"}]}"#).is_none());
    }

    /// Un deposito che ricorda in memoria e tiene il conto di cosa ha scritto.
    #[derive(Default)]
    struct Ricordo {
        voci: std::sync::Mutex<std::collections::HashMap<String, Voce>>,
        scritte: std::sync::Mutex<Vec<String>>,
    }

    impl crate::Deposito for std::sync::Arc<Ricordo> {
        fn leggi(&self, servizio: &str, chiave: &str) -> Option<Voce> {
            self.voci
                .lock()
                .unwrap()
                .get(&format!("{servizio}/{chiave}"))
                .cloned()
        }

        fn scrivi(&self, servizio: &str, chiave: &str, voce: &Voce, _vive_ms: i64) {
            let nome = format!("{servizio}/{chiave}");
            self.scritte.lock().unwrap().push(nome.clone());
            self.voci.lock().unwrap().insert(nome, voce.clone());
        }
    }

    /// Una risposta d'errore ricordata prima di questa correzione vale come
    /// assente: si richiede, e qui — con l'interruttore aperto — la richiesta
    /// non parte, quindi l'esito è «non si sa» e non «Deezer non ha niente».
    #[test]
    fn una_risposta_d_errore_ricordata_non_vale_come_risposta() {
        let ricordo = std::sync::Arc::new(Ricordo::default());
        let termine = itunes::termine("Poetica", "Cesare Cremonini").expect("un termine");
        ricordo.voci.lock().unwrap().insert(
            format!("deezer-ricerca/{}", normalize_for_match(&termine)),
            Voce::Corpo(br#"{"error":{"type":"Exception","message":"busy","code":700}}"#.to_vec()),
        );
        let fornitori = Fornitori::nuovo(Box::new(std::sync::Arc::clone(&ricordo)));
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.deezer.guasto();
        }
        let esito = cerca(&fornitori, "Poetica", "Cesare Cremonini");
        assert!(
            esito.is_err(),
            "letto dalla memoria come risposta: {esito:?}"
        );
    }

    /// Un `200` con dentro un errore non si ricorda, esce come errore, e tre di
    /// fila aprono l'interruttore come tre guasti qualunque.
    #[test]
    fn un_200_con_un_errore_non_si_ricorda() {
        let (base, rete, contate) = crate::prova_rete::servitore(
            200,
            r#"{"error":{"type":"Exception","message":"busy","code":700}}"#,
        );
        let ricordo = std::sync::Arc::new(Ricordo::default());
        let fornitori = Fornitori::con_rete(rete, Box::new(std::sync::Arc::clone(&ricordo)));
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            let esito = fornitori.json(
                &fornitori.deezer,
                "deezer-ricerca",
                "poetica",
                &format!("{base}/search?q=poetica"),
                VIVE_RICERCA_MS,
            );
            assert!(esito.is_err(), "un errore non è un corpo: {esito:?}");
        }
        assert_eq!(contate.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(
            ricordo.scritte.lock().unwrap().is_empty(),
            "ricordato: {:?}",
            ricordo.scritte.lock().unwrap()
        );
        assert!(fornitori.deezer.aperta());
    }
}
