//! Doppioni: lo stesso brano su più dispositivi, una voce sola.
//!
//! # La regola
//!
//! Due righe sono lo stesso brano se hanno la stessa chiave d'identità,
//! `COALESCE(content_key, track_key)` (per una riga specchio la chiave di
//! contenuto è quella del proprietario, in `lontano_brani`). Un gruppo che sta
//! su **un solo** dispositivo non si tocca: due copie locali dello stesso brano
//! restano due righe, come oggi — è il lavoro dello strumento dei doppioni, non
//! di questa libreria unica. Un gruppo con righe di **dispositivi diversi**
//! (questo e un altro, o due altri) ha un dispositivo che vince, e le righe
//! degli altri dispositivi stanno dietro il suo rappresentante: `copia_di` dice
//! quale. `copia_di IS NULL` vuol dire «visibile in Tutti».
//!
//! # La copia migliore
//!
//! Senza perdita (flac, alac, wav, aiff, ape, wavpack) prima; poi la frequenza
//! di campionamento; poi il bitrate. La profondità di bit conta quando è nota,
//! e `tracks` non ha una colonna per lei: non conta. A parità vince la copia di
//! **questo** dispositivo, poi — per non decidere a caso — la chiave del
//! dispositivo più bassa.
//!
//! # La coerenza d'album
//!
//! Senza questa regola un album si spezzerebbe in due voci: la traccia 1 dalla
//! copia migliore, la 2 dalla copia che a parità ha vinto per caso. Quindi
//! prima si sceglie **per album intero**: se un album (stesso titolo, stesso
//! artista, normalizzati) c'è su più dispositivi, lo si prende tutto dal
//! dispositivo la cui copia dell'album è migliore — la qualità *mediana* delle
//! sue tracce, che una traccia sola ritoccata non sposta — e a parità da quello
//! con più brani. Poi i brani che da lì non hanno una copia, e quelli senza
//! album, si scelgono uno per uno.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use rymembra_domain::album::UNKNOWN_ALBUM;
use rymembra_domain::keys::normalize_key;

/// Quanto è buona una copia: il confronto è in quest'ordine, e `Ord` lo fa da
/// solo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Qualita {
    /// Senza perdita.
    pub senza_perdita: bool,
    /// Frequenza di campionamento, Hz; 0 se non si sa.
    pub campionamento: i64,
    /// Bitrate, kbps; 0 se non si sa.
    pub bitrate: i64,
}

impl Qualita {
    /// La qualità di una riga, dalle colonne che `tracks` ha.
    ///
    /// Il contenitore Mp4 può stare per AAC o per ALAC e la colonna non lo
    /// distingue: con un bitrate da senza perdita (oltre i 600 kbps, che un AAC
    /// non raggiunge) si prende per tale.
    #[must_use]
    pub fn da_colonne(
        codec: Option<&str>,
        campionamento: Option<i64>,
        bitrate: Option<i64>,
    ) -> Self {
        let codec = codec
            .map(|c| c.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let bitrate = bitrate.unwrap_or(0).max(0);
        let senza_perdita = matches!(
            codec.as_str(),
            "flac" | "alac" | "wav" | "wave" | "aiff" | "aif" | "ape" | "wavpack" | "pcm"
        ) || (matches!(codec.as_str(), "mp4" | "m4a") && bitrate >= 600);
        Self {
            senza_perdita,
            campionamento: campionamento.unwrap_or(0).max(0),
            bitrate,
        }
    }
}

/// Una riga per la scelta: quel che serve a sapere di che gruppo e di che
/// album è, da dove viene e quanto vale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigaDoppione {
    /// La riga di `tracks`.
    pub id: i64,
    /// La chiave d'identità del brano.
    pub gruppo: String,
    /// La chiave del dispositivo da cui viene, o `None` se è di questo.
    pub dispositivo: Option<String>,
    /// Quanto è buona.
    pub qualita: Qualita,
    /// L'identità dell'album fra i dispositivi, se è un album con un nome.
    pub album: Option<String>,
    /// Il valore di `copia_di` di adesso.
    pub copia_di: Option<i64>,
}

/// L'identità di un album fra dispositivi: titolo e artista normalizzati, o
/// `None` per un brano senza album — il segnaposto della scansione non è un
/// titolo.
#[must_use]
pub fn identita_album(album: &str, album_artist: Option<&str>, artist: &str) -> Option<String> {
    let titolo = normalize_key(Some(album));
    if titolo.is_empty() || titolo == normalize_key(Some(UNKNOWN_ALBUM)) {
        return None;
    }
    let artista = normalize_key(Some(
        album_artist
            .filter(|a| !a.trim().is_empty())
            .unwrap_or(artist),
    ));
    Some(format!("{titolo}\u{1f}{artista}"))
}

