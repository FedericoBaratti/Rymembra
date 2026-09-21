//! La portabilità: la stessa musica, un'altra lettera di unità.
//!
//! # Il caso normale, non quello raro
//!
//! Un profilo si esporta su un computer e si importa su un altro. Le
//! preferenze attraversano senza problemi — un tema è un tema — ma i
//! **percorsi** no: `D:\Musica` sull'altra macchina è la lettera di un disco
//! che non c'è, o c'è e contiene altro. Fino alla 2.3.0 il profilo lo diceva e
//! basta: scriveva le radici così com'erano, avvisava che non esistevano, e
//! lasciava a chi importava il compito di riscriverle a mano nelle
//! impostazioni.
//!
//! Bastava finché il profilo portava solo le preferenze. Adesso porta anche la
//! libreria, e la libreria è fatta di `tracks.path`: diciottomila righe che
//! nominano `D:\Musica\…`. Riscrivere le due radici a mano e lasciare
//! diciottomila percorsi che puntano a un disco che non c'è vorrebbe dire una
//! libreria intera che non suona, con la scansione che la ricostruisce da capo
//! perdendo l'abbinamento con tutto quello che il profilo ha appena portato.
//!
//! # Cosa fa una rimappatura, e cosa non fa
//!
//! Dice «quel che stava sotto *questo* prefisso adesso sta sotto *quello*», e
//! si applica in tre posti: `library.roots`, `download.folder` e, brano per
//! brano, `tracks.path`.
//!
//! **Solo quando il file nuovo esiste davvero.** Un percorso riscritto verso un
//! file che non c'è è peggio di uno vecchio che non c'è: il vecchio dice la
//! verità su dov'era, il nuovo mente su dov'è. I brani che non si ritrovano si
//! **contano** e si dicono, e la loro riga resta esattamente com'era.
//!
//! **Nessuna riga si cancella.** Vale qui la stessa regola di tutto il resto
//! dell'importazione: un profilo aggiunge, non toglie.
//!
//! # Perché il confronto passa da `path_key` e non dalle stringhe
//!
//! Perché `D:\Musica`, `d:/musica` e `D:\Musica\` sono lo stesso posto, e un
//! confronto a stringhe li vedrebbe come tre. La chiave è la stessa che usa la
//! scansione per decidere se un file sta dentro una cartella sorvegliata, e
//! usarne una seconda qui vorrebbe dire due idee di «stesso posto» nello stesso
//! programma.
//!
//! Il confine di separatore è tutto il punto, e viene gratis da
//! [`aether_domain::paths::is_under`]: senza, `D:\Musica` conterrebbe
//! `D:\Musical`, e una rimappatura sposterebbe i percorsi di una cartella che
//! non c'entra niente.

use aether_domain::errors::AppError;
use aether_domain::paths::{PathRules, is_under, path_key};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::library::db_error;

/// Una radice del profilo che qui non c'è, e dove metterla.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rimappatura {
    /// Il prefisso come sta nel profilo.
    pub da: String,
    /// Il prefisso su questo computer. Vuoto finché nessuno l'ha scelto.
    pub a: String,
    /// Quanti brani di **questa** libreria stanno sotto `da`.
    ///
    /// È il numero che dà senso alla proposta: zero vuol dire che qui non c'è
    /// niente da spostare e la rimappatura riguarda solo le due preferenze;
    /// diciottomila vuol dire che è tutta la libreria.
    pub brani: usize,
}

/// Cosa una rimappatura ha cambiato.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Riscritti {
    /// Percorsi di brani riscritti, perché il file nuovo c'è.
    pub percorsi: usize,
    /// Brani il cui file, sotto il prefisso nuovo, non si trova.
    ///
    /// La loro riga resta com'era: dire dove **non** sono è meno utile che
    /// continuare a dire dove erano.
    pub irrintracciabili: usize,
    /// Brani il cui percorso nuovo è già di un'altra riga.
    ///
    /// Capita quando la stessa cartella è già stata scansionata dalla parte
    /// nuova: il brano c'è già, con la sua storia, e sovrascriverlo vorrebbe
    /// dire perderne una delle due. `tracks.path` è `UNIQUE`, quindi il
    /// database lo impedirebbe comunque — qui lo si conta invece di lasciare
    /// che faccia fallire tutta l'importazione.
    pub gia_presenti: usize,
}

/// Le regole di confronto di questa piattaforma.
fn regole() -> PathRules {
    PathRules::for_current_platform()
}

/// Toglie i separatori in coda, senza toccare il resto della grafia.
fn senza_coda(percorso: &str) -> &str {
    let potato = percorso.trim_end_matches(['/', '\\']);
    // Una radice che è **solo** separatori resterebbe vuota, e un prefisso
    // vuoto contiene mezzo filesystem. Meglio lasciarla com'era e lasciare che
    // `is_under` la rifiuti.
    if potato.is_empty() { percorso } else { potato }
}

