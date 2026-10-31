//! Il filo della decodifica davanti ai comandi: [`Contesto::esegui`] e quel che
//! ne segue — brano nuovo, salto, stop, equalizzatore, sfumatura del timer.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rymembra_domain::errors::{AppError, ErrorCodeKind};

use super::{
    BranoAperto, Comando, Contesto, Evento, Intese, PASSI_EQ, Posizione, Segno, Sfumatura, Voce,
    avvicina, fattore_di_sfumatura,
};
use crate::ancora::Mappa;
use crate::decodifica::Decodificatore;
use crate::equalizzatore::{BANDE, Coefficienti};
use crate::uscita::FormatoUscita;
use crate::{Condiviso, Livello, volume_uscita};

/// Com'è andata la ricerca del brano su cui far cadere un salto.
///
/// Tre esiti e non un `bool`, perché il salto che fallisce ha bisogno di
/// distinguere gli ultimi due: se nessuno ha toccato niente, un salto fallito
/// lascia la puntina dov'era; se per saltare si sono rimessi in fila i
/// decodificatori, l'anello non racconta più quel che verrà e va buttato
/// comunque.
enum Puntina {
    /// `corrente` era già il brano che si sente.
    AlSuoPosto,
    /// Il brano che si sente è stato riportato in `corrente`.
    Riportata,
    /// Non c'è modo di riportarlo: il salto si lascia cadere.
    Persa,
}

