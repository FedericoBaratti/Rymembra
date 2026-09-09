//! Le date di Spotify, ridotte a millisecondi dall'epoca.
//!
//! # Perché sta nel dominio
//!
//! È nato dentro `aether-archivio`, quando l'unico posto in cui Aether leggeva
//! una data di Spotify era lo zip che Spotify manda per posta. Con la via OAuth
//! i lettori erano due — la Web API leggeva il `played_at` di
//! `/me/player/recently-played` — e due implementazioni di un lettore di date
//! divergono in silenzio: la seconda sbaglia un caso limite che la prima
//! trattava, e nessuno se ne accorge finché una cronologia non finisce datata
//! male. È puro, non tocca niente, e questa è la casa delle cose pure.
//!
//! # Perché a mano e non con una libreria di date
//!
//! Perché il problema è più piccolo di quel che una libreria di date risolve.
//! Servono due formati, entrambi già in UTC, entrambi senza fuso orario da
//! interpretare e senza aritmetica sui calendari da fare: si legge un istante e
//! lo si converte in un numero. `chrono` o `time` porterebbero dentro un
//! modello di zone, un database dei fusi e una superficie di API che nessuno
//! qui userebbe mai.
//!
//! I formati, come Spotify li scrive:
//!
//! - `2020-01-01T12:00:00Z` — la cronologia estesa dell'archivio, ISO 8601 con
//!   i secondi. È anche la forma del `played_at` della Web API, che però ci
//!   mette i millisecondi: `2020-01-01T12:00:00.123Z`, troncati ai secondi.
//! - `2023-01-01 12:00` — la cronologia breve, con lo spazio al posto della `T`
//!   e **senza secondi**. Non è una variante dello stesso: è un altro formato,
//!   e trattarli con lo stesso lettore è ciò che permette di non accorgersi che
//!   uno dei due non si legge.
//!
//! # La conversione
//!
//! `giorni_dall_epoca` è l'algoritmo `days_from_civil` di Howard Hinnant: la
//! stessa aritmetica che sta sotto `<chrono>` di C++ e sotto la maggior parte
//! delle librerie di date. Vale per ogni data dopo il 1º marzo del -32768, che
//! copre comodamente Spotify.

/// Millisecondi in un giorno.
const MS_AL_GIORNO: i64 = 86_400_000;

/// Legge un istante di Spotify, nell'uno o nell'altro formato.
///
/// `None` quando la stringa non è nessuno dei due. Non è un errore: un archivio
/// con una riga storta non deve fermare l'importazione delle altre quarantamila,
/// e chi chiama la conta fra gli scarti.
#[must_use]
pub fn istante_ms(testo: &str) -> Option<i64> {
    let testo = testo.trim();
    // La `T` dell'ISO e lo spazio della forma breve separano le stesse due
    // metà: si accettano tutti e due invece di scrivere due lettori.
    let (data, ora) = testo
        .split_once('T')
        .or_else(|| testo.split_once(' '))
        .map_or((testo, ""), |(d, o)| (d, o));

    let mut pezzi_data = data.split('-');
    let anno: i64 = pezzi_data.next()?.parse().ok()?;
    let mese: u32 = pezzi_data.next()?.parse().ok()?;
    let giorno: u32 = pezzi_data.next()?.parse().ok()?;
    if pezzi_data.next().is_some() || !(1..=12).contains(&mese) || !(1..=31).contains(&giorno) {
        return None;
    }

    // La `Z` finale si toglie e non si interpreta: entrambi i formati sono già
    // in UTC, e un archivio con un offset vero non l'ha mai prodotto nessuno.
    // Se un giorno lo producesse, questa funzione direbbe `None` invece di
    // sbagliare l'ora di qualche ora — che è la differenza fra accorgersene e no.
    let ora = ora.trim_end_matches('Z');
    let (ore, minuti, secondi) = if ora.is_empty() {
        (0, 0, 0)
    } else {
        let mut pezzi = ora.split(':');
        let ore: i64 = pezzi.next()?.parse().ok()?;
        let minuti: i64 = pezzi.next()?.parse().ok()?;
        // I secondi mancano nella cronologia breve, e la loro assenza è
        // normale — non un motivo per buttare via la riga.
        let secondi: i64 = match pezzi.next() {
            // I frazionari, se un giorno arrivassero: si tronca ai secondi.
            Some(s) => s.split('.').next()?.parse().ok()?,
            None => 0,
        };
        if pezzi.next().is_some() || !(0..24).contains(&ore) || !(0..60).contains(&minuti) {
            return None;
        }
        // 60 è ammesso: è il secondo intercalare, e rifiutarlo butterebbe via
        // una riga valida due volte l'anno.
        if !(0..=60).contains(&secondi) {
            return None;
        }
        (ore, minuti, secondi)
    };

    let giorni = giorni_dall_epoca(anno, mese, giorno);
    let del_giorno = ore * 3_600 + minuti * 60 + secondi;
    Some(
        giorni
            .saturating_mul(MS_AL_GIORNO)
            .saturating_add(del_giorno.saturating_mul(1_000)),
    )
}

