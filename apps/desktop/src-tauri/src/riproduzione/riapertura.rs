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

use rymembra_app::playback::{Equalizzazione, Normalizzazione, Volume};
use rymembra_domain::errors::{AppError, ErrorCode, ErrorCodeKind};
use rymembra_domain::queue::Queue;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::nota;
use crate::spegnimento::Emette as _;

use super::coda::{leggi_il_salvato, riga_delle_latenze, rimetti_il_salvato, riprendi_coda};
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
/// `rymembra_play::motore::fili_caduti`, che cresce quando il filo cade.
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
    let caduti = rymembra_play::motore::fili_caduti();
    let mut guardia = ULTIMA_RIPRESA
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (si_puo, ultima) = decidi_ripresa(*guardia, track_id, caduti);
    *guardia = ultima;
    si_puo
}

/// La regola di [`si_puo_rimettere_la_puntina`], senza la casella e senza il
/// contatore da cui vengono i numeri: la risposta, e quel che la casella deve
/// tenere dopo.
///
/// Due cose che sbagliate non si vedono finché non fanno danno, ed è per
/// questo che sta a sé e ha le sue prove. Il confronto è **stretto**: lo stesso
/// brano ripreso due volte senza che nessun filo sia caduto in mezzo — due
/// stacchi del Bluetooth nella stessa canzone — si riprende ancora. E
/// l'annotazione si scrive **solo** quando si riprende: annotata anche al
/// rifiuto, il confronto dopo vedrebbe sempre lo stesso numero e direbbe di sì,
/// e il ciclo del file che fa cadere il decodificatore tornerebbe.
fn decidi_ripresa(
    ultima: Option<(i64, u32)>,
    track_id: i64,
    caduti: u32,
) -> (bool, Option<(i64, u32)>) {
    if let Some((brano, allora)) = ultima
        && brano == track_id
        && caduti > allora
    {
        return (false, ultima);
    }
    (true, Some((track_id, caduti)))
}

/// Il brano il cui ascolto deve **continuare** al prossimo annuncio d'inizio.
///
/// # Il difetto che chiude
///
/// Riaprire il dispositivo buttava l'ascolto in corso: passare alle cuffie
/// Bluetooth a metà di una canzone costava il suo conteggio e il suo scrobble,
/// perché la metà prima del cambio se ne andava senza essere scritta e quella
/// dopo, da sola, di solito non arrivava alla soglia. Eppure è sempre la stessa
/// canzone ascoltata da capo a fondo: il cavo non l'ha sentita nessuno, la
/// musica sì.
///
/// L'ascolto adesso resta nel lettore attraverso la riapertura — fermo mentre
/// il motore non suona — e quel che serve è che il motore nuovo, annunciando
/// l'inizio del brano rimesso nel solco, non lo chiuda per aprirne uno nuovo.
/// Questa casella dice a [`su_evento`](super::su_evento) che quell'annuncio è
/// una ripresa e non un inizio: vedi [`e_la_ripresa_dell_ascolto`].
///
/// # Perché una casella di modulo
///
/// Per la stessa ragione di [`ULTIMA_RIPRESA`]: fra chi la scrive e chi la
/// legge c'è un motore buttato e uno costruito, e uno è il lettore. Si scrive
/// con il lucchetto del lettore in mano, subito dopo aver rimesso la puntina,
/// e l'annuncio che la legge ha bisogno dello stesso lucchetto: non può
/// arrivare prima.
static ASCOLTO_DA_CONTINUARE: std::sync::Mutex<Option<i64>> = std::sync::Mutex::new(None);

/// L'annuncio d'inizio di questo brano è la ripresa dell'ascolto in corso?
///
/// `true` una volta sola, per il primo annuncio dopo che [`riprendi_dov_era`]
/// ha rimesso la puntina su `track_id`, e solo se l'ascolto che il lettore
/// tiene è proprio di quel brano. Chi riceve `true` **non** chiude l'ascolto e
/// non ne comincia uno nuovo: lo fa ripartire, o lo lascia fermo se il motore è
/// in pausa. Chi riceve `false` fa quel che faceva sempre.
///
/// Il segno si consuma comunque, anche quando la risposta è no: se nel
/// frattempo è partito un altro brano, l'annuncio è suo e la ripresa non c'è
/// più.
///
/// `conta` è la riga che l'ascolto misura — vedi `ospiti::da_contare` — e
/// non per forza l'id annunciato: in sessione il motore annuncia la maniglia
/// della voce, e l'ascolto in corso è della riga che quella voce apre.
pub(super) fn e_la_ripresa_dell_ascolto(
    lettore: &Lettore,
    track_id: i64,
    conta: Option<i64>,
) -> bool {
    let segnato = ASCOLTO_DA_CONTINUARE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    segnato == Some(track_id)
        && conta.is_some_and(|riga| {
            lettore
                .ascolto
                .as_ref()
                .is_some_and(|ascolto| ascolto.track_id() == riga)
        })
}

/// Il prossimo annuncio d'inizio di `track_id` continua l'ascolto in corso
/// invece di chiuderlo: vedi [`ASCOLTO_DA_CONTINUARE`].
///
/// Serve anche alla sessione, che riapre il brano corrente sotto un altro
/// nome — la maniglia della sua voce, entrando come host, o la voce di prima,
/// spegnendo la calibrazione — senza che chi ascolta abbia cambiato brano.
/// Si chiama col lucchetto del lettore in mano, subito dopo aver dato il
/// brano al motore, per la ragione scritta sulla casella.
pub(super) fn continua_l_ascolto(track_id: i64) {
    *ASCOLTO_DA_CONTINUARE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(track_id);
}

