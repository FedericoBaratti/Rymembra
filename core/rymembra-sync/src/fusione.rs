//! Da N documenti a uno stato solo.
//!
//! # Perché gli ascolti non passano da `Contenuto::fondi`
//!
//! Perché sono l'unica cosa qui dentro che ha bisogno di sapere **da chi** viene.
//! Tutto il resto — voti, preferiti, posizioni, playlist — si fonde a due a due
//! senza guardare in faccia nessuno, e fonderlo in fila o ad albero dà lo stesso
//! risultato. Un conteggio invece va tenuto separato per dispositivo fino alla
//! fine, o la somma non si può più fare: sommando via via si raddoppierebbe a
//! ogni passata, prendendo il massimo via via si perderebbe.
//!
//! Da qui la forma di questo modulo: una piega sola su tutti i documenti, in cui
//! gli ascolti si raccolgono per dispositivo e il resto si fonde.
//!
//! # Perché l'ordine dei documenti non conta
//!
//! Ogni fusione qui dentro è commutativa, associativa e idempotente — sono le tre
//! proprietà che i test di `registro`, `contatore` e `sequenza` verificano una per
//! una. Ne segue che [`fondi`] dà lo stesso risultato comunque siano ordinati i
//! documenti, comunque siano arrivati e quante volte lo si rifaccia. È l'unica
//! ragione per cui una sincronia senza server può funzionare, e un test lo
//! sorveglia mescolando l'ordine.

use std::collections::{BTreeMap, BTreeSet};

use crate::contatore::Contatore;
use crate::documento::{Documento, Lapidi, PlaylistSincronizzata};
use crate::registro::{Interruttore, Momento, Registro, Scelta, Voto};

/// Lo stato che esce dalla fusione: quel che tutti i dispositivi sanno, insieme.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fuso {
    /// Gli ascolti, ripartiti per dispositivo. Il totale è `.totale()`.
    pub ascolti: BTreeMap<String, Contatore>,
    /// L'ultimo ascolto di ciascun brano.
    pub ultimo: BTreeMap<String, i64>,
    /// I voti.
    pub voti: BTreeMap<String, Voto>,
    /// I preferiti.
    pub preferiti: BTreeMap<String, Interruttore>,
    /// Dove si era arrivati, per i brani lasciati a metà.
    pub posizioni: BTreeMap<String, Momento>,
    /// Le playlist ancora vive: quelle cancellate sono già state tolte.
    pub playlist: BTreeMap<String, PlaylistSincronizzata>,
    /// Le cancellazioni, che continuano a viaggiare anche dopo essere state applicate.
    pub lapidi: Lapidi,
    /// Le cartelle sorvegliate, accese e spente.
    pub cartelle: BTreeMap<String, Interruttore>,
    /// Le skin installate da qualche parte: identificativo → impronta.
    pub skin: BTreeMap<String, String>,
    /// Le bozze dello Studio: identificativo → impronta.
    pub bozze: BTreeMap<String, String>,
    /// La skin attiva.
    pub skin_attiva: Option<Scelta>,
    /// I dispositivi che hanno contribuito, in ordine.
    pub dispositivi: Vec<String>,
}

impl Fuso {
    /// Il conteggio d'ascolto di un brano, sommato fra tutti i dispositivi.
    #[must_use]
    pub fn ascolti_di(&self, brano: &str) -> i64 {
        self.ascolti.get(brano).map_or(0, Contatore::totale)
    }

    /// Tutti i conteggi, già sommati: la forma che il database vuole.
    #[must_use]
    pub fn ascolti_totali(&self) -> BTreeMap<String, i64> {
        self.ascolti
            .iter()
            .map(|(brano, contatore)| (brano.clone(), contatore.totale()))
            .collect()
    }

    /// Le cartelle da sorvegliare adesso: quelle accese, in ordine.
    #[must_use]
    pub fn cartelle_accese(&self) -> Vec<&str> {
        self.cartelle
            .iter()
            .filter(|(_, stato)| stato.on)
            .map(|(percorso, _)| percorso.as_str())
            .collect()
    }
}

