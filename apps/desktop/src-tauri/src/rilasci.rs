//! Le release di GitHub, per gli aggiornamenti del telefono.
//!
//! Le regole senza la rete e senza Android — quale progetto, quale versione è
//! più nuova, quale APK è di questo telefono, se la sua firma torna, se questa
//! copia lo può installare — staccate da `mobile/aggiornamenti.rs` perché si
//! provino anche sul computer, con `cargo test`. Là c'è il filo, la rete e
//! l'installatore.
//!
//! # Le note
//!
//! Non il corpo della release: quella è la pagina web, scritta per chi scarica
//! l'installer di Windows (SmartScreen, l'icona vicino all'orologio, il
//! Markdown), e sul telefono finiva tale e quale sotto «Che cosa cambia». Le
//! note sono quelle che legge il computer, il campo `notes` di `latest.json`,
//! che `strumenti/manifesto.js` ricava dal CHANGELOG ed è un allegato della
//! stessa release ([`note_dal_manifesto`]).
//!
//! # La firma
//!
//! Sul computer l'updater verifica la firma minisign dell'installer con la
//! chiave pubblica compilata dentro l'eseguibile (`plugins.updater.pubkey`).
//! Sul telefono lo stesso, con la stessa chiave, sull'APK: la release porta
//! accanto a `…android-arm64.apk` la sua firma `…android-arm64.apk.sig`, e un
//! APK senza la firma non si annuncia ([`scegli`]). La verifica
//! ([`verifica_la_firma`]) viene **prima** di quella di Android, che controlla
//! soltanto che l'APK sia firmato con la stessa chiave di questa copia:
//! insieme dicono che i byte sono quelli che abbiamo pubblicato noi, e che
//! Android li installa sopra.

use serde::Deserialize;

/// Una release di GitHub, per quel che serve qui.
///
/// Il corpo (`body`) non c'è, e non per dimenticanza: vedi «Le note» qui sopra.
#[derive(Debug, Deserialize)]
pub struct Rilascio {
    /// L'etichetta: `v3.3.0`.
    pub tag_name: String,
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
    /// Le note di rilascio, da `latest.json`: le mette chi ha la rete
    /// (`mobile/aggiornamenti.rs`), [`scegli`] le lascia vuote.
    pub note: Option<String>,
    /// Quando è uscita.
    pub data_ms: Option<i64>,
    /// Dove si scarica l'APK.
    pub indirizzo: String,
    /// Il nome del file.
    pub nome: String,
    /// Quanto pesa, se GitHub lo dice.
    pub peso: Option<u64>,
    /// Dove si scarica la firma minisign dell'APK, `<nome>.sig`.
    pub firma: String,
    /// Dove si scarica `latest.json`, da cui vengono le note. Una release che
    /// non lo porta si annuncia lo stesso, senza note.
    pub manifesto: Option<String>,
}

/// Il nome dell'allegato da cui vengono le note: lo stesso che legge l'updater
/// del computer.
const MANIFESTO: &str = "latest.json";

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
/// `architettura` (`std::env::consts::ARCH`) che si scarica da GitHub, con
/// accanto la sua firma (`<nome dell'APK>.sig`), anche lei da GitHub.
///
/// Senza la firma l'APK non si verifica, e non si annuncia: un avviso che porta
/// a decine di megabyte scaricati e poi rifiutati è peggio di nessun avviso. Le
/// note non sono il corpo della release (vedi «Le note» nel preambolo): le
/// mette chi ha la rete, da [`Trovato::manifesto`].
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
    let nome_della_firma = format!("{}.sig", allegato.name);
    let firma = rilascio
        .assets
        .iter()
        .find(|altro| altro.name == nome_della_firma)
        .map(|firma| firma.browser_download_url.clone())
        .filter(|indirizzo| allegato_di_github(indirizzo))?;
    let manifesto = rilascio
        .assets
        .iter()
        .find(|altro| altro.name == MANIFESTO)
        .map(|manifesto| manifesto.browser_download_url.clone())
        .filter(|indirizzo| allegato_di_github(indirizzo));
    Some(Trovato {
        note: None,
        data_ms: rilascio
            .published_at
            .as_deref()
            .and_then(rymembra_domain::tempo::istante_ms),
        indirizzo: allegato.browser_download_url.clone(),
        nome: allegato.name.clone(),
        peso: allegato.size,
        firma,
        manifesto,
        versione,
    })
}

