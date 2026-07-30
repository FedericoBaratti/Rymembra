//! Percorsi: cosa è un brano, e quando due percorsi sono lo stesso.
//!
//! Sembra utilità da poco. È il posto dove il vecchio albero ha avuto i suoi due
//! difetti più cari, entrambi con lo stesso esito — **righe cancellate per file
//! che esistevano ancora** — ed entrambi invisibili finché non capitavano al
//! percorso giusto:
//!
//! - `startsWith` per dire «sta dentro la cartella sorvegliata»: `C:\Musica`
//!   comincia con `C:\Music`, quindi una scansione di `C:\Music` cancellava le
//!   righe della cartella accanto;
//! - confronto esatto fra il percorso salvato nel database e quello prodotto
//!   dalla camminata: bastano una barra rovesciata contro una dritta, o una
//!   maiuscola, e il file «non risulta più trovato».
//!
//! Da qui la separazione fra le due nozioni di percorso. Quello **per il disco**
//! si usa così com'è, sempre: normalizzarlo per leggerlo sarebbe un altro modo
//! di sbagliare file. Quello **per l'appartenenza a un insieme** è una forma
//! canonica che serve solo a rispondere «è lo stesso percorso?», e non deve mai
//! finire in una `open()`.

/// Le estensioni che consideriamo musica. Elenco chiuso, come nel vecchio albero.
pub const SUPPORTED_EXTENSIONS: [&str; 10] = [
    "mp3", "flac", "m4a", "aac", "ogg", "wav", "aiff", "aif", "opus", "wma",
];

/// La cartella dei doppioni scartati, dentro quella dei download.
///
/// Esclusa dalla scansione e dalla sorveglianza: senza, un file spostato lì
/// dalla deduplicazione rientrerebbe in libreria alla scansione successiva, e
/// l'utente lo vedrebbe tornare da solo dopo averlo tolto.
pub const TRASH_DIR_NAME: &str = ".trash";

/// Sotto questa soglia non è un brano.
///
/// Sono avanzi troncati — una conversione ffmpeg uccisa, un download interrotto
/// — e il punto è che i lettori di metadati spesso ci riescono lo stesso: senza
/// questa guardia entrano in libreria come tracce apparentemente normali, e il
/// guasto si scopre premendo play.
pub const MIN_TRACK_BYTES: u64 = 32 * 1024;

/// Le regole di confronto fra percorsi, dichiarate da chi chiama.
///
/// `case_insensitive` non ha un valore predefinito, ed è deliberato: sbagliarlo
/// perde dati **in entrambe le direzioni**. A `false` su un filesystem che
/// ignora le maiuscole, `C:\Music\a.mp3` nel database e `C:\music\A.MP3` dalla
/// camminata sono due cose diverse: la riga viene cancellata e il file
/// reinserito. A `true` su un filesystem che le distingue, `a.mp3` e `A.mp3`
/// sono due brani veri che collassano in uno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathRules {
    /// Il filesystem ignora la differenza fra maiuscole e minuscole.
    pub case_insensitive: bool,
}

impl PathRules {
    /// Le regole del filesystem su cui questo binario sta girando.
    #[must_use]
    pub const fn for_current_platform() -> Self {
        Self {
            case_insensitive: cfg!(any(target_os = "windows", target_os = "macos")),
        }
    }
}

/// L'ultimo segmento di un percorso, con qualunque separatore.
#[must_use]
pub fn base_name(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(cut) => path.get(cut + 1..).unwrap_or(path),
        None => path,
    }
}

/// L'estensione in minuscolo, senza punto. Stringa vuota se non ce n'è.
///
/// Si guarda solo l'ultimo segmento: una cartella chiamata `Album.2019` non deve
/// dare estensione `2019/traccia` a ciò che contiene. E un nome che comincia con
/// un punto e non ne ha altri (`.trashinfo`) non ha estensione, ha solo un nome
/// — per questo la posizione del punto deve essere **maggiore di zero**, non
/// solo trovata.
#[must_use]
pub fn extension_of(path: &str) -> String {
    let name = base_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.get(dot + 1..).unwrap_or("").to_lowercase(),
        _ => String::new(),
    }
}

/// Il nome del file senza estensione.
///
/// È il ripiego per il titolo di un brano che non ha tag: un file che in
/// libreria comparisse senza nome non si potrebbe né leggere né cercare, e il
/// nome che l'utente gli ha dato sul disco è l'unica cosa che si sa di lui.
///
/// Le stesse regole di [`extension_of`], al contrario: un punto in posizione
/// zero non separa niente, quindi `.trashinfo` ha come radice sé stesso.
#[must_use]
pub fn file_stem(path: &str) -> &str {
    let name = base_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.get(..dot).unwrap_or(name),
        _ => name,
    }
}

/// I segmenti non vuoti di un percorso, con qualunque separatore.
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split(['/', '\\']).filter(|part| !part.is_empty())
}

/// Il percorso attraversa la cartella dei doppioni scartati?
///
/// Il confronto è per segmento intero: `non.trash/` e `.trashy/` non sono il
/// cestino, e una sottostringa li scambierebbe per tale facendo sparire dalla
/// libreria dei brani legittimi.
#[must_use]
pub fn is_in_trash(path: &str) -> bool {
    segments(path).any(|part| part == TRASH_DIR_NAME)
}

