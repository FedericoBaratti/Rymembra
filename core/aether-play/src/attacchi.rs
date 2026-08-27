//! Dove il suono comincia: gli attacchi di un brano, in millisecondi.
//!
//! Serve a una cosa sola, ed è meglio dirla subito: **raddrizzare le battute di
//! chi sincronizza un testo a mano**. Premere un tasto a ogni riga mentre la
//! canzone suona è il modo più veloce di farlo, e produce tempi sistematicamente
//! in ritardo di due-tre decimi di secondo — quel che passa fra l'orecchio e il
//! dito. Il suono però sa già dov'era l'inizio: sono questi.
//!
//! La correzione vera la fa [`aether_domain::testo::aggancia`], che è pura e si
//! prova senza aprire un file. Qui si produce solo l'elenco dei candidati.
//!
//! # Perché sta in `aether-play` e non altrove
//!
//! Perché i due pezzi che servono sono già qui e sono già provati: il
//! decodificatore, che sa aprire tutti i formati che l'app sa suonare, e la
//! trasformata di [`crate::spettro`], che è scritta a mano proprio per non
//! portarsi dietro un albero di dipendenze. Un crate nuovo per un flusso
//! spettrale vorrebbe dire una seconda copia di entrambi.
//!
//! # Il flusso spettrale, e perché a banda stretta
//!
//! Un attacco è un istante in cui l'energia di *nuove* frequenze compare. Si
//! misura confrontando lo spettro di una finestra con quello della precedente e
//! sommando **solo gli aumenti**: i cali sono note che finiscono, e una nota
//! che finisce non è un inizio.
//!
//! La somma però non copre tutto lo spettro, ma solo da circa 300 a 4000 Hz. È
//! la banda in cui sta la voce, ed è una scelta che guarda al mestiere: senza,
//! la cassa e il rullante — che sono l'energia più forte del disco — coprirebbero
//! ogni consonante, e le battute finirebbero agganciate al ritmo invece che
//! alle parole. Con il taglio, gli attacchi che restano sono quasi tutti
//! sillabe.

use aether_domain::errors::AppError;

use crate::decodifica::{Decodificatore, Sorgente};
use crate::spettro::{C, rovescia, trasforma};

/// La frequenza a cui si analizza.
///
/// 22 050 Hz, la metà del solito. Le frequenze sopra gli 11 kHz non dicono
/// niente sull'inizio di una sillaba e costerebbero il doppio del lavoro su un
/// file che va decodificato per intero.
pub const FREQUENZA: u32 = 22_050;

/// Quanti campioni entrano in una finestra di analisi.
///
/// 1024 a 22 050 Hz sono 46 ms: abbastanza lunghi da avere una risoluzione in
/// frequenza utile (21,5 Hz per bin), abbastanza corti da non spalmare un
/// attacco su un quarto di secondo.
pub const FINESTRA: usize = 1024;

/// Di quanto si avanza fra una finestra e la successiva.
///
/// Mezza finestra, cioè 23 ms. È anche la **precisione** dell'elenco che ne
/// esce, e va confrontata con la finestra di aggancio del dominio: 250 ms. Un
/// passo più fine costerebbe il doppio delle trasformate per una precisione che
/// sparisce sotto l'errore della mano.
pub const PASSO: usize = 512;

/// Il primo bin che si guarda: 300 Hz, cioè `300 / (22050 / 1024)` ≈ 14.
const BIN_BASSO: usize = 14;
/// L'ultimo: 4000 Hz, cioè `4000 / (22050 / 1024)` ≈ 186.
const BIN_ALTO: usize = 186;

/// Quante finestre intorno definiscono la soglia locale.
///
/// Venti per lato, cioè poco meno di mezzo secondo. Una soglia globale non
/// funzionerebbe: fra la strofa sussurrata e il ritornello pieno ci sono venti
/// decibel, e una soglia sola o perde tutta la strofa o prende ogni fruscio del
/// ritornello.
const INTORNO: usize = 20;

/// Di quanto il flusso deve superare la media locale per essere un attacco.
///
/// Una volta e mezza. Sotto, si prende il respiro fra le parole; sopra, si
/// perdono le sillabe morbide — che sono quelle su cui la mano sbaglia di più,
/// cioè proprio quelle per cui questo elenco esiste.
const QUANTO_SOPRA: f32 = 1.5;

/// Quanto vicini possono stare due attacchi.
///
/// Cinquanta millisecondi. È più corto della sillaba più veloce che qualcuno
/// canti, e serve a non registrare due volte lo stesso attacco quando il flusso
/// resta alto per due finestre di fila.
const DISTANZA_MINIMA_MS: u32 = 50;

