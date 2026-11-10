//! Le release di GitHub, per gli aggiornamenti del telefono.
//!
//! Le regole senza la rete e senza Android — quale progetto, quale versione è
//! più nuova, quale APK è di questo telefono — staccate da
//! `mobile/aggiornamenti.rs` perché si provino anche sul computer, con
//! `cargo test`. Là c'è il filo, la rete e l'installatore.

use serde::Deserialize;

/// Una release di GitHub, per quel che serve qui.
#[derive(Debug, Deserialize)]
pub struct Rilascio {
    /// L'etichetta: `v3.3.0`.
    pub tag_name: String,
    /// Le note.
    #[serde(default)]
    pub body: Option<String>,
    /// Quando è uscita, in ISO 8601.
    #[serde(default)]
    pub published_at: Option<String>,
    /// Una bozza: non si annuncia.
    #[serde(default)]
    pub draft: bool,
    /// Una versione di prova: non si annuncia.
    #[serde(default)]
    pub prerelease: bool,
    /// I file allegati.
    #[serde(default)]
    pub assets: Vec<Allegato>,
}

/// Un file allegato a una release.
#[derive(Debug, Deserialize)]
pub struct Allegato {
    /// Il nome del file.
    pub name: String,
    /// Da dove si scarica.
    pub browser_download_url: String,
    /// Quanto pesa.
    #[serde(default)]
    pub size: Option<u64>,
}

/// Una versione nuova, con il suo APK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trovato {
    /// Il numero di versione, senza la `v`.
    pub versione: String,
    /// Le note di rilascio.
    pub note: Option<String>,
    /// Quando è uscita.
    pub data_ms: Option<i64>,
    /// Dove si scarica l'APK.
    pub indirizzo: String,
    /// Il nome del file.
    pub nome: String,
    /// Quanto pesa, se GitHub lo dice.
    pub peso: Option<u64>,
}

/// `proprietario/progetto` da un indirizzo di GitHub
/// (`https://github.com/<proprietario>/<progetto>/…`), quello dell'updater del
/// computer in `tauri.conf.json`.
#[must_use]
pub fn progetto_da(indirizzo: &str) -> Option<String> {
    let resto = indirizzo.strip_prefix("https://github.com/")?;
    let mut parti = resto.split('/');
    let proprietario = parti.next().filter(|parte| !parte.is_empty())?;
    let nome = parti.next().filter(|parte| !parte.is_empty())?;
    Some(format!("{proprietario}/{nome}"))
}

/// La versione nuova di una release, se lo è e se ha un APK per
/// `architettura` (`std::env::consts::ARCH`) che si scarica da GitHub.
#[must_use]
pub fn scegli(rilascio: Rilascio, corrente: &str, architettura: &str) -> Option<Trovato> {
    if rilascio.draft || rilascio.prerelease {
        return None;
    }
    let versione = rilascio
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_owned();
    if !piu_nuova(&versione, corrente) {
        return None;
    }
    let allegato = apk_per(&rilascio.assets, architettura)?;
    if !allegato_di_github(&allegato.browser_download_url) {
        return None;
    }
    Some(Trovato {
        note: rilascio
            .body
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty())
            .map(ToOwned::to_owned),
        data_ms: rilascio
            .published_at
            .as_deref()
            .and_then(rymembra_domain::tempo::istante_ms),
        indirizzo: allegato.browser_download_url.clone(),
        nome: allegato.name.clone(),
        peso: allegato.size,
        versione,
    })
}

/// L'indirizzo di un allegato ha la forma che GitHub gli dà:
/// `https://github.com/<proprietario>/<progetto>/releases/download/<etichetta>/<nome>`.
///
/// # Perché, se l'APK lo verifica comunque Android
///
/// Perché la risposta della release la si legge e basta: quel che dice dove
/// scaricare è un dato, e un dato che manda il telefono altrove — un allegato
/// caricato con un indirizzo suo, una risposta storta — non deve diventare uno
/// scaricamento verso chiunque. Il progetto non si confronta: chi può scrivere
/// le release del progetto può anche caricarci un allegato, e un progetto
/// rinominato risponde con il nome nuovo. Quel che conta è che i byte arrivino
/// da GitHub; che siano Rymembra lo verifica `Installatore.kt` prima di
/// consegnarli.
#[must_use]
pub fn allegato_di_github(indirizzo: &str) -> bool {
    let Some(resto) = indirizzo.strip_prefix("https://github.com/") else {
        return false;
    };
    let parti: Vec<&str> = resto.split('/').collect();
    matches!(
        parti.as_slice(),
        [proprietario, progetto, "releases", "download", etichetta, nome]
            if [proprietario, progetto, etichetta, nome].iter().all(|parte| !parte.is_empty())
    )
}

