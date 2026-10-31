//! Il racconto: quale brano si sente e a che punto, la mappa per le ancore, gli
//! annunci. `suonati` e `uditi`, e il `//!` del modulo, stanno in
//! [`crate::motore`].

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use rymembra_domain::errors::{AppError, ErrorCode};

use super::{
    Annuncio, CADUTI, CAUSA_FILO_CADUTO, Contesto, Evento, Posizione, Segno, fotogrammi_da_ms,
    fotogrammi_da_ms_con_segno, istantanea, ms_da_fotogrammi,
};

impl Contesto {
    /// Quanti fotogrammi separano il contatore della callback dall'orecchio.
    ///
    /// La somma dei due ritardi che la callback non conta: quello **misurato** —
    /// il buffer del dispositivo, che `cpal` riporta a ogni blocco — e quello
    /// **dichiarato a mano**, cioè tutto il resto della catena d'uscita. Il
    /// `//!` in testa a [`crate::motore`] li distingue tutti e tre.
    ///
    /// Può uscire **negativo**, e non è un errore: una correzione a mano negativa
    /// dice «la posizione raccontata arriva tardi, anticipala», ed è il verso che
    /// serve a chi ha già tarato l'anticipo dei testi su un'altra uscita.
    ///
    /// # Perché una misura assurda si butta
    ///
    /// Perché quel che `cpal` riporta è la durata del buffer del dispositivo:
    /// decine di millisecondi, non centinaia. Un quarto di secondo lì dentro non
    /// è un'uscita lenta — è un orologio del dispositivo che ha risposto una
    /// sciocchezza — e compensarlo sposterebbe cursore e testi di un quarto di
    /// secondo senza che nessuno possa risalire al perché. Sopra
    /// [`crate::uscita::LATENZA_MASSIMA_MS`] resta solo il numero che qualcuno ha
    /// scelto guardando l'effetto.
    fn ritardo_fotogrammi(&self) -> i64 {
        let frequenza = self.formato.frequenza;
        let misurata = self.condiviso.latenza_fotogrammi.load(Ordering::Relaxed);
        let tetto = fotogrammi_da_ms(crate::uscita::LATENZA_MASSIMA_MS, frequenza);
        let misurata = if misurata > tetto { 0 } else { misurata };
        let misurata = i64::try_from(misurata).unwrap_or(i64::MAX);
        misurata.saturating_add(fotogrammi_da_ms_con_segno(
            self.latenza_manuale_ms,
            frequenza,
        ))
    }

    /// Traduce i fotogrammi usciti in un brano e in un istante.
    ///
    /// Due contatori e non uno, ed è la distinzione che il `//!` di [`crate::motore`]
    /// spiega: i **`suonati`** sono i fotogrammi che la callback ha consegnato al
    /// buffer del dispositivo, gli **`uditi`** sono quelli che l'orecchio ha
    /// davvero ricevuto. Gli `uditi` governano sia la scelta del segno sia i
    /// millisecondi riportati — raccontare dove si è, per definizione, è
    /// raccontare dov'è l'orecchio. I `suonati` grezzi vanno a
    /// [`Contesto::forse_fine`], e il perché è scritto là.
    ///
    /// In fondo ripubblica la [`Mappa`], se qualcosa è cambiato da quando la
    /// si è pubblicata l'ultima volta: una volta per giro al più, e mai a
    /// metà di un cambio.
    pub(super) fn aggiorna(&mut self) {
        self.aggiorna_posizione();
        self.pubblica_la_mappa();
    }

    /// I segni come li vede chi traduce le ancore, se sono cambiati.
    ///
    /// L'epoca si legge qui, dalla casella che solo questo filo scrive: è
    /// l'epoca dei segni che si stanno pubblicando, sempre.
    fn pubblica_la_mappa(&mut self) {
        if !std::mem::take(&mut self.mappa_sporca) {
            return;
        }
        let epoca = self.condiviso.epoca.load(Ordering::Acquire);
        let mut mappa = self.mappa.lock().unwrap_or_else(PoisonError::into_inner);
        mappa.epoca = epoca;
        mappa.frequenza = self.formato.frequenza;
        mappa.latenza_manuale_ms = self.latenza_manuale_ms;
        mappa.tolto = self.tolto;
        // Lo stesso vettore, svuotato e riempito: dopo il primo brano la mappa
        // non chiede più memoria a nessuno.
        mappa.segni.clear();
        mappa.segni.extend(self.segni.iter().map(Segno::pubblico));
    }