/// I giorni fra il 1º gennaio 1970 e una data del calendario gregoriano.
///
/// `days_from_civil` di Howard Hinnant, trascritto. L'idea è spostare l'inizio
/// dell'anno a marzo — così il 29 febbraio finisce in fondo e non c'è nessun
/// caso speciale da scrivere — e poi contare le ere di 400 anni, che sono il
/// periodo esatto con cui il calendario gregoriano si ripete.
///
/// Non tenta di validare il giorno rispetto al mese: un «31 febbraio» dà il
/// 3 marzo invece di un errore. È il comportamento dell'algoritmo originale e
/// va benissimo qui — quella data non esce da un archivio di Spotify, e il
/// controllo che conta (1..=31) l'ha già fatto chi chiama.
// Le divisioni intere qui **sono** l'algoritmo: `/4` e `/100` contano gli anni
// bisestili di un'era, `/5` distribuisce i mesi di 30 e 31 giorni. Il troncamento
// non è una perdita di precisione da evitare, è il conto. Con i decimali il
// risultato sarebbe sbagliato, non più preciso.
#[expect(
    clippy::integer_division,
    reason = "il troncamento è l'algoritmo: /4 e /100 contano i bisestili di un'era, /5 distribuisce i mesi di 30 e 31 giorni"
)]
#[must_use]
pub(crate) fn giorni_dall_epoca(anno: i64, mese: u32, giorno: u32) -> i64 {
    let mese = i64::from(mese);
    let giorno = i64::from(giorno);
    // L'anno comincia a marzo: gennaio e febbraio appartengono a quello prima.
    let y = if mese <= 2 { anno - 1 } else { anno };
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let anno_nell_era = y - era * 400; // [0, 399]
    let mese_spostato = if mese > 2 { mese - 3 } else { mese + 9 }; // [0, 11]
    let giorno_nell_anno = (153 * mese_spostato + 2) / 5 + giorno - 1; // [0, 365]
    let giorno_nell_era =
        anno_nell_era * 365 + anno_nell_era / 4 - anno_nell_era / 100 + giorno_nell_anno; // [0, 146096]
    // 146097 giorni in un'era; 719468 è lo scarto fra l'origine dell'algoritmo
    // (1º marzo dell'anno 0) e l'epoca Unix.
    era * 146_097 + giorno_nell_era - 719_468
}

/// La data del calendario gregoriano, dai giorni contati dall'epoca.
///
/// `civil_from_days` di Howard Hinnant: l'inversa esatta di
/// [`giorni_dall_epoca`], e sta accanto a lei perché due funzioni che devono
/// annullarsi a vicenda si controllano leggendole insieme. La prova
/// `andata_e_ritorno` percorre un secolo giorno per giorno.
///
/// Restituisce `(anno, mese, giorno)`, col mese in `1..=12`.
// Le divisioni intere qui **sono** l'algoritmo, come nella funzione inversa:
// contano gli anni bisestili di un'era e ridistribuiscono i mesi di 30 e 31
// giorni. Il troncamento è il conto, non una perdita da evitare.
#[expect(
    clippy::integer_division,
    reason = "il troncamento è l'algoritmo inverso: /1460, /36524 e /365 contano gli anni di un'era, /153 e /5 i mesi di 30 e 31 giorni"
)]
#[must_use]
pub(crate) fn data_dall_epoca(giorni: i64) -> (i64, u32, u32) {
    // Si riporta l'origine al 1º marzo dell'anno 0, dove il 29 febbraio cade in
    // fondo all'anno e smette di essere un caso speciale.
    let spostato = giorni + 719_468;
    let era = if spostato >= 0 {
        spostato
    } else {
        spostato - 146_096
    }
    .div_euclid(146_097);
    let giorno_nell_era = spostato - era * 146_097; // [0, 146096]
    let anno_nell_era = (giorno_nell_era - giorno_nell_era / 1460 + giorno_nell_era / 36_524
        - giorno_nell_era / 146_096)
        / 365; // [0, 399]
    let anno = anno_nell_era + era * 400;
    let giorno_nell_anno =
        giorno_nell_era - (365 * anno_nell_era + anno_nell_era / 4 - anno_nell_era / 100); // [0, 365]
    let mese_spostato = (5 * giorno_nell_anno + 2) / 153; // [0, 11]
    let giorno = giorno_nell_anno - (153 * mese_spostato + 2) / 5 + 1; // [1, 31]
    let mese = if mese_spostato < 10 {
        mese_spostato + 3
    } else {
        mese_spostato - 9
    }; // [1, 12]
    // Gennaio e febbraio appartengono all'anno successivo a quello dell'era.
    let anno = if mese <= 2 { anno + 1 } else { anno };
    (
        anno,
        u32::try_from(mese).unwrap_or(1),
        u32::try_from(giorno).unwrap_or(1),
    )
}

