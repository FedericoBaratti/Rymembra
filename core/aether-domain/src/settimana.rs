//! Il lunedì: le raccolte che si rifanno una volta a settimana.
//!
//! # Perché un appuntamento e non un pulsante
//!
//! Il meccanismo più copiato di Spotify è la Discover Weekly, e il dato che
//! Spotify stessa pubblica è che chi la usa ascolta più del doppio di chi non la
//! usa. L'ipotesi corrente su **perché** — sostenuta da stime di terzi che non
//! ho verificato alla fonte — è che a costruire l'abitudine sia il rinnovo
//! settimanale più della qualità delle scelte: l'appuntamento, non l'algoritmo.
//!
//! Se è vero, la parte che conta di questo modulo non è il punteggio: è che le
//! raccolte **cambiano di lunedì** e non quando ti va di premere un tasto. Un
//! pulsante «rigenera» distruggerebbe la cosa che si sta cercando di costruire,
//! ed è per questo che non c'è.
//!
//! # Il lunedì di chi
//!
//! Qui non si sa che ore sono e non si sa dove si è: [`lunedi`] riceve
//! l'istante **e** lo scostamento dal tempo universale, e chi la chiama li
//! prende dal sistema. È lo stesso motivo per cui `primo.rs` riceve i percorsi
//! invece di indovinarli — un fuso orario è una cosa che il sistema dichiara, e
//! un modulo che si prova offline non può avercelo dentro.
//!
//! Sbagliare fuso qui non è un dettaglio estetico: vorrebbe dire che a Roma le
//! raccolte nuove compaiono la domenica alle undici di sera, cioè in un giorno
//! che nel calendario di chi guarda non è lunedì.
//!
//! # Le due raccolte, e la terza che non c'è
//!
//! * [`ripescaggio`] — **Ripescati**: quel che hai amato e non senti da mesi.
//!   È la raccolta che nessun servizio in streaming può fare, perché per lui non
//!   possiedi niente e la tua storia comincia il giorno in cui ti sei iscritto.
//! * [`raccogli`] — **Ancora**: gruppi coerenti trovati nello spazio delle
//!   impronte sonore.
//!
//! La terza del progetto — **Fuori**, musica nuova dai cataloghi liberi — non è
//! qui perché non è un problema di matematica: richiede di **cercare** in un
//! catalogo per somiglianza, e i cataloghi liberi che Aether conosce sanno
//! rispondere soltanto a «hai questo preciso brano?». Il giorno in cui sapranno
//! rispondere all'altra domanda, la funzione che manca va accanto a queste due.

use crate::affinita::{Gruppo, distanza2};

/// Un giorno, in millisecondi.
pub const GIORNO_MS: i64 = 24 * 60 * 60 * 1000;

/// Una settimana, in millisecondi.
pub const SETTIMANA_MS: i64 = 7 * GIORNO_MS;

/// Quanti giorni dall'ultimo ascolto perché un brano cominci a essere dimenticato.
///
/// Sessanta. Sotto, il brano è ancora in circolo: riproporlo come «ripescato»
/// sarebbe una bugia che chi guarda riconosce subito, ed è il modo più veloce di
/// far smettere di aprire un ripiano.
pub const OBLIO_MINIMO_GIORNI: f32 = 60.0;

/// Dopo quanti giorni un brano è dimenticato del tutto.
///
/// Un anno. Fra i due mesi e l'anno l'oblio sale per gradi, così un disco che
/// non senti da otto mesi vale più di uno che non senti da tre — che è l'ordine
/// in cui una persona li ritroverebbe volentieri.
pub const OBLIO_PIENO_GIORNI: f32 = 365.0;

/// Quanti ascolti bastano perché l'affetto sia pieno.
///
/// Sei, lo stesso di `affinita::RIFERIMENTO_ASCOLTI`, e per la stessa ragione:
/// oltre la mezza dozzina la differenza fra «gli piace» e «gli piace molto» non
/// la dicono più i conteggi.
pub const ASCOLTI_PIENI: f32 = 6.0;

/// Quanto pesa il cuore rispetto agli ascolti.
///
/// Un terzo. Un «mi piace» è una dichiarazione esplicita e vale molto, ma non
/// deve poter da solo portare in cima un brano ascoltato una volta: la raccolta
/// si chiama Ripescati e presuppone che ci sia qualcosa da ripescare.
pub const PESO_CUORE: f32 = 1.0 / 3.0;

