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
//! Senza perdita (flac, alac, wav, aiff, ape, wavpack) prima. Fra due copie
//! senza perdita conta la frequenza di campionamento, poi il bitrate. Fra due
//! copie **con perdita** conta solo il bitrate: la frequenza di campionamento
//! non dice quanto suona bene un MP3 o un AAC — una registrazione a 48 kHz e
//! 128 kbps non batte un MP3 a 44,1 kHz e 320 kbps. La profondità di bit conta
//! quando è nota, e `tracks` non ha una colonna per lei: non conta. A parità
//! vince la copia di **questo** dispositivo, poi — per non decidere a caso — la
//! chiave del dispositivo più bassa.
//!
//! # Quel che un pari dichiara non è un fatto
//!
//! Codec, frequenza e bitrate di una riga specchio li scrive il proprietario, e
//! chi dichiara «flac a 384 kHz» vince su ogni copia di qui. Un controllo di
//! plausibilità li confronta con quel che la riga dice anche della dimensione
//! del file e della durata ([`Qualita::da_riga`]): il bitrate medio **reale** è
//! `dimensione × 8 / durata`. Una copia «senza perdita» che pesa meno di quel
//! che pesa, per la sua frequenza, un file senza perdita, non è senza perdita,
//! e un bitrate dichiarato molto più alto di quel che il file pesa si riporta a
//! quel che pesa. Senza dimensione o durata non c'è niente da confrontare, e si
//! crede alla dichiarazione. Non è una prova — chi mente su tutti e tre in modo
//! coerente non si scopre — ma chiude la bugia più economica, senza
//! impostazioni nuove.
//!
//! Il controllo non deve declassare un file vero, e per questo la soglia non è un
//! numero solo. Un WAV o un AIFF, PCM non compresso, pesa quanto la sua frequenza
//! (un messaggio vocale a 8 bit, mono, a 22,05 kHz: 176 kbps); un FLAC o un ALAC
//! di parlato mono a 22,05 kHz pesa 80–150 kbps. Per il PCM la soglia è la metà
//! di quel che pesa il PCM più leggero (quattro bit per campione, un ADPCM), per
//! gli altri senza perdita due bit per campione — e mai meno di
//! [`MINIMO_SENZA_PERDITA_KBPS`]: la soglia cresce con la frequenza dichiarata,
//! quindi una bugia a 384 kHz si scopre anche pesando 700 kbps, e un FLAC mono
//! di parlato non si tocca.
//!
//! # Album: i brani in comune, uno per uno
//!
//! Un album che sta su più dispositivi si scelge confrontando **i brani che i
//! dispositivi hanno in comune**, e questo confronto è la regola di ogni brano
//! qui sopra: la copia migliore, a parità quella di qui, poi la chiave più bassa.
//! Non c'è un secondo passo che prenda l'album intero da un dispositivo solo. C'era
//! — la qualità *mediana* dell'album, e a parità chi ha più brani — e in un album
//! di formati misti dava il peggio brano per brano (collaudo del 18/11, D5): il
//! FLAC di un dispositivo restava dietro un MP3 a 128 kbps dell'altro perché
//! l'altro aveva «la mediana migliore», e la copia identica di qui, che a parità
//! deve vincere, passava dietro quella di là perché là i brani erano di più. Con
//! §1 — «qualità migliore, sempre» e «a parità vince la locale» — la regola
//! dell'album intero non si tiene in piedi.
//!
//! L'album resta **una voce sola** per un'altra strada: la chiave d'album delle
//! righe specchio si allinea a quella delle righe di qui
//! (`archivio::riallinea_le_chiavi_d_album`, con la stessa [`identita_album`]),
//! così le copie dello stesso disco sono una scheda, coi brani scelti uno per uno.
//! A parità fra due dispositivi diversi da questo vince lo stesso per tutte le
//! tracce — la chiave più bassa — e un disco non si spezza per caso.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use rymembra_domain::album::UNKNOWN_ALBUM;
use rymembra_domain::keys::normalize_key;