/// Chi vince a parità: **questo** dispositivo, poi la chiave più bassa.
fn rango(dispositivo: Option<&str>) -> (bool, Reverse<&str>) {
    (dispositivo.is_none(), Reverse(dispositivo.unwrap_or("")))
}

/// La riga migliore fra quelle date.
fn la_migliore<'a>(righe: impl Iterator<Item = &'a RigaDoppione>) -> Option<&'a RigaDoppione> {
    righe.max_by_key(|r| (r.qualita, rango(r.dispositivo.as_deref()), Reverse(r.id)))
}

/// La qualità *mediana* (la bassa, se sono pari) di un insieme di righe.
fn mediana(qualita: &[Qualita]) -> Qualita {
    let mut ordinate = qualita.to_vec();
    ordinate.sort_unstable();
    ordinate
        .get(ordinate.len().saturating_sub(1).div_euclid(2))
        .copied()
        .unwrap_or_default()
}

/// Per ogni riga, la riga che la rappresenta: `None` se è visibile in «Tutti».
///
/// Puro: vedi il `//!` per la regola. Restituisce solo le righe il cui
/// `copia_di` **cambia** rispetto a quello che avevano, come
/// `(id, nuovo valore)`: applicate, una seconda chiamata non cambia niente.
#[must_use]
pub fn scegli(righe: &[RigaDoppione]) -> Vec<(i64, Option<i64>)> {
    // I gruppi che stanno su più dispositivi: gli altri non si toccano.
    let mut per_gruppo: BTreeMap<&str, Vec<&RigaDoppione>> = BTreeMap::new();
    for riga in righe {
        per_gruppo.entry(&riga.gruppo).or_default().push(riga);
    }
    per_gruppo.retain(|_, membri| {
        let primo = membri.first().and_then(|r| r.dispositivo.as_deref());
        membri.iter().any(|r| r.dispositivo.as_deref() != primo)
    });
    if per_gruppo.is_empty() {
        // Niente doppioni fra dispositivi: ogni riga è visibile.
        return righe
            .iter()
            .filter(|r| r.copia_di.is_some())
            .map(|r| (r.id, None))
            .collect();
    }

    // La coerenza d'album: com'è ogni album su ogni dispositivo, con **tutte**
    // le sue righe — anche quelle che non hanno doppioni — perché «più brani»
    // conti il disco com'è.
    let mut istanze: BTreeMap<&str, BTreeMap<Option<&str>, Vec<Qualita>>> = BTreeMap::new();
    for riga in righe {
        if let Some(album) = riga.album.as_deref() {
            istanze
                .entry(album)
                .or_default()
                .entry(riga.dispositivo.as_deref())
                .or_default()
                .push(riga.qualita);
        }
    }
    // Il dispositivo che vince ogni album che sta su più d'uno.
    let mut vincitore_dell_album: HashMap<&str, Option<&str>> = HashMap::new();
    for (album, per_dispositivo) in &istanze {
        if per_dispositivo.len() < 2 {
            continue;
        }
        let vince = per_dispositivo
            .iter()
            .max_by_key(|(dispositivo, qualita)| {
                (mediana(qualita), qualita.len(), rango(**dispositivo))
            })
            .map(|(dispositivo, _)| *dispositivo);
        if let Some(vince) = vince {
            vincitore_dell_album.insert(album, vince);
        }
    }

    let mut decisi: HashMap<i64, Option<i64>> = HashMap::new();
    for membri in per_gruppo.values() {
        // Il dispositivo che vince questo gruppo: quello di un album che ha il
        // suo vincitore e una riga anche qui; se no, la riga migliore di tutte.
        let dall_album = membri
            .iter()
            .filter_map(|r| r.album.as_deref())
            .filter_map(|album| vincitore_dell_album.get(album).copied())
            .filter(|vince| membri.iter().any(|r| r.dispositivo.as_deref() == *vince))
            .max_by_key(|vince| rango(*vince));
        let vince: Option<&str> = match dall_album {
            Some(vince) => vince,
            None => match la_migliore(membri.iter().copied()) {
                Some(r) => r.dispositivo.as_deref(),
                None => continue,
            },
        };
        // Il rappresentante: la migliore delle righe del dispositivo che vince.
        let Some(rappresentante) = la_migliore(
            membri
                .iter()
                .copied()
                .filter(|r| r.dispositivo.as_deref() == vince),
        ) else {
            continue;
        };
        for riga in membri {
            let nuovo = if riga.dispositivo.as_deref() == vince {
                None
            } else {
                Some(rappresentante.id)
            };
            decisi.insert(riga.id, nuovo);
        }
    }
    righe
        .iter()
        .filter_map(|riga| {
            // Fuori dai gruppi su più dispositivi non c'è copia: visibile.
            let nuovo = decisi.get(&riga.id).copied().unwrap_or(None);
            (nuovo != riga.copia_di).then_some((riga.id, nuovo))
        })
        .collect()
}

