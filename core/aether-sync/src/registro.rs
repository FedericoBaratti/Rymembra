//! I valori che si fondono scegliendo un vincitore.
//!
//! Voto, preferito, punto di riascolto, cartelle sorvegliate, skin attiva: sono
//! tutti fatti singoli su cui due dispositivi possono avere idee diverse, e per
//! cui l'unica risposta sensata è «l'ultimo che ha deciso».
//!
//! # Perché la data sta accanto al valore e non nella busta
//!
//! Perché è la data della **decisione**, non della scrittura del file. Un
//! documento scritto stamattina può portare un voto messo l'anno scorso, e se a
//! decidere fosse la data del file quel voto vincerebbe su uno più recente per il
//! solo fatto che il portatile si è acceso dopo. È la stessa ragione per cui
//! `TrackStats` ha `liked_at` separato da `stats_updated_at`, e la ragione per
//! cui chi costruisce un documento dal database deve copiare la data che c'è, mai
//! metterci «adesso»: riaffermare un valore che si è imparato da un altro
//! dispositivo è innocuo finché si riafferma anche quando è stato deciso.
//!
//! # Perché a parità esatta vince chi non distrugge
//!
//! Due decisioni con lo stesso millisecondo esistono — orologi che si somigliano,
//! importazioni in blocco che datano tutto uguale — e a quel punto non c'è più
//! niente che le ordini. Scegliere in base a chi si chiama «locale» renderebbe la
//! fusione non commutativa, cioè farebbe dipendere il risultato dall'ordine in
//! cui i dispositivi si sono parlati. Si sceglie allora la direzione che
//! conserva: il voto più alto, il preferito acceso. È la stessa regola di
//! [`aether_domain::merge::merge_stats`], per lo stesso motivo.

use std::collections::BTreeMap;

/// Un valore che si fonde con un altro dello stesso tipo.
///
/// Le implementazioni devono essere **commutative** (`a.fondi(b) == b.fondi(a)`),
/// **associative** e **idempotenti** (`a.fondi(a) == a`). Non sono tre eleganze:
/// sono ciò che rende sicuro fondere senza sapere quanti dispositivi ci sono, in
/// che ordine sono arrivati, né quante volte lo si è già fatto. Un test per
/// tipo le verifica.
pub trait Registro: Clone {
    /// Il valore che sopravvive.
    #[must_use]
    fn fondi(&self, altro: &Self) -> Self;
}

/// Un voto, con quando è stato messo.
///
/// `v` vale da 0 a 5. Lo zero qui è un valore vero — «ho tolto il voto» — e non
/// l'assenza di uno: l'assenza è non avere affatto la voce nella mappa. È la
/// distinzione che `TrackStats` non può fare, avendo un `u8` e basta, ed è il
/// motivo per cui `merge_stats` deve trattare lo zero come «non pervenuto»
/// mentre qui non serve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Voto {
    /// Da 0 (tolto) a 5.
    pub v: u8,
    /// Quando è stato deciso, in millisecondi.
    pub at: i64,
}

impl Registro for Voto {
    fn fondi(&self, altro: &Self) -> Self {
        match self.at.cmp(&altro.at) {
            std::cmp::Ordering::Greater => *self,
            std::cmp::Ordering::Less => *altro,
            std::cmp::Ordering::Equal => Self {
                v: self.v.max(altro.v),
                at: self.at,
            },
        }
    }
}

/// Un sì o un no, con quando è stato deciso.
///
/// Serve al preferito e alle cartelle sorvegliate, che sono la stessa domanda
/// posta a due cose diverse. Un solo tipo e non due: due copie della stessa
/// fusione sono due posti in cui correggere lo stesso difetto, e uno dei due
/// resta indietro.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Interruttore {
    /// Acceso.
    pub on: bool,
    /// Quando è stato deciso, in millisecondi.
    pub at: i64,
}

impl Registro for Interruttore {
    fn fondi(&self, altro: &Self) -> Self {
        match self.at.cmp(&altro.at) {
            std::cmp::Ordering::Greater => *self,
            std::cmp::Ordering::Less => *altro,
            // Senza una data che li ordini si tiene l'acceso: è il caso del
            // vecchio database, dove `liked_at` può mancare del tutto, e togliere
            // un preferito che non si sa quando è stato messo vuol dire buttare
            // via l'unica informazione che una scansione non ricostruisce.
            std::cmp::Ordering::Equal => Self {
                on: self.on || altro.on,
                at: self.at,
            },
        }
    }
}

