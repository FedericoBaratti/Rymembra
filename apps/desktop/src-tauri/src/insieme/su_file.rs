//! La copertina di una voce di sessione, come file.
//!
//! # Perché
//!
//! Le copertine di «Ascolta insieme» (`insieme-<uid>`) arrivano dalla rete e
//! stanno in memoria ([`super::voci::Copertine`]): la finestra le chiede a
//! `rymembra-cover`, che le serve da lì. Le schede del sistema invece — Android
//! Auto, la notifica, la schermata di blocco, le SMTC di Windows — vogliono un
//! **file**: senza, un brano che suona dalla sessione di un altro PC restava
//! senza immagine fuori dalla finestra (F1, punto 8).
//!
//! # Dove
//!
//! Nella cache delle copertine lontane, che Android sa già servire
//! (`Copertine.kt`, la seconda radice), in una cartella riservata: la
//! sessantaquattro zeri ([`CARTELLA`]), che non è la chiave di nessun
//! dispositivo, e dentro `<impronta del gettone>.jpg`. Il percorso resta nella
//! forma `lontane/<64 hex>/<hex>.jpg` che il fornitore accetta già, e il suo
//! controllo non si allarga.
//!
//! # Il nome
//!
//! Viene dal [`Gettone`] della copertina, che è casuale per sessione e per
//! file: due sessioni non si confondono, anche se i numeri di voce (`uid`)
//! ricominciano, e due voci con la stessa copertina dividono lo stesso file. È
//! però la sua [impronta](Gettone::impronta) e non il gettone: il gettone è un
//! permesso di lettura che chi lo ha può usare sul PC dell'altro, e un nome di
//! file finisce in un indirizzo `content://` che ogni controllore della
//! sessione multimediale (l'auto, un orologio, un ascoltatore di notifiche)
//! può leggere. L'impronta è fatta apposta per finire dove un gettone non deve.
//!
//! # Quando si toglie
//!
//! Alla fine della sessione ([`svuota`]), con la memoria: una copertina di un
//! altro PC non deve restare sul telefono dopo che l'ascolto è finito. Se il
//! processo muore a metà restano i file, e vanno via alla fine della sessione
//! dopo, o con la potatura della cache (`lontano::sfoglia::pota`, che conta
//! anche questa cartella).

use std::path::{Path, PathBuf};

use rymembra_insieme::Gettone;

/// Il nome della cartella riservata: sessantaquattro zeri. Passa la stessa
/// regola delle chiavi dei dispositivi (64 cifre esadecimali) senza essere la
/// chiave di nessuno: una chiave `lontano-<64 zeri>-…` non supera mai il
/// cancello di `lontano::sfoglia`, che vuole un dispositivo collegato.
pub(super) const CARTELLA: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Il file della copertina di `gettone`, dentro la cache `cache`.
pub(super) fn percorso(cache: &Path, gettone: &Gettone) -> PathBuf {
    cache
        .join(CARTELLA)
        .join(format!("{}.jpg", gettone.impronta()))
}

/// Lo stesso file come lo chiede Android: `lontane/<64 zeri>/<impronta>.jpg`,
/// con `lontane` al posto della cache, che Rust dice a Kotlin una volta sola
/// (`sessione.pronta`). `None` se `file` non è uno di quelli di [`percorso`].
#[cfg(any(mobile, test))]
pub(super) fn per_android(file: &Path) -> Option<String> {
    let nome = file.file_name()?.to_str()?;
    let impronta = nome.strip_suffix(".jpg")?;
    let cartella = file.parent()?.file_name()?.to_str()?;
    let esadecimale = (2..=128).contains(&impronta.len())
        && impronta
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    (cartella == CARTELLA && esadecimale).then(|| format!("lontane/{CARTELLA}/{nome}"))
}

/// Mette la copertina nella cartella riservata, se non c'è già, e dice dove sta.
///
/// Un file con quel nome c'è già se un'altra scheda l'ha messo prima: il nome
/// viene dal gettone, e il gettone è di quei byte soltanto. Si scrive in un
/// file provvisorio che poi si rinomina, così chi legge non trova mai un file a
/// metà.
///
/// # Errori
///
/// Quelli del disco: la cartella che non si crea, il file che non si scrive.
pub(super) fn scrivi_se_manca(
    cache: &Path,
    gettone: &Gettone,
    byte: &[u8],
) -> std::io::Result<PathBuf> {
    let file = percorso(cache, gettone);
    if file.is_file() {
        return Ok(file);
    }
    if let Some(cartella) = file.parent() {
        std::fs::create_dir_all(cartella)?;
    }
    let provvisorio = file.with_extension("part");
    let scritto =
        std::fs::write(&provvisorio, byte).and_then(|()| std::fs::rename(&provvisorio, &file));
    if let Err(err) = scritto {
        let _ = std::fs::remove_file(&provvisorio);
        return Err(err);
    }
    Ok(file)
}

