//! Le skin e le bozze dello Studio sul disco: quali ci sono, e come si scrivono.
//!
//! Il resto del backup passa dal database; questa parte no. Una skin è un
//! archivio in `<dati>\skin\<id>.ryskin`, una bozza è un `skin.json` in
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
//! Nemmeno `plain`: viene dal binario, non dalla cartella. Un `plain.ryskin`
//! scritto qui sarebbe un file che nessuno legge — la ricerca della sorgente
//! risponde prima di guardare il disco — e che non si può più togliere
//! dall'elenco.
//!
//! # Sul disco, solo l'estensione di adesso
//!
//! Le skin installate hanno l'estensione `.ryskin` e basta: qui non si elencano,
//! non si leggono e non si scrivono pacchetti con quella di prima
//! ([`rymembra_domain::eredita::ESTENSIONE_SKIN`]). Quelli già sul disco li rinomina il trasloco
//! dei dati all'avvio, prima che questo modulo li guardi; sul lato di Drive
//! invece i nomi di prima si leggono ancora, ed è compito di [`crate::servizio`].
//!
//! # Le due regole del ripristino
//!
//! **Non si sovrascrive mai il lavoro di qualcuno.** Né una skin installata né
//! una bozza. Una skin locale può essere stata modificata a mano; una bozza *è*
//! lavoro non ancora salvato. Un ripristino è una cosa che si fa quando si è già
//! perso qualcosa: non deve poter essere la seconda perdita.
//!
//! Quel che si sostituisce è soltanto ciò che il lavoro di nessuno non può
//! essere: una copia di quegli stessi byte rimasta a metà, e una skin che non si
//! apre nemmeno come archivio. Il perché, e come si riconoscono, sta in
//! [`da_scrivere`]. E da qui in poi si scrive in modo che a metà non resti
//! niente: vedi [`scrivi`].
//!
//! **Non ci si fida dei byte scaricati.** [`scrivi_skin`] li fa passare per
//! `rymembra_skin::read_skin_package`, che porta le guardie dell'archivio non
//! fidato — traversal, bomba di decompressione, dimensione dichiarata — e poi
//! controlla che l'identificatore dentro il pacchetto sia quello che abbiamo
//! chiesto. Il file arriva dalla rete: che sia il nostro Drive non lo rende un
//! archivio fidato.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rymembra_app::backup::impronta;
use rymembra_app::scrittura::scrivi_atomico;
use rymembra_domain::errors::{AppError, ErrorCode};

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
            .and_then(|nome| nome.strip_suffix(".ryskin"))
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

/// Come [`elenca_skin`], senza le skin il cui archivio non si apre.
///
/// # Perché il backup e il ripristino non devono vederle
///
/// Un `.ryskin` troncato — una copia interrotta, un disco che ha perso un
/// settore — ha un'impronta diversa da quella del backup. Per il backup
/// «diversa» vuol dire «cambiata», e la passata dopo lo caricherebbe **sopra**
/// la copia buona che sta su Drive: il guasto di un disco diventerebbe il
/// guasto del backup, l'unico posto da cui si sarebbe potuto rimediare. Per il
/// ripristino, una skin elencata vuol dire «già installata», e la copia buona
/// non verrebbe mai riscaricata. Lasciarla fuori fa il giusto da tutti e due i
/// lati: il backup non la tocca (e lassù non si cancella mai niente), il
/// ripristino la vede mancante e la rimette da Drive.
///
/// Rotta vuol dire soltanto `skin.packageCorrupt`, come in [`scrivi_skin`]:
/// una skin che si apre e porta una voce che questa versione non conosce può
/// venire da una versione più nuova, ed è sana.
#[must_use]
pub fn elenca_skin_sane(data_dir: &Path) -> BTreeMap<String, String> {
    let mut trovate = elenca_skin(data_dir);
    trovate.retain(|id, _| !leggi_skin(data_dir, id).is_ok_and(|bytes| archivio_rotto(&bytes)));
    trovate
}

