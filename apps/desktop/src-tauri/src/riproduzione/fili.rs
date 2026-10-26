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
use rymembra_app::playback;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, adesso_ms, con_libreria_entro};

use super::coda::{scegli_da_solo, scrivi_coda};
use super::{
    AVANZAMENTO_MINIMO_MS, BATTITI_PER_SEGNO, BandeIpc, FINE_DEL_BRANO, Lettore, PASSO_SPETTRO,
    PASSO_SPETTRO_FERMO, PASSO_TEMPO, StatoLettore, Strada, Tempo, annota_ripresa,
    aspetta_il_seguito, brano_di, con_lettore, guasto_di, insieme, manda_stato, ospiti,
    presa_spettro_se_libera, strada,
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
pub(super) fn prepara_prossimo(app: &tauri::AppHandle, lettore: &mut Lettore) {
    // Il successivo in canna che non è più il brano dopo esce **adesso**, non
    // quando il preparatore avrà aperto quello nuovo: fino ad allora il motore
    // lo attaccherebbe alla fine del corrente — il brano appena tolto dalla
    // coda, o quello di prima di un riordino — e se il nuovo non si apre, quel
    // momento non arriva mai. Un gapless perso per qualche millisecondo costa
    // meno di un brano sbagliato. È una scrittura su un canale: il lucchetto in
    // mano non la rallenta.
    if lettore.in_canna.is_some() && lettore.in_canna != lettore.coda.peek_next() {
        lettore.prepara(None);
    }
    lettore.non_aperto = None;
    // Il lucchetto in mano dice anche un'altra cosa: è mentre ce l'ha che il
    // colpetto va mandato — così il filo che si sveglia trova la coda già nello
    // stato nuovo e non in quello di un istante prima.
    //
    // `try_state`: questa funzione la chiamano anche gli annunci del motore, su
    // un filo che può ritrovarsi vivo mentre gli stati se ne vanno. Senza lo
    // stato non c'è nessun preparatore da svegliare.
    if let Some(stato) = app.try_state::<StatoLettore>() {
        sveglia_preparatore(&stato);
    }
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

/// Il nome del filo del preparatore: nel diario dei panici e nella salute dei
/// fili, che lo contano per nome.
const FILO_PREPARATORE: &str = "rymembra-preparatore";

/// Il nome del filo dell'orologio. Vedi [`FILO_PREPARATORE`].
const FILO_OROLOGIO: &str = "rymembra-orologio";

/// Il nome del filo dello spettro. Vedi [`FILO_PREPARATORE`].
const FILO_SPETTRO: &str = "rymembra-spettro";

/// Il filo che apre il brano successivo.
///
/// Esiste per una ragione sola: **togliere l'apertura di un file dal filo della
/// decodifica**, dove stava perché l'osservatore ci gira sopra. Il perché per
/// esteso è in [`prepara_prossimo`].
///
/// Modellato sugli altri fili di questo albero — `avvia_orologio`,
/// `nuvola::avvia_filo`: un thread nominato, un canale, nessun runtime
/// asincrono.
///
/// # Sorvegliato
///
/// Ogni passata gira dentro `crate::stato::sorvegliato`. Prima un panico in una
/// passata — un brano aperto a metà, uno stato già smontato — srotolava fuori
/// dal ciclo e il filo finiva in silenzio: le spinte successive cadevano in un
/// canale che nessuno leggeva più, il brano dopo non veniva più preparato, e la
/// musica si fermava alla fine di ogni brano senza un errore da nessuna parte.
/// Adesso la passata caduta finisce nel diario e nella salute dei fili, e il
/// filo aspetta la spinta dopo.
///
/// Niente attesa dopo la caduta, al contrario dei fili a battito: questo non
/// gira da solo, gira a spinte, e le spinte arrivano a ogni cambio di brano —
/// un panico che si ripete non diventa un ciclo che consuma il processore.
pub fn avvia_preparatore(app: tauri::AppHandle, orecchio: std::sync::mpsc::Receiver<()>) {
    let avviato = std::thread::Builder::new()
        .name(FILO_PREPARATORE.to_owned())
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
                let _ = crate::stato::sorvegliato(FILO_PREPARATORE, || {
                    prepara_prossimo_adesso(&app);
                });
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
    // `try_state` e non `state`: `state` panica se lo stato non c'è, e questo
    // filo gira anche mentre gli stati vengono lasciati cadere.
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return;
    };

    // ── 0. l'autoplay, se la coda non ha un dopo ──
    //
    // Scegliere legge la libreria, e si fa **fuori** dal lucchetto del
    // lettore: prima si faceva dentro, e per tutta la scelta la pausa premuta
    // dalla finestra aspettava. Tre passi: sotto lucchetto si guarda se serve
    // e si fotografa la coda; fuori si sceglie; sotto lucchetto si accoda —
    // solo se nel frattempo la coda è rimasta quella fotografata, altrimenti
    // chi l'ha cambiata ha già deciso lui cosa viene dopo.
    let da_scegliere = con_lettore(&stato, |lettore| {
        let fine = stato.spegnimento.load(std::sync::atomic::Ordering::Relaxed) == FINE_DEL_BRANO;
        // In sessione l'autoplay tace: sceglierebbe dalla cronologia di uno
        // solo un brano per tutti, e la coda di sessione è fatta di voci, non
        // di righe. La preferenza resta scritta com'era, e torna all'uscita.
        // Vedi `insieme::autoplay`.
        if fine || !insieme::autoplay(lettore) || lettore.coda.peek_next().is_some() {
            return Ok(None);
        }
        Ok(lettore
            .coda
            .current()
            .map(|corrente| (corrente, lettore.coda.in_play_order())))
    });
    if let Ok(Some((corrente, esclusi))) = da_scegliere
        && let Some(scelta) = scegli_da_solo(app, corrente, &esclusi)
    {
        let _ = con_lettore(&stato, |lettore| {
            if lettore.coda.current() == Some(corrente)
                && lettore.coda.peek_next().is_none()
                && lettore.coda.in_play_order() == esclusi
            {
                lettore.coda.enqueue(&[scelta.id]);
                lettore.motivo_prossimo = Some((scelta.id, scelta.motivo.codice()));
            }
            Ok(())
        });
    }

    // ── 1. la decisione, sotto lucchetto ──
    let scelta = con_lettore(&stato, |lettore| {
        // Il timer «fine del brano» si fa qui, e non con una sveglia: non è un
        // istante da aspettare ma un successivo che non deve esserci. Detto
        // così, la musica finisce dove sarebbe finita comunque — senza
        // dissolvenze, senza tagli, senza un secondo di silenzio prima del
        // previsto.
        let fine = stato.spegnimento.load(std::sync::atomic::Ordering::Relaxed) == FINE_DEL_BRANO;
        // Durante la calibrazione il motore suona i clic, che non stanno in
        // coda: un successivo in canna lo attaccherebbe alla fine dei clic,
        // e la voce di prima la riapre la calibrazione quando si spegne.
        if fine || insieme::in_calibrazione(lettore) {
            lettore.prepara(None);
            return Ok(None);
        }

        // La coda non ha un dopo: è lì che l'autoplay entra — al passo zero,
        // qui sopra — e **lì** e non sull'evento `Fermato`. Accodando adesso,
        // mentre il brano corrente suona ancora, quel che si sceglie passa
        // dalla stessa strada di tutti gli altri: viene aperto in anticipo,
        // attacca senza stacco, e non c'è nessun istante in cui l'applicazione
        // si sia fermata. Reagire a `Fermato` vorrebbe dire ripartire *dopo* il
        // silenzio.
        let formato = lettore.motore.formato();
        match lettore.coda.peek_next() {
            // Nessun successivo: azzerare è l'unica cosa da fare, dura quanto
            // una scrittura su un canale, e si fa subito qui.
            None => {
                lettore.prepara(None);
                Ok(None)
            }
            // Già in canna: non c'è niente da fare, e farlo sarebbe peggio
            // che inutile.
            //
            // Il preparatore viene svegliato due volte per lo stesso
            // successivo a ogni partenza — una da `avvia_corrente`, una
            // dall'annuncio di `Iniziato` — e l'antirimbalzo di
            // [`RAFFICA_PREPARA`] le fonde solo se cadono vicine, che è quel che
            // succede quasi sempre e non quel che si può dare per buono. La
            // seconda passata riapriva lo stesso file per consegnare lo stesso
            // brano.
            //
            // «Peggio che inutile» perché il motore, nel frattempo, può averlo
            // **già attaccato**: davanti alla callback c'è più di un secondo di
            // musica — vedi `rymembra_play::uscita::RISERVA_MS` — e in quel
            // margine `Iniziato` non è ancora arrivato, quindi la coda non è
            // ancora avanzata e `peek_next` indica ancora lui. La seconda copia
            // finiva in canna dietro sé stessa, e alla fine del brano il motore
            // l'attaccava: lo stesso brano due volte di fila, che con un
            // intermezzo più corto della riserva capitava a ogni traccia.
            //
            // `in_canna` è esattamente la domanda «il successivo è già questo?»,
            // ed è quel che il suo doc-comment su [`crate::riproduzione::Lettore`]
            // dichiara di servire a sapere.
            Some(id) if lettore.in_canna == Some(id) => Ok(None),
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
            // Quel che era in canna prima non è il brano dopo: se la coda dice
            // ancora questo, in canna non deve restare niente — altrimenti alla
            // fine del corrente suonerebbe il successivo di prima, e la barra
            // mostrerebbe questo. Vedi `Lettore::non_aperto` per cosa succede
            // quando il corrente finisce.
            let _ = con_lettore(&stato, |lettore| {
                if lettore.coda.peek_next() == Some(id) {
                    lettore.prepara(None);
                    lettore.non_aperto = Some(id);
                }
                Ok(())
            });
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
        // E può essere cambiato il motore: un'uscita riaperta mentre il file si
        // apriva — le cuffie a 48 kHz al posto degli altoparlanti a 44,1, un
        // DAC a sei canali al posto di due. Il brano è stato preparato per il
        // formato di prima, e consegnato a questo suonerebbe alla velocità o con
        // i canali sbagliati. Si lascia cadere e si chiede di nuovo, col
        // formato di adesso.
        if lettore.motore.formato() != formato {
            sveglia_preparatore(&stato);
            return Ok(());
        }
        lettore.prepara(Some(brano));
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

    // In sessione si rifiuta, anche per spegnerlo: all'ingresso lo si è già
    // spento, e un timer che ferma l'host fermerebbe tutti. Vedi
    // `crate::insieme::strada`.
    if let Strada::Fatto = strada(&app, &crate::insieme::Gesto::Timer).map_err(errore)? {
        return Ok(());
    }
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
        // Un timer toccato mentre la musica si sta già spegnendo è un timer che
        // qualcuno ha deciso di cambiare: la discesa si lascia perdere, e il
        // volume torna dov'era. Vedi [`scade_il_timer`].
        if stato.sfumando_fino.swap(0, Ordering::Relaxed) != 0 {
            lettore.motore.annulla_sfumatura();
        }
        prepara_prossimo(&app, lettore);
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Quanto dura la discesa del volume quando scade il timer di spegnimento.
///
/// Sei secondi: abbastanza perché la musica sembri allontanarsi invece di
/// interrompersi, pochi abbastanza da non sembrare un timer che non scatta.
const SFUMATURA_SPEGNIMENTO: Duration = Duration::from_secs(6);

/// Fa scendere il volume e poi mette in pausa, se il timer di spegnimento è
/// scaduto.
///
/// Pausa e non `ferma`: chi si addormenta con la musica accesa, al risveglio,
/// vuole ritrovare il segno dov'era. `ferma` butterebbe la posizione, e il
/// mattino dopo il brano ripartirebbe da capo senza che nessuno capisca
/// perché.
///
/// # Perché una discesa e non la pausa secca
///
/// Perché la pausa secca era il modo peggiore di finire la serata di chi si
/// stava addormentando: la musica si fermava a metà di una frase, e un
/// silenzio improvviso sveglia quanto un rumore. Adesso il motore scende in
/// [`SFUMATURA_SPEGNIMENTO`] e si mette in pausa da solo — vedi
/// `rymembra_play::Motore::sfuma` — e questa funzione, a discesa finita, ferma
/// l'ascolto e lo dice alla finestra. Qualunque gesto nel frattempo — play,
/// un altro brano, il timer toccato — la annulla.
///
/// Il modo «fine del brano» non passa di qui: quello non è una scadenza ma un
/// brano successivo che non viene preparato, e lo decide
/// [`prepara_prossimo`].
fn scade_il_timer(app: &tauri::AppHandle, stato_lettore: &StatoLettore) {
    use std::sync::atomic::Ordering;

    // La discesa finita: il motore è già in pausa, se nessuno l'ha fermata.
    let fino = stato_lettore.sfumando_fino.load(Ordering::Relaxed);
    if fino > 0
        && adesso_ms() >= fino
        && stato_lettore
            .sfumando_fino
            .compare_exchange(fino, 0, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        let _ = con_lettore(stato_lettore, |lettore| {
            // In pausa vuol dire che la discesa è arrivata in fondo. Se no,
            // qualcuno l'ha interrotta scegliendo un altro brano o premendo
            // play, e non c'è niente da fermare.
            if lettore.motore.posizione().in_pausa
                && let Some(ascolto) = lettore.ascolto.as_mut()
            {
                ascolto.pause(adesso_ms());
            }
            manda_stato(app, lettore);
            Ok(())
        });
    }

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
        // Già fermo: non c'è niente da far scendere, resta da dire alla
        // finestra che il timer non c'è più.
        if !lettore.motore.posizione().in_pausa {
            let durata = u64::try_from(SFUMATURA_SPEGNIMENTO.as_millis()).unwrap_or(6_000);
            lettore.motore.sfuma(durata);
            // La sveglia con mezzo secondo di margine: la discesa la misura il
            // filo della decodifica col suo orologio, e controllare prima che
            // abbia finito scambierebbe una discesa in corso per una
            // interrotta — cioè un ascolto che continua a contare a musica
            // ferma.
            let fino = adesso_ms()
                .saturating_add(i64::try_from(durata).unwrap_or(6_000))
                .saturating_add(500);
            stato_lettore.sfumando_fino.store(fino, Ordering::Relaxed);
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
///
/// # E la coda
///
/// Dalla 3.0 anche la coda passa di qui: il gesto alza
/// [`StatoLettore::coda_da_salvare`] e la scrittura arriva al giro dopo. Stessa
/// bandiera, stessa scadenza, stessa regola del rialzo — e stessa ragione:
/// scriverla dentro il gesto voleva dire tenere il lucchetto del lettore per
/// tutto il tempo di una transazione.
fn salva_quel_che_manca(app: &tauri::AppHandle, stato: &StatoLettore, entro: Duration) {
    use std::sync::atomic::Ordering::Relaxed;
    if stato.coda_da_salvare.swap(false, Relaxed) {
        match scrivi_coda(app, stato, entro) {
            Some(Ok(())) => {}
            Some(Err(err)) => nota!(
                "[riproduzione] coda non salvata codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            ),
            None => {
                stato.coda_da_salvare.fetch_or(true, Relaxed);
                nota!(
                    "[riproduzione] coda rinviata: la libreria era occupata da più di {} ms",
                    entro.as_millis()
                );
            }
        }
    }
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
///
/// Prima, però, si lascia finire il filo del seguito — con la stessa scadenza:
/// là dentro possono esserci gli ascolti chiusi un istante prima di uscire, e
/// un ascolto finito che non arriva sul disco è una canzone che non conta.
pub fn salva_uscendo(app: &tauri::AppHandle) {
    if let Some(stato) = app.try_state::<StatoLettore>() {
        aspetta_il_seguito(app, ATTESA_USCENDO);
        salva_quel_che_manca(app, &stato, ATTESA_USCENDO);
    }
}

/// Avvia il filo che manda la posizione alla finestra.
///
/// Manda solo mentre suona: un'applicazione ferma in secondo piano non deve
/// svegliare il webview quattro volte al secondo per dirgli che non è cambiato
/// niente.
///
/// # Sorvegliato
///
/// Ogni giro gira dentro `crate::stato::sorvegliato`. Questo filo fa quattro
/// lavori che non hanno nessun altro a farli — la posizione alla finestra, il
/// timer di spegnimento, il salvataggio di volume, curva e coda, e il primo
/// avviso del dispositivo perso — e prima un panico in un giro li fermava tutti
/// e quattro per il resto della sessione, senza una riga da nessuna parte che
/// non fosse la traccia del panico. Adesso il giro caduto si annota e il filo
/// riparte, dopo un'attesa che cresce se continua a cadere
/// (`crate::stato::attesa_dopo_la_caduta`): un panico che si ripete identico a
/// ogni giro non deve diventare quattro righe al secondo nel diario.
///
/// Se il filo non parte lo dice il diario, come per gli altri fili del lettore.
/// Prima l'esito della `spawn` si buttava, e un orologio mai partito era un
/// cursore fermo senza nessuna spiegazione.
pub fn avvia_orologio(app: tauri::AppHandle) {
    let avviato = std::thread::Builder::new()
        .name(FILO_OROLOGIO.to_owned())
        .spawn(move || {
            let mut memoria = Orologio::default();
            let mut cadute_di_fila = 0_u32;
            loop {
                std::thread::sleep(PASSO_TEMPO);
                // Prima di qualunque cosa che tocchi l'`AppHandle`: se la
                // finestra si sta chiudendo, un `emit` da qui fa panicare il
                // ciclo degli eventi. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                match crate::stato::sorvegliato(FILO_OROLOGIO, || {
                    giro_d_orologio(&app, &mut memoria)
                }) {
                    Some(std::ops::ControlFlow::Break(())) => return,
                    Some(std::ops::ControlFlow::Continue(())) => cadute_di_fila = 0,
                    None => {
                        cadute_di_fila = cadute_di_fila.saturating_add(1);
                        std::thread::sleep(crate::stato::attesa_dopo_la_caduta(cadute_di_fila));
                    }
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo dell'orologio non è partito: {err}");
    }
}

/// Quel che l'orologio ricorda da un giro all'altro.
///
/// Locale al filo: non lo guarda nessun altro, e non merita né un atomico né un
/// lucchetto. In una struttura e non in cinque variabili perché il giro gira
/// dentro `crate::stato::sorvegliato`, che vuole una chiusura: la chiusura se
/// la presta, e quel che ricorda sopravvive al giro anche quando il giro cade.
struct Orologio {
    /// Il giro prima era fermo: serve a mandare un colpo anche quando la musica
    /// si è appena fermata, per non lasciare il cursore a interpolare nel vuoto.
    ultimo_fermo: bool,
    /// Quanti giri d'orologio sono passati, per il segno di «riprendi dov'eri».
    battiti: u32,
    /// Il dispositivo era già sparito all'ultimo giro: serve perché l'annuncio
    /// parta **una volta**, non quattro al secondo per tutto il tempo in cui le
    /// cuffie restano staccate.
    gia_perso: bool,
    /// Per «in attesa di dati»: com'era al giro prima — brano, millisecondo,
    /// vuoti serviti dalla callback. Vedi `Tempo::in_attesa`.
    prima: (Option<i64>, u64, u64),
    /// Da quanti giri di fila la musica non avanza mentre la callback serve
    /// silenzio.
    giri_a_secco: u8,
}

impl Default for Orologio {
    fn default() -> Self {
        Self {
            ultimo_fermo: true,
            battiti: 0,
            gia_perso: false,
            prima: (None, 0, 0),
            giri_a_secco: 0,
        }
    }
}

/// Un giro d'orologio. `Break` quando non c'è più niente da raccontare a
/// nessuno — lo stato del lettore se n'è andato — e il filo deve uscire.
fn giro_d_orologio(app: &tauri::AppHandle, memoria: &mut Orologio) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow;

    // `try_state` e non `state`: `state` panica se lo stato non c'è, e questo
    // filo gira anche mentre lo stato viene lasciato cadere. Nessuno stato vuol
    // dire che non c'è più niente da raccontare a nessuno.
    let Some(stato_lettore) = app.try_state::<StatoLettore>() else {
        return ControlFlow::Break(());
    };

    // Le preferenze audio che i cursori hanno lasciato in sospeso. Quasi sempre
    // non c'è niente da fare: due letture rilassate.
    salva_quel_che_manca(app, &stato_lettore, ATTESA_ORDINARIA);

    // Prima della posizione, perché è la ragione per cui la posizione ha smesso
    // di muoversi. Un dispositivo sparito lascia la callback senza nessuno che
    // la chiami: i fotogrammi non avanzano più, il cursore resta fermo, e senza
    // questo controllo la finestra continua a dire che sta suonando.
    let perso =
        con_lettore(&stato_lettore, |lettore| Ok(guasto_di(&lettore.motore))).unwrap_or(None);
    match (&perso, memoria.gia_perso) {
        (Some(guasto), false) => {
            nota!("[riproduzione] il motore non suona più: {}", guasto.causa);
            // Dov'era la musica, segnato adesso e una volta sola, sul fronte. La
            // posizione la tiene il motore, e [`riapri_audio`] il motore lo
            // butta per costruirne un altro — che di quel che il primo stava
            // suonando non sa niente. Senza questa riga «Riapri» ricomincia
            // dall'inizio del brano; con questa riga rimette la puntina nel
            // solco. È la stessa funzione, e la stessa ragione, del ramo di rete
            // in [`su_evento`].
            annota_ripresa(&stato_lettore);
            app.emetti("riproduzione:audio", guasto.clone());
            // E lo stato intero, perché chi non stava ascoltando l'evento — una
            // schermata aperta dopo — lo trovi lì.
            let _ = con_lettore(&stato_lettore, |lettore| {
                manda_stato(app, lettore);
                Ok(())
            });
            memoria.gia_perso = true;
        }
        (None, true) => memoria.gia_perso = false,
        _ => {}
    }

    // Il timer di spegnimento. Prima della posizione perché se scade adesso, la
    // posizione che manderemmo fra due righe sarebbe già quella di un lettore in
    // pausa.
    scade_il_timer(app, &stato_lettore);

    let letto = con_lettore(&stato_lettore, |lettore| {
        let p = lettore.motore.posizione();
        // In sessione, fra una partenza programmata e il primo campione, il
        // motore dice ancora il punto del brano di prima: alla finestra va
        // quello promesso, con la durata della voce. Vedi
        // `insieme::posizione_da_mostrare`.
        let mostrata = insieme::posizione_da_mostrare(lettore, &p).map(|ms| {
            let durata = insieme::corrente_da_mostrare(lettore)
                .and_then(|id| ospiti::durata_ms(app, id))
                .unwrap_or(p.durata_ms);
            (ms, durata)
        });
        Ok((
            p.track_id,
            lettore.motore.vuoti(),
            Tempo {
                posizione_ms: p.ms,
                durata_ms: p.durata_ms,
                in_pausa: p.in_pausa,
                in_attesa: false,
            },
            mostrata,
        ))
    });
    let Ok((di_chi, vuoti, mut tempo, mostrata)) = letto else {
        return ControlFlow::Continue(());
    };
    // A secco: nessuna pausa voluta, la callback ha servito silenzio dall'ultimo
    // giro, e la posizione dello stesso brano non si è mossa. Due giri di fila,
    // e non uno, perché un salto su un file svuota l'anello e lo riempie in
    // qualche millisecondo: un vuoto solo è quello, e annunciarlo farebbe
    // lampeggiare la scritta a ogni salto.
    let (brano_prima, ms_prima, vuoti_prima) = memoria.prima;
    let a_secco = !tempo.in_pausa
        && vuoti > vuoti_prima
        && di_chi == brano_prima
        && tempo.posizione_ms.saturating_sub(ms_prima) < AVANZAMENTO_MINIMO_MS;
    memoria.giri_a_secco = if a_secco {
        memoria.giri_a_secco.saturating_add(1)
    } else {
        0
    };
    tempo.in_attesa = memoria.giri_a_secco >= 2;
    memoria.prima = (di_chi, tempo.posizione_ms, vuoti);
    // Dopo il conto «a secco», che parla del motore e non di quel che si
    // mostra: un punto promesso che non si muove per un quarto di secondo non
    // è una rete che non risponde.
    let ms_del_motore = tempo.posizione_ms;
    if let Some((ms, durata)) = mostrata {
        tempo.posizione_ms = ms;
        tempo.durata_ms = durata;
    }
    // Il filo dell'analisi sonora legge da qui se può leggere dal disco. Passa
    // da un'atomica e non da `con_lettore` perché chiedere al lettore come sta,
    // per sapere se disturbarlo, sarebbe gia` disturbarlo: la ragione per esteso
    // sta in testa a `analisi.rs`.
    crate::analisi::segna_riproduzione(!tempo.in_pausa && perso.is_none());
    // Anche al sistema operativo, che dei salti relativi delle cuffie sa solo il
    // «di quanto» e mai il «da dove»: senza questa riga il suo «da dove»
    // resterebbe quello dell'ultimo `manda_stato`, cioè quasi sempre l'inizio
    // del brano.
    crate::media::segna_posizione(app, tempo.posizione_ms);
    // Con il dispositivo perso la posizione è ferma per definizione: mandarla
    // quattro volte al secondo direbbe «sta suonando» a chi interpola, che è la
    // bugia che questo giro è venuto a togliere.
    if perso.is_some() {
        return ControlFlow::Continue(());
    }
    // Un colpo anche quando si è appena fermato, per non lasciare il cursore a
    // interpolare nel vuoto.
    if !tempo.in_pausa || !memoria.ultimo_fermo {
        app.emetti("riproduzione:tempo", tempo);
    }
    memoria.ultimo_fermo = tempo.in_pausa;

    // Il segno per «riprendi dov'eri», ogni tanto.
    //
    // Non a ogni giro: sarebbero quattro scritture al secondo sul database per
    // un numero che serve una volta sola, alla prossima apertura. Ogni venti
    // giri sono cinque secondi, che è la peggior imprecisione possibile su una
    // cosa che si riprende a mano — e chi chiude a metà brano ritrova il segno a
    // cinque secondi da dove l'aveva lasciato, non all'inizio.
    //
    // Con il brano accanto: vedi `playback::save_posizione`.
    memoria.battiti = memoria.battiti.wrapping_add(1);
    //
    // Con una scadenza, come ogni altra scrittura di questo filo: senza, una
    // libreria occupata lo teneva fermo qui, e con lui la posizione, il timer e
    // la sorveglianza del dispositivo. Un segno saltato si riscrive fra cinque
    // secondi.
    //
    // Solo per una riga della libreria: una maniglia domani non vuol dire
    // niente, e `save_posizione` la rifiuterebbe con una riga nel diario ogni
    // cinque secondi per tutta la sessione.
    if let Some(brano) = di_chi.filter(|id| ospiti::e_di_libreria(*id))
        && !tempo.in_pausa
        && memoria.battiti.is_multiple_of(BATTITI_PER_SEGNO)
        && let Some(stato_app) = app.try_state::<Stato>()
    {
        let _ = con_libreria_entro(&stato_app, ATTESA_ORDINARIA, |libreria| {
            playback::save_posizione(&libreria.connection, brano, ms_del_motore)
        });
    }
    ControlFlow::Continue(())
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
///
/// # Sorvegliato
///
/// Come l'orologio, e con una ragione in più per l'attesa dopo la caduta: questo
/// filo gira trenta volte al secondo, e un panico che si ripetesse a ogni giro
/// sarebbero trenta righe al secondo nel diario.
///
/// # A scadenze, non a sonni
///
/// Fino alla 3.0.1 dormiva [`PASSO_SPETTRO`] **dopo** il lavoro, quindi il
/// periodo vero era il passo più il lavoro più l'attesa del lucchetto: una
/// cadenza che deriva, e che la scena — che fa scorrere il passato a file che
/// arrivano — vedeva come scorrimento irregolare. Adesso dorme fino a una
/// scadenza assoluta che avanza di un passo per giro (vedi [`attesa_fino`]), e
/// il lavoro del giro sta dentro il passo invece di aggiungersi.
pub fn avvia_spettro(app: tauri::AppHandle) {
    let avviato = std::thread::Builder::new()
        .name(FILO_SPETTRO.to_owned())
        .spawn(move || {
            let mut cadute_di_fila = 0_u32;
            // Vive qui e non nel giro: è la presa da usare quando il lettore è
            // occupato. Vedi `presa_spettro_se_libera`.
            let mut presa = None;
            let mut scadenza = std::time::Instant::now();
            loop {
                // Trenta giri al secondo: è il filo che ha più probabilità di
                // tutti di avere un messaggio in volo nell'istante in cui la
                // finestra si smonta, ed è per questo che il controllo sta in
                // cima. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                match crate::stato::sorvegliato(FILO_SPETTRO, || {
                    giro_dello_spettro(&app, &mut presa)
                }) {
                    Some(Some(true)) => {
                        cadute_di_fila = 0;
                        let adesso = std::time::Instant::now();
                        std::thread::sleep(attesa_fino(&mut scadenza, adesso, PASSO_SPETTRO));
                    }
                    Some(Some(false)) => {
                        cadute_di_fila = 0;
                        std::thread::sleep(PASSO_SPETTRO_FERMO);
                        // Il primo giro dopo il riposo parte subito: la
                        // scadenza vecchia è secondi indietro, e non c'è niente
                        // da recuperare.
                        scadenza = std::time::Instant::now();
                    }
                    Some(None) => return,
                    None => {
                        cadute_di_fila = cadute_di_fila.saturating_add(1);
                        std::thread::sleep(crate::stato::attesa_dopo_la_caduta(cadute_di_fila));
                        scadenza = std::time::Instant::now();
                    }
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo dello spettro non è partito: {err}");
    }
}

/// Quanto concedere di ritardo prima di ripartire invece di recuperare.
///
/// Mezzo passo. Un giro arrivato poco dopo la sua scadenza — un sonno che si è
/// allungato di un millisecondo, un filo che il sistema ha servito tardi — parte
/// subito, e la cadenza si tiene. Uno arrivato con un passo intero di ritardo
/// **non** si recupera con un giro a raffica: due eventi a pochi millesimi l'uno
/// dall'altro sarebbero proprio lo scatto che questa cadenza esiste per non
/// mandare. Si riparte da adesso, e la fila persa resta persa.
const TOLLERANZA_SPETTRO: Duration = Duration::from_millis(16);

/// Quanto dormire fino alla scadenza, e la scadenza dopo.
///
/// Scadenze assolute: `scadenza` avanza di `passo` a ogni giro, qualunque cosa
/// il giro abbia fatto, quindi il lavoro sta **dentro** il passo e la cadenza
/// non deriva. Oltre [`TOLLERANZA_SPETTRO`] di ritardo si riparte da `adesso`,
/// senza recuperare.
fn attesa_fino(
    scadenza: &mut std::time::Instant,
    adesso: std::time::Instant,
    passo: Duration,
) -> Duration {
    if adesso.saturating_duration_since(*scadenza) > TOLLERANZA_SPETTRO {
        *scadenza = adesso;
    }
    let attesa = scadenza.saturating_duration_since(adesso);
    *scadenza += passo;
    attesa
}

/// Un giro del filo dello spettro: `Some(true)` se qualcuno guarda, `Some(false)`
/// se no, `None` se lo stato del lettore se n'è andato e il filo deve uscire.
///
/// # Perché la trasformata si fa fuori dal lucchetto
///
/// Perché leggere le bande è svuotare l'anello dei campioni e trasformarne la
/// finestra, trenta volte al secondo, e fino a qui si faceva con il lucchetto
/// del lettore in mano: la pausa, il salto e gli annunci del motore si mettevano
/// in fila dietro una trasformata per tutto il tempo in cui la schermata era
/// aperta. Sotto il lucchetto adesso si prende soltanto la presa — un `Arc` da
/// clonare, vedi `rymembra_play::Motore::presa_spettro` — e il resto si fa qui,
/// a mani libere. La quantizzazione resta **dentro** il lucchetto della presa,
/// che è suo e di nessun altro: è l'unico modo di leggere fino a 1024 bande
/// senza copiarle prima in un vettore da buttare via subito dopo.
///
/// E nemmeno per la presa si aspetta più: vedi [`presa_spettro_se_libera`].
fn giro_dello_spettro(
    app: &tauri::AppHandle,
    presa: &mut Option<rymembra_play::motore::PresaSpettro>,
) -> Option<bool> {
    let stato_lettore = app.try_state::<StatoLettore>()?;
    if !stato_lettore
        .spettro
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        // Nessuno guarda: la presa si lascia, così non tiene in vita il lettore
        // di un motore che nel frattempo potrebbe essere sostituito.
        *presa = None;
        return Some(false);
    }
    if let Some(nuova) = presa_spettro_se_libera(&stato_lettore) {
        *presa = Some(nuova);
    }
    if let Some(presa) = presa.as_ref()
        && let Some(bande) = presa.leggi(BandeIpc::da)
    {
        app.emetti("riproduzione:spettro", bande);
    }
    Some(true)
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::time::Instant;

    const PASSO: Duration = Duration::from_millis(33);

    #[test]
    fn la_cadenza_non_deriva_col_lavoro() {
        // Il difetto di prima: dormire un passo **dopo** il lavoro fa del
        // periodo il passo più il lavoro. Con le scadenze, dieci giri con dieci
        // millesimi di lavoro l'uno finiscono esattamente a dieci passi.
        let inizio = Instant::now();
        let mut scadenza = inizio;
        let mut adesso = inizio;
        for _ in 0..10 {
            adesso += Duration::from_millis(10);
            let attesa = attesa_fino(&mut scadenza, adesso, PASSO);
            adesso += attesa;
        }
        assert_eq!(adesso.duration_since(inizio), Duration::from_millis(9 * 33));
        assert_eq!(
            scadenza.duration_since(inizio),
            Duration::from_millis(10 * 33)
        );
    }

    #[test]
    fn un_ritardo_piccolo_si_assorbe() {
        // Un sonno allungato di qualche millesimo: il giro parte subito e la
        // scadenza dopo resta dov'era, cioè la cadenza si tiene.
        let inizio = Instant::now();
        let mut scadenza = inizio + PASSO;
        let attesa = attesa_fino(
            &mut scadenza,
            inizio + PASSO + Duration::from_millis(5),
            PASSO,
        );
        assert_eq!(attesa, Duration::ZERO);
        assert_eq!(scadenza, inizio + PASSO * 2);
    }

    #[test]
    fn un_ritardo_grande_non_diventa_una_raffica() {
        // Un filo fermo per un decimo di secondo non recupera tre giri a
        // raffica: riparte da adesso, e il giro dopo è un passo più in là.
        let inizio = Instant::now();
        let mut scadenza = inizio + PASSO;
        let tardi = inizio + Duration::from_millis(140);
        let attesa = attesa_fino(&mut scadenza, tardi, PASSO);
        assert_eq!(attesa, Duration::ZERO);
        assert_eq!(scadenza, tardi + PASSO);
        let dopo = attesa_fino(&mut scadenza, tardi + Duration::from_millis(1), PASSO);
        assert_eq!(dopo, Duration::from_millis(32));
    }
}
