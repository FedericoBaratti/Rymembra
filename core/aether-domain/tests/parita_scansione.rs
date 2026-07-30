//! Parità con l'originale su ciò che può cancellare righe di libreria.
//!
//! Stesso metodo dei vettori sulle chiavi: `golden/scan.json` è l'uscita di
//! `paths.ts` e `scanPlan.ts` eseguiti su scenari costruiti sui due difetti che
//! il vecchio albero ha davvero avuto. Il confronto è **sul piano intero**, non
//! sui conteggi: un piano che rimuove la riga giusta per il motivo sbagliato è
//! un piano che manda a cercare nel posto sbagliato quando qualcosa va storto.

use aether_domain::paths::{
    PathRules, base_name, extension_of, is_in_trash, is_supported_audio_path, is_under, path_key,
};
use aether_domain::scan_plan::{DiscoveredFile, KnownTrack, ScanInput, plan_scan};
use serde::Deserialize;

const GOLDEN: &str = include_str!("golden/scan.json");

type Esito = Result<(), serde_json::Error>;

fn vettori() -> Result<Vettori, serde_json::Error> {
    serde_json::from_str(GOLDEN)
}

fn regole(case_insensitive: bool) -> PathRules {
    PathRules { case_insensitive }
}

#[derive(Deserialize)]
struct Vettori {
    #[serde(rename = "pathKey")]
    path_key: Vec<CasoPercorsoRegole<String>>,
    #[serde(rename = "baseName")]
    base_name: Vec<CasoPercorso<String>>,
    #[serde(rename = "extensionOf")]
    extension_of: Vec<CasoPercorso<String>>,
    #[serde(rename = "isInTrash")]
    is_in_trash: Vec<CasoPercorso<bool>>,
    #[serde(rename = "isSupportedAudioPath")]
    is_supported_audio_path: Vec<CasoPercorso<bool>>,
    #[serde(rename = "isUnder")]
    is_under: Vec<CasoUnder>,
    #[serde(rename = "planScan")]
    plan_scan: Vec<CasoPiano>,
}

#[derive(Deserialize)]
struct CasoPercorso<T> {
    nota: String,
    input: String,
    atteso: T,
}

#[derive(Deserialize)]
struct CasoPercorsoRegole<T> {
    nota: String,
    input: String,
    #[serde(rename = "caseInsensitive")]
    case_insensitive: bool,
    atteso: T,
}

#[derive(Deserialize)]
struct CasoUnder {
    nota: String,
    path: String,
    folder: String,
    #[serde(rename = "caseInsensitive")]
    case_insensitive: bool,
    atteso: bool,
}

#[derive(Deserialize)]
struct CasoPiano {
    nota: String,
    #[serde(rename = "caseInsensitive")]
    case_insensitive: bool,
    input: IngressiPiano,
    atteso: PianoAtteso,
}

#[derive(Deserialize)]
struct IngressiPiano {
    roots: Vec<String>,
    found: Vec<FileTrovato>,
    known: Vec<RigaNota>,
}

#[derive(Deserialize)]
struct FileTrovato {
    path: String,
    #[serde(rename = "sizeBytes")]
    size_bytes: u64,
    #[serde(rename = "modifiedMs")]
    modified_ms: i64,
}

#[derive(Deserialize)]
struct RigaNota {
    id: i64,
    path: String,
    #[serde(rename = "modifiedMs")]
    modified_ms: i64,
}

