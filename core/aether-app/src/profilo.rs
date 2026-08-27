//! Il profilo: portare le proprie preferenze su un altro computer.
//!
//! Un file JSON con quel che si è scelto — tema, skin, equalizzatore,
//! scorciatoie, cartelle — e non con quel che si è ascoltato. Per quello c'è il
//! backup su Drive, che è un'altra cosa e risolve un altro problema: là si
//! salva ciò che una scansione non sa ricostruire, qui ciò che una persona ha
//! deciso.
//!
//! # Un elenco di quel che **esce**, non di quel che resta
//!
//! È la decisione che dà forma al modulo, e la ragione ha un nome:
//! `nuvola.dispositivo`.
//!
//! Quella chiave è l'identificativo con cui il backup distingue questo computer
//! dagli altri quando fonde le statistiche. Due macchine che dichiarano lo
//! stesso identificativo non danno nessun errore: si sovrascrivono a vicenda
//! nel backup, e il danno si scopre mesi dopo, guardando conteggi d'ascolto che
//! non tornano.
//!
//! Con un elenco di **esclusioni**, una chiave nuova viaggia per difetto — e la
//! prossima `nuvola.dispositivo` uscirebbe in silenzio, scritta da qualcuno che
//! non sapeva che questo file esistesse. Con un elenco di **inclusioni**, una
//! chiave nuova non viaggia finché qualcuno non la mette in [`CATALOGO`]: il
//! guasto peggiore diventa «una preferenza non si è portata dietro», che si
//! nota subito e non rompe niente.
//!
//! Perché «si nota subito» sia vero, l'esportazione **dichiara** cosa ha
//! lasciato fuori e perché. Un elenco di inclusioni silenzioso avrebbe lo
//! stesso difetto dell'altro, spostato di un passo.
//!
//! # I segreti non entrano, e non c'è nemmeno la riga che potrebbe
//!
//! Token, chiavi di sessione e segreti stanno nel portachiavi di sistema
//! (`aether_oauth::portachiavi`), e questo modulo non lo vede. Non è una
//! precauzione presa qui: è che non c'è niente da cui prenderli.
//!
//! # Perché l'importazione ha un piano
//!
//! Come ogni altra operazione irreversibile del programma. Un profilo cambia la
//! skin, il tema, le cartelle sorvegliate e l'equalizzatore tutti insieme, e
//! senza un piano l'unico modo di sapere cosa cambierà è guardare cos'è
//! cambiato. Il piano dice, chiave per chiave, cosa c'è adesso e cosa ci
//! sarebbe — e per i percorsi, quali di quelli che arrivano **non esistono su
//! questo computer**, che è il caso normale quando il profilo viene da una
//! macchina diversa.

use aether_domain::errors::{AppError, ErrorCode};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::settings;

/// Cosa rappresenta una chiave, e quindi come la si tratta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Genere {
    /// Una scelta: viaggia sempre.
    Preferenza,
    /// Un percorso sul disco: viaggia, ma su un'altra macchina può non esistere.
    Percorso,
}

