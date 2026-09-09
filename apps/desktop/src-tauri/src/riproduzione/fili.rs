//! I fili che girano per conto loro.
//!
//! Tre, e nessuno di loro risponde a un `invoke`: il preparatore che apre in
//! anticipo il brano successivo, l'orologio che manda la posizione, e quello
//! che manda le bande dello spettro.
//!
//! Con loro sta il timer di spegnimento, che un filo suo non ce l'ha — a farlo
//! scadere è un giro d'orologio, in `scade_il_timer`, ed è per questo che vive
//! qui e non fra gli altri comandi. Un comando però ce l'ha, ed è l'unico di
//! questo file: [`spegnimento`] lo accende, lo cambia e lo spegne.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui. È anzi la ragione
//! per cui questi fili esistono: aprire un file o una connessione è un'attesa,
//! e un'attesa non si fa né sul filo che disegna la finestra né su quello che
//! riempie l'anello dei campioni.

use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::playback;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria, con_libreria_entro};

use super::coda::scegli_da_solo;
use super::{
    BATTITI_PER_SEGNO, BandeIpc, FINE_DEL_BRANO, Lettore, PASSO_SPETTRO, PASSO_SPETTRO_FERMO,
    PASSO_TEMPO, StatoLettore, Tempo, annota_ripresa, brano_di, con_lettore, guasto_di,
    manda_stato,
};

/// Dice al motore quale sarà il brano dopo.
///
/// È tutto il gapless: il file successivo viene aperto **mentre** il corrente
/// suona ancora, così quando tocca a lui i suoi campioni sono già pronti. Un
/// brano che non si apre non è un guasto da mostrare adesso: lo si scoprirà
/// quando toccherà a lui, e nel frattempo quello che suona non va interrotto.
///
/// # Perché questa funzione non apre più niente
///
/// Perché viene chiamata anche **dall'osservatore**, cioè dal filo della
/// decodifica: il ramo `Evento::Iniziato` di [`su_evento`](super::su_evento) la
/// invoca a ogni cambio di traccia. Aprire là dentro voleva dire fermare il
/// filo che riempie l'anello per tutta la durata dell'apertura — fino ai cinque
/// secondi della scadenza, su una share che risponde male — contro una riserva
/// che a 48 kHz stereo vale poco più di tre secondi. Cioè un buco udibile, a
/// ogni cambio di traccia, proprio quando la rete è già in difficoltà.
///
/// Adesso è una **spinta**: si manda un colpetto sul canale e si torna subito.
/// Il lavoro vero lo fa il filo di [`avvia_preparatore`].
pub(super) fn prepara_prossimo(app: &tauri::AppHandle, _lettore: &mut Lettore) {
    // `_lettore` resta nella firma apposta: dice che chi chiama ha il lucchetto
    // in mano, ed è mentre ce l'ha che il colpetto va mandato — così il filo che
    // si sveglia trova la coda già nello stato nuovo e non in quello di un
    // istante prima.
    sveglia_preparatore(&app.state::<StatoLettore>());
}

/// Il colpetto sul canale del preparatore.
///
/// Non blocca, e l'unico errore possibile non è interessante: il canale è senza
/// limite, quindi una `send` fallisce solo se il filo è morto — cioè se
/// l'applicazione sta uscendo, quando non c'è più nessun brano dopo da
/// preparare.
fn sveglia_preparatore(stato: &StatoLettore) {
    let _ = stato.prepara.send(());
}

/// Quanto si aspetta che una raffica di spinte si calmi.
///
/// Riordinare una playlist a trascinamenti manda una spinta per movimento, e
/// preparare il successivo dieci volte di fila vuol dire aprire dieci file per
/// buttarne nove. Quindici millisecondi non si sentono — il gapless comincia a
/// contare secondi prima della fine del brano — e tolgono di mezzo la raffica.
const RAFFICA_PREPARA: Duration = Duration::from_millis(15);

