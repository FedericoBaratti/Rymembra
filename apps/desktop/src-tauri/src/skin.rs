//! La skin attiva, compilata dal nucleo.
//!
//! Questo modulo non produce una riga di CSS: la produce `aether-skin`, e qui si
//! sceglie soltanto quale skin compilare. È la stessa regola dei comandi —
//! nessuna decisione fuori dal nucleo — e vale doppio qui, perché su Android il
//! foglio non servirà affatto e serviranno gli stessi token in un'altra forma.
//!
//! # Il foglio di base, e perché è copiato in `stile.css`
//!
//! L'IPC risponde qualche millisecondo dopo il primo disegno. In quei
//! millisecondi la finestra esiste e i token no, quindi `stile.css` porta il
//! blocco della skin di serie sotto `:root`, e il compilatore lo sovrascrive con
//! `:root[data-skin='<id>']`, che è più specifico. È lo stesso strato che nel
//! vecchio albero era il blocco `:root` di `global.css`.
//!
//! Una copia è una cosa che può divergere in silenzio, ed è esattamente il
//! difetto che il motore delle skin esiste per togliere. Perciò non è affidata a
//! un commento: il test in fondo la confronta con l'uscita del compilatore.

use aether_domain::errors::{AppError, ErrorCode};
use serde::Serialize;

use crate::errore::{Esito, errore};

/// La skin usata quando non se ne chiede una.
pub const DI_SERIE: &str = "plain";

/// Le skin incluse nell'applicazione.
///
/// Una sola, per ora. Quando ci saranno quelle installate dall'utente, questa
/// funzione avrà un secondo ramo che le cerca su disco — e il tipo di ritorno
/// resta lo stesso, perché una skin è il suo manifest.
fn sorgente(id: &str) -> Option<&'static str> {
    match id {
        DI_SERIE => Some(aether_skin::PLAIN_SOURCE),
        _ => None,
    }
}

/// Una skin compilata, nella forma che la finestra riceve.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinIpc {
    /// L'identificatore, da mettere su `data-skin`.
    pub id: String,
    /// Il foglio, da adottare così com'è.
    pub css: String,
    /// Il costo dei motivi e delle superfici, sul budget della singola superficie.
    pub cost: u32,
    /// I token che seguono la copertina: quando ci sarà la riproduzione, sono
    /// quelli che il runtime dovrà riscrivere a ogni brano.
    pub dynamic_tokens: Vec<String>,
}

/// Compila una skin.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non è di nessuna skin conosciuta; i
/// codici della validazione se il manifest non è valido — cosa che per una skin
/// di serie sarebbe un guasto nostro, e che il test di fedeltà del nucleo scopre
/// prima di qui.
#[tauri::command]
pub fn skin(id: Option<String>) -> Esito<SkinIpc> {
    let id = id.unwrap_or_else(|| DI_SERIE.to_owned());
    let Some(sorgente) = sorgente(&id) else {
        return Err(errore(AppError::new(ErrorCode::SkinNotFound { id })));
    };
    let documento = aether_skin::parse_skin_json(sorgente).map_err(errore)?;
    let compilata = aether_skin::compile_skin(&documento);
    Ok(SkinIpc {
        id: compilata.id,
        css: compilata.css,
        cost: compilata.cost,
        dynamic_tokens: compilata
            .dynamic_tokens
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STILE: &str = include_str!("../../src/stile.css");
    const CONFIGURAZIONE: &str = include_str!("../tauri.conf.json");

    /// Il corpo di un blocco CSS: quel che sta fra la graffa aperta e la chiusa.
    fn corpo(css: &str, apertura: &str) -> String {
        let inizio = css
            .find(apertura)
            .map(|i| i + apertura.len())
            .unwrap_or_else(|| panic!("non trovo «{apertura}»"));
        let resto = css.get(inizio..).unwrap_or_default();
        let fine = resto
            .find("\n}")
            .unwrap_or_else(|| panic!("blocco non chiuso"));
        resto.get(..fine).unwrap_or_default().trim().to_owned()
    }

    fn compilata() -> aether_skin::CompiledSkin {
        let documento =
            aether_skin::parse_skin_json(aether_skin::PLAIN_SOURCE).expect("la skin di serie");
        aether_skin::compile_skin(&documento)
    }

    #[test]
    fn il_foglio_di_base_e_quello_che_produce_il_compilatore() {
        // Fra i due marcatori, `stile.css` porta l'uscita del compilatore sotto
        // `:root` invece che sotto `:root[data-skin='plain']`. Se qualcuno
        // ritocca un colore lì, questo test lo vede: senza, la finestra
        // lampeggerebbe del colore vecchio per un fotogramma a ogni avvio, che è
        // il tipo di difetto che si nota una volta su venti e non si riproduce.
        let generato = corpo(
            STILE,
            "/* ── inizio blocco generato ────────────────────────────────────────────────── */\n:root {",
        );
        let atteso = corpo(&compilata().css, ":root[data-skin='plain'] {");
        assert_eq!(
            generato, atteso,
            "\nrigenera con: cargo run -p aether-skin --example compila\n"
        );
    }

    #[test]
    fn il_fondo_della_finestra_e_quello_della_skin_di_serie() {
        // Il colore dell'avvio a freddo: lo dipinge il sistema operativo prima
        // che esista una pagina, quindi deve stare nella configurazione e non
        // può venire dall'IPC. Nel vecchio albero è il difetto ancora aperto —
        // MainActivity e capacitor.config cablano `#09090d`, il fondo di UNA
        // skin — e la differenza qui non è che il valore non sia cablato: è che
        // se si scolla dalla skin di serie, questo test lo dice.
        let css = compilata().css;
        let surface = css
            .lines()
            .find_map(|riga| riga.trim().strip_prefix("--color-surface-0: "))
            .map(|valore| valore.trim_end_matches(';'))
            .expect("la skin di serie dichiara il fondo");

        let configurato = CONFIGURAZIONE
            .lines()
            .find_map(|riga| riga.trim().strip_prefix("\"backgroundColor\": "))
            .map(|valore| valore.trim_end_matches(',').trim_matches('"'))
            .expect("la finestra dichiara un fondo");

        assert_eq!(
            aether_skin::values::parse_color(configurato),
            aether_skin::values::parse_color(surface),
            "il fondo della finestra ({configurato}) non è quello di «{DI_SERIE}» ({surface})"
        );
    }

    #[test]
    fn una_skin_che_non_esiste_lo_dice() {
        let err = skin(Some("nocturne".to_owned())).expect_err("compilata");
        assert_eq!(err.code, "skin.notFound");
        // Ritentare non ha senso e l'interfaccia deve saperlo senza leggere il
        // messaggio.
        assert!(!err.retryable);
    }

    #[test]
    fn senza_id_si_compila_quella_di_serie() {
        let compilata = skin(None).expect("compilata");
        assert_eq!(compilata.id, DI_SERIE);
        assert!(compilata.css.contains(":root[data-skin='plain']"));
        assert_eq!(compilata.cost, 0);
    }
}