/// Un istante in millisecondi, scritto come `2026-08-27 14:03:11.482Z`.
///
/// La gemella di [`istante_ms`], che fa il cammino contrario. Serve al diario
/// dell'applicazione, e la forma è scelta per chi lo leggerà: ordinabile come
/// testo, e con la `Z` in fondo perché **è UTC**. Un orario locale
/// richiederebbe il fuso del sistema operativo — cioè una dipendenza in più, o
/// dell'`unsafe` — e soprattutto renderebbe illeggibile un diario spedito da
/// un'altra parte del mondo, che è esattamente il caso in cui si legge un
/// diario.
///
/// Un istante prima dell'epoca si scrive lo stesso: `div_euclid` e `rem_euclid`
/// non cambiano verso sotto lo zero, quindi non escono ore negative.
// Le divisioni qui sono conversioni fra unità di tempo, non misure: i
// millisecondi in un secondo sono mille esatti.
#[expect(
    clippy::integer_division,
    reason = "conversioni fra unità di tempo, non misure: i millisecondi in un secondo sono mille esatti, i secondi in un minuto sessanta"
)]
#[must_use]
pub fn istante_iso(ms: i64) -> String {
    let giorni = ms.div_euclid(MS_AL_GIORNO);
    let nel_giorno = ms.rem_euclid(MS_AL_GIORNO);
    let (anno, mese, giorno) = data_dall_epoca(giorni);
    let millisecondi = nel_giorno % 1_000;
    let secondi_totali = nel_giorno / 1_000;
    let secondi = secondi_totali % 60;
    let minuti = (secondi_totali / 60) % 60;
    let ore = secondi_totali / 3_600;
    format!("{anno:04}-{mese:02}-{giorno:02} {ore:02}:{minuti:02}:{secondi:02}.{millisecondi:03}Z")
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn l_epoca_e_zero() {
        assert_eq!(giorni_dall_epoca(1970, 1, 1), 0);
        assert_eq!(istante_ms("1970-01-01T00:00:00Z"), Some(0));
    }

    #[test]
    fn i_due_formati_di_spotify_si_leggono_tutti_e_due() {
        // Esteso: ISO con i secondi e la Z.
        assert_eq!(
            istante_ms("2020-01-01T12:00:00Z"),
            Some(1_577_880_000_000),
            "cronologia estesa"
        );
        // Breve: spazio al posto della T, e senza secondi. Non è una variante:
        // è un altro formato, e leggerlo con lo stesso lettore è ciò che
        // impedisce di non accorgersi che uno dei due non si legge.
        assert_eq!(
            istante_ms("2020-01-01 12:00"),
            Some(1_577_880_000_000),
            "cronologia breve"
        );
    }

    #[test]
    fn gli_anni_bisestili_tornano() {
        // 2000 è bisestile (divisibile per 400), 1900 no (per 100 ma non 400):
        // è il caso che l'aritmetica delle ere esiste per prendere.
        assert_eq!(
            istante_ms("2000-02-29T00:00:00Z"),
            Some(951_782_400_000),
            "il 29 febbraio del 2000 esiste"
        );
        assert_eq!(
            giorni_dall_epoca(2001, 1, 1) - giorni_dall_epoca(2000, 1, 1),
            366
        );
        assert_eq!(
            giorni_dall_epoca(1901, 1, 1) - giorni_dall_epoca(1900, 1, 1),
            365,
            "il 1900 non è bisestile"
        );
    }

    #[test]
    fn una_data_prima_dell_epoca_va_indietro() {
        assert!(istante_ms("1969-12-31T23:59:59Z").is_some_and(|ms| ms == -1_000));
    }

    #[test]
    fn una_riga_storta_non_ferma_un_archivio() {
        // `None` e non un errore: una riga illeggibile fra quarantamila non è
        // una ragione per rifiutare le altre.
        assert_eq!(istante_ms(""), None);
        assert_eq!(istante_ms("ieri"), None);
        assert_eq!(istante_ms("2020-01"), None, "manca il giorno");
        assert_eq!(istante_ms("2020-13-01T00:00:00Z"), None, "mese inesistente");
        assert_eq!(
            istante_ms("2020-01-32T00:00:00Z"),
            None,
            "giorno inesistente"
        );
        assert_eq!(istante_ms("2020-01-01T25:00:00Z"), None, "ora inesistente");
        assert_eq!(
            istante_ms("2020-01-01T00:61:00Z"),
            None,
            "minuto inesistente"
        );
        assert_eq!(
            istante_ms("2020-01-01-02T00:00:00Z"),
            None,
            "un pezzo di troppo"
        );
    }

    #[test]
    fn il_secondo_intercalare_non_butta_via_la_riga() {
        // Succede due volte l'anno, ed è una riga valida.
        assert!(istante_ms("2016-12-31T23:59:60Z").is_some());
        assert_eq!(istante_ms("2016-12-31T23:59:61Z"), None);
    }

    #[test]
    fn i_frazionari_si_troncano_ai_secondi() {
        assert_eq!(
            istante_ms("2020-01-01T12:00:00.523Z"),
            istante_ms("2020-01-01T12:00:00Z")
        );
    }

    #[test]
    fn un_giro_completo_su_dieci_anni_di_date() {
        // Ogni 1º del mese per dieci anni: le date crescono sempre, e ogni
        // passo è fra 28 e 31 giorni. Prende gli errori di segno e di soglia
        // dell'aritmetica delle ere senza scrivere trecento aspettative a mano.
        let mut precedente = None;
        for anno in 2015..2025 {
            for mese in 1..=12u32 {
                let ms = istante_ms(&format!("{anno}-{mese:02}-01T00:00:00Z"))
                    .expect("una data del primo del mese si legge");
                if let Some(prima) = precedente {
                    // Divisione intera voluta: si stanno contando giorni interi.
                    #[expect(
                        clippy::integer_division,
                        reason = "divisione intera voluta: si stanno contando giorni interi"
                    )]
                    let giorni = (ms - prima) / MS_AL_GIORNO;
                    assert!(
                        (28..=31).contains(&giorni),
                        "{anno}-{mese:02}: {giorni} giorni dal mese prima"
                    );
                }
                precedente = Some(ms);
            }
        }
    }

    #[test]
    fn andata_e_ritorno_su_un_secolo() {
        // Le due funzioni devono annullarsi a vicenda per ogni giorno, non per
        // qualche data scelta bene: un errore di un giorno nella
        // redistribuzione dei mesi si nasconde benissimo fra due campioni.
        // Dal 1º gennaio 1970 al 2069, giorno per giorno.
        for giorni in 0..36_525_i64 {
            let (anno, mese, giorno) = data_dall_epoca(giorni);
            assert_eq!(
                giorni_dall_epoca(anno, mese, giorno),
                giorni,
                "il giorno {giorni} torna {anno:04}-{mese:02}-{giorno:02}, che non ci ritorna"
            );
            assert!((1..=12).contains(&mese), "mese fuori scala: {mese}");
            assert!((1..=31).contains(&giorno), "giorno fuori scala: {giorno}");
        }
    }

    #[test]
    fn il_ventinove_febbraio_va_e_torna() {
        // Il caso che l'aritmetica delle ere esiste per prendere, nel verso
        // che la funzione nuova percorre.
        assert_eq!(
            data_dall_epoca(giorni_dall_epoca(2000, 2, 29)),
            (2000, 2, 29)
        );
        assert_eq!(
            data_dall_epoca(giorni_dall_epoca(2024, 2, 29)),
            (2024, 2, 29)
        );
        // 1900 non è bisestile: il 28 è l'ultimo giorno di febbraio.
        assert_eq!(
            data_dall_epoca(giorni_dall_epoca(1900, 2, 28)),
            (1900, 2, 28)
        );
        assert_eq!(data_dall_epoca(giorni_dall_epoca(1900, 3, 1)), (1900, 3, 1));
    }

    #[test]
    fn l_istante_si_scrive_e_si_rilegge() {
        assert_eq!(istante_iso(0), "1970-01-01 00:00:00.000Z");
        // La stessa data che `istante_ms` legge nella prova qui sopra, scritta
        // dall'altra funzione: è il giro completo fra le due.
        assert_eq!(istante_iso(1_577_880_000_000), "2020-01-01 12:00:00.000Z");
        assert_eq!(
            istante_ms("2020-01-01T12:00:00Z"),
            Some(1_577_880_000_000),
            "e la lettura torna indietro"
        );
        // I millisecondi non si perdono per strada.
        assert_eq!(istante_iso(1_577_880_000_482), "2020-01-01 12:00:00.482Z");
    }

    #[test]
    fn prima_dell_epoca_non_escono_ore_negative() {
        // Una macchina con l'orologio indietro non deve produrre un diario
        // illeggibile: `div_euclid` e `rem_euclid` tengono il verso.
        let scritto = istante_iso(-1);
        assert_eq!(scritto, "1969-12-31 23:59:59.999Z");
        assert!(!scritto.contains('-') || scritto.starts_with("19"));
    }
}