/// Il percorso nuovo, se questo sta sotto `da`.
///
/// Restituisce `None` quando non ci sta sotto: il chiamante lo lascia com'era.
#[must_use]
pub fn riscrivi(percorso: &str, da: &str, a: &str) -> Option<String> {
    // Lo spazio si toglie prima dei separatori, e non è pedanteria: una
    // destinazione fatta di soli spazi è una proposta che nessuno ha
    // compilato, e senza questa riga passerebbe per un prefisso valido —
    // riscrivendo diciottomila percorsi verso «   \qualcosa».
    let da = senza_coda(da.trim());
    let a = senza_coda(a.trim());
    if a.is_empty() || !is_under(percorso, da, regole()) {
        return None;
    }
    // Il resto si prende dall'**originale** e non dalla chiave: la chiave ha
    // già unificato le barre e piegato le maiuscole, e ricostruire un percorso
    // da lì darebbe un `d:/musica/...` minuscolo con le barre al contrario da
    // aprire su Windows. Si taglia per lunghezza, dopo aver verificato che il
    // taglio cade su un confine di carattere e che il pezzo tagliato è davvero
    // `da`.
    let testa = percorso.get(..da.len())?;
    if path_key(testa, regole()) != path_key(da, regole()) {
        return None;
    }
    let resto = percorso.get(da.len()..)?;
    Some(format!("{a}{resto}"))
}

/// Una riga di proposta per ogni radice del profilo che qui non esiste.
///
/// `esiste` dice se un percorso c'è su questo computer; gliela si passa invece
/// di guardare il disco da qui, così questa funzione si prova senza avere le
/// cartelle di nessuno — è la stessa scelta che [`crate::profilo`] fa da
/// sempre.
///
/// # Errori
///
/// `db.queryFailed` se il conteggio dei brani fallisce.
pub fn proposte(
    connection: &Connection,
    radici: &[String],
    esiste: &dyn Fn(&str) -> bool,
) -> Result<Vec<Rimappatura>, AppError> {
    let mut proposte = Vec::new();
    for radice in radici {
        let radice = radice.trim();
        if radice.is_empty() || esiste(radice) {
            continue;
        }
        proposte.push(Rimappatura {
            da: radice.to_owned(),
            a: String::new(),
            brani: quanti_sotto(connection, radice)?,
        });
    }
    Ok(proposte)
}

/// Quanti brani di questa libreria stanno sotto un prefisso.
///
/// Si legge tutto e si conta in Rust invece di scrivere un `LIKE`: il confronto
/// deve passare da `path_key`, e un `LIKE 'D:\Musica%'` sarebbe insensibile al
/// confine di separatore e alle due grafie della barra — cioè darebbe un numero
/// diverso da quello che la rimappatura poi userà. Una scansione della colonna
/// `path` su diciottomila righe è una decina di millisecondi, una volta, mentre
/// si guarda un piano.
fn quanti_sotto(connection: &Connection, radice: &str) -> Result<usize, AppError> {
    let mut istruzione = connection
        .prepare("SELECT path FROM tracks WHERE path IS NOT NULL")
        .map_err(|err| db_error("conteggio dei brani sotto una radice", &err))?;
    let righe = istruzione
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("conteggio dei brani sotto una radice", &err))?;
    let mut quanti = 0;
    for riga in righe {
        let percorso = riga.map_err(|err| db_error("lettura di un percorso", &err))?;
        if is_under(&percorso, senza_coda(radice), regole()) {
            quanti += 1;
        }
    }
    Ok(quanti)
}

/// Riscrive un percorso solo, con la prima rimappatura che lo riguarda.
#[must_use]
pub fn rimappa_percorso(percorso: &str, rimappature: &[Rimappatura]) -> String {
    rimappature
        .iter()
        .find_map(|r| riscrivi(percorso, &r.da, &r.a))
        .unwrap_or_else(|| percorso.to_owned())
}

/// Riscrive un valore di preferenza che contiene percorsi.
///
/// Due forme, perché nel database ce ne sono due: `library.roots` è un elenco
/// JSON, `download.folder` è un percorso e basta. La distinzione la fa il
/// valore, non la chiave — vedi `percorsi_di` in [`crate::profilo`], che ha la
/// stessa scelta e la stessa ragione.
#[must_use]
pub fn rimappa_valore(valore: &str, rimappature: &[Rimappatura]) -> String {
    let rifai = |percorso: &str| -> String { rimappa_percorso(percorso, rimappature) };
    match serde_json::from_str::<Vec<String>>(valore) {
        Ok(elenco) => {
            let rifatti: Vec<String> = elenco.iter().map(|p| rifai(p)).collect();
            serde_json::to_string(&rifatti).unwrap_or_else(|_| valore.to_owned())
        }
        Err(_) => rifai(valore),
    }
}

