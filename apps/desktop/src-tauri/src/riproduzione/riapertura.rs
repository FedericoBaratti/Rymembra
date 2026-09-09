//! Rimettere in piedi il motore, e la puntina nel solco.
//!
//! Il dispositivo audio che se ne va — staccato, cambiato, piantato — e il
//! motore nuovo che prende il posto del vecchio senza portarsi via la coda, il
//! volume, la curva e il punto in cui la musica si era interrotta. Più il tasto
//! «Riprova» dell'avviso di rete, che è la stessa strada per un guasto diverso.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui, con l'aggiunta che
//! aprire un dispositivo audio è un'attesa come le altre: si apre il motore
//! nuovo **prima** di chiedere il lucchetto del lettore.

use aether_app::playback::{Equalizzazione, Normalizzazione, Volume};
use aether_domain::errors::AppError;
use aether_domain::queue::Queue;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;

use super::coda::riprendi_coda;
use super::fili::prepara_prossimo;
use super::{
    Lettore, StatoLettore, apri_motore, brano_di, con_lettore, manda_stato,
    manda_stato_con_posizione,
};

/// L'ultimo brano su cui la puntina è stata rimessa, e quanti fili di
/// decodifica erano caduti in quel momento.
///
/// # Il ciclo che questa casella rompe
///
/// Un file che fa cadere il decodificatore lo rifà **ogni volta** che lo si
/// rilegge: i byte sono gli stessi, e symphonia ci inciampa allo stesso punto.
/// Da quando il filo caduto alza la stessa bandiera del dispositivo perso, la
/// catena si chiude su sé stessa — il sorvegliante vede il guasto, ricostruisce
/// il motore, [`riprendi_dov_era`] rimette la puntina su quel brano, il filo
/// cade di nuovo — e quel che l'utente ha davanti è un motore ricostruito ogni
/// due secondi, per sempre. Rimettere la puntina nello stesso solco è la mossa
/// giusta per un cavo staccato e la peggiore per un file che fa cadere chi lo
/// legge, e i due casi si distinguono da un numero solo:
/// `aether_play::motore::fili_caduti`, che cresce quando il filo cade.
///
/// # Perché una casella di modulo e non un campo dello stato
///
/// Perché quel che si confronta non sopravvive dentro nessuno degli oggetti che
/// il guasto porta via: fra il momento in cui si annota e quello in cui si
/// rilegge ci sono un motore buttato e uno costruito. Sta qui accanto alla
/// funzione che la usa, che è l'unica, e ci sta bene proprio come il contatore
/// che confronta: uno per processo, perché uno è il lettore.
///
/// Un brano solo, il più recente, ed è quanto basta: il ciclo da rompere è
/// quello di un brano rimesso al suo posto subito dopo aver messo giù il filo.
static ULTIMA_RIPRESA: std::sync::Mutex<Option<(i64, u32)>> = std::sync::Mutex::new(None);

/// Si può rimettere la puntina su questo brano, o è lui ad aver messo giù il
/// filo?
///
/// `true` quando si può — e allora si annota che ci si prova adesso, con quanti
/// fili erano caduti fino a questo istante. `false` quando l'ultimo tentativo
/// su **questo stesso brano** è finito con un filo per terra: il numero è
/// cresciuto da allora, e cresce solo in un modo. Vedi [`ULTIMA_RIPRESA`].
fn si_puo_rimettere_la_puntina(track_id: i64) -> bool {
    let caduti = aether_play::motore::fili_caduti();
    let mut guardia = ULTIMA_RIPRESA
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((brano, allora)) = *guardia
        && brano == track_id
        && caduti > allora
    {
        return false;
    }
    *guardia = Some((track_id, caduti));
    true
}