/// Gli attacchi di un brano, in millisecondi dall'inizio, in ordine.
///
/// Decodifica il file **per intero**: su un brano di quattro minuti sono uno o
/// due secondi di lavoro, e va fatto su un filo di sottofondo. Non si conserva
/// niente — l'elenco serve finché l'editor di sincronizzazione è aperto, e
/// ricalcolarlo costa meno che tenerne una copia da invalidare.
///
/// # Errori
///
/// Quelli del decodificatore: `playback.formatUnsupported` per un formato che
/// non si sa aprire, `playback.decodeFailed` per un file rovinato.
pub fn attacchi(sorgente: Sorgente) -> Result<Vec<u32>, AppError> {
    let flusso = flusso_spettrale(sorgente)?;
    Ok(picchi(&flusso))
}

/// Il flusso spettrale finestra per finestra.
fn flusso_spettrale(sorgente: Sorgente) -> Result<Vec<f32>, AppError> {
    let mut decodificatore = Decodificatore::apri(sorgente, FREQUENZA, 1)?;

    let bit = FINESTRA.trailing_zeros();
    let ordine: Vec<usize> = (0..FINESTRA).map(|i| rovescia(i, bit)).collect();
    #[expect(
        clippy::cast_precision_loss,
        reason = "FINESTRA è 1024: l'indice sta in f64 senza perdere niente"
    )]
    let hann: Vec<f32> = (0..FINESTRA)
        .map(|i| {
            let x = 2.0 * std::f64::consts::PI * i as f64 / FINESTRA as f64;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "la finestra di Hann sta in [0, 1] per costruzione"
            )]
            let v = (0.5 - 0.5 * x.cos()) as f32;
            v
        })
        .collect();

    // La finestra scorrevole come anello, esattamente come fa lo spettro: così
    // avanzare di PASSO campioni costa PASSO scritture invece di una copia di
    // tutta la finestra.
    let mut anello = vec![0.0_f32; FINESTRA];
    let mut cursore = 0_usize;
    let mut visti = 0_usize;
    let mut dall_ultima = 0_usize;

    let mut lavoro = vec![C::default(); FINESTRA];
    let mut precedenti = vec![0.0_f32; FINESTRA];
    let mut correnti = vec![0.0_f32; FINESTRA];
    let mut flusso: Vec<f32> = Vec::new();
    let mut prima_finestra = true;

    let mut blocco: Vec<f32> = Vec::with_capacity(4096);
    while decodificatore.prossimo(&mut blocco)? {
        for campione in &blocco {
            if let Some(posto) = anello.get_mut(cursore) {
                *posto = *campione;
            }
            cursore = cursore.saturating_add(1) % FINESTRA;
            visti = visti.saturating_add(1);
            dall_ultima = dall_ultima.saturating_add(1);

            if visti < FINESTRA || dall_ultima < PASSO {
                continue;
            }
            dall_ultima = 0;

            // Finestratura e rimescolamento dei bit in un passaggio solo: si
            // legge l'anello dal più vecchio, cioè dal cursore.
            for (posto, i) in lavoro.iter_mut().zip(0..FINESTRA) {
                let da = ordine.get(i).copied().unwrap_or(0);
                let campione = anello
                    .get(cursore.saturating_add(da) % FINESTRA)
                    .copied()
                    .unwrap_or(0.0);
                let peso = hann.get(da).copied().unwrap_or(0.0);
                *posto = C::nuovo(campione * peso, 0.0);
            }
            trasforma(&mut lavoro);

            for (bin, valore) in correnti.iter_mut().enumerate() {
                *valore = lavoro.get(bin).map_or(0.0, |c| c.potenza().sqrt());
            }

            if prima_finestra {
                prima_finestra = false;
            } else {
                // Solo gli aumenti, e solo nella banda della voce: la ragione
                // per entrambe le restrizioni sta in testa al modulo.
                let mut somma = 0.0_f32;
                for bin in BIN_BASSO..=BIN_ALTO.min(FINESTRA.saturating_sub(1)) {
                    let ora = correnti.get(bin).copied().unwrap_or(0.0);
                    let prima = precedenti.get(bin).copied().unwrap_or(0.0);
                    let salita = ora - prima;
                    if salita > 0.0 {
                        somma += salita;
                    }
                }
                flusso.push(somma);
            }
            std::mem::swap(&mut precedenti, &mut correnti);
        }
    }
    Ok(flusso)
}

/// I millesimi a cui corrisponde una finestra.
///
/// La finestra `n` copre i campioni che finiscono a `(n + 1) · PASSO`, ma
/// l'attacco che ha rilevato sta al **suo inizio**: il flusso confronta questa
/// finestra con la precedente, e quel che è cambiato è cambiato lì. Da qui il
/// `+ 1` e non un `+ 2`, che sposterebbe ogni attacco in avanti di 23 ms.
#[expect(
    clippy::integer_division,
    reason = "convertire dei campioni in millesimi è una divisione per la frequenza"
)]
fn quando(finestra: usize) -> u32 {
    let campioni = finestra.saturating_add(1).saturating_mul(PASSO);
    let ms = (campioni as u64).saturating_mul(1000) / u64::from(FREQUENZA);
    u32::try_from(ms).unwrap_or(u32::MAX)
}

