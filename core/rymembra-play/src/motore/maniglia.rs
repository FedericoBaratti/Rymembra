//! La maniglia sul motore: [`Motore`] apre il dispositivo, avvia i due fili e
//! manda i comandi. Il `//!` a cui rimandano i commenti è quello di
//! [`crate::motore`].

use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use rymembra_domain::errors::{AppError, ErrorCode};

#[cfg(doc)]
use super::SFUMATURA_DB;
use super::{
    Annuncio, BranoAperto, CAUSA_FILO_CADUTO, Comando, Contatori, Contesto, Evento, Intese,
    Posizione, RifiutoAvvio, Voce, filo_sorvegliato, istantanea, istante_d_avvio,
};
use crate::Condiviso;
use crate::ancora::{Ancora, Mappa};
use crate::equalizzatore::{BANDE, Coefficienti};
use crate::uscita::{FormatoUscita, RISERVA_MS, Uscita};

/// La maniglia sul motore.
///
/// Ogni metodo manda un messaggio e torna subito: nessuno di questi si blocca
/// aspettando il disco o la scheda audio, perché tutti vengono chiamati dal
/// filo che serve i comandi dell'interfaccia.
pub struct Motore {
    comandi: Sender<Comando>,
    condiviso: Arc<Condiviso>,
    posizione: Arc<std::sync::Mutex<Posizione>>,
    /// I segni del filo della decodifica, per tradurre le ancore: vedi
    /// [`Motore::ancora`].
    mappa: Arc<Mutex<Mappa>>,
    formato: FormatoUscita,
    /// Il lettore dello spettro.
    ///
    /// Dietro un lucchetto, e va bene: chi lo prende è il filo che serve i
    /// comandi, non la callback audio. Il lucchetto sta **fuori** dal percorso
    /// in tempo reale, che è l'unico posto in cui non poteva stare.
    ///
    /// In un `Arc` perché chi lo legge possa portarselo via: vedi
    /// [`Motore::presa_spettro`].
    spettro: Arc<std::sync::Mutex<crate::spettro::Spettro>>,
    /// Il nome dell'uscita su cui si è aperto davvero. Vedi
    /// [`Motore::dispositivo`].
    dispositivo: String,
    /// Gli accordi con il filo della decodifica che non passano dalla coda.
    intese: Arc<Intese>,
    /// Tenerla viva tiene aperto il flusso; [`Uscita::sveglia`] lo rimette in
    /// moto quando riposa, prima di ogni comando che ha bisogno della callback.
    uscita: Uscita,
}

/// Il filo «rymembra-eventi»: riceve gli annunci e li passa all'osservatore.
///
/// # Perché un filo in mezzo
///
/// Perché l'osservatore dell'applicazione **lavora**: a ogni cambio di brano
/// prende il lucchetto del lettore, scrive l'ascolto sul database, conserva la
/// coda, compone lo stato per la finestra. Finché lo chiamava il filo della
/// decodifica, tutto quel lavoro stava fra un blocco di campioni e il
/// successivo — con il lucchetto del lettore conteso dalla finestra e quello
/// della libreria conteso da una scansione. L'anello regge qualche decimo di
/// secondo a 192 kHz su otto canali: un database occupato per un secondo, al
/// cambio di brano, era un buco nel suono proprio dove il gapless doveva non
/// farne.
///
/// Adesso il filo della decodifica consegna un messaggio e torna a decodificare.
/// L'ordine resta quello in cui gli eventi sono successi: il canale è uno, e chi
/// lo svuota è uno.
///
/// # Quando finisce
///
/// Quando il contesto — l'unico che tiene l'altro capo — viene lasciato cadere,
/// cioè quando il filo della decodifica esce: il canale si chiude, gli annunci
/// rimasti si consegnano, e il ciclo termina da sé. Quelli di un motore
/// abbandonato arrivano comunque all'osservatore, che li riconosce con
/// [`Annuncio::ha_voce`].
///
/// # Quando l'osservatore cade
///
/// Ogni consegna gira dentro un `catch_unwind`, e il filo va avanti. Prima un
/// panico dentro l'osservatore — che lavora: lucchetti, database, lo stato da
/// comporre — srotolava fuori dal ciclo e il filo finiva. Da fuori non si
/// vedeva niente: il motore suonava, ma i suoi annunci finivano in un canale
/// senza più nessuno in fondo, e con loro l'avanzare della coda. Il brano
/// finiva, il successivo non si preparava più, e la musica si fermava a fine
/// brano senza un errore da nessuna parte.
///
/// Si perde l'annuncio che è caduto, e nessun altro. **Non** si alza `perso`:
/// un panico che si ripete identico sull'annuncio di un certo brano
/// diventerebbe un motore ricostruito ogni due secondi, lo stesso ciclo che
/// `fili_caduti` esiste per rompere sul filo della decodifica. La traccia la
/// scrive il gancio dei panici dell'applicazione, col nome di questo filo,
/// prima di arrivare qui.
///
/// `AssertUnwindSafe` perché l'annuncio caduto non lo guarda più nessuno, e
/// l'osservatore è una `Fn`: quel che tocca di condiviso lo tocca dietro
/// lucchetti che chi sta sopra recupera dal veleno.
pub(super) fn voce_su_filo(osservatore: Box<dyn Fn(Annuncio) + Send>) -> Result<Voce, AppError> {
    let (manda, ricevi) = std::sync::mpsc::channel::<Annuncio>();
    std::thread::Builder::new()
        .name("rymembra-eventi".to_owned())
        .spawn(move || {
            for annuncio in ricevi {
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| osservatore(annuncio)));
            }
        })
        .map_err(|err| {
            AppError::new(ErrorCode::PlaybackEngineUnavailable)
                .with_cause(format!("filo degli eventi: {err}"))
        })?;
    Ok(Box::new(move |annuncio| {
        // Un canale chiuso vuol dire un filo degli eventi che non c'è più: non
        // c'è nessuno a cui dirlo, e il filo della decodifica non deve cadere
        // per questo.
        let _ = manda.send(annuncio);
    }))
}

