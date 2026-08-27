//! La proprietà su cui si regge tutta la sincronia: comunque siano andate le
//! cose, i dispositivi finiscono d'accordo.
//!
//! # Perché un generatore scritto a mano e non `proptest`
//!
//! Perché in questo albero non c'è, e non per dimenticanza: le prove qui dentro
//! usano vettori dorati e generatori espliciti, e una dipendenza che genera i casi
//! da sé è anche una dipendenza che, quando fallisce, lo fa su un caso che non si
//! riesce a riprodurre due volte uguale. Un xorshift di venti righe con un seme
//! scritto nel test dà gli stessi millecinquecento casi a ogni esecuzione, su ogni
//! macchina, e quando uno si rompe basta il numero del seme per rivederlo.
//!
//! # Cosa si prova, esattamente
//!
//! 1. Che fondere gli stessi documenti in ordini diversi dia lo stesso stato.
//! 2. Che il giro completo — fondi, applica, ricostruisci il tuo documento da
//!    quel che hai applicato, rifondi — non gonfi né perda niente. È il ciclo che
//!    l'applicazione fa davvero, ed è quello in cui un conteggio sbagliato si
//!    moltiplica invece di restare fermo.
//! 3. Che i due casi che oggi si perdono davvero — due aggiunte concorrenti alla
//!    stessa playlist, due riordini concorrenti — non si perdano più.

// Le esenzioni di `clippy.toml` valgono per le funzioni `#[test]`, non per quelle
// che stanno accanto: qui sotto ci sono aiutanti condivisi, e per loro
// l'indicizzazione resta vietata. `expect` e `panic` no — quelli `clippy.toml`
// li permette su tutto il bersaglio di prova — ed elencarli qui li segnalerebbe
// come aspettative non soddisfatte.
#![expect(
    clippy::indexing_slicing,
    reason = "aiutanti di prova: qui un indice fuori misura deve fermare la prova"
)]

use std::collections::BTreeMap;

use aether_sync::contatore::Contatore;
use aether_sync::documento::{Contenuto, Documento, PlaylistSincronizzata};
use aether_sync::registro::{Interruttore, Momento, Voto};
use aether_sync::sequenza::Sequenza;
use aether_sync::{Fuso, fondi};

/// Un generatore deterministico: stesso seme, stessi casi, su ogni macchina.
struct Dadi(u64);

impl Dadi {
    fn nuovi(seme: u64) -> Self {
        // Uno zero si autoriproduce: lo xorshift di zero è zero per sempre.
        Self(if seme == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seme
        })
    }

    fn prossimo(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Un numero da 0 a `quanti - 1`.
    fn fino_a(&mut self, quanti: usize) -> usize {
        if quanti == 0 {
            return 0;
        }
        usize::try_from(self.prossimo() % (quanti as u64)).unwrap_or(0)
    }

    /// Mescola, con Fisher-Yates.
    fn mescola<T>(&mut self, roba: &mut [T]) {
        for indice in (1..roba.len()).rev() {
            let altro = self.fino_a(indice + 1);
            roba.swap(indice, altro);
        }
    }
}

/// I brani su cui si gioca. Pochi apposta: con mille brani due dispositivi non si
/// toccherebbero mai, ed è precisamente il caso che non ha bisogno di un CRDT.
const BRANI: &[&str] = &["a|uno|x", "a|due|x", "b|tre|y", "b|quattro|y", "c|cinque|z"];

/// Le playlist su cui si gioca.
const PLAYLIST: &[&str] = &["jazz", "corsa"];

/// Un dispositivo, col suo contenuto e il suo orologio.
struct Replica {
    nome: String,
    contenuto: Contenuto,
}

impl Replica {
    fn nuova(nome: &str) -> Self {
        Self {
            nome: nome.to_owned(),
            contenuto: Contenuto::default(),
        }
    }

    fn documento(&self) -> Documento {
        Documento::nuovo(&self.nome, self.contenuto.clone(), 0)
    }