/// Le finestre in cui il flusso supera la soglia locale ed è un massimo.
///
/// Tre condizioni insieme, e servono tutt'e tre:
///
/// * **sopra la media locale** di [`QUANTO_SOPRA`], che è la soglia adattiva;
/// * **massimo fra i vicini**, perché un attacco dura più di una finestra e
///   senza questa condizione se ne registrerebbero tre di fila;
/// * **lontano dal precedente** almeno [`DISTANZA_MINIMA_MS`].
fn picchi(flusso: &[f32]) -> Vec<u32> {
    let mut fuori: Vec<u32> = Vec::new();
    let mut ultimo: Option<u32> = None;

    for (indice, valore) in flusso.iter().enumerate() {
        if *valore <= 0.0 {
            continue;
        }
        let da = indice.saturating_sub(INTORNO);
        let a = indice.saturating_add(INTORNO).min(flusso.len());
        let Some(intorno) = flusso.get(da..a).filter(|f| !f.is_empty()) else {
            continue;
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "il numero di finestre di un brano sta in f32 con enorme margine"
        )]
        let quante = intorno.len() as f32;
        let media = intorno.iter().sum::<f32>() / quante;
        if *valore < media * QUANTO_SOPRA {
            continue;
        }
        // Massimo locale stretto: tre finestre per lato, cioè 70 ms.
        let da = indice.saturating_sub(3);
        let a = indice.saturating_add(4).min(flusso.len());
        let e_massimo = flusso
            .get(da..a)
            .is_some_and(|vicini| vicini.iter().all(|v| v <= valore));
        if !e_massimo {
            continue;
        }
        let ms = quando(indice);
        if ultimo.is_some_and(|prima| ms.saturating_sub(prima) < DISTANZA_MINIMA_MS) {
            continue;
        }
        ultimo = Some(ms);
        fuori.push(ms);
    }
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Il flusso che produrrebbe un brano con degli impulsi ai punti dati.
    ///
    /// Si prova il rilevatore di picchi contro un flusso costruito, invece che
    /// contro un file finto: la trasformata ha già le sue prove in `spettro`, e
    /// quel che qui può sbagliare è la scelta dei massimi.
    fn flusso_con_impulsi(quante: usize, dove: &[usize]) -> Vec<f32> {
        let mut flusso = vec![0.05_f32; quante];
        for indice in dove {
            if let Some(posto) = flusso.get_mut(*indice) {
                *posto = 3.0;
            }
        }
        flusso
    }

    #[test]
    fn un_impulso_diventa_un_attacco() {
        let flusso = flusso_con_impulsi(200, &[50, 120]);
        let trovati = picchi(&flusso);
        assert_eq!(trovati, vec![quando(50), quando(120)]);
    }

    #[test]
    fn il_silenzio_non_accende_niente() {
        assert!(picchi(&vec![0.0_f32; 500]).is_empty());
        assert!(picchi(&[]).is_empty());
    }

    #[test]
    fn un_livello_costante_non_e_un_attacco() {
        // Il flusso alto e piatto è un disco forte, non un inizio: la soglia è
        // relativa alla media locale proprio per questo.
        assert!(picchi(&vec![2.0_f32; 500]).is_empty());
    }

    #[test]
    fn due_finestre_di_fila_sono_un_attacco_solo() {
        let mut flusso = vec![0.05_f32; 200];
        for indice in 50..53 {
            if let Some(posto) = flusso.get_mut(indice) {
                *posto = 3.0;
            }
        }
        assert_eq!(picchi(&flusso).len(), 1);
    }

    #[test]
    fn i_millesimi_partono_dall_inizio_della_finestra() {
        // La finestra 0 finisce a 512 campioni, cioè 23 ms a 22 050 Hz.
        assert_eq!(quando(0), 23);
        // (43 + 1) · 512 = 22 528 campioni, cioè 1021,67 ms troncati.
        assert_eq!(quando(43), 1_021);
        // Un brano di quattro minuti sono circa diecimila finestre.
        assert!(quando(10_000) > 230_000);
    }

    #[test]
    fn la_soglia_segue_il_brano() {
        // Un impulso dentro una strofa piano e uno dentro un ritornello forte:
        // una soglia globale ne prenderebbe uno solo.
        let mut flusso = vec![0.02_f32; 400];
        for (indice, posto) in flusso.iter_mut().enumerate() {
            if indice >= 200 {
                *posto = 1.0;
            }
        }
        if let Some(posto) = flusso.get_mut(100) {
            *posto = 0.5;
        }
        if let Some(posto) = flusso.get_mut(300) {
            *posto = 8.0;
        }
        let trovati = picchi(&flusso);
        assert!(
            trovati.contains(&quando(100)),
            "la strofa piano: {trovati:?}"
        );
        assert!(trovati.contains(&quando(300)), "il ritornello: {trovati:?}");
    }
}