#[cfg(test)]
mod prove {
    use super::*;

    fn mp3(bitrate: i64) -> Qualita {
        Qualita::da_colonne(Some("Mpeg"), Some(44_100), Some(bitrate))
    }

    fn flac(campionamento: i64) -> Qualita {
        Qualita::da_colonne(Some("Flac"), Some(campionamento), None)
    }

    fn riga(
        id: i64,
        gruppo: &str,
        dispositivo: Option<&str>,
        qualita: Qualita,
        album: Option<&str>,
    ) -> RigaDoppione {
        RigaDoppione {
            id,
            gruppo: gruppo.to_owned(),
            dispositivo: dispositivo.map(str::to_owned),
            qualita,
            album: album.map(str::to_owned),
            copia_di: None,
        }
    }

    /// Applica i cambi alle righe, come farebbe il database.
    fn applica(righe: &mut [RigaDoppione], cambi: &[(i64, Option<i64>)]) {
        for (id, nuovo) in cambi {
            if let Some(r) = righe.iter_mut().find(|r| r.id == *id) {
                r.copia_di = *nuovo;
            }
        }
    }

    /// Rifà la scelta e la applica.
    fn rifai(righe: &mut [RigaDoppione]) {
        let cambi = scegli(righe);
        applica(righe, &cambi);
    }

    fn copia_di(righe: &[RigaDoppione], id: i64) -> Option<i64> {
        righe.iter().find(|r| r.id == id).and_then(|r| r.copia_di)
    }

    #[test]
    fn senza_perdita_batte_con_perdita_anche_se_remota() {
        let mut righe = vec![
            riga(1, "a|b|c", None, mp3(320), None),
            riga(2, "a|b|c", Some("pc"), flac(44_100), None),
        ];
        let cambi = scegli(&righe);
        applica(&mut righe, &cambi);
        assert_eq!(
            copia_di(&righe, 1),
            Some(2),
            "la locale sta dietro la remota"
        );
        assert_eq!(copia_di(&righe, 2), None, "la migliore è visibile");
        assert!(
            scegli(&righe).is_empty(),
            "una seconda volta non cambia niente"
        );
    }

    #[test]
    fn campionamento_poi_bitrate_decidono_fra_copie_dello_stesso_genere() {
        let mut righe = vec![
            riga(1, "k", None, flac(44_100), None),
            riga(2, "k", Some("pc"), flac(96_000), None),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(2), "96 kHz batte 44,1");

        let mut righe = vec![
            riga(1, "k", None, mp3(192), None),
            riga(2, "k", Some("pc"), mp3(320), None),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(2), "320 batte 192");
    }