/// Sotto questo bitrate medio reale (kbps) una copia senza perdita compressa
/// (FLAC, ALAC, APE, WavPack) non è creduta, qualunque frequenza dichiari: il
/// parlato mono a 16 kHz in FLAC pesa 50–80, e sotto i 64 non c'è niente che sia
/// davvero senza perdita e non una bugia.
pub const MINIMO_SENZA_PERDITA_KBPS: i64 = 64;

/// Bit per campione sotto i quali un file senza perdita compresso non è
/// credibile: il parlato mono di un audiolibro, silenzi compresi, sta intorno ai
/// quattro, due lasciano il margine per quel che ha più silenzio che voce.
const BIT_PER_CAMPIONE_COMPRESSO: i64 = 2;

/// Lo stesso per il PCM non compresso (WAV, AIFF): il più leggero pesa otto bit
/// per campione, e un ADPCM quattro.
const BIT_PER_CAMPIONE_PCM: i64 = 4;

/// Il peso medio sotto cui un file che dichiara di essere senza perdita non è
/// creduto, per la frequenza di campionamento che dichiara. Vedi il `//!`.
fn minimo_plausibile_kbps(pcm: bool, campionamento: i64) -> i64 {
    if pcm {
        campionamento
            .saturating_mul(BIT_PER_CAMPIONE_PCM)
            .div_euclid(1000)
    } else {
        campionamento
            .saturating_mul(BIT_PER_CAMPIONE_COMPRESSO)
            .div_euclid(1000)
            .max(MINIMO_SENZA_PERDITA_KBPS)
    }
}

/// Quanto un bitrate dichiarato può superare quel che il file pesa davvero,
/// prima di non essere creduto: un quarto, più sedici kbps (il peso di un
/// po' di tag e di una copertina, e dei file brevi).
const TOLLERANZA_BITRATE_KBPS: i64 = 16;

/// Quanto è buona una copia: il confronto è in quest'ordine, e `Ord` lo fa da
/// solo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Qualita {
    /// Senza perdita.
    pub senza_perdita: bool,
    /// Frequenza di campionamento, Hz; 0 se non si sa **o se la copia ha
    /// perdita**: per un codec con perdita non dice quanto suona bene.
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
            // Con perdita la frequenza non conta: vedi il `//!`.
            campionamento: if senza_perdita {
                campionamento.unwrap_or(0).max(0)
            } else {
                0
            },
            bitrate,
        }
    }

    /// Come [`Qualita::da_colonne`], con il controllo di plausibilità contro la
    /// dimensione del file e la durata: vedi il `//!`.
    ///
    /// «Senza perdita» si declassa solo se il file pesa meno di quel che può pesare
    /// uno senza perdita di quella frequenza ([`minimo_plausibile_kbps`]): un WAV a
    /// otto bit o un FLAC di parlato non sono bugie.
    ///
    /// Con dimensione o durata mancanti, o non positive, vale la dichiarazione.
    /// Un brano d'immagine CD — dove la dimensione è quella dell'intero disco e
    /// la durata quella del pezzo — risulta più pesante di quel che è, e il
    /// controllo, che si muove solo verso il basso, non lo tocca.
    #[must_use]
    pub fn da_riga(
        codec: Option<&str>,
        campionamento: Option<i64>,
        bitrate: Option<i64>,
        dimensione: Option<i64>,
        durata_ms: Option<i64>,
    ) -> Self {
        let dichiarata = Self::da_colonne(codec, campionamento, bitrate);
        let reale = match (dimensione, durata_ms) {
            (Some(byte), Some(ms)) if byte > 0 && ms > 0 => byte.saturating_mul(8).checked_div(ms),
            _ => None,
        };
        let Some(reale) = reale else {
            return dichiarata;
        };
        let pcm = matches!(
            codec
                .map(|c| c.trim().to_ascii_lowercase())
                .unwrap_or_default()
                .as_str(),
            "wav" | "wave" | "aiff" | "aif" | "pcm"
        );
        if dichiarata.senza_perdita && reale < minimo_plausibile_kbps(pcm, dichiarata.campionamento)
        {
            // «Senza perdita» che pesa come un MP3: si conta per quel che pesa.
            return Self {
                senza_perdita: false,
                campionamento: 0,
                bitrate: reale,
            };
        }
        if dichiarata.bitrate
            > reale
                .saturating_add(reale.div_euclid(4))
                .saturating_add(TOLLERANZA_BITRATE_KBPS)
        {
            return Self {
                bitrate: reale,
                ..dichiarata
            };
        }
        dichiarata
    }
}

