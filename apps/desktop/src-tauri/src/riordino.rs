//! Il riordino della libreria sul disco.
//!
//! È **l'unico comando che modifica i file dell'utente**, e tutto qui dentro è
//! organizzato attorno a quel fatto: si vede prima cosa succederebbe, si
//! conferma, e si può tornare indietro dopo.
//!
//! # Il giornale, e perché ne resta uno solo
//!
//! `aether_app::organize::execute` scrive una riga per ogni spostamento
//! riuscito, subito e sincronizzata. Il file resta nella cartella dati, e
//! `annulla` prende **il più recente**: annullare due riordini di fila
//! all'indietro sarebbe possibile, ma l'unico caso che conta è «ho appena
//! riordinato e non mi piace». Un giornale annullato si rinomina invece di
//! sparire — è la prova di cosa è successo, e serve se l'annullamento a sua
//! volta fallisce a metà.

use std::path::PathBuf;

use aether_app::organize::{execute, read_journal, tracks_to_organize, undo};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::organize::{OrganizePlan, plan_organize};
use aether_domain::paths::PathRules;
use serde::Serialize;
use tauri::{Emitter as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, adesso_ms, con_libreria};

/// Uno spostamento proposto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spostamento {
    /// Da dove.
    pub da: String,
    /// A dove.
    pub a: String,
}

/// Quanti brani restano fermi, per ogni motivo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fermi {
    /// Il nome stabile del motivo, es. `alreadyInPlace`.
    pub motivo: String,
    /// Quanti.
    pub quanti: usize,
}

/// Un gruppo su cui i tag non bastano a decidere.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaRivedere {
    /// Il titolo dell'album, come sta nei tag.
    pub album: String,
    /// Quanti brani.
    pub brani: usize,
    /// Gli artisti trovati, dal più frequente.
    pub artisti: Vec<String>,
}

/// Cosa il riordino proporrebbe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Piano {
    /// La cartella su cui è calcolato.
    pub radice: String,
    /// Quanti brani della libreria sono stati considerati.
    pub letti: usize,
    /// Gli spostamenti, per esteso: è l'anteprima, e un conteggio non basta a
    /// decidere.
    pub spostamenti: Vec<Spostamento>,
    /// I fermi, raggruppati per motivo.
    pub fermi: Vec<Fermi>,
    /// I gruppi da guardare a mano, dal più grande.
    pub da_rivedere: Vec<DaRivedere>,
    /// C'è un riordino precedente che si può ancora annullare.
    pub annullabile: bool,
}

/// Uno spostamento che non è avvenuto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fallito {
    /// Da dove.
    pub da: String,
    /// A dove.
    pub a: String,
    /// Perché no, come record e non come frase.
    ///
    /// Il motivo per cui non è una stringa è quello scritto in
    /// [`crate::errore`]: «destinazione occupata» e «permesso negato» chiedono
    /// due cose diverse a chi legge, e distinguerle da un testo vorrebbe dire
    /// fare `indexOf` su una frase.
    pub errore: crate::errore::ErroreIpc,
}

/// Traduce gli spostamenti falliti.
fn falliti(esito: &aether_app::organize::OrganizeOutcome) -> Vec<Fallito> {
    esito
        .failed
        .iter()
        .map(|f| Fallito {
            da: f.mov.from.clone(),
            a: f.mov.to.clone(),
            errore: crate::errore::ErroreIpc::from(f.error.clone()),
        })
        .collect()
}

/// Cosa il riordino ha fatto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoRiordino {
    /// Quanti file si sono spostati.
    pub spostati: usize,
    /// Quelli che non ce l'hanno fatta.
    pub falliti: Vec<Fallito>,
    /// Le cartelle rimaste vuote e rimosse.
    pub cartelle_rimosse: usize,
    /// Resta qualcosa da annullare.
    pub annullabile: bool,
}

/// L'avanzamento di un riordino.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avanzamento {
    /// File spostati finora.
    pub fatti: usize,
    /// File da spostare in tutto.
    pub totale: usize,
}

/// Dove stanno i giornali.
fn cartella_giornali(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("riordino")
}

/// Il giornale più recente ancora annullabile, se c'è.
fn ultimo_giornale(data_dir: &std::path::Path) -> Option<PathBuf> {
    let dir = cartella_giornali(data_dir);
    let mut trovati: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|voce| voce.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    // Il nome porta il timestamp, quindi l'ordine alfabetico è quello
    // cronologico: leggerlo dal nome evita di chiedere al filesystem una data
    // di modifica che un backup o una sincronizzazione possono aver riscritto.
    trovati.sort();
    trovati.pop()
}