impl Motore {
    /// Apre il dispositivo e avvia i fili.
    ///
    /// `voluto` è il nome dell'uscita da aprire, `None` per quella predefinita
    /// di sistema. Chiedere un nome che non c'è **non** fallisce: si ripiega
    /// sul predefinito, e [`Motore::dispositivo`] dice dove si è finiti.
    ///
    /// L'osservatore riceve soltanto l'evento: il controllo del motore
    /// abbandonato lo fa questa funzione, subito prima di chiamarlo. Chi deve
    /// farlo sotto un lucchetto suo — perché sotto quel lucchetto il motore si
    /// sostituisce — usa [`Motore::avvia_con_annunci`].
    pub fn avvia(
        osservatore: impl Fn(Evento) + Send + 'static,
        voluto: Option<String>,
    ) -> Result<Self, AppError> {
        Self::avvia_con_annunci(
            move |annuncio: Annuncio| {
                if annuncio.ha_voce() {
                    osservatore(annuncio.evento);
                }
            },
            voluto,
        )
    }

    /// Come [`Motore::avvia`], ma l'osservatore riceve l'[`Annuncio`] intero.
    ///
    /// Gira sul filo «rymembra-eventi», mai su quello della decodifica né sulla
    /// callback audio: può prendere lucchetti, scrivere sul database e aspettare
    /// senza che il suono ne sappia niente.
    ///
    /// # Errori
    ///
    /// `playback.engineUnavailable` se non parte uno dei due fili, e gli errori
    /// di apertura del dispositivo.
    pub fn avvia_con_annunci(
        osservatore: impl Fn(Annuncio) + Send + 'static,
        voluto: Option<String>,
    ) -> Result<Self, AppError> {
        // Il filo degli eventi **prima** del dispositivo: se non parte, non
        // resta aperto un flusso audio che nessuno chiuderà.
        let voce = voce_su_filo(Box::new(osservatore))?;
        // L'orologio del processo prima di aprire l'uscita: la sua prima
        // lettura lo inizializza, e un'inizializzazione può aspettare un altro
        // filo. Fatta qui, la callback — che lo legge a ogni blocco — non può
        // mai essere la prima. Vedi [`crate::tempo`].
        let _ = crate::tempo::base();
        let condiviso = Condiviso::nuovo();

        // L'anello si dimensiona prima di sapere la frequenza vera, quindi si
        // prende il caso peggiore ragionevole: 192 kHz su 8 canali. Sono meno di
        // due megabyte, e sbagliare per eccesso qui costa memoria mentre
        // sbagliare per difetto costa interruzioni del suono.
        let capacita = usize::try_from(
            192_000u64
                .saturating_mul(8)
                .saturating_mul(RISERVA_MS)
                .saturating_div(1000),
        )
        .unwrap_or(320_000);
        let (produttore, consumatore) = rtrb::RingBuffer::<f32>::new(capacita);

        // Il secondo anello, quello delle curve dell'equalizzatore. Trentadue
        // posti: la callback lo svuota a ogni blocco, quindi in pratica non si
        // riempie mai — e se si riempisse, chi spinge se le tiene invece di
        // perderle.
        let (manda_coefficienti, ricevi_coefficienti) = rtrb::RingBuffer::<Coefficienti>::new(32);

        // Il terzo anello: i campioni che escono, per lo spettro. Otto
        // fotogrammi di finestra — a 48 kHz mezzo secondo — perché chi legge lo
        // fa una trentina di volte al secondo e un ritardo dello scheduler non
        // deve buttare via mezza finestra. Quando è pieno si perde: lo spettro
        // è decorazione, e l'unica alternativa sarebbe far aspettare la
        // callback, cioè l'unica cosa che il tempo reale vieta.
        let (manda_spettro, ricevi_spettro) = rtrb::RingBuffer::<f32>::new(32_768);

        let uscita = crate::uscita::apri(
            consumatore,
            ricevi_coefficienti,
            manda_spettro,
            Arc::clone(&condiviso),
            voluto,
        )?;
        let formato = uscita.formato;
        let dispositivo = uscita.dispositivo.clone();
        condiviso
            .canali
            .store(u32::from(formato.canali), Ordering::Relaxed);

        let posizione = Arc::new(std::sync::Mutex::new(Posizione::default()));
        let (manda, ricevi) = std::sync::mpsc::channel();

        let contesto = Contesto::nuovo(
            ricevi,
            produttore,
            manda_coefficienti,
            Arc::clone(&condiviso),
            Arc::clone(&posizione),
            formato,
            voce,
            uscita.campanello(),
        );
        let intese = Arc::clone(&contesto.intese);
        let mappa = Arc::clone(&contesto.mappa);

        std::thread::Builder::new()
            .name("rymembra-decodifica".to_owned())
            .spawn(move || filo_sorvegliato(contesto))
            .map_err(|err| {
                AppError::new(ErrorCode::PlaybackEngineUnavailable)
                    .with_cause(format!("filo della decodifica: {err}"))
            })?;

        Ok(Self {
            comandi: manda,
            condiviso,
            posizione,
            mappa,
            formato,
            spettro: Arc::new(std::sync::Mutex::new(crate::spettro::Spettro::nuovo(
                ricevi_spettro,
                formato.frequenza,
            ))),
            dispositivo,
            intese,
            uscita,
        })
    }

    /// Accende o spegne la presa dello spettro.
    ///
    /// Spenta, la callback non scrive niente: chi non ha la schermata aperta
    /// non paga un `push` per campione. Spegnendola si svuota anche quel che
    /// era rimasto — l'anello **e** la finestra, vedi
    /// [`crate::spettro::Spettro::svuota`] — così riaprendo la schermata le
    /// barre non ripartono da un pezzo di musica di dieci minuti fa.
    pub fn guarda_spettro(&self, acceso: bool) {
        self.condiviso
            .spettro
            .store(acceso, std::sync::atomic::Ordering::Relaxed);
        if !acceso && let Ok(mut lettore) = self.spettro.lock() {
            lettore.svuota();
        }
    }

