//! I percorsi dei dati, come li vede chi sta fuori dal pacchetto MSIX.
//!
//! # Il problema (R20-4, collaudo in Windows Sandbox)
//!
//! Nella copia del Microsoft Store Windows manda nella cartella privata del
//! pacchetto — `%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalCache` — i file
//! che l'app **crea** in `%APPDATA%` (`LocalCache\Roaming\…`) e in
//! `%LOCALAPPDATA%` (`LocalCache\Local\…`). Dal di dentro il percorso resta
//! quello di sempre, e la libreria della copia del Store sta in una cartella,
//! `io.github.federicobaratti.rymembra.store`, che fuori dal pacchetto non esiste
//! (`cartella_di_serie` in `lib.rs`). Ma Esplora risorse gira fuori:
//! `tauri_plugin_opener::open_path` sul percorso visto da dentro dava «Percorso
//! non disponibile», proprio nel gesto che serve quando il database non si apre.
//!
//! # La regola
//!
//! Nella copia del Store, un percorso sotto `%APPDATA%` diventa
//! `<LocalCache>\Roaming\<resto>`, uno sotto `%LOCALAPPDATA%` diventa
//! `<LocalCache>\Local\<resto>` — **solo se** quello tradotto esiste. Da dentro
//! il pacchetto la cartella privata è leggibile, quindi lo si sa; se non c'è, il
//! percorso resta com'è: un file che esisteva prima del pacchetto è dove era, e
//! una cartella scelta dall'utente fuori da quelle due non è nostra. Fuori dal
//! Store, e su Android, non cambia niente.
//!
//! # Da dove viene `<LocalCache>`
//!
//! Dal sistema, non da una costante: il `PackageFamilyName` che il Partner
//! Center darà a questa app sarà diverso da quello delle prove, e dal nome
//! dell'eseguibile non si ricava per un pacchetto registrato da una cartella.
//! Lo chiede `rymembra_barra::cartella_cache_del_pacchetto` (API WinRT sicura,
//! nessuna dipendenza nuova, nessun `unsafe`). Si guarda prima, a buon mercato,
//! se la copia è quella del Store (il marcatore accanto all'eseguibile): la
//! copia installata dal sito non tocca mai WinRT.
//!
//! Qui sta la regola, pura, con le tre cartelle passate da fuori: si prova con
//! cartelle temporanee, senza `%APPDATA%` vero e senza un pacchetto.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Le tre cartelle fra cui la regola traduce.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cartelle<'a> {
    /// `%APPDATA%`, come la vede l'app (`…\AppData\Roaming`).
    pub roaming: &'a Path,
    /// `%LOCALAPPDATA%`, come la vede l'app (`…\AppData\Local`).
    pub locale: &'a Path,
    /// `LocalCache` del pacchetto: dove finiscono i file creati nelle due di
    /// sopra, in `Roaming` e in `Local`.
    pub cache: &'a Path,
}

/// Il resto di `percorso` dopo `base`, o `None` se non ci sta sotto.
///
/// Per componenti, e senza badare alle maiuscole: Windows non ci bada, e la
/// stessa cartella arriva scritta `C:\Users\Mario` da un posto e `c:\users\mario`
/// da un altro. Per componenti e non per testo: `Roaming2` non sta sotto
/// `Roaming`.
fn resto_sotto(percorso: &Path, base: &Path) -> Option<PathBuf> {
    let minuscolo =
        |pezzo: std::path::Component<'_>| pezzo.as_os_str().to_string_lossy().to_lowercase();
    let mut dentro = percorso.components();
    for atteso in base.components() {
        let trovato = dentro.next()?;
        if minuscolo(trovato) != minuscolo(atteso) {
            return None;
        }
    }
    Some(dentro.as_path().to_path_buf())
}

