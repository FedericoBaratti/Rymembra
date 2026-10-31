//! Il codice di quattro cifre: impegno → nonce → rivelazione.
//!
//! # Perché l'impegno
//!
//! Dopo la stretta Noise i due PC parlano cifrato, ma non sanno ancora se
//! parlano **fra loro**: un terzo nel mezzo può aver fatto due strette, una
//! per parte. Il codice che le due persone confrontano a occhio lega le due
//! strette — dentro c'è l'`handshake_hash`, che con un terzo nel mezzo è
//! diverso sui due lati — e un terzo nel mezzo vince solo se i due codici
//! coincidono per caso.
//!
//! Senza l'impegno il caso lo sceglierebbe lui: vedendo il nonce di una parte
//! prima di mandare il suo all'altra, potrebbe provarne milioni finché i due
//! codici non vengono uguali. Con l'impegno chi chiede l'associazione (il
//! membro) si lega al suo nonce **prima** di vedere quello dell'host, e l'host
//! manda il suo **prima** di vedere quello del membro: nessuno dei due nonce
//! si può scegliere guardando l'altro, e al terzo nel mezzo resta una
//! probabilità su diecimila a tentativo. È lo schema del confronto numerico
//! di Bluetooth.
//!
//! # Perché i tentativi si contano
//!
//! Una su diecimila a tentativo vuol dire che diecimila tentativi bastano.
//! [`Tentativi`] ne lascia tre per chiave ogni dieci minuti, e dieci in tutto
//! (un terzo nel mezzo può cambiare chiave a ogni tentativo, il tetto totale
//! no): a quel ritmo, uno al minuto, servirebbe circa una settimana di
//! tentativi ininterrotti (diecimila minuti; uno su due ci riesce già in
//! circa cinque giorni), ciascuno con una persona che guarda due schermi e
//! dice di no. Il freno vero è lei; i tetti fanno sì che non basti un
//! pomeriggio. Fino alla revisione del 30 ottobre qui c'era scritto «mesi».

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::identita::ChiavePubblica;

/// Il contesto blake3 dell'impegno. Contesti diversi per usi diversi: un
/// impegno non può mai valere come codice, né il contrario.
const CONTESTO_IMPEGNO: &str = "Rymembra insieme 2026-10-23 impegno del membro";

/// Il contesto blake3 del codice.
const CONTESTO_CODICE: &str = "Rymembra insieme 2026-10-23 codice di quattro cifre";

/// Le due chiavi in ordine di byte: la stessa coppia su tutti e due i PC,
/// qualunque sia «la mia» e «la tua».
fn in_ordine<'a>(a: &'a ChiavePubblica, b: &'a ChiavePubblica) -> (&'a [u8; 32], &'a [u8; 32]) {
    if a.0 <= b.0 {
        (&a.0, &b.0)
    } else {
        (&b.0, &a.0)
    }
}

/// L'impegno del membro sul suo nonce: `H(nonce_membro ‖ chiavi in ordine)`.
pub(crate) fn impegno(nonce_membro: &[u8; 32], a: &ChiavePubblica, b: &ChiavePubblica) -> [u8; 32] {
    let (prima, seconda) = in_ordine(a, b);
    let mut hasher = blake3::Hasher::new_derive_key(CONTESTO_IMPEGNO);
    hasher.update(nonce_membro);
    hasher.update(prima);
    hasher.update(seconda);
    *hasher.finalize().as_bytes()
}

/// L'impegno ricevuto corrisponde al nonce rivelato?
///
/// Il confronto è quello di `blake3::Hash`, a tempo costante: un confronto
/// che si ferma al primo byte diverso direbbe a chi misura i tempi quanti
/// byte ha indovinato.
pub(crate) fn impegno_mantenuto(
    ricevuto: &[u8; 32],
    nonce_rivelato: &[u8; 32],
    a: &ChiavePubblica,
    b: &ChiavePubblica,
) -> bool {
    blake3::Hash::from_bytes(impegno(nonce_rivelato, a, b)) == blake3::Hash::from_bytes(*ricevuto)
}