/// Dimentica un ascolto da continuare rimasto in sospeso: entrando o uscendo
/// da una sessione, il brano a cui si riferiva non c'è più sotto quel nome.
pub(super) fn dimentica_l_ascolto_da_continuare() {
    *ASCOLTO_DA_CONTINUARE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Ferma l'ascolto in corso, se il motore non suona più.
///
/// Un ascolto misura il tempo che passa mentre la musica va, e con il
/// dispositivo sparito la musica non va: senza questa pausa, un DAC staccato
/// per dieci minuti e poi riattaccato avrebbe regalato dieci minuti di silenzio
/// al conteggio della canzone che suonava — adesso che l'ascolto sopravvive
/// alla riapertura invece di essere buttato. La chiama il sorvegliante dei
/// dispositivi a ogni passata, anche quando non c'è nessuna uscita su cui
/// riaprire; `ListenTracker::pause` chiamata due volte non accumula due volte.
pub(super) fn ferma_l_ascolto_se_perso(stato: &StatoLettore) {
    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Ok(lettore) = guardia.as_mut()
        && lettore.motore.dispositivo_perso()
        && let Some(ascolto) = lettore.ascolto.as_mut()
    {
        ascolto.pause(crate::stato::adesso_ms());
    }
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
/// «predefinito» ha chiesto proprio di seguire il sistema, e la musica va dove
/// va Windows; la finestra dice **su quale uscita** è finito il suono.
///
/// Fino alla 3.0 la promessa era scritta qui ma non mantenuta: la scheda fissata
/// che spariva faceva ripiegare la riapertura automatica sul predefinito, cioè
/// proprio sugli altoparlanti. Adesso il sorvegliante **aspetta** che torni —
/// vedi `uscite::da_riaprire` e [`Modo::Automatica`] — e la finestra dice quale
/// si sta aspettando. Questo tasto invece ripiega: chi lo preme ha chiesto di
/// sentire adesso, da dove si può.
///
/// Resta vera la ragione tecnica scritta in `uscita.rs`: riaprire può
/// bloccarsi, e non si può fare dal filo che segnala il guasto — quello è un
/// `StreamError` che arriva da dentro `cpal`.
///
/// Fuori dal filo della finestra: aprire un dispositivo audio parla con il
/// sistema e può prendersi il suo tempo. Sul filo principale sarebbe la
/// finestra ferma, su un worker del runtime gli altri comandi: il comando
/// [`riapri_audio`] lo manda sul pool bloccante.
pub fn riapri_audio_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    // A mano si riparte come si stava: se la musica andava, riprende; se era
    // in pausa, resta ferma. La stessa regola dell'automatico — l'ha decisa
    // `annota_ripresa`, non chi chiama. A differenza dell'automatico, a mano si
    // ripiega: chi preme «Riapri» mentre aspetta le cuffie fissate ha chiesto
    // di sentire adesso, da dove si può. Vedi [`Modo`].
    riapri_su(&app, &stato, None, Modo::AMano)
        .map(|_| ())
        .map_err(errore)
}

/// Chi chiede di riaprire, che vuol dire: quanto si può ripiegare, e cosa fare
/// dopo.
///
/// # Perché il ripiego dipende da chi chiede
///
/// Perché l'uscita fissata che sparisce — le cuffie Bluetooth che si staccano —
/// adesso **si aspetta**: il suono non passa agli altoparlanti dell'ufficio da
/// solo, ed è la ragione per cui la si fissa. Ma l'attesa è del sorvegliante,
/// non di chi preme un tasto: chi preme «Riapri» o play mentre le cuffie sono
/// via ha chiesto di sentire adesso, e lo si accontenta da dove si può.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Modo {
    /// Il sorvegliante dei dispositivi: sull'uscita fissata o su niente, e la
    /// musica riparte come stava.
    Automatica,
    /// «Riapri» e la scelta di un'uscita nelle impostazioni: si ripiega se la
    /// fissata non c'è, e la musica riparte come stava.
    AMano,
    /// Play premuto a dispositivo perso: si ripiega, e la musica riparte
    /// **suonando** — il gesto vince sul bit segnato al momento del guasto.
    Suona,
    /// Un brano sta per partire da sé: serve soltanto un dispositivo vivo, e
    /// non c'è nessun punto da rimettere.
    SoloDispositivo,
}

/// Com'è andata una riapertura riuscita.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Riaperta {
    /// Ce n'era già una in corso: questa non ha fatto niente.
    Saltata,
    /// Il motore è nuovo e la puntina è tornata nel solco del brano di prima.
    Rimessa,
    /// Il motore è nuovo, e non c'era niente da rimettere al suo posto.
    Vuota,
}

/// [`riapri_audio_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
#[tauri::command]
pub async fn riapri_audio(app: tauri::AppHandle) -> Esito<()> {
    super::col_lettore_in_disparte(app, "riapertura dell'audio", riapri_audio_ora).await
}