/// Quel che esce, e cos'è.
///
/// L'unico posto in cui si decide che una chiave può lasciare questo computer.
/// Aggiungerne una qui è una riga; dimenticarsene costa una preferenza che non
/// si porta dietro, ed è per questo che l'omissione è il difetto che si vuole
/// avere invece dell'altro.
const CATALOGO: &[(&str, Genere)] = &[
    // ── l'aspetto ──
    (crate::preferenze::CHIAVE_TEMA, Genere::Preferenza),
    // La lingua viaggia, e su un'altra macchina può nominare un file che di là
    // non c'è: la finestra ripiega sul sistema, che è il comportamento giusto —
    // meglio dell'inglese imposto da un profilo scritto altrove.
    (crate::preferenze::CHIAVE_LINGUA, Genere::Preferenza),
    ("skin.active", Genere::Preferenza),
    ("skin.dynamicAccent", Genere::Preferenza),
    // ── la riproduzione ──
    // `player.queue` **non** c'è, ed è la seconda ragione per cui questo elenco
    // è di inclusioni: contiene identificativi di righe di `tracks`, che su
    // un'altra libreria nominano canzoni diverse o nessuna.
    ("player.volume", Genere::Preferenza),
    ("player.eq", Genere::Preferenza),
    ("player.eq.presets", Genere::Preferenza),
    ("player.replaygain", Genere::Preferenza),
    // Viaggia, al contrario del timer di spegnimento: «continua quando la coda
    // finisce» è come uno vuole che il lettore si comporti, e vale su ogni
    // macchina. Il timer invece è una decisione di stasera, e ritrovarlo su un
    // altro computer sarebbe una musica che si spegne da sola senza motivo.
    ("player.autoplay", Genere::Preferenza),
    // Viaggia per la stessa ragione: quanto si vuole che due brani si
    // sovrappongano è un gusto d'ascolto, non un fatto di questa macchina.
    ("player.crossfade", Genere::Preferenza),
    // ── la tastiera ──
    (crate::preferenze::CHIAVE_SCORCIATOIE, Genere::Preferenza),
    // ── gli automatismi ──
    ("enrich.auto", Genere::Preferenza),
    ("scrobble.attivo", Genere::Preferenza),
    ("nuvola.attivo", Genere::Preferenza),
    // ── i percorsi ──
    // Viaggiano, e il piano dice quali non esistono di qua. L'alternativa —
    // lasciarli fuori — renderebbe inutile il caso più comune di tutti: la
    // reinstallazione sulla stessa macchina.
    ("library.roots", Genere::Percorso),
    ("download.folder", Genere::Percorso),
];

/// Come si riconosce un file di profilo.
const FIRMA: &str = "aether.profilo";

/// La versione del formato.
///
/// Un profilo di una versione futura si **rifiuta** invece di essere letto a
/// metà: leggerne le chiavi che si riconoscono e ignorare le altre sembra
/// generoso e produce una macchina configurata a metà, senza dire quale metà.
const VERSIONE: u32 = 1;

/// Il file, come sta sul disco.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SuDisco {
    /// La firma, per non provare a leggere un JSON qualunque.
    aether: String,
    /// La versione del formato.
    versione: u32,
    /// Quando è stato scritto, in millisecondi.
    creato_ms: i64,
    /// Le chiavi e i loro valori.
    voci: std::collections::BTreeMap<String, String>,
}

/// Cosa è uscito, e cosa no.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Esportazione {
    /// Il documento JSON, pronto da scrivere su un file.
    pub json: String,
    /// Quante chiavi ha portato via.
    pub voci: usize,
    /// Quali chiavi presenti nel database sono rimaste qui.
    ///
    /// Dichiarate e non taciute: un elenco di inclusioni silenzioso avrebbe lo
    /// stesso difetto di uno di esclusioni, spostato di un passo.
    pub lasciate: Vec<String>,
}

/// Una chiave che l'importazione cambierebbe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cambio {
    /// Quale chiave.
    pub chiave: String,
    /// Cos'è.
    pub genere: Genere,
    /// Cosa c'è adesso. `None` se la chiave non c'è.
    pub prima: Option<String>,
    /// Cosa ci sarebbe.
    pub dopo: String,
}

/// Cosa farebbe l'importazione.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Piano {
    /// Quando il profilo è stato scritto.
    pub creato_ms: i64,
    /// Le chiavi che cambierebbero.
    pub cambi: Vec<Cambio>,
    /// Le chiavi del file che questa versione non porta.
    ///
    /// Non è un guasto: è un profilo scritto da una versione che ne conosceva
    /// una in più, o una chiave tolta dal catalogo. Si dice, e si tira dritto.
    pub sconosciute: Vec<String>,
    /// I percorsi che arrivano e che su questo computer non esistono.
    pub percorsi_mancanti: Vec<String>,
    /// Quante chiavi sono già uguali a quel che c'è.
    pub invariate: usize,
}

/// Traduce un guasto di SQLite nominando l'operazione.
fn db_error(cosa: &str, err: &rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::DbQueryFailed {
        detail: Some(cosa.to_owned()),
    })
    .with_cause(err.to_string())
}