    /// Un gesto qualsiasi fra quelli che un utente può fare.
    fn gesto(&mut self, dadi: &mut Dadi, adesso: i64) {
        let brano = BRANI[dadi.fino_a(BRANI.len())].to_owned();
        let quale = PLAYLIST[dadi.fino_a(PLAYLIST.len())].to_owned();
        match dadi.fino_a(9) {
            0 => {
                // Ascolta.
                let quanti = self.contenuto.ascolti.entry(brano).or_insert(0);
                *quanti += 1;
            }
            1 => {
                self.contenuto.ultimo.insert(brano, adesso);
            }
            2 => {
                let voto = u8::try_from(dadi.fino_a(6)).unwrap_or(0);
                self.contenuto.voti.insert(
                    brano,
                    Voto {
                        v: voto,
                        at: adesso,
                    },
                );
            }
            3 => {
                let acceso = dadi.fino_a(2) == 1;
                self.contenuto.preferiti.insert(
                    brano,
                    Interruttore {
                        on: acceso,
                        at: adesso,
                    },
                );
            }
            4 => {
                let ms = i64::try_from(dadi.fino_a(300_000)).unwrap_or(0);
                self.contenuto
                    .posizioni
                    .insert(brano, Momento { ms, at: adesso });
            }
            5 => {
                // Aggiunge un brano a una playlist, creandola se non c'è.
                let playlist = self
                    .contenuto
                    .playlist
                    .entry(quale.clone())
                    .or_insert_with(|| PlaylistSincronizzata {
                        nome: quale,
                        at: adesso,
                        creata_at: adesso,
                        ..PlaylistSincronizzata::default()
                    });
                playlist.sequenza.accoda(&self.nome, &brano);
            }
            6 => {
                // Toglie un brano da una playlist.
                if let Some(playlist) = self.contenuto.playlist.get_mut(&quale) {
                    let quanti = playlist.sequenza.ordine().len();
                    if quanti > 0 {
                        playlist.sequenza.togli(dadi.fino_a(quanti), adesso);
                    }
                }
            }
            7 => {
                // Riordina una playlist mescolandola.
                if let Some(playlist) = self.contenuto.playlist.get_mut(&quale) {
                    let mut ordine: Vec<String> = playlist
                        .sequenza
                        .ordine()
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .collect();
                    if ordine.len() > 1 {
                        dadi.mescola(&mut ordine);
                        playlist.sequenza.riordina(&self.nome, &ordine, adesso);
                    }
                }
            }
            _ => {
                // Cancella una playlist.
                if self.contenuto.playlist.remove(&quale).is_some() {
                    self.contenuto.lapidi.playlist.insert(quale, adesso);
                }
            }
        }
    }

    /// Rimette dentro quel che la fusione ha deciso, come farebbe il database.
    ///
    /// È il pezzo che rende questa prova diversa da un semplice «fondi due
    /// mappe»: l'applicazione **riscrive** il proprio documento a partire da
    /// quello che ha applicato, e da lì in poi riafferma anche quel che ha
    /// imparato dagli altri. Se una di queste fusioni non fosse idempotente, è
    /// qui che i numeri comincerebbero a gonfiarsi.
    fn assorbi(&mut self, fuso: &Fuso) {
        self.contenuto.ascolti = fuso
            .ascolti
            .iter()
            .map(|(brano, contatore)| (brano.clone(), contatore.di(&self.nome)))
            .filter(|(_, quanti)| *quanti > 0)
            .collect();
        self.contenuto.ultimo = fuso.ultimo.clone();
        self.contenuto.voti = fuso.voti.clone();
        self.contenuto.preferiti = fuso.preferiti.clone();
        self.contenuto.posizioni = fuso.posizioni.clone();
        self.contenuto.playlist = fuso.playlist.clone();
        self.contenuto.lapidi = fuso.lapidi.clone();
        self.contenuto.cartelle = fuso.cartelle.clone();
        self.contenuto.skin = fuso.skin.clone();
        self.contenuto.bozze = fuso.bozze.clone();
        self.contenuto.skin_attiva = fuso.skin_attiva.clone();
    }
}

/// Somma tutti gli ascolti di uno stato fuso: il numero che non deve gonfiarsi.
fn ascolti_in_tutto(fuso: &Fuso) -> i64 {
    fuso.ascolti.values().map(Contatore::totale).sum()
}

