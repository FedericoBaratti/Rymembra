//! Dove la finestra può chiedere di scrivere.
//!
//! # Il difetto che questo modulo toglie
//!
//! Quattro comandi scrivono su un percorso che arriva dalla pagina:
//! `playlist::playlist_esporta`, `studio::studio_esporta`,
//! `profilo::profilo_esporta` e `sincronia::sincronia_magazzino`. Fino alla
//! 2.4 lo prendevano così com'era. Nel giro normale non se ne accorge nessuno —
//! il percorso esce da un dialogo di sistema, e un dialogo di sistema non
//! propone `C:\Windows\qualcosa.dll` — ma il giro normale non è l'unico: la
//! pagina è la parte del programma che tocca più testo altrui (tag dei brani,
//! skin condivise, risposte dei cataloghi), e i quattro comandi erano quattro
//! modi di scrivere byte **qualunque** in un posto **qualunque** per chiunque
//! un giorno riuscisse a far girare uno script lì dentro. Il CSP e l'elenco
//! chiuso di `capabilities/default.json` sono il primo cancello; questo è il
//! secondo, ed è quello che non dipende dal primo.
//!
//! # Le regole, per un file
//!
//! 1. **Assoluto, e di un disco o di una condivisione.** `C:\…`, `\\server\…`
//!    e le loro forme `\\?\`. Non `\\.\…`, che è lo spazio dei dispositivi e
//!    delle pipe con nome, e non `\\?\GLOBALROOT\…`: un dialogo di salvataggio
//!    non li restituisce mai, quindi chi li manda il dialogo l'ha scavalcato.
//!    Un percorso relativo si risolverebbe contro la cartella di lavoro del
//!    processo, che nessuno ha scelto.
//! 2. **Niente `..` e niente `:` nei nomi.** Il primo perché un dialogo
//!    restituisce percorsi già risolti; il secondo perché su NTFS
//!    `brano.exe:note.m3u8` non è un nome strano, è il flusso `note.m3u8` di un
//!    file `brano.exe` — che viene creato, vuoto, col nome che si vuole.
//! 3. **L'estensione del formato**, senza badare alle maiuscole: `SERATA.M3U8`
//!    è una playlist come `serata.m3u8`. Qualunque altra si rifiuta con
//!    `fs.extensionRefused`.
//!
//! # Perché rifiutare, e non aggiustare
//!
//! Aggiungere `.m3u8` a un nome che non ce l'ha sembrerebbe più gentile. Ma il
//! dialogo di Windows l'estensione la aggiunge già da sé — il primo filtro
//! diventa l'estensione di serie — quindi un nome senza arriva solo da chi il
//! dialogo non l'ha usato. E scrivere su un nome diverso da quello scelto vuol
//! dire poter sovrascrivere un file che chi ha premuto «Salva» non ha mai
//! visto nel dialogo, perché il dialogo chiede conferma solo per il nome che
//! mostra.
//!
//! # Le regole, per una cartella
//!
//! La cartella della sincronia si **scrive a mano** in un campo di testo, e
//! prima diventava `create_dir_all`: un errore di battitura in
//! `D:\Sincronia\Rymembra` era un albero di cartelle nuove su un disco a caso,
//! senza che nessuno lo dicesse. Adesso le regole 1 e 2 valgono uguali, e si
//! crea **al più l'ultimo gradino**: se la cartella che la contiene esiste, la
//! si crea; se no si dice di no, con la stessa frase di sempre — «scegli una
//! cartella che esiste».

use std::path::{Component, Path, PathBuf, Prefix};

use rymembra_domain::errors::{AppError, ErrorCode};

/// Le estensioni di una playlist.
///
/// Sono esattamente quelle che `FormatoPlaylist::da_estensione` riconosce, e
/// `m3u8` per prima perché è quella con cui l'esportazione scrive: un test
/// qui sotto tiene le due liste allineate.
pub(crate) const ESTENSIONI_PLAYLIST: &[&str] = &["m3u8", "m3u", "pls", "xspf"];

/// L'estensione di un pacchetto di skin, **in scrittura**.
///
/// Solo quella di adesso: le estensioni di prima si leggono (la finestra di
/// scelta, il trascinamento) ma non si scrivono mai, e una lista di file
/// ammessi in scrittura che le contenesse farebbe nascere dati sotto il nome
/// vecchio. Vedi `rymembra_domain::eredita`.
pub(crate) const ESTENSIONI_SKIN: &[&str] = &["ryskin"];

/// L'estensione di un profilo, **in scrittura**. Il `.json` della 2.2 e il
/// profilo con l'estensione di prima si **leggono** ancora, ma non si scrivono
/// più: un profilo esportato oggi è sempre un `.ryprofile`.
pub(crate) const ESTENSIONI_PROFILO: &[&str] = &["ryprofile"];