/// Il profilo di questo computer.
///
/// # Errori
///
/// `db.queryFailed` se le impostazioni non si leggono.
pub fn esporta(connection: &Connection, adesso_ms: i64) -> Result<Esportazione, AppError> {
    let mut voci = std::collections::BTreeMap::new();
    for (chiave, _) in CATALOGO {
        if let Some(valore) = settings::read(connection, chiave)? {
            voci.insert((*chiave).to_owned(), valore);
        }
    }

    let documento = SuDisco {
        aether: FIRMA.to_owned(),
        versione: VERSIONE,
        creato_ms: adesso_ms,
        voci,
    };
    let json = serde_json::to_string_pretty(&documento).map_err(|err| {
        AppError::new(ErrorCode::InternalUnexpected {
            detail: Some("il profilo non si è serializzato".to_owned()),
        })
        .with_cause(err.to_string())
    })?;

    Ok(Esportazione {
        voci: documento.voci.len(),
        lasciate: lasciate(connection, &documento.voci)?,
        json,
    })
}

/// Le chiavi che stanno nel database e che il catalogo non porta via.
fn lasciate(
    connection: &Connection,
    portate: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<String>, AppError> {
    let mut istruzione = connection
        .prepare("SELECT key FROM settings ORDER BY key")
        .map_err(|err| db_error("elenco delle impostazioni", &err))?;
    let righe = istruzione
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("lettura delle impostazioni", &err))?;

    let mut fuori = Vec::new();
    for riga in righe {
        let chiave = riga.map_err(|err| db_error("lettura di una impostazione", &err))?;
        if !portate.contains_key(&chiave) {
            fuori.push(chiave);
        }
    }
    Ok(fuori)
}

/// Cosa cambierebbe importare questo profilo.
///
/// `esiste` dice se un percorso c'è su questo computer: gliela si passa invece
/// di guardare il disco da qui, così la funzione si prova senza avere le
/// cartelle di nessuno.
///
/// # Errori
///
/// `settings.corrupt` se il file non è un profilo di Aether o è di una versione
/// che questa build non sa leggere. `db.queryFailed` per il resto.
pub fn piano(
    connection: &Connection,
    json: &str,
    esiste: &dyn Fn(&str) -> bool,
) -> Result<Piano, AppError> {
    let documento = leggi(json)?;
    let mut piano = Piano {
        creato_ms: documento.creato_ms,
        ..Piano::default()
    };

    for (chiave, valore) in &documento.voci {
        let Some((_, genere)) = CATALOGO.iter().find(|(nome, _)| nome == chiave) else {
            piano.sconosciute.push(chiave.clone());
            continue;
        };
        let prima = settings::read(connection, chiave)?;
        if prima.as_ref() == Some(valore) {
            piano.invariate += 1;
            continue;
        }
        if *genere == Genere::Percorso {
            piano
                .percorsi_mancanti
                .extend(percorsi_di(valore).into_iter().filter(|p| !esiste(p)));
        }
        piano.cambi.push(Cambio {
            chiave: chiave.clone(),
            genere: *genere,
            prima,
            dopo: valore.clone(),
        });
    }
    Ok(piano)
}

/// Applica il profilo, e restituisce lo stesso piano di [`piano`].
///
/// Il piano è l'esecuzione annullata, come ovunque qui: le due funzioni fanno
/// esattamente la stessa cosa, e la differenza è che una abbandona la
/// transazione. Ne segue che i numeri mostrati nell'anteprima sono quelli che si
/// otterranno, non una previsione fatta da un'altra parte del codice.
///
/// # Errori
///
/// Come [`piano`], più `db.queryFailed` se la scrittura non riesce.
pub fn importa(
    connection: &mut Connection,
    json: &str,
    esiste: &dyn Fn(&str) -> bool,
) -> Result<Piano, AppError> {
    let tx = connection
        .transaction()
        .map_err(|err| db_error("apertura della transazione del profilo", &err))?;
    let piano = piano(&tx, json, esiste)?;
    for cambio in &piano.cambi {
        settings::write(&tx, &cambio.chiave, &cambio.dopo)?;
    }
    tx.commit()
        .map_err(|err| db_error("chiusura della transazione del profilo", &err))?;
    Ok(piano)
}