/// L'archivio di una skin non si apre nemmeno come archivio.
fn archivio_rotto(bytes: &[u8]) -> bool {
    rymembra_skin::package::leggi_manifesto(bytes)
        .is_err_and(|err| matches!(err.code(), ErrorCode::SkinPackageCorrupt { .. }))
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
    let percorso = cartella_skin(data_dir).join(format!("{id}.ryskin"));
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

/// Installa una skin scaricata, **senza mai sovrascriverne una sana che c'è**.
///
/// Restituisce `false` quando la skin c'era già — identica, o diversa e sana:
/// non è un guasto, è la condizione normale di un ripristino ripetuto. Una che
/// c'è ma è rotta — una copia a metà, un archivio che non si apre — si
/// sostituisce: vedi [`da_scrivere`].
///
/// # I tre controlli, e cosa impedisce ciascuno
///
/// 1. `id_sicuro` — l'identificatore diventa un nome di file.
/// 2. `read_skin_package` — l'archivio arriva dalla rete, e porta con sé tutte
///    le trappole di un archivio non fidato.
/// 3. **L'identificatore dentro il pacchetto deve essere quello chiesto.** È il
///    controllo che sembra pedante e non lo è: senza, un file di nome
///    `rymembra-skin-notte.ryskin` il cui manifest dichiara `id: "giorno"`
///    finirebbe scritto come `notte.ryskin` con dentro un'altra skin, e l'unica
///    persona a scoprirlo sarebbe chi si ritrova il tema sbagliato.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non va o non combacia; i codici di
/// `rymembra_skin` se l'archivio non è valido; `fs.writeFailed` se la scrittura
/// fallisce.
pub fn scrivi_skin(data_dir: &Path, id: &str, bytes: &[u8]) -> Result<bool, AppError> {
    if !id_sicuro(id) || id == DI_SERIE {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    let percorso = cartella_skin(data_dir).join(format!("{id}.ryskin"));
    // Un archivio che non si apre nemmeno come archivio non è il lavoro di
    // nessuno: è l'avanzo di una scrittura interrotta, anche se viene da un
    // backup diverso da questo. Vedi [`archivio_rotto`].
    if !da_scrivere(&percorso, bytes, archivio_rotto) {
        return Ok(false);
    }

    let pacchetto = rymembra_skin::read_skin_package(bytes)?;
    if pacchetto.document.id != id {
        return Err(
            AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }).with_cause(format!(
                "il pacchetto dichiara «{}» invece di «{id}»",
                pacchetto.document.id
            )),
        );
    }

    scrivi(&percorso, bytes)?;
    Ok(true)
}

/// Scrive una bozza dello Studio, **solo se non c'è già**.
///
/// Restituisce `false` quando la bozza c'era. «C'era» vuol dire il suo
/// `skin.json`, non la cartella, e non una copia a metà di questi stessi byte:
/// vedi [`da_scrivere`]. Una bozza è lavoro non salvato: un
/// ripristino non se lo mangia, nemmeno se quello nel backup è più recente —
/// «più recente» qui vorrebbe dire «salvato più tardi su un altro computer», che
/// non dice niente su quale dei due valga di più.
///
/// La bozza si ripristina **com'è**, come lo Studio l'aveva salvata: anche
/// una che non passa la validazione. Lo Studio salva il lavoro in corso così
/// com'è, e lo apre così com'è — la validazione gli dice dove sta l'errore —,
/// e una bozza non diventa una skin installata senza passare dall'esportazione,
/// che valida. Fino alla 3.2.0 di prova si validava qui, e una bozza salvata a metà di
/// una modifica non tornava mai dal backup: il lavoro c'era, su Drive, e
/// restava lì.
///
/// Si controlla solo che sia testo: un manifest che non è UTF-8 lo Studio non
/// lo legge, e ripiegherebbe in silenzio sul pacchetto installato, quindi
/// scriverlo non riporterebbe niente.
///
/// # Errori
///
/// `skin.notFound` se l'identificatore non va; `skin.manifestInvalid` se il
/// manifest non è testo UTF-8; `fs.writeFailed` se la scrittura fallisce.
pub fn scrivi_bozza(data_dir: &Path, id: &str, bytes: &[u8]) -> Result<bool, AppError> {
    if !id_sicuro(id) {
        return Err(AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() }));
    }
    // Il manifest e non la cartella: una cartella senza `skin.json` è il
    // residuo di un salvataggio interrotto — [`elenca_bozze`] non la elenca, e
    // proprio per questo il piano la chiede — e «la cartella c'è» la saltava
    // a ogni ripristino, per sempre.
    //
    // Nessun «rotto» oltre la copia a metà: lo Studio salva anche le bozze che
    // non passano la validazione, perché sono lavoro in corso, e un manifest
    // che non si legge può essere esattamente quello.
    let file = cartella_bozze(data_dir).join(id).join(MANIFEST);
    if !da_scrivere(&file, bytes, |_| false) {
        return Ok(false);
    }

    std::str::from_utf8(bytes).map_err(|err| {
        AppError::new(ErrorCode::SkinManifestInvalid {
            detail: Some("la bozza non è testo".to_owned()),
        })
        .with_cause(err.to_string())
    })?;

    scrivi(&file, bytes)?;
    Ok(true)
}