fn traduci(plan: &OrganizePlan, radice: String, letti: usize, annullabile: bool) -> Piano {
    let mut per_motivo: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for s in &plan.skipped {
        *per_motivo.entry(s.reason.as_str()).or_insert(0) += 1;
    }
    let mut da_rivedere: Vec<DaRivedere> = plan
        .needs_review
        .iter()
        .map(|r| DaRivedere {
            album: r.album.clone(),
            brani: r.tracks,
            artisti: r.artists.clone(),
        })
        .collect();
    da_rivedere.sort_by_key(|r| std::cmp::Reverse(r.brani));

    Piano {
        radice,
        letti,
        spostamenti: plan
            .moves
            .iter()
            .map(|m| Spostamento {
                da: m.from.clone(),
                a: m.to.clone(),
            })
            .collect(),
        fermi: per_motivo
            .into_iter()
            .map(|(motivo, quanti)| Fermi {
                motivo: motivo.to_owned(),
                quanti,
            })
            .collect(),
        da_rivedere,
        annullabile,
    }
}

/// Calcola il piano senza toccare niente.
#[tauri::command]
pub fn piano_riordino(stato: State<'_, Stato>, radice: String) -> Esito<Piano> {
    con_libreria(&stato, |libreria| {
        let tracks = tracks_to_organize(&libreria.connection)?;
        let plan = plan_organize(&tracks, &radice, PathRules::for_current_platform());
        let annullabile = ultimo_giornale(&libreria.data_dir).is_some();
        Ok(traduci(&plan, radice.clone(), tracks.len(), annullabile))
    })
    .map_err(errore)
}

/// Esegue il riordino. Sposta file veri.
///
/// Il piano si **ricalcola** invece di ricevere quello mostrato: fra l'anteprima
/// e la conferma può essere passato del tempo, e muovere file secondo un piano
/// vecchio è il modo di scoprire che una destinazione nel frattempo è occupata.
#[tauri::command]
pub fn esegui_riordino(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    radice: String,
) -> Esito<EsitoRiordino> {
    con_libreria(&stato, |libreria| {
        let tracks = tracks_to_organize(&libreria.connection)?;
        let plan = plan_organize(&tracks, &radice, PathRules::for_current_platform());

        let dir = cartella_giornali(&libreria.data_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|err| aether_app::files::io_error(&dir.display().to_string(), &err))?;
        let giornale = dir.join(format!("riordino-{}.jsonl", adesso_ms()));

        let mut ultimo = 0usize;
        let esito = execute(&plan, &giornale, |fatti, totale| {
            // Come per la scansione: non un evento per file. Uno spostamento
            // dura microsecondi, e millequattrocento eventi inonderebbero il
            // canale per muovere una barra di meno di un pixel per volta.
            if fatti == totale || fatti.saturating_sub(ultimo) >= 25 {
                ultimo = fatti;
                let _ = app.emit("riordino:avanzamento", Avanzamento { fatti, totale });
            }
        })?;

        Ok(EsitoRiordino {
            spostati: esito.moved.len(),
            falliti: falliti(&esito),
            cartelle_rimosse: esito.removed_dirs,
            annullabile: true,
        })
    })
    .map_err(errore)
}

/// Rimette tutto com'era, leggendo l'ultimo giornale al contrario.
#[tauri::command]
pub fn annulla_riordino(app: tauri::AppHandle, stato: State<'_, Stato>) -> Esito<EsitoRiordino> {
    con_libreria(&stato, |libreria| {
        let Some(giornale) = ultimo_giornale(&libreria.data_dir) else {
            return Err(AppError::new(ErrorCode::FsNotFound {
                path: cartella_giornali(&libreria.data_dir).display().to_string(),
            })
            .with_cause("nessun riordino da annullare".to_owned()));
        };
        let righe = read_journal(&giornale)?;

        let mut ultimo = 0usize;
        let esito = undo(&righe, |fatti, totale| {
            if fatti == totale || fatti.saturating_sub(ultimo) >= 25 {
                ultimo = fatti;
                let _ = app.emit("riordino:avanzamento", Avanzamento { fatti, totale });
            }
        });

        // Il giornale si rinomina invece di sparire: è la prova di cosa è
        // successo, e se questo annullamento è a sua volta fallito a metà è
        // l'unico posto in cui sta scritto quali file erano stati spostati.
        let annullato = giornale.with_extension("jsonl.annullato");
        let _ = std::fs::rename(&giornale, &annullato);

        Ok(EsitoRiordino {
            spostati: esito.moved.len(),
            falliti: falliti(&esito),
            cartelle_rimosse: esito.removed_dirs,
            annullabile: ultimo_giornale(&libreria.data_dir).is_some(),
        })
    })
    .map_err(errore)
}