/// Il codice: i primi quattro byte di
/// `H(handshake_hash ‖ nonce_membro ‖ nonce_host ‖ chiavi in ordine)`, come
/// `u32`, modulo diecimila, in quattro cifre.
///
/// Il modulo non è uniforme alla perfezione — 2³² non è un multiplo di
/// diecimila — ma lo scarto è di una parte su mezzo milione, e il codice lo
/// legge una persona.
pub(crate) fn codice(
    hash_stretta: &[u8; 32],
    nonce_membro: &[u8; 32],
    nonce_host: &[u8; 32],
    a: &ChiavePubblica,
    b: &ChiavePubblica,
) -> String {
    let (prima, seconda) = in_ordine(a, b);
    let mut hasher = blake3::Hasher::new_derive_key(CONTESTO_CODICE);
    hasher.update(hash_stretta);
    hasher.update(nonce_membro);
    hasher.update(nonce_host);
    hasher.update(prima);
    hasher.update(seconda);
    let hash = hasher.finalize();
    let (quattro, _) = hash
        .as_bytes()
        .split_first_chunk::<4>()
        .unwrap_or((&[0; 4], &[]));
    format!("{:04}", u32::from_le_bytes(*quattro) % 10_000)
}

/// La finestra in cui si contano le associazioni fallite.
pub(crate) const FINESTRA: Duration = Duration::from_secs(10 * 60);

/// Quante associazioni fallite con la stessa chiave, nella finestra.
pub(crate) const PER_CHIAVE: usize = 3;

/// Quante associazioni fallite in tutto, nella finestra.
pub(crate) const IN_TUTTO: usize = 10;

/// Perché un'associazione non può cominciare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rifiuto {
    /// Troppi tentativi nella finestra: si potrà riprovare fra tanto così.
    Troppi(Duration),
    /// Chi ospita ne ha già una in corso. Vedi [`Tentativi::prenota`].
    Occupato,
}

/// I tentativi d'associazione degli ultimi [`FINESTRA`], falliti **o in
/// corso**. Vedi «Perché i tentativi si contano» nel `//!` del modulo.
///
/// # Prenotati all'inizio, tolti solo a riuscita avvenuta
///
/// Fino alla revisione del 26 ottobre si contavano solo le associazioni
/// fallite, cioè finite: una prova ne ha aperte dodici insieme, e l'host
/// mostrava dodici codici. Un intermediario poteva fare lo stesso con N
/// strette contemporanee e rivelare il suo nonce solo su quella in cui il
/// codice coincideva: le altre restavano «in corso», e non contavano mai.
///
/// Adesso un tentativo conta da quando comincia — l'impegno mandato o
/// ricevuto — e si toglie dal conto solo se riesce. Uno interrotto per
/// qualunque ragione resta lì come fallito, senza che nessuno debba
/// ricordarsi di contarlo: tranne, dal lato di chi chiede, quello finito
/// prima del nonce di chi ospita — un `Occupato`, o la connessione chiusa
/// per qualunque perché — che esce come se non fosse mai cominciato (vedi
/// [`Tentativi::annulla`]).
///
/// Solo in memoria: riavviare Rymembra azzera il conto, ma riavviarlo è un
/// gesto della persona davanti allo schermo, non di chi sta nel mezzo.
#[derive(Debug, Default)]
pub(crate) struct Tentativi {
    /// Quando, con chi, e il numero della prenotazione. Al più [`IN_TUTTO`]
    /// voci dentro la finestra: oltre, non si prenota niente.
    voci: VecDeque<(Instant, ChiavePubblica, u64)>,
    /// Il numero della prossima prenotazione.
    prossima: u64,
    /// La prenotazione dell'associazione in corso dal lato di chi ospita, se
    /// ce n'è una. Una sola alla volta: con due, un intermediario potrebbe
    /// tenerne aperte quante vuole e scegliere quale finire.
    risponde: Option<u64>,
}

impl Tentativi {
    /// Prenota un tentativo con questa chiave. `da_host` è il lato di chi
    /// ospita, che ne tiene uno solo in corso alla volta.
    ///
    /// # Errori
    ///
    /// [`Rifiuto::Occupato`] se chi ospita ne ha già uno in corso;
    /// [`Rifiuto::Troppi`] oltre i tetti, con fra quanto si potrà.
    pub(crate) fn prenota(
        &mut self,
        chiave: &ChiavePubblica,
        adesso: Instant,
        da_host: bool,
    ) -> Result<u64, Rifiuto> {
        self.pota(adesso);
        if da_host && self.risponde.is_some() {
            return Err(Rifiuto::Occupato);
        }
        let resta = |quando: Instant| {
            quando
                .checked_add(FINESTRA)
                .map_or(FINESTRA, |fine| fine.saturating_duration_since(adesso))
        };
        let suoi: Vec<Instant> = self
            .voci
            .iter()
            .filter(|(_, di, _)| di == chiave)
            .map(|(quando, _, _)| *quando)
            .collect();
        if suoi.len() >= PER_CHIAVE {
            // Si riapre quando esce dalla finestra il più vecchio dei suoi.
            return Err(Rifiuto::Troppi(
                suoi.first().map_or(FINESTRA, |q| resta(*q)),
            ));
        }
        if self.voci.len() >= IN_TUTTO {
            return Err(Rifiuto::Troppi(
                self.voci.front().map_or(FINESTRA, |(q, _, _)| resta(*q)),
            ));
        }
        let numero = self.prossima;
        self.prossima = self.prossima.wrapping_add(1);
        self.voci.push_back((adesso, *chiave, numero));
        if da_host {
            self.risponde = Some(numero);
        }
        Ok(numero)
    }