    /// Quante bande fini calcolare accanto alle dieci d'ottava.
    ///
    /// Vedi [`crate::spettro::RISOLUZIONI`]. Un valore fuori scala si stringe
    /// invece di fallire: chi chiama arriva da una preferenza su disco.
    pub fn spettro_dettaglio(&self, quante: u16) {
        if let Ok(mut lettore) = self.spettro.lock() {
            lettore.dettaglio(quante);
        }
    }

    /// Le bande, dentro una funzione che le legge senza copiarle.
    ///
    /// Una chiusura e non un valore restituito: le bande fini sono fino a
    /// milleventiquattro numeri letti trenta volte al secondo, e restituirle
    /// vorrebbe dire allocare un vettore trenta volte al secondo per buttarlo
    /// via subito dopo averlo serializzato. Chi chiama prende quel che gli
    /// serve — di solito la forma che va sul filo verso la finestra — e il
    /// lucchetto si chiude appena finito.
    ///
    /// Restituisce `None` se il lucchetto è avvelenato — cioè se qualcuno è
    /// caduto tenendolo. Non è più un'ipotesi di scuola: il filo della
    /// decodifica **può** cadere, e cadere è un fatto previsto da quando lo
    /// sorveglia `filo_sorvegliato`. Quel filo questo lucchetto non lo
    /// prende, quindi in pratica il `None` resta il caso che non arriva mai; ma
    /// la ragione per non dichiararlo impossibile con un `unwrap` adesso è un
    /// fatto e non una prudenza.
    pub fn spettro<R>(&self, prendi: impl FnOnce(&crate::spettro::Bande) -> R) -> Option<R> {
        self.presa_spettro().leggi(prendi)
    }

    /// Lo spettro, da leggere **senza** tenere in mano il motore.
    ///
    /// # Perché
    ///
    /// Perché leggere le bande non è una lettura: è svuotare l'anello dei
    /// campioni e farne una trasformata, fino a duemila punti, trenta volte al
    /// secondo. Nell'applicazione il motore sta dietro il lucchetto del lettore,
    /// e con [`Motore::spettro`] quella trasformata si faceva col lucchetto in
    /// mano: la pausa, il salto e gli annunci del motore si mettevano in fila
    /// dietro a lei trenta volte al secondo, per tutto il tempo in cui la
    /// schermata era aperta. La presa si prende sotto il lucchetto — è un `Arc`
    /// da clonare — e si legge fuori.
    ///
    /// Una presa di un motore sostituito legge l'anello di quel motore, che
    /// nessuno riempie più: dà bande vecchie per un giro, finché chi legge non
    /// ne chiede una nuova. Non tiene aperto niente: l'anello dei campioni è
    /// soltanto memoria.
    #[must_use]
    pub fn presa_spettro(&self) -> PresaSpettro {
        PresaSpettro(Arc::clone(&self.spettro))
    }

    /// Il campanello del flusso, da suonare senza tenere in mano il motore.
    ///
    /// Chi lo suona prima di un comando che svuota l'anello — un salto, uno
    /// stop — paga l'eventuale risveglio del dispositivo **fuori** dal proprio
    /// lucchetto; il comando, dopo, lo trova già sveglio. Vedi
    /// [`crate::uscita::Campanello`].
    #[must_use]
    pub fn campanello(&self) -> crate::uscita::Campanello {
        self.uscita.campanello()
    }

    /// Come si è aperto il dispositivo.
    #[must_use]
    pub const fn formato(&self) -> FormatoUscita {
        self.formato
    }

    /// L'uscita aperta era il predefinito di sistema nell'istante
    /// dell'apertura.
    ///
    /// Serve a chi riapre da solo per riconoscere il ripiego che non voleva:
    /// senza una preferenza si apre il predefinito, e se nell'elenco non ce
    /// n'era nessuno marcato — succede, nel mezzo di un cambio di dispositivo —
    /// l'apertura prende il primo che trova, che può essere un'uscita virtuale.
    #[must_use]
    pub const fn era_predefinito(&self) -> bool {
        self.uscita.era_predefinito
    }

    /// Su quale uscita si sta suonando.
    ///
    /// È il nome del dispositivo **davvero** aperto, che può non essere quello
    /// chiesto: una preferenza che punta a una scheda staccata ripiega sul
    /// predefinito. Chi sorveglia i dispositivi confronta questa stringa con
    /// quel che l'utente voleva e con il predefinito di adesso: sono i tre
    /// termini che dicono se vale la pena riaprire.
    #[must_use]
    pub fn dispositivo(&self) -> &str {
        &self.dispositivo
    }

    /// L'uscita aperta è Bluetooth: in pausa la libera dopo un secondo invece
    /// che dopo trenta. Vedi `uscita::RIPOSO_BLUETOOTH`.
    #[must_use]
    pub const fn su_bluetooth(&self) -> bool {
        self.uscita.bluetooth
    }

    /// Comincia a suonare questo brano, adesso, scartando quel che c'era.
    ///
    /// Il brano arriva **già aperto**: il perché sta in [`BranoAperto`].
    pub fn suona(&self, brano: BranoAperto) {
        self.suona_da(brano, 0);
    }

