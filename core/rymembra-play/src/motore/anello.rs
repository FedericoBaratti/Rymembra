//! Riempire l'anello: la decodifica a blocchi col suo bilancio di tempo, la
//! dissolvenza fra due brani e il passaggio senza buco al brano dopo.

use std::sync::atomic::Ordering;
use std::time::Instant;

use rymembra_domain::errors::ErrorCodeKind;

use super::{
    BILANCIO_RIEMPI, BranoAperto, Contesto, Evento, RIPRESA_MS, Segno, Uscente, fotogrammi,
    fotogrammi_da_ms,
};
use crate::{correzione, dissolvi, punto_curva, risali};

impl Contesto {
    /// Riempie l'anello finché c'è posto e c'è roba. `true` se ha fatto qualcosa.
    ///
    /// # Perché a pezzi e non un campione alla volta
    ///
    /// Perché un `push` per campione è un confronto fra i due indici
    /// dell'anello — con una lettura atomica dell'altro filo quando la copia in
    /// cache non basta — per ognuno dei quasi due milioni di campioni al secondo
    /// di un'uscita a 192 kHz su otto canali. Un pezzo chiede il posto libero
    /// una volta, copia tutto quel che ci sta e pubblica il nuovo indice una
    /// volta sola. Il suono è lo stesso; il filo della decodifica torna prima a
    /// fare il suo mestiere, che è decodificare.
    ///
    /// Alla fine pubblica quanti fotogrammi sono entrati davvero nell'anello:
    /// vedi `Condiviso::spinti`.
    pub(super) fn riempi(&mut self) -> bool {
        let fatto = self.riempi_anello();
        // Non `self.spinti`: quello conta anche i campioni ancora in `resto`,
        // decodificati e non ancora entrati. La riserva che si può saltare è
        // quella che la callback ha già davanti.
        let fuori = fotogrammi(self.resto.len(), self.formato.canali);
        self.condiviso
            .spinti
            .store(self.spinti.saturating_sub(fuori), Ordering::Relaxed);
        fatto
    }

    /// Il corpo di [`Contesto::riempi`].
    ///
    /// # Perché un bilancio di tempo
    ///
    /// Perché [`filo`] esegue i comandi, avanza equalizzatore e sfumatura e
    /// aggiorna la posizione **fra** una chiamata e l'altra, mai durante. Senza
    /// un tetto questa tornava solo ad anello pieno o a decodificatore secco:
    /// con una sorgente che consegna appena più svelta del tempo reale — una
    /// condivisione lenta, un nodo di rete stanco — riempire tre secondi
    /// d'anello voleva dire restare qui per secondi dopo ogni avvio o salto,
    /// e con una più lenta del tempo reale per tutto il brano. Nel frattempo
    /// volume, muto, sfumatura del timer e brano dopo aspettavano in coda, il
    /// cursore restava fermo e l'`Iniziato` arrivava in ritardo.
    ///
    /// Passato [`BILANCIO_RIEMPI`] si torna con `true`: il filo non dorme, fa
    /// il suo giro e riprende subito a riempire, quindi la decodifica non
    /// rende di meno. Resta fuori quel che un bilancio non può prendere: una
    /// lettura sola che aspetta la rete — una finestra di un flusso HTTP — dura
    /// quel che dura, dentro una sola chiamata a
    /// [`Contesto::decodifica_un_blocco`].
    fn riempi_anello(&mut self) -> bool {
        let scadenza = Instant::now() + BILANCIO_RIEMPI;
        let mut fatto = false;
        loop {
            if !self.resto.is_empty() {
                // Uno svuotamento chiesto e non ancora fatto: la stretta di
                // mano di `scarta_in_volo` è scaduta, e la callback, quando
                // tornerà a girare, butterà tutto quel che trova nell'anello
                // senza contarlo. Quel che si spingesse adesso sarebbe un
                // debito per sempre; aspetta in `resto`, al più un blocco.
                let quanti = if self.condiviso.svuota.load(Ordering::Acquire) {
                    0
                } else {
                    self.produttore.slots().min(self.resto.len())
                };
                if quanti > 0
                    && let Ok(pezzo) = self.produttore.write_chunk_uninit(quanti)
                {
                    // Si copiano e **poi** si tolgono, contando quelli scritti
                    // davvero: un campione tolto da `resto` e non arrivato
                    // nell'anello sarebbe un buco nel suono.
                    let scritti = pezzo.fill_from_iter(self.resto.iter().copied());
                    self.resto.drain(..scritti);
                    fatto |= scritti > 0;
                }
                if !self.resto.is_empty() {
                    // Anello pieno, o svuotamento in sospeso: si riproverà al
                    // giro dopo.
                    return fatto;
                }
            }
            // Il tetto si guarda qui, con quel che era stato decodificato già
            // entrato nell'anello: si torna al filo senza lasciare niente in
            // `resto` che potesse già suonare.
            if fatto && Instant::now() >= scadenza {
                return true;
            }
            if !self.decodifica_un_blocco() {
                return fatto;
            }
            fatto = true;
        }
    }

