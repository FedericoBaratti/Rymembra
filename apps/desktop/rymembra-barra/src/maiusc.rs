//! «Maiusc tenuto mentre Rymembra si apre»: la domanda al sistema e la regola
//! che decide (D-48, F10-2).
//!
//! # Perché sta qui, e non nella pagina
//!
//! Maiusc tenuto all'avvio è l'uscita d'emergenza da una skin che rende la
//! finestra inservibile: per questo non può dipendere da niente che la pagina
//! o la skin facciano. La prima versione ascoltava i `keydown` della pagina
//! per cinque secondi, a partire da quando la skin arrivava. Dal vivo non
//! bastava, per tre ragioni che la pagina non può risolvere da sé:
//!
//! - la finestra **compare prima** che la skin sia pronta — dopo due secondi
//!   anche se la pagina non ha finito (`rete_di_sicurezza` in `lib.rs`), e
//!   durante una migrazione lunga ben prima — e chi ha tenuto Maiusc «finché è
//!   comparsa la finestra» lo rilascia quando i cinque secondi non sono ancora
//!   cominciati;
//! - un `keydown` arriva alla pagina solo se la pagina è caricata **e ha il
//!   fuoco**: una finestra che si apre dietro un'altra, o una pagina che sta
//!   ancora caricando, non lo riceve;
//! - la pagina non sa quando la finestra è comparsa, e il tempo giusto da
//!   misurare è quello da cui chi guarda può vedere Rymembra.
//!
//! Il sistema invece lo sa sempre: `GetAsyncKeyState` dice se Maiusc è giù
//! adesso, qualunque finestra abbia il fuoco. Questo modulo è la sola chiamata
//! `unsafe` che serve (il quinto posto dichiarato del crate) e la regola che
//! la usa, pura, perché si possa provare senza una tastiera.
//!
//! # La regola
//!
//! Si guarda il tasto ogni [`PASSO`], dall'avvio. Conta come «tenuto» un
//! Maiusc che resta giù per [`TENUTO`] di seguito — non una maiuscola scritta
//! in fretta, non un clic con Maiusc per scegliere un intervallo — e che è
//! ancora giù quando la finestra è visibile, entro [`FINESTRA`] dalla sua
//! comparsa. Il tempo della pressione conta anche se è cominciato prima della
//! comparsa: chi tiene Maiusc mentre avvia il programma ci arriva già premuto.
//! Un Maiusc rilasciato prima che la finestra si veda non conta: chi lo
//! tiene, lo tiene finché vede Rymembra.
//!
//! Questo non ascolta la tastiera: legge **un tasto**, il suo stato di adesso,
//! e solo nei primi secondi dopo l'avvio. Niente di quel che si scrive passa di
//! qui, e niente si ricorda oltre il sì o il no.

use std::time::Duration;

/// Per quanto Maiusc deve restare giù, di seguito, perché valga «tenuto».
///
/// È il ritardo di ripetizione di serie di Windows: una pressione che lo
/// raggiunge sarebbe già diventata una ripetizione, ed era il criterio della
/// prima versione («`repeat`»). Una maiuscola scritta in fretta dura molto
/// meno.
pub const TENUTO: Duration = Duration::from_millis(500);

/// Quanto dura l'ascolto, dalla comparsa della finestra.
pub const FINESTRA: Duration = Duration::from_secs(5);

/// Ogni quanto si guarda il tasto: venticinque volte al secondo, che per un
/// tasto tenuto mezzo secondo sono dodici letture e per un processore non sono
/// niente.
pub const PASSO: Duration = Duration::from_millis(40);

/// Quel che si sa finora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Esito {
    /// Non ancora deciso: la finestra non si vede, o i suoi cinque secondi non
    /// sono finiti.
    InCorso,
    /// Maiusc era tenuto quando la finestra si è vista.
    Tenuto,
    /// No, e non lo sarà più: i cinque secondi sono passati.
    No,
}

/// La regola, un campione alla volta.
///
/// Il tempo lo dà chi chiama (`ora`, da un punto fisso qualunque, per esempio
/// l'avvio dell'ascolto): così le prove non aspettano niente.
#[derive(Debug, Default)]
pub struct Ascolto {
    /// Da quando Maiusc è giù, di seguito. `None` se adesso è su.
    giu_da: Option<Duration>,
    /// Il primo campione in cui la finestra era visibile.
    comparsa: Option<Duration>,
    /// Deciso, e non cambia più.
    deciso: Option<bool>,
}

