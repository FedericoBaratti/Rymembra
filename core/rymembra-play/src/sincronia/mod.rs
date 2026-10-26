//! Stare a tempo con un altro PC: i conti, senza il motore.
//!
//! «Ascolta insieme in LAN» chiede a più PC di far uscire lo stesso istante di
//! musica dalle casse nello stesso istante. Il motore ([`crate::motore`]) sa
//! dire *quando* un punto del brano arriva all'orecchio — l'ancora — e sa
//! eseguire correzioni esatte: partire a un istante, saltare o tacere per N
//! fotogrammi, consumare il suono un poco più in fretta o più piano. Quello
//! che non sa è *cosa* correggere. È il mestiere di questo modulo, diviso in
//! tre pezzi che non si conoscono:
//!
//! - [`orologio`]: di quanto l'orologio dell'ospite è avanti rispetto al mio,
//!   e di quanto quello scarto cambia nel tempo, da scambi in stile NTP;
//! - [`regolatore`]: dalle due ancore, la mia e quella dell'ospite, la mossa da
//!   chiedere al motore;
//! - [`correlazione`]: di quanto un canale arriva dopo l'altro in una
//!   registrazione, cioè la misura acustica che dice se tutto il resto ha
//!   funzionato davvero.
//!
//! # Perché puri
//!
//! Niente I/O, niente orologio di sistema, niente [`crate::Motore`]: il tempo
//! arriva sempre come argomento, in nanosecondi `u64` contati sulla base dei
//! tempi di processo del motore. Così le prove possono far passare un'ora in un
//! millisecondo, mettere una deriva di cento ppm fra due PC che non esistono, e
//! dare ogni volta lo stesso risultato. E così chi li usa — l'esempio
//! `insieme_locale` adesso, il desktop nella fase 1 — sceglie lui il filo, la
//! frequenza e da dove prendere gli istanti.
//!
//! # Le convenzioni di segno, una volta per tutte
//!
//! - **Scarto dell'orologio**: ospite meno locale. Positivo se l'orologio
//!   dell'ospite segna di più.
//! - **Errore del regolatore**: positivo se il membro è **avanti** rispetto
//!   all'ospite.
//! - **Gradino**: come `Motore::gradino`: positivo inserisce silenzio (serve a
//!   chi è avanti), negativo salta contenuto (serve a chi è indietro).
//! - **Deriva**: come `Motore::deriva`: positivo consuma il contenuto più in
//!   fretta (serve a chi è indietro).
//! - **Sfasamento acustico**: positivo se il canale destro arriva dopo il
//!   sinistro.

pub mod correlazione;
pub mod orologio;
pub mod regolatore;