/// Legge il file, rifiutando quel che non è un profilo leggibile.
fn leggi(json: &str) -> Result<SuDisco, AppError> {
    let documento: SuDisco = serde_json::from_str(json).map_err(|err| {
        AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!("il file non è un profilo di Aether: {err}"))
    })?;
    if documento.aether != FIRMA {
        return Err(AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!(
            "il file dice di essere «{}», non un profilo di Aether",
            documento.aether
        )));
    }
    if documento.versione > VERSIONE {
        return Err(AppError::new(ErrorCode::SettingsCorrupt {
            quarantined_as: None,
        })
        .with_cause(format!(
            "profilo di formato {} scritto da una versione più recente: questa legge fino al {VERSIONE}",
            documento.versione
        )));
    }
    Ok(documento)
}

/// I percorsi dentro un valore.
///
/// Due forme, perché nel database ce ne sono due: `library.roots` è un elenco
/// JSON, `download.folder` è un percorso e basta. Distinguerle guardando la
/// chiave sarebbe una terza tabella da tenere allineata; guardare il valore
/// funziona e non ha niente da allineare.
fn percorsi_di(valore: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(valore)
        .ok()
        .unwrap_or_else(|| vec![valore.to_owned()])
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod prove {
    use super::*;

    fn libreria() -> Connection {
        crate::db::open_in_memory().expect("database").connection
    }

    fn c_e_tutto(_: &str) -> bool {
        true
    }

    fn non_c_e_niente(_: &str) -> bool {
        false
    }

    #[test]
    fn l_identificativo_del_dispositivo_non_esce() {
        let c = libreria();
        settings::write(&c, "nuvola.dispositivo", "questo-computer").expect("scrittura");
        settings::write(&c, "player.volume", "0.8").expect("scrittura");

        let uscito = esporta(&c, 0).expect("esportazione");
        assert!(
            !uscito.json.contains("questo-computer"),
            "due macchine con lo stesso identificativo si sovrascrivono nel backup, senza errori"
        );
        assert!(uscito.json.contains("player.volume"));
        assert!(
            uscito.lasciate.contains(&"nuvola.dispositivo".to_owned()),
            "e quel che resta qui va detto, o l'elenco di inclusioni sarebbe silenzioso quanto l'altro"
        );
    }

    #[test]
    fn la_coda_non_esce() {
        let c = libreria();
        settings::write(&c, "player.queue", r#"{"tracks":[1,2,3]}"#).expect("scrittura");
        let uscito = esporta(&c, 0).expect("esportazione");
        assert!(
            !uscito.json.contains("player.queue"),
            "sono identificativi di righe: su un'altra libreria nominano canzoni diverse"
        );
        assert!(uscito.lasciate.contains(&"player.queue".to_owned()));
    }

    #[test]
    fn quel_che_esce_rientra_uguale() {
        let sorgente = libreria();
        settings::write(&sorgente, "player.volume", "0.42").expect("scrittura");
        settings::write(&sorgente, "skin.active", "sala").expect("scrittura");
        settings::write(&sorgente, crate::preferenze::CHIAVE_TEMA, "chiaro").expect("scrittura");
        let uscito = esporta(&sorgente, 1_700_000_000_000).expect("esportazione");
        assert_eq!(uscito.voci, 3);

        let mut destinazione = libreria();
        let piano = importa(&mut destinazione, &uscito.json, &c_e_tutto).expect("importazione");
        assert_eq!(piano.cambi.len(), 3);
        assert_eq!(piano.creato_ms, 1_700_000_000_000);
        assert_eq!(
            settings::read(&destinazione, "skin.active"),
            Ok(Some("sala".to_owned()))
        );
    }

    #[test]
    fn reimportare_lo_stesso_profilo_non_cambia_piu_niente() {
        let mut c = libreria();
        settings::write(&c, "player.volume", "0.42").expect("scrittura");
        let uscito = esporta(&c, 0).expect("esportazione");

        importa(&mut c, &uscito.json, &c_e_tutto).expect("prima");
        let secondo = importa(&mut c, &uscito.json, &c_e_tutto).expect("seconda");
        assert!(secondo.cambi.is_empty());
        assert_eq!(secondo.invariate, 1);
    }

    #[test]
    fn il_piano_non_scrive_niente() {
        let c = libreria();
        let json = esporta(&libreria(), 0)
            .expect("esportazione")
            .json
            .replace(r#""voci": {}"#, r#""voci": {"player.volume": "0.9"}"#);

        let piano = piano(&c, &json, &c_e_tutto).expect("piano");
        assert_eq!(piano.cambi.len(), 1);
        assert_eq!(
            settings::read(&c, "player.volume"),
            Ok(None),
            "il piano guarda e non tocca"
        );
    }

    #[test]
    fn i_percorsi_che_non_ci_sono_si_dicono() {
        let c = libreria();
        let sorgente = libreria();
        settings::write(&sorgente, "library.roots", r#"["D:\\Musica","E:\\Altro"]"#)
            .expect("scrittura");
        settings::write(&sorgente, "download.folder", r#"D:\Scarichi"#).expect("scrittura");
        let uscito = esporta(&sorgente, 0).expect("esportazione");

        let mancanti = piano(&c, &uscito.json, &non_c_e_niente).expect("piano");
        assert_eq!(
            mancanti.percorsi_mancanti.len(),
            3,
            "due radici più la cartella degli scaricamenti"
        );
        assert!(
            mancanti
                .percorsi_mancanti
                .iter()
                .any(|p| p.contains("Scarichi"))
        );

        let tutti_qui = piano(&c, &uscito.json, &c_e_tutto).expect("piano");
        assert!(tutti_qui.percorsi_mancanti.is_empty());
    }

    #[test]
    fn un_json_qualunque_non_e_un_profilo() {
        let c = libreria();
        let err = piano(&c, r#"{"tema":"chiaro"}"#, &c_e_tutto).expect_err("deve rifiutare");
        assert_eq!(err.code().kind().code(), "settings.corrupt");

        let finto = r#"{"aether":"altro","versione":1,"creato_ms":0,"voci":{}}"#;
        assert!(piano(&c, finto, &c_e_tutto).is_err());
    }

    #[test]
    fn un_profilo_di_domani_si_rifiuta_invece_di_leggerlo_a_meta() {
        let c = libreria();
        let futuro = r#"{"aether":"aether.profilo","versione":99,"creato_ms":0,
                         "voci":{"player.volume":"0.5"}}"#;
        let err = piano(&c, futuro, &c_e_tutto).expect_err("deve rifiutare");
        assert!(
            err.cause().is_some_and(|causa| causa.contains("99")),
            "e deve dire quale formato ha trovato"
        );
    }

    #[test]
    fn una_chiave_che_questa_versione_non_conosce_si_dichiara_e_non_ferma_niente() {
        let c = libreria();
        let json = r#"{"aether":"aether.profilo","versione":1,"creato_ms":0,
                       "voci":{"player.volume":"0.5","ui.qualcosa":"x"}}"#;
        let piano = piano(&c, json, &c_e_tutto).expect("piano");
        assert_eq!(piano.cambi.len(), 1);
        assert_eq!(piano.sconosciute, vec!["ui.qualcosa".to_owned()]);
    }

    #[test]
    fn i_percorsi_si_leggono_in_tutte_e_due_le_forme() {
        assert_eq!(
            percorsi_di(r#"["C:\\a","C:\\b"]"#),
            vec![r"C:\a".to_owned(), r"C:\b".to_owned()]
        );
        assert_eq!(percorsi_di(r"C:\solo"), vec![r"C:\solo".to_owned()]);
        assert!(percorsi_di("  ").is_empty());
    }
}