    /// Come [`Motore::suona`], ma dal millisecondo `da_ms`.
    ///
    /// # Perché non `suona` e poi `vai_a`
    ///
    /// Per la stessa ragione di [`Motore::apri_fermo`]: fra i due comandi il
    /// filo può già aver messo nell'anello qualche millisecondo dall'inizio
    /// del brano, e la callback farlo sentire prima del salto. Qui apertura e
    /// salto sono un comando solo, eseguito prima di qualunque decodifica.
    ///
    /// E non `apri_fermo` più `riprendi`, che pure farebbe lo stesso suono: qui
    /// l'avvio è un comando solo, senza passare dalla ripresa. Non per la
    /// sfumatura del timer di spegnimento: un brano avviato — da `suona_da`
    /// come da `apri_fermo` — la annulla in ogni caso, perché un brano scelto
    /// mentre la musica si spegneva è una risposta al timer (vedi
    /// `Contesto::avvia_brano`). E un motore riaperto non ne ha nessuna da
    /// annullare: nasce senza. Chi vuole che la discesa sopravviva a una
    /// riapertura del dispositivo deve rimandare [`Motore::sfuma`], col tempo
    /// che resta, **dopo** questo comando.
    pub fn suona_da(&self, brano: BranoAperto, da_ms: u64) {
        // Il flusso prima di tutto: il comando fa svuotare l'anello, e lo
        // svuotamento è una stretta di mano con la callback che a flusso fermo
        // nessuno stringe. Vedi [`Uscita::sveglia`].
        self.uscita.sveglia();
        // La svolta si conta **prima** di partire: un annuncio che il filo
        // manda da qui al momento in cui la esegue parla del brano di prima.
        // Vedi [`Annuncio::superato`].
        self.intese.chieste.fetch_add(1, Ordering::SeqCst);
        // Si scrive che la pausa non la si vuole più, qui e non all'apertura
        // del file — per la ragione, e per l'ordine, scritti in
        // [`Motore::pausa`]. Ma la pausa **in corso** resta alzata: la abbassa
        // il filo della decodifica dopo aver buttato l'anello, altrimenti la
        // callback suonerebbe quel che ci era rimasto del brano di prima. Vedi
        // `Intese::pausa_voluta`.
        self.intese.pausa_voluta.store(false, Ordering::SeqCst);
        // Un avvio programmato apparteneva al brano di prima.
        self.condiviso.avvio_ns.store(0, Ordering::Release);
        // Quel che il filo sta aspettando dalla rete non serve più: vedi
        // `Condiviso::in_lettura`. Prima del comando, non dopo.
        self.condiviso.interrompi_la_lettura();
        self.manda(Comando::Suona {
            brano: Box::new(brano),
            da_ms,
        });
    }

    /// Apre un brano **fermo**, già al millisecondo `da_ms`: in pausa, con
    /// l'anello pieno dal punto giusto, pronto a ripartire all'istante.
    ///
    /// # Perché non `suona`, `vai_a` e `pausa` in fila
    ///
    /// Perché era così, e aveva due difetti. Il primo è una gara: `suona` scrive
    /// che la pausa non la si vuole più, e una `pausa` arrivata un istante dopo
    /// deve vincere su un comando che è ancora in coda — vince, ma nel
    /// frattempo la callback può aver già fatto sentire qualche millisecondo
    /// dal punto zero. Il secondo è il salto in mezzo: fra i due comandi il filo
    /// riempie l'anello dall'inizio del brano, e il salto lo deve buttare.
    ///
    /// Qui la pausa — voluta e in corso — si scrive **prima** di mandare il
    /// comando, come fa [`Motore::pausa`], e il comando porta con sé il punto
    /// d'arrivo: il filo apre e salta senza decodificare niente in mezzo. Dal
    /// punto zero non esce nessun campione.
    ///
    /// Per ripartire: [`Motore::riprendi`], o [`Motore::riprendi_a`] per un
    /// istante preciso.
    pub fn apri_fermo(&self, brano: BranoAperto, da_ms: u64) {
        // Come `suona`: lo svuotamento è una stretta di mano con la callback,
        // e a flusso fermo nessuno la stringe.
        self.uscita.sveglia();
        self.intese.chieste.fetch_add(1, Ordering::SeqCst);
        self.condiviso.avvio_ns.store(0, Ordering::Release);
        self.intese.pausa(&self.condiviso);
        self.condiviso.interrompi_la_lettura();
        self.manda(Comando::Suona {
            brano: Box::new(brano),
            da_ms,
        });
    }

    /// Tiene pronto il brano dopo, per attaccarlo senza buco.
    ///
    /// È tutto il gapless: quando il corrente finisce, i campioni del prossimo
    /// sono già in coda per entrare nell'anello, e fra i due non c'è nessuna
    /// apertura di file da aspettare.
    pub fn prepara(&self, brano: Option<BranoAperto>) {
        self.manda(Comando::Prepara(brano.map(Box::new)));
    }

    /// Riprende.
    ///
    /// Lascia perdere anche la sfumatura del timer di spegnimento, se ce n'è
    /// una in corso: chi preme play mentre la musica si sta spegnendo la vuole,
    /// e lasciarla scendere fino alla pausa vorrebbe dire ignorarlo. Vedi
    /// [`Motore::sfuma`].
    pub fn riprendi(&self) {
        // A flusso fermo, togliere la pausa non farebbe uscire niente.
        self.uscita.sveglia();
        // Adesso, non a un istante programmato prima.
        self.condiviso.avvio_ns.store(0, Ordering::Release);
        self.intese.riprendi(&self.condiviso);
        self.manda(Comando::AnnullaSfuma);
    }

    /// Riprende in modo che la musica arrivi all'orecchio all'istante
    /// `quando_ns`, sull'orologio di [`crate::tempo`].
    ///
    /// È la ripresa di chi ascolta insieme a un altro computer: l'istante è
    /// quello in cui l'altro sentirà lo stesso fotogramma. La callback tace fino
    /// al primo fotogramma del suo blocco che non suonerebbe **prima**
    /// dell'istante, e da lì apre la porta; se l'istante è già passato quando
    /// la callback ci arriva, parte subito e salta la musica che doveva essere
    /// già uscita.
    ///
    /// # Come si fa il conto
    ///
    /// 1. Si sveglia il flusso, come in [`Motore::riprendi`]: anche in
    ///    sessione riposa dopo una pausa, ed è così che le cuffie Bluetooth
    ///    tornano libere. Il risveglio costa qualche decina di millisecondi —
    ///    sul Bluetooth anche di più — e se l'istante passa nel frattempo la
    ///    callback salta la musica già dovuta: si arriva tardi all'orecchio, ma
    ///    al punto giusto. Chi sceglie l'istante può svegliarlo **prima**, con
    ///    [`Motore::sveglia`], e allora qui è un giro di messaggio.
    /// 2. Si toglie la latenza dichiarata a mano: `quando_ns` è un istante
    ///    dell'orecchio, la callback ragiona sull'istante in cui il fotogramma
    ///    lascia il dispositivo.
    /// 3. Si scrive in `Condiviso::avvio_ns`, e **poi** si toglie la pausa:
    ///    nell'ordine inverso la callback potrebbe ripartire subito.
    ///
    /// La specifica voleva l'istante portato sull'orologio del flusso col
    /// ponte. Resta invece su quello del processo, e il conto torna lo stesso:
    /// il ponte è uno spostamento fisso, e la callback porta già **il suo**
    /// istante di qua — una volta per blocco, in un posto solo (vedi
    /// `uscita::Battito`). Così l'avvio e l'ancora parlano lo stesso orologio,
    /// e le prove lo possono fingere.
    ///
    /// # Errori
    ///
    /// [`RifiutoAvvio::NelPassato`] se l'istante è passato da più di un
    /// secondo: lì il salto da fare non è più un dettaglio dell'avvio, e chi
    /// chiama deve chiederlo per quel che è.
    pub fn riprendi_a(&self, quando_ns: u64) -> Result<(), RifiutoAvvio> {
        // Prima del conto: il controllo «nel passato» deve vedere l'istante
        // di dopo il risveglio, non quello di prima. Per questo l'orologio si
        // legge qui, dopo `sveglia`, e non dentro il conto.
        self.uscita.sveglia();
        let avvio = istante_d_avvio(
            quando_ns,
            self.latenza_della_mappa(),
            crate::tempo::adesso_ns(),
        )?;
        self.condiviso.avvio_ns.store(avvio, Ordering::Release);
        self.intese.riprendi(&self.condiviso);
        self.manda(Comando::AnnullaSfuma);
        Ok(())
    }