    /// Il corpo di [`Contesto::aggiorna`]: la posizione e gli annunci.
    fn aggiorna_posizione(&mut self) {
        let suonati = self.condiviso.fotogrammi.load(Ordering::Relaxed);
        let ritardo = self.ritardo_fotogrammi();
        let uditi = if ritardo >= 0 {
            suonati.saturating_sub(u64::try_from(ritardo).unwrap_or(0))
        } else {
            suonati.saturating_add(ritardo.unsigned_abs())
        };

        // Il segno valido è l'ultimo già raggiunto da quel che si sente. Sui
        // `suonati` si cambierebbe brano mentre dalle casse esce ancora il
        // precedente, cioè si annuncerebbe un `Iniziato` in anticipo — e lo
        // scrobble partirebbe prima del primo campione udibile.
        while self.segni.len() > 1 {
            let prossimo_arrivato = self.segni.get(1).is_some_and(|s| s.da <= uditi);
            if prossimo_arrivato {
                // Tolto dalla fila, ma non dimenticato: con una latenza
                // dichiarata negativa gli uditi corrono davanti alla callback,
                // e le sue ancore parlano ancora di lui. Vedi `Mappa::tolto`.
                self.tolto = self.segni.pop_front().as_ref().map(Segno::pubblico);
                self.mappa_sporca = true;
            } else {
                break;
            }
        }

        // Il brano messo da parte serve finché un suo campione deve ancora
        // essere udito: dopo non lo ascolta più nessuno, e tenerne aperto il
        // file costerebbe una cancellazione negata per tutta la durata del brano
        // dopo. Vedi [`Contesto::uscente`].
        //
        // La domanda è «il suo segno è ancora in fila?», e non «è ancora quello
        // in testa»: quando l'anello tiene da parte più di un brano —
        // dell'ordine dei tre secondi, vedi [`crate::uscita::RISERVA_MS`] — la
        // decodifica finisce il brano dopo *prima* che il primo smetta di
        // sentirsi, e `passa_al_prossimo` posa qui il secondo mentre in testa
        // c'è ancora il primo. Guardare la testa lo butterebbe via un brano in
        // anticipo, ed è il salto durante quel brano a restare senza.
        if self
            .uscente
            .as_ref()
            .is_some_and(|u| !self.segni.iter().any(|s| s.ascolto == u.ascolto))
        {
            self.uscente = None;
        }

        let Some(segno) = self.segni.front() else {
            self.scrivi_posizione(Posizione::default());
            self.forse_fine(suonati);
            return;
        };

        // Una somma di fotogrammi e una divisione sola: vedi
        // `Segno::offset_fotogrammi` per il troncamento che toglie.
        let scorsi = uditi.saturating_sub(segno.da);
        let ms = ms_da_fotogrammi(
            segno.offset_fotogrammi.saturating_add(scorsi),
            self.formato.frequenza,
        );
        let (track_id, durata_ms, replaygain_db, ascolto, formato) = (
            segno.track_id,
            segno.durata_ms,
            segno.replaygain_db,
            segno.ascolto,
            segno.formato,
        );

        // `in_pausa` resta al valore di riposo: lo riempie [`Motore::posizione`]
        // quando qualcuno legge, dal bit condiviso.
        //
        // **Prima** dell'annuncio, e non dopo com'era: l'annuncio si porta
        // dietro la posizione di quest'istante — vedi [`Annuncio::posizione`] —
        // e un `Iniziato` che viaggiasse con la posizione del brano di prima
        // direbbe a chi sta sopra la durata sbagliata.
        self.scrivi_posizione(Posizione {
            track_id: Some(track_id),
            ms,
            durata_ms,
            formato,
            ..Posizione::default()
        });

        // L'ascolto e non il brano: vedi [`Segno::ascolto`].
        if self.annunciato != Some(ascolto) {
            self.annunciato = Some(ascolto);
            // Nessun `applica_guadagno` qui: la correzione del brano è già nei
            // campioni da quando sono stati decodificati. Prima andava
            // riapplicata **adesso** — al fotogramma in cui il brano nuovo
            // cominciava a sentirsi — ed era l'unica ragione per cui `Segno`
            // portava con sé un `replaygain_db`.
            self.annuncia(Evento::Iniziato { track_id });
        }
        let _ = replaygain_db;
        self.forse_fine(suonati);
    }