/// Se un ripristino deve scrivere `percorso` con `remoti`, guardando cosa c'è
/// già sul disco.
///
/// # Il confronto, al posto di «c'è già»
///
/// Fino alla 3.0 bastava che il file ci fosse. Ma il ripristino scriveva con un
/// `fs::write` qualunque, e uno interrotto — la corrente, un'uscita forzata, un
/// disco pieno — lasciava il file col nome giusto e dentro una parte del
/// pacchetto: un `.ryskin` che non si apre, che «c'era già» a ogni ripristino
/// dopo e che quindi non tornava più. Adesso si guarda **cosa** c'è:
///
/// * **niente** — si scrive;
/// * **gli stessi byte** — non c'è niente da fare, ed è il caso normale di un
///   ripristino ripetuto;
/// * **una copia a metà di questi byte** — più corta o lunga uguale, e dove
///   differisce ci sono zeri: è quel che `write` lascia quando si ferma, o
///   quando la lunghezza è arrivata sul disco e il contenuto no. Nessuno
///   scrive a mano un file così, e si completa;
/// * **qualcosa che `rotto` riconosce come guasto** — si sostituisce. Per le
///   skin è un archivio che non si apre; per le bozze niente, vedi
///   [`scrivi_bozza`];
/// * **altro** — è lavoro di qualcuno, e resta. Anche quando non si legge: un
///   file che il ripristino non sa interpretare non è un file che il
///   ripristino ha il diritto di buttare.
///
/// Un errore di lettura diverso da «non c'è» vale come «altro»: se il disco
/// non lascia leggere, scriverci sopra alla cieca è peggio che non ripristinare.
fn da_scrivere(percorso: &Path, remoti: &[u8], rotto: impl Fn(&[u8]) -> bool) -> bool {
    let locali = match std::fs::read(percorso) {
        Ok(locali) => locali,
        Err(err) => return err.kind() == std::io::ErrorKind::NotFound,
    };
    if locali == remoti {
        return false;
    }
    let a_meta = locali.len() <= remoti.len()
        && locali
            .iter()
            .zip(remoti)
            .all(|(locale, remoto)| locale == remoto || *locale == 0);
    a_meta || rotto(&locali)
}