/// Un punto nel brano, con quando ci si è arrivati.
///
/// È il riascolto: si chiude l'app a metà di un podcast di due ore e lo si
/// riprende dal telefono. Vince il più recente e non il più avanti — chi ha
/// riascoltato per ultimo sa dove è arrivato meglio di chi era andato più in là
/// la settimana scorsa.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Momento {
    /// Il punto, in millisecondi dall'inizio.
    pub ms: i64,
    /// Quando ci si è arrivati, in millisecondi.
    pub at: i64,
}

impl Registro for Momento {
    fn fondi(&self, altro: &Self) -> Self {
        match self.at.cmp(&altro.at) {
            std::cmp::Ordering::Greater => *self,
            std::cmp::Ordering::Less => *altro,
            std::cmp::Ordering::Equal => Self {
                ms: self.ms.max(altro.ms),
                at: self.at,
            },
        }
    }
}

/// Una scelta fra tante, con quando è stata fatta.
///
/// La skin attiva. A parità di data vince l'identificativo più grande in ordine
/// alfabetico: non perché sia migliore, ma perché serve una regola qualsiasi
/// purché sia **la stessa su tutti i dispositivi**, e l'ordine di due stringhe è
/// l'unica che non dipende da chi sta guardando.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scelta {
    /// Cosa è stato scelto.
    pub id: String,
    /// Quando, in millisecondi.
    pub at: i64,
}

impl Registro for Scelta {
    fn fondi(&self, altro: &Self) -> Self {
        match (self.at.cmp(&altro.at), self.id.cmp(&altro.id)) {
            (std::cmp::Ordering::Greater, _)
            | (std::cmp::Ordering::Equal, std::cmp::Ordering::Greater) => self.clone(),
            _ => altro.clone(),
        }
    }
}

/// Fonde due mappe di registri: unione delle chiavi, fusione dei valori in comune.
///
/// L'unione e non l'intersezione: una chiave che sta solo di là è una decisione
/// presa su un dispositivo che qui non si è ancora vista, e scartarla vorrebbe
/// dire che sincronizzarsi cancella. Le cancellazioni viaggiano per conto loro,
/// come lapidi, e sono l'unico modo di togliere qualcosa.
#[must_use]
pub fn fondi_mappa<V: Registro>(
    a: &BTreeMap<String, V>,
    b: &BTreeMap<String, V>,
) -> BTreeMap<String, V> {
    let mut fuso = a.clone();
    for (chiave, valore) in b {
        fuso.entry(chiave.clone())
            .and_modify(|gia| *gia = gia.fondi(valore))
            .or_insert_with(|| valore.clone());
    }
    fuso
}