/// Fonde tutti i documenti in uno stato solo.
///
/// L'ordine dei documenti non cambia il risultato: è la proprietà che rende
/// sicuro chiamarla senza sapere chi si è sincronizzato prima.
///
/// Le cancellazioni delle playlist vengono applicate qui, e quelle dei brani no.
/// Non è un'asimmetria distratta: è la stessa regola di `backup.rs`, e la ragione
/// è che una riga di brano non la crea mai una sincronia — la crea la scansione,
/// leggendo un file che c'è. Una lapide di brano potrebbe quindi solo buttare via
/// una storia d'ascolto senza togliere niente in cambio.
#[must_use]
pub fn fondi(documenti: &[Documento]) -> Fuso {
    let mut fuso = Fuso::default();
    let mut visti: BTreeSet<String> = BTreeSet::new();

    let mut contenuto_comune = crate::documento::Contenuto::default();
    for documento in documenti {
        // Un documento senza mittente non può contribuire agli ascolti: il suo
        // conteggio non si saprebbe a chi attribuire, e attribuirlo a «» vorrebbe
        // dire che tutti i documenti anonimi si sovrascrivono a vicenda.
        if !documento.device.is_empty() {
            visti.insert(documento.device.clone());
            for (brano, quanti) in &documento.content.ascolti {
                fuso.ascolti
                    .entry(brano.clone())
                    .or_insert_with(Contatore::nuovo)
                    .segna(&documento.device, *quanti);
            }
        }
        // Lo storico ereditato viaggia invece anche senza mittente: appartiene a
        // `importazione`, che è lo stesso pseudo-dispositivo ovunque, e `segna`
        // tiene il massimo. Due dispositivi che hanno importato lo stesso vecchio
        // database convergono così sul numero che c'era, non sul suo doppio.
        for (brano, quanti) in &documento.content.ereditati {
            fuso.ascolti
                .entry(brano.clone())
                .or_insert_with(Contatore::nuovo)
                .segna(crate::contatore::IMPORTAZIONE, *quanti);
        }
        contenuto_comune = contenuto_comune.fondi(&documento.content);
    }

    fuso.ultimo = contenuto_comune.ultimo;
    fuso.voti = contenuto_comune.voti;
    fuso.preferiti = contenuto_comune.preferiti;
    fuso.posizioni = contenuto_comune.posizioni;
    fuso.cartelle = contenuto_comune.cartelle;
    fuso.skin = contenuto_comune.skin;
    fuso.bozze = contenuto_comune.bozze;
    fuso.skin_attiva = contenuto_comune.skin_attiva;
    fuso.lapidi = contenuto_comune.lapidi;
    fuso.dispositivi = visti.into_iter().collect();

    fuso.playlist = contenuto_comune
        .playlist
        .into_iter()
        .filter(|(chiave, playlist)| {
            // Una playlist rifatta dopo essere stata cancellata sopravvive: la
            // lapide dice quando è stata tolta, e `at` quando è stata toccata.
            // Senza questo confronto, ricreare una playlist con lo stesso nome su
            // un dispositivo la vedrebbe sparire di nuovo alla prima passata.
            fuso.lapidi
                .playlist
                .get(chiave)
                .is_none_or(|tolta| playlist.at > *tolta)
        })
        .collect();

    fuso
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::documento::Contenuto;
    use crate::sequenza::Sequenza;

    fn documento(dispositivo: &str, riempi: impl FnOnce(&mut Contenuto)) -> Documento {
        let mut contenuto = Contenuto::default();
        riempi(&mut contenuto);
        Documento::nuovo(dispositivo, contenuto, 1_000)
    }

    #[test]
    fn gli_ascolti_di_tre_dispositivi_si_sommano() {
        let documenti = vec![
            documento("portatile", |c| {
                c.ascolti.insert("so-what".to_owned(), 5);
            }),
            documento("telefono", |c| {
                c.ascolti.insert("so-what".to_owned(), 3);
            }),
            documento("fisso", |c| {
                c.ascolti.insert("so-what".to_owned(), 2);
            }),
        ];
        let fuso = fondi(&documenti);
        assert_eq!(fuso.ascolti_di("so-what"), 10);
        assert_eq!(fuso.dispositivi.len(), 3);
    }

    #[test]
    fn l_ordine_dei_documenti_non_cambia_niente() {
        // La proprietà su cui si regge tutto: nessuno sa chi si sincronizza per
        // primo, e il risultato non deve dipenderne.
        let uno = documento("uno", |c| {
            c.ascolti.insert("a".to_owned(), 4);
            c.voti.insert("a".to_owned(), Voto { v: 5, at: 10 });
            c.preferiti
                .insert("a".to_owned(), Interruttore { on: true, at: 30 });
        });
        let due = documento("due", |c| {
            c.ascolti.insert("a".to_owned(), 7);
            c.voti.insert("a".to_owned(), Voto { v: 2, at: 20 });
            c.preferiti
                .insert("a".to_owned(), Interruttore { on: false, at: 40 });
        });
        let tre = documento("tre", |c| {
            c.ascolti.insert("a".to_owned(), 1);
            c.voti.insert("a".to_owned(), Voto { v: 3, at: 15 });
        });

        let dritto = fondi(&[uno.clone(), due.clone(), tre.clone()]);
        let rovescio = fondi(&[tre.clone(), due.clone(), uno.clone()]);
        let misto = fondi(&[due, uno, tre]);
        assert_eq!(dritto, rovescio);
        assert_eq!(dritto, misto);
        assert_eq!(dritto.ascolti_di("a"), 12);
        assert_eq!(dritto.voti.get("a").map(|v| v.v), Some(2));
        assert_eq!(dritto.preferiti.get("a").map(|p| p.on), Some(false));
    }

    #[test]
    fn rifondere_gli_stessi_documenti_non_cambia_niente() {
        let documenti = vec![
            documento("uno", |c| {
                c.ascolti.insert("a".to_owned(), 4);
            }),
            documento("due", |c| {
                c.ascolti.insert("a".to_owned(), 7);
            }),
        ];
        let una = fondi(&documenti);
        let due = fondi(&documenti);
        assert_eq!(una, due);
        assert_eq!(una.ascolti_di("a"), 11);
    }

    #[test]
    fn una_playlist_cancellata_non_torna_indietro() {
        let mut chi_cancella = documento("uno", |c| {
            c.lapidi.playlist.insert("jazz".to_owned(), 500);
        });
        chi_cancella.content.playlist.remove("jazz");

        let chi_non_sa = documento("due", |c| {
            c.playlist.insert(
                "jazz".to_owned(),
                PlaylistSincronizzata {
                    nome: "Jazz".to_owned(),
                    at: 100,
                    ..PlaylistSincronizzata::default()
                },
            );
        });

        let fuso = fondi(&[chi_cancella, chi_non_sa]);
        assert!(
            !fuso.playlist.contains_key("jazz"),
            "la playlist cancellata è tornata indietro dall'altro dispositivo"
        );
    }

    #[test]
    fn una_playlist_rifatta_dopo_la_cancellazione_sopravvive() {
        let chi_cancella = documento("uno", |c| {
            c.lapidi.playlist.insert("jazz".to_owned(), 500);
        });
        let chi_la_rifa = documento("due", |c| {
            c.playlist.insert(
                "jazz".to_owned(),
                PlaylistSincronizzata {
                    nome: "Jazz".to_owned(),
                    at: 900,
                    ..PlaylistSincronizzata::default()
                },
            );
        });
        let fuso = fondi(&[chi_cancella, chi_la_rifa]);
        assert!(
            fuso.playlist.contains_key("jazz"),
            "rifarla dopo averla cancellata deve valere"
        );
    }

    #[test]
    fn due_aggiunte_concorrenti_alla_stessa_playlist_sopravvivono_entrambe() {
        // Il caso per cui questo crate esiste, dal principio alla fine.
        let base = Sequenza::dalla_lista("comune", &["a".to_owned()]);

        let mut sua = base.clone();
        sua.accoda("telefono", "blue-in-green");
        let mut mia = base;
        mia.accoda("portatile", "so-what");

        let uno = documento("portatile", |c| {
            c.playlist.insert(
                "jazz".to_owned(),
                PlaylistSincronizzata {
                    nome: "Jazz".to_owned(),
                    at: 10,
                    sequenza: mia,
                    ..PlaylistSincronizzata::default()
                },
            );
        });
        let due = documento("telefono", |c| {
            c.playlist.insert(
                "jazz".to_owned(),
                PlaylistSincronizzata {
                    nome: "Jazz".to_owned(),
                    at: 20,
                    sequenza: sua,
                    ..PlaylistSincronizzata::default()
                },
            );
        });

        let fuso = fondi(&[uno, due]);
        let ordine = fuso
            .playlist
            .get("jazz")
            .map(|p| p.sequenza.ordine())
            .unwrap_or_default();
        assert_eq!(ordine.len(), 3, "ordine ottenuto: {ordine:?}");
        assert!(ordine.contains(&"so-what"));
        assert!(ordine.contains(&"blue-in-green"));
    }

    #[test]
    fn un_documento_senza_mittente_non_contribuisce_agli_ascolti() {
        // Attribuirlo a «» vorrebbe dire che tutti gli anonimi si sovrascrivono a
        // vicenda, cioè che un solo documento rotto ne cancella dieci.
        let anonimo = documento("", |c| {
            c.ascolti.insert("a".to_owned(), 999);
            c.voti.insert("a".to_owned(), Voto { v: 4, at: 10 });
        });
        let vero = documento("uno", |c| {
            c.ascolti.insert("a".to_owned(), 3);
        });
        let fuso = fondi(&[anonimo, vero]);
        assert_eq!(fuso.ascolti_di("a"), 3);
        assert_eq!(
            fuso.voti.get("a").map(|v| v.v),
            Some(4),
            "il resto del documento resta comunque buono"
        );
    }

    #[test]
    fn nessun_documento_da_uno_stato_vuoto() {
        let fuso = fondi(&[]);
        assert_eq!(fuso, Fuso::default());
        assert_eq!(fuso.ascolti_di("qualsiasi"), 0);
    }

    #[test]
    fn le_cartelle_accese_escono_in_ordine() {
        let uno = documento("uno", |c| {
            c.cartelle
                .insert("D:/Musica".to_owned(), Interruttore { on: true, at: 10 });
            c.cartelle
                .insert("C:/Vecchia".to_owned(), Interruttore { on: true, at: 10 });
        });
        let due = documento("due", |c| {
            c.cartelle
                .insert("C:/Vecchia".to_owned(), Interruttore { on: false, at: 20 });
        });
        let fuso = fondi(&[uno, due]);
        assert_eq!(fuso.cartelle_accese(), vec!["D:/Musica"]);
    }
}