/// Il filo che apre il brano successivo.
///
/// Esiste per una ragione sola: **togliere l'apertura di un file dal filo della
/// decodifica**, dove stava perché l'osservatore ci gira sopra. Il perché per
/// esteso è in [`prepara_prossimo`].
///
/// Modellato sugli altri fili di questo albero — `avvia_orologio`,
/// `nuvola::avvia_filo`: un thread nominato, un canale, nessun runtime
/// asincrono.
pub fn avvia_preparatore(app: tauri::AppHandle, orecchio: std::sync::mpsc::Receiver<()>) {
    let avviato = std::thread::Builder::new()
        .name("aether-preparatore".to_owned())
        .spawn(move || {
            loop {
                // Canale chiuso: l'applicazione sta uscendo.
                if orecchio.recv().is_err() {
                    return;
                }
                // Si lascia finire la raffica: `recv_timeout` fa da
                // antirimbalzo, e si esce quando per quindici millisecondi non
                // arriva più niente — o subito, se il canale si è chiuso.
                while orecchio.recv_timeout(RAFFICA_PREPARA).is_ok() {}
                prepara_prossimo_adesso(&app);
            }
        });
    if avviato.is_err() {
        // Senza il filo si perde il gapless, non la riproduzione: ogni brano
        // verrà aperto quando tocca a lui. Vale una riga nel diario, non un
        // avvio fallito.
        nota!("[riproduzione] il filo del preparatore non è partito: niente gapless");
    }
}