/// Riapre il dispositivo audio, a mano.
///
/// # Perché esiste ancora, se adesso si riapre da sola
///
/// Perché [`avvia_sorveglianza`](super::uscite::avvia_sorveglianza) guarda
/// **l'elenco dei dispositivi**, e ci sono guasti che l'elenco non racconta: un
/// driver che si pianta lasciando l'endpoint al suo posto, un'apertura in
/// modalità esclusiva rubata da un'altra applicazione, un `BackendSpecific`
/// qualunque. In quei casi il dispositivo c'è, ha sempre lo stesso nome, e il
/// sorvegliante non ha niente da confrontare: il tasto resta l'ultima parola.
///
/// # La regola di prima, e perché è cambiata
///
/// Qui c'era scritto che riaprire da soli era sbagliato: `cpal` apriva il
/// **predefinito di sistema**, e cuffie staccate voleva dire predefinito
/// tornato agli altoparlanti, cioè musica in un ufficio, di notte, in una
/// riunione. Era vero finché l'uscita non si poteva scegliere.
///
/// Adesso si può — `player.output`, vedi
/// [`scegli_dispositivo_audio`](super::uscite::scegli_dispositivo_audio) — e la
/// riapertura è automatica: chi non vuole che il suono si sposti fissa la sua
/// scheda, e la preferenza vale più del predefinito di sistema. Chi resta su
/// «predefinito» ha chiesto proprio di seguire il sistema. In tutti e due i
/// casi la finestra dice **su quale uscita** è finito il suono, che è la parte
/// che prima mancava del tutto.
///
/// Resta vera la ragione tecnica scritta in `uscita.rs`: riaprire può
/// bloccarsi, e non si può fare dal filo che segnala il guasto — quello è un
/// `StreamError` che arriva da dentro `cpal`.
///
/// `(async)`: aprire un dispositivo audio parla con il sistema e può prendersi
/// il suo tempo. Sul filo principale sarebbe la finestra ferma.
#[tauri::command(async)]
pub fn riapri_audio(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    // A mano si riparte come si stava: se la musica andava, riprende; se era
    // in pausa, resta ferma. La stessa regola dell'automatico — l'ha decisa
    // `annota_ripresa`, non chi chiama.
    riapri_su(&app, &stato, None).map_err(errore)
}

/// Il lavoro vero di [`riapri_audio`], senza il comando intorno.
///
/// In una funzione sua perché la chiamano in tre: il tasto, il sorvegliante dei
/// dispositivi, e la scelta di un'uscita nuova dalle impostazioni. Tre copie di
/// questa strada vorrebbero dire tre modi diversi di sbagliare a rimettere in
/// piedi il volume — ed è la stessa ragione per cui [`apri_motore`] esiste.
///
/// `uscita` dice su cosa aprire: `None` vuol dire «quel che è scritto nella
/// preferenza», ed è quasi sempre la risposta giusta. Passarne una esplicita
/// serve a un caso solo — le impostazioni, che l'hanno appena cambiata e non
/// devono aspettare che la copia in memoria si allinei.
///
/// # Cosa sopravvive
///
/// La coda, il volume, la curva dell'equalizzatore e la normalizzazione: sono
/// tutte cose del lettore, non del dispositivo, e ricostruirle dal database a
/// ogni riapertura le farebbe divergere da quel che l'utente ha davanti se una
/// scrittura fosse fallita.
///
/// **E la posizione.** Il motore nuovo nasce senza niente aperto, ma dove la
/// musica si era fermata l'ha segnato il filo dell'orologio nell'istante in cui
/// il dispositivo è sparito — vedi [`annota_ripresa`](super::annota_ripresa) —
/// e quel segno vive fuori dal lucchetto del lettore proprio per sopravvivere a
/// questa sostituzione. [`riprendi_dov_era`], in fondo, riapre quel brano a
/// quel millisecondo: una canzone di venti minuti staccata al quattordicesimo
/// non ricomincia da capo, e riparte se stava andando.
///
/// Non sopravvive l'ascolto in corso, ed è giusto: il conteggio misura quel che
/// si è **sentito**, e i minuti passati con le cuffie staccate non li ha
/// sentiti nessuno.
///
/// Funziona anche quando il motore non si è **mai** aperto, che è il caso più
/// utile: l'applicazione avviata senza scheda audio è un catalogo consultabile
/// finché qualcuno non attacca le cuffie, e prima di oggi l'unico modo di
/// accorgersene era riavviare.
pub(super) fn riapri_su(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    uscita: Option<Option<String>>,
) -> Result<(), AppError> {
    // La bandiera si alza **prima** di tutto e si abbassa comunque vada.
    // Aprire un dispositivo può prendersi dei secondi, e il sorvegliante bussa
    // ogni due: senza questa riga due aperture si sovrapporrebbero, e due
    // motori vivi sulla stessa scheda sono la stessa musica suonata due volte
    // sfasata.
    if stato
        .riapertura_in_corso
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return Ok(());
    }
    let esito = riapri_davvero(app, stato, uscita);
    stato
        .riapertura_in_corso
        .store(false, std::sync::atomic::Ordering::Release);
    esito
}