    #[test]
    fn a_parita_vince_la_locale_e_poi_la_chiave_piu_bassa() {
        let mut righe = vec![
            riga(1, "k", Some("bb"), mp3(320), None),
            riga(2, "k", None, mp3(320), None),
            riga(3, "k", Some("aa"), mp3(320), None),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 2), None, "la locale a parità");
        assert_eq!(copia_di(&righe, 1), Some(2));
        assert_eq!(copia_di(&righe, 3), Some(2));
        // Senza la locale, fra due remote, la chiave più bassa.
        let mut righe = vec![
            riga(1, "k", Some("bb"), mp3(320), None),
            riga(3, "k", Some("aa"), mp3(320), None),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 3), None);
        assert_eq!(copia_di(&righe, 1), Some(3));
    }

    #[test]
    fn due_copie_dello_stesso_dispositivo_restano_due_voci() {
        // Il doppione locale non è affare della libreria unica.
        let mut righe = vec![
            riga(1, "k", None, mp3(128), None),
            riga(2, "k", None, flac(44_100), None),
            riga(3, "altro", Some("pc"), mp3(320), None),
        ];
        rifai(&mut righe);
        assert!(righe.iter().all(|r| r.copia_di.is_none()), "{righe:?}");
    }

    #[test]
    fn un_album_si_prende_tutto_dalla_copia_migliore() {
        // L'album A su due dispositivi. Locale: tre mp3 da 320. Remoto: due
        // flac e un mp3 da 128 (la traccia 3): per traccia, la 3 resterebbe
        // locale e l'album si spezzerebbe. La mediana del remoto è flac, e
        // l'album va tutto dal remoto.
        let a = Some("album a");
        let mut righe = vec![
            riga(1, "t1", None, mp3(320), a),
            riga(2, "t2", None, mp3(320), a),
            riga(3, "t3", None, mp3(320), a),
            riga(11, "t1", Some("pc"), flac(44_100), a),
            riga(12, "t2", Some("pc"), flac(44_100), a),
            riga(13, "t3", Some("pc"), mp3(128), a),
        ];
        rifai(&mut righe);
        for (locale, remota) in [(1, 11), (2, 12), (3, 13)] {
            assert_eq!(copia_di(&righe, locale), Some(remota), "traccia {locale}");
            assert_eq!(copia_di(&righe, remota), None);
        }
        // E per traccia, senza album, la 3 sarebbe rimasta locale.
        let mut senza_album = righe.clone();
        for r in &mut senza_album {
            r.album = None;
            r.copia_di = None;
        }
        rifai(&mut senza_album);
        assert_eq!(
            copia_di(&senza_album, 13),
            Some(3),
            "il caso che la regola evita"
        );
    }

    #[test]
    fn a_parita_di_qualita_l_album_va_da_chi_ha_piu_brani() {
        let a = Some("album a");
        let mut righe = vec![
            riga(1, "t1", None, mp3(320), a),
            riga(2, "t2", None, mp3(320), a),
            riga(3, "t3", None, mp3(320), a),
            riga(11, "t1", Some("pc"), mp3(320), a),
            riga(12, "t2", Some("pc"), mp3(320), a),
            riga(13, "t3", Some("pc"), mp3(320), a),
            riga(14, "t4", Some("pc"), mp3(320), a),
        ];
        rifai(&mut righe);
        assert_eq!(
            copia_di(&righe, 1),
            Some(11),
            "il remoto ha quattro brani, la locale tre"
        );
        assert_eq!(copia_di(&righe, 11), None);
        assert_eq!(
            copia_di(&righe, 14),
            None,
            "il quarto brano non ha doppioni"
        );
    }

    #[test]
    fn i_brani_che_il_vincitore_non_ha_si_scelgono_da_soli() {
        // Il remoto vince l'album (flac) ma non ha la traccia 3, che c'è
        // locale: è una voce sola e resta visibile.
        let a = Some("album a");
        let mut righe = vec![
            riga(1, "t1", None, mp3(320), a),
            riga(3, "t3", None, mp3(320), a),
            riga(11, "t1", Some("pc"), flac(44_100), a),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(11));
        assert_eq!(copia_di(&righe, 3), None);
    }

    #[test]
    fn la_scelta_si_corregge_quando_cambia_la_realta() {
        // Il PC aveva la copia migliore, poi la sua riga se ne va: la locale
        // torna visibile. E se arriva un flac sul telefono, la scelta si rifà.
        let mut righe = vec![
            riga(1, "k", None, mp3(320), None),
            riga(2, "k", Some("pc"), flac(44_100), None),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(2));
        righe.retain(|r| r.id != 2);
        let cambi = scegli(&righe);
        assert_eq!(cambi, vec![(1, None)]);
        applica(&mut righe, &cambi);
        righe.push(riga(3, "k", None, flac(96_000), None));
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), None, "due locali restano due voci");
        assert_eq!(copia_di(&righe, 3), None);
    }

    #[test]
    fn l_identita_dell_album_non_conta_maiuscole_ne_il_segnaposto() {
        assert_eq!(
            identita_album("The Wall", None, "Pink Floyd"),
            identita_album("the wall", Some("PINK FLOYD"), "Roger Waters")
        );
        assert_eq!(identita_album(UNKNOWN_ALBUM, None, "X"), None);
        assert_eq!(identita_album("  ", None, "X"), None);
        assert_ne!(
            identita_album("Greatest Hits", None, "A"),
            identita_album("Greatest Hits", None, "B")
        );
    }

    #[test]
    fn la_qualita_riconosce_il_senza_perdita() {
        assert!(Qualita::da_colonne(Some("Flac"), None, None).senza_perdita);
        assert!(Qualita::da_colonne(Some("Wav"), None, None).senza_perdita);
        assert!(Qualita::da_colonne(Some("Mp4"), Some(44_100), Some(900)).senza_perdita);
        assert!(!Qualita::da_colonne(Some("Mp4"), Some(44_100), Some(256)).senza_perdita);
        assert!(!Qualita::da_colonne(Some("Mpeg"), None, Some(320)).senza_perdita);
        assert!(!Qualita::da_colonne(None, None, None).senza_perdita);
        assert!(flac(44_100) > mp3(320));
        assert!(flac(96_000) > flac(44_100));
        assert!(mp3(320) > mp3(192));
    }
}