    /// L'ultimo campione è uscito davvero?
    ///
    /// Non basta che il decodificatore sia a secco: fra la fine della
    /// decodifica e la fine del suono ci sono i duecento millisecondi
    /// dell'anello. Dichiarare la fine quando finisce la decodifica taglierebbe
    /// la coda di ogni brano — e con una coda che avanza da sola, la
    /// taglierebbe a ogni brano dell'album.
    ///
    /// # Perché legge i `suonati` e non gli `uditi`
    ///
    /// Perché la fine si dichiara quando l'ultimo campione ha lasciato **l'anello
    /// nostro**, non quando ha lasciato le casse. Questo evento è il segnale con
    /// cui chi sta sopra fa avanzare la coda e prepara il brano dopo, e ritardarlo
    /// della latenza d'uscita vorrebbe dire ritardare di altrettanto il brano
    /// successivo: su un album gapless si sentirebbe un buco fra una traccia e
    /// l'altra, lungo esattamente la latenza compensata. La compensazione serve a
    /// raccontare meglio dove si è, non a far suonare la musica più tardi.
    ///
    /// Il prezzo, dichiarato: il `Fermato` dell'ultimo brano della coda arriva una
    /// latenza d'uscita prima che il suono finisca davvero. Sono i millisecondi in
    /// cui il pulsante mostra già il triangolo mentre l'ultima coda esce dal DAC,
    /// e nessuno li vede.
    ///
    /// # Perché azzera la posizione
    ///
    /// Per la stessa ragione di [`Contesto::ferma`], che è l'altro modo in cui
    /// la musica finisce: dichiarata la fine non c'è più nessun brano nel
    /// motore, e lasciare l'ultimo `track_id` scritto nella posizione vorrebbe
    /// dire un motore che si dice fermo su un brano che non ha più. Chi sta
    /// sopra legge quel campo per distinguere «in pausa a metà» da «non ho
    /// niente in mano»: con l'ultimo brano ancora lì, «riprendi» chiedeva al
    /// motore vuoto di ripartire — cioè non faceva niente — mentre il pulsante
    /// diventava «pausa». I segni se ne vanno con lui: sono la mappa fra
    /// fotogrammi e brani, e i fotogrammi ricominceranno da zero.
    fn forse_fine(&mut self, suonati: u64) {
        if self.fine_dichiarata || self.corrente.is_some() || self.prossimo.is_some() {
            return;
        }
        if suonati >= self.spinti && self.resto.is_empty() {
            self.fine_dichiarata = true;
            // Voluta e in corso insieme: a coda finita chi chiede dove siamo
            // deve sentirsi dire «fermo», e il pulsante diventare il triangolo.
            self.intese.pausa(&self.condiviso);
            self.segni.clear();
            self.tolto = None;
            self.mappa_sporca = true;
            // Con la musica finita non c'è più niente da sentire, e il brano
            // messo da parte è solo un file che resta aperto — su Windows, un
            // file che non si può cancellare. Vedi [`Contesto::uscente`].
            self.uscente = None;
            self.annunciato = None;
            self.scrivi_posizione(Posizione::default());
            self.annuncia(Evento::Fermato);
        }
    }

    pub(super) fn scrivi_posizione(&self, nuova: Posizione) {
        match self.posizione.lock() {
            Ok(mut g) => *g = nuova,
            Err(avvelenato) => *avvelenato.into_inner() = nuova,
        }
    }

