//! Le skin e le bozze dello Studio sul disco: quali ci sono, e come si scrivono.
//!
//! Il resto del backup passa dal database; questa parte no. Una skin è un
//! archivio in `<dati>\skin\<id>.aeskin`, una bozza è un `skin.json` in
//! `<dati>\skin\bozze\<id>\`. Sono l'unica parte del backup che non si può
//! ricostruire **né** da una scansione **né** dal database: se qualcuno ha
//! passato una serata a fare un tema, quella serata sta lì dentro.
//!
//! # Cosa non si salva
//!
//! `<dati>\skin\bozze\<id>\istantanee\*.json`. Sono una cronologia di
//! annullamento locale, tenuta a venti file e **riscritta a ogni salvataggio**
//! dello Studio: sarebbero la cosa più rumorosa del progetto anche sulla rete, e
//! non servono a niente su un computer diverso. Di una bozza si salva il solo
//! manifest.
//!
//! Nemmeno `plain`: viene dal binario, non dalla cartella. Un `plain.aeskin`
//! scritto qui sarebbe un file che nessuno legge — la ricerca della sorgente
//! risponde prima di guardare il disco — e che non si può più togliere
//! dall'elenco.
//!
//! # Le due regole del ripristino
//!
//! **Non si sovrascrive mai.** Né una skin installata né una bozza. Una skin
//! locale può essere stata modificata a mano; una bozza *è* lavoro non ancora
//! salvato. Un ripristino è una cosa che si fa quando si è già perso qualcosa:
//! non deve poter essere la seconda perdita.
//!
//! **Non ci si fida dei byte scaricati.** [`scrivi_skin`] li fa passare per
//! `aether_skin::read_skin_package`, che porta le guardie dell'archivio non
//! fidato — traversal, bomba di decompressione, dimensione dichiarata — e poi
//! controlla che l'identificatore dentro il pacchetto sia quello che abbiamo
//! chiesto. Il file arriva dalla rete: che sia il nostro Drive non lo rende un
//! archivio fidato.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aether_app::backup::impronta;
use aether_domain::errors::{AppError, ErrorCode};

/// La skin che viene dal binario e non dalla cartella.
pub const DI_SERIE: &str = "plain";

/// Il nome del manifest dentro la cartella di una bozza.
const MANIFEST: &str = "skin.json";

/// Dove stanno le skin installate.
fn cartella_skin(data_dir: &Path) -> PathBuf {
    data_dir.join("skin")
}

/// Dove stanno le bozze dello Studio.
fn cartella_bozze(data_dir: &Path) -> PathBuf {
    data_dir.join("skin").join("bozze")
}