/// Quel che si sa di un brano trascurato.
///
/// # Perché non basta `affinita::gusto`
///
/// Perché quella funzione fa decadere il gusto col tempo, ed è giusto: quel che
/// ascoltavi tre anni fa dice meno di quel che ascolti adesso. Qui serve
/// esattamente l'opposto sull'asse del tempo — l'affetto di allora **integro**,
/// e la distanza da allora come **merito**. Usare la stessa funzione darebbe la
/// raccolta di quel che hai già in circolo, cioè la Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Trascurato {
    /// Quante volte lo hai fatto partire, in tutta la cronologia.
    pub ascolti: u32,
    /// Da quanti millisecondi non lo senti. `None` se non l'hai mai sentito.
    pub da_quanto_ms: Option<i64>,
    /// Ha il cuore.
    pub preferito: bool,
}

/// Quanto vale ripescare questo brano, da `0.0` a `1.0`.
///
/// È il prodotto di due fattori e non la loro somma, perché sono due condizioni
/// e non due indizi: un brano molto amato e sentito ieri non va ripescato, e un
/// brano dimenticato da tre anni che non ti è mai piaciuto nemmeno. Una somma
/// li farebbe passare entrambi a metà punteggio; un prodotto li azzera, che è
/// quel che si vuole.
///
/// Un brano **mai** ascoltato vale zero: non è un ripescaggio, è una scoperta, e
/// per quella c'è già il ripiano «Trascurati» nella Home.
#[must_use]
pub fn ripescaggio(brano: &Trascurato, _adesso_ms: i64) -> f32 {
    let Some(da_quanto_ms) = brano.da_quanto_ms else {
        return 0.0;
    };
    if brano.ascolti == 0 {
        return 0.0;
    }
    affetto(brano) * oblio(da_quanto_ms)
}

/// Quanto ti era piaciuto, da `0.0` a `1.0`.
fn affetto(brano: &Trascurato) -> f32 {
    let per_ascolti =
        (f32::from(u16::try_from(brano.ascolti.min(u32::from(u16::MAX))).unwrap_or(0))
            / ASCOLTI_PIENI)
            .min(1.0);
    let cuore = if brano.preferito { PESO_CUORE } else { 0.0 };
    // Il cuore **aggiunge** invece di far media: chi ha messo il cuore a un
    // brano e l'ha sentito sei volte non deve valere meno di chi l'ha solo
    // sentito sei volte.
    (per_ascolti + cuore).min(1.0)
}

/// Quanto è dimenticato, da `0.0` a `1.0`.
fn oblio(da_quanto_ms: i64) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "millisecondi in giorni: oltre i 97 anni la precisione di un f32 \
                  scende sotto il giorno, e il fattore è già saturo da 96 anni"
    )]
    let giorni = da_quanto_ms.max(0) as f32 / GIORNO_MS as f32;
    if giorni <= OBLIO_MINIMO_GIORNI {
        return 0.0;
    }
    let arco = OBLIO_PIENO_GIORNI - OBLIO_MINIMO_GIORNI;
    ((giorni - OBLIO_MINIMO_GIORNI) / arco).clamp(0.0, 1.0)
}

/// Il lunedì della settimana in cui cade `adesso_ms`, a mezzanotte locale.
///
/// `scostamento_minuti` sono i minuti da **aggiungere** al tempo universale per
/// ottenere l'ora locale: `+60` per l'Italia d'inverno, `+120` d'estate, `-300`
/// per New York. È il contrario del segno che restituisce `getTimezoneOffset()`
/// in JavaScript, ed è il verso che si legge senza doverci pensare.
///
/// Il risultato è di nuovo in millisecondi dall'epoca, cioè universale: è
/// l'istante in cui è cominciato il lunedì di chi guarda.
#[must_use]
pub fn lunedi(adesso_ms: i64, scostamento_minuti: i32) -> i64 {
    let scostamento_ms = i64::from(scostamento_minuti).saturating_mul(60 * 1000);
    let locale = adesso_ms.saturating_add(scostamento_ms);
    let giorni = locale.div_euclid(GIORNO_MS);
    // L'epoca cade di giovedì: `giorni % 7 == 0` è un giovedì, e il lunedì
    // precedente sta quattro giorni prima. Senza questo `-4` le raccolte si
    // rinnoverebbero di giovedì, che è il genere di difetto che nessuno nota
    // per sei mesi e poi non si spiega.
    let dal_lunedi = (giorni - 4).rem_euclid(7);
    (giorni - dal_lunedi)
        .saturating_mul(GIORNO_MS)
        .saturating_sub(scostamento_ms)
}