/// Un file che la finestra può far scrivere: dove, e con quale estensione.
#[derive(Debug)]
pub(crate) struct FileAmmesso {
    /// Il percorso, così come è arrivato: niente viene riscritto.
    pub(crate) percorso: PathBuf,
    /// Quale delle estensioni ammesse ha, minuscola e senza punto.
    pub(crate) estensione: &'static str,
}

/// Il file su cui la finestra chiede di scrivere, se lo può chiedere.
///
/// Vedi il `//!` per le tre regole. Non tocca il disco: dice solo se il
/// percorso ha la forma giusta, e se c'è un guasto vero — la cartella non
/// esiste, il disco è pieno — lo dirà la scrittura, con il suo codice.
///
/// # Errori
///
/// `fs.pathInvalid` se il percorso non è assoluto, non è di un disco o di una
/// condivisione, o ha `..` o `:` nei nomi; `fs.extensionRefused` se
/// l'estensione non è fra `ammesse`.
pub(crate) fn file_da_scrivere(
    percorso: &str,
    ammesse: &'static [&'static str],
) -> Result<FileAmmesso, AppError> {
    let path = forma_ammessa(percorso)?;
    let trovata = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(|e| ammesse.iter().copied().find(|a| a.eq_ignore_ascii_case(e)));
    let Some(estensione) = trovata else {
        return Err(AppError::new(ErrorCode::FsExtensionRefused {
            path: percorso.to_owned(),
            allowed: ammesse.iter().map(|&a| a.to_owned()).collect(),
        })
        .with_cause(format!("servono .{}", ammesse.join(", ."))));
    };
    Ok(FileAmmesso {
        percorso: path,
        estensione,
    })
}

/// La cartella della sincronia, pronta: esisteva già, o è stata creata adesso.
///
/// Spazi in testa e in coda si tolgono, perché il percorso arriva da un campo
/// di testo e uno spazio incollato per sbaglio non è parte di un nome. Poi le
/// regole 1 e 2 del `//!`, e si crea **al più l'ultimo gradino**: la cartella
/// scritta, se quella che la contiene esiste.
///
/// Tocca il disco — è il suo mestiere — quindi va chiamata fuori dal filo
/// della finestra: `sincronia_magazzino` è `(async)` per questo.
///
/// # Errori
///
/// `fs.pathInvalid` per un percorso vuoto o di forma sbagliata, per un file che
/// porta già quel nome, per una cartella che la dovrebbe contenere e non
/// esiste, e se la creazione fallisce (con la causa del sistema).
pub(crate) fn cartella_da_usare(percorso: &str) -> Result<PathBuf, AppError> {
    let scritto = percorso.trim();
    let invalido = |causa: &str| {
        AppError::new(ErrorCode::FsPathInvalid {
            path: scritto.to_owned(),
        })
        .with_cause(causa.to_owned())
    };
    if scritto.is_empty() {
        return Err(invalido("nessun percorso"));
    }
    let path = forma_ammessa(scritto)?;
    if path.is_dir() {
        return Ok(path);
    }
    if path.exists() {
        return Err(invalido("c'è già un file con questo nome"));
    }
    let contenitore_c_e = path.parent().is_some_and(Path::is_dir);
    if !contenitore_c_e {
        return Err(invalido(
            "manca la cartella che la dovrebbe contenere: si crea solo l'ultimo gradino",
        ));
    }
    match std::fs::create_dir(&path) {
        Ok(()) => Ok(path),
        // Qualcun altro l'ha creata fra il controllo e adesso — un altro
        // dispositivo che scrive nella stessa cartella condivisa. Va bene lo
        // stesso, purché sia una cartella.
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(path),
        Err(err) => Err(invalido(&err.to_string())),
    }
}