/// Sceglie, apre e consegna il brano successivo. Gira sul filo del preparatore.
///
/// Tre tempi, e la divisione è tutto il punto: **decidere** vuole il lucchetto e
/// dura microsecondi, **aprire** non lo vuole e può durare secondi, **consegnare**
/// lo rivuole e dura di nuovo microsecondi. Tenerli insieme vorrebbe dire il
/// lucchetto del lettore in mano per tutta l'apertura — e allora tanto varrebbe
/// essere rimasti sul filo della decodifica.
///
/// # Quando il brano dopo è un flusso
///
/// Il secondo tempo diventa una **connessione aperta in anticipo**: 256 kilobyte
/// chiesti a un catalogo mentre il brano corrente suona ancora. La divisione in
/// tre tempi, che era nata per le share lente, è quel che rende la cosa
/// sostenibile — l'attesa avviene qui, su questo filo, con le mani vuote.
///
/// L'antirimbalzo di [`RAFFICA_PREPARA`] conta il doppio in questo caso: dieci
/// file aperti per buttarne nove erano dieci `open` sul disco di chi ascolta,
/// dieci flussi aperti per buttarne nove sono dieci richieste a un archivio
/// pubblico che nessuno paga. Il terzo tempo lascia comunque cadere quel che non
/// serve più — un successivo sbagliato attaccato in gapless si sente — ma la
/// richiesta, a quel punto, è già partita: è la raffica a doverla evitare, non
/// il controllo finale.
fn prepara_prossimo_adesso(app: &tauri::AppHandle) {
    let stato = app.state::<StatoLettore>();

    // ── 1. la decisione, sotto lucchetto ──
    let scelta = con_lettore(&stato, |lettore| {
        // Il timer «fine del brano» si fa qui, e non con una sveglia: non è un
        // istante da aspettare ma un successivo che non deve esserci. Detto
        // così, la musica finisce dove sarebbe finita comunque — senza
        // dissolvenze, senza tagli, senza un secondo di silenzio prima del
        // previsto.
        let fine = stato.spegnimento.load(std::sync::atomic::Ordering::Relaxed) == FINE_DEL_BRANO;
        if fine {
            lettore.motore.prepara(None);
            return Ok(None);
        }

        // La coda non ha un dopo: è qui che l'autoplay entra, e **qui** e non
        // sull'evento `Fermato`. Accodando adesso — mentre il brano corrente
        // suona ancora — quel che si sceglie passa dalla stessa strada di tutti
        // gli altri: viene aperto in anticipo, attacca senza stacco, e non c'è
        // nessun istante in cui l'applicazione si sia fermata. Reagire a
        // `Fermato` vorrebbe dire ripartire *dopo* il silenzio.
        if lettore.coda.peek_next().is_none() && lettore.autoplay {
            if let Some(scelta) = scegli_da_solo(app, lettore) {
                lettore.coda.enqueue(&[scelta.id]);
                lettore.motivo_prossimo = Some((scelta.id, scelta.motivo.codice()));
            }
        }

        let formato = lettore.motore.formato();
        match lettore.coda.peek_next() {
            // Nessun successivo: azzerare è l'unica cosa da fare, dura quanto
            // una scrittura su un canale, e si fa subito qui.
            None => {
                lettore.motore.prepara(None);
                Ok(None)
            }
            Some(id) => Ok(Some((id, formato))),
        }
    });
    let Ok(Some((id, formato))) = scelta else {
        return;
    };

    // ── 2. l'apertura, senza lucchetti in mano ──
    let brano = match brano_di(app, id, formato) {
        Ok(brano) => brano,
        Err(err) => {
            // Si dice adesso, mentre il corrente suona ancora, invece di
            // scoprirlo nel silenzio fra i due.
            //
            // **Senza** annotare una ripresa, al contrario di quel che si fa
            // quando la rete cade a brano avviato: qui non si è interrotto
            // niente — quel che si sente continua — e segnarsi un punto a cui
            // tornare vorrebbe dire offrire un «Riprova» che riavvolge una
            // canzone che sta suonando bene.
            nota!(
                "[riproduzione] il brano dopo non si apre: {} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            app.emetti("riproduzione:errore", crate::errore::errore(err));
            return;
        }
    };

    // ── 3. la consegna, di nuovo sotto lucchetto ──
    let _ = con_lettore(&stato, |lettore| {
        // Nel frattempo la coda può essere cambiata, e chi l'ha cambiata ha
        // mandato la sua spinta: questo brano non serve più a nessuno. Si lascia
        // cadere invece di metterlo in canna — un successivo sbagliato attaccato
        // in gapless si sente, ed è peggio di un gapless mancato.
        if lettore.coda.peek_next() != Some(id) {
            return Ok(());
        }
        lettore.motore.prepara(Some(brano));
        Ok(())
    });
}

/// Quanto manca allo spegnimento, come lo legge la finestra.
///
/// `None` se non c'è nessun timer, `Some(0)` per «alla fine di questo brano»,
/// che non ha una durata da mostrare. Il conto non scende mai sotto uno: un
/// timer scaduto ma non ancora raccolto dall'orologio — c'è un quarto di
/// secondo in cui può succedere — mostrerebbe altrimenti un numero negativo, e
/// azzerarlo e basta lo farebbe passare per l'altro `Some(0)`, cioè per «alla
/// fine di questo brano»: la finestra scriverebbe che il timer aspetta la fine
/// del brano proprio nell'istante in cui invece sta per spegnere tutto.
pub(super) fn quanto_manca(stato_lettore: &StatoLettore) -> Option<i64> {
    let quando = stato_lettore
        .spegnimento
        .load(std::sync::atomic::Ordering::Relaxed);
    match quando {
        0 => None,
        FINE_DEL_BRANO => Some(0),
        scadenza => Some((scadenza - adesso_ms()).max(1)),
    }
}

/// Accende, cambia o spegne il timer di spegnimento.
///
/// `minuti` a zero spegne il timer; [`FINE_DEL_BRANO`] chiede di fermarsi dove
/// finisce quel che sta suonando; qualunque altro numero positivo sono i
/// minuti da adesso.
///
/// # Perché i minuti e non un istante
///
/// Perché «fra mezz'ora» è quel che si intende, e un istante calcolato dalla
/// finestra sarebbe calcolato con l'orologio della finestra. Sono lo stesso
/// orologio finché nessuno cambia fuso, e «finché nessuno» non è una
/// garanzia.
#[tauri::command(async)]
pub fn spegnimento(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    minuti: i64,
) -> Esito<()> {
    use std::sync::atomic::Ordering;

    let quando = match minuti {
        0 => 0,
        FINE_DEL_BRANO => FINE_DEL_BRANO,
        // Un tetto a ventiquattro ore: oltre non è più un timer per
        // addormentarsi, e `adesso_ms` più un numero arbitrario è il modo di
        // farlo traboccare.
        minuti => adesso_ms().saturating_add(minuti.clamp(1, 24 * 60).saturating_mul(60_000)),
    };
    stato.spegnimento.store(quando, Ordering::Relaxed);

    // «Fine del brano» cambia quel che il motore ha già in canna: il brano
    // successivo era stato preparato quando questo è cominciato, e senza
    // questa riga suonerebbe lo stesso.
    con_lettore(&stato, |lettore| {
        prepara_prossimo(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Mette in pausa se il timer di spegnimento è scaduto.
///
/// Pausa e non `ferma`: chi si addormenta con la musica accesa, al risveglio,
/// vuole ritrovare il segno dov'era. `ferma` butterebbe la posizione, e il
/// mattino dopo il brano ripartirebbe da capo senza che nessuno capisca
/// perché.
///
/// Il modo «fine del brano» non passa di qui: quello non è una scadenza ma un
/// brano successivo che non viene preparato, e lo decide
/// [`prepara_prossimo`].
fn scade_il_timer(app: &tauri::AppHandle, stato_lettore: &StatoLettore) {
    use std::sync::atomic::Ordering;

    let quando = stato_lettore.spegnimento.load(Ordering::Relaxed);
    if quando <= 0 || adesso_ms() < quando {
        return;
    }
    // Si azzera **prima** di agire, e solo se nel frattempo nessuno l'ha
    // cambiato: uno scambio secco metterebbe a zero anche una scadenza nuova —
    // o un «fine del brano» — arrivata fra la lettura qui sopra e questa riga,
    // spegnendo un timer che qualcuno aveva appena acceso.
    if stato_lettore
        .spegnimento
        .compare_exchange(quando, 0, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let _ = con_lettore(stato_lettore, |lettore| {
        lettore.motore.pausa();
        if let Some(ascolto) = lettore.ascolto.as_mut() {
            ascolto.pause(adesso_ms());
        }
        manda_stato(app, lettore);
        Ok(())
    });
}

/// Scrive sul disco volume e curva, se sono cambiati da quando li si è scritti.
///
/// # Chi la chiama, e perché non il comando
///
/// La chiama il filo dell'orologio a ogni giro, e il `RunEvent` d'uscita in
/// `main` un'ultima volta. I comandi `volume` ed `equalizzatore` alzano soltanto
/// una bandiera: la ragione per esteso sta su
/// [`StatoLettore::volume_da_salvare`], e in breve è che quei due comandi
/// arrivano una dozzina di volte al secondo da un cursore sotto un dito, e che
/// aprire una transazione su SQLite dodici volte al secondo dal filo che disegna
/// la finestra è il modo di far sembrare morta la finestra.
///
/// # Perché prende le bandiere prima del lucchetto
///
/// Perché `swap` le abbassa e dice com'erano in un colpo solo: se un comando
/// arriva mentre questa scrive, rialza la sua e il giro dopo la riscrive. Il
/// contrario — leggere, scrivere, poi abbassare — perderebbe quel cambio.
///
/// Quattro volte al secondo si legge una coppia di atomici rilassati e quasi
/// sempre si scopre che non c'è niente da fare, che è il costo che questo
/// modulo paga già per `spettro` e per `spegnimento`.
///
/// # Perché il lucchetto ha una scadenza
///
/// Perché questa funzione la chiamano due fili che non possono aspettare, e per
/// due ragioni diverse.
///
/// L'orologio la chiama quattro volte al secondo: se restasse fermo sul
/// lucchetto della libreria mentre una scansione legge una condivisione di rete,
/// smetterebbe di mandare la posizione, di far scadere il timer di spegnimento e
/// di accorgersi che il dispositivo audio è sparito — tre cose che non c'entrano
/// niente col volume e che si fermerebbero insieme a lui. [`ATTESA_ORDINARIA`]
/// gli dà un margine largo, perché il caso normale è che il lucchetto sia libero.
///
/// L'uscita la chiama una volta sola, e lì aspettare è peggio che non fare: un
/// processo che non si chiude è il guasto che chi guarda risolve terminandolo,
/// cioè lasciando il WAL sporco e la libreria da riparare al prossimo avvio.
/// [`ATTESA_USCENDO`] è corta apposta.
///
/// Quando il lucchetto non arriva le bandiere si **rialzano**. Sono state
/// abbassate da uno `swap` prima di sapere se si sarebbe scritto, e lasciarle
/// giù vorrebbe dire un volume cambiato che nessuno riprova più a salvare: al
/// giro d'orologio dopo la funzione uscirebbe subito dicendo che non c'è niente
/// da fare. `fetch_or` e non `store` perché nel frattempo un comando può averle
/// rialzate lui, e uno `store` non le abbasserebbe ma nemmeno racconterebbe la
/// stessa cosa.
fn salva_quel_che_manca(app: &tauri::AppHandle, stato: &StatoLettore, entro: Duration) {
    use std::sync::atomic::Ordering::Relaxed;
    let volume = stato.volume_da_salvare.swap(false, Relaxed);
    let eq = stato.eq_da_salvare.swap(false, Relaxed);
    if !volume && !eq {
        return;
    }
    // Una lettura sola sotto il lucchetto del lettore, e poi lo si molla: il
    // database si tocca **fuori**, che è la regola di tutto questo modulo.
    let valori = con_lettore(stato, |lettore| Ok((lettore.volume, lettore.eq.clone())));
    let Ok((salva_volume, salva_eq)) = valori else {
        return;
    };
    let Some(stato_app) = app.try_state::<Stato>() else {
        return;
    };
    let esito = con_libreria_entro(&stato_app, entro, |libreria| {
        if volume {
            playback::save_volume(&libreria.connection, salva_volume)?;
        }
        if eq {
            playback::save_eq(&libreria.connection, &salva_eq)?;
        }
        Ok(())
    });
    let Some(esito) = esito else {
        stato.volume_da_salvare.fetch_or(volume, Relaxed);
        stato.eq_da_salvare.fetch_or(eq, Relaxed);
        nota!(
            "[riproduzione] preferenze audio rinviate: la libreria era occupata da più di {} ms",
            entro.as_millis()
        );
        return;
    };
    if let Err(err) = esito {
        nota!(
            "[riproduzione] preferenze audio non salvate codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }
}

/// Quanto l'orologio aspetta la libreria per scrivere volume ed equalizzatore.
///
/// Due secondi: il caso normale è che il lucchetto sia libero, e questa soglia
/// serve solo a non lasciare fermo per sempre il filo che manda la posizione. È
/// lo stesso numero oltre il quale `stato::con_libreria` scrive nel diario, e
/// non per caso: sopra i due secondi qualcuno sta tenendo il lucchetto per una
/// ragione che va guardata, e questo filo non è chi deve aspettarla.
const ATTESA_ORDINARIA: Duration = Duration::from_secs(2);

/// Quanto l'uscita aspetta la libreria per l'ultimo salvataggio.
///
/// Quattrocento millisecondi. Bastano a una transazione su un database locale, e
/// sono abbastanza pochi da non far sembrare bloccata la chiusura: quel che si
/// perde rinunciando è un volume da rimettere, quel che si perde aspettando è un
/// processo terminato a mano con il WAL a metà.
const ATTESA_USCENDO: Duration = Duration::from_millis(400);

/// L'ultimo giro di [`salva_quel_che_manca`], mentre si chiude.
///
/// Il filo dell'orologio esce appena la bandiera dello spegnimento si alza, e
/// senza questa chiamata l'ultima posizione di un cursore mosso un attimo prima
/// di chiudere sarebbe l'unica a non arrivare mai sul disco. Qui lo stato c'è
/// ancora: il `RunEvent` d'uscita arriva mentre la finestra esiste.
///
/// Con [`ATTESA_USCENDO`] e non con l'attesa dell'orologio: vedi «Perché il
/// lucchetto ha una scadenza» su [`salva_quel_che_manca`].
pub fn salva_uscendo(app: &tauri::AppHandle) {
    if let Some(stato) = app.try_state::<StatoLettore>() {
        salva_quel_che_manca(app, &stato, ATTESA_USCENDO);
    }
}

/// Avvia il filo che manda la posizione alla finestra.
///
/// Manda solo mentre suona: un'applicazione ferma in secondo piano non deve
/// svegliare il webview quattro volte al secondo per dirgli che non è cambiato
/// niente.
pub fn avvia_orologio(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("aether-orologio".to_owned())
        .spawn(move || {
            let mut ultimo_fermo = true;
            // Quanti giri d'orologio sono passati, per il segno di «riprendi
            // dov'eri». Un contatore locale al filo: non lo guarda nessun
            // altro, e non merita né un atomico né un lucchetto.
            let mut battiti: u32 = 0;
            // Il dispositivo era già sparito all'ultimo giro: serve perché
            // l'annuncio parta **una volta**, non quattro al secondo per tutto
            // il tempo in cui le cuffie restano staccate.
            let mut gia_perso = false;
            loop {
                std::thread::sleep(PASSO_TEMPO);
                // Prima di qualunque cosa che tocchi l'`AppHandle`: se la
                // finestra si sta chiudendo, un `emit` da qui fa panicare il
                // ciclo degli eventi. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                // `try_state` e non `state`: `state` panica se lo stato non
                // c'è, e questo filo gira anche mentre lo stato viene lasciato
                // cadere. Nessuno stato vuol dire che non c'è più niente da
                // raccontare a nessuno.
                let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
                    return;
                };

                // Le preferenze audio che i cursori hanno lasciato in sospeso.
                // Quasi sempre non c'è niente da fare: due letture rilassate.
                salva_quel_che_manca(&app, &stato_lettore, ATTESA_ORDINARIA);

                // Prima della posizione, perché è la ragione per cui la
                // posizione ha smesso di muoversi. Un dispositivo sparito
                // lascia la callback senza nessuno che la chiami: i fotogrammi
                // non avanzano più, il cursore resta fermo, e senza questo
                // controllo la finestra continua a dire che sta suonando.
                let perso = con_lettore(&stato_lettore, |lettore| Ok(guasto_di(&lettore.motore)))
                    .unwrap_or(None);
                match (&perso, gia_perso) {
                    (Some(guasto), false) => {
                        nota!("[riproduzione] il motore non suona più: {}", guasto.causa);
                        // Dov'era la musica, segnato adesso e una volta sola,
                        // sul fronte. La posizione la tiene il motore, e
                        // [`riapri_audio`] il motore lo butta per costruirne
                        // un altro — che di quel che il primo stava suonando
                        // non sa niente. Senza questa riga «Riapri» ricomincia
                        // dall'inizio del brano; con questa riga rimette la
                        // puntina nel solco. È la stessa funzione, e la stessa
                        // ragione, del ramo di rete in [`su_evento`].
                        annota_ripresa(&stato_lettore);
                        app.emetti("riproduzione:audio", guasto.clone());
                        // E lo stato intero, perché chi non stava ascoltando
                        // l'evento — una schermata aperta dopo — lo trovi lì.
                        let _ = con_lettore(&stato_lettore, |lettore| {
                            manda_stato(&app, lettore);
                            Ok(())
                        });
                        gia_perso = true;
                    }
                    (None, true) => gia_perso = false,
                    _ => {}
                }

                // Il timer di spegnimento. Prima della posizione perché se
                // scade adesso, la posizione che manderemmo fra due righe
                // sarebbe già quella di un lettore in pausa.
                scade_il_timer(&app, &stato_lettore);

                let tempo = con_lettore(&stato_lettore, |lettore| {
                    let p = lettore.motore.posizione();
                    Ok(Tempo {
                        posizione_ms: p.ms,
                        durata_ms: p.durata_ms,
                        in_pausa: p.in_pausa,
                    })
                });
                let Ok(tempo) = tempo else { continue };
                // Il filo dell'analisi sonora legge da qui se può leggere dal
                // disco. Passa da un'atomica e non da `con_lettore` perché
                // chiedere al lettore come sta, per sapere se disturbarlo,
                // sarebbe gia` disturbarlo: la ragione per esteso sta in testa
                // a `analisi.rs`.
                crate::analisi::segna_riproduzione(!tempo.in_pausa && perso.is_none());
                // Anche al sistema operativo, che dei salti relativi delle
                // cuffie sa solo il «di quanto» e mai il «da dove»: senza
                // questa riga il suo «da dove» resterebbe quello dell'ultimo
                // `manda_stato`, cioè quasi sempre l'inizio del brano.
                crate::media::segna_posizione(&app, tempo.posizione_ms);
                // Con il dispositivo perso la posizione è ferma per definizione:
                // mandarla quattro volte al secondo direbbe «sta suonando» a chi
                // interpola, che è la bugia che questo giro è venuto a togliere.
                if perso.is_some() {
                    continue;
                }
                // Un colpo anche quando si è appena fermato, per non lasciare il
                // cursore a interpolare nel vuoto.
                if !tempo.in_pausa || !ultimo_fermo {
                    app.emetti("riproduzione:tempo", tempo);
                }
                ultimo_fermo = tempo.in_pausa;

                // Il segno per «riprendi dov'eri», ogni tanto.
                //
                // Non a ogni giro: sarebbero quattro scritture al secondo sul
                // database per un numero che serve una volta sola, alla
                // prossima apertura. Ogni venti giri sono cinque secondi, che
                // è la peggior imprecisione possibile su una cosa che si
                // riprende a mano — e chi chiude a metà brano ritrova il segno
                // a cinque secondi da dove l'aveva lasciato, non all'inizio.
                battiti = battiti.wrapping_add(1);
                if !tempo.in_pausa && battiti % BATTITI_PER_SEGNO == 0 {
                    let stato_app = app.state::<Stato>();
                    let _ = con_libreria(&stato_app, |libreria| {
                        playback::save_posizione(&libreria.connection, tempo.posizione_ms)
                    });
                }
            }
        })
        .ok();
}

/// Avvia il filo che manda le bande dello spettro.
///
/// Separato dall'orologio e non un ramo dentro di esso: sono due cadenze
/// diverse per due ragioni diverse — vedi [`PASSO_SPETTRO`] — e infilarle nello
/// stesso ciclo avrebbe voluto dire mandare la posizione trenta volte al
/// secondo o le bande quattro.
///
/// Quando nessuno guarda dorme e non prende nessun lucchetto: la schermata
/// chiusa non costa niente né qui né nella callback audio.
pub fn avvia_spettro(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("aether-spettro".to_owned())
        .spawn(move || {
            loop {
                // Trenta giri al secondo: è il filo che ha più probabilità di
                // tutti di avere un messaggio in volo nell'istante in cui la
                // finestra si smonta, ed è per questo che il controllo sta in
                // cima. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
                    return;
                };
                if !stato_lettore
                    .spettro
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    std::thread::sleep(PASSO_SPETTRO_FERMO);
                    continue;
                }
                // La quantizzazione avviene **dentro** il lucchetto, e non è
                // una distrazione: è l'unico modo di leggere fino a 1024 bande
                // senza copiarle prima in un vettore da buttare via subito dopo.
                let bande = con_lettore(&stato_lettore, |lettore| {
                    Ok(lettore.motore.spettro(BandeIpc::da))
                });
                if let Ok(Some(bande)) = bande {
                    app.emetti("riproduzione:spettro", bande);
                }
                std::thread::sleep(PASSO_SPETTRO);
            }
        })
        .ok();
}