/// Un brano con la sua impronta già normalizzata.
#[derive(Debug, Clone)]
pub struct Punto {
    /// Quale brano.
    pub id: i64,
    /// L'impronta normalizzata sulla scala della libreria.
    pub vettore: Vec<f32>,
}

/// Divide i brani in raccolte coerenti, di lunghezza fissa.
///
/// # Perché non è un `k-means`
///
/// Perché un `k-means` produce gruppi di dimensione qualunque, e qui serve una
/// **playlist**: dieci gruppi da tre brani e uno da millequattrocento sono un
/// risultato corretto e un prodotto inutilizzabile. Il numero che l'utente vede
/// non è «quanti gruppi ha trovato l'algoritmo», è «quanto dura questa
/// raccolta».
///
/// Quindi semina e cresci: si prende un seme, gli si mettono accanto i suoi
/// `quanti - 1` vicini più prossimi, si toglie tutto dal mucchio e si ricomincia
/// dal brano **più lontano** dai semi già usati. Il risultato è deterministico,
/// non ha iterazioni da far convergere, e ogni raccolta ha la lunghezza che
/// deve avere.
///
/// Il primo seme è `da_dove`, che chi chiama fa dipendere dal lunedì: è tutto ciò
/// che serve perché la settimana prossima le raccolte siano altre, senza tenere
/// da nessuna parte quelle di questa.
///
/// Restituisce meno di `raccolte` gruppi se i brani non bastano, e mai gruppi
/// più corti di `quanti`: mezza raccolta è peggio di nessuna raccolta.
#[must_use]
pub fn raccogli(
    punti: &[Punto],
    gruppi: &[Gruppo],
    raccolte: usize,
    quanti: usize,
    da_dove: usize,
) -> Vec<Vec<i64>> {
    if quanti == 0 || raccolte == 0 || punti.len() < quanti {
        return Vec::new();
    }
    let mut liberi: Vec<&Punto> = punti.iter().collect();
    let mut fuori = Vec::new();
    // Il primo seme è l'unico scelto senza guardare le distanze: dopo, ogni seme
    // è il punto più lontano da tutti i semi già usati, che è ciò che tiene le
    // raccolte distinte fra loro invece di farne quattro sfumature della stessa.
    let mut seme = da_dove % liberi.len();
    let mut semi: Vec<Vec<f32>> = Vec::new();

    for _ in 0..raccolte {
        if liberi.len() < quanti {
            break;
        }
        let Some(centro) = liberi.get(seme).map(|p| p.vettore.clone()) else {
            break;
        };

        // I `quanti` più vicini al seme, seme compreso.
        let mut per_distanza: Vec<(usize, f32)> = liberi
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    i,
                    distanza2(&centro, &p.vettore, gruppi).unwrap_or(f32::MAX),
                )
            })
            .collect();
        per_distanza.sort_by(|a, b| a.1.total_cmp(&b.1));
        let scelti: Vec<usize> = per_distanza.iter().take(quanti).map(|(i, _)| *i).collect();

        fuori.push(
            scelti
                .iter()
                .filter_map(|i| liberi.get(*i).map(|p| p.id))
                .collect(),
        );
        semi.push(centro);

        // Si tolgono dal mucchio partendo dal fondo, o gli indici scivolano.
        let mut da_togliere = scelti;
        da_togliere.sort_unstable();
        for i in da_togliere.iter().rev() {
            if *i < liberi.len() {
                liberi.remove(*i);
            }
        }
        if liberi.len() < quanti {
            break;
        }

        // Il prossimo seme: il più lontano da tutti quelli di prima.
        seme = liberi
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let piu_vicino = semi
                    .iter()
                    .map(|s| distanza2(s, &p.vettore, gruppi).unwrap_or(0.0))
                    .fold(f32::MAX, f32::min);
                (i, piu_vicino)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(i, _)| i);
    }
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il lunedì è un lunedì, e non un giovedì.
    ///
    /// Il 5 settembre 2026 è un sabato; il suo lunedì è il 31 agosto.
    #[test]
    fn il_lunedi_e_il_lunedi_prima() {
        // 2026-09-05T12:00:00Z
        let sabato = 1_788_004_800_000_i64;
        let l = lunedi(sabato, 0);
        // 2026-08-31T00:00:00Z
        assert_eq!(l, 1_787_529_600_000);
        // Ed è a mezzanotte esatta.
        assert_eq!(l.rem_euclid(GIORNO_MS), 0);
    }

    /// Un lunedì resta se stesso per tutta la settimana, e cambia il lunedì dopo.
    #[test]
    fn dentro_la_settimana_non_cambia_e_alla_fine_si() {
        let sabato = 1_788_004_800_000_i64;
        let l = lunedi(sabato, 0);
        for giorno in 0..7 {
            assert_eq!(
                lunedi(l + giorno * GIORNO_MS + 1, 0),
                l,
                "il giorno {giorno} è finito in un'altra settimana"
            );
        }
        assert_eq!(lunedi(l + SETTIMANA_MS, 0), l + SETTIMANA_MS);
    }

    /// Il fuso sposta il confine, ed è tutto il punto di riceverlo.
    ///
    /// A Roma, la domenica alle 23:30 UTC sono le 00:30 di lunedì: le raccolte
    /// nuove ci sono già. Con lo stesso istante letto in UTC sarebbero ancora
    /// quelle della settimana prima.
    #[test]
    fn a_roma_il_lunedi_comincia_un_ora_prima_che_a_greenwich() {
        let l = lunedi(1_788_004_800_000_i64, 0);
        let domenica_tardi = l + 6 * GIORNO_MS + 23 * 60 * 60 * 1000 + 30 * 60 * 1000;
        assert_eq!(
            lunedi(domenica_tardi, 0),
            l,
            "in UTC è ancora domenica e la settimana è quella"
        );
        // Il valore che torna è l'istante **universale** in cui è cominciato il
        // lunedì di chi guarda: a Roma è la mezzanotte locale, cioè le 23 di
        // domenica a Greenwich. Un'ora prima, non un'ora dopo.
        assert_eq!(
            lunedi(domenica_tardi, 60),
            l + SETTIMANA_MS - 60 * 60 * 1000,
            "a Roma è già lunedì, e il suo lunedì comincia un'ora prima"
        );
    }

    /// Prima dell'epoca non si cade: `rem_euclid` non è `%`.
    #[test]
    fn anche_prima_del_millenovecentosettanta_e_un_lunedi() {
        let l = lunedi(-1, 0);
        assert_eq!(l.rem_euclid(GIORNO_MS), 0);
        assert!(l <= -1);
        assert!(-1 - l < SETTIMANA_MS);
    }

    /// Un brano mai sentito non si ripesca.
    #[test]
    fn quel_che_non_hai_mai_sentito_non_si_ripesca() {
        let mai = Trascurato {
            ascolti: 0,
            da_quanto_ms: None,
            preferito: true,
        };
        assert!((ripescaggio(&mai, 0) - 0.0).abs() < f32::EPSILON);
    }

    /// Un brano sentito ieri non si ripesca, per quanto amato.
    #[test]
    fn quel_che_hai_sentito_ieri_non_si_ripesca() {
        let ieri = Trascurato {
            ascolti: 100,
            da_quanto_ms: Some(GIORNO_MS),
            preferito: true,
        };
        assert!((ripescaggio(&ieri, 0) - 0.0).abs() < f32::EPSILON);
    }

    /// Fra due dimenticati vince quello che era piaciuto di più.
    #[test]
    fn a_parita_di_oblio_vince_l_affetto() {
        let anno = 400 * GIORNO_MS;
        let amato = Trascurato {
            ascolti: 20,
            da_quanto_ms: Some(anno),
            preferito: true,
        };
        let sfiorato = Trascurato {
            ascolti: 1,
            da_quanto_ms: Some(anno),
            preferito: false,
        };
        assert!(ripescaggio(&amato, 0) > ripescaggio(&sfiorato, 0));
    }

    /// Fra due amati uguali vince quello più lontano nel tempo.
    #[test]
    fn a_parita_di_affetto_vince_l_oblio() {
        let fatto = |giorni: i64| Trascurato {
            ascolti: 10,
            da_quanto_ms: Some(giorni * GIORNO_MS),
            preferito: false,
        };
        assert!(ripescaggio(&fatto(300), 0) > ripescaggio(&fatto(90), 0));
    }

    /// L'oblio è pieno a un anno e non cresce oltre.
    #[test]
    fn oltre_l_anno_l_oblio_non_cresce_piu() {
        let fatto = |giorni: i64| Trascurato {
            ascolti: 10,
            da_quanto_ms: Some(giorni * GIORNO_MS),
            preferito: true,
        };
        let a_un_anno = ripescaggio(&fatto(365), 0);
        let a_dieci_anni = ripescaggio(&fatto(3650), 0);
        assert!((a_un_anno - a_dieci_anni).abs() < 1e-6);
        assert!(
            a_un_anno > 0.99,
            "un anno di oblio e venti ascolti: {a_un_anno}"
        );
    }

    /// Il punteggio non esce mai da zero-uno.
    #[test]
    fn il_punteggio_resta_in_scala() {
        for ascolti in [0_u32, 1, 6, 1000, u32::MAX] {
            for giorni in [0_i64, 59, 61, 365, 100_000] {
                let p = ripescaggio(
                    &Trascurato {
                        ascolti,
                        da_quanto_ms: Some(giorni * GIORNO_MS),
                        preferito: true,
                    },
                    0,
                );
                assert!((0.0..=1.0).contains(&p), "{ascolti}/{giorni} → {p}");
            }
        }
    }

    /// Il raggruppamento in prova: una dimensione sola, un gruppo solo.
    fn una_dimensione() -> [Gruppo; 1] {
        [Gruppo {
            nome: "prova",
            inizio: 0,
            fine: 1,
            peso: 1.0,
        }]
    }

    fn punti(semi: &[(i64, f32)]) -> Vec<Punto> {
        semi.iter()
            .map(|(id, x)| Punto {
                id: *id,
                vettore: vec![*x],
            })
            .collect()
    }

    /// Due mucchi lontani diventano due raccolte, e non due metà miste.
    #[test]
    fn due_mucchi_lontani_diventano_due_raccolte() {
        let p = punti(&[
            (1, 0.0),
            (2, 0.1),
            (3, 0.2),
            (10, 50.0),
            (11, 50.1),
            (12, 50.2),
        ]);
        let raccolte = raccogli(&p, &una_dimensione(), 2, 3, 0);
        assert_eq!(raccolte.len(), 2);
        for raccolta in &raccolte {
            let basso = raccolta.iter().filter(|id| **id < 10).count();
            assert!(
                basso == 0 || basso == 3,
                "una raccolta ha mescolato i due mucchi: {raccolta:?}"
            );
        }
    }

    /// Ogni raccolta ha la lunghezza chiesta, o non esiste.
    #[test]
    fn nessuna_raccolta_esce_a_meta() {
        let p = punti(&[(1, 0.0), (2, 1.0), (3, 2.0), (4, 3.0), (5, 4.0)]);
        // Cinque brani, raccolte da tre: ne esce una sola.
        let raccolte = raccogli(&p, &una_dimensione(), 4, 3, 0);
        assert_eq!(raccolte.len(), 1);
        assert_eq!(raccolte.first().map(Vec::len), Some(3));
    }

    /// Un brano non finisce in due raccolte.
    #[test]
    fn nessun_brano_in_due_raccolte() {
        let p = punti(&(0..30).map(|i| (i, i as f32)).collect::<Vec<_>>());
        let raccolte = raccogli(&p, &una_dimensione(), 3, 5, 0);
        let mut visti = Vec::new();
        for raccolta in &raccolte {
            visti.extend(raccolta.iter().copied());
        }
        let quanti = visti.len();
        visti.sort_unstable();
        visti.dedup();
        assert_eq!(quanti, visti.len(), "un brano è finito in due raccolte");
    }

    /// Un seme diverso dà raccolte diverse: è il rinnovo del lunedì.
    #[test]
    fn il_lunedi_dopo_le_raccolte_sono_altre() {
        let p = punti(&(0..40).map(|i| (i, i as f32)).collect::<Vec<_>>());
        let questa = raccogli(&p, &una_dimensione(), 2, 5, 0);
        let prossima = raccogli(&p, &una_dimensione(), 2, 5, 17);
        assert_ne!(questa, prossima, "il lunedì dopo è uguale a questo");
    }

    /// Meno brani della lunghezza chiesta: niente, e non un guasto.
    #[test]
    fn una_libreria_troppo_piccola_non_produce_niente() {
        let p = punti(&[(1, 0.0), (2, 1.0)]);
        assert!(raccogli(&p, &una_dimensione(), 3, 10, 0).is_empty());
        assert!(raccogli(&[], &una_dimensione(), 3, 5, 0).is_empty());
        assert!(raccogli(&p, &una_dimensione(), 0, 1, 0).is_empty());
        assert!(raccogli(&p, &una_dimensione(), 3, 0, 0).is_empty());
    }
}