/// Riscrive i percorsi dei brani, **solo quando il file nuovo esiste**.
///
/// `esiste` è la stessa funzione di [`proposte`], e serve qui per la ragione
/// più importante di tutte: senza, una rimappatura sbagliata riscriverebbe
/// diciottomila righe verso un posto vuoto e la libreria diventerebbe un elenco
/// di file che non si aprono. Con, una rimappatura sbagliata non fa niente e lo
/// dice.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub fn applica_in(
    tx: &Transaction<'_>,
    rimappature: &[Rimappatura],
    esiste: &dyn Fn(&str) -> bool,
) -> Result<Riscritti, AppError> {
    let vive: Vec<&Rimappatura> = rimappature
        .iter()
        .filter(|r| !r.da.trim().is_empty() && !r.a.trim().is_empty())
        .collect();
    if vive.is_empty() {
        return Ok(Riscritti::default());
    }

    let da_riscrivere: Vec<(i64, String)> = {
        let mut istruzione = tx
            .prepare("SELECT id, path FROM tracks WHERE path IS NOT NULL")
            .map_err(|err| db_error("percorsi da rimappare", &err))?;
        let righe = istruzione
            .query_map([], |riga| {
                Ok((riga.get::<_, i64>(0)?, riga.get::<_, String>(1)?))
            })
            .map_err(|err| db_error("percorsi da rimappare", &err))?;
        let mut trovati = Vec::new();
        for riga in righe {
            let (id, percorso) = riga.map_err(|err| db_error("lettura di un percorso", &err))?;
            if let Some(nuovo) = vive.iter().find_map(|r| riscrivi(&percorso, &r.da, &r.a)) {
                trovati.push((id, nuovo));
            }
        }
        trovati
    };

    let mut esito = Riscritti::default();
    let mut occupato = tx
        .prepare("SELECT 1 FROM tracks WHERE path = ?1 AND id <> ?2")
        .map_err(|err| db_error("percorsi da rimappare", &err))?;
    let mut sposta = tx
        .prepare("UPDATE tracks SET path = ?2 WHERE id = ?1")
        .map_err(|err| db_error("percorsi da rimappare", &err))?;

    for (id, nuovo) in da_riscrivere {
        if !esiste(&nuovo) {
            esito.irrintracciabili += 1;
            continue;
        }
        let gia = occupato
            .exists(rusqlite::params![nuovo, id])
            .map_err(|err| db_error("percorsi da rimappare", &err))?;
        if gia {
            esito.gia_presenti += 1;
            continue;
        }
        esito.percorsi += sposta
            .execute(rusqlite::params![id, nuovo])
            .map_err(|err| db_error("riscrittura di un percorso", &err))?;
    }
    Ok(esito)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_prefisso_si_sostituisce_conservando_la_grafia() {
        assert_eq!(
            riscrivi(
                r"D:\Musica\Pink Floyd\a.flac",
                r"D:\Musica",
                r"E:\Media\Musica"
            ),
            Some(r"E:\Media\Musica\Pink Floyd\a.flac".to_owned())
        );
        // La coda di separatori nella radice non cambia il risultato.
        assert_eq!(
            riscrivi(r"D:\Musica\a.flac", "D:\\Musica\\", r"E:\M"),
            Some(r"E:\M\a.flac".to_owned())
        );
    }

    #[test]
    fn il_confine_di_separatore_non_sposta_la_cartella_sbagliata() {
        // Senza il confine `D:\Musical` finirebbe sotto `D:\Musica`, e i suoi
        // percorsi verrebbero riscritti da una rimappatura che non lo riguarda.
        assert_eq!(riscrivi(r"D:\Musical\a.flac", r"D:\Musica", r"E:\M"), None);
    }

    #[test]
    fn una_destinazione_vuota_non_riscrive_niente() {
        // Una proposta che nessuno ha compilato deve restare una proposta.
        assert_eq!(riscrivi(r"D:\Musica\a.flac", r"D:\Musica", ""), None);
        assert_eq!(riscrivi(r"D:\Musica\a.flac", r"D:\Musica", "   "), None);
    }

    #[test]
    fn le_due_forme_di_valore_si_rimappano_tutte_e_due() {
        let rimappature = vec![Rimappatura {
            da: r"D:\Musica".to_owned(),
            a: r"E:\M".to_owned(),
            brani: 0,
        }];
        assert_eq!(
            rimappa_valore(r#"["D:\\Musica","F:\\Altro"]"#, &rimappature),
            r#"["E:\\M","F:\\Altro"]"#
        );
        assert_eq!(
            rimappa_valore(r"D:\Musica\Scarichi", &rimappature),
            r"E:\M\Scarichi"
        );
    }
}