    /// Il tentativo `numero` è finito. Riuscito, esce dal conto; fallito,
    /// resta fino all'uscita dalla finestra.
    pub(crate) fn finito(&mut self, numero: u64, riuscito: bool) {
        if self.risponde == Some(numero) {
            self.risponde = None;
        }
        if riuscito {
            self.voci.retain(|(_, _, n)| *n != numero);
        }
    }

    /// Il tentativo `numero` non è mai cominciato davvero: dal lato di chi
    /// chiede, è finito prima di vedere il nonce di chi ospita — un
    /// «occupato» (`Trasporto::Occupato`), o la connessione chiusa, per
    /// esempio dal `troppiTentativi` di un host al suo tetto per colpa di
    /// altri PC. Esce dal conto come uno riuscito.
    ///
    /// Non apre un buco nel conto: senza il nonce di chi ospita non c'è un
    /// codice, e chi chiede non ha rivelato il suo — a un terzo nel mezzo che
    /// risponde «occupato», o chiude, resta al più l'impegno, cioè un hash, e
    /// niente da provare.
    pub(crate) fn annulla(&mut self, numero: u64) {
        self.finito(numero, true);
    }

    /// Toglie quel che è uscito dalla finestra.
    fn pota(&mut self, adesso: Instant) {
        while let Some((quando, _, _)) = self.voci.front() {
            if adesso.saturating_duration_since(*quando) < FINESTRA {
                break;
            }
            self.voci.pop_front();
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    const A: ChiavePubblica = ChiavePubblica([1; 32]);
    const B: ChiavePubblica = ChiavePubblica([2; 32]);
    const C: ChiavePubblica = ChiavePubblica([3; 32]);

    #[test]
    fn il_codice_e_lo_stesso_sui_due_capi_qualunque_sia_l_ordine_delle_chiavi() {
        let hash = [9; 32];
        let (nm, nh) = ([4; 32], [5; 32]);
        let dal_membro = codice(&hash, &nm, &nh, &A, &B);
        let dall_host = codice(&hash, &nm, &nh, &B, &A);
        assert_eq!(dal_membro, dall_host);
        assert_eq!(dal_membro.len(), 4);
        assert!(dal_membro.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn ogni_ingrediente_cambia_il_codice() {
        // Non una prova di crittografia: che ognuno dei cinque ingredienti
        // entri davvero nel conto. Uno dimenticato — l'hash della stretta,
        // soprattutto — renderebbe il codice uguale anche con un terzo nel
        // mezzo.
        let base = codice(&[9; 32], &[4; 32], &[5; 32], &A, &B);
        let varianti = [
            codice(&[8; 32], &[4; 32], &[5; 32], &A, &B),
            codice(&[9; 32], &[3; 32], &[5; 32], &A, &B),
            codice(&[9; 32], &[4; 32], &[6; 32], &A, &B),
            codice(&[9; 32], &[4; 32], &[5; 32], &A, &C),
        ];
        // Con quattro cifre due codici uguali per caso capitano una volta su
        // diecimila, ma gli ingredienti qui sono fissi: la prova è
        // deterministica, e con questi non capita.
        for (quale, variante) in varianti.iter().enumerate() {
            assert_ne!(*variante, base, "ingrediente {quale}");
        }
        assert_ne!(impegno(&[4; 32], &A, &B), impegno(&[3; 32], &A, &B));
        assert_ne!(impegno(&[4; 32], &A, &B), impegno(&[4; 32], &A, &C));
    }

    #[test]
    fn l_impegno_si_verifica_solo_col_suo_nonce() {
        let nonce = [7; 32];
        let impegno_dato = impegno(&nonce, &A, &B);
        assert!(impegno_mantenuto(&impegno_dato, &nonce, &B, &A));
        assert!(
            !impegno_mantenuto(&impegno_dato, &[8; 32], &A, &B),
            "la rivelazione falsa"
        );
        assert!(
            !impegno_mantenuto(&impegno_dato, &nonce, &A, &C),
            "le chiavi di un'altra coppia"
        );
    }

    /// Prenota e fa fallire.
    fn fallisce(tentativi: &mut Tentativi, chiave: &ChiavePubblica, quando: Instant) {
        let numero = tentativi
            .prenota(chiave, quando, false)
            .expect("si prenota");
        tentativi.finito(numero, false);
    }

    #[test]
    fn tre_fallimenti_con_la_stessa_chiave_fermano_quella_chiave_per_dieci_minuti() {
        let zero = Instant::now();
        let mut tentativi = Tentativi::default();
        for minuto in 0..3 {
            fallisce(&mut tentativi, &A, zero + Duration::from_secs(minuto * 60));
        }
        let adesso = zero + Duration::from_secs(3 * 60);
        let rifiuto = tentativi
            .prenota(&A, adesso, false)
            .expect_err("tre bastano");
        assert_eq!(
            rifiuto,
            Rifiuto::Troppi(Duration::from_secs(7 * 60)),
            "esce il primo"
        );
        assert!(
            tentativi.prenota(&B, adesso, false).is_ok(),
            "le altre chiavi no"
        );
        assert!(
            tentativi
                .prenota(&A, zero + FINESTRA + Duration::from_secs(1), false)
                .is_ok(),
            "il primo è uscito dalla finestra"
        );
    }

    #[test]
    fn dieci_fallimenti_in_tutto_fermano_anche_le_chiavi_nuove() {
        let zero = Instant::now();
        let mut tentativi = Tentativi::default();
        for i in 0_u8..10 {
            fallisce(
                &mut tentativi,
                &ChiavePubblica([i.saturating_add(10); 32]),
                zero,
            );
        }
        assert!(
            tentativi
                .prenota(&ChiavePubblica([99; 32]), zero, false)
                .is_err(),
            "chi cambia chiave a ogni tentativo resta fermo lo stesso"
        );
        assert!(
            tentativi
                .prenota(&ChiavePubblica([99; 32]), zero + FINESTRA, false)
                .is_ok()
        );
    }

    #[test]
    fn i_tentativi_in_corso_contano_come_falliti_finche_non_riescono() {
        // Il difetto della revisione: dieci associazioni lasciate a metà non
        // contavano, perché nessuna era «fallita».
        let zero = Instant::now();
        let mut tentativi = Tentativi::default();
        let in_corso: Vec<u64> = (0_u8..10)
            .map(|i| {
                tentativi
                    .prenota(&ChiavePubblica([i; 32]), zero, false)
                    .expect("si prenota")
            })
            .collect();
        assert!(
            matches!(
                tentativi.prenota(&ChiavePubblica([50; 32]), zero, false),
                Err(Rifiuto::Troppi(_))
            ),
            "l'undicesimo, con dieci ancora aperti"
        );
        // Una riesce: libera il suo posto.
        tentativi.finito(*in_corso.first().expect("uno"), true);
        assert!(
            tentativi
                .prenota(&ChiavePubblica([50; 32]), zero, false)
                .is_ok()
        );
    }

    #[test]
    fn diecimila_tentativi_al_ritmo_dei_tetti_sono_circa_una_settimana() {
        // Il numero che dice il `//!` del modulo: chi cambia i tetti deve
        // cambiare anche quello.
        let finestra_in_minuti = FINESTRA.as_secs().div_euclid(60);
        let minuti =
            (10_000 * finestra_in_minuti).div_euclid(u64::try_from(IN_TUTTO).expect("sta"));
        assert!(
            minuti > 6 * 24 * 60 && minuti < 8 * 24 * 60,
            "{minuti} minuti"
        );
    }

    #[test]
    fn chi_ospita_ne_tiene_una_sola_in_corso() {
        let zero = Instant::now();
        let mut tentativi = Tentativi::default();
        let prima = tentativi.prenota(&A, zero, true).expect("la prima");
        assert_eq!(tentativi.prenota(&B, zero, true), Err(Rifiuto::Occupato));
        assert!(
            tentativi.prenota(&B, zero, false).is_ok(),
            "dal lato di chi chiede non vale"
        );
        tentativi.finito(prima, false);
        assert!(
            tentativi.prenota(&C, zero, true).is_ok(),
            "finita, se ne può cominciare un'altra"
        );
    }
}