/// Una riga per la scelta: quel che serve a sapere di che gruppo è, da dove
/// viene e quanto vale.
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

    let mut decisi: HashMap<i64, Option<i64>> = HashMap::new();
    for membri in per_gruppo.values() {
        // Il rappresentante è la riga migliore del gruppo — qualità, poi questo
        // dispositivo, poi la chiave più bassa — e il dispositivo che vince è il
        // suo: le righe degli altri dispositivi gli stanno dietro.
        let Some(rappresentante) = la_migliore(membri.iter().copied()) else {
            continue;
        };
        let vince = rappresentante.dispositivo.as_deref();
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

    fn riga(id: i64, gruppo: &str, dispositivo: Option<&str>, qualita: Qualita) -> RigaDoppione {
        RigaDoppione {
            id,
            gruppo: gruppo.to_owned(),
            dispositivo: dispositivo.map(str::to_owned),
            qualita,
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
            riga(1, "a|b|c", None, mp3(320)),
            riga(2, "a|b|c", Some("pc"), flac(44_100)),
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
            riga(1, "k", None, flac(44_100)),
            riga(2, "k", Some("pc"), flac(96_000)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(2), "96 kHz batte 44,1");

        let mut righe = vec![
            riga(1, "k", None, mp3(192)),
            riga(2, "k", Some("pc"), mp3(320)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(2), "320 batte 192");
    }

    #[test]
    fn a_parita_vince_la_locale_e_poi_la_chiave_piu_bassa() {
        let mut righe = vec![
            riga(1, "k", Some("bb"), mp3(320)),
            riga(2, "k", None, mp3(320)),
            riga(3, "k", Some("aa"), mp3(320)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 2), None, "la locale a parità");
        assert_eq!(copia_di(&righe, 1), Some(2));
        assert_eq!(copia_di(&righe, 3), Some(2));
        // Senza la locale, fra due remote, la chiave più bassa.
        let mut righe = vec![
            riga(1, "k", Some("bb"), mp3(320)),
            riga(3, "k", Some("aa"), mp3(320)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 3), None);
        assert_eq!(copia_di(&righe, 1), Some(3));
    }

    #[test]
    fn due_copie_dello_stesso_dispositivo_restano_due_voci() {
        // Il doppione locale non è affare della libreria unica.
        let mut righe = vec![
            riga(1, "k", None, mp3(128)),
            riga(2, "k", None, flac(44_100)),
            riga(3, "altro", Some("pc"), mp3(320)),
        ];
        rifai(&mut righe);
        assert!(righe.iter().all(|r| r.copia_di.is_none()), "{righe:?}");
    }

    #[test]
    fn i_brani_che_il_vincitore_non_ha_si_scelgono_da_soli() {
        // Il remoto ha il flac della traccia 1 ma non la traccia 3, che c'è
        // solo di qui: non è un doppione, è una voce sola e resta visibile.
        let mut righe = vec![
            riga(1, "t1", None, mp3(320)),
            riga(3, "t3", None, mp3(320)),
            riga(11, "t1", Some("pc"), flac(44_100)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(11));
        assert_eq!(copia_di(&righe, 3), None);
    }

    /// L'album «Formati» del collaudo del 18/11 (D5), nei due database.
    ///
    /// A ha sei brani, uno in FLAC e cinque con perdita, quattro dei quali a
    /// 64 kbps; B ne ha due: un MP3 a 128 kbps ricavato dal FLAC di A e un MP3 a
    /// 192 kbps identico a quello di A. Per §1 vale il brano, non l'album: il
    /// FLAC batte l'MP3 a 128 («qualità migliore, sempre») e fra due copie uguali
    /// vince quella di qui («a parità vince la locale»), qualunque sia il
    /// dispositivo da cui si guarda.
    fn il_disco_formati(a_e_di_qui: bool) -> (Vec<RigaDoppione>, i64, i64, i64, i64) {
        // Le righe di A (id 1-6, o 101-106 se A sta in rete) e di B (11, 12 /
        // 111, 112): quello che non è di qui ha la chiave del suo dispositivo.
        let a = |id: i64| if a_e_di_qui { id } else { 100 + id };
        let b = |id: i64| if a_e_di_qui { 10 + id } else { 110 + id };
        let (disp_a, disp_b) = if a_e_di_qui {
            (None, Some("b"))
        } else {
            (Some("a"), None)
        };
        let mut righe = vec![
            riga(a(1), "t1", disp_a, flac(44_100)),
            riga(a(2), "t2", disp_a, mp3(192)),
        ];
        for n in 3..=6 {
            righe.push(riga(a(n), &format!("t{n}"), disp_a, mp3(64)));
        }
        righe.push(riga(b(1), "t1", disp_b, mp3(128)));
        righe.push(riga(b(2), "t2", disp_b, mp3(192)));
        (righe, a(1), a(2), b(1), b(2))
    }

    #[test]
    fn nel_disco_formati_visto_da_a_il_flac_di_a_batte_l_mp3_di_b_e_la_copia_identica_resta_di_a() {
        // A è «questo» dispositivo (`None`), B è il telefono.
        let (mut righe, flac_a, cbr_a, mp3_b, cbr_b) = il_disco_formati(true);
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, flac_a), None, "il FLAC di A è visibile");
        assert_eq!(
            copia_di(&righe, mp3_b),
            Some(flac_a),
            "l'MP3 a 128 di B sta dietro il FLAC"
        );
        assert_eq!(copia_di(&righe, cbr_a), None, "a parità, la copia di qui");
        assert_eq!(copia_di(&righe, cbr_b), Some(cbr_a));
        for n in 3..=6 {
            assert_eq!(copia_di(&righe, n), None, "i brani senza doppione restano");
        }
    }

    #[test]
    fn nel_disco_formati_visto_da_b_il_flac_di_a_batte_l_mp3_di_b_e_la_copia_identica_resta_di_b() {
        // Stesso disco, dall'altra parte: qui «questo» dispositivo è B, e A è
        // l'altro.
        let (mut righe, flac_a, cbr_a, mp3_b, cbr_b) = il_disco_formati(false);
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, flac_a), None, "il FLAC di A è visibile");
        assert_eq!(
            copia_di(&righe, mp3_b),
            Some(flac_a),
            "l'MP3 a 128, di qui, sta dietro il FLAC di là"
        );
        assert_eq!(copia_di(&righe, cbr_b), None, "a parità, la copia di qui");
        assert_eq!(copia_di(&righe, cbr_a), Some(cbr_b));
        for n in 3..=6 {
            assert_eq!(copia_di(&righe, 100 + n), None);
        }
    }

    #[test]
    fn a_parita_di_qualita_vince_la_copia_di_qui_anche_se_l_altro_ha_piu_brani() {
        let mut righe = vec![
            riga(1, "t1", None, mp3(320)),
            riga(2, "t2", None, mp3(320)),
            riga(3, "t3", None, mp3(320)),
            riga(11, "t1", Some("pc"), mp3(320)),
            riga(12, "t2", Some("pc"), mp3(320)),
            riga(13, "t3", Some("pc"), mp3(320)),
            riga(14, "t4", Some("pc"), mp3(320)),
        ];
        rifai(&mut righe);
        for (locale, remota) in [(1, 11), (2, 12), (3, 13)] {
            assert_eq!(
                copia_di(&righe, locale),
                None,
                "traccia {locale}: la locale"
            );
            assert_eq!(copia_di(&righe, remota), Some(locale));
        }
        assert_eq!(
            copia_di(&righe, 14),
            None,
            "il quarto brano non ha doppioni"
        );
    }

    #[test]
    fn un_album_di_formati_misti_non_perde_qualita_su_nessun_brano_in_comune() {
        // Locale: tre MP3 a 320. Remoto: due FLAC e un MP3 a 128 (la traccia 3).
        // Le tracce 1 e 2 vanno al remoto, la 3 resta di qui: l'album è una voce
        // sola per la chiave d'album allineata, non perché si prenda tutto da un
        // dispositivo.
        let mut righe = vec![
            riga(1, "t1", None, mp3(320)),
            riga(2, "t2", None, mp3(320)),
            riga(3, "t3", None, mp3(320)),
            riga(11, "t1", Some("pc"), flac(44_100)),
            riga(12, "t2", Some("pc"), flac(44_100)),
            riga(13, "t3", Some("pc"), mp3(128)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(11));
        assert_eq!(copia_di(&righe, 2), Some(12));
        assert_eq!(copia_di(&righe, 3), None, "320 batte 128");
        assert_eq!(copia_di(&righe, 13), Some(3));
    }

    #[test]
    fn fra_due_dispositivi_che_non_sono_di_qui_a_parita_vince_la_chiave_piu_bassa_per_ogni_brano() {
        // Nessuna copia di qui: lo stesso dispositivo vince tutte le tracce pari,
        // e il disco non si spezza per caso.
        let mut righe = Vec::new();
        for n in 1..=3 {
            righe.push(riga(10 + n, &format!("t{n}"), Some("bb"), mp3(320)));
            righe.push(riga(20 + n, &format!("t{n}"), Some("aa"), mp3(320)));
        }
        rifai(&mut righe);
        for n in 1..=3 {
            assert_eq!(copia_di(&righe, 20 + n), None, "«aa» vince la traccia {n}");
            assert_eq!(copia_di(&righe, 10 + n), Some(20 + n));
        }
    }

    #[test]
    fn la_scelta_si_corregge_quando_cambia_la_realta() {
        // Il PC aveva la copia migliore, poi la sua riga se ne va: la locale
        // torna visibile. E se arriva un flac sul telefono, la scelta si rifà.
        let mut righe = vec![
            riga(1, "k", None, mp3(320)),
            riga(2, "k", Some("pc"), flac(44_100)),
        ];
        rifai(&mut righe);
        assert_eq!(copia_di(&righe, 1), Some(2));
        righe.retain(|r| r.id != 2);
        let cambi = scegli(&righe);
        assert_eq!(cambi, vec![(1, None)]);
        applica(&mut righe, &cambi);
        righe.push(riga(3, "k", None, flac(96_000)));
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

    /// Un file di `minuti` minuti che pesa `kbps` kbps di media: dimensione e
    /// durata come le scriverebbe un proprietario onesto.
    fn peso(kbps: i64, minuti: i64) -> (Option<i64>, Option<i64>) {
        let ms = minuti * 60_000;
        (Some((kbps * ms).div_euclid(8)), Some(ms))
    }

    #[test]
    fn un_flac_a_384_khz_che_pesa_come_un_mp3_non_e_senza_perdita() {
        // La bugia più economica: dichiarare «flac 384 kHz» per vincere su ogni
        // copia di qui. Il file pesa 128 kbps in media, e un FLAC di musica non
        // pesa così poco.
        let (dimensione, durata) = peso(128, 4);
        let bugia = Qualita::da_riga(Some("flac"), Some(384_000), Some(2_000), dimensione, durata);
        assert!(!bugia.senza_perdita, "{bugia:?}");
        assert_eq!(bugia.bitrate, 128, "si conta per quel che pesa");
        assert!(
            mp3(320) > bugia,
            "una copia di qui a 320 kbps batte la bugia: {bugia:?}"
        );
        // Un FLAC vero, che pesa quel che pesa un FLAC, resta quel che dice.
        let (dimensione, durata) = peso(900, 4);
        let vero = Qualita::da_riga(Some("flac"), Some(96_000), None, dimensione, durata);
        assert!(vero.senza_perdita);
        assert_eq!(vero.campionamento, 96_000);
        assert!(vero > mp3(320));
    }

    #[test]
    fn la_soglia_cresce_con_la_frequenza_e_non_declassa_un_file_vero() {
        // I file veri che pesano poco: WAV a 8 bit mono, FLAC e ALAC di parlato.
        let (dimensione, durata) = peso(176, 3);
        assert!(
            Qualita::da_riga(Some("wav"), Some(22_050), Some(176), dimensione, durata)
                .senza_perdita
        );
        let (dimensione, durata) = peso(110, 30);
        assert!(
            Qualita::da_riga(Some("flac"), Some(22_050), Some(110), dimensione, durata)
                .senza_perdita
        );
        let (dimensione, durata) = peso(190, 5);
        assert!(
            Qualita::da_riga(Some("alac"), Some(44_100), Some(190), dimensione, durata)
                .senza_perdita
        );
        // Le bugie: un PCM a 384 kHz che pesa 128 kbps, un FLAC a 44,1 kHz che pesa
        // 60, un FLAC senza frequenza che pesa quanto un MP3 da 32.
        let (dimensione, durata) = peso(128, 4);
        let wav = Qualita::da_riga(Some("wav"), Some(384_000), Some(128), dimensione, durata);
        assert!(!wav.senza_perdita, "{wav:?}");
        let (dimensione, durata) = peso(60, 4);
        let flac = Qualita::da_riga(Some("flac"), Some(44_100), Some(60), dimensione, durata);
        assert!(!flac.senza_perdita, "{flac:?}");
        let (dimensione, durata) = peso(32, 4);
        let senza = Qualita::da_riga(Some("flac"), None, Some(32), dimensione, durata);
        assert!(!senza.senza_perdita, "{senza:?}");
        // Un WAV a 96 kHz che pesa 700 kbps è sopra la metà di quel che pesa il
        // PCM più leggero a quella frequenza (384 kbps): resta com'è.
        let (dimensione, durata) = peso(700, 2);
        let alto = Qualita::da_riga(Some("wav"), Some(96_000), Some(700), dimensione, durata);
        assert!(alto.senza_perdita, "{alto:?}");
    }

    #[test]
    fn un_bitrate_dichiarato_sopra_quel_che_il_file_pesa_si_riporta_a_quel_che_pesa() {
        let (dimensione, durata) = peso(128, 4);
        let dichiarato = Qualita::da_riga(Some("mp3"), Some(44_100), Some(320), dimensione, durata);
        assert_eq!(dichiarato.bitrate, 128);
        // Dentro la tolleranza (tag, copertina) si crede alla dichiarazione.
        let (dimensione, durata) = peso(300, 4);
        let onesto = Qualita::da_riga(Some("mp3"), Some(44_100), Some(320), dimensione, durata);
        assert_eq!(onesto.bitrate, 320);
    }

    #[test]
    fn senza_dimensione_o_durata_si_crede_alla_dichiarazione() {
        for (dimensione, durata) in [
            (None, Some(240_000)),
            (Some(5_000_000), None),
            (Some(0), Some(240_000)),
            (Some(5_000_000), Some(0)),
        ] {
            let q = Qualita::da_riga(Some("flac"), Some(96_000), Some(1_000), dimensione, durata);
            assert_eq!(
                q,
                Qualita::da_colonne(Some("flac"), Some(96_000), Some(1_000))
            );
        }
    }

    #[test]
    fn un_pezzo_di_un_disco_d_immagine_non_si_declassa() {
        // La dimensione è quella dell'immagine intera, la durata quella del
        // pezzo: il bitrate risulta più alto del vero, e il controllo — che si
        // muove solo verso il basso — lo lascia stare.
        let q = Qualita::da_riga(
            Some("flac"),
            Some(44_100),
            Some(900),
            Some(300_000_000),
            Some(240_000),
        );
        assert!(q.senza_perdita);
        assert_eq!(q.bitrate, 900);
    }

    #[test]
    fn fra_due_copie_con_perdita_conta_il_bitrate_e_la_frequenza_no() {
        let aac_48 = Qualita::da_colonne(Some("aac"), Some(48_000), Some(128));
        assert_eq!(aac_48.campionamento, 0);
        assert!(mp3(320) > aac_48);
        // Senza perdita la frequenza conta ancora.
        assert!(flac(96_000) > flac(44_100));
    }
}
