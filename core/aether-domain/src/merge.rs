//! Fondere due versioni delle stesse statistiche d'ascolto.
//!
//! Serve in due punti che sembrano diversi e non lo sono: importare il vecchio
//! database, e allineare due dispositivi. In entrambi arrivano due versioni
//! della stessa riga e bisogna decidere cosa sopravvive, senza poter chiedere.
//!
//! # Perché non «l'ultimo che scrive vince»
//!
//! È la regola che viene da sé, ed è quella che perde dati. Conteggi d'ascolto,
//! voti e preferiti sono l'unica cosa in tutta la libreria che **non si può
//! ricostruire**: i tag stanno nei file, le copertine stanno nei file, la
//! durata si ricalcola. Il numero di volte che una canzone è stata ascoltata
//! esiste in un posto solo, e quando lo si sovrascrive è finito.
//!
//! Da qui la forma delle regole qui sotto: ognuna sceglie, a parità di
//! informazione, la direzione che **non distrugge**. Un conteggio sale e non
//! scende; uno zero non batte un valore, perché zero vuol dire «non l'ho mai
//! fatto» e non «l'ho messo a zero».
//!
//! # E perché è idempotente
//!
//! `merge(merge(a, b), b) == merge(a, b)`. Non è eleganza: un'importazione la
//! si rifà — perché è andata storta a metà, perché non si ricorda di averla
//! fatta — e una fusione che sommasse i conteggi raddoppierebbe la storia
//! d'ascolto a ogni ripetizione. Un test lo verifica.

/// Le statistiche di un brano: ciò che una scansione non sa ricostruire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackStats {
    /// Quante volte è stato ascoltato.
    pub play_count: i64,
    /// L'ultimo ascolto, in millisecondi.
    pub last_played_at: Option<i64>,
    /// Il voto, da 0 (nessuno) a 5.
    pub rating: u8,
    /// È fra i preferiti.
    pub liked: bool,
    /// Quando la preferenza è stata espressa, in millisecondi.
    ///
    /// Vale anche per un `liked` **tolto**: è il timestamp della decisione, non
    /// del gradimento, ed è ciò che permette a un «non mi piace più» di vincere
    /// su un «mi piace» più vecchio invece di essere riassorbito.
    pub liked_at: Option<i64>,
    /// Quando voto o preferito sono stati toccati l'ultima volta.
    pub stats_updated_at: i64,
}