/// Le note di `latest.json` (il campo `notes`), se parla di `versione`.
///
/// Un manifesto di un'altra versione — una release ricaricata a metà, un
/// allegato rimasto da prima — non dà note: sarebbero quelle di un'altra
/// versione sotto il titolo di questa. Niente note è meglio di note sbagliate,
/// e il ripiego sul corpo della release non c'è.
#[must_use]
pub fn note_dal_manifesto(corpo: &[u8], versione: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Manifesto {
        version: String,
        #[serde(default)]
        notes: Option<String>,
    }
    let manifesto: Manifesto = serde_json::from_slice(corpo).ok()?;
    if senza_v(&manifesto.version) != senza_v(versione) {
        return None;
    }
    manifesto
        .notes
        .map(|note| note.trim().to_owned())
        .filter(|note| !note.is_empty())
}

/// Una versione senza la `v` davanti e senza spazi ai bordi.
fn senza_v(versione: &str) -> &str {
    versione.trim().trim_start_matches(['v', 'V'])
}

// ── la chiave di questa copia ───────────────────────────────────────────────

/// L'impronta SHA-256 del certificato con cui si firmano gli APK pubblicati.
///
/// Android installa un APK sopra Rymembra solo se è firmato con la stessa
/// chiave della copia installata. Le copie di prova — compilate in debug, con
/// la chiave di prova di Gradle — non possono quindi ricevere l'APK pubblico, e
/// saperlo **prima** di scaricarlo evita quaranta megabyte che Android
/// rifiuterebbe ([`copia_di_prova`]). La stessa impronta che il lavoro della
/// release controlla sull'APK firmato.
pub const IMPRONTA_DEL_RILASCIO: &str =
    "2485942c0f5d734297cd96a66519c72e4922c70537e8f39ab2d9806024182ac1";