/// La forma in cui il generatore ha appiattito il piano. Il port ne ricostruisce
/// una uguale e le due si confrontano per intero.
#[derive(Deserialize, PartialEq, Eq, Debug)]
struct PianoAtteso {
    #[serde(rename = "toInsert")]
    to_insert: Vec<String>,
    #[serde(rename = "toUpdate")]
    to_update: Vec<VoceAggiornamento>,
    #[serde(rename = "toRemove")]
    to_remove: Vec<VoceRimozione>,
    skipped: Vec<VoceScarto>,
    unchanged: usize,
    untouched: usize,
    #[serde(rename = "isNoOp")]
    is_no_op: bool,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct VoceAggiornamento {
    path: String,
    #[serde(rename = "trackId")]
    track_id: i64,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct VoceRimozione {
    id: i64,
    path: String,
    reason: String,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct VoceScarto {
    path: String,
    reason: String,
}

/// Riporta tutte le differenze in una volta, non solo la prima.
fn confronta<T: PartialEq + std::fmt::Debug>(nome: &str, esiti: Vec<(String, String, T, T)>) {
    let differenze: Vec<_> = esiti
        .into_iter()
        .filter(|(_, _, atteso, ottenuto)| atteso != ottenuto)
        .collect();
    assert!(
        differenze.is_empty(),
        "{} vettori di {nome} non combaciano con l'originale TypeScript:\n{}",
        differenze.len(),
        differenze
            .iter()
            .map(|(nota, input, atteso, ottenuto)| format!(
                "  {input}\n      atteso   {atteso:?}\n      ottenuto {ottenuto:?}\n      ({nota})"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn path_key_combacia_con_l_originale() -> Esito {
    confronta(
        "path_key",
        vettori()?
            .path_key
            .into_iter()
            .map(|c| {
                let ottenuto = path_key(&c.input, regole(c.case_insensitive));
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn base_name_combacia_con_l_originale() -> Esito {
    confronta(
        "base_name",
        vettori()?
            .base_name
            .into_iter()
            .map(|c| {
                let ottenuto = base_name(&c.input).to_owned();
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn extension_of_combacia_con_l_originale() -> Esito {
    confronta(
        "extension_of",
        vettori()?
            .extension_of
            .into_iter()
            .map(|c| {
                let ottenuto = extension_of(&c.input);
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn is_in_trash_combacia_con_l_originale() -> Esito {
    confronta(
        "is_in_trash",
        vettori()?
            .is_in_trash
            .into_iter()
            .map(|c| {
                let ottenuto = is_in_trash(&c.input);
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn is_supported_audio_path_combacia_con_l_originale() -> Esito {
    confronta(
        "is_supported_audio_path",
        vettori()?
            .is_supported_audio_path
            .into_iter()
            .map(|c| {
                let ottenuto = is_supported_audio_path(&c.input);
                (c.nota, format!("{:?}", c.input), c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn is_under_combacia_con_l_originale() -> Esito {
    confronta(
        "is_under",
        vettori()?
            .is_under
            .into_iter()
            .map(|c| {
                let ottenuto = is_under(&c.path, &c.folder, regole(c.case_insensitive));
                let mostrato = format!("{:?} dentro {:?}", c.path, c.folder);
                (c.nota, mostrato, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}

#[test]
fn plan_scan_combacia_con_l_originale() -> Esito {
    let casi = vettori()?.plan_scan;
    assert!(
        casi.len() >= 20,
        "i vettori sono stati svuotati per sbaglio?"
    );
    confronta(
        "plan_scan",
        casi.into_iter()
            .map(|c| {
                let found: Vec<DiscoveredFile> = c
                    .input
                    .found
                    .iter()
                    .map(|f| DiscoveredFile {
                        path: f.path.clone(),
                        size_bytes: f.size_bytes,
                        modified_ms: f.modified_ms,
                    })
                    .collect();
                let known: Vec<KnownTrack> = c
                    .input
                    .known
                    .iter()
                    .map(|k| KnownTrack {
                        id: k.id,
                        path: k.path.clone(),
                        modified_ms: k.modified_ms,
                    })
                    .collect();
                let piano = plan_scan(
                    ScanInput {
                        roots: &c.input.roots,
                        found: &found,
                        known: &known,
                    },
                    regole(c.case_insensitive),
                );
                let ottenuto = PianoAtteso {
                    to_insert: piano.to_insert.iter().map(|f| f.path.clone()).collect(),
                    to_update: piano
                        .to_update
                        .iter()
                        .map(|u| VoceAggiornamento {
                            path: u.file.path.clone(),
                            track_id: u.track_id,
                        })
                        .collect(),
                    to_remove: piano
                        .to_remove
                        .iter()
                        .map(|r| VoceRimozione {
                            id: r.track.id,
                            path: r.track.path.clone(),
                            reason: r.reason.as_str().to_owned(),
                        })
                        .collect(),
                    skipped: piano
                        .skipped
                        .iter()
                        .map(|s| VoceScarto {
                            path: s.file.path.clone(),
                            reason: s.reason.as_str().to_owned(),
                        })
                        .collect(),
                    unchanged: piano.unchanged,
                    untouched: piano.untouched,
                    is_no_op: piano.is_no_op(),
                };
                (c.nota.clone(), c.nota, c.atteso, ottenuto)
            })
            .collect(),
    );
    Ok(())
}