/// [`riprova_corrente_ora`], sul pool bloccante: vedi
/// `super::col_lettore_in_disparte`.
#[tauri::command]
pub async fn riprova_corrente(app: tauri::AppHandle) -> Esito<()> {
    super::col_lettore_in_disparte(app, "riprova", riprova_corrente_ora).await
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
/// **E l'ascolto in corso.** Fino alla 2.x qui si buttava, con la ragione che
/// il conteggio misura quel che si è **sentito** e i minuti con le cuffie
/// staccate non li ha sentiti nessuno. La ragione è giusta, la cura no: buttare
/// l'ascolto toglieva anche quel che si era sentito **prima** del cambio, e
/// passare alle cuffie a metà canzone costava il conteggio della canzone.
/// Adesso l'ascolto si ferma — il tempo col dispositivo via non conta — e
/// riparte quando il motore nuovo annuncia il brano rimesso nel solco: vedi
/// [`ASCOLTO_DA_CONTINUARE`].
///
/// Funziona anche quando il motore non si è **mai** aperto, che è il caso più
/// utile: l'applicazione avviata senza scheda audio è un catalogo consultabile
/// finché qualcuno non attacca le cuffie, e prima di oggi l'unico modo di
/// accorgersene era riavviare.
pub(super) fn riapri_su(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    uscita: Option<Option<String>>,
    modo: Modo,
) -> Result<Riaperta, AppError> {
    // La bandiera si alza **prima** di tutto e si abbassa comunque vada.
    // Aprire un dispositivo può prendersi dei secondi, e il sorvegliante bussa
    // ogni due: senza questa riga due aperture si sovrapporrebbero, e due
    // motori vivi sulla stessa scheda sono la stessa musica suonata due volte
    // sfasata.
    let Some(_riaprendo) = Riaprendo::prendi(&stato.riapertura_in_corso) else {
        return Ok(Riaperta::Saltata);
    };
    riapri_davvero(app, stato, uscita, modo)
}

/// Tiene alzata [`StatoLettore::riapertura_in_corso`] finché vive, e la
/// abbassa comunque finisca la riapertura — riuscita, fallita, o caduta a metà.
///
/// # Il difetto che chiude
///
/// La bandiera si abbassava con una riga dopo la chiamata, e un panico dentro
/// [`riapri_davvero`] — un driver che risponde una cosa che `cpal` non si
/// aspettava, una scheda letta a metà — la saltava: la bandiera restava alzata
/// **per sempre**. Da lì ogni riapertura tornava `Saltata` senza fare niente:
/// il sorvegliante non rimetteva più in piedi il motore, «Riapri» non
/// rispondeva, e ogni play a dispositivo perso aspettava i cinque secondi di
/// `aspetta_la_riapertura` prima di arrendersi. Il sorvegliante oggi il panico
/// lo raccoglie e va avanti; la bandiera, finché non si abbassava da sola, no.
///
/// La stessa forma di [`super::Aprendo`], per la stessa ragione: un `Drop` si
/// esegue anche durante l'unwind, una riga dopo la chiamata no.
struct Riaprendo<'a> {
    bandiera: &'a std::sync::atomic::AtomicBool,
}

impl<'a> Riaprendo<'a> {
    /// Alza la bandiera, se non era già alzata. `None` se c'è già una
    /// riapertura in corso: chi chiama non ha niente da fare.
    fn prendi(bandiera: &'a std::sync::atomic::AtomicBool) -> Option<Self> {
        (!bandiera.swap(true, std::sync::atomic::Ordering::AcqRel)).then_some(Self { bandiera })
    }
}