/// Il più recente fra due istanti, tenendo conto che possono mancare.
fn later(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

/// Il voto che sopravvive.
///
/// Zero non è un voto: è l'assenza di un voto. Trattarlo come un valore
/// significherebbe che il dispositivo su cui non si è mai votato cancella i
/// voti dell'altro semplicemente sincronizzandosi per ultimo.
///
/// Quando entrambi hanno votato e i voti differiscono decide l'orologio delle
/// statistiche; a parità esatta vince il voto più alto — non perché sia più
/// giusto, ma perché a informazione pari va scelta la direzione che non
/// distrugge, e perché rende la funzione commutativa invece di dipendere da
/// quale dei due si è chiamato «locale».
fn merge_rating(local: &TrackStats, incoming: &TrackStats) -> u8 {
    match (local.rating, incoming.rating) {
        (0, other) | (other, 0) => other,
        (a, b) if a == b => a,
        (a, b) => match local.stats_updated_at.cmp(&incoming.stats_updated_at) {
            std::cmp::Ordering::Greater => a,
            std::cmp::Ordering::Less => b,
            std::cmp::Ordering::Equal => a.max(b),
        },
    }
}

/// Il preferito che sopravvive, e quando è stato deciso.
///
/// Decide `liked_at`, non `liked`: è il timestamp della **decisione**. Senza,
/// un «non mi piace più» sarebbe indistinguibile da «non l'ho mai messo», e
/// togliere un preferito su un dispositivo se lo vedrebbe rimettere dall'altro
/// alla prima sincronizzazione — cioè il modo più veloce di far smettere alla
/// gente di fidarsi di una sincronia.
///
/// Quando nessuno dei due ha una data la decisione non si può datare: si tiene
/// il preferito se almeno uno ce l'ha, che è di nuovo la direzione che non
/// distrugge. È il caso del vecchio database, dove `liked_at` può mancare.
fn merge_liked(local: &TrackStats, incoming: &TrackStats) -> (bool, Option<i64>) {
    match (local.liked_at, incoming.liked_at) {
        (Some(a), Some(b)) if a != b => {
            if a > b {
                (local.liked, local.liked_at)
            } else {
                (incoming.liked, incoming.liked_at)
            }
        }
        // Stessa data, o una sola: non c'è modo di datare la decisione più
        // recente, quindi si conserva.
        _ => (
            local.liked || incoming.liked,
            later(local.liked_at, incoming.liked_at),
        ),
    }
}

/// Fonde due versioni delle statistiche di uno stesso brano.
///
/// Commutativa (`merge(a, b) == merge(b, a)`), associativa sui campi che
/// contano, e **idempotente**: rifare un'importazione non raddoppia niente.
/// Sono le tre proprietà che rendono sicuro chiamarla senza sapere quante volte
/// è già stata chiamata — che è precisamente la situazione di una
/// sincronizzazione fra due dispositivi che si sono persi di vista.
///
/// ```
/// use aether_domain::merge::{TrackStats, merge_stats};
/// let locale = TrackStats { play_count: 3, ..TrackStats::default() };
/// let dal_vecchio = TrackStats { play_count: 15, ..TrackStats::default() };
/// let fuso = merge_stats(&locale, &dal_vecchio);
/// assert_eq!(fuso.play_count, 15);
/// // Rifarla non somma: la storia d'ascolto non raddoppia.
/// assert_eq!(merge_stats(&fuso, &dal_vecchio), fuso);
/// ```
#[must_use]
pub fn merge_stats(local: &TrackStats, incoming: &TrackStats) -> TrackStats {
    let (liked, liked_at) = merge_liked(local, incoming);
    TrackStats {
        // Il massimo e non la somma. Sommare sembra giusto — «gli ascolti di
        // qui più quelli di là» — ed è il difetto che raddoppia la storia a
        // ogni ripetizione della stessa importazione, senza che nessuno se ne
        // accorga finché i numeri non diventano assurdi.
        play_count: local.play_count.max(incoming.play_count),
        last_played_at: later(local.last_played_at, incoming.last_played_at),
        rating: merge_rating(local, incoming),
        liked,
        liked_at,
        stats_updated_at: local.stats_updated_at.max(incoming.stats_updated_at),
    }
}

/// Fonde le statistiche di più righe che sono lo stesso brano.
///
/// Serve quando due file diversi portano lo stesso brano — formati diversi, un
/// doppione mai deduplicato — e le loro storie vanno riunite in una.
///
/// Qui i conteggi **si sommano**, al contrario di [`merge_stats`], e la
/// differenza è la domanda a cui si risponde: là sono due versioni della stessa
/// riga (quante volte è stata ascoltata? una risposta sola, la migliore), qui
/// sono ascolti di file distinti (quante volte in tutto? la somma). Fondere
/// prima i doppioni e poi il risultato con la riga locale tiene l'insieme
/// idempotente lo stesso, perché il secondo passaggio non somma.
#[must_use]
pub fn collapse_duplicates(rows: &[TrackStats]) -> TrackStats {
    let mut out = TrackStats::default();
    for (index, row) in rows.iter().enumerate() {
        let somma = out.play_count.saturating_add(row.play_count);
        out = if index == 0 {
            *row
        } else {
            merge_stats(&out, row)
        };
        out.play_count = somma;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con_ascolti(play_count: i64) -> TrackStats {
        TrackStats {
            play_count,
            ..TrackStats::default()
        }
    }

    #[test]
    fn rifare_un_importazione_non_raddoppia_la_storia() {
        // La proprietà che rende sicuro un importatore «una tantum» che invece
        // qualcuno lancerà due volte.
        let locale = con_ascolti(3);
        let vecchio = con_ascolti(15);
        let una = merge_stats(&locale, &vecchio);
        let due = merge_stats(&una, &vecchio);
        let tre = merge_stats(&due, &vecchio);
        assert_eq!(una.play_count, 15);
        assert_eq!(due, una);
        assert_eq!(tre, una);
    }

    #[test]
    fn fondere_non_dipende_da_chi_si_chiama_locale() {
        let a = TrackStats {
            play_count: 4,
            rating: 5,
            liked: true,
            liked_at: Some(100),
            stats_updated_at: 100,
            last_played_at: Some(90),
        };
        let b = TrackStats {
            play_count: 9,
            rating: 3,
            liked: false,
            liked_at: Some(50),
            stats_updated_at: 50,
            last_played_at: Some(120),
        };
        assert_eq!(merge_stats(&a, &b), merge_stats(&b, &a));
    }

    #[test]
    fn uno_zero_non_cancella_un_voto() {
        // Il dispositivo su cui non si è mai votato non deve azzerare i voti
        // dell'altro solo perché si sincronizza per ultimo.
        let votato = TrackStats {
            rating: 4,
            stats_updated_at: 10,
            ..TrackStats::default()
        };
        let mai_votato = TrackStats {
            rating: 0,
            stats_updated_at: 999,
            ..TrackStats::default()
        };
        assert_eq!(merge_stats(&votato, &mai_votato).rating, 4);
        assert_eq!(merge_stats(&mai_votato, &votato).rating, 4);
    }

    #[test]
    fn fra_due_voti_veri_decide_l_orologio() {
        let vecchio = TrackStats {
            rating: 5,
            stats_updated_at: 10,
            ..TrackStats::default()
        };
        let nuovo = TrackStats {
            rating: 2,
            stats_updated_at: 20,
            ..TrackStats::default()
        };
        assert_eq!(merge_stats(&vecchio, &nuovo).rating, 2);
    }

    #[test]
    fn togliere_un_preferito_non_se_lo_rimette_l_altro_dispositivo() {
        // Il difetto classico di una sincronia ingenua: si toglie il cuoricino,
        // e alla passata dopo torna. Decide `liked_at`, che data la decisione.
        let messo = TrackStats {
            liked: true,
            liked_at: Some(100),
            ..TrackStats::default()
        };
        let tolto = TrackStats {
            liked: false,
            liked_at: Some(200),
            ..TrackStats::default()
        };
        assert!(!merge_stats(&messo, &tolto).liked, "il più recente vince");
        assert!(!merge_stats(&tolto, &messo).liked);
    }

    #[test]
    fn senza_data_un_preferito_si_conserva() {
        // Il caso del vecchio database, dove `liked_at` può mancare: non c'è
        // modo di sapere quale decisione sia più recente, e si tiene quella che
        // non distrugge.
        let messo = TrackStats {
            liked: true,
            liked_at: None,
            ..TrackStats::default()
        };
        assert!(merge_stats(&messo, &TrackStats::default()).liked);
        assert!(merge_stats(&TrackStats::default(), &messo).liked);
    }

    #[test]
    fn l_ultimo_ascolto_e_il_piu_recente_dei_due() {
        let a = TrackStats {
            last_played_at: Some(100),
            ..TrackStats::default()
        };
        let b = TrackStats {
            last_played_at: Some(300),
            ..TrackStats::default()
        };
        assert_eq!(merge_stats(&a, &b).last_played_at, Some(300));
        let mai = TrackStats::default();
        assert_eq!(merge_stats(&a, &mai).last_played_at, Some(100));
    }

    #[test]
    fn due_file_dello_stesso_brano_sommano_gli_ascolti() {
        // Qui sommare è giusto: sono ascolti di file distinti, non due versioni
        // della stessa riga.
        let fuso = collapse_duplicates(&[con_ascolti(3), con_ascolti(4)]);
        assert_eq!(fuso.play_count, 7);
        // …ma il risultato resta idempotente quando incontra la riga locale.
        let locale = con_ascolti(7);
        assert_eq!(merge_stats(&locale, &fuso).play_count, 7);
    }

    #[test]
    fn riunire_i_doppioni_tiene_il_voto_e_il_preferito() {
        let votato = TrackStats {
            play_count: 1,
            rating: 4,
            stats_updated_at: 10,
            ..TrackStats::default()
        };
        let piaciuto = TrackStats {
            play_count: 2,
            liked: true,
            liked_at: Some(50),
            ..TrackStats::default()
        };
        let fuso = collapse_duplicates(&[votato, piaciuto]);
        assert_eq!(fuso.play_count, 3);
        assert_eq!(fuso.rating, 4);
        assert!(fuso.liked);
    }

    #[test]
    fn nessun_doppione_da_statistiche_vuote() {
        assert_eq!(collapse_duplicates(&[]), TrackStats::default());
    }
}