/// Gli ospiti verso cui GitHub può rimandare lo scaricamento di un allegato.
///
/// `github.com` per il primo salto, e per il rimando di un progetto
/// rinominato; poi chi serve davvero i byte: `release-assets.githubusercontent.com`
/// (quello di oggi, visto rispondere a un `…/releases/download/…`) e
/// `objects.githubusercontent.com` (quello di prima). Non tutto
/// `*.githubusercontent.com`: `raw.githubusercontent.com` serve i file di
/// qualunque progetto.
const OSPITI_DEGLI_ALLEGATI: [&str; 3] = [
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

/// Un rimando dello scaricamento dell'APK si può seguire: `https://` e un
/// ospite di [`OSPITI_DEGLI_ALLEGATI`], esatto. L'ospite si legge dopo
/// l'ultima chiocciola (`rymembra_net::http::ospite`), così
/// `github.com@esempio.com` è `esempio.com`.
#[must_use]
pub fn rimando_di_github(bersaglio: &str) -> bool {
    if !bersaglio.starts_with("https://") {
        return false;
    }
    let Some(ospite) = rymembra_net::http::ospite(bersaglio) else {
        return false;
    };
    let ospite = ospite.trim_end_matches('.').to_ascii_lowercase();
    OSPITI_DEGLI_ALLEGATI.contains(&ospite.as_str())
}

/// I segni con cui il nome di un APK dice per quale architettura è: tutti
/// quelli che si usano, non solo quelli dei telefoni per cui Rymembra esce.
const ARCHITETTURE: [&str; 7] = [
    "arm64", "aarch64", "armeabi", "armv7", "x86_64", "x86", "x64",
];

/// L'APK giusto per questo telefono: quello che nomina la sua architettura,
/// o quello universale, o l'unico che c'è purché non ne nomini un'altra.
/// Nessuno se la release è solo per il computer.
///
/// # Perché l'unico non basta
///
/// Il ripiego sull'unico APK esiste per un file che non dice niente
/// (`rymembra.apk`). Uno che dice un'altra architettura — caricato solo quello
/// dell'emulatore, `…-x86_64.apk`, su un telefono `arm64` — Android lo
/// rifiuterebbe («App non installata») dopo decine di megabyte scaricati, e
/// l'avviso avrebbe promesso un aggiornamento che non c'è.
#[must_use]
pub fn apk_per<'a>(allegati: &'a [Allegato], architettura: &str) -> Option<&'a Allegato> {
    let apk: Vec<&Allegato> = allegati
        .iter()
        .filter(|allegato| allegato.name.to_ascii_lowercase().ends_with(".apk"))
        .collect();
    let nomi: &[&str] = match architettura {
        "aarch64" => &["arm64", "aarch64"],
        "x86_64" => &["x86_64", "x64"],
        _ => &[],
    };
    let nomina = |allegato: &&Allegato, parole: &[&str]| {
        let nome = allegato.name.to_ascii_lowercase();
        parole.iter().any(|parola| nome.contains(parola))
    };
    apk.iter()
        .find(|allegato| nomina(allegato, nomi))
        .or_else(|| apk.iter().find(|allegato| nomina(allegato, &["universal"])))
        .or(match apk.as_slice() {
            [unico] if !nomina(unico, &ARCHITETTURE) => Some(unico),
            _ => None,
        })
        .copied()
}

/// `nuova` viene dopo `corrente`: numeri separati da punti, confrontati come
/// numeri. Un suffisso (`3.3.0-beta.1`) vale come prima della versione piena.
#[must_use]
pub fn piu_nuova(nuova: &str, corrente: &str) -> bool {
    fn pezzi(versione: &str) -> (Vec<u64>, bool) {
        let (numeri, suffisso) = match versione.split_once(['-', '+']) {
            Some((numeri, _)) => (numeri, true),
            None => (versione, false),
        };
        let mut pezzi: Vec<u64> = numeri
            .split('.')
            .map(|pezzo| pezzo.trim().parse().unwrap_or(0))
            .collect();
        while pezzi.len() < 3 {
            pezzi.push(0);
        }
        (pezzi, suffisso)
    }
    let (a, a_suffisso) = pezzi(nuova);
    let (b, b_suffisso) = pezzi(corrente);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b_suffisso && !a_suffisso,
    }
}