    /// Dice a chi sta sopra cos'è successo — se questo motore ha ancora voce.
    ///
    /// L'unico punto da cui gli eventi escono, e la ragione è che un motore
    /// sostituito non deve più parlare: il suo osservatore è vivo e parla allo
    /// stato dell'applicazione, che nel frattempo ha in mano il motore nuovo.
    /// Vedi [`Condiviso::abbandonato`], che [`Motore::drop`] alza prima ancora
    /// di chiedere la chiusura.
    ///
    /// Tacere e non accodare: quel che questo motore aveva da dire riguardava
    /// dei campioni che non usciranno mai dalle casse.
    ///
    /// Questo è il primo dei due controlli, e il meno importante: taglia gli
    /// annunci che partirebbero **dopo** la sostituzione. Quelli partiti un
    /// istante prima sono già in viaggio, e il controllo che conta per loro lo
    /// fa chi li riceve — vedi [`Annuncio::ha_voce`].
    ///
    /// Consegnare non aspetta nessuno: sul motore vero la voce è un canale
    /// verso il filo «rymembra-eventi» (vedi [`voce_su_filo`]), e il lavoro che
    /// l'annuncio scatena lo fa quel filo, non questo.
    pub(super) fn annuncia(&self, evento: Evento) {
        if self.condiviso.abbandonato.load(Ordering::Acquire) {
            return;
        }
        let annuncio = Annuncio {
            evento,
            posizione: istantanea(&self.posizione, &self.intese),
            condiviso: Arc::clone(&self.condiviso),
            intese: Arc::clone(&self.intese),
            svolte: self.intese.eseguite.load(Ordering::SeqCst),
        };
        (self.voce)(annuncio);
    }

    /// Il filo della decodifica è caduto: lo dice, e lascia il motore in uno
    /// stato da cui si può ricostruire.
    ///
    /// # Perché `playback.stalled` e non gli altri due candidati
    ///
    /// `playback.decodeFailed` dice all'utente che il file è danneggiato e
    /// conviene sostituirlo: qui non lo sappiamo: un decodificatore che cade su
    /// byte troncati dalla rete parla di un cavo, non di un file da buttare, e
    /// il codice non è ritentabile — cioè niente «Riprova» proprio dove
    /// riprovare è l'unica mossa. `playback.engineUnavailable` è `Fatal`: non
    /// c'è nessun motore audio, chiudi e riapri l'applicazione — ed è falso,
    /// perché il dispositivo è aperto e a un motore nuovo risponderebbe subito.
    ///
    /// `playback.stalled` è invece esattamente questo fatto: la riproduzione si
    /// è impantanata. `Warning`, sempre ritentabile, già nel catalogo e già
    /// tradotto in tutte e due le lingue, con dentro i due campi che servono a
    /// tornare dov'era la puntina.
    ///
    /// # Perché la stessa bandiera del dispositivo perso
    ///
    /// Perché la cura è la stessa — un motore nuovo — e la via che lo
    /// ricostruisce è già scritta: il sorvegliante delle uscite legge `perso`,
    /// annota dove eravamo e riapre. Una seconda bandiera avrebbe voluto dire
    /// una seconda via di riapertura da tenere in pari con la prima.
    ///
    /// # Perché la causa **prima** della bandiera
    ///
    /// Perché il sorvegliante gira su un altro filo e legge le due caselle in
    /// quest'ordine: vista `perso` alta, va a chiedere la causa. Scrivendola
    /// dopo ci sarebbe una finestra — piccola, e larga abbastanza — in cui la
    /// causa è ancora zero, cioè «causa sconosciuta» nel diario e nel banner
    /// proprio nel caso che il banner esiste per raccontare.
    ///
    /// # Perché non tocca `in_pausa`
    ///
    /// Perché `annota_ripresa`, nell'applicazione, legge la pausa **voluta**
    /// per decidere se la musica dovrà ripartire da sola dopo la riapertura.
    /// Alzarla qui vorrebbe dire raccontargli che l'utente aveva messo in
    /// pausa, e un brano che stava suonando resterebbe fermo dopo un guasto che
    /// nessuno ha chiesto.
    pub(super) fn caduto(&mut self, panico: &str) {
        CADUTI.fetch_add(1, Ordering::AcqRel);
        let dove = istantanea(&self.posizione, &self.intese);
        self.condiviso
            .causa_perdita
            .store(CAUSA_FILO_CADUTO, Ordering::Release);
        self.condiviso.perso.store(true, Ordering::Release);
        self.annuncia(Evento::Errore(Box::new(
            AppError::new(ErrorCode::PlaybackStalled {
                track_id: dove.track_id,
                position_ms: Some(dove.ms),
            })
            .with_cause(format!("il filo della decodifica è caduto: {panico}")),
        )));
    }
}