    /// Un blocco dal decodificatore corrente dentro `resto`. `false` se non c'è
    /// più niente da decodificare.
    ///
    /// # La giuntura senza stacco
    ///
    /// Quando il brano dopo è già in canna, senza una dissolvenza in mezzo e
    /// con la stessa forma di questo, la fine del flusso non consegna la coda
    /// del ricampionatore: la lascia nel filtro, e il filtro passa al seguito
    /// con [`Decodificatore::passa_la_coda`]. Così la giuntura di un album
    /// senza stacchi la attraversa un ricampionatore solo — il perché sta in
    /// testa a `decodifica/ricampionatore.rs` — e il segno del brano nuovo si
    /// sposta avanti dei fotogrammi di coda che usciranno insieme ai suoi
    /// primi.
    fn decodifica_un_blocco(&mut self) -> bool {
        let Some(decodificatore) = self.corrente.as_mut() else {
            return false;
        };
        let lascia = self.entrante.is_none()
            && self
                .prossimo
                .as_ref()
                .is_some_and(|p| decodificatore.puo_passare_la_coda(&p.decodificatore));
        // Letto **prima** della consegna: dopo, il decodificatore li ha già
        // tolti dal conto.
        let altrui = decodificatore.ereditati();
        // Questa lettura si può lasciare a metà: vedi `Condiviso::in_lettura`.
        self.condiviso.pubblica_lettura(decodificatore.bandiera());
        let esito = if lascia {
            decodificatore.prossimo_lasciando_la_coda(&mut self.blocco)
        } else {
            decodificatore.prossimo(&mut self.blocco)
        };
        self.condiviso.ritira_lettura();
        match esito {
            // Annullata da un comando arrivato mentre si aspettava la rete: non
            // è un guasto, e non c'è niente da fare in questo giro. Il comando
            // lo raccoglie il giro dopo — di solito un salto, che riporta il
            // decodificatore in un punto pulito.
            Err(err) if err.code().kind() == ErrorCodeKind::InternalAborted => false,
            Ok(true) => {
                let arrivati = decodificatore.consegnati();
                self.accoda_il_blocco(arrivati, altrui);
                true
            }
            Ok(false) if decodificatore.coda_sospesa() => {
                let passati = self
                    .prossimo
                    .as_mut()
                    .and_then(|p| decodificatore.passa_la_coda(&mut p.decodificatore));
                if let Some(rinvio) = passati {
                    self.rg_altrui = self.rg_decodifica;
                    return self.passa_al_prossimo_dopo(rinvio);
                }
                // Non capita: fra la domanda e la fine del flusso non è
                // cambiato niente. Ma se capitasse, la coda esce come a ogni
                // fine brano, imbottita di silenzio, e non si perde.
                match decodificatore.consegna_la_coda(&mut self.blocco) {
                    Ok(true) => {
                        let arrivati = decodificatore.consegnati();
                        self.accoda_il_blocco(arrivati, 0);
                        true
                    }
                    Ok(false) => self.passa_al_prossimo(),
                    Err(err) => {
                        self.annuncia(Evento::Errore(Box::new(err)));
                        self.passa_al_prossimo()
                    }
                }
            }
            Ok(false) => self.passa_al_prossimo(),
            Err(err) => {
                // # Perché un guasto di rete non fa passare al brano dopo
                //
                // Perché il brano dopo, con ogni probabilità, sta sulla stessa
                // condivisione che è appena morta. Passare a lui vuol dire
                // un'altra attesa di quaranta secondi, un altro errore, e così
                // via lungo tutta la coda: in un minuto l'utente ha perso il
                // punto in cui stava ascoltando, ha ricevuto una raffica di
                // avvisi e si ritrova la coda finita. Un cavo staccato non deve
                // costare la serata.
                //
                // Fermarsi invece la conserva: il brano resta quello, la
                // posizione pure, e la finestra offre «Riprova» perché il codice
                // che arriva è ritentabile. Vedi `Decodificatore::guasto`.
                //
                // Ogni altro guasto continua a far saltare il brano, ed è
                // giusto: un file davvero rotto non si aggiusta aspettando, e
                // fermare la riproduzione su di lui vorrebbe dire che un album
                // con un file corrotto in mezzo non arriva più in fondo.
                let di_rete = err.code().kind() == ErrorCodeKind::FsNetworkUnavailable;
                self.annuncia(Evento::Errore(Box::new(err)));
                if di_rete {
                    self.ferma();
                    return false;
                }
                self.passa_al_prossimo()
            }
        }
    }

