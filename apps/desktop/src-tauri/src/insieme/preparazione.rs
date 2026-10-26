//! Chi ospita: quali voci della coda preparare per il filo, quando, e quando
//! smettere di provarci.
//!
//! # Il difetto che toglie
//!
//! Preparare una voce di questo PC vuol dire leggerne la riga nel database e
//! chiedere al disco dimensione e data del file (`ponte::voci_di_questo`). Su
//! una coda di mille brani in un NAS raggiunto in Wi-Fi, fatto a ogni
//! pubblicazione nel filo della sessione voleva dire due cose:
//!
//! - **il filo delle ancore fermo sul disco**: nove secondi alla prima
//!   pubblicazione, durante i quali nessuno riceveva un'ancora;
//! - **le voci che non si preparano richieste a ogni giro**, fino a dieci volte
//!   al secondo, ognuna con un filo e il lucchetto della libreria, e una riga
//!   di diario ogni volta: quattrocento righe uguali in mezzo minuto.
//!
//! Adesso le voci si preparano **in blocco, in un filo a parte**, e il filo
//! della sessione chiede solo quelle mai chieste o la cui attesa è finita.
//! [`Preparazione`] tiene, per `uid`, a che punto è ciascuna.
//!
//! # Riprovare, e rinunciare
//!
//! Una voce che non si prepara può essere un guasto che passa (un NAS che si
//! sveglia, il Wi-Fi che si riprende) o un file che non c'è più. La prima si
//! riprova con un'attesa che raddoppia, da [`PRIMA_ATTESA_MS`] fino a
//! [`ATTESA_MASSIMA_MS`], e si rinuncia dopo [`TENTATIVI`] tentativi, un paio
//! di minuti; la seconda si lascia subito. Lo dice il ponte
//! (`ponte::Mancanza`, vedi [`definitiva`]): solo `Irraggiungibile` si
//! riprova. Una voce lasciata resta fuori dallo stato della sessione **per
//! decisione**, ed è contata nel diario e nella riga che lo dice.
//!
//! # L'ordine
//!
//! Prima la voce che suona, poi quelle che vengono dopo, poi quelle prima: è
//! l'ordine in cui chi segue ne avrà bisogno. Le richieste partono a blocchi di
//! [`BLOCCO`], e lo stato si ripubblica a ogni blocco arrivato: la voce che
//! suona arriva a chi segue in decine di millisecondi, non dopo l'ultima delle
//! mille.

use std::collections::{HashMap, HashSet};

use crate::riproduzione::insieme::Mancanza;

/// Quante voci per blocco chiede il filo della sessione al filo che prepara,
/// e quante ne rimanda indietro per volta.
pub const BLOCCO: usize = 25;

/// La prima attesa dopo una voce non preparata: due secondi.
pub const PRIMA_ATTESA_MS: u64 = 2_000;

/// L'attesa più lunga fra due tentativi: un minuto.
pub const ATTESA_MASSIMA_MS: u64 = 60_000;

/// Dopo quanti tentativi non riusciti si rinuncia: sei, cioè due minuti circa
/// (2 + 4 + 8 + 16 + 32 + 60 s).
pub const TENTATIVI: u32 = 6;

/// A che punto è una voce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Punto {
    /// Chiesta, la risposta non è ancora tornata; `volte` sono i tentativi
    /// già falliti prima di questo.
    InCorso { volte: u32 },
    /// Non preparata `volte` volte; si riprova non prima di `riprova_alle`.
    Mancata { volte: u32, riprova_alle: u64 },
    /// Lasciata: non si riprova più in questa sessione.
    Lasciata,
}

/// Vedi il `//!`.
#[derive(Debug, Default)]
pub struct Preparazione {
    punti: HashMap<u64, Punto>,
    /// L'ultimo conto delle voci non mandabili scritto nel diario.
    detto: Option<(usize, usize)>,
}

impl Preparazione {
    /// Le voci da chiedere adesso fra `candidate` (già nell'ordine di
    /// priorità, e senza quelle già preparate): le mai chieste, e le mancate
    /// la cui attesa è finita. Diventano «in corso».
    pub fn da_chiedere(&mut self, candidate: &[u64], adesso: u64) -> Vec<u64> {
        let mut scelte = Vec::new();
        for uid in candidate {
            let pronta = match self.punti.get(uid) {
                None => true,
                Some(Punto::Mancata { riprova_alle, .. }) => *riprova_alle <= adesso,
                Some(Punto::InCorso { .. } | Punto::Lasciata) => false,
            };
            if pronta {
                // Le volte di prima restano: servono all'attesa dopo.
                let volte = self.volte(*uid);
                self.punti.insert(*uid, Punto::InCorso { volte });
                scelte.push(*uid);
            }
        }
        scelte
    }

    /// Quante volte non è riuscita, finora.
    fn volte(&self, uid: u64) -> u32 {
        match self.punti.get(&uid) {
            Some(Punto::Mancata { volte, .. } | Punto::InCorso { volte }) => *volte,
            _ => 0,
        }
    }