impl Drop for Riaprendo<'_> {
    fn drop(&mut self) {
        self.bandiera
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Il corpo di [`riapri_su`], dentro la bandiera che ne impedisce due insieme.
///
/// Separata perché la bandiera la tiene [`Riaprendo`] da fuori: con il `?`
/// sparso qui dentro, e con un panico possibile in fondo a ogni chiamata al
/// driver, l'unico posto in cui abbassarla con certezza è un `Drop`.
fn riapri_davvero(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    uscita: Option<Option<String>>,
    modo: Modo,
) -> Result<Riaperta, AppError> {
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
    // resterebbero tutti in coda dietro la riapertura. Il pool bloccante del
    // comando toglie di mezzo il filo principale, non il lucchetto.
    let motore = apri_con_un_secondo_tentativo(app, voluta.as_deref())?;
    // Il formato si legge **prima** che il motore entri nel lettore: subito
    // dopo è dietro il lucchetto, e serve a [`riprendi_dov_era`] che gira
    // quando il lucchetto è già stato lasciato. Sono due interi `Copy`.
    let formato = motore.formato();
    // La riga gemella di `riga_di_avvio_lettore`: le cuffie Bluetooth si
    // collegano quasi sempre a programma già aperto, e senza questa il diario
    // direbbe soltanto com'era l'uscita all'avvio.
    //
    // `predefinito` dice se l'uscita aperta era quella di sistema in
    // quell'istante: è la domanda che il diario non sapeva risolvere davanti a
    // una riapertura finita su un'uscita virtuale — l'aveva scelta Windows, o
    // il nostro ripiego?
    nota!(
        "[riproduzione] audio riaperto uscita={} frequenza={} canali={} bluetooth={} predefinito={}",
        motore.dispositivo(),
        formato.frequenza,
        formato.canali,
        if motore.su_bluetooth() { "sì" } else { "no" },
        if motore.era_predefinito() {
            "sì"
        } else {
            "no"
        }
    );

    // Il lettore mai aperto — l'avvio senza scheda audio — nasce qui, e nasce
    // **completo**: coda, volume, curva e il resto si leggono adesso, a lettore
    // lasciato, e il lettore si pubblica dopo con dentro quel che c'è scritto.
    // Pubblicarlo vuoto e riempirlo un attimo dopo, com'era, lasciava una
    // finestra — larga quanto una scansione che tiene la libreria — in cui chi
    // premeva play o muoveva il volume lo faceva sul lettore vuoto, e la coda e
    // il volume salvati arrivavano dopo a cancellare il suo gesto.
    //
    // Due istruzioni e non una: la guardia della domanda deve cadere **prima**
    // della lettura, che aspetta la libreria.
    let mai_aperto = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_err();
    let salvato = mai_aperto.then(|| leggi_il_salvato(app));
    // Le latenze per uscita, per la stessa ragione e nello stesso punto: dopo
    // l'apertura — il motore nuovo sa già su quale uscita suona — e prima del
    // lucchetto del lettore. Il lettore mai aperto non ne ha bisogno: le ha
    // già lette `leggi_il_salvato`, e le applica `rimetti_il_salvato`.
    let latenze = if mai_aperto {
        None
    } else {
        leggi_le_latenze(app)
    };

    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    match guardia.as_mut() {
        Ok(lettore) => {
            // Il motore nuovo nasce in pausa e non ha suonato niente:
            // lasciarlo cadere qui non si sente. Vedi [`rifiuto_automatico`].
            if let Some(causa) = rifiuto_automatico(
                modo,
                voluta.as_deref(),
                motore.dispositivo(),
                motore.era_predefinito(),
            ) {
                drop(guardia);
                drop(motore);
                return Err(AppError::new(ErrorCode::PlaybackDeviceLost).with_cause(causa));
            }
            // Un cambio voluto — un'uscita scelta nelle impostazioni, «Riapri»
            // a dispositivo sano — non passa da nessun guasto, e nessuno ha
            // segnato dove fosse la puntina: [`riprendi_dov_era`] non trovava
            // niente a cui tornare, e il brano si perdeva a metà. Lo si segna
            // adesso dal motore vecchio, che fino alla riga dopo sta suonando.
            //
            // Un segno già scritto vince solo se parla dello stesso brano a
            // musica ferma: è quello dell'istante del guasto, o il punto di
            // «Riprova» di un flusso caduto, e un motore fermato da un guasto
            // racconta una posizione che non vale più. A musica in corso la
            // posizione viva è quella giusta per definizione.
            //
            // Non quando sta per partire un brano: chi ha chiamato lo apre lui
            // subito dopo, e un segno scritto qui riporterebbe indietro quello
            // di prima.
            let viva = lettore.motore.posizione();
            let segnato = stato
                .ripresa
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .map(|ripresa| ripresa.track_id);
            if modo != Modo::SoloDispositivo
                && viva.track_id.is_some()
                && (!viva.in_pausa || segnato != viva.track_id)
            {
                let _ = super::annota_ripresa_da(stato, viva);
            }
            // L'ascolto in corso **resta**, fermo: il tempo che passa da qui
            // all'annuncio del motore nuovo non l'ha sentito nessuno, e quel
            // che si è sentito prima del cambio non deve andare perso. Lo fa
            // ripartire l'annuncio del brano rimesso nel solco — vedi
            // [`ASCOLTO_DA_CONTINUARE`] — e se la puntina non torna al suo
            // posto lo chiude chi volta pagina, come ogni altro ascolto.
            if let Some(ascolto) = lettore.ascolto.as_mut() {
                ascolto.pause(crate::stato::adesso_ms());
            }
            lettore.motore = motore;
            // Il motore nuovo non ha niente in canna.
            lettore.in_canna = None;
            lettore.non_aperto = None;
            lettore.annuncio_atteso = None;
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
            // La dissolvenza e la latenza: due manopole che il `Lettore` ricorda
            // e il motore nuovo no. Senza queste due righe, staccare le cuffie
            // spegneva la dissolvenza in silenzio — il cursore delle
            // impostazioni continuava a dire «sei secondi» e il passaggio fra due
            // brani tornava secco — e azzerava la compensazione di latenza,
            // rimettendo i testi fuori fase esattamente quando si era appena
            // cambiata uscita.
            //
            // Quella che vale adesso: in sessione è zero, qualunque cosa dica
            // la preferenza. Vedi `insieme::dissolvenza_ms`.
            lettore
                .motore
                .dissolvenza(super::insieme::dissolvenza_ms(lettore));
            // La latenza non è più una sola: è quella dell'uscita su cui il
            // motore nuovo si è **davvero** aperto, o la globale se quell'uscita
            // un numero suo non ce l'ha. Passare dalle casse alle cuffie
            // Bluetooth porta con sé il numero delle cuffie. Senza una lettura
            // riuscita — la libreria occupata oltre l'attesa — resta quella di
            // prima, che è esattamente com'era fino a questa versione. La copia
            // nel lettore segue, e `manda_stato` qui sotto porta il cursore
            // sul numero nuovo.
            let latenza = latenze
                .as_ref()
                .map_or(lettore.latenza_ms, |(globale, voci)| {
                    rymembra_app::playback::latenza_effettiva(
                        *globale,
                        voci,
                        Some(lettore.motore.dispositivo()),
                    )
                });
            lettore.motore.latenza(latenza);
            lettore.latenza_ms = latenza;
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
            // E la sessione, se ce n'è una: il motore nuovo nasce col flusso
            // libero di riposare e senza correzioni, e chi ospita o segue ha
            // bisogno del primo e non vuole le seconde del motore di prima.
            super::insieme::motore_sostituito(app, lettore);
        }
        Err(_) => {
            // Il caso del motore mai aperto: si costruisce il lettore adesso, e
            // la coda di ieri la rimettono le stesse funzioni di
            // `riprendi_coda` — non una loro copia — prima che il lettore si
            // veda: vedi il commento su `mai_aperto`, più su.
            let mut nuovo = Lettore {
                motore,
                coda: Queue::new(),
                ascolto: None,
                volume: Volume::default(),
                eq: Equalizzazione::default(),
                normalizzazione: Normalizzazione::default(),
                // Come volume, curva e normalizzazione qui sopra: valori di
                // partenza che `rimetti_il_salvato`, due righe più giù,
                // rimpiazza con quel che c'è scritto in `settings`.
                autoplay: false,
                dissolvenza_s: 0,
                latenza_ms: 0,
                motivo_prossimo: None,
                coda_di_prima: None,
                coda_curata: false,
                coda_del_giro: None,
                pausa_chiesta: (0, false),
                in_canna: None,
                non_aperto: None,
                annuncio_atteso: None,
                puntina_a_vuoto: None,
                // Un lettore mai aperto non è mai entrato in una sessione:
                // entrarci chiede un lettore.
                sessione: None,
                fermato_della_sessione: false,
            };
            let letto = salvato.is_some();
            if let Some(salvato) = salvato {
                rimetti_il_salvato(stato, &mut nuovo, salvato);
            }
            *guardia = Ok(nuovo);
            if !letto {
                // Non succede: il lettore torna `Err` soltanto all'avvio, e
                // questa strada è una sola alla volta. Se succedesse — era
                // aperto quando lo si è chiesto, e non lo è più — la strada di
                // prima: pubblicato, e riempito subito dopo.
                drop(guardia);
                riprendi_coda(app);
                return con_lettore(stato, |lettore| {
                    manda_stato(app, lettore);
                    Ok(Riaperta::Vuota)
                });
            }
            if let Ok(lettore) = guardia.as_ref() {
                manda_stato(app, lettore);
            }
            return Ok(Riaperta::Vuota);
        }
    }

    if let Ok(lettore) = guardia.as_mut() {
        manda_stato(app, lettore);
    }
    // Il lucchetto si lascia **prima** di riaprire il brano, e non a fine
    // funzione: [`brano_di`] parla col disco o con la rete, e la regola in
    // testa al modulo dice che nessun lucchetto si tiene mentre si aspetta.
    drop(guardia);

    if modo == Modo::SoloDispositivo {
        return Ok(Riaperta::Vuota);
    }
    if riprendi_dov_era(app, stato, formato, modo == Modo::Suona) {
        Ok(Riaperta::Rimessa)
    } else {
        Ok(Riaperta::Vuota)
    }
}

/// Quanto la riapertura aspetta la libreria per rileggere le latenze.
///
/// Un secondo, e poi si va avanti con quella di prima. Il motore nuovo è già
/// aperto, e il passaggio alla nuova uscita aspetta questa lettura: una
/// scansione che tiene la libreria non deve poter trattenere le cuffie appena
/// collegate — o la musica di un dispositivo perso — per tutto il suo lotto.
/// Il prezzo di una rinuncia è piccolo e già noto — è il comportamento di
/// prima, un numero solo per tutte le uscite — e lo corregge la riapertura dopo
/// o il primo tocco del cursore.
const ATTESA_LATENZE: std::time::Duration = std::time::Duration::from_secs(1);

/// Il motore appena aperto dal sorvegliante va lasciato cadere? `Some` con la
/// causa da scrivere nell'errore, `None` se va bene così.
///
/// È la seconda rete che tiene le cuffie fissate fuori dagli altoparlanti — la
/// prima è `uscite::perche_riaprire` — e vale per la sola riapertura
/// automatica: un gesto a mano («Riapri», play, un'uscita scelta) ha chiesto di
/// sentire adesso, da dove si può. Una funzione a sé perché si possa provare
/// senza un motore vero: `aperta` è l'uscita su cui il motore nuovo si è
/// aperto, `era_predefinito` se in quell'istante era quella di sistema.
///
/// Due casi:
///
/// - **la fissata non è quella aperta.** Il sorvegliante voleva l'uscita
///   fissata, e `cpal` ha aperto un'altra cosa: la scheda è sparita fra il
///   controllo di `da_riaprire` e l'apertura, e `costruisci` ha ripiegato sul
///   predefinito. Per un gesto a mano va bene; per il sorvegliante no — è
///   proprio il salto agli altoparlanti che l'attesa esiste per evitare. Il
///   sorvegliante insiste una volta, e alla passata dopo `da_riaprire` vede che
///   la fissata non c'è e aspetta.
/// - **nessuna preferenza, e l'aperta non era il predefinito.** Lo stesso per
///   chi segue il sistema: senza una preferenza si apre il predefinito, e se
///   nell'istante dell'apertura nessuna uscita era marcata tale — le cuffie
///   appena sparite, Windows che non ha ancora scelto chi le sostituisce —
///   l'apertura ha preso la prima dell'elenco. Il diario vero la mostrava:
///   «AMD Streaming Audio Device» a 16 kHz, un'uscita virtuale che nessuno
///   ascolta, e due secondi dopo una seconda riapertura verso il predefinito
///   vero. Il salto lo faceva il nostro ripiego, non Windows; qui lo si lascia
///   cadere, e il sorvegliante riprova quando il sistema ha deciso. Vedi
///   `rymembra_play::dispositivi::scegli_per_riaprire`.
fn rifiuto_automatico(
    modo: Modo,
    voluta: Option<&str>,
    aperta: &str,
    era_predefinito: bool,
) -> Option<String> {
    if modo != Modo::Automatica {
        return None;
    }
    match voluta {
        Some(fissata) if fissata != aperta => Some(format!(
            "l'uscita fissata «{fissata}» non c'è più, e si aspetta che torni invece di suonare \
             su «{aperta}»"
        )),
        None if !era_predefinito => Some(format!(
            "nessuna uscita era il predefinito di sistema: si aspetta che Windows ne scelga uno \
             invece di suonare su «{aperta}»"
        )),
        _ => None,
    }
}

/// La latenza globale e quelle per uscita, lette per la riapertura.
///
/// `None` se la libreria non c'è, non risponde, o resta occupata oltre
/// [`ATTESA_LATENZE`]: chi chiama tiene il numero che il lettore aveva già. Si
/// chiama **senza** il lucchetto del lettore, e la riga del diario — quando le
/// voci non si leggono tutte, o la libreria non arriva — si scrive qui, prima
/// di prenderlo.
fn leggi_le_latenze(
    app: &tauri::AppHandle,
) -> Option<(i64, rymembra_app::playback::LatenzePerUscita)> {
    let stato_app = super::libreria_di(app, "latenze per uscita").ok()?;
    let letto = crate::stato::con_libreria_entro(&stato_app, ATTESA_LATENZE, |libreria| {
        let c = &libreria.connection;
        Ok((
            rymembra_app::playback::load_latenza(c)?,
            rymembra_app::playback::load_latenze_per_uscita(c)?,
        ))
    });
    match letto {
        Some(Ok((globale, lette))) => {
            if let Some(riga) = riga_delle_latenze(&lette) {
                nota!("{riga}");
            }
            Some((globale, lette.voci))
        }
        Some(Err(err)) => {
            nota!(
                "[riproduzione] latenze per uscita non rilette alla riapertura codice={}: resta \
                 quella di prima",
                err.code().kind().code()
            );
            None
        }
        None => {
            nota!(
                "[riproduzione] latenze per uscita non rilette: la libreria era occupata da più \
                 di {} ms, resta quella di prima",
                ATTESA_LATENZE.as_millis()
            );
            None
        }
    }
}

/// Quanto si aspetta prima del secondo tentativo d'apertura.
///
/// Un quarto di secondo: quel che serve a un endpoint in transizione — le
/// cuffie Bluetooth che finiscono di negoziare, il predefinito di Windows che
/// si assesta — per smettere di esserlo, e poco accanto ai due secondi del
/// giro del sorvegliante che altrimenti toccherebbe aspettare.
const SECONDO_TENTATIVO: std::time::Duration = std::time::Duration::from_millis(250);

/// Apre il motore, e se il dispositivo era in transizione riprova una volta.
///
/// # Perché qui e non dentro l'apertura
///
/// Perché il diario vero mostrava una riapertura automatica fallita con
/// «Failed to get audio clock» — `AUDCLNT_E_DEVICE_INVALIDATED` a metà
/// apertura, l'endpoint invalidato fra la sua scelta e il suo orologio — e il
/// sorvegliante, a quel punto, raddoppiava l'attesa: quattro secondi di
/// silenzio in più per un guasto che un quarto di secondo dopo non c'era già
/// più. Dentro `rymembra_play` il secondo tentativo non si può fare: l'apertura
/// consuma gli anelli del motore che sta costruendo, e il motore nuovo si
/// costruisce da capo.
///
/// Solo per `playback.deviceLost`, che è come l'apertura dice «endpoint in
/// transizione» (vedi `rymembra_play::uscita::errore_di_apertura`). **Non** per
/// la scadenza dei venti secondi né per gli altri guasti: riprovarli subito
/// vorrebbe dire raddoppiare un'attesa che è già stata lunga, o insistere su
/// qualcosa che fra un quarto di secondo sarà identico.
fn apri_con_un_secondo_tentativo(
    app: &tauri::AppHandle,
    voluta: Option<&str>,
) -> Result<rymembra_play::Motore, AppError> {
    match apri_motore(app, voluta.map(str::to_owned)) {
        Err(err) if err.code().kind() == ErrorCodeKind::PlaybackDeviceLost => {
            nota!(
                "[riproduzione] dispositivo in transizione, si riprova fra {} ms: {}",
                SECONDO_TENTATIVO.as_millis(),
                err.cause().unwrap_or("—")
            );
            std::thread::sleep(SECONDO_TENTATIVO);
            apri_motore(app, voluta.map(str::to_owned))
        }
        esito => esito,
    }
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
/// Con due eccezioni. `suona` vero — play premuto a dispositivo perso — vince
/// sul bit: il gesto è più recente del guasto. E un segno più vecchio di
/// [`RIPRESA_FRESCA`](super::RIPRESA_FRESCA) non riparte da solo anche se la
/// musica stava andando: l'uscita fissata si aspetta, e aspettarla può voler
/// dire fino al mattino dopo.
///
/// # Perché non restituisce un errore
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
///
/// Dice soltanto se la puntina è tornata nel solco: a play premuto a
/// dispositivo perso serve sapere se c'è ancora un brano da far partire.
fn riprendi_dov_era(
    app: &tauri::AppHandle,
    stato: &StatoLettore,
    formato: rymembra_play::FormatoUscita,
    suona: bool,
) -> bool {
    let annotato = *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(ripresa) = annotato else {
        // Nessun segno: o il dispositivo se n'è andato a lettore fermo, o non
        // si era mai aperto. Non c'è un punto a cui tornare, e inventarne uno
        // vorrebbe dire far partire un brano che nessuno stava ascoltando.
        return false;
    };
    let fresca =
        crate::stato::adesso_ms().saturating_sub(ripresa.quando_ms) < super::RIPRESA_FRESCA;
    let riparte = suona || (ripresa.suonava && fresca);

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
        return false;
    }

    // Il numero dell'ultima pausa o ripresa chiesta, prima di aprire: vedi
    // [`super::consegna`].
    let Ok(gesti_prima) = con_lettore(stato, |lettore| Ok(lettore.pausa_chiesta.0)) else {
        return false;
    };
    let brano = match brano_di(app, ripresa.track_id, formato) {
        Ok(brano) => brano,
        Err(err) => {
            nota!(
                "[riproduzione] posizione non ripresa dopo la riapertura codice={} causa={}",
                err.code().kind().code(),
                err.cause().unwrap_or("—")
            );
            return false;
        }
    };

    let esito = con_lettore(stato, |lettore| {
        // La coda ha voltato pagina mentre il dispositivo era via: riportare
        // di forza il brano di prima sarebbe una musica che riparte da sola,
        // ed è la stessa rinuncia di [`riprova_corrente`].
        if lettore.coda.current() != Some(ripresa.track_id) {
            return Ok(false);
        }
        // Fermo o suonando, dal punto annotato, in un comando solo: vedi
        // [`super::consegna`]. Il ramo fermo serve a chi era in pausa — o a
        // chi ha aspettato le cuffie troppo a lungo: per lui partire vorrebbe
        // dire una musica che non aveva chiesto.
        let da_ms = super::consegna(lettore, brano, !riparte, ripresa.ms, gesti_prima);
        // L'annuncio d'inizio che il motore nuovo manderà per questo brano è
        // la ripresa dell'ascolto di prima, non un ascolto nuovo. Scritto qui,
        // col lucchetto del lettore in mano: l'annuncio ha bisogno dello
        // stesso lucchetto, e non può leggerlo prima.
        *ASCOLTO_DA_CONTINUARE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ripresa.track_id);
        prepara_prossimo(app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()` direbbe
        // zero. Stessa ragione di [`riprova_corrente`].
        manda_stato_con_posizione(app, lettore, da_ms);
        Ok(true)
    });
    let Ok(rimessa) = esito else {
        return false;
    };
    if rimessa {
        // I tre ingressi della decisione, e non solo l'esito: `suonando=no` da
        // solo non diceva se la musica era ferma già al guasto (una pausa, o
        // le cuffie che mandano «pausa» staccandosi), o se il segno era più
        // vecchio di [`super::RIPRESA_FRESCA`]. Sono due cause con due rimedi.
        nota!(
            "[riproduzione] posizione ripresa dopo la riapertura ms={} suonando={} \
             suonava={} fresca={} play={}",
            ripresa.ms,
            riparte,
            ripresa.suonava,
            fresca,
            suona
        );
    }

    // Consumata. Si cancella qui e non prima, come in [`riprova_corrente`]:
    // arrivare fin qui vuol dire o che la puntina è tornata nel solco, o che la
    // coda ha voltato pagina — e in tutti e due i casi non c'è più niente da
    // riprendere.
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    rimessa
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
/// # Cosa succede se non c'è niente di annotato
///
/// Il «Riprova» compare anche per il brano **dopo** che il preparatore non è
/// riuscito ad aprire: lì non si è interrotto niente, e un punto a cui tornare
/// non c'è. Premuto, non faceva niente. Adesso, se il motore ha ancora in mano
/// il brano corrente, si rimanda il preparatore su quello dopo; se si è già
/// fermato — il corrente è finito e la coda si è spostata sul brano che non si
/// apriva — è un play, e lo fa [`super::riprendi`].
///
/// # A dispositivo perso
///
/// Prima il dispositivo, poi il brano: è un play anche questo, e
/// [`super::riprendi`] sa già fare tutte e due le cose — riaprire l'uscita e
/// rimettere la puntina sul segno annotato, suonando.
///
/// # In una sessione, da chi segue
///
/// I due «è un play» qui sopra non valgono: chi segue non suona da sé, e
/// [`super::riprendi`] manderebbe la ripresa a chi ospita — togliendo la pausa
/// a tutti per un guasto di un PC solo, o tornando con un «non concesso» a chi
/// è entrato per ascoltare. Si rifà l'uscita di qui se è lei che manca, come
/// [`riapri_audio`], e nient'altro: il brano lo rimette il regolatore, che
/// riapre da sé la voce di chi ospita quando qui non suona niente.
///
/// Fuori dal filo della finestra: riapre un file che può stare su una cartella
/// di rete, e quella è esattamente l'attesa da cui la finestra — e i worker
/// del runtime — vanno tenuti fuori. Il comando [`riprova_corrente`] lo manda
/// sul pool bloccante.
pub fn riprova_corrente_ora(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    let annotato = *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Il formato dell'uscita si legge sotto lucchetto e il lucchetto si lascia
    // subito: sono due interi da una struttura `Copy`, non un'apertura di file.
    let (formato, perso, con_brano, segue, gesti_prima) = con_lettore(&stato, |lettore| {
        Ok((
            lettore.motore.formato(),
            lettore.motore.dispositivo_perso(),
            lettore.motore.posizione().track_id.is_some(),
            super::insieme::segue(lettore),
            lettore.pausa_chiesta.0,
        ))
    })
    .map_err(errore)?;
    let ripresa = match annotato {
        Some(ripresa) if !perso => ripresa,
        // Chi segue: vedi «In una sessione» qui sopra.
        _ if segue => {
            return if perso {
                riapri_su(&app, &stato, None, Modo::AMano)
                    .map(|_| ())
                    .map_err(errore)
            } else {
                Ok(())
            };
        }
        _ if perso || !con_brano => return super::riprendi_ora(app, stato),
        _ => {
            return con_lettore(&stato, |lettore| {
                prepara_prossimo(&app, lettore);
                Ok(())
            })
            .map_err(errore);
        }
    };

    // Il brano si apre **prima di chiedere il lucchetto**, per la stessa
    // ragione scritta in [`riapri_audio`]: fin qui l'apertura ha già mostrato
    // di poterci mettere dei secondi, ed è la ragione per cui esiste questo
    // comando. Una sola apertura per brano, come in `avvia_corrente`: tre
    // colpi su «Riprova» mentre il NAS si sveglia sono un'attesa, non tre.
    let Ok(mut aprendo) = super::Aprendo::prendi(&stato.aprendo, ripresa.track_id) else {
        return Ok(());
    };
    let brano = match brano_di(&app, ripresa.track_id, formato) {
        Ok(brano) => brano,
        // Ancora giù: l'annotazione resta, e la fascia deve ridire il guasto
        // con il suo codice — risalito al comando come errore, arrivava alla
        // finestra già tradotto, e il «Riprova» spariva proprio al tentativo in
        // cui serviva di nuovo.
        Err(err) if err.is_retryable() => {
            app.emetti("riproduzione:errore", errore(err));
            return Ok(());
        }
        Err(err) => return Err(errore(err)),
    };

    let consegnato = con_lettore(&stato, |lettore| {
        if lettore.coda.current() != Some(ripresa.track_id) {
            return Ok(true);
        }
        // Il motore è cambiato mentre il file si apriva: il brano è stato
        // aperto per un'altra uscita. Lo rifà `avvia_corrente`, qui sotto.
        if lettore.motore.formato() != formato {
            return Ok(false);
        }
        // Suonando, dal punto annotato: vedi [`super::consegna`]. La casella
        // si libera qui, sotto il lucchetto, come in `avvia_corrente`: tenuta
        // fino in fondo alla funzione, un salto chiesto in quell'istante
        // finiva su una puntina che nessuno raccoglieva più.
        let da_ms = super::consegna(lettore, brano, false, ripresa.ms, gesti_prima);
        drop(aprendo.take());
        prepara_prossimo(&app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()`
        // direbbe zero. Stessa ragione di [`vai_a`].
        manda_stato_con_posizione(&app, lettore, da_ms);
        Ok(true)
    })
    .map_err(errore)?;
    if !consegnato {
        // La casella va lasciata prima: `avvia_corrente` la vuole per lo
        // stesso brano, e la troverebbe presa da noi.
        drop(aprendo);
        return super::avvia_corrente(&app, &stato, false, ripresa.ms).map_err(errore);
    }

    // Si cancella qui e non prima: se l'apertura fosse fallita saremmo usciti
    // sopra, e l'annotazione sarebbe rimasta per il tentativo dopo. Arrivare
    // fin qui vuol dire o che la musica è ripartita, o che la coda ha voltato
    // pagina: in tutti e due i casi non c'è più niente da riprendere.
    *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    Ok(())
}

#[cfg(test)]
mod prove {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// La bandiera della riapertura si abbassa anche quando la riapertura cade.
    ///
    /// Era il difetto: abbassata da una riga dopo la chiamata, un panico la
    /// lasciava alzata per sempre, e da lì ogni riapertura tornava «saltata»
    /// senza fare niente.
    #[test]
    fn una_riapertura_caduta_non_lascia_la_bandiera_alzata() {
        let bandiera = AtomicBool::new(false);
        let caduta = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _riaprendo = Riaprendo::prendi(&bandiera).expect("la bandiera era giù");
            assert!(bandiera.load(Ordering::Acquire), "alzata mentre si riapre");
            std::panic::panic_any("il driver ha risposto una cosa inattesa");
        }));
        assert!(caduta.is_err());
        assert!(
            !bandiera.load(Ordering::Acquire),
            "la bandiera è rimasta alzata dopo il panico"
        );
        // E la riapertura dopo si può fare.
        assert!(Riaprendo::prendi(&bandiera).is_some());
    }

    /// Due riaperture insieme no: la seconda trova la bandiera alzata e non fa
    /// niente, e quando la prima finisce la bandiera torna giù.
    #[test]
    fn la_seconda_riapertura_e_saltata() {
        let bandiera = AtomicBool::new(false);
        let prima = Riaprendo::prendi(&bandiera);
        assert!(prima.is_some());
        assert!(Riaprendo::prendi(&bandiera).is_none());
        drop(prima);
        assert!(!bandiera.load(Ordering::Acquire));
    }

    // ── la puntina nel solco: vedi `decidi_ripresa` ──

    /// La prima ripresa si fa, e si annota.
    #[test]
    fn la_prima_ripresa_si_fa() {
        assert_eq!(decidi_ripresa(None, 7, 0), (true, Some((7, 0))));
    }

    /// Due stacchi del Bluetooth nello stesso brano, senza fili caduti in
    /// mezzo: la seconda ripresa si fa ancora. Con un confronto largo (`>=`)
    /// la posizione si buttava e la musica non ripartiva.
    #[test]
    fn lo_stesso_brano_senza_cadute_si_riprende_ancora() {
        assert_eq!(decidi_ripresa(Some((7, 2)), 7, 2), (true, Some((7, 2))));
    }

    /// Il brano su cui l'ultimo tentativo ha fatto cadere il filo non si
    /// rimette nel solco, e l'annotazione resta quella di prima: annotata al
    /// rifiuto, il confronto dopo vedrebbe lo stesso numero e direbbe di sì —
    /// il motore ricostruito ogni due secondi, per sempre.
    #[test]
    fn un_brano_che_ha_fatto_cadere_il_filo_non_si_rimette_nel_solco() {
        assert_eq!(decidi_ripresa(Some((7, 2)), 7, 3), (false, Some((7, 2))));
        // E non si rimette nemmeno al giro dopo: la casella non si è spostata,
        // e il motore ricostruito un'altra volta riceve lo stesso no.
        let (_, dopo) = decidi_ripresa(Some((7, 2)), 7, 3);
        assert!(!decidi_ripresa(dopo, 7, 3).0);
    }

    /// Un altro brano non paga per quello che ha fatto cadere il filo.
    #[test]
    fn un_altro_brano_si_riprende_anche_dopo_una_caduta() {
        assert_eq!(decidi_ripresa(Some((7, 2)), 8, 3), (true, Some((8, 3))));
    }

    // ── il motore che il sorvegliante lascia cadere: vedi `rifiuto_automatico` ──

    /// Il sorvegliante voleva le cuffie fissate e `cpal` ha aperto gli
    /// altoparlanti: il motore nuovo si lascia cadere.
    #[test]
    fn la_fissata_che_non_e_quella_aperta_si_rifiuta() {
        let causa = rifiuto_automatico(Modo::Automatica, Some("Cuffie BT"), "Altoparlanti", true);
        assert!(
            causa
                .as_deref()
                .is_some_and(|c| c.contains("«Cuffie BT»") && c.contains("«Altoparlanti»")),
            "{causa:?}"
        );
    }

    /// La fissata aperta davvero va bene.
    #[test]
    fn la_fissata_aperta_va_bene() {
        assert_eq!(
            rifiuto_automatico(Modo::Automatica, Some("DAC"), "DAC", false),
            None
        );
    }

    /// Chi segue il sistema: un'uscita che non era il predefinito è il nostro
    /// ripiego, non la scelta di Windows. Il predefinito vero va bene.
    #[test]
    fn senza_preferenza_solo_il_predefinito_va_bene() {
        assert!(rifiuto_automatico(Modo::Automatica, None, "AMD Streaming", false).is_some());
        assert_eq!(
            rifiuto_automatico(Modo::Automatica, None, "Altoparlanti", true),
            None
        );
    }

    /// Un gesto a mano ha chiesto di sentire adesso, da dove si può: niente
    /// si rifiuta.
    #[test]
    fn un_gesto_a_mano_non_si_rifiuta_mai() {
        for modo in [Modo::AMano, Modo::Suona, Modo::SoloDispositivo] {
            assert_eq!(
                rifiuto_automatico(modo, Some("Cuffie BT"), "Altoparlanti", true),
                None
            );
            assert_eq!(rifiuto_automatico(modo, None, "AMD Streaming", false), None);
        }
    }
}