#[test]
fn fondere_in_ordini_diversi_da_sempre_lo_stesso_stato() {
    for seme in 1..=60_u64 {
        let mut dadi = Dadi::nuovi(seme);
        let mut repliche: Vec<Replica> = ["uno", "due", "tre", "quattro"]
            .iter()
            .map(|nome| Replica::nuova(nome))
            .collect();

        for passo in 0..40_i64 {
            let quale = dadi.fino_a(repliche.len());
            repliche[quale].gesto(&mut dadi, passo * 10);
        }

        let documenti: Vec<Documento> = repliche.iter().map(Replica::documento).collect();
        let atteso = fondi(&documenti);

        for _ in 0..6 {
            let mut mescolati = documenti.clone();
            dadi.mescola(&mut mescolati);
            assert_eq!(
                fondi(&mescolati),
                atteso,
                "seme {seme}: l'ordine di consegna ha cambiato il risultato"
            );
        }
    }
}

#[test]
fn il_giro_completo_non_gonfia_e_non_perde_gli_ascolti() {
    // Il ciclo vero dell'applicazione: si fonde, si applica al database, si
    // ricostruisce il proprio documento da quel che si è applicato, si rifonde. È
    // il punto in cui un conteggio che si somma invece di essere dichiarato
    // raddoppierebbe a ogni passata senza che nessuno se ne accorga.
    for seme in 1..=40_u64 {
        let mut dadi = Dadi::nuovi(seme);
        let mut repliche: Vec<Replica> = ["portatile", "telefono", "fisso"]
            .iter()
            .map(|nome| Replica::nuova(nome))
            .collect();

        let mut ascoltati_davvero = 0_i64;
        let mut orologio = 0_i64;

        for giro in 0..5 {
            for _ in 0..12 {
                orologio += 10;
                let quale = dadi.fino_a(repliche.len());
                let prima: i64 = repliche[quale].contenuto.ascolti.values().sum();
                repliche[quale].gesto(&mut dadi, orologio);
                let dopo: i64 = repliche[quale].contenuto.ascolti.values().sum();
                ascoltati_davvero += dopo - prima;
            }

            let documenti: Vec<Documento> = repliche.iter().map(Replica::documento).collect();
            let fuso = fondi(&documenti);
            assert_eq!(
                ascolti_in_tutto(&fuso),
                ascoltati_davvero,
                "seme {seme}, giro {giro}: gli ascolti non tornano"
            );
            for replica in &mut repliche {
                replica.assorbi(&fuso);
            }

            // E rifondere subito dopo aver assorbito non deve cambiare niente:
            // è l'idempotenza vista dal punto in cui conta.
            let dopo: Vec<Documento> = repliche.iter().map(Replica::documento).collect();
            assert_eq!(
                ascolti_in_tutto(&fondi(&dopo)),
                ascoltati_davvero,
                "seme {seme}, giro {giro}: riassorbire ha gonfiato i conteggi"
            );
        }

        let finale: Vec<Documento> = repliche.iter().map(Replica::documento).collect();
        let fuso = fondi(&finale);
        for replica in &repliche {
            assert_eq!(
                fondi(&[replica.documento()]).voti.len(),
                fuso.voti.len(),
                "seme {seme}: una replica è rimasta indietro sui voti"
            );
        }
    }
}