/// Fonde due mappe di istanti tenendo il più recente.
///
/// Serve all'ultimo ascolto e alle lapidi, dove il valore **è** la data e non c'è
/// niente da mettergli accanto.
#[must_use]
pub fn fondi_istanti(
    a: &BTreeMap<String, i64>,
    b: &BTreeMap<String, i64>,
) -> BTreeMap<String, i64> {
    let mut fuso = a.clone();
    for (chiave, quando) in b {
        fuso.entry(chiave.clone())
            .and_modify(|gia| *gia = (*gia).max(*quando))
            .or_insert(*quando);
    }
    fuso
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Le tre proprietà, su una terna qualsiasi di valori.
    fn le_tre_proprieta<V: Registro + PartialEq + std::fmt::Debug>(a: &V, b: &V, c: &V) {
        assert_eq!(a.fondi(b), b.fondi(a), "non commutativa");
        assert_eq!(a.fondi(a), *a, "non idempotente");
        assert_eq!(a.fondi(b).fondi(c), a.fondi(&b.fondi(c)), "non associativa");
    }

    #[test]
    fn il_voto_piu_recente_vince() {
        let vecchio = Voto { v: 5, at: 10 };
        let nuovo = Voto { v: 2, at: 20 };
        assert_eq!(vecchio.fondi(&nuovo), nuovo);
        le_tre_proprieta(&vecchio, &nuovo, &Voto { v: 3, at: 15 });
    }

    #[test]
    fn togliere_un_voto_si_propaga() {
        // Quel che `merge_stats` non può fare, avendo solo un `u8`: là lo zero
        // vale «non pervenuto», e un voto tolto se lo rimetterebbe l'altro
        // dispositivo. Qui lo zero è una decisione, e ha la sua data.
        let messo = Voto { v: 4, at: 100 };
        let tolto = Voto { v: 0, at: 200 };
        assert_eq!(messo.fondi(&tolto).v, 0);
        assert_eq!(tolto.fondi(&messo).v, 0);
    }

    #[test]
    fn a_parita_di_data_il_voto_non_si_abbassa() {
        let quattro = Voto { v: 4, at: 50 };
        let due = Voto { v: 2, at: 50 };
        assert_eq!(quattro.fondi(&due).v, 4);
        assert_eq!(due.fondi(&quattro).v, 4);
    }

    #[test]
    fn togliere_un_preferito_non_se_lo_rimette_l_altro_dispositivo() {
        // Il difetto classico di una sincronia ingenua: si toglie il cuoricino, e
        // alla passata dopo torna.
        let messo = Interruttore { on: true, at: 100 };
        let tolto = Interruttore { on: false, at: 200 };
        assert!(!messo.fondi(&tolto).on);
        assert!(!tolto.fondi(&messo).on);
        le_tre_proprieta(&messo, &tolto, &Interruttore { on: true, at: 300 });
    }

    #[test]
    fn senza_una_data_che_ordini_il_preferito_si_conserva() {
        let messo = Interruttore { on: true, at: 0 };
        let mai = Interruttore { on: false, at: 0 };
        assert!(messo.fondi(&mai).on);
        assert!(mai.fondi(&messo).on);
    }

    #[test]
    fn il_riascolto_lo_decide_chi_ha_ascoltato_per_ultimo() {
        // Non il punto più avanti: chi ha riascoltato ieri sa dove è arrivato
        // meglio di chi era andato più in là il mese scorso.
        let avanti_ma_vecchio = Momento {
            ms: 900_000,
            at: 10,
        };
        let indietro_ma_nuovo = Momento { ms: 3_000, at: 20 };
        assert_eq!(avanti_ma_vecchio.fondi(&indietro_ma_nuovo).ms, 3_000);
        le_tre_proprieta(
            &avanti_ma_vecchio,
            &indietro_ma_nuovo,
            &Momento { ms: 1, at: 15 },
        );
    }

    #[test]
    fn la_skin_scelta_per_ultima_vince_e_a_parita_decide_il_nome() {
        let a = Scelta {
            id: "notte".to_owned(),
            at: 10,
        };
        let b = Scelta {
            id: "giorno".to_owned(),
            at: 20,
        };
        assert_eq!(a.fondi(&b), b);
        let pari_uno = Scelta {
            id: "alba".to_owned(),
            at: 5,
        };
        let pari_due = Scelta {
            id: "zenit".to_owned(),
            at: 5,
        };
        assert_eq!(
            pari_uno.fondi(&pari_due),
            pari_due.fondi(&pari_uno),
            "a parità di data la scelta non può dipendere da chi chiama"
        );
        le_tre_proprieta(&a, &b, &pari_uno);
    }

    #[test]
    fn fondere_due_mappe_le_unisce() {
        let mut a = BTreeMap::new();
        a.insert("uno".to_owned(), Voto { v: 3, at: 10 });
        a.insert("due".to_owned(), Voto { v: 5, at: 10 });
        let mut b = BTreeMap::new();
        b.insert("due".to_owned(), Voto { v: 1, at: 20 });
        b.insert("tre".to_owned(), Voto { v: 4, at: 30 });

        let fuso = fondi_mappa(&a, &b);
        assert_eq!(fuso.len(), 3, "l'unione, non l'intersezione");
        assert_eq!(fuso.get("uno").map(|v| v.v), Some(3));
        assert_eq!(
            fuso.get("due").map(|v| v.v),
            Some(1),
            "vince il più recente"
        );
        assert_eq!(fuso.get("tre").map(|v| v.v), Some(4));
        assert_eq!(fondi_mappa(&b, &a), fuso, "non commutativa");
    }

    #[test]
    fn fondere_due_mappe_di_istanti_tiene_il_piu_recente() {
        let a = BTreeMap::from([("uno".to_owned(), 100), ("due".to_owned(), 500)]);
        let b = BTreeMap::from([("due".to_owned(), 200), ("tre".to_owned(), 300)]);
        let fuso = fondi_istanti(&a, &b);
        assert_eq!(fuso.get("due"), Some(&500));
        assert_eq!(fuso.len(), 3);
        assert_eq!(fondi_istanti(&b, &a), fuso);
    }
}