/// Il corpo di [`riapri_su`], dentro la bandiera che ne impedisce due insieme.
///
/// Separata solo per questo: con il `?` sparso dentro, riabbassare la bandiera
/// su ogni uscita anticipata vorrebbe dire ricordarsene ogni volta — e la volta
/// che ci si dimentica il lettore non riapre mai più.
fn riapri_davvero(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    uscita: Option<Option<String>>,
) -> Result<(), AppError> {
    // Su cosa aprire. Il lucchetto della preferenza si prende e si lascia in
    // una riga: è il clone di una stringa corta, e nessuno lo tiene mentre
    // parla col sistema.
    let voluta = uscita.unwrap_or_else(|| {
        stato
            .uscita_voluta
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    });
    // Il motore nuovo si apre **prima** di buttare via quello vecchio: se
    // l'apertura fallisce, quel che c'era resta dov'è. Un riaprire fallito che
    // lascia il lettore peggio di come l'ha trovato è la cosa che un tasto
    // «riprova» non deve mai fare.
    //
    // E si apre **prima di chiedere il lucchetto del lettore**, non dopo.
    // Aprire un dispositivo audio parla con il sistema e può prendersi
    // secondi: con quel lucchetto già in mano, pausa, volume e «prossimo»
    // resterebbero tutti in coda dietro la riapertura. `(async)` toglie di
    // mezzo il filo principale, non il lucchetto.
    let motore = apri_motore(app, voluta)?;
    // Il formato si legge **prima** che il motore entri nel lettore: subito
    // dopo è dietro il lucchetto, e serve a [`riprendi_dov_era`] che gira
    // quando il lucchetto è già stato lasciato. Sono due interi `Copy`.
    let formato = motore.formato();

    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    match guardia.as_mut() {
        Ok(lettore) => {
            // L'ascolto in corso si lascia cadere senza scriverlo: misura
            // quanto si è **sentito**, e i minuti con le cuffie staccate non
            // li ha sentiti nessuno. La posizione invece torna, e a rimetterla
            // è [`riprendi_dov_era`] qui sotto.
            lettore.ascolto = None;
            lettore.motore = motore;
            lettore
                .motore
                .volume(lettore.volume.volume, lettore.volume.muto);
            lettore
                .motore
                .equalizzatore(&lettore.eq.guadagni, lettore.eq.attivo);
            lettore.motore.replaygain(
                lettore.normalizzazione.attivo,
                lettore.normalizzazione.bersaglio_db,
            );
            // Anche lo spettro: il lettore dentro il motore nuovo è nuovo pure
            // lui, e senza queste due righe chi stava guardando le barre le
            // vedrebbe fermarsi a zero dopo aver riaperto il dispositivo.
            lettore
                .motore
                .guarda_spettro(stato.spettro.load(std::sync::atomic::Ordering::Relaxed));
            lettore.motore.spettro_dettaglio(
                stato
                    .spettro_bande
                    .load(std::sync::atomic::Ordering::Relaxed),
            );
        }
        Err(_) => {
            // Il caso del motore mai aperto: si costruisce il lettore adesso, e
            // la coda di ieri la rimette `riprendi_coda` — che è la stessa
            // funzione dell'avvio, non una sua copia.
            *guardia = Ok(Lettore {
                motore,
                coda: Queue::new(),
                ascolto: None,
                volume: Volume::default(),
                eq: Equalizzazione::default(),
                normalizzazione: Normalizzazione::default(),
                // Come volume, curva e normalizzazione qui sopra: valori di
                // partenza che `riprendi_coda`, due righe più giù, rimpiazza
                // con quel che c'è scritto in `settings`.
                autoplay: false,
                dissolvenza_s: 0,
                motivo_prossimo: None,
            });
            drop(guardia);
            riprendi_coda(app);
            let stato_lettore = app.state::<StatoLettore>();
            return con_lettore(&stato_lettore, |lettore| {
                manda_stato(app, lettore);
                Ok(())
            });
        }
    }

    if let Ok(lettore) = guardia.as_mut() {
        manda_stato(app, lettore);
    }
    // Il lucchetto si lascia **prima** di riaprire il brano, e non a fine
    // funzione: [`brano_di`] parla col disco o con la rete, e la regola in
    // testa al modulo dice che nessun lucchetto si tiene mentre si aspetta.
    drop(guardia);

    riprendi_dov_era(app, stato, formato);
    Ok(())
}