/// L'ultimo controllo riuscito è di meno di `guardia` millisecondi fa. Un
/// orologio tornato indietro non conta come «poco fa»: bloccherebbe i
/// controlli fino a che non torna avanti.
#[must_use]
pub fn troppo_presto(ultimo_ms: Option<i64>, adesso_ms: i64, guardia_ms: i64) -> bool {
    let Some(ultimo_ms) = ultimo_ms else {
        return false;
    };
    (0..guardia_ms).contains(&adesso_ms.saturating_sub(ultimo_ms))
}

#[cfg(test)]
mod prove {
    use super::*;

    fn allegato(nome: &str) -> Allegato {
        allegato_da(
            nome,
            &format!("https://github.com/FedericoBaratti/Rymembra/releases/download/v3.3.0/{nome}"),
        )
    }

    fn allegato_da(nome: &str, indirizzo: &str) -> Allegato {
        Allegato {
            name: nome.to_owned(),
            browser_download_url: indirizzo.to_owned(),
            size: Some(10),
        }
    }

    fn rilascio(tag: &str, allegati: Vec<Allegato>) -> Rilascio {
        Rilascio {
            tag_name: tag.to_owned(),
            body: Some("  note  ".to_owned()),
            published_at: Some("2026-11-03T10:00:00Z".to_owned()),
            draft: false,
            prerelease: false,
            assets: allegati,
        }
    }

    #[test]
    fn il_progetto_si_legge_dall_indirizzo_dell_updater() {
        assert_eq!(
            progetto_da(
                "https://github.com/FedericoBaratti/Rymembra/releases/latest/download/latest.json"
            )
            .as_deref(),
            Some("FedericoBaratti/Rymembra")
        );
        assert_eq!(progetto_da("https://example.com/latest.json"), None);
        assert_eq!(progetto_da("https://github.com/solo"), None);
    }

    #[test]
    fn le_versioni_si_confrontano_come_numeri() {
        assert!(piu_nuova("3.10.0", "3.9.9"));
        assert!(piu_nuova("4.0", "3.99.99"));
        assert!(!piu_nuova("3.2.0", "3.2.0"));
        assert!(!piu_nuova("3.1.9", "3.2.0"));
        // La versione piena viene dopo la sua beta, mai prima.
        assert!(piu_nuova("3.3.0", "3.3.0-beta.1"));
        assert!(!piu_nuova("3.3.0-beta.1", "3.3.0"));
    }

    #[test]
    fn l_apk_giusto_e_quello_di_questo_telefono() {
        let allegati = [
            allegato("latest.json"),
            allegato("Rymembra_3.3.0_x64-setup.exe"),
            allegato("rymembra-3.3.0-x86_64.apk"),
            allegato("rymembra-3.3.0-arm64.apk"),
        ];
        assert_eq!(
            apk_per(&allegati, "aarch64").map(|a| a.name.as_str()),
            Some("rymembra-3.3.0-arm64.apk")
        );
        assert_eq!(
            apk_per(&allegati, "x86_64").map(|a| a.name.as_str()),
            Some("rymembra-3.3.0-x86_64.apk")
        );
        // Solo i file del computer: niente da annunciare sul telefono.
        assert!(apk_per(&allegati[..2], "aarch64").is_none());
        // Un APK solo, senza architettura nel nome: è quello.
        assert!(apk_per(&[allegato("rymembra.apk")], "aarch64").is_some());
        // Due senza architettura: nessuno, invece di uno a caso.
        assert!(apk_per(&[allegato("a.apk"), allegato("b.apk")], "aarch64").is_none());
    }

    #[test]
    fn l_unico_apk_di_un_altra_architettura_non_si_propone() {
        // L'unico che c'è, ma nomina un'altra architettura: l'installatore di
        // Android lo rifiuterebbe dopo decine di megabyte scaricati.
        assert!(apk_per(&[allegato("rymembra-3.3.0-x86_64.apk")], "aarch64").is_none());
        assert!(apk_per(&[allegato("rymembra-3.3.0-arm64.apk")], "x86_64").is_none());
        assert!(apk_per(&[allegato("rymembra-armeabi-v7a.apk")], "aarch64").is_none());
        assert!(apk_per(&[allegato("rymembra-x86.apk")], "x86_64").is_none());
        // Un telefono di cui non si conosce l'architettura: un APK che ne
        // nomina una non è il suo.
        assert!(apk_per(&[allegato("rymembra-arm64.apk")], "riscv64").is_none());
        // L'unico che non ne nomina nessuna resta quello.
        assert_eq!(
            apk_per(&[allegato("Rymembra-3.3.0.apk")], "x86_64").map(|a| a.name.as_str()),
            Some("Rymembra-3.3.0.apk")
        );
    }