impl Contesto {
    /// Tutte le manopole al valore di riposo, e niente di aperto.
    ///
    /// Fuori da [`Motore::avvia`] perché è l'unico modo di provare questo filo
    /// senza una scheda audio: qui dentro non si apre nessun dispositivo, e le
    /// prove in `motore/prove.rs` costruiscono un contesto vero — decodificatori
    /// veri, dissolvenza vera — con un anello loro. Il valore di riposo di
    /// ventisette campi scritto in due posti diversi sarebbe invece il modo
    /// sicuro di provare qualcosa che non è quel che gira davvero.
    #[expect(
        clippy::too_many_arguments,
        reason = "sono i fili che il contesto tiene con il resto del motore — \
                  comandi, tre anelli, le caselle condivise, la posizione, la \
                  voce e il campanello del flusso — e ognuno arriva da un posto \
                  diverso: raggrupparli in un tipo servirebbe solo a spostare \
                  lo stesso elenco"
    )]
    pub(super) fn nuovo(
        comandi: Receiver<Comando>,
        produttore: rtrb::Producer<f32>,
        coefficienti: rtrb::Producer<Coefficienti>,
        condiviso: Arc<Condiviso>,
        posizione: Arc<std::sync::Mutex<Posizione>>,
        formato: FormatoUscita,
        voce: Voce,
        campanello: crate::uscita::Campanello,
    ) -> Self {
        let mappa = Mappa {
            frequenza: formato.frequenza,
            ..Mappa::default()
        };
        Self {
            comandi,
            produttore,
            condiviso,
            posizione,
            mappa: Arc::new(Mutex::new(mappa)),
            mappa_sporca: false,
            tolto: None,
            etichetta_corrente: None,
            voce,
            intese: Intese::nuove(),
            formato,
            corrente: None,
            ascolto_corrente: 0,
            ascolto_entrante: 0,
            sfumatura: None,
            riallinea_segno: false,
            uscente: None,
            prossimo: None,
            segni: VecDeque::new(),
            resto: VecDeque::new(),
            blocco: Vec::new(),
            spinti: 0,
            annunciato: None,
            ascolti: 0,
            fine_dichiarata: true,
            volume: 0.8,
            muto: false,
            replaygain_attivo: true,
            bersaglio_db: -18.0,
            rg_decodifica: Livello::NESSUNO,
            rg_altrui: Livello::NESSUNO,
            dissolvenza_ms: 0,
            latenza_manuale_ms: 0,
            durata_corrente_ms: 0,
            dissolvenza_fatti: 0,
            dissolvenza_durata: 0,
            segno_dissolvenza: false,
            entrante: None,
            blocco_entrante: Vec::new(),
            coda_entrante: Vec::new(),
            entrata_durata: 0,
            entrata_fatti: 0,
            entrata_da: 1.0,
            coefficienti,
            eq_correnti: [0.0; BANDE],
            eq_bersaglio: [0.0; BANDE],
            eq_passi: 0,
            eq_in_attesa: None,
            campanello,
        }
    }

    pub(super) fn esegui(&mut self, comando: Comando) {
        match comando {
            Comando::Suona { brano, da_ms } => {
                let posizione = Posizione {
                    track_id: Some(brano.decodificatore.track_id()),
                    ms: da_ms,
                    durata_ms: brano.durata_ms,
                    formato: Some(brano.decodificatore.formato_suonato()),
                    ..Posizione::default()
                };
                self.avvia_brano(*brano);
                // La posizione del brano nuovo, al punto chiesto, **prima** di
                // qualunque annuncio: un errore del salto qui sotto, o della
                // prima lettura, se la porta dietro — vedi
                // [`Annuncio::posizione`] — e «Riprova» deve trovarci questo
                // brano e questo punto. Prima restava quella dell'ultimo
                // `aggiorna`, cioè del brano di prima o quella vuota di un
                // `ferma`, e il punto si perdeva. Il primo `aggiorna` la
                // riallinea a dove si è atterrati davvero.
                self.scrivi_posizione(posizione);
                // Il salto subito, prima di qualunque decodifica: l'anello è
                // appena stato buttato e nessun campione del punto zero ci è
                // ancora entrato. Vedi `Comando::Suona`.
                let salto = if da_ms > 0 {
                    self.cerca_dall_avvio(da_ms)
                } else {
                    Ok(())
                };
                // Contata **dopo** aver buttato l'anello, e la pausa in corso
                // allineata dopo ancora: finché la svolta risulta in viaggio,
                // `Intese::riprendi` lascia la pausa in corso alzata, e una
                // ripresa premuta nel frattempo non fa suonare i resti del brano
                // di prima.
                self.intese.eseguite.fetch_add(1, Ordering::SeqCst);
                // L'errore del salto **dopo** aver contato la svolta: è la
                // risposta a questa richiesta, non un annuncio di prima, e non
                // deve risultare superato — chi sta sopra lo scarterebbe senza
                // annotare dove riprendere. Annullato vuol dire che il comando
                // dopo è già in fila: un avviso per un salto che nessuno vuole
                // più sarebbe sbagliato, come in `vai_a`.
                if let Err(err) = salto
                    && err.code().kind() != ErrorCodeKind::InternalAborted
                {
                    self.annuncia(Evento::Errore(Box::new(err)));
                }
                self.intese.allinea(&self.condiviso);
            }
            Comando::Prepara(brano) => self.prepara(brano.map(|b| *b)),
            Comando::Ferma => {
                // Contata **prima** di fermarsi: il `Fermato` che parte da
                // `ferma` è la risposta a questa richiesta, non un annuncio di
                // prima, e non deve risultare superato.
                self.intese.eseguite.fetch_add(1, Ordering::SeqCst);
                self.ferma();
            }
            Comando::VaiA(ms) => self.vai_a(ms),
            Comando::Sfuma { ms } => {
                self.sfumatura = Some(Sfumatura {
                    inizio: Instant::now(),
                    durata: Duration::from_millis(ms.max(1)),
                    finita: false,
                });
            }
            Comando::AnnullaSfuma => {
                if self.sfumatura.take().is_some() {
                    self.applica_guadagno();
                }
            }
            Comando::Volume { volume, muto } => {
                self.volume = volume;
                self.muto = muto;
                self.applica_guadagno();
            }
            Comando::ReplayGain {
                attivo,
                bersaglio_db,
            } => {
                self.replaygain_attivo = attivo;
                self.bersaglio_db = bersaglio_db;
                // Il guadagno d'uscita non dipende più dal ReplayGain, ma la
                // chiamata resta: `volume_uscita` va ripubblicato comunque, e
                // costa un `store` atomico.
                self.applica_guadagno();
            }
            Comando::Dissolvenza { ms } => {
                self.dissolvenza_ms = ms;
            }
            // Vale dal prossimo giro d'orologio, che è fra quattro millisecondi:
            // non c'è niente da rifare e niente da svuotare, perché la latenza
            // non tocca i campioni — sposta soltanto quel che si racconta di
            // loro.
            Comando::Latenza { ms } => {
                self.latenza_manuale_ms = ms;
                // Ma chi traduce le ancore la somma agli istanti: la mappa va
                // ripubblicata, o `Motore::ancora` continuerebbe con la vecchia
                // finché un segno non cambia.
                self.mappa_sporca = true;
            }
            Comando::Equalizzatore { guadagni, attivo } => {
                // Spegnere non è un salto a piatto: è una corsa verso lo zero,
                // come qualunque altro cambio di curva. `Coefficienti::calcola`
                // riconosce da sola che dieci zeri non sono niente da fare, e
                // da lì in poi la callback smette di filtrare.
                self.eq_bersaglio = if attivo { guadagni } else { [0.0; BANDE] };
                self.eq_passi = PASSI_EQ;
            }
            Comando::Chiudi => {}
        }
    }

    /// Un passo verso la curva voluta, e la sua consegna alla callback.
    ///
    /// Interpola i **decibel** e ricalcola i coefficienti a ogni passo, invece
    /// di interpolare i coefficienti: la strada fra due biquad stabili passa per
    /// biquad che non lo sono, e un filtro instabile per due millisecondi non è
    /// un transitorio — è un fischio.
    pub(super) fn avanza_eq(&mut self) {
        // Prima quel che era rimasto indietro: l'anello era pieno, e una curva
        // persa qui è un cursore che non arriva mai a destinazione.
        if let Some(rimasta) = self.eq_in_attesa.take()
            && self.coefficienti.push(rimasta).is_err()
        {
            self.eq_in_attesa = Some(rimasta);
            return;
        }
        if self.eq_passi == 0 {
            return;
        }

        self.eq_passi = self.eq_passi.saturating_sub(1);
        avvicina(&mut self.eq_correnti, &self.eq_bersaglio, self.eq_passi);

        let coefficienti = Coefficienti::calcola(&self.eq_correnti, true, self.formato.frequenza);
        if self.coefficienti.push(coefficienti).is_err() {
            self.eq_in_attesa = Some(coefficienti);
        }
    }

    /// Pubblica alla callback il guadagno d'uscita: volume e silenziamento.
    ///
    /// La correzione ReplayGain **non** passa più di qui. Sta nei campioni, e
    /// li raggiunge in [`Contesto::decodifica_un_blocco`]: durante una
    /// dissolvenza ci sono due brani con due correzioni diverse, e un unico
    /// numero atomico non può dirle entrambe.
    ///
    /// Il prezzo, dichiarato: cambiare il livello di normalizzazione ora si
    /// sente dopo la riserva dell'anello — qualche secondo — invece che
    /// all'istante, perché i campioni già decodificati portano il guadagno di
    /// prima. È una manopola che si tocca dalle impostazioni una volta ogni
    /// tanto, e il cambio arriva senza un salto.
    ///
    /// Durante una [`Sfumatura`] il volume esce moltiplicato per quel che resta
    /// della discesa: così un cursore del volume mosso mentre la musica si sta
    /// spegnendo cambia il livello da cui si scende, e non la fa risalire.
    fn applica_guadagno(&self) {
        let g = volume_uscita(self.volume, self.muto) * self.resto_della_sfumatura();
        self.condiviso
            .guadagno
            .store(g.to_bits(), Ordering::Relaxed);
    }

    /// Quanto del volume resta a questo punto della sfumatura: uno se non ce
    /// n'è una.
    fn resto_della_sfumatura(&self) -> f32 {
        let Some(sfumatura) = self.sfumatura else {
            return 1.0;
        };
        fattore_di_sfumatura(sfumatura.inizio.elapsed(), sfumatura.durata)
    }

    /// Un passo della sfumatura, e alla fine la pausa.
    ///
    /// # Perché il volume resta giù fino alla ripresa
    ///
    /// Qui c'era scritto che il volume tornava quello del cursore subito dopo
    /// la pausa, «tanto la callback in pausa non suona niente». Non è vero per
    /// i primi dodici millisecondi: la pausa nella callback è una porta che si
    /// chiude in `DISSOLVENZA_MS` (vedi `uscita.rs`), e mentre si chiudeva la
    /// rampa del guadagno inseguiva già il volume pieno partendo dal fondo
    /// della discesa. Il prodotto delle due faceva un «blip» di qualche
    /// millisecondo a una ventina di decibel sotto il volume — dopo sei secondi
    /// di silenzio quasi totale, nella stanza di chi si stava addormentando.
    ///
    /// Adesso la sfumatura arrivata in fondo **resta**, segnata come finita: il
    /// guadagno rimane al fondo, la porta si chiude sul quasi-silenzio, e il
    /// volume del cursore torna solo quando la si toglie — la ripresa
    /// ([`Motore::riprendi`] manda `AnnullaSfuma`), un brano nuovo, uno stop.
    /// Alla ripresa la rampa riparte dal fondo, che è la ripartenza morbida
    /// che il commento di prima prometteva.
    pub(super) fn avanza_sfumatura(&mut self) {
        let Some(sfumatura) = self.sfumatura.as_mut() else {
            return;
        };
        // Già in pausa e già al fondo: non c'è niente da fare, e rimettere la
        // pausa a ogni giro si mangerebbe una ripresa premuta nel frattempo.
        if sfumatura.finita {
            return;
        }
        if sfumatura.inizio.elapsed() < sfumatura.durata {
            self.applica_guadagno();
            return;
        }
        sfumatura.finita = true;
        self.intese.pausa(&self.condiviso);
        self.applica_guadagno();
    }

    /// Butta via quel che è già in viaggio verso le casse.
    ///
    /// La stretta di mano con la callback: si alza `svuota`, si aspetta che lo
    /// abbassi. Solo allora l'anello è certamente vuoto, e solo allora si può
    /// azzerare il contatore dei fotogrammi senza correre contro di lei — con
    /// l'anello vuoto la callback non conta niente, quindi la finestra è sicura.
    ///
    /// # Il campanello, prima
    ///
    /// Chi manda il comando sveglia il flusso, ma il comando può arrivare qui
    /// molto dopo: un salto su un flusso di rete fa prima la sua ricerca, che
    /// dura secondi, e in pausa — un secondo sul Bluetooth — il flusso intanto
    /// è tornato a riposo. A flusso fermo nessuno stringe la mano. Suonato
    /// qui, a flusso acceso costa un giro di messaggio; a riposo lo rimette in
    /// moto prima della stretta di mano.
    ///
    /// # Se la callback non risponde
    ///
    /// Si va avanti dopo un decimo di secondo, e il filo non resta fermo: un
    /// dispositivo che accetta di ripartire e poi non chiama più la callback
    /// non deve appendere i comandi dietro di sé. Ma non è detto che il
    /// dispositivo sia morto: può anche solo riposare, e la rete di sicurezza
    /// del filo dell'uscita lo risveglierà entro un quarto di secondo. Quando
    /// lo fa, la callback svuota **tutto** l'anello senza contarlo: per questo,
    /// finché la bandiera resta alta, [`Contesto::riempi_anello`] non ci spinge
    /// niente — il punto nuovo aspetta in `resto`, che `spinti` conta già, e
    /// non diventa un debito fra spinti e suonati.
    fn scarta_in_volo(&mut self) {
        self.campanello.suona();
        self.resto.clear();
        self.condiviso.svuota.store(true, Ordering::Release);
        // La callback gira ogni pochi millisecondi: un decimo di secondo senza
        // risposta vuol dire un flusso che non gira. Vedi sopra cosa succede
        // dopo.
        for _ in 0..100 {
            if !self.condiviso.svuota.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.condiviso.fotogrammi.store(0, Ordering::Relaxed);
        self.spinti = 0;
        self.condiviso.spinti.store(0, Ordering::Relaxed);
        // I campioni alzati dell'epoca vecchia sono appena stati buttati, e il
        // conteggio con cui la callback li riconosceva riparte da zero.
        self.condiviso.piega_fino.store(0, Ordering::Relaxed);
        self.segni.clear();
        self.tolto = None;
        // Un'epoca nuova: i fotogrammi ricominciano da zero, e un'ancora
        // contata prima di questa riga non deve trovare i segni che verranno.
        // **Dopo** l'azzeramento e lo sgombero dei segni, **prima** di spingere
        // un campione nuovo: la callback legge l'epoca dopo aver consumato i
        // suoi campioni, e i campioni nuovi entrano nell'anello solo dopo
        // questa scrittura — l'anello stesso la porta fino a lei. Vedi
        // [`crate::ancora`].
        self.condiviso.epoca.fetch_add(1, Ordering::Release);
        self.mappa_sporca = true;
        // Con l'anello vuoto non si sente più niente di quel che c'era dentro:
        // il brano messo da parte non ha più nessuno che lo ascolti. Vedi
        // [`Contesto::uscente`].
        self.uscente = None;
    }

    /// Installa un brano già aperto e comincia a consegnarne i campioni.
    ///
    /// Non c'è nessun ramo d'errore, e non è una dimenticanza: il brano è
    /// arrivato aperto, quindi tutto quel che poteva andare storto — il
    /// contenitore irriconoscibile, la rete che non risponde — è già andato
    /// storto dall'altra parte del confine, dove c'era una scadenza a
    /// raccoglierlo e un `Result` da restituire a chi aveva premuto play. Vedi
    /// [`BranoAperto`].
    fn avvia_brano(&mut self, brano: BranoAperto) {
        let track_id = brano.decodificatore.track_id();
        let durata_ms = brano.durata_ms;
        let livello = brano.livello;
        let etichetta = brano.etichetta;
        let formato = brano.decodificatore.formato_suonato();
        self.scarta_in_volo();
        self.corrente = Some(brano.decodificatore);
        self.etichetta_corrente = etichetta;
        self.prossimo = None;
        self.annunciato = None;
        self.fine_dichiarata = false;
        self.rg_decodifica = livello;
        self.durata_corrente_ms = durata_ms;
        self.azzera_dissolvenza();
        // Un brano scelto a mano mentre il timer spegneva la musica è una
        // risposta al timer: la discesa si lascia perdere.
        self.sfumatura = None;
        // Il segno di un salto chiesto prima non c'è più: l'ha buttato
        // `scarta_in_volo` insieme all'anello.
        self.riallinea_segno = false;
        self.applica_guadagno();
        let ascolto = self.nuovo_ascolto();
        self.ascolto_corrente = ascolto;
        self.segni.push_back(Segno {
            da: 0,
            track_id,
            durata_ms,
            offset_fotogrammi: 0,
            etichetta,
            replaygain_db: livello.guadagno_db,
            ascolto,
            formato: Some(formato),
        });
        self.mappa_sporca = true;
    }

    /// Il salto che accompagna un brano appena avviato: vedi
    /// [`Motore::apri_fermo`].
    ///
    /// Non è [`Contesto::vai_a`]: qui non c'è niente da buttare — l'anello è
    /// vuoto da un istante e nessun blocco è ancora stato decodificato — e
    /// nessuna puntina da rimettere a posto, perché il brano che si sentirà è
    /// questo. Si sposta il decodificatore e si corregge il segno appena
    /// annotato: comincia sempre al fotogramma d'anello zero, ma da più avanti
    /// nel brano. Il primo blocco dirà dove si è atterrati davvero, come dopo
    /// ogni salto.
    ///
    /// Un salto che fallisce lascia il brano dove il decodificatore è rimasto —
    /// di solito il suo inizio — come fa un salto fallito a musica in corso, e
    /// la finestra riceve l'errore per dirlo. Il segno segue il decodificatore
    /// in tutti e due i casi: con l'anello vuoto non c'è altro da tenere in
    /// pari.
    ///
    /// L'errore lo **restituisce**, e non lo annuncia: tocca a
    /// `esegui(Suona)` farlo, dopo aver contato la svolta. Annunciato da qui
    /// partiva con le svolte di prima, cioè già «superato».
    fn cerca_dall_avvio(&mut self, ms: u64) -> Result<(), AppError> {
        let Some(decodificatore) = self.corrente.as_mut() else {
            return Ok(());
        };
        self.condiviso.pubblica_lettura(decodificatore.bandiera());
        let cercato = decodificatore.cerca(ms);
        self.condiviso.ritira_lettura();
        let raggiunto = decodificatore.consegnati();
        if let Some(segno) = self.segni.back_mut() {
            segno.offset_fotogrammi = raggiunto;
        }
        self.riallinea_segno = true;
        self.mappa_sporca = true;
        cercato
    }

    /// Il numero di un ascolto che comincia adesso: vedi [`Segno::ascolto`].
    pub(super) const fn nuovo_ascolto(&mut self) -> u64 {
        self.ascolti = self.ascolti.wrapping_add(1);
        self.ascolti
    }

    /// Mette in canna il brano dopo, o toglie quello che c'era.
    ///
    /// Anche qui niente ramo d'errore: chi apre è chi chiama, e un successivo
    /// che non si apre lo scopre **lui**, mentre il corrente suona ancora — che
    /// era già il momento giusto per scoprirlo, solo dall'altra parte del
    /// confine. Vedi [`BranoAperto`].
    ///
    /// # Quando arriva dopo che il corrente è già finito
    ///
    /// Allora si attacca **adesso**, e non è un caso di scuola: è quel che
    /// succede a ogni brano più corto dell'anello. La decodifica corre avanti
    /// finché c'è posto davanti alla callback, e su un'uscita normale quel posto
    /// vale più di tre secondi di musica — vedi [`crate::uscita::RISERVA_MS`],
    /// che quel conto lo fa per esteso. Un intermezzo di due secondi è quindi
    /// decodificato **per intero** entro qualche decina di millisecondi dal
    /// `play`: [`Contesto::passa_al_prossimo`] scatta mentre dalle casse esce
    /// ancora l'inizio del brano, non trova niente in canna e azzera il
    /// corrente. Il preparatore — che parte dall'annuncio di `Iniziato`, aspetta
    /// l'antirimbalzo della raffica e poi apre un file — arriva qualche
    /// millisecondo più tardi, e arriva qui.
    ///
    /// Senza questa riga quel brano lasciava il motore in uno stato da cui non
    /// si usciva più: `corrente` vuoto, quindi
    /// [`Contesto::decodifica_un_blocco`] esce subito e non chiama mai
    /// `passa_al_prossimo`; `prossimo` pieno, quindi [`Contesto::forse_fine`]
    /// non dichiara la fine. Niente `Iniziato`, niente `Fermato`, la coda ferma
    /// e il cursore piantato sull'ultimo millisecondo — finché qualcuno non
    /// premeva un pulsante. Con un album di intermezzi la riproduzione moriva al
    /// primo.
    ///
    /// Attaccare qui non fa nessun buco: l'anello è ancora pieno di quel che
    /// resta del brano finito, e il segno che `passa_al_prossimo` annota cade su
    /// `spinti`, cioè esattamente dopo il suo ultimo campione. È lo stesso
    /// gapless di sempre, deciso qualche millisecondo più tardi.
    ///
    /// Non vale a fine dichiarata: lì il motore si è già detto fermo e la
    /// callback esce in silenzio, quindi attaccare un brano vorrebbe dire una
    /// musica che riparte da sola dopo che era finita — e senza che si senta,
    /// perché [`Condiviso::in_pausa`] è alzato. Chi prepara a motore fermo sta
    /// solo tenendo pronto il dopo per quando qualcuno ripremerà play.
    fn prepara(&mut self, brano: Option<BranoAperto>) {
        self.prossimo = brano;
        if self.corrente.is_none() && self.prossimo.is_some() && !self.fine_dichiarata {
            self.passa_al_prossimo();
        }
    }

    /// Butta tutto e si ferma.
    ///
    /// Anche la pausa si alza qui, e non soltanto in [`Motore::ferma`]: il filo
    /// si ferma anche da solo, quando la rete muore a metà brano, e un motore
    /// fermo che si dice «non in pausa» è un motore che la finestra disegna
    /// come se suonasse. La posizione vuota si scrive **prima** dell'annuncio,
    /// perché l'annuncio se la porta dietro — vedi [`Annuncio::posizione`].
    pub(super) fn ferma(&mut self) {
        self.scarta_in_volo();
        self.corrente = None;
        self.prossimo = None;
        self.azzera_dissolvenza();
        self.annunciato = None;
        if self.sfumatura.take().is_some() {
            self.applica_guadagno();
        }
        self.intese.pausa(&self.condiviso);
        self.scrivi_posizione(Posizione::default());
        if !self.fine_dichiarata {
            self.fine_dichiarata = true;
            self.annuncia(Evento::Fermato);
        }
    }

    /// Rimette sotto la puntina il brano che si **sente**, se la decodifica è
    /// già passata a quello dopo. [`Puntina::Persa`] se non c'è modo di farlo,
    /// e allora chi chiama deve lasciar perdere invece di andare avanti su
    /// `corrente`.
    ///
    /// # Perché serve
    ///
    /// Perché `corrente` non è il brano che esce dalle casse: è quello che il
    /// decodificatore sta macinando, e negli ultimi secondi di ogni brano i due
    /// sono diversi — la ragione, e il difetto che ne nasceva, stanno in
    /// [`Contesto::uscente`]. Chi deve agire su quel che si sente lo chiede a
    /// `segni.front()`, che è la stessa fonte da cui [`Contesto::aggiorna`]
    /// prende il titolo e la posizione mostrati.
    ///
    /// # Perché si confronta l'ascolto e non il brano
    ///
    /// Perché con «ripeti uno» il brano che si sente e quello che si decodifica
    /// hanno lo stesso identificativo e sono due ascolti diversi: il confronto
    /// sull'identificativo li dava per lo stesso, e il salto finiva sulla
    /// ripetizione successiva. Vedi [`Contesto::ascolto_corrente`].
    ///
    /// Lo scambio è quello che [`Contesto::puntina_in_dissolvenza`] fa per
    /// l'entrante di una dissolvenza nella sua prima metà: il brano che stava in
    /// canna torna a essere il **prossimo**, riavvolto al suo inizio, e quello
    /// di prima torna corrente. La dissolvenza non passa di qui: là i brani
    /// vivi sono tre, e la domanda è un'altra.
    fn riporta_sotto_la_puntina(&mut self) -> Puntina {
        let Some(testa) = self.segni.front() else {
            // Nessun segno: non c'è nessun brano che si sente, e non c'è niente
            // da riportare indietro.
            return Puntina::AlSuoPosto;
        };
        let ascolto = testa.ascolto;
        if self.corrente.is_some() && self.ascolto_corrente == ascolto {
            return Puntina::AlSuoPosto;
        }
        // Serve quello messo da parte, e serve che sia proprio l'ascolto in
        // testa: su brani più corti di quel che l'anello tiene da parte
        // `passa_al_prossimo` scatta due volte prima che il primo dei due
        // finisca di sentirsi, e il secondo passaggio si porta via il
        // decodificatore del primo. Lì non c'è niente da riportare indietro, e
        // il salto si lascia cadere.
        if self.uscente.as_ref().map(|u| u.ascolto) != Some(ascolto) {
            return Puntina::Persa;
        }
        // Con qualcosa già in canna lo scambio perderebbe un brano: quello che
        // sposta indietro andrebbe scritto dove sta il seguente. Non capita —
        // chi prepara non consegna finché l'annuncio non è arrivato — ma se
        // capitasse, un salto lasciato cadere è meglio di una coda che salta una
        // riga.
        if self.prossimo.is_some() {
            return Puntina::Persa;
        }
        let Some(uscente) = self.uscente.take() else {
            return Puntina::Persa;
        };
        // Quel che stava in canna torna a essere il prossimo, dal suo inizio:
        // l'anello con i suoi campioni sta per essere buttato, e riattaccarlo a
        // metà lo farebbe cominciare dove nessuno l'ha sentito. Se non sa
        // tornare al proprio inizio si lascia perdere, come per l'entrante.
        if let Some(decodificatore) = self.corrente.take() {
            let mut futuro = BranoAperto {
                decodificatore,
                durata_ms: self.durata_corrente_ms,
                livello: self.rg_decodifica,
                etichetta: self.etichetta_corrente,
            };
            if futuro.decodificatore.cerca(0).is_ok() {
                self.prossimo = Some(futuro);
            }
        }
        self.durata_corrente_ms = uscente.brano.durata_ms;
        self.rg_decodifica = uscente.brano.livello;
        self.etichetta_corrente = uscente.brano.etichetta;
        self.ascolto_corrente = uscente.ascolto;
        self.corrente = Some(uscente.brano.decodificatore);
        Puntina::Riportata
    }

    /// Il salto arriva a metà di una dissolvenza: su quale dei due brani cade.
    ///
    /// # La domanda è quale si sente
    ///
    /// Come per ogni altro salto, e la risposta la dà il segno in testa. A metà
    /// curva il brano che entra viene annunciato: prima di quel punto chi
    /// ascolta sente soprattutto quello che esce, dopo soprattutto quello che
    /// entra, e la finestra ne mostra già il titolo.
    ///
    /// **Nella prima metà** il salto è per il brano che esce: resta corrente, e
    /// quello che entrava torna in canna dal suo inizio. La coda di chi sta
    /// sopra non si è mossa, perché l'annuncio non è ancora partito.
    ///
    /// **Nella seconda metà** il salto è per il brano che entra, ed era qui il
    /// difetto. Il ramo di prima faceva sempre la cosa della prima metà: il
    /// salto cadeva sul brano che **usciva** mentre la finestra mostrava
    /// l'altro, e il brano che entrava, riavvolto, veniva scritto in canna
    /// **sopra** il successivo — che chi sta sopra aveva già consegnato,
    /// rispondendo all'annuncio di metà curva. Il motore lo perdeva, chi sta
    /// sopra lo credeva ancora in canna e non lo riapriva più: dopo il brano
    /// che entrava, la musica si fermava. Adesso l'uscente se ne va — ne
    /// restavano pochi secondi a volume calante — l'entrante diventa corrente
    /// e il successivo resta dov'è.
    ///
    /// La canna si riempie solo se è vuota, anche nella prima metà: se chi sta
    /// sopra ci ha messo altro nel frattempo — la coda è cambiata — quel che ha
    /// scelto vale più di un brano che la coda non indica più.
    fn puntina_in_dissolvenza(&mut self) -> Puntina {
        let Some(mut entrante) = self.entrante.take() else {
            return Puntina::AlSuoPosto;
        };
        let si_sente_l_entrante = self
            .segni
            .front()
            .is_some_and(|segno| segno.ascolto == self.ascolto_entrante);
        if si_sente_l_entrante {
            self.durata_corrente_ms = entrante.durata_ms;
            self.rg_decodifica = entrante.livello;
            self.etichetta_corrente = entrante.etichetta;
            self.ascolto_corrente = self.ascolto_entrante;
            self.corrente = Some(entrante.decodificatore);
        } else if self.prossimo.is_none() && entrante.decodificatore.cerca(0).is_ok() {
            self.prossimo = Some(entrante);
        }
        Puntina::Riportata
    }

    fn vai_a(&mut self, ms: u64) {
        // Il salto cade sul brano che si sente, non su quello che si decodifica:
        // vedi [`Contesto::riporta_sotto_la_puntina`] e, a metà di una
        // dissolvenza, [`Contesto::puntina_in_dissolvenza`]. Quando i due non
        // sono lo stesso e non c'è modo di rimetterli in fila, il salto si
        // lascia cadere — la puntina resta dov'era, che è quel che fa anche un
        // salto fallito qui sotto.
        let puntina = if self.entrante.is_some() {
            self.puntina_in_dissolvenza()
        } else {
            self.riporta_sotto_la_puntina()
        };
        if matches!(puntina, Puntina::Persa) {
            return;
        }
        let Some(decodificatore) = self.corrente.as_mut() else {
            return;
        };
        let track_id = decodificatore.track_id();
        // Un salto su un flusso può aspettare la rete, e il salto dopo deve
        // poterlo interrompere: vedi `Condiviso::in_lettura`.
        self.condiviso.pubblica_lettura(decodificatore.bandiera());
        let cercato = decodificatore.cerca(ms);
        self.condiviso.ritira_lettura();
        let raggiunto = match cercato {
            // Dove si è atterrati, che non è sempre dove si era chiesto: sopra
            // la fine del flusso `Decodificatore::cerca` mette un tetto invece
            // di fallire, e scrivere qui il millisecondo **chiesto** manderebbe
            // il cursore fuori dalla barra per gli ultimi istanti del brano. In
            // fotogrammi e non in millisecondi: vedi `Segno::offset_fotogrammi`.
            Ok(()) => decodificatore.consegnati(),
            Err(err) => {
                let dov_e = decodificatore.consegnati();
                // Si annuncia e si torna indietro, **senza** fermare: un salto
                // che non riesce lascia la puntina dov'era, e dov'era è ancora
                // un posto buono da cui continuare.
                //
                // Vale anche quando il guasto viene dalla rete, ed è voluto:
                // `Decodificatore::cerca` lo riconosce per quel che è, quindi il
                // codice che esce di qui è già ritentabile — la finestra si
                // annota il punto e disegna «Riprova». Se poi la share è davvero
                // morta, la prima lettura che segue lo scopre e **là** il motore
                // si ferma, dal ramo di rete di
                // [`Contesto::decodifica_un_blocco`]. Fermarsi già qui vorrebbe
                // dire buttare via un brano per un salto fallito su una rete che
                // magari sta solo respirando male.
                //
                // Un salto **annullato** non si annuncia: lo ha interrotto il
                // comando dopo, che è già in fila, e un «Riprova» per un salto
                // che nessuno vuole più sarebbe un avviso sbagliato.
                if err.code().kind() != ErrorCodeKind::InternalAborted {
                    self.annuncia(Evento::Errore(Box::new(err)));
                }
                if matches!(puntina, Puntina::AlSuoPosto) {
                    return;
                }
                // Ma se per saltare si sono rimessi in fila i decodificatori,
                // l'anello non racconta più quel che verrà: contiene il seguito
                // di un brano che non è più corrente, o una dissolvenza con uno
                // dei due brani tolto di mezzo. Lo si butta lo stesso, e si
                // riparte da dove il decodificatore rimasto è arrivato.
                dov_e
            }
        };
        self.scarta_in_volo();
        // Una sovrapposizione in corso non sopravvive a un salto: i campioni
        // già mescolati sono appena stati buttati, e quale dei due brani resta
        // l'ha già deciso [`Contesto::puntina_in_dissolvenza`].
        self.azzera_dissolvenza();
        // L'ascolto è quello del brano rimasto sotto la puntina, cioè di quello
        // che si sentiva. Se non era ancora stato annunciato — un salto un
        // istante dopo il play — lo sarà col segno nuovo; quello già annunciato
        // non lo sarà una seconda volta, perché il conteggio dell'ascolto
        // ripartirebbe da capo e un salto farebbe avanzare la coda. Durata e
        // ReplayGain sono i suoi per la stessa ragione: dopo la scelta di chi
        // salta, `corrente` e le sue caselle parlano dello stesso brano.
        self.segni.push_back(Segno {
            da: 0,
            track_id,
            durata_ms: self.durata_corrente_ms,
            offset_fotogrammi: raggiunto,
            etichetta: self.etichetta_corrente,
            replaygain_db: self.rg_decodifica.guadagno_db,
            ascolto: self.ascolto_corrente,
            formato: self.corrente.as_ref().map(Decodificatore::formato_suonato),
        });
        self.fine_dichiarata = false;
        self.riallinea_segno = true;
        self.mappa_sporca = true;
    }
}