/// L'identificatore è anche un nome di file: non deve poter uscire dalla
/// cartella.
///
/// `..`, le barre e i due punti di un percorso di Windows sono tutto ciò che
/// serve a scrivere altrove. Vale il doppio qui, dove l'identificatore arriva da
/// un file **scaricato**: la stessa regola dell'installazione a mano, applicata
/// nel punto in cui la sorgente è ancora meno controllabile.
///
/// Volutamente identica a `skin::id_sicuro` nell'applicazione. Sarebbe un
/// candidato a stare nel dominio; finché non ci sta, una copia di sei righe con
/// questa nota costa meno di una dipendenza fra la finestra e la nuvola.
#[must_use]
pub fn id_sicuro(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Le skin installate, con l'impronta del loro archivio.
///
/// # Perché si ridigerisce ogni volta invece di ricordarsi le impronte
///
/// Era previsto tenerle in cache in `settings`, con chiave `(id, mtime,
/// size)`. Non si fa, per due ragioni. La prima è che costerebbe a questo crate
/// una dipendenza dal database, cioè proprio l'invariante che tiene la rete
/// lontana dal lucchetto della libreria. La seconda è che non serve: blake3
/// macina più di un gigabyte al secondo, quindi cinque skin da venti megabyte
/// sono meno di cento millisecondi, una volta ogni quarto d'ora, su un filo di
/// sottofondo. Una cache costerebbe più in invalidazione di quanto risparmi.
///
/// Un file illeggibile non è un guasto: si salta. Su una cartella che l'utente
/// può toccare a mano, l'alternativa sarebbe che un file rotto impedisca il
/// backup di tutto il resto.
///
/// # Errori
///
/// Nessuno: una cartella che non esiste è un'installazione senza skin.
#[must_use]
pub fn elenca_skin(data_dir: &Path) -> BTreeMap<String, String> {
    let mut trovate = BTreeMap::new();
    let Ok(voci) = std::fs::read_dir(cartella_skin(data_dir)) else {
        return trovate;
    };
    for voce in voci.flatten() {
        let percorso = voce.path();
        let Some(id) = percorso
            .file_name()
            .and_then(|nome| nome.to_str())
            .and_then(|nome| nome.strip_suffix(".aeskin"))
        else {
            continue;
        };
        if !id_sicuro(id) || id == DI_SERIE {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&percorso) {
            trovate.insert(id.to_owned(), impronta(&bytes));
        }
    }
    trovate
}

/// Le bozze dello Studio, con l'impronta del loro manifest.
///
/// Una cartella di bozza senza `skin.json` non è una bozza: è il residuo di un
/// salvataggio interrotto, e non c'è niente da salvare.
#[must_use]
pub fn elenca_bozze(data_dir: &Path) -> BTreeMap<String, String> {
    let mut trovate = BTreeMap::new();
    let Ok(voci) = std::fs::read_dir(cartella_bozze(data_dir)) else {
        return trovate;
    };
    for voce in voci.flatten() {
        let Some(id) = voce.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if !id_sicuro(&id) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(voce.path().join(MANIFEST)) {
            trovate.insert(id, impronta(&bytes));
        }
    }
    trovate
}

/// I byte di una skin installata.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non è utilizzabile o il file non c'è.
pub fn leggi_skin(data_dir: &Path, id: &str) -> Result<Vec<u8>, AppError> {
    if !id_sicuro(id) {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    let percorso = cartella_skin(data_dir).join(format!("{id}.aeskin"));
    std::fs::read(&percorso).map_err(|err| {
        AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }).with_cause(err.to_string())
    })
}

/// Il manifest di una bozza.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non è utilizzabile o la bozza non c'è.
pub fn leggi_bozza(data_dir: &Path, id: &str) -> Result<Vec<u8>, AppError> {
    if !id_sicuro(id) {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    let percorso = cartella_bozze(data_dir).join(id).join(MANIFEST);
    std::fs::read(&percorso).map_err(|err| {
        AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }).with_cause(err.to_string())
    })
}

/// Installa una skin scaricata, **senza mai sovrascriverne una che c'è**.
///
/// Restituisce `false` quando la skin c'era già: non è un guasto, è la
/// condizione normale di un ripristino ripetuto.
///
/// # I tre controlli, e cosa impedisce ciascuno
///
/// 1. `id_sicuro` — l'identificatore diventa un nome di file.
/// 2. `read_skin_package` — l'archivio arriva dalla rete, e porta con sé tutte
///    le trappole di un archivio non fidato.
/// 3. **L'identificatore dentro il pacchetto deve essere quello chiesto.** È il
///    controllo che sembra pedante e non lo è: senza, un file di nome
///    `aether-skin-notte.aeskin` il cui manifest dichiara `id: "giorno"`
///    finirebbe scritto come `notte.aeskin` con dentro un'altra skin, e l'unica
///    persona a scoprirlo sarebbe chi si ritrova il tema sbagliato.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non va o non combacia; i codici di
/// `aether_skin` se l'archivio non è valido; `fs.writeFailed` se la scrittura
/// fallisce.
pub fn scrivi_skin(data_dir: &Path, id: &str, bytes: &[u8]) -> Result<bool, AppError> {
    if !id_sicuro(id) || id == DI_SERIE {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    let percorso = cartella_skin(data_dir).join(format!("{id}.aeskin"));
    if percorso.exists() {
        return Ok(false);
    }

    let pacchetto = aether_skin::read_skin_package(bytes)?;
    if pacchetto.document.id != id {
        return Err(
            AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }).with_cause(format!(
                "il pacchetto dichiara «{}» invece di «{id}»",
                pacchetto.document.id
            )),
        );
    }

    let cartella = cartella_skin(data_dir);
    std::fs::create_dir_all(&cartella).map_err(|err| scrittura(&cartella, &err))?;
    std::fs::write(&percorso, bytes).map_err(|err| scrittura(&percorso, &err))?;
    Ok(true)
}