/// Rimette la puntina dove il dispositivo l'aveva lasciata.
///
/// La chiama [`riapri_su`] a dispositivo già riaperto, e ripercorre la strada
/// di [`riprova_corrente`]: il brano annotato, il millisecondo annotato, la
/// coda che deve ancora puntarci.
///
/// # Se riparte o resta ferma
///
/// **Come stava**: il bit lo ha scritto
/// [`annota_ripresa`](super::annota_ripresa) nell'istante del guasto, e dice se
/// la musica stava andando. Qui c'era scritto «sempre in pausa», con la ragione
/// della riunione: riaprire portava per forza sul predefinito di sistema, che
/// dopo un cavo staccato sono gli altoparlanti. Quella ragione è caduta con
/// [`scegli_dispositivo_audio`](super::uscite::scegli_dispositivo_audio) — chi
/// non vuole che il suono si sposti fissa la sua scheda — e resta l'altra metà,
/// che vale ancora: chi aveva messo in **pausa** e poi ha staccato le cuffie
/// non ha chiesto niente, e ritrovare la musica in corso sarebbe un comando che
/// nessuno ha dato.
///
/// # Perché non restituisce niente
///
/// Perché il suo fallimento non è quello di chi l'ha chiamata. Il dispositivo a
/// questo punto è **già** riaperto: se il brano non si riapre — la chiavetta
/// staccata nel frattempo, il NAS ancora giù — il lettore resta pronto e fermo,
/// che è comunque meglio di com'era un istante prima. Farlo risalire fino al
/// comando direbbe a chi ha premuto «Riapri» che non ha funzionato niente, e
/// non sarebbe vero. È la regola scritta in testa a [`riapri_audio`]: non
/// peggiorare quel che c'era.
///
/// L'annotazione, in quel caso, **resta**: come per «Riprova» della rete, si
/// può premere di nuovo.
fn riprendi_dov_era(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    formato: aether_play::FormatoUscita,
) {
    let annotato = *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(ripresa) = annotato else {
        // Nessun segno: o il dispositivo se n'è andato a lettore fermo, o non
        // si era mai aperto. Non c'è un punto a cui tornare, e inventarne uno
        // vorrebbe dire far partire un brano che nessuno stava ascoltando.
        return;
    };

    // Prima di riaprire il file, e non dopo: aprire un brano vuol dire far
    // riconoscere il contenitore a symphonia, cioè far rileggere proprio i byte
    // su cui il filo è caduto. La guardia sta davanti al punto in cui il danno
    // si rifà, non davanti a quello in cui si sente.
    //
    // L'annotazione si butta, perché non c'è più niente a cui tornare: la coda
    // resta dov'è e può andare avanti, invece di essere riportata indietro su
    // questo brano ogni volta che il motore si ricostruisce. Il tasto
    // «Riprova» resta la strada di chi vuole insistere lo stesso — quella
    // passa da [`riprova_corrente`], ed è una scelta di chi ascolta.
    if !si_puo_rimettere_la_puntina(ripresa.track_id) {
        nota!(
            "[riproduzione] posizione non ripresa brano={} \
             causa=aveva fatto cadere il filo della decodifica",
            ripresa.track_id
        );
        *stato
            .ripresa
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        return;
    }

    let brano = match brano_di(app, ripresa.track_id, formato) {
        Ok(brano) => brano,
        Err(err) => {
            nota!(
                "[riproduzione] posizione non ripresa dopo la riapertura codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            return;
        }
    };

    let esito = con_lettore(stato, |lettore| {
        // La coda ha voltato pagina mentre il dispositivo era via: riportare
        // di forza il brano di prima sarebbe una musica che riparte da sola,
        // ed è la stessa rinuncia di [`riprova_corrente`].
        if lettore.coda.current() != Some(ripresa.track_id) {
            return Ok(());
        }
        lettore.motore.suona(brano);
        lettore.motore.vai_a(ripresa.ms);
        // La pausa **dopo** `suona`, non prima: `suona` abbassa il bit da sé
        // (vedi `Motore::pausa` per l'ordine), quindi non rialzarlo qui vuol
        // dire ripartire — ed è il ramo normale, da quando la riapertura è
        // automatica. Il ramo fermo serve a chi era in pausa: per lui `suona`
        // avrebbe appena acceso una musica che non aveva chiesto.
        if !ripresa.suonava {
            lettore.motore.pausa();
        }
        prepara_prossimo(app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()` direbbe
        // zero. Stessa ragione di [`riprova_corrente`].
        manda_stato_con_posizione(app, lettore, ripresa.ms);
        Ok(())
    });
    if esito.is_err() {
        return;
    }
    nota!(
        "[riproduzione] posizione ripresa dopo la riapertura ms={} suonando={}",
        ripresa.ms,
        ripresa.suonava
    );

    // Consumata. Si cancella qui e non prima, come in [`riprova_corrente`]:
    // arrivare fin qui vuol dire o che la puntina è tornata nel solco, o che la
    // coda ha voltato pagina — e in tutti e due i casi non c'è più niente da
    // riprendere.
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Riprende il brano che una cartella di rete aveva interrotto.
///
/// È il tasto «Riprova» dell'avviso di rete. Riapre **lo stesso** brano e
/// torna al millisecondo a cui la musica si era fermata, che è tutto il punto:
/// una condivisione che sparisce a metà del secondo movimento non deve
/// costare il secondo movimento.
///
/// # Cosa succede se la rete è ancora giù
///
/// L'apertura fallisce di nuovo, con lo stesso errore ritentabile, e
/// l'annotazione **resta**: si può premere «Riprova» quante volte si vuole,
/// fino a quando il NAS si riaccende. Si cancella solo quando ha funzionato.
///
/// # Cosa succede se nel frattempo si è ascoltato altro
///
/// Niente. Se la coda non punta più a quel brano, chi ascolta ha già voltato
/// pagina e riportarcelo di forza sarebbe una musica che riparte da sola.
/// L'annotazione si butta e basta.
///
/// `(async)`: riapre un file che può stare su una cartella di rete, e quella è
/// esattamente l'attesa da cui la finestra va tenuta fuori.
#[tauri::command(async)]
pub fn riprova_corrente(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    let annotato = *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(ripresa) = annotato else {
        return Ok(());
    };

    // Il formato dell'uscita si legge sotto lucchetto e il lucchetto si lascia
    // subito: sono due interi da una struttura `Copy`, non un'apertura di file.
    let formato = con_lettore(&stato, |lettore| Ok(lettore.motore.formato())).map_err(errore)?;

    // Il brano si apre **prima di chiedere il lucchetto**, per la stessa
    // ragione scritta in [`riapri_audio`]: fin qui l'apertura ha già mostrato
    // di poterci mettere dei secondi, ed è la ragione per cui esiste questo
    // comando.
    let brano = brano_di(&app, ripresa.track_id, formato).map_err(errore)?;

    con_lettore(&stato, |lettore| {
        if lettore.coda.current() != Some(ripresa.track_id) {
            return Ok(());
        }
        lettore.motore.suona(brano);
        lettore.motore.vai_a(ripresa.ms);
        prepara_prossimo(&app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()`
        // direbbe zero. Stessa ragione di [`vai_a`].
        manda_stato_con_posizione(&app, lettore, ripresa.ms);
        Ok(())
    })
    .map_err(errore)?;

    // Si cancella qui e non prima: se l'apertura fosse fallita saremmo usciti
    // sopra con il `?`, e l'annotazione sarebbe rimasta per il tentativo dopo.
    // Arrivare fin qui vuol dire o che la musica è ripartita, o che la coda ha
    // voltato pagina: in tutti e due i casi non c'è più niente da riprendere.
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    Ok(())
}