    /// Quando scade la prima attesa: allora il filo della sessione rifà il
    /// giro e richiede le voci da riprovare.
    #[must_use]
    pub fn prossimo_tentativo(&self) -> Option<u64> {
        self.punti
            .values()
            .filter_map(|punto| match punto {
                Punto::Mancata { riprova_alle, .. } => Some(*riprova_alle),
                _ => None,
            })
            .min()
    }

    /// La voce è pronta: non serve più seguirla (ce l'ha `Servite`).
    pub fn arrivata(&mut self, uid: u64) {
        self.punti.remove(&uid);
    }

    /// La voce non si è preparata. `definitiva`: chi preparava sa che non
    /// si preparerà mai (il file non c'è), e non si riprova.
    pub fn mancata(&mut self, uid: u64, adesso: u64, definitiva: bool) {
        let volte = self.volte(uid).saturating_add(1);
        let punto = if definitiva || volte >= TENTATIVI {
            Punto::Lasciata
        } else {
            Punto::Mancata {
                volte,
                riprova_alle: adesso.saturating_add(attesa_dopo(volte)),
            }
        };
        self.punti.insert(uid, punto);
    }

    /// Dimentica le voci che non sono più in coda.
    pub fn tieni(&mut self, uids: &HashSet<u64>) {
        self.punti.retain(|uid, _| uids.contains(uid));
    }

    /// Fine della sessione.
    pub fn svuota(&mut self) {
        self.punti.clear();
        self.detto = None;
    }

    /// Fra `uids`, quante non si sono potute mandare: in attesa di un altro
    /// tentativo, e lasciate.
    #[must_use]
    pub fn non_mandabili(&self, uids: &HashSet<u64>) -> (usize, usize) {
        let mut in_attesa = 0_usize;
        let mut lasciate = 0_usize;
        for (uid, punto) in &self.punti {
            if !uids.contains(uid) {
                continue;
            }
            match punto {
                Punto::Mancata { .. } => in_attesa = in_attesa.saturating_add(1),
                Punto::Lasciata => lasciate = lasciate.saturating_add(1),
                // Una voce che si sta riprovando conta ancora fra quelle da
                // riprovare: non è stata mandata.
                Punto::InCorso { volte } if *volte > 0 => {
                    in_attesa = in_attesa.saturating_add(1);
                }
                Punto::InCorso { .. } => {}
            }
        }
        (in_attesa, lasciate)
    }

    /// La riga del diario sulle voci non mandabili, solo quando il conto
    /// cambia: lo stesso conto a ogni pubblicazione è rumore.
    pub fn riga_se_cambia(&mut self, conto: (usize, usize)) -> Option<String> {
        if self.detto == Some(conto) {
            return None;
        }
        let prima = self.detto.replace(conto);
        if prima.is_none() && conto == (0, 0) {
            return None;
        }
        let (in_attesa, lasciate) = conto;
        Some(format!(
            "[insieme] voci della coda non mandabili: {in_attesa} da riprovare, {lasciate} lasciate"
        ))
    }
}

/// Una voce mancata per questo motivo non si preparerà mai, e non si riprova:
/// non è di questo PC, la sua riga non c'è più, o il suo file non c'è. Solo
/// un disco che non ha risposto (un NAS che si sveglia, il Wi-Fi che si
/// riprende) merita un altro tentativo.
#[must_use]
pub const fn definitiva(mancanza: Mancanza) -> bool {
    match mancanza {
        Mancanza::NonDiQuesto | Mancanza::RigaSparita | Mancanza::FileAssente => true,
        Mancanza::Irraggiungibile => false,
    }
}

/// L'attesa dopo `volte` tentativi falliti: 2, 4, 8, … secondi, fino a uno.
#[must_use]
pub fn attesa_dopo(volte: u32) -> u64 {
    let raddoppi = volte.saturating_sub(1).min(16);
    PRIMA_ATTESA_MS
        .saturating_mul(1_u64 << raddoppi)
        .min(ATTESA_MASSIMA_MS)
}