/// Toglie tutte le copertine di sessione dalla cache `cache`: la cartella
/// riservata, e niente altro. Le cartelle dei dispositivi collegati restano.
///
/// Una cartella che non c'è non è un guasto. Un file che non si toglie — un
/// lettore che lo tiene aperto, su Windows — resta, e se ne va con la potatura
/// della cache o alla fine della sessione dopo: lo si scrive nel diario, una
/// volta per chiamata.
pub(super) fn svuota(cache: &Path) {
    match std::fs::remove_dir_all(cache.join(CARTELLA)) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => crate::nota!(
            "[insieme] le copertine di sessione non si tolgono tutte: {}",
            err.kind()
        ),
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    const G1: Gettone = Gettone([1; 16]);
    const G2: Gettone = Gettone([2; 16]);

    #[test]
    fn il_nome_viene_dal_gettone_e_non_e_il_gettone() {
        let cache = Path::new("cache").join("copertine-lontane");
        let file = percorso(&cache, &G1);
        assert_eq!(file.parent(), Some(cache.join(CARTELLA).as_path()));
        let nome = file.file_name().unwrap().to_str().unwrap();
        assert_eq!(nome, format!("{}.jpg", G1.impronta()));
        // Due gettoni, due file; lo stesso gettone, lo stesso file.
        assert_ne!(percorso(&cache, &G1), percorso(&cache, &G2));
        assert_eq!(percorso(&cache, &G1), percorso(&cache, &Gettone([1; 16])));
        // Il gettone per esteso non compare mai in un nome di file.
        let esteso: String = G1.0.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!file.to_str().unwrap().contains(&esteso));
    }

    /// La forma che `Copertine.kt` accetta: `lontane/<64 hex>/<hex 2-128>.jpg`.
    #[test]
    fn per_android_e_la_forma_che_kotlin_accetta() {
        let cache = Path::new("cache");
        let file = percorso(cache, &G1);
        let relativo = per_android(&file).expect("un percorso di sessione");
        assert_eq!(
            relativo,
            format!("lontane/{CARTELLA}/{}.jpg", G1.impronta())
        );
        let parti: Vec<&str> = relativo.split('/').collect();
        assert_eq!(parti.len(), 3);
        assert_eq!(parti[0], "lontane");
        assert_eq!(parti[1].len(), 64);
        assert!(parti[1].bytes().all(|b| b == b'0'));
        // Il nome è esadecimale minuscolo di 2-128 cifre.
        let impronta = parti[2].strip_suffix(".jpg").unwrap();
        assert!((2..=128).contains(&impronta.len()));
        assert!(
            impronta
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        // Quel che non è di sessione non passa: un'altra cartella, un nome
        // storto, un'altra estensione.
        for storto in [
            Path::new("cache").join("a".repeat(64)).join("abcd.jpg"),
            Path::new("cache").join(CARTELLA).join("ABCD.jpg"),
            Path::new("cache").join(CARTELLA).join("abcd.png"),
            Path::new("cache").join(CARTELLA).join("a.jpg"),
            Path::new("cache").join(CARTELLA).join("..jpg"),
            Path::new("abcd.jpg").to_path_buf(),
        ] {
            assert_eq!(per_android(&storto), None, "{storto:?}");
        }
    }

    #[test]
    fn si_scrive_una_volta_e_poi_si_riusa() {
        let cartella = tempfile::tempdir().unwrap();
        let cache = cartella.path().join("copertine-lontane");
        let file = scrivi_se_manca(&cache, &G1, b"\xFF\xD8prima").unwrap();
        assert_eq!(file, percorso(&cache, &G1));
        assert_eq!(std::fs::read(&file).unwrap(), b"\xFF\xD8prima");
        // Lo stesso gettone: il file c'è, e non si riscrive (stessi byte, e
        // intanto un lettore può averlo aperto).
        let dopo = scrivi_se_manca(&cache, &G1, b"\xFF\xD8altri").unwrap();
        assert_eq!(dopo, file);
        assert_eq!(std::fs::read(&file).unwrap(), b"\xFF\xD8prima");
        // Nessun file provvisorio rimasto.
        let rimasti: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|voce| voce.unwrap().file_name())
            .collect();
        assert_eq!(rimasti.len(), 1, "{rimasti:?}");
        // Un altro gettone, un altro file.
        let altro = scrivi_se_manca(&cache, &G2, b"\xFF\xD8due").unwrap();
        assert_ne!(altro, file);
        assert_eq!(std::fs::read(&altro).unwrap(), b"\xFF\xD8due");
    }

    /// Alla fine della sessione se ne va la cartella riservata, e le copertine
    /// dei dispositivi collegati, accanto, restano dov'erano.
    #[test]
    fn svuotare_toglie_solo_le_copertine_di_sessione() {
        let cartella = tempfile::tempdir().unwrap();
        let cache = cartella.path().join("copertine-lontane");
        let dispositivo = cache.join("a".repeat(64));
        std::fs::create_dir_all(&dispositivo).unwrap();
        std::fs::write(dispositivo.join("0123456789abcdef.jpg"), b"\xFF\xD8dev").unwrap();
        std::fs::write(cache.join("vecchia.jpg"), b"\xFF\xD8vecchia").unwrap();
        let uno = scrivi_se_manca(&cache, &G1, b"\xFF\xD8uno").unwrap();
        let due = scrivi_se_manca(&cache, &G2, b"\xFF\xD8due").unwrap();
        svuota(&cache);
        assert!(!uno.exists() && !due.exists());
        assert!(!cache.join(CARTELLA).exists());
        assert!(dispositivo.join("0123456789abcdef.jpg").is_file());
        assert!(cache.join("vecchia.jpg").is_file());
        // Una cartella che non c'è non è un guasto: la sessione può finire
        // senza che nessuna copertina sia mai arrivata.
        svuota(&cache);
        svuota(&cartella.path().join("non-c-e"));
        // E si può ricominciare.
        let ancora = scrivi_se_manca(&cache, &G1, b"\xFF\xD8uno").unwrap();
        assert!(ancora.is_file());
    }
}