    #[test]
    fn una_release_senza_apk_o_non_piu_nuova_non_si_annuncia() {
        let trovato = scegli(
            rilascio("v3.3.0", vec![allegato("rymembra-arm64.apk")]),
            "3.2.0",
            "aarch64",
        )
        .expect("una versione nuova con l'APK");
        assert_eq!(trovato.versione, "3.3.0");
        assert_eq!(trovato.note.as_deref(), Some("note"));
        assert!(trovato.data_ms.is_some());
        assert!(
            scegli(
                rilascio("v3.3.0", vec![allegato("setup.exe")]),
                "3.2.0",
                "aarch64"
            )
            .is_none()
        );
        assert!(
            scegli(
                rilascio("v3.2.0", vec![allegato("rymembra.apk")]),
                "3.2.0",
                "aarch64"
            )
            .is_none()
        );
        let mut prova = rilascio("v9.0.0", vec![allegato("rymembra.apk")]);
        prova.prerelease = true;
        assert!(scegli(prova, "3.2.0", "aarch64").is_none());
    }

    #[test]
    fn un_allegato_fuori_da_github_non_si_annuncia() {
        let con = |indirizzo: &str| {
            scegli(
                rilascio("v3.3.0", vec![allegato_da("rymembra-arm64.apk", indirizzo)]),
                "3.2.0",
                "aarch64",
            )
        };
        assert!(con("https://example.invalid/rymembra-arm64.apk").is_none());
        // La chiocciola: l'ospite vero è quello dopo.
        assert!(
            con(
                "https://github.com@example.invalid/o/r/releases/download/v3.3.0/rymembra-arm64.apk"
            )
            .is_none()
        );
        assert!(con("http://github.com/o/r/releases/download/v3.3.0/rymembra-arm64.apk").is_none());
        assert!(
            con("https://github.com:8443/o/r/releases/download/v3.3.0/rymembra-arm64.apk")
                .is_none()
        );
        // Un ospite di GitHub, ma non quello degli allegati: lì serve file
        // chiunque abbia un progetto.
        assert!(con("https://raw.githubusercontent.com/o/r/main/rymembra-arm64.apk").is_none());
        assert!(
            con("https://release-assets.githubusercontent.com/github-production-release-asset/1/2")
                .is_none()
        );
        // Su github.com, ma non fra gli allegati di una release.
        assert!(con("https://github.com/o/r/raw/main/rymembra-arm64.apk").is_none());
        assert!(con("https://github.com/o/r/releases/download/v3.3.0/").is_none());
        // La forma che GitHub dà davvero.
        assert!(
            con("https://github.com/o/r/releases/download/v3.3.0/rymembra-arm64.apk").is_some()
        );
    }

    #[test]
    fn i_rimandi_dello_scaricamento_restano_a_casa_di_github() {
        // Dove GitHub manda davvero lo scaricamento di un allegato.
        assert!(rimando_di_github(
            "https://release-assets.githubusercontent.com/github-production-release-asset/212613049/6460b392?sp=r&sig=x"
        ));
        assert!(rimando_di_github(
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2?X-Amz-Signature=x"
        ));
        // Un progetto rinominato rimanda prima al nome nuovo.
        assert!(rimando_di_github(
            "https://github.com/nuovo/r/releases/download/v3.3.0/rymembra-arm64.apk"
        ));
        assert!(rimando_di_github(
            "https://Release-Assets.GitHubUserContent.com./x"
        ));
        assert!(!rimando_di_github("https://example.invalid/x.apk"));
        assert!(!rimando_di_github(
            "http://release-assets.githubusercontent.com/x"
        ));
        assert!(!rimando_di_github(
            "https://release-assets.githubusercontent.com@example.invalid/x"
        ));
        assert!(!rimando_di_github(
            "https://raw.githubusercontent.com/o/r/main/x.apk"
        ));
        assert!(!rimando_di_github("https://evilgithub.com/x"));
        assert!(!rimando_di_github(
            "https://release-assets.githubusercontent.com.example.invalid/x"
        ));
    }

    #[test]
    fn la_guardia_tiene_fuori_solo_i_controlli_di_poco_fa() {
        let ora = 100 * 60 * 60 * 1_000;
        let sei_ore = 6 * 60 * 60 * 1_000;
        assert!(troppo_presto(Some(ora - 60_000), ora, sei_ore));
        assert!(!troppo_presto(
            Some(ora - 7 * 60 * 60 * 1_000),
            ora,
            sei_ore
        ));
        assert!(!troppo_presto(None, ora, sei_ore));
        assert!(!troppo_presto(Some(ora + 60_000), ora, sei_ore));
    }
}
