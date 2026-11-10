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

/// Le estensioni che consideriamo musica. Elenco chiuso.
///
/// # Perché non è più «come nel vecchio albero»
///
/// Perché due voci in più ci sono: `oga` e `ape`. La riga diceva «come nel
/// vecchio albero» e quella parità è anche il senso di
/// `tests/golden/scan.json`, che è generato eseguendo il `paths.ts` legacy —
/// quindi queste divergenze vanno dichiarate qui, o il prossimo che legge i due
/// file insieme trova una contraddizione e non sa da che parte stia il guasto.
///
/// **`oga` è voluta, e costa zero.** `.oga` è l'estensione che Xiph raccomanda
/// per l'audio dentro Ogg: stesso contenitore di `.ogg`, stesso demultiplatore,
/// e dentro ci può stare Vorbis, FLAC o Opus indifferentemente — tutti e tre già
/// decodificabili. Escluderlo non proteggeva da niente: lasciava fuori dalla
/// libreria dei file che l'app sa aprire, scrivere e suonare.
///
/// **`ape` viene dalle immagini CD** (vedi [`crate::cue`]). Monkey's Audio è il
/// secondo formato dei rip fatti con EAC: sul NAS misurato, 143 immagini su
/// 300 sono `.ape`, accanto al loro `.cue`. Senza questa voce quelle
/// immagini non si potevano abbinare al foglio, e i loro dischi restavano fuori
/// dalla libreria per intero. Il decodificatore sta in `rymembra-play`.
///
/// Nessuno dei casi del golden usa `.oga` né `.ape`, quindi la parità che quel
/// file misura resta verde: quel che è cambiato è l'elenco, non il modo di
/// leggerlo.
pub const SUPPORTED_EXTENSIONS: [&str; 12] = [
    "mp3", "flac", "m4a", "aac", "ogg", "oga", "wav", "aiff", "aif", "opus", "wma", "ape",
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
///
/// E il file non è uno che Android tiene da parte: vedi
/// [`tenuto_da_parte_da_android`].
#[must_use]
pub fn is_supported_audio_path(path: &str) -> bool {
    if tenuto_da_parte_da_android(base_name(path)) {
        return false;
    }
    let ext = extension_of(path);
    SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

/// Un nome che `MediaStore` dà a un file che tiene da parte, nella sua stessa
/// cartella: `.trashed-<scadenza>-<nome>` è nel Cestino di Android,
/// `.pending-<scadenza>-<nome>` è ancora in scrittura.
///
/// # Perché conta
///
/// Perché il Cestino di Android non è una cartella, come quello di Windows
/// (`$RECYCLE.BIN`, che la camminata salta): è un nome nuovo accanto a quello
/// di prima. Un brano eliminato da Rymembra sul telefono spariva dalla libreria
/// e un istante dopo la sorveglianza delle cartelle vedeva comparire
/// `.trashed-1796321480-01 Ghiaccio I.mp3` — un MP3 nuovo — e lo rimetteva in
/// elenco, con lo stesso titolo. Vale anche sul computer: una cartella del
/// telefono sincronizzata sul PC si porta dietro i suoi file nel Cestino.
#[must_use]
pub fn tenuto_da_parte_da_android(nome: &str) -> bool {
    nome.starts_with(".trashed-") || nome.starts_with(".pending-")
}

/// La forma canonica per l'APPARTENENZA A UN INSIEME. Mai per leggere un file.
///
/// Unifica i separatori e, dove il filesystem non distingue le maiuscole, piega
/// il caso. Serve a far combaciare il percorso salvato nel database con quello
/// appena prodotto dalla camminata: un confronto esatto fra i due li vedrebbe
/// diversi per una barra, e la scansione cancellerebbe la riga di un file che
/// sta ancora lì.
///
/// # Quel che questa chiave NON riconcilia
///
/// `Z:\musica\a.mp3` e `\\server\share\musica\a.mp3` possono essere lo stesso
/// file — la lettera è una condivisione mappata — e qui restano due chiavi
/// diverse. Non è una dimenticanza: risalire dalla lettera alla condivisione
/// vuol dire chiedere a Windows (le API `WNet*`), cioè `unsafe` o un crate in
/// più, oppure interpretare l'uscita di `net use`, che è testo tradotto nella
/// lingua del sistema. Il prezzo è una libreria che, se la stessa cartella viene
/// aggiunta due volte nelle due grafie, tiene due righe per lo stesso brano.
///
/// Il prezzo era molto più alto finché la scansione poteva **cancellare** le
/// righe dell'altra grafia: adesso non può — una radice che non risponde non
/// giudica nessuno (`rymembra_app::library::plan`) — e quel che resta è una
/// duplicazione visibile, non una perdita silenziosa.
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
/// use rymembra_domain::paths::{is_under, PathRules};
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

/// I nomi che Windows riserva ai dispositivi, fuori dalle porte numerate.
///
/// `CONIN$` e `CONOUT$` sono la console, come `CON`: il `$` non sta in nessuno
/// degli alfabeti che questo crate fa passare, ma la regola è sul nome, e un
/// nome che un giorno lo ammettesse non deve riaprire il buco.
const DISPOSITIVI: [&str; 6] = ["con", "prn", "aux", "nul", "conin$", "conout$"];

/// Il nome è un dispositivo di Windows, e non un file: `CON`, `NUL`, `COM1`…
///
/// Conta la **radice**, quel che sta prima del primo punto: `nul.tar.gz` è
/// `NUL` quanto `nul`, e lo stesso vale per una cartella. Senza badare alle
/// maiuscole, e senza gli spazi in coda che Windows toglie da sé. Le porte
/// sono `COM` e `LPT` con una cifra, zero compreso, o con gli apici ¹ ² ³, che
/// la documentazione di Microsoft elenca a parte.
///
/// # Perché qui, e una volta sola
///
/// Su Windows 10 — che Rymembra promette dalla 1809 in su — un file chiamato
/// così è il dispositivo: la scrittura fallisce con un errore di I/O che non
/// dice niente, o finisce nel nulla. Su Windows 11 i nomi dentro un percorso
/// sono file normali, e il guasto non si vede da chi sviluppa. È una regola
/// del nome di file, e i posti che costruiscono un nome dal mondo — un id di
/// skin, una risorsa di un pacchetto, una voce di un profilo, un titolo preso
/// da un catalogo — devono dire la stessa cosa: un elenco per posto sarebbe un
/// elenco che si aggiorna in un posto solo.
#[must_use]
pub fn riservato_da_windows(nome: &str) -> bool {
    let radice = nome
        .split('.')
        .next()
        .unwrap_or(nome)
        .trim_end_matches(' ')
        .to_ascii_lowercase();
    if DISPOSITIVI.contains(&radice.as_str()) {
        return true;
    }
    ["com", "lpt"].iter().any(|porta| {
        let mut resto = radice.strip_prefix(porta).unwrap_or_default().chars();
        matches!(
            (resto.next(), resto.next()),
            (Some('0'..='9' | '¹' | '²' | '³'), None)
        )
    })
}

/// Un identificativo che diventa un nome di file o di cartella: lettere e
/// cifre ASCII, trattini e trattini bassi, da 1 a 64, e non un dispositivo di
/// Windows ([`riservato_da_windows`]).
///
/// `..`, le barre e i due punti di un percorso di Windows sono tutto ciò che
/// serve a scrivere altrove, e un dispositivo è un nome che non scrive affatto.
/// È la regola delle skin installate, delle bozze dello Studio e delle voci di
/// un profilo, che fino alla 3.2 ne avevano una copia ciascuna con la nota
/// «volutamente identica»: quella nota si mantiene da sé solo se la regola sta
/// in un posto.
#[must_use]
pub fn id_di_file(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !riservato_da_windows(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_dispositivi_di_windows_si_riconoscono_con_qualunque_estensione() {
        for nome in [
            "con",
            "CON",
            "Con.png",
            "nul.tar.gz",
            "aux",
            "prn.txt",
            "com1",
            "COM9.woff2",
            "com0",
            "lpt0",
            "LPT9",
            "com¹",
            "lpt³.png",
            "conin$",
            "CONOUT$",
            "nul ",
            "con .txt",
        ] {
            assert!(riservato_da_windows(nome), "«{nome}»");
        }
        for nome in [
            "console",
            "nulla",
            "com10",
            "lpt",
            "com",
            "auxilio",
            "connie.png",
            "a.con",
            "com-1",
            "",
            ".con",
        ] {
            assert!(!riservato_da_windows(nome), "«{nome}»");
        }
    }

    #[test]
    fn un_id_di_file_non_esce_dalla_cartella_e_non_e_un_dispositivo() {
        for cattivo in ["../fuori", "a/b", "C:\\altrove", "", "con", "NUL", "lpt1"] {
            assert!(!id_di_file(cattivo), "«{cattivo}»");
        }
        assert!(!id_di_file(&"a".repeat(65)));
        for buono in ["notte-2_v3", "Mia", "console"] {
            assert!(id_di_file(buono), "«{buono}»");
        }
    }

    const WIN: PathRules = PathRules {
        case_insensitive: true,
    };
    const NIX: PathRules = PathRules {
        case_insensitive: false,
    };

    #[test]
    fn i_file_nel_cestino_di_android_non_sono_musica() {
        // Il brano appena eliminato dal telefono: `MediaStore` lo rinomina
        // nella sua cartella, e la sorveglianza lo vede comparire.
        assert!(!is_supported_audio_path(
            "/storage/emulated/0/Music/Aurora Boreale/Ghiaccio/.trashed-1796321480-01 Ghiaccio I.mp3"
        ));
        assert!(!is_supported_audio_path(
            "/storage/emulated/0/Download/.pending-1796321480-nuovo.flac"
        ));
        // Un nome che comincia con un punto, e basta, resta musica: non è una
        // regola sui file nascosti.
        assert!(is_supported_audio_path(
            "/storage/emulated/0/Music/.intro.mp3"
        ));
        assert!(is_supported_audio_path(
            "/storage/emulated/0/Music/trashed-01.mp3"
        ));
    }

    #[test]
    fn oga_e_musica_come_ogg() {
        // Il caso sta qui e non nel golden: quel file è il responso registrato
        // del vecchio albero TypeScript, e aggiungerci a mano una riga vorrebbe
        // dire inventare una risposta che quel ramo non ha mai dato.
        assert!(is_supported_audio_path(r"C:\Music\a.oga"));
        assert!(is_supported_audio_path(r"C:\Music\A.OGA"));
        assert!(is_supported_audio_path(r"C:\Music\a.opus"));
        assert!(!is_supported_audio_path(r"C:\Music\a.ogg.txt"));
    }

    #[test]
    fn ape_e_musica_e_il_cue_no() {
        // Il `.cue` non è un brano: lo diventano le voci virtuali che se ne
        // ricavano, e quelle le riconosce il piano, non l'estensione.
        assert!(is_supported_audio_path(r"\\nas\Cd\Album.ape"));
        assert!(is_supported_audio_path(r"\\nas\Cd\ALBUM.APE"));
        assert!(!is_supported_audio_path(r"\\nas\Cd\Album.cue"));
        assert!(!is_supported_audio_path(r"\\nas\Cd\Album.cue|03"));
        assert!(!is_supported_audio_path(r"\\nas\Cd\Album.wv"));
    }

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
    fn i_percorsi_unc_hanno_una_sola_chiave() {
        // Una libreria su una condivisione si nomina in due modi — con le barre
        // rovesce come le scrive Esplora risorse, o dritte come le scrive un
        // M3U — e le due grafie devono restare lo stesso posto. Se non lo
        // fossero, la scansione vedrebbe sparire ogni brano che il database ha
        // salvato nell'altra forma.
        assert_eq!(
            path_key(r"\\srv\musica\a.mp3", WIN),
            path_key("//SRV/Musica/A.MP3", WIN)
        );
        // E la radice di rete contiene i suoi brani, con lo stesso confine di
        // separatore di tutte le altre: `\\srv\musicali` non sta dentro
        // `\\srv\musica`.
        assert!(is_under(r"\\srv\musica\a.mp3", "//srv/musica", WIN));
        assert!(is_under("//srv/musica/a.mp3", r"\\srv\musica", WIN));
        assert!(!is_under(r"\\srv\musicali\a.mp3", r"\\srv\musica", WIN));
        // Due server diversi non si contengono a vicenda, nemmeno quando il nome
        // dell'uno comincia come quello dell'altro.
        assert!(!is_under(r"\\srv2\musica\a.mp3", r"\\srv\musica", WIN));
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
