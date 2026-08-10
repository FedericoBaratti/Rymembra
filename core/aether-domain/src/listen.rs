//! Quando un brano sentito conta come un brano ascoltato.
//!
//! Sembra una domanda oziosa e non lo è: la risposta finisce in `play_count`,
//! che è una delle tre colonne che [`crate::merge`] descrive come non
//! ricostruibili. Un conteggio sbagliato non si accorge subito e non si ripara
//! mai, perché non esiste nessun posto da cui rileggere quante volte una
//! canzone è stata davvero ascoltata.
//!
//! # Il difetto che questo modulo corregge
//!
//! Nel vecchio albero il conteggio si incrementava all'**avvio** del brano
//! (`usePlayerStore.ts`, dentro `onPlay`), mentre lo scrobble verso Last.fm
//! partiva solo dopo cinque secondi accumulati. Due misure della stessa cosa che
//! non concordano, e quella sbagliata è la prima: scorrere cinquanta brani
//! saltandoli dopo un secondo lasciava cinquanta `play_count` incrementati.
//!
//! Su quei numeri poggiano l'ordinamento «più ascoltati», la pagina delle
//! statistiche e i consigli. Cinque minuti passati a cercare una canzone
//! bastavano a sporcare tutti e tre.
//!
//! # La regola
//!
//! Quella di Last.fm, che ha il pregio di essere già la convenzione condivisa:
//! conta quando si è sentita **metà** del brano, o **quattro minuti**, quel che
//! viene prima. Sotto i trenta secondi di durata non conta mai — non sono
//! canzoni, sono stacchetti e tracce di silenzio.
//!
//! Il tempo che si misura è quello **effettivamente suonato**: la pausa non
//! accumula. Un brano lasciato in pausa per un'ora non è un brano ascoltato.

/// Sotto questa durata, un brano non conta mai come ascolto.
pub const DURATA_MINIMA_MS: u64 = 30_000;

/// Ascoltato per tanto, conta comunque, qualunque sia la durata.
pub const SOGLIA_ASSOLUTA_MS: u64 = 4 * 60 * 1_000;

/// Questo ascolto conta come ascolto?
///
/// ```
/// use aether_domain::listen::counts_as_play;
/// // Un brano di tre minuti saltato dopo due secondi: no.
/// assert!(!counts_as_play(2_000, 180_000));
/// // Lo stesso brano sentito fino a metà: sì.
/// assert!(counts_as_play(90_000, 180_000));
/// // Mezz'ora di concerto: bastano quattro minuti.
/// assert!(counts_as_play(240_000, 1_800_000));
/// ```
#[must_use]
pub fn counts_as_play(listened_ms: u64, duration_ms: u64) -> bool {
    // Durata sconosciuta: la metà non si può calcolare, resta la soglia
    // assoluta. Capita con i flussi, e capiterà con i podcast.
    if duration_ms == 0 {
        return listened_ms >= SOGLIA_ASSOLUTA_MS;
    }
    if duration_ms < DURATA_MINIMA_MS {
        return false;
    }
    // Moltiplicare invece di dividere: `duration_ms / 2` perderebbe il
    // millisecondo dispari, e su un brano di durata dispari la soglia
    // risulterebbe raggiunta un millisecondo prima di quando lo è.
    listened_ms >= SOGLIA_ASSOLUTA_MS || listened_ms.saturating_mul(2) >= duration_ms
}

/// Un ascolto concluso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listen {
    /// Quale brano.
    pub track_id: i64,
    /// Quando l'ascolto è **cominciato**, in millisecondi dall'epoca.
    ///
    /// L'inizio e non la fine: è ciò che `play_history.played_at` deve
    /// contenere, ed è ciò che il protocollo Last.fm chiede quando lo
    /// scrobbling arriverà. Registrare la fine sposterebbe ogni ascolto avanti
    /// della durata del brano, e su una cronologia sarebbe visibile.
    pub started_at: i64,
    /// Quanto è stato suonato davvero, pause escluse.
    pub listened_ms: u64,
    /// Conta come ascolto secondo [`counts_as_play`].
    pub counts: bool,
}

/// Misura quanto di un brano è stato davvero suonato.
///
/// Non guarda l'orologio: ogni metodo riceve l'istante. È la regola di questo
/// crate, e qui ha un vantaggio concreto — un ascolto lungo mezz'ora si prova in
/// microsecondi, passando i numeri che servono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenTracker {
    track_id: i64,
    duration_ms: u64,
    started_at: i64,
    accumulated_ms: u64,
    /// Da quando sta suonando, se sta suonando.
    playing_since: Option<i64>,
}

impl ListenTracker {
    /// Comincia a misurare un ascolto che parte adesso.
    #[must_use]
    pub const fn begin(track_id: i64, duration_ms: u64, now_ms: i64) -> Self {
        Self {
            track_id,
            duration_ms,
            started_at: now_ms,
            accumulated_ms: 0,
            playing_since: Some(now_ms),
        }
    }

    /// Quale brano si sta misurando.
    #[must_use]
    pub const fn track_id(&self) -> i64 {
        self.track_id
    }