impl Ascolto {
    /// Un ascolto nuovo, senza campioni.
    #[must_use]
    pub fn nuovo() -> Self {
        Self::default()
    }

    /// Un campione: Maiusc `giu`, la finestra `visibile`, all'istante `ora`.
    ///
    /// I campioni vanno dati in ordine di tempo. Dopo una decisione non cambia
    /// niente: il sì e il no restano.
    pub fn campiona(&mut self, ora: Duration, giu: bool, visibile: bool) -> Esito {
        if self.deciso.is_some() {
            return self.esito();
        }
        self.giu_da = giu.then(|| self.giu_da.unwrap_or(ora));
        if visibile && self.comparsa.is_none() {
            self.comparsa = Some(ora);
        }
        if let Some(comparsa) = self.comparsa {
            let tenuto = self
                .giu_da
                .is_some_and(|da| ora.saturating_sub(da) >= TENUTO);
            if tenuto && ora <= comparsa + FINESTRA {
                self.deciso = Some(true);
            } else if ora > comparsa + FINESTRA {
                self.deciso = Some(false);
            }
        }
        self.esito()
    }

    /// Quel che si sa adesso.
    #[must_use]
    pub const fn esito(&self) -> Esito {
        match self.deciso {
            None => Esito::InCorso,
            Some(true) => Esito::Tenuto,
            Some(false) => Esito::No,
        }
    }
}

/// Maiusc — il sinistro o il destro — è giù adesso, secondo il sistema.
///
/// Fuori da Windows risponde sempre di no: sul telefono non c'è Maiusc da
/// tenere, e altrove l'uscita d'emergenza non è stata chiesta.
#[must_use]
pub fn maiusc_giu() -> bool {
    #[cfg(windows)]
    {
        stato_di_maiusc()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// `GetAsyncKeyState(VK_SHIFT)`: il bit più alto dice «giù adesso».
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "GetAsyncKeyState non ha un'interfaccia sicura: il blocco dice perché \
              è corretto"
)]
fn stato_di_maiusc() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    // SAFETY: nessun puntatore e nessuna memoria nostra: un numero di tasto
    // virtuale, costante e documentato, e una risposta a sedici bit. Si può
    // chiamare da qualunque filo e in qualunque momento; se il desktop non è
    // quello di chi lavora (schermo bloccato) risponde zero, che qui vuol dire
    // «non tenuto». Il tasto è «giù» se il bit più alto è acceso, cioè se il
    // valore, con segno, è negativo.
    unsafe { GetAsyncKeyState(i32::from(VK_SHIFT.0)) < 0 }
}

#[cfg(test)]
mod prove {
    use super::*;

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Dà all'ascolto i campioni di una scena: `(da, a)` è l'intervallo in cui
    /// Maiusc è giù, `comparsa` l'istante in cui la finestra si vede, tutto in
    /// millisecondi dall'avvio. Si ferma alla fine (`fino`).
    fn scena(giu: Option<(u64, u64)>, comparsa: u64, fino: u64) -> Esito {
        let mut a = Ascolto::nuovo();
        let mut t = 0;
        let mut esito = Esito::InCorso;
        while t <= fino {
            let premuto = giu.is_some_and(|(da, fino)| t >= da && t < fino);
            esito = a.campiona(ms(t), premuto, t >= comparsa);
            t += u64::try_from(PASSO.as_millis()).unwrap_or(40);
        }
        esito
    }

    /// F10-2: Maiusc tenuto dall'avvio e rilasciato dopo la comparsa della
    /// finestra — a +1 s, +3 s, +5 s — vale. La prima versione, che misurava
    /// cinque secondi dalla skin e leggeva solo i `keydown` della pagina, non
    /// lo vedeva se rilasciato prima di +4,3 s.
    #[test]
    fn un_maiusc_tenuto_fino_dopo_la_comparsa_scatta_se_rilasciato_presto() {
        for rilascio in [1_000, 3_000, 5_000] {
            // La finestra compare a 800 ms dall'avvio, Maiusc è giù da 100.
            let comparsa = 800;
            assert_eq!(
                scena(Some((100, comparsa + rilascio)), comparsa, comparsa + 6_000),
                Esito::Tenuto,
                "rilasciato a +{rilascio} ms dalla comparsa"
            );
        }
    }