/// Le voci di una coda nell'ordine in cui servono: la corrente, poi quelle
/// dopo, poi quelle prima (le più vicine per prime).
#[must_use]
pub fn in_ordine(coda: &[u64], corrente: Option<u64>) -> Vec<u64> {
    let posto = corrente
        .and_then(|c| coda.iter().position(|u| *u == c))
        .unwrap_or(0);
    let (prima, dopo) = coda.split_at(posto.min(coda.len()));
    dopo.iter().chain(prima.iter().rev()).copied().collect()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_corrente_prima_poi_quelle_dopo_poi_quelle_prima() {
        assert_eq!(in_ordine(&[1, 2, 3, 4, 5], Some(3)), vec![3, 4, 5, 2, 1]);
        assert_eq!(in_ordine(&[1, 2, 3], None), vec![1, 2, 3]);
        assert_eq!(in_ordine(&[1, 2, 3], Some(9)), vec![1, 2, 3]);
        assert!(in_ordine(&[], Some(1)).is_empty());
    }

    #[test]
    fn una_voce_si_chiede_una_volta_e_poi_aspetta_la_risposta() {
        let mut p = Preparazione::default();
        let coda: Vec<u64> = (1..=60).collect();
        let prime = p.da_chiedere(&coda, 0);
        assert_eq!(prime, coda, "tutte, in blocco e nell'ordine dato");
        assert!(
            p.da_chiedere(&coda, 10).is_empty(),
            "in corso: non si richiedono"
        );
        // Una voce arrivata esce dalle candidate (ce l'ha `Servite`), e le
        // altre restano in corso.
        p.arrivata(5);
        let senza_la_5: Vec<u64> = coda.iter().copied().filter(|u| *u != 5).collect();
        assert!(p.da_chiedere(&senza_la_5, 20).is_empty());
        assert_eq!(p.non_mandabili(&coda.iter().copied().collect()), (0, 0));
    }

    #[test]
    fn una_voce_mancata_si_riprova_con_un_attesa_che_raddoppia_e_poi_si_lascia() {
        let mut p = Preparazione::default();
        let mut adesso = 0_u64;
        assert_eq!(p.da_chiedere(&[7], adesso), vec![7]);
        let mut attese = Vec::new();
        for _ in 0..TENTATIVI {
            p.mancata(7, adesso, false);
            let mut quando = adesso;
            // Prima dell'attesa non si richiede, a nessun giro.
            while p.da_chiedere(&[7], quando).is_empty() {
                quando += 500;
                if quando > adesso + 10 * ATTESA_MASSIMA_MS {
                    break;
                }
            }
            if quando > adesso + 10 * ATTESA_MASSIMA_MS {
                break;
            }
            attese.push(quando - adesso);
            adesso = quando;
        }
        assert_eq!(attese, vec![2_000, 4_000, 8_000, 16_000, 32_000]);
        // Il sesto tentativo fallito lascia la voce: non si chiede più.
        assert!(
            p.da_chiedere(&[7], adesso + 100 * ATTESA_MASSIMA_MS)
                .is_empty()
        );
        assert_eq!(p.non_mandabili(&HashSet::from([7])), (0, 1));
    }

    #[test]
    fn il_prossimo_tentativo_e_quello_dell_attesa_piu_corta() {
        let mut p = Preparazione::default();
        assert_eq!(p.prossimo_tentativo(), None);
        p.da_chiedere(&[1, 2], 0);
        p.mancata(1, 1_000, false);
        p.mancata(2, 0, false);
        p.mancata(2, 0, false);
        assert_eq!(p.prossimo_tentativo(), Some(1_000 + PRIMA_ATTESA_MS));
    }

    #[test]
    fn un_file_che_non_c_e_si_lascia_subito_un_disco_che_tace_si_riprova() {
        for (mancanza, lasciata) in [
            (Mancanza::FileAssente, true),
            (Mancanza::RigaSparita, true),
            (Mancanza::NonDiQuesto, true),
            (Mancanza::Irraggiungibile, false),
        ] {
            let mut p = Preparazione::default();
            p.da_chiedere(&[3], 0);
            p.mancata(3, 0, definitiva(mancanza));
            let atteso = if lasciata { (0, 1) } else { (1, 0) };
            assert_eq!(p.non_mandabili(&HashSet::from([3])), atteso, "{mancanza:?}");
            assert_eq!(
                p.da_chiedere(&[3], PRIMA_ATTESA_MS).is_empty(),
                lasciata,
                "{mancanza:?}: dopo la prima attesa si richiede solo se si riprova"
            );
        }
    }

    #[test]
    fn le_voci_uscite_dalla_coda_si_dimenticano() {
        let mut p = Preparazione::default();
        p.da_chiedere(&[1, 2], 0);
        p.mancata(1, 0, false);
        p.mancata(2, 0, true);
        p.tieni(&HashSet::from([2]));
        assert_eq!(p.non_mandabili(&HashSet::from([1, 2])), (0, 1));
        // Tornata in coda dopo essere uscita, la 1 si richiede subito.
        assert_eq!(p.da_chiedere(&[1], 1), vec![1]);
    }

    #[test]
    fn il_diario_dice_il_conto_solo_quando_cambia() {
        let mut p = Preparazione::default();
        assert_eq!(p.riga_se_cambia((0, 0)), None, "niente da dire all'inizio");
        let prima = p.riga_se_cambia((38, 0)).expect("cambia");
        assert!(prima.contains("38 da riprovare"));
        for _ in 0..400 {
            assert_eq!(p.riga_se_cambia((38, 0)), None);
        }
        assert!(p.riga_se_cambia((15, 23)).is_some());
        assert_eq!(p.riga_se_cambia((15, 23)), None);
        assert!(
            p.riga_se_cambia((0, 0)).is_some(),
            "anche quando torna a zero"
        );
    }
}