/// Le regole 1 e 2 del `//!`, comuni a file e cartelle.
///
/// Visibile al resto del crate per chi deve rifare la domanda **senza toccare
/// il disco**: la cartella di sincronia salvata dalla 2.4, che si ricontrolla
/// all'avvio (`sincronia::rivaluta_la_cartella`), e la cartella dei download
/// (`procura::cartella_download_ammessa`). Una seconda copia delle regole
/// scritta là sarebbe quella che un giorno dice una cosa diversa.
///
/// # Errori
///
/// `fs.pathInvalid`, con la regola che non passa nella causa.
pub(crate) fn forma_ammessa(percorso: &str) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(percorso);
    let rifiuta = |causa: &str| {
        Err(AppError::new(ErrorCode::FsPathInvalid {
            path: percorso.to_owned(),
        })
        .with_cause(causa.to_owned()))
    };
    if !path.is_absolute() {
        return rifiuta("il percorso non è assoluto");
    }
    for parte in path.components() {
        match parte {
            Component::Prefix(prefisso) => {
                // Su Windows il prefisso c'è sempre, in un percorso assoluto;
                // altrove non c'è mai, e la regola non ha niente da guardare.
                let di_un_disco = matches!(
                    prefisso.kind(),
                    Prefix::Disk(_)
                        | Prefix::UNC(..)
                        | Prefix::VerbatimDisk(_)
                        | Prefix::VerbatimUNC(..)
                );
                if !di_un_disco {
                    return rifiuta("non è il percorso di un disco o di una condivisione");
                }
            }
            Component::ParentDir => return rifiuta("il percorso risale con «..»"),
            Component::Normal(nome) if nome.to_string_lossy().contains(':') => {
                return rifiuta("un nome contiene «:», cioè un flusso NTFS");
            }
            Component::RootDir | Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Il codice di un errore, per confrontarlo senza i parametri.
    fn codice(esito: Result<impl std::fmt::Debug, AppError>) -> &'static str {
        esito
            .expect_err("doveva essere rifiutato")
            .code()
            .kind()
            .code()
    }

    #[test]
    fn le_estensioni_delle_playlist_sono_quelle_che_si_sanno_scrivere() {
        use rymembra_domain::playlist_file::FormatoPlaylist;
        // Ogni estensione ammessa ha un formato: nessuna ricade più su M3U per
        // ripiego, che era il difetto.
        for estensione in ESTENSIONI_PLAYLIST {
            assert!(
                FormatoPlaylist::da_estensione(estensione).is_some(),
                "{estensione} ammessa ma senza formato"
            );
        }
        // E ogni formato che l'esportazione sa scrivere si può scegliere.
        for formato in [
            FormatoPlaylist::M3u,
            FormatoPlaylist::Pls,
            FormatoPlaylist::Xspf,
        ] {
            assert!(
                ESTENSIONI_PLAYLIST.contains(&formato.estensione()),
                "{} si scrive ma non si può scegliere",
                formato.estensione()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn il_percorso_di_un_dialogo_passa_con_la_sua_estensione() {
        let file = file_da_scrivere(r"C:\Musica\Serata d'estate.m3u8", ESTENSIONI_PLAYLIST)
            .expect("il caso normale");
        assert_eq!(file.estensione, "m3u8");
        assert_eq!(
            file.percorso,
            PathBuf::from(r"C:\Musica\Serata d'estate.m3u8")
        );

        // Le maiuscole non contano, e l'estensione che torna è quella ammessa.
        let file =
            file_da_scrivere(r"C:\Musica\SERATA.PLS", ESTENSIONI_PLAYLIST).expect("maiuscole");
        assert_eq!(file.estensione, "pls");
        let file = file_da_scrivere(r"D:\skin\Notte.RySkin", ESTENSIONI_SKIN).expect("miste");
        assert_eq!(file.estensione, "ryskin");

        // Un nome con dei punti dentro è ancora un nome: conta l'ultimo.
        assert!(file_da_scrivere(r"C:\p\Vol. 2.xspf", ESTENSIONI_PLAYLIST).is_ok());
        assert!(file_da_scrivere(r"C:\p\rymembra-profilo.ryprofile", ESTENSIONI_PROFILO).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn condivisioni_e_forme_verbatim_passano() {
        assert!(file_da_scrivere(r"\\nas\musica\serata.m3u", ESTENSIONI_PLAYLIST).is_ok());
        assert!(file_da_scrivere(r"\\?\C:\Musica\serata.m3u8", ESTENSIONI_PLAYLIST).is_ok());
        assert!(file_da_scrivere(r"\\?\UNC\nas\musica\serata.pls", ESTENSIONI_PLAYLIST).is_ok());
    }

    #[test]
    fn le_estensioni_di_scrittura_sono_solo_quelle_nuove() {
        use rymembra_domain::eredita;
        // Le estensioni di prima sono ammesse in lettura da altre parti, e qui
        // non devono rientrare: un salvataggio che le usasse riporterebbe sul
        // disco il nome che il trasloco ha appena tolto.
        assert!(!ESTENSIONI_SKIN.contains(&eredita::ESTENSIONE_SKIN));
        assert!(!ESTENSIONI_PROFILO.contains(&eredita::ESTENSIONE_PROFILO));
    }

    #[cfg(windows)]
    #[test]
    fn un_estensione_che_non_e_del_formato_si_rifiuta() {
        let rifiutato = "fs.extensionRefused";
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\serata.txt", ESTENSIONI_PLAYLIST)),
            rifiutato
        );
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\serata", ESTENSIONI_PLAYLIST)),
            rifiutato
        );
        assert_eq!(
            codice(file_da_scrivere(
                r"C:\p\serata.m3u8.exe",
                ESTENSIONI_PLAYLIST
            )),
            rifiutato
        );
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\serata.m3u8 ", ESTENSIONI_PLAYLIST)),
            rifiutato
        );
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\serata.m3u8.", ESTENSIONI_PLAYLIST)),
            rifiutato
        );
        // Un file che si chiama solo `.ryskin` non ha estensione: è un nome.
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\.ryskin", ESTENSIONI_SKIN)),
            rifiutato
        );
        // Le liste non si mescolano.
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\notte.ryskin", ESTENSIONI_PROFILO)),
            rifiutato
        );
        // E il nome di prima non si scrive: si legge, ma non rinasce sul disco.
        let skin_di_prima = format!(r"C:\p\notte.{}", rymembra_domain::eredita::ESTENSIONE_SKIN);
        assert_eq!(
            codice(file_da_scrivere(&skin_di_prima, ESTENSIONI_SKIN)),
            rifiutato
        );
        let profilo_di_prima = format!(
            r"C:\p\profilo.{}",
            rymembra_domain::eredita::ESTENSIONE_PROFILO
        );
        assert_eq!(
            codice(file_da_scrivere(&profilo_di_prima, ESTENSIONI_PROFILO)),
            rifiutato
        );
        assert_eq!(
            codice(file_da_scrivere(r"C:\p\serata.m3u8", ESTENSIONI_SKIN)),
            rifiutato
        );
        assert_eq!(
            codice(file_da_scrivere(
                r"C:\Windows\System32\x.dll",
                ESTENSIONI_SKIN
            )),
            rifiutato
        );
    }

    #[cfg(windows)]
    #[test]
    fn i_percorsi_che_un_dialogo_non_restituisce_si_rifiutano() {
        let invalido = "fs.pathInvalid";
        for percorso in [
            // Relativi: alla cartella di lavoro, a quella del disco, alla radice.
            "serata.m3u8",
            r"C:serata.m3u8",
            r"\serata.m3u8",
            r"..\serata.m3u8",
            // Dispositivi e pipe.
            r"\\.\pipe\rymembra.m3u8",
            r"\\.\COM1.m3u8",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\serata.m3u8",
            // Risalite e flussi NTFS.
            r"C:\Musica\..\Windows\serata.m3u8",
            r"C:\Musica\brano.exe:note.m3u8",
            "",
        ] {
            assert_eq!(
                codice(file_da_scrivere(percorso, ESTENSIONI_PLAYLIST)),
                invalido,
                "{percorso:?} doveva essere rifiutato"
            );
        }
    }

    #[test]
    fn la_cartella_che_esiste_si_usa_com_e() {
        let radice = tempfile::tempdir().expect("cartella temporanea");
        let usata = cartella_da_usare(&radice.path().display().to_string()).expect("esiste");
        assert_eq!(usata, radice.path());
    }

    #[test]
    fn si_crea_solo_l_ultimo_gradino() {
        let radice = tempfile::tempdir().expect("cartella temporanea");
        let nuova = radice.path().join("Rymembra");
        // Con uno spazio in coda, come esce da un campo di testo incollato.
        let scritto = format!("{} ", nuova.display());
        assert_eq!(
            cartella_da_usare(&scritto).expect("il contenitore c'è"),
            nuova
        );
        assert!(nuova.is_dir(), "l'ultimo gradino dev'essere stato creato");

        let profonda = radice.path().join("non").join("ci").join("sono");
        assert_eq!(
            codice(cartella_da_usare(&profonda.display().to_string())),
            "fs.pathInvalid"
        );
        assert!(
            !radice.path().join("non").exists(),
            "niente alberi creati in silenzio"
        );
    }

    #[test]
    fn un_file_con_quel_nome_non_e_una_cartella() {
        let radice = tempfile::tempdir().expect("cartella temporanea");
        let file = radice.path().join("occupato");
        std::fs::write(&file, b"x").expect("scritto");
        assert_eq!(
            codice(cartella_da_usare(&file.display().to_string())),
            "fs.pathInvalid"
        );
    }

    #[test]
    fn una_cartella_vuota_o_relativa_si_rifiuta() {
        assert_eq!(codice(cartella_da_usare("   ")), "fs.pathInvalid");
        assert_eq!(codice(cartella_da_usare("sincronia")), "fs.pathInvalid");
        assert!(
            !Path::new("sincronia").exists(),
            "un percorso relativo non deve creare niente nella cartella di lavoro"
        );
    }
}