    /// La finestra compare tardi (dopo due secondi, o dopo una migrazione):
    /// quel che conta è che Maiusc sia giù quando compare.
    #[test]
    fn la_finestra_che_compare_dopo_non_cambia_niente() {
        for comparsa in [2_000, 13_000, 60_000] {
            assert_eq!(
                scena(Some((0, comparsa + 700)), comparsa, comparsa + 6_000),
                Esito::Tenuto,
                "finestra visibile a {comparsa} ms"
            );
        }
    }

    /// Una pressione che comincia dopo la comparsa conta, se dura.
    #[test]
    fn un_maiusc_premuto_dopo_la_comparsa_scatta_se_dura() {
        // Premuto a +2 s, rilasciato a +3 s.
        assert_eq!(
            scena(Some((2_800, 3_800)), 800, 6_800),
            Esito::Tenuto,
            "premuto a +2 s per un secondo"
        );
    }

    /// Il contratto di F10: una pressione breve non scatta. Una maiuscola
    /// scritta in fretta, un clic con Maiusc.
    #[test]
    fn una_pressione_breve_non_scatta() {
        for durata in [40, 120, 300, 440] {
            assert_eq!(
                scena(Some((1_500, 1_500 + durata)), 800, 7_000),
                Esito::No,
                "Maiusc giù per {durata} ms a +0,7 s dalla comparsa"
            );
        }
        // E nemmeno una pressione breve già in corso alla comparsa, se finisce
        // prima di TENUTO in tutto.
        assert_eq!(scena(Some((700, 1_100)), 800, 7_000), Esito::No);
    }

    /// Il bordo della finestra: una pressione che raggiunge TENUTO entro cinque
    /// secondi dalla comparsa scatta, una che ci arriva dopo no.
    #[test]
    fn i_cinque_secondi_hanno_un_bordo() {
        let comparsa = 800;
        // Raggiunge mezzo secondo a comparsa + 4,9 s: dentro.
        assert_eq!(
            scena(
                Some((comparsa + 4_400, comparsa + 6_000)),
                comparsa,
                comparsa + 8_000
            ),
            Esito::Tenuto
        );
        // Raggiunge mezzo secondo a comparsa + 5,5 s: fuori.
        assert_eq!(
            scena(
                Some((comparsa + 5_000, comparsa + 7_000)),
                comparsa,
                comparsa + 8_000
            ),
            Esito::No
        );
    }

    /// Maiusc rilasciato prima che la finestra si veda non conta: chi lo tiene,
    /// lo tiene finché vede Rymembra.
    #[test]
    fn un_maiusc_rilasciato_prima_della_comparsa_non_conta() {
        assert_eq!(scena(Some((0, 1_500)), 3_000, 10_000), Esito::No);
    }

    /// Senza Maiusc non succede niente, e finché la finestra non si vede non si
    /// decide niente.
    #[test]
    fn senza_maiusc_si_aspetta_la_comparsa_e_poi_i_cinque_secondi() {
        assert_eq!(scena(None, 800, 5_000), Esito::InCorso);
        assert_eq!(scena(None, 800, 5_900), Esito::No);
        // La finestra non compare mai: l'ascolto resta aperto.
        assert_eq!(scena(None, u64::MAX, 60_000), Esito::InCorso);
        assert_eq!(scena(Some((0, 60_000)), u64::MAX, 60_000), Esito::InCorso);
    }

    /// Una decisione non si rimangia: un Maiusc che arriva dopo il no, o un
    /// rilascio dopo il sì, non cambiano niente.
    #[test]
    fn la_decisione_resta() {
        let mut a = Ascolto::nuovo();
        for t in (0..=700).step_by(40) {
            a.campiona(ms(t), true, t >= 100);
        }
        assert_eq!(a.esito(), Esito::Tenuto);
        assert_eq!(a.campiona(ms(900), false, true), Esito::Tenuto);
        assert_eq!(a.campiona(ms(20_000), false, true), Esito::Tenuto);

        let mut a = Ascolto::nuovo();
        for t in (0..=6_000).step_by(40) {
            a.campiona(ms(t), false, true);
        }
        assert_eq!(a.esito(), Esito::No);
        for t in (6_040..=7_500).step_by(40) {
            a.campiona(ms(t), true, true);
        }
        assert_eq!(a.esito(), Esito::No);
    }

    /// La lettura vera del sistema parte e risponde un booleano; che sia un no,
    /// qui, lo garantisce solo il fatto che i test non premono Maiusc.
    #[test]
    fn la_domanda_al_sistema_risponde() {
        let _ = maiusc_giu();
    }
}