/// L'estensione è fra quelle che consideriamo musica?
#[must_use]
pub fn is_supported_audio_path(path: &str) -> bool {
    let ext = extension_of(path);
    SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

/// La forma canonica per l'APPARTENENZA A UN INSIEME. Mai per leggere un file.
///
/// Unifica i separatori e, dove il filesystem non distingue le maiuscole, piega
/// il caso. Serve a far combaciare il percorso salvato nel database con quello
/// appena prodotto dalla camminata: un confronto esatto fra i due li vedrebbe
/// diversi per una barra, e la scansione cancellerebbe la riga di un file che
/// sta ancora lì.
#[must_use]
pub fn path_key(path: &str, rules: PathRules) -> String {
    // Barre unificate e code di separatori tolte: `C:\Music\` e `C:/Music` sono
    // lo stesso posto.
    let unified: String = path
        .chars()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    let trimmed = unified.trim_end_matches('/');
    if rules.case_insensitive {
        trimmed.to_lowercase()
    } else {
        trimmed.to_owned()
    }
}

/// `path` è la cartella stessa, o qualcosa strettamente dentro?
///
/// Il confine di separatore è tutto il punto: senza, `C:\Musica` risulterebbe
/// dentro `C:\Music` — sono due cartelle diverse, e nel vecchio albero questo
/// bastava a far cancellare le righe della seconda quando si scansionava la
/// prima.
///
/// ```
/// use aether_domain::paths::{is_under, PathRules};
/// let win = PathRules { case_insensitive: true };
/// assert!(!is_under(r"C:\Musica\a.mp3", r"C:\Music", win));
/// assert!(is_under(r"C:\Music\a.mp3", r"C:\Music", win));
/// ```
#[must_use]
pub fn is_under(path: &str, folder: &str, rules: PathRules) -> bool {
    let child = path_key(path, rules);
    let parent = path_key(folder, rules);
    // Una radice vuota non contiene niente. Senza questa guardia il prefisso
    // sarebbe `"/"` e mezzo filesystem risulterebbe sorvegliato.
    if parent.is_empty() {
        return false;
    }
    child == parent || child.starts_with(&format!("{parent}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };
    const NIX: PathRules = PathRules {
        case_insensitive: false,
    };

    #[test]
    fn il_difetto_storico_del_prefisso() {
        assert!(!is_under(r"C:\Musica\a.mp3", r"C:\Music", WIN));
        assert!(!is_under(r"C:\MusicX\a.mp3", r"C:\Music", WIN));
        assert!(is_under(r"C:\Music\a.mp3", r"C:\Music", WIN));
        assert!(
            is_under(r"C:\Music", r"C:\Music", WIN),
            "la cartella stessa"
        );
    }

    #[test]
    fn barre_e_maiuscole_non_fanno_sparire_un_file() {
        assert_eq!(
            path_key(r"C:\Music\a.mp3", WIN),
            path_key("C:/MUSIC/A.MP3", WIN)
        );
        // …ma dove il filesystem le distingue, distinguerle è obbligatorio.
        assert_ne!(path_key("/m/a.mp3", NIX), path_key("/m/A.mp3", NIX));
    }

    #[test]
    fn separatori_finali_e_doppi() {
        assert_eq!(path_key(r"C:\Music\", WIN), "c:/music");
        assert_eq!(path_key("C:/Music///", WIN), "c:/music");
    }

    #[test]
    fn radice_vuota_non_contiene_niente() {
        assert!(!is_under(r"C:\Music\a.mp3", "", WIN));
    }

    #[test]
    fn estensione_solo_dall_ultimo_segmento() {
        assert_eq!(extension_of(r"C:\Music\Album.2019\traccia"), "");
        assert_eq!(extension_of(r"C:\Music\a.MP3"), "mp3");
        assert_eq!(extension_of(r"C:\Music\a.mp3.txt"), "txt");
        // Un nome che comincia con un punto ha un nome, non un'estensione.
        assert_eq!(extension_of(r"C:\Music\.trashinfo"), "");
    }

    #[test]
    fn la_radice_del_nome_serve_da_titolo_di_ripiego() {
        assert_eq!(file_stem(r"C:\Music\01 - Traccia.mp3"), "01 - Traccia");
        assert_eq!(file_stem("C:/Music/senza estensione"), "senza estensione");
        // Come `extension_of`: un punto in testa non separa niente.
        assert_eq!(file_stem(r"C:\Music\.trashinfo"), ".trashinfo");
        assert_eq!(file_stem("a.mp3.txt"), "a.mp3");
    }

    #[test]
    fn il_cestino_si_riconosce_per_segmento_intero() {
        assert!(is_in_trash(r"C:\Music\.trash\a.mp3"));
        assert!(is_in_trash(r"C:\Music\.trash"));
        // Questi due sono cartelle legittime: confonderle col cestino
        // toglierebbe dalla libreria brani che ci devono stare.
        assert!(!is_in_trash(r"C:\Music\non.trash\a.mp3"));
        assert!(!is_in_trash(r"C:\Music\.trashy\a.mp3"));
    }
}