    /// Sposta la musica di tanti fotogrammi d'uscita: positivo inserisce
    /// silenzio, negativo salta musica.
    ///
    /// La callback chiude la porta in una dozzina di millisecondi, esegue, e la
    /// riapre allo stesso passo; il conto dei fotogrammi segue esatto, quindi
    /// la posizione, le ancore e la fine della coda restano vere. Un salto più
    /// lungo di quel che l'anello tiene si completa quando il decodificatore
    /// ne porta altro. Una pausa, un salto o un brano nuovo lo fanno cadere.
    ///
    /// Finché non è finito [`Motore::in_correzione`] dice vero: chi corregge
    /// aspetta che torni falso prima di misurare di nuovo.
    pub fn gradino(&self, fotogrammi: i64) {
        if fotogrammi == 0 {
            return;
        }
        // La richiesta **prima** della bandiera: vedi `uscita::regia::libera`, che le
        // legge nell'ordine opposto.
        self.condiviso
            .gradino
            .fetch_add(fotogrammi, Ordering::SeqCst);
        self.condiviso.in_correzione.store(true, Ordering::SeqCst);
    }

    /// Accelera (positivo) o rallenta (negativo) il consumo della musica di
    /// tante parti per milione, fino a ±[`crate::uscita::DERIVA_MASSIMA_PPM`].
    ///
    /// Serve a tenere in pari due schede audio i cui quarzi non contano allo
    /// stesso passo: vedi `uscita::Andamento::accumulo` per come si fa senza
    /// che si senta. Zero la spegne.
    pub fn deriva(&self, ppm: i32) {
        let tetto = crate::uscita::DERIVA_MASSIMA_PPM;
        self.condiviso
            .deriva_ppm
            .store(ppm.clamp(-tetto, tetto), Ordering::Relaxed);
    }

    /// Rimette in moto il flusso se riposava, e torna quando la callback gira.
    ///
    /// È [`crate::uscita::Uscita::sveglia`], per chi deve chiamarla da fuori:
    /// può aspettare fino a mezzo secondo. La chiama chi sta per scegliere
    /// l'istante di un [`Motore::riprendi_a`], perché il risveglio non se lo
    /// mangi.
    pub fn sveglia(&self) {
        self.uscita.sveglia();
    }

    /// Mette in pausa.
    ///
    /// # Perché non passa dalla coda dei comandi
    ///
    /// Ci passava, e il comando non faceva altro che alzare questo bit: la
    /// pausa vera sta nella callback dell'uscita, che legge `in_pausa` a ogni
    /// blocco, sfuma in una dozzina di millisecondi — gli unici fotogrammi che
    /// la pausa consuma — e poi manda silenzio senza toccare l'anello. Il giro per il filo
    /// della decodifica non serviva a mettere in pausa — serviva solo a farsi
    /// aspettare.
    ///
    /// E chi aspettava era la finestra. `pausa()` tornava subito, il filo
    /// alzava il bit qualche millisecondo dopo, e il chiamante che nel
    /// frattempo componeva lo stato da mandare leggeva il valore **di prima**:
    /// il pulsante mostrava il triangolo mentre il brano suonava e le due
    /// stanghette a brano fermo, per sempre, perché dopo una pausa non arriva
    /// nessun evento a correggere il tiro.
    ///
    /// Alzarlo qui rimette anche l'ordine giusto. Pausa, ripresa e avvio
    /// scrivono tutti dal filo di chi preme, quindi valgono nell'ordine in cui
    /// si preme: con l'avvio in coda e la pausa immediata, una pausa premuta
    /// subito dopo un avvio se la sarebbe mangiata l'avvio, arrivando dopo.
    ///
    /// Le caselle sono due, e questa funzione le scrive tutte e due: quella che
    /// dice cosa si vuole e quella che legge la callback. Il perché delle due
    /// sta su `Intese::pausa_voluta`.
    pub fn pausa(&self) {
        // Un avvio programmato non sopravvive a una pausa chiesta dopo.
        self.condiviso.avvio_ns.store(0, Ordering::Release);
        self.intese.pausa(&self.condiviso);
    }

    /// Smette e dimentica tutto.
    pub fn ferma(&self) {
        // Anche fermarsi svuota l'anello: stessa stretta di mano di `suona`.
        self.uscita.sveglia();
        self.intese.chieste.fetch_add(1, Ordering::SeqCst);
        self.condiviso.avvio_ns.store(0, Ordering::Release);
        // Il bit prima del comando: svuotare l'anello è lavoro del filo, dire
        // che siamo fermi no — vedi [`Motore::pausa`].
        self.intese.pausa(&self.condiviso);
        self.condiviso.interrompi_la_lettura();
        self.manda(Comando::Ferma);
    }