/// Un'impronta SHA-256 in esadecimale minuscolo, senza separatori: accetta i
/// due punti e le maiuscole di `apksigner` e `keytool`. `None` se non è
/// un'impronta.
#[must_use]
pub fn impronta_normale(testo: &str) -> Option<String> {
    let cifre: String = testo
        .chars()
        .filter(|c| *c != ':' && !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    (cifre.len() == 64 && cifre.chars().all(|c| c.is_ascii_hexdigit())).then_some(cifre)
}

/// Questa copia è firmata con un'altra chiave di quella del rilascio:
/// `Some(true)` se l'APK pubblico non le si installa sopra.
///
/// `firmatari` sono le impronte dei certificati con cui è firmata adesso
/// (`Installatore.firmatari`). Con più firmatari Android vuole lo stesso
/// insieme, e l'APK pubblico ne ha uno solo: anche quella è un'altra chiave.
/// `None` se non si sa — un elenco vuoto, un'impronta illeggibile —, e allora
/// decide la verifica di Android dopo lo scaricamento.
#[must_use]
pub fn copia_di_prova(firmatari: &[String]) -> Option<bool> {
    let impronte: Option<Vec<String>> = firmatari.iter().map(|f| impronta_normale(f)).collect();
    let impronte = impronte.filter(|impronte| !impronte.is_empty())?;
    Some(impronte != [IMPRONTA_DEL_RILASCIO])
}

/// L'APK di `versione` non si installa sopra questa copia, e non si scarica:
/// la copia è firmata con un'altra chiave ([`copia_di_prova`]), o Android
/// l'ha già rifiutato una volta per la firma (`rifiutata` è la versione che il
/// database ricorda). Il rifiuto vale per quella versione sola: la prossima si
/// riprova.
#[must_use]
pub fn non_installabile(
    copia_di_prova: Option<bool>,
    rifiutata: Option<&str>,
    versione: &str,
) -> bool {
    copia_di_prova == Some(true)
        || rifiutata.is_some_and(|rifiutata| senza_v(rifiutata) == senza_v(versione))
}

// ── la firma minisign ───────────────────────────────────────────────────────

/// Quanto si legge alla volta dell'APK, verificandolo.
#[cfg(any(target_os = "android", test))]
const PASSO_DI_LETTURA: usize = 64 * 1024;

/// I byte di `dati` sono quelli firmati con la chiave dell'updater, per
/// `versione`.
///
/// `chiave` è `plugins.updater.pubkey` di `tauri.conf.json`, la stessa che
/// verifica gli installer del computer: base64 di un file `.pub` di minisign.
/// `firma` è il contenuto del `.sig` che `tauri signer sign --app-version`
/// scrive accanto all'APK: base64 del testo minisign. La versione nel commento
/// fidato (`version:3.3.0`) deve essere quella della release, come vuole
/// `requireSignedVersion` sul computer: una firma vera di un APK vecchio,
/// accanto al numero di una versione nuova, non passa.
///
/// Si legge a pezzi: quaranta megabyte in memoria su un telefono per sapere
/// se tornano sono troppi. Le firme «preimpastate» di minisign, quelle che fa
/// Tauri, lo permettono; una firma del vecchio tipo si legge intera, come la
/// legge l'updater del computer.
///
/// # Errori
///
/// Il motivo, in testo, per la causa dell'errore: la chiave o la firma che non
/// si leggono, la firma che non torna, una versione diversa, una lettura che
/// non riesce.
#[cfg(any(target_os = "android", test))]
pub fn verifica_la_firma(
    chiave: &str,
    firma: &str,
    versione: &str,
    dati: &mut impl std::io::Read,
) -> Result<(), String> {
    use minisign_verify::{PublicKey, Signature};

    let chiave = testo_da_base64(chiave).ok_or("la chiave pubblica dell'updater non è base64")?;
    let chiave = PublicKey::decode(&chiave)
        .map_err(|err| format!("la chiave pubblica dell'updater non si legge: {err}"))?;
    let firma = testo_da_base64(firma).ok_or("la firma dell'APK non è base64")?;
    let firma = Signature::decode(&firma)
        .map_err(|err| format!("la firma dell'APK non si legge: {err}"))?;
    let lettura = |err: std::io::Error| format!("l'APK non si legge: {err}");
    match chiave.verify_stream(&firma) {
        Ok(mut verifica) => {
            let mut pezzo = vec![0_u8; PASSO_DI_LETTURA];
            loop {
                let letti = dati.read(&mut pezzo).map_err(lettura)?;
                let Some(letto) = pezzo.get(..letti).filter(|letto| !letto.is_empty()) else {
                    break;
                };
                verifica.update(letto);
            }
            verifica
                .finalize()
                .map_err(|err| format!("la firma dell'APK non torna: {err}"))?;
        }
        Err(minisign_verify::Error::UnsupportedLegacyMode) => {
            let mut tutti = Vec::new();
            dati.read_to_end(&mut tutti).map_err(lettura)?;
            chiave
                .verify(&tutti, &firma, true)
                .map_err(|err| format!("la firma dell'APK non torna: {err}"))?;
        }
        Err(err) => return Err(format!("la firma dell'APK non torna: {err}")),
    }
    // Solo adesso il commento fidato è fidato: la firma globale lo copre, e
    // l'ha appena verificata `finalize` (o `verify`).
    let firmata = firma
        .trusted_comment()
        .split('\t')
        .find_map(|campo| campo.strip_prefix("version:"));
    match firmata {
        Some(firmata) if senza_v(firmata) == senza_v(versione) => Ok(()),
        Some(firmata) => Err(format!(
            "la firma copre la versione {firmata}, la release annuncia la {versione}"
        )),
        None => Err("la firma non dice quale versione copre".to_owned()),
    }
}

/// Il testo dentro una base64 dell'alfabeto di serie, quella di
/// `tauri signer`. Una a capo in fondo si toglie; `-` e `_` no, perché non
/// sono di questo alfabeto: passano per [`rymembra_oauth::da_base64url`] con
/// `+` e `/` scambiati, ed è l'unica differenza fra i due.
#[cfg(any(target_os = "android", test))]
fn testo_da_base64(testo: &str) -> Option<String> {
    let testo = testo.trim();
    if testo.contains(['-', '_']) {
        return None;
    }
    let byte = rymembra_oauth::da_base64url(&testo.replace('+', "-").replace('/', "_"))?;
    String::from_utf8(byte).ok()
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

/// La passata d'avvio si salta: l'ultimo controllo riuscito è di poco fa
/// ([`troppo_presto`]) **e** non aveva trovato niente.
///
/// `trovata` è la versione che l'ultimo controllo riuscito aveva trovato,
/// come la ricorda il database. L'aggiornamento trovato vive solo in memoria,
/// e su un telefono il processo muore di continuo: senza questa eccezione chi
/// riapre l'app un'ora dopo l'avviso non lo rivedrebbe fino alla passata dopo,
/// sei ore più in là. È la regola del computer (`CHIAVE_TROVATA` in
/// `aggiornamenti.rs`).
#[must_use]
pub fn passata_d_avvio_saltata(
    ultimo_ms: Option<i64>,
    adesso_ms: i64,
    guardia_ms: i64,
    trovata: Option<&str>,
) -> bool {
    troppo_presto(ultimo_ms, adesso_ms, guardia_ms)
        && !trovata.is_some_and(|versione| !versione.trim().is_empty())
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
            published_at: Some("2026-11-03T10:00:00Z".to_owned()),
            draft: false,
            prerelease: false,
            assets: allegati,
        }
    }

    /// Un APK e la sua firma, come li pubblica la release.
    fn con_firma(nome: &str) -> Vec<Allegato> {
        vec![allegato(nome), allegato(&format!("{nome}.sig"))]
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
            rilascio("v3.3.0", con_firma("rymembra-arm64.apk")),
            "3.2.0",
            "aarch64",
        )
        .expect("una versione nuova con l'APK");
        assert_eq!(trovato.versione, "3.3.0");
        assert!(trovato.data_ms.is_some());
        assert!(trovato.firma.ends_with("/rymembra-arm64.apk.sig"));
        // Senza `latest.json` fra gli allegati, niente note da prendere.
        assert_eq!(trovato.manifesto, None);
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

    /// Il corpo della release è la pagina web, scritta per l'installer di
    /// Windows: SmartScreen, l'icona vicino all'orologio, il Markdown a vista.
    /// Sul telefono finiva tale e quale sotto «Che cosa cambia».
    #[test]
    fn le_note_del_telefono_non_sono_il_corpo_della_release() {
        // La risposta di GitHub, col corpo che la release ha davvero.
        let risposta = serde_json::json!({
            "tag_name": "v3.3.0",
            "body": "### Before installing\n**This installer is not signed.**",
            "assets": [
                { "name": "Rymembra_3.3.0_android-arm64.apk",
                  "browser_download_url": "https://github.com/o/r/releases/download/v3.3.0/Rymembra_3.3.0_android-arm64.apk" },
                { "name": "Rymembra_3.3.0_android-arm64.apk.sig",
                  "browser_download_url": "https://github.com/o/r/releases/download/v3.3.0/Rymembra_3.3.0_android-arm64.apk.sig" },
                { "name": "latest.json",
                  "browser_download_url": "https://github.com/o/r/releases/download/v3.3.0/latest.json" }
            ]
        });
        let con_corpo: Rilascio = serde_json::from_value(risposta).expect("una release");
        let trovato = scegli(con_corpo, "3.2.1", "aarch64").expect("una versione nuova con l'APK");
        assert!(
            !trovato.note.as_deref().unwrap_or("").contains("installer"),
            "le note sono il corpo della release: {:?}",
            trovato.note
        );
        // Le note vengono dal manifesto della stessa release.
        assert_eq!(
            trovato.manifesto.as_deref(),
            Some("https://github.com/o/r/releases/download/v3.3.0/latest.json")
        );
    }

    /// Le note del telefono sono quelle del computer: il campo `notes` di
    /// `latest.json`, se il manifesto parla di questa versione.
    #[test]
    fn le_note_vengono_dal_manifesto_della_stessa_versione() {
        let manifesto =
            br#"{"version":"3.3.0","notes":"  * Aggiunto: l'equalizzatore\n  ","platforms":{}}"#;
        assert_eq!(
            note_dal_manifesto(manifesto, "3.3.0").as_deref(),
            Some("* Aggiunto: l'equalizzatore")
        );
        assert!(note_dal_manifesto(manifesto, "v3.3.0").is_some());
        // Il manifesto di un'altra versione: le sue note non sono di questa.
        assert_eq!(note_dal_manifesto(manifesto, "3.3.1"), None);
        // Note vuote, assenti, o un corpo che non è un manifesto: niente.
        assert_eq!(
            note_dal_manifesto(br#"{"version":"3.3.0","notes":"  "}"#, "3.3.0"),
            None
        );
        assert_eq!(note_dal_manifesto(br#"{"version":"3.3.0"}"#, "3.3.0"), None);
        assert_eq!(note_dal_manifesto(b"<html>", "3.3.0"), None);
    }

    #[test]
    fn una_copia_con_chiave_di_prova_non_scarica() {
        // La forma di `apksigner`: due punti e maiuscole.
        let rilascio = "24:85:94:2C:0F:5D:73:42:97:CD:96:A6:65:19:C7:2E:49:22:C7:05:37:E8:F3:9A:B2:D9:80:60:24:18:2A:C1";
        assert_eq!(
            impronta_normale(rilascio).as_deref(),
            Some(IMPRONTA_DEL_RILASCIO)
        );
        assert_eq!(copia_di_prova(&[rilascio.to_owned()]), Some(false));
        assert_eq!(
            copia_di_prova(&[IMPRONTA_DEL_RILASCIO.to_owned()]),
            Some(false)
        );
        // Un'altra chiave, come quella di debug di Gradle che firma le copie
        // uscite finora.
        let debug = "4d".repeat(32);
        assert_eq!(copia_di_prova(std::slice::from_ref(&debug)), Some(true));
        // Due firmatari: Android vuole lo stesso insieme, e l'APK ne ha uno.
        assert_eq!(
            copia_di_prova(&[IMPRONTA_DEL_RILASCIO.to_owned(), debug.clone()]),
            Some(true)
        );
        // Non si sa: decide Android, dopo.
        assert_eq!(copia_di_prova(&[]), None);
        assert_eq!(copia_di_prova(&["non è un'impronta".to_owned()]), None);
        // E una copia di prova non scarica nessuna versione.
        assert!(non_installabile(Some(true), None, "3.3.0"));
        assert!(!non_installabile(Some(false), None, "3.3.0"));
        assert!(!non_installabile(None, None, "3.3.0"));
    }

    #[test]
    fn dopo_un_rifiuto_di_firma_la_versione_non_si_ripropone_come_installabile() {
        // Android l'ha rifiutata: non si riscarica a ogni tocco.
        assert!(non_installabile(None, Some("3.3.0"), "3.3.0"));
        assert!(non_installabile(Some(false), Some("v3.3.0"), "3.3.0"));
        // La versione dopo si riprova.
        assert!(!non_installabile(Some(false), Some("3.3.0"), "3.3.1"));
    }

    // ── la firma minisign ──
    //
    // Una chiave usa e getta, fatta per queste prove con `tauri signer
    // generate`, e la firma di `DATI_DI_PROVA` con `tauri signer sign
    // --app-version 3.3.0`: la stessa forma della chiave dell'updater e dei
    // `.sig` della release. La chiave segreta non c'è più.

    /// Il `.pub` della chiave di prova, in base64 come `plugins.updater.pubkey`.
    const CHIAVE_DI_PROVA: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDRGMDhEQjhBNTE3QkZDNDUKUldSRi9IdFJpdHNJVDdtdWh2UnRYbmhzSDNJMkdXL0xJZHRVNmtBWHcvWUJvRUdUd25aMS9LRHkK";

    /// Il `.sig` di `DATI_DI_PROVA`, com'è scritto accanto all'APK.
    const FIRMA_DI_PROVA: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSRi9IdFJpdHNJVDBaOFhweGJoYUFtR3BkTExrRm9xV29ydG5oV0dETWN3Y2psckNYUlVXMzlWazRnY2ZTZFNJR1NDWU8yQTBOM0RSZ0FWUnFZZmc5QmwzUnMydzBZYkE4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0NDAxMzc2CWZpbGU6UnltZW1icmFfMy4zLjBfYW5kcm9pZC1hcm02NC5hcGsJdmVyc2lvbjozLjMuMApaR2xmYlBidFY3aXFhOFJUWkZDOE5Qb3YrSGozN2h3dnNJQ0hmQzNUVy80U3MxSjFZQjQvN0lZZDdvL2diZDkyUm8vRmljdExBMlZoSkZhbm13ZDFEQT09Cg==";

    const DATI_DI_PROVA: &[u8] = b"Un APK di prova: non e' un APK, e' la firma che conta.\n";

    fn verifica(chiave: &str, firma: &str, versione: &str, dati: &[u8]) -> Result<(), String> {
        verifica_la_firma(chiave, firma, versione, &mut std::io::Cursor::new(dati))
    }

    #[test]
    fn l_apk_firmato_con_la_chiave_dell_updater_passa() {
        assert_eq!(
            verifica(CHIAVE_DI_PROVA, FIRMA_DI_PROVA, "3.3.0", DATI_DI_PROVA),
            Ok(())
        );
        // La `v` del tag, e l'a capo in fondo a un `.sig` letto da un file.
        let con_a_capo = format!("{FIRMA_DI_PROVA}\n");
        assert_eq!(
            verifica(CHIAVE_DI_PROVA, &con_a_capo, "v3.3.0", DATI_DI_PROVA),
            Ok(())
        );
    }

    #[test]
    fn un_apk_cambiato_non_passa() {
        let mut cambiato = DATI_DI_PROVA.to_vec();
        if let Some(primo) = cambiato.first_mut() {
            *primo ^= 1;
        }
        assert!(verifica(CHIAVE_DI_PROVA, FIRMA_DI_PROVA, "3.3.0", &cambiato).is_err());
        let monco = DATI_DI_PROVA.get(..20).unwrap_or_default();
        assert!(verifica(CHIAVE_DI_PROVA, FIRMA_DI_PROVA, "3.3.0", monco).is_err());
    }

    #[test]
    fn una_firma_vera_di_un_altra_versione_non_passa() {
        // L'APK della 3.3.0, annunciato come 3.4.0: la firma è vera, la
        // versione no. È quel che `requireSignedVersion` ferma sul computer.
        let err = verifica(CHIAVE_DI_PROVA, FIRMA_DI_PROVA, "3.4.0", DATI_DI_PROVA)
            .expect_err("un'altra versione");
        assert!(err.contains("3.3.0") && err.contains("3.4.0"), "{err}");
    }

    #[test]
    fn la_firma_di_un_altra_chiave_non_passa() {
        // La chiave vera dell'updater, quella della configurazione: la firma di
        // prova non è sua.
        let configurazione: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("la configurazione");
        let vera = configurazione["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("la chiave dell'updater");
        let err =
            verifica(vera, FIRMA_DI_PROVA, "3.3.0", DATI_DI_PROVA).expect_err("un'altra chiave");
        // Ma la chiave vera si legge: è la stessa forma di quella di prova.
        assert!(!err.contains("non si legge"), "{err}");
    }

    #[test]
    fn una_firma_storta_non_passa() {
        for storta in ["", "non è base64!", "dGVzdG8=", "-_-_"] {
            assert!(
                verifica(CHIAVE_DI_PROVA, storta, "3.3.0", DATI_DI_PROVA).is_err(),
                "passata: {storta:?}"
            );
        }
        assert!(verifica("", FIRMA_DI_PROVA, "3.3.0", DATI_DI_PROVA).is_err());
    }

    /// Senza la firma minisign l'APK non si verifica, e non si annuncia: un
    /// avviso che porta a decine di megabyte scaricati e poi rifiutati è peggio
    /// di nessun avviso (D-6).
    #[test]
    fn una_release_senza_la_firma_dell_apk_non_si_annuncia() {
        assert!(
            scegli(
                rilascio("v3.3.0", vec![allegato("Rymembra_3.3.0_android-arm64.apk")]),
                "3.2.1",
                "aarch64"
            )
            .is_none(),
            "annunciato un APK senza la sua firma"
        );
    }

    #[test]
    fn un_allegato_fuori_da_github_non_si_annuncia() {
        let con = |indirizzo: &str| {
            scegli(
                rilascio(
                    "v3.3.0",
                    vec![
                        allegato_da("rymembra-arm64.apk", indirizzo),
                        allegato("rymembra-arm64.apk.sig"),
                    ],
                ),
                "3.2.0",
                "aarch64",
            )
        };
        // E la firma, anche lei, solo da GitHub.
        assert!(
            scegli(
                rilascio(
                    "v3.3.0",
                    vec![
                        allegato("rymembra-arm64.apk"),
                        allegato_da("rymembra-arm64.apk.sig", "https://example.invalid/x.sig"),
                    ],
                ),
                "3.2.0",
                "aarch64"
            )
            .is_none()
        );
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

    #[test]
    fn la_guardia_cede_se_l_ultimo_controllo_aveva_trovato_qualcosa() {
        let ora = 100 * 60 * 60 * 1_000;
        let sei_ore = 6 * 60 * 60 * 1_000;
        let un_ora_fa = Some(ora - 60 * 60 * 1_000);
        // Un'ora fa non c'era niente: la passata d'avvio si salta.
        assert!(passata_d_avvio_saltata(un_ora_fa, ora, sei_ore, None));
        // Un'ora fa c'era la 3.3.0, e il processo nel frattempo è morto con
        // l'avviso: la passata d'avvio si fa, ed è il solo modo di riaverlo.
        assert!(!passata_d_avvio_saltata(
            un_ora_fa,
            ora,
            sei_ore,
            Some("3.3.0")
        ));
        // Una chiave vuota non è una versione.
        assert!(passata_d_avvio_saltata(un_ora_fa, ora, sei_ore, Some("  ")));
        // Senza un controllo recente si controlla comunque.
        assert!(!passata_d_avvio_saltata(None, ora, sei_ore, None));
        assert!(!passata_d_avvio_saltata(
            Some(ora - 7 * 60 * 60 * 1_000),
            ora,
            sei_ore,
            None
        ));
    }
}