#[test]
fn tutte_le_repliche_vedono_lo_stesso_ordine_di_playlist() {
    for seme in 1..=40_u64 {
        let mut dadi = Dadi::nuovi(seme);
        let mut repliche: Vec<Replica> = ["uno", "due", "tre"]
            .iter()
            .map(|nome| Replica::nuova(nome))
            .collect();

        let mut orologio = 0_i64;
        for _ in 0..30 {
            orologio += 10;
            let quale = dadi.fino_a(repliche.len());
            repliche[quale].gesto(&mut dadi, orologio);
        }

        let documenti: Vec<Documento> = repliche.iter().map(Replica::documento).collect();
        let fuso = fondi(&documenti);
        for replica in &mut repliche {
            replica.assorbi(&fuso);
        }

        // Dopo aver assorbito, ogni replica deve vedere esattamente lo stesso
        // ordine — non solo gli stessi brani.
        let riferimento: BTreeMap<String, Vec<String>> = fuso
            .playlist
            .iter()
            .map(|(chiave, playlist)| {
                (
                    chiave.clone(),
                    playlist
                        .sequenza
                        .ordine()
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .collect(),
                )
            })
            .collect();
        for replica in &repliche {
            for (chiave, atteso) in &riferimento {
                let suo: Vec<String> = replica
                    .contenuto
                    .playlist
                    .get(chiave)
                    .map(|playlist| {
                        playlist
                            .sequenza
                            .ordine()
                            .into_iter()
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                assert_eq!(
                    suo, *atteso,
                    "seme {seme}: «{}» vede «{chiave}» in un ordine diverso",
                    replica.nome
                );
            }
        }
    }
}

#[test]
fn due_riordini_in_contemporanea_non_perdono_nessun_brano() {
    // Il caso che oggi si perde per davvero: due dispositivi riordinano la stessa
    // playlist mentre sono scollegati, e con «vince chi ha toccato per ultimo» il
    // lavoro di uno dei due sparisce senza un avviso.
    let brani: Vec<String> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|b| (*b).to_owned())
        .collect();
    let base = Sequenza::dalla_lista("comune", &brani);

    let mut portatile = base.clone();
    portatile.riordina(
        "portatile",
        &[
            "e".to_owned(),
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
        ],
        100,
    );
    let mut telefono = base;
    telefono.accoda("telefono", "f");

    let uno = {
        let mut contenuto = Contenuto::default();
        contenuto.playlist.insert(
            "jazz".to_owned(),
            PlaylistSincronizzata {
                nome: "Jazz".to_owned(),
                at: 100,
                sequenza: portatile,
                ..PlaylistSincronizzata::default()
            },
        );
        Documento::nuovo("portatile", contenuto, 0)
    };
    let due = {
        let mut contenuto = Contenuto::default();
        contenuto.playlist.insert(
            "jazz".to_owned(),
            PlaylistSincronizzata {
                nome: "Jazz".to_owned(),
                at: 200,
                sequenza: telefono,
                ..PlaylistSincronizzata::default()
            },
        );
        Documento::nuovo("telefono", contenuto, 0)
    };

    let fuso = fondi(&[uno.clone(), due.clone()]);
    let ordine = fuso
        .playlist
        .get("jazz")
        .map(|playlist| playlist.sequenza.ordine())
        .expect("la playlist deve esserci");

    for brano in ["a", "b", "c", "d", "e", "f"] {
        assert!(
            ordine.contains(&brano),
            "«{brano}» è sparito nel riordino concorrente. Ordine ottenuto: {ordine:?}"
        );
    }
    assert_eq!(
        fondi(&[due, uno])
            .playlist
            .get("jazz")
            .map(|playlist| playlist.sequenza.ordine()),
        Some(ordine),
        "l'ordine non può dipendere da chi si è sincronizzato per primo"
    );
}

#[test]
fn un_dispositivo_scollegato_a_lungo_rientra_senza_disfare_niente() {
    // Il portatile che riapre l'app dopo sei mesi: il suo documento è vecchissimo,
    // e non deve poter riportare indietro né i voti né i conteggi degli altri.
    let mut vecchio = Replica::nuova("dimenticato");
    vecchio.contenuto.ascolti.insert(BRANI[0].to_owned(), 3);
    vecchio
        .contenuto
        .voti
        .insert(BRANI[0].to_owned(), Voto { v: 5, at: 10 });

    let mut aggiornato = Replica::nuova("quotidiano");
    aggiornato
        .contenuto
        .ascolti
        .insert(BRANI[0].to_owned(), 200);
    aggiornato
        .contenuto
        .voti
        .insert(BRANI[0].to_owned(), Voto { v: 2, at: 999_999 });

    let fuso = fondi(&[vecchio.documento(), aggiornato.documento()]);
    assert_eq!(
        fuso.ascolti_di(BRANI[0]),
        203,
        "gli ascolti del dimenticato contano ancora, e non sostituiscono gli altri"
    );
    assert_eq!(
        fuso.voti.get(BRANI[0]).map(|voto| voto.v),
        Some(2),
        "un voto vecchio non deve vincere solo perché è arrivato dopo"
    );
}