    /// Porta il volume a zero in `ms` millisecondi, poi mette in pausa.
    ///
    /// È la fine del timer di spegnimento. Prima era una pausa secca, cioè il
    /// modo peggiore di svegliare chi si stava addormentando: la musica si
    /// fermava a metà di una frase, e il silenzio improvviso fa l'effetto di un
    /// rumore. La discesa è lineare in decibel — vedi [`SFUMATURA_DB`] — e il
    /// volume torna dov'era alla ripresa, risalendo dal fondo: chi riprende il
    /// giorno dopo non ritrova il cursore a zero, e intanto la pausa si è
    /// chiusa sul silenzio e non su un ritorno improvviso del volume.
    ///
    /// La fa il filo della decodifica, a ogni suo giro: quattro millisecondi
    /// fra un passo e l'altro, che la rampa della callback leviga del tutto.
    /// Un brano nuovo, uno stop o una ripresa la annullano.
    pub fn sfuma(&self, ms: u64) {
        self.manda(Comando::Sfuma { ms });
    }

    /// Lascia perdere la sfumatura in corso, se ce n'è una.
    ///
    /// Serve a chi spegne il timer mentre la musica sta già scendendo.
    pub fn annulla_sfumatura(&self) {
        self.manda(Comando::AnnullaSfuma);
    }

    /// Salta a un punto del brano corrente.
    pub fn vai_a(&self, ms: u64) {
        // Un salto svuota l'anello: stessa stretta di mano di `suona`, e un
        // salto in pausa col flusso a riposo è proprio il caso che la perdeva.
        self.uscita.sveglia();
        // Un salto chiesto mentre il precedente aspetta ancora la rete: quello
        // smette di aspettare, e di più salti in fila il filo fa solo l'ultimo
        // (vedi `compatta`).
        self.condiviso.interrompi_la_lettura();
        self.manda(Comando::VaiA(ms));
    }

    /// Cambia volume e silenziamento.
    pub fn volume(&self, volume: f32, muto: bool) {
        self.manda(Comando::Volume { volume, muto });
    }

    /// Accende o spegne la correzione ReplayGain.
    pub fn replaygain(&self, attivo: bool, bersaglio_db: f32) {
        self.manda(Comando::ReplayGain {
            attivo,
            bersaglio_db,
        });
    }

    /// Quanto si sovrappongono due brani, in millisecondi. Zero: per niente.
    ///
    /// A zero il passaggio resta il gapless di sempre — i campioni del brano
    /// dopo cominciano dove finiscono quelli di prima, senza un istante di
    /// silenzio e senza un istante di sovrapposizione.
    pub fn dissolvenza(&self, ms: u64) {
        self.manda(Comando::Dissolvenza { ms });
    }

    /// Di quanti millisecondi la catena d'uscita ritarda il suono, dichiarati.
    ///
    /// È il terzo dei tre ritardi elencati nel `//!` di [`crate::motore`]: quello che
    /// nessuno misura — mixer di sistema, driver, DAC, e su un'uscita senza fili
    /// la radio. Si somma alla latenza che `cpal` riporta e il totale si **toglie**
    /// dalla posizione raccontata, così quel che il cursore e i testi dicono è
    /// quel che l'orecchio sta ricevendo adesso.
    ///
    /// Positivo ritarda la posizione riportata, negativo la anticipa: il secondo
    /// verso serve a chi trova che i testi arrivino **tardi** anche con la misura
    /// in mano, cioè quando l'anticipo dei testi è già stato tarato per un'altra
    /// uscita.
    ///
    /// Non tocca il suono, non tocca il gapless e non sposta la fine del brano:
    /// vedi [`Contesto::forse_fine`]. Chi chiama ritaglia il valore — qui entra
    /// quel che arriva da una preferenza, e un numero assurdo sposterebbe il
    /// cursore e basta.
    pub fn latenza(&self, ms: i64) {
        self.manda(Comando::Latenza { ms });
    }

    /// Cambia la curva dell'equalizzatore.
    ///
    /// I guadagni sono in decibel, uno per banda, nell'ordine di
    /// [`crate::equalizzatore::CENTRI_HZ`]. Una curva più corta si completa con
    /// degli zeri e una più lunga si tronca: chi chiama può arrivare da un
    /// valore conservato su disco, e adattarsi qui è meglio che pretendere che
    /// tutti sappiano già quante bande ci sono.
    ///
    /// Il taglio a ±[`crate::equalizzatore::LIMITE_DB`] e lo scarto dei valori
    /// non finiti li fa [`Coefficienti::calcola`], che è l'unico posto in cui
    /// quella regola deve stare.
    pub fn equalizzatore(&self, guadagni: &[f32], attivo: bool) {
        let mut curva = [0.0f32; BANDE];
        for (posto, valore) in curva.iter_mut().zip(guadagni) {
            *posto = *valore;
        }
        self.manda(Comando::Equalizzatore {
            guadagni: curva,
            attivo,
        });
    }

    /// Dove siamo.
    ///
    /// Il tempo viene dall'istantanea che scrive il filo della decodifica; la
    /// pausa no, viene dal bit condiviso letto adesso. Chi ha appena premuto
    /// pausa non deve aspettare il prossimo giro di quel filo per sentirselo
    /// dire — vedi [`Motore::pausa`].
    ///
    /// Il bit è quello della pausa **voluta**, non quella in corso: subito dopo
    /// [`Motore::suona`] le due differiscono per qualche millisecondo, e chi
    /// chiede deve sentirsi dire quel che ha appena chiesto. Vedi
    /// `Intese::pausa_voluta`.
    #[must_use]
    pub fn posizione(&self) -> Posizione {
        istantanea(&self.posizione, &self.intese)
    }