/// Scrive una bozza dello Studio, **solo se non c'è già**.
///
/// Restituisce `false` quando la bozza c'era. Una bozza è lavoro non salvato: un
/// ripristino non se lo mangia, nemmeno se quello nel backup è più recente —
/// «più recente» qui vorrebbe dire «salvato più tardi su un altro computer», che
/// non dice niente su quale dei due valga di più.
///
/// Il manifest si valida prima di scriverlo: una bozza illeggibile farebbe
/// fallire l'apertura dello Studio, e da lì non c'è modo di rimediare se non
/// andando a cancellare un file a mano.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non va; i codici di validazione se il
/// manifest non è valido; `fs.writeFailed` se la scrittura fallisce.
pub fn scrivi_bozza(data_dir: &Path, id: &str, bytes: &[u8]) -> Result<bool, AppError> {
    if !id_sicuro(id) {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    let cartella = cartella_bozze(data_dir).join(id);
    if cartella.exists() {
        return Ok(false);
    }

    let sorgente = std::str::from_utf8(bytes).map_err(|err| {
        AppError::new(ErrorCode::SkinManifestInvalid {
            detail: Some("la bozza non è testo".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    aether_skin::parse_skin_json(sorgente)?;

    std::fs::create_dir_all(&cartella).map_err(|err| scrittura(&cartella, &err))?;
    let file = cartella.join(MANIFEST);
    std::fs::write(&file, bytes).map_err(|err| scrittura(&file, &err))?;
    Ok(true)
}

/// Una scrittura fallita, nominando il file.
fn scrittura(percorso: &Path, err: &std::io::Error) -> AppError {
    AppError::new(ErrorCode::FsWriteFailed {
        path: percorso.display().to_string(),
        detail: Some(err.kind().to_string()),
    })
    .with_cause(err.to_string())
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un pacchetto `.aeskin` con l'identificatore dato.
    fn pacchetto(id: &str) -> Vec<u8> {
        let sorgente =
            aether_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""));
        aether_skin::write_skin_package(&aether_skin::package::WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[],
        })
        .expect("pacchetto")
    }

    #[test]
    fn un_identificatore_che_esce_dalla_cartella_si_rifiuta() {
        // Sono le tre forme che bastano a scrivere altrove sul disco.
        assert!(!id_sicuro("../fuori"));
        assert!(!id_sicuro("a/b"));
        assert!(!id_sicuro("C:\\altrove"));
        assert!(!id_sicuro(""));
        assert!(!id_sicuro(&"a".repeat(65)));
        assert!(id_sicuro("notte-2_v3"));
    }

    #[test]
    fn una_cartella_che_non_esiste_e_un_installazione_senza_skin() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert!(elenca_skin(dir.path()).is_empty());
        assert!(elenca_bozze(dir.path()).is_empty());
    }

    #[test]
    fn le_skin_installate_si_elencano_con_la_loro_impronta() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let bytes = pacchetto("notte");
        assert_eq!(scrivi_skin(dir.path(), "notte", &bytes), Ok(true));

        let elenco = elenca_skin(dir.path());
        assert_eq!(elenco.len(), 1);
        assert_eq!(elenco.get("notte"), Some(&impronta(&bytes)));
    }

    #[test]
    fn una_skin_gia_installata_non_si_sovrascrive() {
        // Potrebbe essere stata modificata a mano. Un ripristino non se la
        // mangia, e ripeterlo non è un guasto.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        scrivi_skin(dir.path(), "notte", &pacchetto("notte")).expect("prima");
        let mia = std::fs::read(dir.path().join("skin").join("notte.aeskin")).expect("letta");

        assert_eq!(
            scrivi_skin(dir.path(), "notte", &pacchetto("notte")),
            Ok(false),
            "la seconda volta non fa niente e non è un errore"
        );
        let dopo = std::fs::read(dir.path().join("skin").join("notte.aeskin")).expect("letta");
        assert_eq!(mia, dopo);
    }

    #[test]
    fn un_pacchetto_che_dichiara_un_altro_identificatore_si_rifiuta() {
        // Senza questo controllo, `notte.aeskin` finirebbe scritto con dentro
        // la skin `giorno`, e l'unico a scoprirlo sarebbe chi si ritrova il tema
        // sbagliato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let err = scrivi_skin(dir.path(), "notte", &pacchetto("giorno")).expect_err("respinta");
        assert_eq!(err.code().kind().code(), "skin.notFound");
        assert!(!dir.path().join("skin").join("notte.aeskin").exists());
    }

    #[test]
    fn quel_che_non_e_un_archivio_non_si_scrive() {
        // I byte arrivano dalla rete: che siano dal nostro Drive non li rende
        // un archivio fidato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert!(scrivi_skin(dir.path(), "notte", b"non e uno zip").is_err());
        assert!(!dir.path().join("skin").join("notte.aeskin").exists());
    }

    #[test]
    fn la_skin_di_serie_non_si_installa_ne_si_elenca() {
        // `plain` viene dal binario: un file sarebbe illeggibile e
        // incancellabile dall'elenco.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert!(scrivi_skin(dir.path(), DI_SERIE, &pacchetto(DI_SERIE)).is_err());

        // E se ce lo trovasse comunque, l'elenco lo salta: non ha senso
        // spedire su Drive una copia di quel che sta già dentro il binario.
        std::fs::create_dir_all(dir.path().join("skin")).expect("cartella");
        std::fs::write(
            dir.path().join("skin").join("plain.aeskin"),
            pacchetto(DI_SERIE),
        )
        .expect("scritta");
        assert!(elenca_skin(dir.path()).is_empty());
    }

    #[test]
    fn una_bozza_fa_andata_e_ritorno() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let sorgente = aether_skin::PLAIN_SOURCE.as_bytes();
        assert_eq!(scrivi_bozza(dir.path(), "in-corso", sorgente), Ok(true));

        assert_eq!(leggi_bozza(dir.path(), "in-corso").as_deref(), Ok(sorgente));
        let elenco = elenca_bozze(dir.path());
        assert_eq!(elenco.get("in-corso"), Some(&impronta(sorgente)));
    }

    #[test]
    fn una_bozza_gia_qui_non_si_tocca() {
        // Una bozza è lavoro non ancora salvato: è la cosa che un backup non ha
        // mai salvato e che un ripristino non deve poter cancellare.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let mia = aether_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
        scrivi_bozza(dir.path(), "in-corso", mia.as_bytes()).expect("prima");

        assert_eq!(
            scrivi_bozza(dir.path(), "in-corso", aether_skin::PLAIN_SOURCE.as_bytes()),
            Ok(false)
        );
        assert_eq!(
            leggi_bozza(dir.path(), "in-corso").as_deref(),
            Ok(mia.as_bytes()),
            "il lavoro di qui è rimasto"
        );
    }

    #[test]
    fn una_bozza_illeggibile_non_si_scrive() {
        // Una bozza rotta farebbe fallire l'apertura dello Studio, e da lì non
        // c'è modo di rimediare senza andare a cancellare un file a mano.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert!(scrivi_bozza(dir.path(), "rotta", b"{non json").is_err());
        assert!(!dir.path().join("skin").join("bozze").join("rotta").exists());
    }

    #[test]
    fn le_istantanee_non_finiscono_nell_elenco_delle_bozze() {
        // Sono una cronologia di annullamento locale, riscritta a ogni
        // salvataggio: la cosa più rumorosa del progetto, anche sulla rete.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        scrivi_bozza(dir.path(), "in-corso", aether_skin::PLAIN_SOURCE.as_bytes())
            .expect("scritta");
        let istantanee = dir
            .path()
            .join("skin")
            .join("bozze")
            .join("in-corso")
            .join("istantanee");
        std::fs::create_dir_all(&istantanee).expect("cartella");
        std::fs::write(istantanee.join("1-manuale-2-x.json"), "{}").expect("istantanea");

        let elenco = elenca_bozze(dir.path());
        assert_eq!(elenco.len(), 1, "una bozza, non due");
        assert_eq!(
            elenco.get("in-corso"),
            Some(&impronta(aether_skin::PLAIN_SOURCE.as_bytes())),
            "l'impronta è quella del manifest, non della cartella"
        );
    }
}