/// Scrive un pacchetto ripristinato in modo che nessuno possa trovarlo a metà.
///
/// Passa da [`rymembra_app::scrittura::scrivi_atomico`]: un ripristino interrotto
/// adesso lascia il file di prima o nessun file, mai un pezzo — ed è quel che
/// rende vero il confronto di [`da_scrivere`] anche domani. Crea le cartelle
/// che mancano.
fn scrivi(percorso: &Path, bytes: &[u8]) -> Result<(), AppError> {
    scrivi_atomico(percorso, bytes).map_err(|err| {
        AppError::new(ErrorCode::FsWriteFailed {
            path: percorso.display().to_string(),
            detail: Some(err.code().kind().code().to_owned()),
        })
        .with_cause(err.to_string())
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un pacchetto `.ryskin` con l'identificatore dato.
    fn pacchetto(id: &str) -> Vec<u8> {
        let sorgente =
            rymembra_skin::PLAIN_SOURCE.replace("\"id\": \"plain\"", &format!("\"id\": \"{id}\""));
        rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
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
    fn una_skin_rotta_in_locale_non_va_nel_backup_e_si_riscarica() {
        // `notte` è sana, `giorno` è stata troncata da una copia interrotta.
        // Il backup non deve vedere `giorno` — la caricherebbe sopra la copia
        // buona di Drive — e il ripristino la deve vedere mancante, così la
        // copia buona torna al suo posto.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let notte = pacchetto("notte");
        let giorno = pacchetto("giorno");
        assert_eq!(scrivi_skin(dir.path(), "notte", &notte), Ok(true));
        let percorso = cartella_skin(dir.path()).join("giorno.ryskin");
        std::fs::write(
            &percorso,
            giorno.get(..giorno.len().div_euclid(3)).expect("un terzo"),
        )
        .expect("troncata");

        assert_eq!(
            elenca_skin(dir.path()).len(),
            2,
            "elencate lo sono tutte e due"
        );
        let sane = elenca_skin_sane(dir.path());
        assert_eq!(sane.keys().collect::<Vec<_>>(), vec!["notte"]);

        // Il ripristino che la riscarica la rimette intera.
        assert_eq!(scrivi_skin(dir.path(), "giorno", &giorno), Ok(true));
        assert_eq!(std::fs::read(&percorso).expect("rilettura"), giorno);
        assert_eq!(elenca_skin_sane(dir.path()).len(), 2);
    }

    #[test]
    fn una_skin_gia_installata_non_si_sovrascrive() {
        // Potrebbe essere stata modificata a mano. Un ripristino non se la
        // mangia, e ripeterlo non è un guasto.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        scrivi_skin(dir.path(), "notte", &pacchetto("notte")).expect("prima");
        let mia = std::fs::read(dir.path().join("skin").join("notte.ryskin")).expect("letta");

        assert_eq!(
            scrivi_skin(dir.path(), "notte", &pacchetto("notte")),
            Ok(false),
            "la seconda volta non fa niente e non è un errore"
        );
        let dopo = std::fs::read(dir.path().join("skin").join("notte.ryskin")).expect("letta");
        assert_eq!(mia, dopo);
    }

    #[test]
    fn un_pacchetto_che_dichiara_un_altro_identificatore_si_rifiuta() {
        // Senza questo controllo, `notte.ryskin` finirebbe scritto con dentro
        // la skin `giorno`, e l'unico a scoprirlo sarebbe chi si ritrova il tema
        // sbagliato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let err = scrivi_skin(dir.path(), "notte", &pacchetto("giorno")).expect_err("respinta");
        assert_eq!(err.code().kind().code(), "skin.notFound");
        assert!(!dir.path().join("skin").join("notte.ryskin").exists());
    }

    #[test]
    fn quel_che_non_e_un_archivio_non_si_scrive() {
        // I byte arrivano dalla rete: che siano dal nostro Drive non li rende
        // un archivio fidato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        assert!(scrivi_skin(dir.path(), "notte", b"non e uno zip").is_err());
        assert!(!dir.path().join("skin").join("notte.ryskin").exists());
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
            dir.path().join("skin").join("plain.ryskin"),
            pacchetto(DI_SERIE),
        )
        .expect("scritta");
        assert!(elenca_skin(dir.path()).is_empty());
    }

    #[test]
    fn sul_disco_si_vede_e_si_scrive_solo_l_estensione_di_adesso() {
        // Una skin con l'estensione di prima non è ancora stata portata a quella
        // nuova (lo fa il trasloco, all'avvio): qui non esiste, e non va
        // spedita su Drive col nome sbagliato.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let skin = dir.path().join("skin");
        std::fs::create_dir_all(&skin).expect("cartella");
        std::fs::write(
            skin.join(format!(
                "vecchia.{}",
                rymembra_domain::eredita::ESTENSIONE_SKIN
            )),
            pacchetto("vecchia"),
        )
        .expect("skin di prima");
        assert!(elenca_skin(dir.path()).is_empty());
        assert!(leggi_skin(dir.path(), "vecchia").is_err());

        // E una skin che si installa ora ha l'estensione di adesso, sempre.
        scrivi_skin(dir.path(), "nuova", &pacchetto("nuova")).expect("scritta");
        let nomi: Vec<String> = std::fs::read_dir(&skin)
            .expect("cartella")
            .flatten()
            .map(|voce| voce.file_name().to_string_lossy().into_owned())
            .filter(|nome| nome.starts_with("nuova"))
            .collect();
        assert_eq!(nomi, ["nuova.ryskin"]);
        assert_eq!(
            elenca_skin(dir.path()).keys().collect::<Vec<_>>(),
            ["nuova"]
        );
    }

    #[test]
    fn una_bozza_fa_andata_e_ritorno() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let sorgente = rymembra_skin::PLAIN_SOURCE.as_bytes();
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
        let mia = rymembra_skin::PLAIN_SOURCE.replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
        scrivi_bozza(dir.path(), "in-corso", mia.as_bytes()).expect("prima");

        assert_eq!(
            scrivi_bozza(
                dir.path(),
                "in-corso",
                rymembra_skin::PLAIN_SOURCE.as_bytes()
            ),
            Ok(false)
        );
        assert_eq!(
            leggi_bozza(dir.path(), "in-corso").as_deref(),
            Ok(mia.as_bytes()),
            "il lavoro di qui è rimasto"
        );
    }

    #[test]
    fn una_bozza_non_valida_si_ripristina_com_era() {
        // Lo Studio salva anche le bozze che non passano la validazione — una
        // virgola di troppo mentre si scrive — e il backup le carica così:
        // rifiutarle al ripristino perdeva proprio il lavoro in corso.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let a_meta: &[u8] = b"{ \"a meta\": ";
        assert_eq!(scrivi_bozza(dir.path(), "in-corso", a_meta), Ok(true));
        assert_eq!(
            leggi_bozza(dir.path(), "in-corso").expect("scritta"),
            a_meta,
            "gli stessi byte, come lo Studio li aveva salvati"
        );

        // Un manifest che non è testo no: lo Studio non lo aprirebbe, e
        // ripiegherebbe in silenzio sul pacchetto installato.
        assert!(scrivi_bozza(dir.path(), "non-testo", &[0xFF, 0xFE]).is_err());
        assert!(
            !dir.path()
                .join("skin")
                .join("bozze")
                .join("non-testo")
                .exists()
        );
    }

    #[test]
    fn le_istantanee_non_finiscono_nell_elenco_delle_bozze() {
        // Sono una cronologia di annullamento locale, riscritta a ogni
        // salvataggio: la cosa più rumorosa del progetto, anche sulla rete.
        let dir = tempfile::tempdir().expect("cartella temporanea");
        scrivi_bozza(
            dir.path(),
            "in-corso",
            rymembra_skin::PLAIN_SOURCE.as_bytes(),
        )
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
            Some(&impronta(rymembra_skin::PLAIN_SOURCE.as_bytes())),
            "l'impronta è quella del manifest, non della cartella"
        );
    }

    /// I nomi dei file in una cartella, ordinati.
    fn nomi(cartella: &Path) -> Vec<String> {
        let mut nomi: Vec<String> = std::fs::read_dir(cartella)
            .expect("cartella")
            .filter_map(|voce| Some(voce.ok()?.file_name().to_string_lossy().into_owned()))
            .collect();
        nomi.sort();
        nomi
    }

    /// Il guasto che il ripristino lasciava: un `.ryskin` scritto a metà da un
    /// ripristino interrotto, col nome giusto. «C'è già» lo saltava a ogni
    /// ripristino dopo, per sempre.
    #[test]
    fn una_skin_troncata_da_un_ripristino_interrotto_si_rimpiazza() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let bytes = pacchetto("notte");
        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        let mezzo = bytes.get(..bytes.len().div_euclid(2)).expect("metà");
        std::fs::write(cartella.join("notte.ryskin"), mezzo).expect("troncata");

        assert_eq!(scrivi_skin(dir.path(), "notte", &bytes), Ok(true));
        assert_eq!(
            std::fs::read(cartella.join("notte.ryskin")).expect("letta"),
            bytes
        );
        assert_eq!(nomi(&cartella), ["notte.ryskin"], "nessun temporaneo");
    }

    /// Un archivio che non si apre nemmeno come archivio non è il lavoro di
    /// nessuno: anche se non è la metà di **questo** pacchetto — il ripristino
    /// interrotto poteva venire da un backup più vecchio — si rimpiazza.
    #[test]
    fn una_skin_che_non_si_apre_si_rimpiazza() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        std::fs::write(cartella.join("notte.ryskin"), b"PK\x03\x04 a meta").expect("rotta");

        let bytes = pacchetto("notte");
        assert_eq!(scrivi_skin(dir.path(), "notte", &bytes), Ok(true));
        assert_eq!(
            std::fs::read(cartella.join("notte.ryskin")).expect("letta"),
            bytes
        );
    }

    /// Una skin sana e diversa da quella del backup resta: può essere stata
    /// modificata a mano, e il ripristino non è il posto dove decidere chi
    /// delle due valga di più.
    #[test]
    fn una_skin_sana_e_diversa_resta() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let sorgente = rymembra_skin::PLAIN_SOURCE
            .replace("\"id\": \"plain\"", "\"id\": \"notte\"")
            .replace("\"name\": \"Plain\"", "\"name\": \"Mia\"");
        let mia = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
            source: &sorgente,
            preview: None,
            assets: &[],
        })
        .expect("pacchetto modificato");
        let cartella = dir.path().join("skin");
        std::fs::create_dir_all(&cartella).expect("cartella");
        std::fs::write(cartella.join("notte.ryskin"), &mia).expect("mia");

        assert_eq!(
            scrivi_skin(dir.path(), "notte", &pacchetto("notte")),
            Ok(false)
        );
        assert_eq!(
            std::fs::read(cartella.join("notte.ryskin")).expect("letta"),
            mia
        );
    }

    /// Una cartella di bozza senza `skin.json` è il residuo di un salvataggio
    /// interrotto — lo dice già [`elenca_bozze`], che non la elenca — e quindi
    /// il piano la chiede. «La cartella c'è» la saltava.
    #[test]
    fn una_bozza_senza_manifest_si_scrive() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let cartella = dir.path().join("skin").join("bozze").join("in-corso");
        std::fs::create_dir_all(&cartella).expect("cartella vuota");

        let sorgente = rymembra_skin::PLAIN_SOURCE.as_bytes();
        assert_eq!(scrivi_bozza(dir.path(), "in-corso", sorgente), Ok(true));
        assert_eq!(leggi_bozza(dir.path(), "in-corso").as_deref(), Ok(sorgente));
        assert_eq!(nomi(&cartella), ["skin.json"], "nessun temporaneo");
    }

    /// Un manifest tagliato a metà, o con la coda di zeri che un'interruzione
    /// di corrente lascia dopo un `write`, è il backup scritto a metà: si
    /// completa.
    #[test]
    fn una_bozza_scritta_a_meta_si_completa() {
        let sorgente = rymembra_skin::PLAIN_SOURCE.as_bytes();
        let mezzo = sorgente
            .get(..sorgente.len().div_euclid(3))
            .expect("un terzo");
        let mut a_zeri = mezzo.to_vec();
        a_zeri.resize(sorgente.len(), 0);
        for rotta in [Vec::new(), mezzo.to_vec(), a_zeri] {
            let dir = tempfile::tempdir().expect("cartella temporanea");
            let cartella = dir.path().join("skin").join("bozze").join("in-corso");
            std::fs::create_dir_all(&cartella).expect("cartella");
            std::fs::write(cartella.join(MANIFEST), &rotta).expect("rotta");

            assert_eq!(
                scrivi_bozza(dir.path(), "in-corso", sorgente),
                Ok(true),
                "{} byte rimasti",
                rotta.len()
            );
            assert_eq!(leggi_bozza(dir.path(), "in-corso").as_deref(), Ok(sorgente));
        }
    }

    /// Una bozza non valida ma **non** rotta resta: lo Studio salva anche le
    /// bozze che non passano la validazione, perché sono lavoro in corso.
    #[test]
    fn una_bozza_non_valida_ma_scritta_da_qualcuno_resta() {
        let dir = tempfile::tempdir().expect("cartella temporanea");
        let cartella = dir.path().join("skin").join("bozze").join("in-corso");
        std::fs::create_dir_all(&cartella).expect("cartella");
        std::fs::write(cartella.join(MANIFEST), b"{ \"a meta\": ").expect("lavoro in corso");

        assert_eq!(
            scrivi_bozza(
                dir.path(),
                "in-corso",
                rymembra_skin::PLAIN_SOURCE.as_bytes()
            ),
            Ok(false)
        );
        assert_eq!(
            leggi_bozza(dir.path(), "in-corso").as_deref(),
            Ok(&b"{ \"a meta\": "[..])
        );
    }
}