    /// Il motore non suona più, e non ricomincerà da solo.
    ///
    /// # Perché il nome parla del dispositivo e la bandiera no
    ///
    /// Perché le cause sono diventate tre e la conseguenza è rimasta una. Due
    /// vengono dal dispositivo — sparito, o guasto nel backend — e la terza dal
    /// filo della decodifica che è caduto: là il dispositivo c'è ancora e
    /// funziona, ma nessuno riempie più l'anello e dalle casse esce silenzio.
    /// Per chi legge questa risposta la differenza non conta: in tutti e tre i
    /// casi la cura è un motore nuovo, e la strada che lo costruisce è quella
    /// del sorvegliante delle uscite. A distinguere serve
    /// [`Motore::causa_perdita`], o [`Motore::filo_caduto`] per la sola domanda
    /// che cambia una decisione.
    ///
    /// Il nome è rimasto quello per non riscrivere i venti punti che lo
    /// chiamano; la bandiera sotto è documentata in `Condiviso::perso`.
    #[must_use]
    pub fn dispositivo_perso(&self) -> bool {
        self.condiviso.perso.load(Ordering::Acquire)
    }

    /// La perdita viene dal filo della decodifica caduto, non dal dispositivo.
    ///
    /// L'unica domanda che cambia una decisione invece di una frase: chi
    /// compone il banner sceglie fra `playback.deviceLost` — «l'audio se n'è
    /// andato» — e `playback.stalled`, che dice la verità di questo caso, cioè
    /// che il dispositivo c'è e la musica no.
    #[must_use]
    pub fn filo_caduto(&self) -> bool {
        self.dispositivo_perso()
            && self.condiviso.causa_perdita.load(Ordering::Acquire) == CAUSA_FILO_CADUTO
    }

    /// La causa della perdita nelle due forme in cui serve: la frase italiana
    /// per il diario, la chiave da tradurre per la finestra.
    ///
    /// Una tabella sola, perché le due forme sono la stessa informazione detta
    /// a due destinatari diversi. Il giorno in cui `cpal` aggiunge un terzo
    /// tipo di `StreamError` c'è un `match` solo da toccare — prima erano due,
    /// affiancati da un commento che chiedeva di ricordarsene, ed è
    /// precisamente il genere di richiesta che prima o poi qualcuno non
    /// esaudisce.
    ///
    /// I primi due codici li scrive la callback dell'uscita, il terzo il filo
    /// della decodifica quando cade: è l'unica riga della tabella che non parli
    /// del dispositivo, e la sua frase lo dice invece di nasconderlo dietro un
    /// «audio non disponibile» che manderebbe l'utente a controllare i cavi.
    const fn perdita(codice: u32) -> (&'static str, &'static str) {
        match codice {
            1 => ("dispositivo non più disponibile", "deviceNotAvailable"),
            2 => ("guasto del sistema audio", "systemFailure"),
            CAUSA_FILO_CADUTO => ("il decodificatore si è interrotto", "decoderCrashed"),
            _ => ("causa sconosciuta", "unknown"),
        }
    }