/// La regola, con tutto passato da fuori: `esiste` dice se il percorso tradotto
/// c'è.
fn traduci(
    percorso: &Path,
    dallo_store: bool,
    cartelle: Option<Cartelle<'_>>,
    esiste: impl Fn(&Path) -> bool,
) -> PathBuf {
    let Some(cartelle) = cartelle.filter(|_| dallo_store) else {
        return percorso.to_path_buf();
    };
    for (base, sotto) in [(cartelle.roaming, "Roaming"), (cartelle.locale, "Local")] {
        let Some(resto) = resto_sotto(percorso, base) else {
            continue;
        };
        let mut tradotto = cartelle.cache.join(sotto);
        // Un `join` di un resto vuoto aggiunge un separatore in fondo.
        if !resto.as_os_str().is_empty() {
            tradotto.push(resto);
        }
        return if esiste(&tradotto) {
            tradotto
        } else {
            percorso.to_path_buf()
        };
    }
    percorso.to_path_buf()
}

/// [`traduci`] per un percorso che esiste davvero: `contiene`, se c'è, è un
/// file che la cartella tradotta deve avere.
fn traduci_su_disco(
    percorso: &Path,
    dallo_store: bool,
    cartelle: Option<Cartelle<'_>>,
    contiene: Option<&str>,
) -> PathBuf {
    traduci(percorso, dallo_store, cartelle, |tradotto| match contiene {
        Some(file) => tradotto.join(file).is_file(),
        None => tradotto.exists(),
    })
}

/// La `LocalCache` del pacchetto, chiesta al sistema una volta sola.
fn cache_del_pacchetto() -> Option<&'static Path> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(rymembra_barra::cartella_cache_del_pacchetto)
        .as_deref()
}

/// Come [`per_esplora`], ma solo se la cartella tradotta contiene `file`.
///
/// Per un percorso che si **mostra** invece di aprirlo: la cartella di una
/// libreria. Una cartella privata che ha solo il giornale di un database
/// — la copia del Store di prima dell'aggiornamento lasciava lì `-wal` e `-shm`
/// accanto a un database che stava fuori — non è dov'è la libreria, e dirlo
/// manderebbe a cercarla nel posto sbagliato.
pub(crate) fn per_esplora_se_contiene(percorso: &Path, file: &str) -> PathBuf {
    visto_da_fuori(percorso, Some(file))
}

/// Il percorso come Esplora risorse lo può aprire: quello che il processo vede
/// da dentro il pacchetto, tradotto nella cartella privata se è lì che sta.
///
/// Fuori dal Store torna `percorso` com'è, senza chiedere niente a WinRT.
pub(crate) fn per_esplora(percorso: &Path) -> PathBuf {
    visto_da_fuori(percorso, None)
}