    /// Il blocco appena decodificato, dalla correzione del brano fino a `resto`.
    ///
    /// `altrui` sono i fotogrammi in testa al blocco che appartengono ancora al
    /// brano prima — la coda arrivata col ricampionatore, vedi
    /// [`Contesto::decodifica_un_blocco`] — e che prendono la **sua**
    /// correzione.
    fn accoda_il_blocco(&mut self, arrivati: u64, altrui: u64) {
        self.riallinea_dopo_il_salto(arrivati);
        // La correzione del brano si applica ai **suoi** campioni, adesso,
        // invece che a tutto quel che esce dall'anello. È l'unico modo perché
        // due brani sovrapposti possano avere due ReplayGain diversi. Il filtro
        // dell'equalizzatore è lineare, quindi moltiplicare prima o dopo di lui
        // dà lo stesso suono; il limitatore in fondo alla catena no, e il
        // guadagno che la callback legge non dice più niente di questa
        // correzione: i blocchi alzati qui glieli segnala `piega_fino`, in
        // fondo a questa funzione.
        let g = correzione(
            self.rg_decodifica,
            self.replaygain_attivo,
            self.bersaglio_db,
        );
        let testa = usize::try_from(altrui)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::from(self.formato.canali))
            .min(self.blocco.len());
        let (di_prima, suoi) = self.blocco.split_at_mut(testa);
        let mut alzato = g > 1.0 && !suoi.is_empty();
        if !di_prima.is_empty() {
            let g_prima = correzione(self.rg_altrui, self.replaygain_attivo, self.bersaglio_db);
            alzato |= g_prima > 1.0;
            if (g_prima - 1.0).abs() > f32::EPSILON {
                for campione in di_prima {
                    *campione *= g_prima;
                }
            }
        }
        if (g - 1.0).abs() > f32::EPSILON {
            for campione in suoi {
                *campione *= g;
            }
        }
        // Prima la salita del brano che è rimasto solo a metà curva, poi
        // l'eventuale curva nuova: sono due cose diverse che possono capitare
        // allo stesso blocco — un brano brevissimo fa in tempo a finire di
        // entrare e a cominciare a uscire — e in quel caso i due guadagni si
        // moltiplicano, che è esattamente quel che deve succedere.
        //
        // Tutte e due sono una sovrapposizione in corso, da far piegare: la
        // dissolvenza somma due brani, e i picchi che coincidono passano
        // l'unità anche con la normalizzazione spenta.
        alzato |= self.entrata_durata > 0;
        self.forse_risali();
        self.forse_dissolvi();
        alzato |= self.entrante.is_some();
        self.resto.extend(self.blocco.iter().copied());
        self.spinti = self
            .spinti
            .saturating_add(fotogrammi(self.blocco.len(), self.formato.canali));
        if alzato {
            self.segna_alzati();
        }
    }

    /// I fotogrammi spinti fin qui contengono campioni alzati: la callback li
    /// passerà per il limitatore. Vedi `Condiviso::piega_fino`.
    ///
    /// **Prima** che entrino nell'anello: chi li consuma deve trovare il numero
    /// già scritto. `spinti` conta anche quel che aspetta in `resto`, ed è lo
    /// stesso conteggio dei fotogrammi della callback — si azzerano insieme in
    /// [`Contesto::scarta_in_volo`] — quindi il confronto della callback è fra
    /// due numeri della stessa epoca.
    fn segna_alzati(&self) {
        self.condiviso
            .piega_fino
            .store(self.spinti, Ordering::Relaxed);
    }

    /// Il primo blocco dopo un salto dice dove si è atterrati: il segno del
    /// salto si corregge su quello. Vedi [`Contesto::riallinea_segno`].
    ///
    /// `arrivati` sono i fotogrammi consegnati dal decodificatore **dopo** il
    /// blocco: l'inizio del blocco è quello, meno il blocco. Si corregge solo il
    /// segno del salto — l'ultimo in fila, annotato al fotogramma d'uscita zero
    /// e ancora a zero fotogrammi spinti — e mai uno arrivato dopo.
    fn riallinea_dopo_il_salto(&mut self, arrivati: u64) {
        if !std::mem::take(&mut self.riallinea_segno) {
            return;
        }
        let inizio = arrivati.saturating_sub(fotogrammi(self.blocco.len(), self.formato.canali));
        let (spinti, ascolto) = (self.spinti, self.ascolto_corrente);
        if let Some(segno) = self.segni.back_mut()
            && segno.da == spinti
            && segno.ascolto == ascolto
        {
            // Il fotogramma esatto, non il millisecondo: è l'inizio vero del
            // primo blocco, ed è quel che l'ancora deve poter ritrovare.
            segno.offset_fotogrammi = inizio;
            self.mappa_sporca = true;
        }
    }

    /// Quanti fotogrammi dura la sovrapposizione, se ci sarà.
    ///
    /// Zero quando la dissolvenza è spenta, quando non c'è un brano dopo, o
    /// quando la durata del corrente non si sa: senza sapere dove finisce non
    /// c'è modo di sapere quando cominciare, e cominciare a caso vorrebbe dire
    /// tagliare un brano nel mezzo.
    /// Una volta cominciata vale quella con cui è cominciata: vedi
    /// [`Contesto::dissolvenza_durata`].
    ///
    /// Zero anche fra due tracce contigue della stessa immagine CD, con la
    /// dissolvenza accesa: sono un segnale solo che il foglio ha tagliato in
    /// due, e sovrapporle vorrebbe dire far sentire due volte, sfasate, gli
    /// ultimi secondi di una e i primi dell'altra. Il passaggio è il gapless,
    /// col ricampionatore che attraversa la giuntura. Quali tracce siano
    /// contigue — poche, e per ragioni precise — lo dice
    /// [`crate::Segmento::precede`].
    fn fotogrammi_dissolvenza(&self) -> u64 {
        if self.entrante.is_some() {
            return self.dissolvenza_durata;
        }
        if self.dissolvenza_ms == 0 || self.durata_corrente_ms == 0 {
            return 0;
        }
        let (Some(corrente), Some(prossimo)) = (self.corrente.as_ref(), self.prossimo.as_ref())
        else {
            return 0;
        };
        if corrente.contiguo_a(&prossimo.decodificatore) {
            return 0;
        }
        fotogrammi_da_ms(self.dissolvenza_ms, self.formato.frequenza)
    }

    /// La sovrapposizione è finita: resta solo un brano.
    ///
    /// Non tocca la ripresa: quando la curva si è interrotta a metà, la salita
    /// del brano rimasto solo comincia proprio qui e deve sopravvivere a questa
    /// chiamata. Vedi [`Contesto::forse_riprendi`].
    fn chiudi_sovrapposizione(&mut self) {
        self.entrante = None;
        self.coda_entrante.clear();
        self.dissolvenza_fatti = 0;
        self.dissolvenza_durata = 0;
        self.segno_dissolvenza = false;
    }

    /// Butta via una sovrapposizione, con tutto quel che ci stava dentro.
    ///
    /// Questa invece spegne anche la ripresa: la chiamano un brano nuovo, uno
    /// stop e un salto, cioè i tre casi in cui quel che stava succedendo non
    /// deve lasciare tracce nei campioni che verranno.
    pub(super) fn azzera_dissolvenza(&mut self) {
        self.chiudi_sovrapposizione();
        self.entrata_durata = 0;
        self.entrata_fatti = 0;
        self.entrata_da = 1.0;
    }

    /// Il brano uscente è finito prima della fine della curva: prepara la
    /// salita che porterà quello entrante a piena ampiezza.
    ///
    /// Il guadagno di partenza è quello che la curva gli aveva dato all'ultimo
    /// blocco mescolato, letto dalla stessa [`punto_curva`] che l'ha
    /// applicato: è così che la ripresa attacca senza gradino. Se invece la
    /// curva era finita — il caso normale — il brano è già a piena ampiezza e
    /// non c'è niente da riprendere.
    fn forse_riprendi(&mut self) {
        let restanti = self
            .dissolvenza_durata
            .saturating_sub(self.dissolvenza_fatti);
        if restanti == 0 {
            return;
        }
        let (_, dentro) = punto_curva(self.dissolvenza_fatti, self.dissolvenza_durata);
        self.entrata_da = dentro;
        self.entrata_fatti = 0;
        // Mai più lunga di quel che restava della curva: se mancava un decimo
        // di quaranta millisecondi, il brano era già quasi a piena ampiezza e
        // allungare la salita vorrebbe dire inventare un livello che non c'era.
        self.entrata_durata = fotogrammi_da_ms(RIPRESA_MS, self.formato.frequenza).min(restanti);
    }

    /// Un pezzo di quella salita, sul blocco appena decodificato.
    fn forse_risali(&mut self) {
        if self.entrata_durata == 0 {
            return;
        }
        let canali = self.formato.canali;
        risali(
            &mut self.blocco,
            self.entrata_da,
            self.entrata_fatti,
            self.entrata_durata,
            canali,
        );
        self.entrata_fatti = self
            .entrata_fatti
            .saturating_add(fotogrammi(self.blocco.len(), canali));
        if self.entrata_fatti >= self.entrata_durata {
            self.entrata_durata = 0;
        }
    }

    /// Tiene nella coda dell'entrante almeno tanti campioni, se ne ha ancora.
    ///
    /// Decodifica quanto serve, e non un pacchetto per blocco: le due lunghezze
    /// non hanno nessuna ragione di coincidere — dipendono dal codec di due file
    /// diversi — e trattarle come se coincidessero vuol dire perdere o
    /// inventare campioni a ogni giro. Vedi [`Contesto::coda_entrante`].
    fn riempi_entrante(&mut self, quanti: usize) {
        let attivo = self.replaygain_attivo;
        let bersaglio = self.bersaglio_db;
        let Some(entrante) = self.entrante.as_mut() else {
            return;
        };
        // Il brano che entra porta la **sua** correzione, non quella di quello
        // che esce.
        let g = correzione(entrante.livello, attivo, bersaglio);
        while self.coda_entrante.len() < quanti {
            // Anche questa lettura si può lasciare a metà, come quella del
            // corrente in [`Contesto::decodifica_un_blocco`]: il brano che entra
            // in dissolvenza sta spesso sulla stessa cartella di rete, e una
            // share che si pianta qui teneva il filo dentro la sua `ReadFile`
            // senza che il «prossimo» o lo stop premuti nel frattempo potessero
            // farlo uscire — la sua bandiera non la pubblicava nessuno.
            self.condiviso
                .pubblica_lettura(entrante.decodificatore.bandiera());
            let esito = entrante.decodificatore.prossimo(&mut self.blocco_entrante);
            self.condiviso.ritira_lettura();
            // Un `let ... else` e non un `match`: gli altri casi — il brano
            // entrante finito, illeggibile, o lasciato a metà da un comando —
            // vogliono la stessa cosa, cioè che non succeda niente. Una
            // dissolvenza mancata è un gapless, che è il comportamento di
            // prima; non è un guasto da segnalare a nessuno, e il comando che
            // ha interrotto la lettura lo raccoglie il giro dopo.
            let Ok(true) = esito else {
                return;
            };
            if (g - 1.0).abs() > f32::EPSILON {
                for campione in &mut self.blocco_entrante {
                    *campione *= g;
                }
            }
            self.coda_entrante
                .extend(self.blocco_entrante.iter().copied());
        }
    }

    /// Mescola il brano che entra dentro il blocco di quello che esce.
    ///
    /// # Perché qui e non nella callback
    ///
    /// Perché l'anello fra i due fili è un flusso piatto di `f32` **senza
    /// confini di traccia**: chi legge di là non sa dove finisce un brano e
    /// comincia l'altro, e non potrebbe saperlo senza un secondo anello e un
    /// secondo decodificatore dentro il percorso realtime — dove non si può né
    /// allocare né prendere un lucchetto. Di qua invece i due decodificatori ci
    /// sono già entrambi: `prossimo` è aperto da quando il brano corrente è
    /// cominciato, ed è quel che rende il gapless gapless.
    fn forse_dissolvi(&mut self) {
        let durata = self.fotogrammi_dissolvenza();
        if durata == 0 {
            return;
        }
        let Some(corrente) = self.corrente.as_ref() else {
            return;
        };
        // Quanto manca alla fine, secondo quel che il decodificatore ha già
        // consegnato. Una volta cominciata, la dissolvenza va avanti anche se
        // il conto dicesse il contrario: fermarla a metà lascerebbe il brano
        // entrante a mezza ampiezza.
        let totali = fotogrammi_da_ms(self.durata_corrente_ms, self.formato.frequenza);
        let mancano = totali.saturating_sub(corrente.consegnati());
        if mancano > durata && self.dissolvenza_fatti == 0 {
            return;
        }

        // La sovrapposizione comincia: il brano preparato lascia la casella del
        // «prossimo» e passa in quella dell'«entrante», dove nessuno lo
        // sostituirà. Da qui in poi `prossimo` è libero di ricevere il brano
        // ancora dopo — cosa che succede fra pochi millisecondi, appena
        // l'annuncio di metà curva arriva a chi sta sopra. Vedi
        // [`Contesto::entrante`].
        if self.entrante.is_none() {
            let Some(preparato) = self.prossimo.take() else {
                return;
            };
            self.entrante = Some(preparato);
            self.coda_entrante.clear();
            self.dissolvenza_durata = durata;
        }

        // Il segno del brano entrante si annota **a metà** della
        // sovrapposizione, e nel futuro: `Segno.da` è in fotogrammi d'uscita, e
        // `aggiorna` lo raccoglie quando il contatore lo raggiunge. Metà e non
        // l'inizio perché è lì che il brano nuovo diventa quello che si sta
        // ascoltando: prima è un sottofondo sotto quello vecchio, e annunciarlo
        // allora vorrebbe dire una finestra che cambia titolo mentre si sente
        // ancora l'altro — e uno scrobble attribuito al brano sbagliato.
        //
        // # `da` e `offset_fotogrammi` descrivono lo stesso istante
        //
        // È la regola di tutto il tipo [`Segno`], e qui era rotta. `da` dice
        // *quando* si sente — il fotogramma d'uscita — e `offset_fotogrammi`
        // dice *dove*
        // si è, dentro il brano, in quell'istante. Devono parlare dello stesso
        // momento, altrimenti `aggiorna` somma una posizione a un tempo scorso
        // che parte da un'altra parte.
        //
        // Questo blocco è il primo che contiene audio dell'entrante: il suo
        // fotogramma zero esce a `self.spinti`. Quindi quando il contatore
        // arriva a `spinti + meta`, l'entrante ha già prodotto `meta`
        // fotogrammi — è a metà dissolvenza del *suo* inizio, non al suo inizio.
        // Con un offset a zero, com'era scritto qui, la posizione riportata
        // restava indietro di mezza dissolvenza **per tutto il brano**: con sei
        // secondi di dissolvenza, tre secondi, dal primo all'ultimo istante. È
        // la causa numero uno dello sfasamento dei testi, e sballava insieme
        // scrubber, «riprendi dov'eri» e il pannello di Windows.
        //
        // # Metà di quel che si sovrappone davvero
        //
        // Che non è sempre la dissolvenza intera: dopo un salto negli ultimi
        // secondi, o con un brano più corto della dissolvenza, all'uscente
        // resta meno della curva — i fotogrammi di questo blocco più quelli che
        // mancano. Il segno a metà della curva intera cadeva **dopo** la fine
        // dell'uscente: con dodici secondi di dissolvenza e uno di brano, per
        // cinque secondi si sentiva solo il brano dopo, e il motore raccontava
        // quello di prima a ventisette secondi su ventidue.
        if !self.segno_dissolvenza {
            let sovrapposti = durata
                .min(mancano.saturating_add(fotogrammi(self.blocco.len(), self.formato.canali)));
            #[expect(
                clippy::integer_division,
                reason = "metà dissolvenza: mezzo fotogramma non sposta il \
                          punto in cui il brano nuovo prende il sopravvento"
            )]
            let meta = sovrapposti / 2;
            let ascolto = self.nuovo_ascolto();
            // Il numero si tiene da parte: un salto a metà curva lo confronta
            // con quello in testa per sapere quale dei due brani si sente. Vedi
            // [`Contesto::puntina_in_dissolvenza`].
            self.ascolto_entrante = ascolto;
            if let Some(preparato) = self.entrante.as_ref() {
                self.segni.push_back(Segno {
                    da: self.spinti.saturating_add(meta),
                    track_id: preparato.decodificatore.track_id(),
                    durata_ms: preparato.durata_ms,
                    offset_fotogrammi: meta,
                    etichetta: preparato.etichetta,
                    replaygain_db: preparato.livello.guadagno_db,
                    ascolto,
                    formato: Some(preparato.decodificatore.formato_suonato()),
                });
                self.mappa_sporca = true;
            }
            self.segno_dissolvenza = true;
            self.fine_dichiarata = false;
        }

        // Tanti campioni quanti ne mescola questo blocco, né uno di meno né uno
        // buttato: quel che il decodificatore dell'entrante consegna in più
        // resta in coda per il blocco dopo.
        self.riempi_entrante(self.blocco.len());

        let canali = self.formato.canali;
        let fatti = self.dissolvenza_fatti;
        dissolvi(&mut self.blocco, &self.coda_entrante, fatti, durata, canali);
        // `dissolvi` si ferma dove finisce il blocco uscente: consumati sono
        // quelli che ci sono entrati davvero. Se l'entrante ne aveva di meno —
        // il suo ultimo blocco, o un brano più corto della sovrapposizione — la
        // coda si svuota e quel che manca è silenzio, cioè un gapless.
        let consumati = self.coda_entrante.len().min(self.blocco.len());
        self.coda_entrante.drain(..consumati);
        // La curva avanza col brano che **esce**: è lui a dettare il tempo
        // d'uscita, ed è la sua fine il punto in cui la sovrapposizione deve
        // essere finita.
        self.dissolvenza_fatti = fatti.saturating_add(fotogrammi(self.blocco.len(), canali));
    }

    /// Il corrente è finito: attacca il preparato, se c'è.
    ///
    /// Il segno si annota **adesso**, al fotogramma dove finisce quel che è già
    /// stato spinto: è lì che il brano nuovo comincerà a sentirsi, e non un
    /// campione prima. È tutto quel che serve perché il gapless sia gapless.
    pub(super) fn passa_al_prossimo(&mut self) -> bool {
        self.passa_al_prossimo_dopo(0)
    }

    /// Come [`Contesto::passa_al_prossimo`], con il brano nuovo che comincia
    /// a sentirsi `rinvio` fotogrammi dopo quel che è già stato spinto: la coda
    /// del corrente che gli è passata col ricampionatore, e che uscirà in testa
    /// ai suoi primi campioni. Vedi [`Contesto::decodifica_un_blocco`].
    fn passa_al_prossimo_dopo(&mut self, rinvio: u64) -> bool {
        // Chi prende il posto è l'entrante, se una sovrapposizione era in
        // corso: è lui che si stava già sentendo, ed è già avanti di tutta la
        // dissolvenza. In quel caso `prossimo` è il brano ancora dopo e deve
        // restare dov'è — prenderlo qui vorrebbe dire saltarne uno.
        let dissolveva = self.entrante.is_some();
        // Il brano che esce dalla puntina non esce dalle casse: quel che ha
        // decodificato è ancora dentro l'anello, e per i prossimi secondi è
        // **lui** quello che si sente. Si mette da parte invece di lasciarlo
        // cadere, perché un salto che arriva adesso deve poterlo ritrovare.
        // Vedi [`Contesto::uscente`].
        //
        // Prima del `match` e non dentro il ramo che attacca il successivo:
        // l'ultimo brano di una coda finisce di decodificarsi con la stessa
        // manciata di secondi d'anticipo, e un salto in quei secondi è
        // esattamente il caso di chi vuole risentire la fine di un brano prima
        // che la musica smetta.
        //
        // Durata e ReplayGain sono ancora i suoi: chi li sovrascrive sta più in
        // basso. L'ascolto pure, ed è [`Contesto::ascolto_corrente`]: prima lo
        // si cercava fra i segni per identificativo, e con «ripeti uno» sotto
        // una dissolvenza il segno trovato era quello dell'entrante — lo stesso
        // brano, l'ascolto dopo.
        //
        // Si scrive solo se c'è davvero qualcosa da mettere da parte: questa
        // funzione viene chiamata una seconda volta quando un preparato arriva
        // **dopo** che il corrente è finito — vedi [`Contesto::prepara`] — e lì
        // `corrente` è già vuoto. Assegnare comunque cancellerebbe il brano che
        // in quel momento si sta ancora sentendo, che è esattamente quello
        // messo da parte al giro di prima.
        if let Some(decodificatore) = self.corrente.take() {
            self.uscente = Some(Uscente {
                ascolto: self.ascolto_corrente,
                brano: BranoAperto {
                    decodificatore,
                    durata_ms: self.durata_corrente_ms,
                    livello: self.rg_decodifica,
                    etichetta: self.etichetta_corrente,
                },
            });
        }
        match self.entrante.take().or_else(|| self.prossimo.take()) {
            Some(preparato) => {
                let track_id = preparato.decodificatore.track_id();
                // Con una dissolvenza in corso il segno è già stato annotato a
                // metà sovrapposizione, e rimetterlo qui vorrebbe dire lo
                // stesso brano che comincia due volte.
                //
                // # Perché qui `offset_fotogrammi` è zero, e non va «uniformato»
                //
                // Perché qui il brano attacca **dal suo primo campione**: senza
                // dissolvenza il passaggio è un gapless, e il fotogramma zero
                // dell'entrante esce esattamente a `self.spinti` — più la coda
                // del corrente, quando è passata col ricampionatore e uscirà
                // davanti a lui: è `rinvio`. `da` e
                // `offset_fotogrammi` descrivono lo stesso istante — la regola di
                // [`Segno`] — e quell'istante è l'inizio del brano, quindi zero è
                // la verità.
                //
                // In [`Contesto::forse_dissolvi`] lo stesso campo vale mezza
                // dissolvenza, e non è un'incoerenza da appianare per simmetria:
                // là il segno è puntato a `spinti + meta`, cioè a un istante in
                // cui l'entrante ha già suonato `meta` fotogrammi. Chi
                // uniformasse le due rimetterebbe il difetto che quel commento
                // racconta.
                if self.segno_dissolvenza {
                    // Il numero è quello del segno di metà curva.
                    self.ascolto_corrente = self.ascolto_entrante;
                } else {
                    let ascolto = self.nuovo_ascolto();
                    self.ascolto_corrente = ascolto;
                    self.segni.push_back(Segno {
                        da: self.spinti.saturating_add(rinvio),
                        track_id,
                        durata_ms: preparato.durata_ms,
                        offset_fotogrammi: 0,
                        etichetta: preparato.etichetta,
                        replaygain_db: preparato.livello.guadagno_db,
                        ascolto,
                        formato: Some(preparato.decodificatore.formato_suonato()),
                    });
                    self.mappa_sporca = true;
                }
                if dissolveva {
                    // Se la curva non era finita, il brano che entrava è
                    // rimasto solo a mezza ampiezza: da qui comincia la sua
                    // salita.
                    self.forse_riprendi();
                    // I campioni decodificati per la sovrapposizione e non
                    // ancora mescolati sono il seguito esatto di quel che è
                    // appena uscito: ripartire dal decodificatore li salterebbe,
                    // e il brano attaccherebbe qualche decina di millisecondi
                    // più avanti di dove si era interrotto. Passano per
                    // `blocco` — che il decodificatore ha appena svuotato — così
                    // la salita gliela applica la stessa riga di codice che la
                    // applicherà a tutti i blocchi dopo di loro.
                    self.blocco.clear();
                    self.blocco.append(&mut self.coda_entrante);
                    self.forse_risali();
                    self.resto.extend(self.blocco.iter().copied());
                    self.spinti = self
                        .spinti
                        .saturating_add(fotogrammi(self.blocco.len(), self.formato.canali));
                    // Sono campioni della sovrapposizione, con la correzione
                    // dell'entrante già dentro: si piegano come gli altri.
                    self.segna_alzati();
                }
                self.rg_decodifica = preparato.livello;
                self.durata_corrente_ms = preparato.durata_ms;
                self.etichetta_corrente = preparato.etichetta;
                self.chiudi_sovrapposizione();
                self.corrente = Some(preparato.decodificatore);
                true
            }
            None => {
                self.corrente = None;
                false
            }
        }
    }
}