    /// Mette in pausa la misura.
    ///
    /// Chiamarla due volte di fila non accumula due volte: il secondo colpo
    /// trova `playing_since` già vuoto e non fa niente. Serve, perché il motore
    /// può segnalare una pausa che l'interfaccia ha già segnalato.
    pub fn pause(&mut self, now_ms: i64) {
        if let Some(da) = self.playing_since.take() {
            self.accumulated_ms = self.accumulated_ms.saturating_add(trascorsi(da, now_ms));
        }
    }

    /// Riprende la misura.
    pub const fn resume(&mut self, now_ms: i64) {
        if self.playing_since.is_none() {
            self.playing_since = Some(now_ms);
        }
    }

    /// Quanto è stato suonato finora.
    #[must_use]
    pub fn listened_ms(&self, now_ms: i64) -> u64 {
        match self.playing_since {
            Some(da) => self.accumulated_ms.saturating_add(trascorsi(da, now_ms)),
            None => self.accumulated_ms,
        }
    }

    /// Chiude l'ascolto e dice cosa ne è venuto fuori.
    #[must_use]
    pub fn finish(mut self, now_ms: i64) -> Listen {
        self.pause(now_ms);
        Listen {
            track_id: self.track_id,
            started_at: self.started_at,
            listened_ms: self.accumulated_ms,
            counts: counts_as_play(self.accumulated_ms, self.duration_ms),
        }
    }
}

/// I millisecondi fra due istanti, con un orologio che può andare indietro.
///
/// Un salto all'indietro — l'ora legale, una sincronizzazione NTP, una macchina
/// virtuale ripresa da uno snapshot — darebbe una differenza negativa. Vale
/// zero: un ascolto non può durare un tempo negativo, e la sottrazione senza
/// guardia produrrebbe un numero enorme una volta convertita.
fn trascorsi(da: i64, a: i64) -> u64 {
    u64::try_from(a.saturating_sub(da)).unwrap_or(0)
}

#[cfg(test)]
mod prove {
    use super::*;

    const TRE_MINUTI: u64 = 180_000;

    #[test]
    fn un_brano_saltato_non_conta() {
        assert!(!counts_as_play(1_000, TRE_MINUTI));
        assert!(!counts_as_play(89_999, TRE_MINUTI));
    }

    #[test]
    fn meta_brano_conta() {
        assert!(counts_as_play(90_000, TRE_MINUTI));
        assert!(counts_as_play(TRE_MINUTI, TRE_MINUTI));
    }

    #[test]
    fn quattro_minuti_contano_anche_su_un_brano_lunghissimo() {
        let un_ora = 3_600_000;
        assert!(!counts_as_play(SOGLIA_ASSOLUTA_MS - 1, un_ora));
        assert!(counts_as_play(SOGLIA_ASSOLUTA_MS, un_ora));
    }

    #[test]
    fn uno_stacchetto_non_conta_mai() {
        // Venti secondi ascoltati per intero: resta sotto la durata minima.
        assert!(!counts_as_play(20_000, 20_000));
    }

    #[test]
    fn una_durata_sconosciuta_ricade_sulla_soglia_assoluta() {
        assert!(!counts_as_play(100_000, 0));
        assert!(counts_as_play(SOGLIA_ASSOLUTA_MS, 0));
    }

    #[test]
    fn la_meta_non_si_raggiunge_un_millisecondo_prima() {
        // Durata dispari: la soglia è 90_000,5 ms, cioè 90_001 interi.
        assert!(!counts_as_play(90_000, 180_001));
        assert!(counts_as_play(90_001, 180_001));
    }

    #[test]
    fn la_pausa_non_accumula() {
        let mut t = ListenTracker::begin(1, TRE_MINUTI, 0);
        t.pause(10_000);
        // Un'ora in pausa.
        assert_eq!(t.listened_ms(3_610_000), 10_000);
        t.resume(3_610_000);
        assert_eq!(t.listened_ms(3_615_000), 15_000);
    }

    #[test]
    fn mettere_in_pausa_due_volte_non_conta_due_volte() {
        let mut t = ListenTracker::begin(1, TRE_MINUTI, 0);
        t.pause(5_000);
        t.pause(9_000);
        assert_eq!(t.listened_ms(9_000), 5_000);
    }

    #[test]
    fn riprendere_due_volte_non_sposta_l_origine() {
        let mut t = ListenTracker::begin(1, TRE_MINUTI, 0);
        t.resume(50_000);
        assert_eq!(t.listened_ms(10_000), 10_000);
    }

    #[test]
    fn l_ascolto_conserva_l_istante_di_inizio() {
        let t = ListenTracker::begin(42, TRE_MINUTI, 1_000_000);
        let l = t.finish(1_100_000);
        assert_eq!(l.track_id, 42);
        assert_eq!(l.started_at, 1_000_000);
        assert_eq!(l.listened_ms, 100_000);
        assert!(l.counts);
    }

    #[test]
    fn un_ascolto_saltato_si_chiude_senza_contare() {
        let t = ListenTracker::begin(7, TRE_MINUTI, 0);
        let l = t.finish(2_000);
        assert_eq!(l.listened_ms, 2_000);
        assert!(!l.counts);
    }

    #[test]
    fn un_orologio_che_torna_indietro_non_produce_un_ascolto_infinito() {
        let t = ListenTracker::begin(1, TRE_MINUTI, 1_000_000);
        // L'ora legale toglie un'ora a metà canzone.
        let l = t.finish(1_000_000 - 3_600_000);
        assert_eq!(l.listened_ms, 0);
        assert!(!l.counts);
    }
}