fn visto_da_fuori(percorso: &Path, contiene: Option<&str>) -> PathBuf {
    // Prima la domanda a buon mercato: la copia del sito e le prove non toccano
    // mai WinRT.
    if !crate::dallo_store() {
        return percorso.to_path_buf();
    }
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    let locale = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let cartelle = match (roaming.as_deref(), locale.as_deref(), cache_del_pacchetto()) {
        (Some(roaming), Some(locale), Some(cache)) => Some(Cartelle {
            roaming,
            locale,
            cache,
        }),
        _ => None,
    };
    traduci_su_disco(percorso, true, cartelle, contiene)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Le tre cartelle di una prova, in una cartella temporanea: i dati veri
    /// dell'utente non si toccano.
    struct Banco {
        _radice: tempfile::TempDir,
        roaming: PathBuf,
        locale: PathBuf,
        cache: PathBuf,
    }

    impl Banco {
        fn nuovo() -> Self {
            let radice = tempfile::tempdir().expect("cartella temporanea");
            let roaming = radice.path().join("AppData").join("Roaming");
            let locale = radice.path().join("AppData").join("Local");
            let cache = radice
                .path()
                .join("Packages")
                .join("Esempio.App_0123456789abc")
                .join("LocalCache");
            for cartella in [&roaming, &locale, &cache] {
                std::fs::create_dir_all(cartella).expect("la cartella");
            }
            Self {
                _radice: radice,
                roaming,
                locale,
                cache,
            }
        }

        fn cartelle(&self) -> Cartelle<'_> {
            Cartelle {
                roaming: &self.roaming,
                locale: &self.locale,
                cache: &self.cache,
            }
        }

        /// Crea `resto` dentro la cartella privata, sotto `Roaming` o `Local`.
        fn nella_cache(&self, sotto: &str, resto: &str) -> PathBuf {
            let cartella = self.cache.join(sotto).join(resto);
            std::fs::create_dir_all(&cartella).expect("la cartella privata");
            cartella
        }
    }

    const LIBRERIA: &str = "io.github.federicobaratti.rymembra.store";

    #[test]
    fn sotto_appdata_diventa_la_cartella_roaming_del_pacchetto() {
        let banco = Banco::nuovo();
        let dentro = banco.roaming.join(LIBRERIA);
        let fuori = banco.nella_cache("Roaming", LIBRERIA);
        assert_eq!(
            traduci_su_disco(&dentro, true, Some(banco.cartelle()), None),
            fuori
        );
        // E una cartella più giù: il diario.
        let diario = banco.nella_cache("Roaming", &format!("{LIBRERIA}/diario"));
        assert_eq!(
            traduci_su_disco(&dentro.join("diario"), true, Some(banco.cartelle()), None),
            diario
        );
    }

    #[test]
    fn sotto_localappdata_diventa_la_cartella_local_del_pacchetto() {
        let banco = Banco::nuovo();
        let dentro = banco.locale.join("io.github.federicobaratti.rymembra");
        let fuori = banco.nella_cache("Local", "io.github.federicobaratti.rymembra");
        assert_eq!(
            traduci_su_disco(&dentro, true, Some(banco.cartelle()), None),
            fuori
        );
        // Il file del diario d'avvio, che sta in questa cartella.
        std::fs::write(fuori.join("avvio.log"), "riga").expect("il file");
        assert_eq!(
            traduci_su_disco(
                &dentro.join("avvio.log"),
                true,
                Some(banco.cartelle()),
                None
            ),
            fuori.join("avvio.log")
        );
    }

    #[test]
    fn se_quello_tradotto_non_esiste_il_percorso_resta() {
        // Un file che esisteva prima del pacchetto sta dov'era: la cartella
        // privata non ce l'ha.
        let banco = Banco::nuovo();
        let dentro = banco.roaming.join("io.github.federicobaratti.rymembra");
        std::fs::create_dir_all(&dentro).expect("la cartella vera");
        assert_eq!(
            traduci_su_disco(&dentro, true, Some(banco.cartelle()), None),
            dentro
        );
        // Nemmeno la roaming dei dati, se la privata ha solo la local.
        let locale = banco.locale.join(LIBRERIA);
        banco.nella_cache("Roaming", "altro");
        assert_eq!(
            traduci_su_disco(&locale, true, Some(banco.cartelle()), None),
            locale
        );
    }

    #[test]
    fn fuori_da_appdata_e_localappdata_il_percorso_resta() {
        // Una cartella scelta dall'utente, anche se ha lo stesso nome di una
        // che il pacchetto ha creato.
        let banco = Banco::nuovo();
        banco.nella_cache("Roaming", "Musica");
        let musica = banco.cache.parent().expect("padre").join("Musica");
        assert_eq!(
            traduci_su_disco(&musica, true, Some(banco.cartelle()), None),
            musica
        );
        // Un nome che comincia come `Roaming` ma è un'altra cartella.
        let fratello = banco.roaming.with_file_name("Roaming2").join("x");
        banco.nella_cache("Roaming", "x");
        assert_eq!(
            traduci_su_disco(&fratello, true, Some(banco.cartelle()), None),
            fratello
        );
    }

    #[test]
    fn fuori_dal_store_il_percorso_resta_anche_se_quello_tradotto_esiste() {
        let banco = Banco::nuovo();
        let dentro = banco.roaming.join(LIBRERIA);
        banco.nella_cache("Roaming", LIBRERIA);
        assert_eq!(
            traduci_su_disco(&dentro, false, Some(banco.cartelle()), None),
            dentro
        );
    }

    #[test]
    fn senza_la_cartella_del_pacchetto_il_percorso_resta() {
        // WinRT ha risposto con un errore, o un Windows senza `LocalCacheFolder`.
        let banco = Banco::nuovo();
        let dentro = banco.roaming.join(LIBRERIA);
        banco.nella_cache("Roaming", LIBRERIA);
        assert_eq!(traduci_su_disco(&dentro, true, None, None), dentro);
    }

    #[test]
    fn le_maiuscole_non_contano_come_per_windows() {
        let banco = Banco::nuovo();
        let fuori = banco.nella_cache("Roaming", LIBRERIA);
        // La cartella base scritta in maiuscolo, il resto com'è: la parte che
        // Windows non distingue è quella.
        let scritto_a_modo_suo =
            PathBuf::from(banco.roaming.to_string_lossy().to_uppercase()).join(LIBRERIA);
        assert_eq!(
            traduci_su_disco(&scritto_a_modo_suo, true, Some(banco.cartelle()), None),
            fuori
        );
    }

    #[test]
    fn una_libreria_si_traduce_solo_se_il_database_e_li() {
        // La copia del Store di prima dell'aggiornamento lasciava il giornale
        // nella cartella privata accanto a un database che stava fuori: la
        // cartella privata esiste, ma la libreria non è lì.
        let banco = Banco::nuovo();
        let dentro = banco.roaming.join("io.github.federicobaratti.rymembra");
        let fuori = banco.nella_cache("Roaming", "io.github.federicobaratti.rymembra");
        std::fs::write(fuori.join("rymembra.db-wal"), "giornale").expect("il giornale");
        assert_eq!(
            traduci_su_disco(&dentro, true, Some(banco.cartelle()), Some("rymembra.db")),
            dentro,
            "con solo il giornale la libreria non sta nella cartella privata"
        );
        std::fs::write(fuori.join("rymembra.db"), "database").expect("il database");
        assert_eq!(
            traduci_su_disco(&dentro, true, Some(banco.cartelle()), Some("rymembra.db")),
            fuori
        );
    }

    /// Dove il percorso dei dati esce dal processo, passa di qui. Sono tre
    /// posti e non ci vuole un'`AppHandle` per vederlo: ogni gesto che apre
    /// la cartella dei dati o del diario, e il dialogo dell'avvio fallito, la
    /// traducono prima di consegnarla a Esplora risorse o all'utente.
    #[test]
    fn i_posti_che_aprono_o_mostrano_i_dati_passano_dalla_traduzione() {
        let manutenzione = include_str!("manutenzione.rs");
        let apri = manutenzione
            .find("pub fn manutenzione_apri_cartella_dati")
            .expect("il comando");
        let corpo = manutenzione.get(apri..).expect("il resto");
        let corpo = corpo
            .get(
                ..corpo
                    .find("/// La copia di prima dell'aggiornamento")
                    .unwrap_or(corpo.len()),
            )
            .expect("il corpo");
        assert!(
            corpo.contains("crate::pacchetto::per_esplora(&dove)"),
            "«Apri la cartella dei dati» apre il percorso visto da dentro il pacchetto"
        );
        assert!(
            manutenzione.contains("crate::pacchetto::per_esplora_se_contiene("),
            "la libreria proposta si mostra col posto in cui sta davvero"
        );

        let diario = include_str!("diario.rs");
        let apri = diario.find("pub fn diario_apri").expect("il comando");
        let corpo = diario.get(apri..).expect("il resto");
        let corpo = corpo
            .get(
                ..corpo
                    .find("fn condividi_sul_telefono")
                    .unwrap_or(corpo.len()),
            )
            .expect("il corpo");
        assert!(
            corpo.contains("crate::pacchetto::per_esplora(&dove)"),
            "«Apri la cartella del diario» apre il percorso visto da dentro il pacchetto"
        );
        let avvio = diario
            .find("pub fn testo_dell_avvio_fallito")
            .expect("il testo");
        let corpo = diario.get(avvio..).expect("il resto");
        let corpo = corpo
            .get(
                ..corpo
                    .find("pub fn mostra_l_avvio_fallito")
                    .unwrap_or(corpo.len()),
            )
            .expect("il corpo");
        assert!(
            corpo.contains("crate::pacchetto::per_esplora("),
            "il dialogo dell'avvio fallito dice dove sta la riga, non dove la vede il pacchetto"
        );
    }
}