    /// Perché è sparito, in una parola che si può mettere in un registro.
    ///
    /// `None` finché il motore suona. Le tre cause non sono la stessa cosa per
    /// chi legge: «non c'è più» è un cavo staccato o un dispositivo predefinito
    /// cambiato — riaprire funziona quasi sempre — un guasto del backend è
    /// tutto il resto del dispositivo, e riaprire è un tentativo e non una
    /// cura, mentre «il decodificatore si è interrotto» non parla del
    /// dispositivo affatto: quello è al suo posto, ed è il filo che leggeva i
    /// byte a essere caduto su un file. Riaprire rimette in piedi il motore in
    /// tutti e tre i casi; solo nel terzo rimettere la puntina **sullo stesso
    /// brano** rifà cadere tutto, ed è per questo che la causa va detta e non
    /// solo contata.
    ///
    /// La frase è **italiana** e resta tale: il diario è italiano tutto, e una
    /// riga tradotta in mezzo alle altre sarebbe l'unica fuori posto. Chi deve
    /// invece mostrare la causa in finestra prende [`Motore::codice_perdita`].
    #[must_use]
    pub fn causa_perdita(&self) -> Option<&'static str> {
        if !self.dispositivo_perso() {
            return None;
        }
        Some(Self::perdita(self.condiviso.causa_perdita.load(Ordering::Acquire)).0)
    }

    /// La stessa causa, ma come chiave da tradurre.
    ///
    /// # Perché non basta [`Motore::causa_perdita`]
    ///
    /// Perché quella restituisce una frase italiana, e quella frase attraversa
    /// l'IPC e finisce stampata cruda nella finestra **accanto** a una stringa
    /// tradotta: chi ha l'interfaccia in inglese legge «There is no audio.
    /// dispositivo non più disponibile.» Mezza riga in una lingua e mezza
    /// nell'altra, con la minuscola in mezzo.
    ///
    /// Il diario continua a usare la frase, perché il diario è italiano tutto;
    /// alla finestra serve il codice, che si traduce. È la stessa regola che
    /// `riproduzione/mod.rs` scrive per `motivo_prossimo`: il codice e non la
    /// frase.
    #[must_use]
    pub fn codice_perdita(&self) -> Option<&'static str> {
        if !self.dispositivo_perso() {
            return None;
        }
        Some(Self::perdita(self.condiviso.causa_perdita.load(Ordering::Acquire)).1)
    }

    /// Quanti campioni sono stati serviti a vuoto: se cresce, il disco non sta
    /// dietro.
    #[must_use]
    pub fn vuoti(&self) -> u64 {
        self.condiviso.vuoti.load(Ordering::Relaxed)
    }

    /// I contatori di quel che è andato storto o è stato corretto.
    #[must_use]
    pub fn contatori(&self) -> Contatori {
        Contatori {
            vuoti: self.condiviso.vuoti.load(Ordering::Relaxed),
            xrun: self.condiviso.xrun.load(Ordering::Relaxed),
            gradini_fatti: self.condiviso.gradini_fatti.load(Ordering::Relaxed),
        }
    }

    /// Quale fotogramma di quale brano arriva all'orecchio, e quando.
    ///
    /// L'ultima ancora pubblicata dalla callback, tradotta con i segni del filo
    /// della decodifica: vedi [`crate::ancora`] e [`Ancora`].
    ///
    /// `None` finché non c'è musica, quando l'ancora e i segni sono di due
    /// epoche diverse — un salto o un brano nuovo appena chiesti — e quando la
    /// callback era a metà di una scrittura per otto letture di fila. In tutti
    /// e tre i casi la risposta giusta arriva al blocco dopo.
    #[must_use]
    pub fn ancora(&self) -> Option<Ancora> {
        let grezza = self.condiviso.ancora.leggi()?;
        let mappa = self.mappa.lock().unwrap_or_else(PoisonError::into_inner);
        mappa.traduci(&grezza)
    }

    /// In pausa: l'etichetta del brano e la posizione, in microsecondi, del
    /// fotogramma da cui la musica ripartirà.
    ///
    /// È la domanda di chi deve far ripartire questo motore insieme a un altro:
    /// da dove riparto, per sapere quanto saltare. `None` se non si è in pausa,
    /// se uno svuotamento è in corso — il punto sta cambiando — o se la mappa
    /// non è ancora quella dell'epoca in cui si è.
    #[must_use]
    pub fn in_attesa(&self) -> Option<(Option<u64>, u64)> {
        if !self.condiviso.in_pausa.load(Ordering::Acquire)
            || self.condiviso.svuota.load(Ordering::Acquire)
        {
            return None;
        }
        // L'epoca prima e dopo il contatore: se è cambiata in mezzo, il
        // contatore può essere già quello dell'epoca nuova.
        let epoca = self.condiviso.epoca.load(Ordering::Acquire);
        let prossimo = self.condiviso.fotogrammi.load(Ordering::Relaxed);
        if self.condiviso.epoca.load(Ordering::Acquire) != epoca {
            return None;
        }
        let mappa = self.mappa.lock().unwrap_or_else(PoisonError::into_inner);
        mappa.attesa(epoca, prossimo)
    }

    /// Quanta musica la callback ha già davanti, in microsecondi: quanto si può
    /// saltare in avanti con [`Motore::gradino`] senza aspettare il
    /// decodificatore.
    #[must_use]
    pub fn riserva_us(&self) -> u64 {
        let entrati = self.condiviso.spinti.load(Ordering::Relaxed);
        let usciti = self.condiviso.fotogrammi.load(Ordering::Relaxed);
        crate::ancora::us_da_fotogrammi(entrati.saturating_sub(usciti), self.formato.frequenza)
    }

    /// C'è un gradino chiesto o in corso: vedi [`Motore::gradino`].
    #[must_use]
    pub fn in_correzione(&self) -> bool {
        self.condiviso.in_correzione.load(Ordering::Acquire)
    }

    /// Il ponte fra l'orologio del flusso e quello del processo, misurato
    /// all'apertura: diagnostica. Vedi [`crate::uscita::Ponte`].
    #[must_use]
    pub const fn ponte(&self) -> crate::uscita::Ponte {
        self.uscita.ponte
    }

    /// La latenza dichiarata a mano, come la sa la mappa.
    ///
    /// Dalla mappa e non da una copia qui: è lo stesso numero con cui
    /// [`Motore::ancora`] sposta gli istanti, e un avvio e un'ancora che lo
    /// leggessero da due posti diversi potrebbero non andare d'accordo.
    fn latenza_della_mappa(&self) -> i64 {
        self.mappa
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latenza_manuale_ms
    }

    fn manda(&self, comando: Comando) {
        // Il canale è chiuso quando il filo della decodifica non c'è più, e da
        // qui non c'è niente di utile da farne: chi doveva accorgersene lo ha
        // già fatto dall'altra parte, dove il filo cadendo alza `perso` e
        // annuncia `playback.stalled`. Propagare un errore da ogni pulsante
        // riempirebbe la finestra di avvisi identici per un guasto già
        // raccontato una volta, e il pulsante che non si può premere sarebbe
        // l'ultimo posto in cui raccontarlo.
        let _ = self.comandi.send(comando);
    }
}

/// Una presa sullo spettro di un motore: vedi [`Motore::presa_spettro`].
#[derive(Clone)]
pub struct PresaSpettro(Arc<std::sync::Mutex<crate::spettro::Spettro>>);

impl PresaSpettro {
    /// Le bande, dentro una funzione che le legge senza copiarle: la stessa
    /// forma, e lo stesso `None` per un lucchetto avvelenato, di
    /// [`Motore::spettro`].
    ///
    /// L'istante della lettura lo prende qui: è quel che permette al lettore di
    /// riconoscere un flusso fermo e chiamarlo silenzio (vedi la prosa in testa
    /// a [`crate::spettro`]).
    pub fn leggi<R>(&self, prendi: impl FnOnce(&crate::spettro::Bande) -> R) -> Option<R> {
        self.0
            .lock()
            .ok()
            .map(|mut lettore| prendi(lettore.leggi(std::time::Instant::now())))
    }
}

impl Drop for Motore {
    /// # Perché la bandiera prima del comando
    ///
    /// Perché fra il `Chiudi` e la morte del filo passa un giro di ciclo, e in
    /// quel giro il filo può ancora annunciare qualcosa: un `Fermato` perché
    /// l'ultimo brano è finito proprio adesso, un `Iniziato` perché un segno è
    /// stato raggiunto. L'osservatore, però, non è morto con il motore — parla
    /// allo stato dell'applicazione, che nel frattempo ha già in mano il motore
    /// **nuovo**. Quell'annuncio in ritardo verrebbe letto come un fatto del
    /// motore nuovo: un `Fermato` che chiude l'ascolto in corso e fa avanzare
    /// una coda che sta suonando altrove.
    ///
    /// Alzando `abbandonato` per primo, ogni annuncio di questo motore da qui
    /// in poi cade nel vuoto. Vedi `Condiviso::abbandonato`.
    ///
    /// Quelli già partiti, invece, sono in viaggio sul filo degli eventi, e la
    /// bandiera li raggiunge dall'altra parte: [`Annuncio::ha_voce`] la rilegge
    /// sotto il lucchetto con cui chi sta sopra sostituisce il motore, che è
    /// l'unico posto in cui la risposta non può più cambiare.
    fn drop(&mut self) {
        self.condiviso.abbandonato.store(true, Ordering::Release);
        self.manda(Comando::Chiudi);
    }
}
