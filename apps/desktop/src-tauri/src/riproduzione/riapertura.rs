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
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::queue::Queue;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::spegnimento::Emette as _;

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
pub(super) fn e_la_ripresa_dell_ascolto(lettore: &Lettore, track_id: i64) -> bool {
    let segnato = ASCOLTO_DA_CONTINUARE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    segnato == Some(track_id)
        && lettore
            .ascolto
            .as_ref()
            .is_some_and(|ascolto| ascolto.track_id() == track_id)
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
/// `(async)`: aprire un dispositivo audio parla con il sistema e può prendersi
/// il suo tempo. Sul filo principale sarebbe la finestra ferma.
#[tauri::command(async)]
pub fn riapri_audio(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
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
    if stato
        .riapertura_in_corso
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return Ok(Riaperta::Saltata);
    }
    let esito = riapri_davvero(app, stato, uscita, modo);
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
    // resterebbero tutti in coda dietro la riapertura. `(async)` toglie di
    // mezzo il filo principale, non il lucchetto.
    let motore = apri_motore(app, voluta.clone())?;
    // Il formato si legge **prima** che il motore entri nel lettore: subito
    // dopo è dietro il lucchetto, e serve a [`riprendi_dov_era`] che gira
    // quando il lucchetto è già stato lasciato. Sono due interi `Copy`.
    let formato = motore.formato();
    // La riga gemella di `riga_di_avvio_lettore`: le cuffie Bluetooth si
    // collegano quasi sempre a programma già aperto, e senza questa il diario
    // direbbe soltanto com'era l'uscita all'avvio.
    nota!(
        "[riproduzione] audio riaperto uscita={} frequenza={} canali={} bluetooth={}",
        motore.dispositivo(),
        formato.frequenza,
        formato.canali,
        if motore.su_bluetooth() { "sì" } else { "no" }
    );

    let mut guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    match guardia.as_mut() {
        Ok(lettore) => {
            // Il sorvegliante voleva l'uscita fissata, e `cpal` ha aperto
            // un'altra cosa: la scheda è sparita fra il controllo di
            // `da_riaprire` e l'apertura, e `costruisci` ha ripiegato sul
            // predefinito. Per un gesto a mano va bene; per il sorvegliante no —
            // è proprio il salto agli altoparlanti che l'attesa esiste per
            // evitare. Il motore nuovo nasce in pausa e non ha suonato niente:
            // lasciarlo cadere qui non si sente. Il sorvegliante insiste una
            // volta, e alla passata dopo `da_riaprire` vede che la fissata non
            // c'è e aspetta.
            if modo == Modo::Automatica
                && let Some(fissata) = voluta.as_deref()
                && fissata != motore.dispositivo()
            {
                let causa = format!(
                    "l'uscita fissata «{fissata}» non c'è più, e si aspetta che torni invece di \
                     suonare su «{}»",
                    motore.dispositivo()
                );
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
            lettore.annuncio_atteso = false;
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
            lettore
                .motore
                .dissolvenza(lettore.dissolvenza_s.saturating_mul(1000));
            lettore.motore.latenza(lettore.latenza_ms);
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
                latenza_ms: 0,
                motivo_prossimo: None,
                coda_di_prima: None,
                coda_curata: false,
                coda_del_giro: None,
                in_canna: None,
                non_aperto: None,
                annuncio_atteso: false,
                puntina_a_vuoto: None,
            });
            drop(guardia);
            riprendi_coda(app);
            let stato_lettore = app.state::<StatoLettore>();
            return con_lettore(&stato_lettore, |lettore| {
                manda_stato(app, lettore);
                Ok(Riaperta::Vuota)
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

    if modo == Modo::SoloDispositivo {
        return Ok(Riaperta::Vuota);
    }
    if riprendi_dov_era(app, stato, formato, modo == Modo::Suona) {
        Ok(Riaperta::Rimessa)
    } else {
        Ok(Riaperta::Vuota)
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
    formato: aether_play::FormatoUscita,
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
        lettore.suona(brano);
        lettore.motore.vai_a(ripresa.ms);
        // L'annuncio d'inizio che il motore nuovo manderà per questo brano è
        // la ripresa dell'ascolto di prima, non un ascolto nuovo. Scritto qui,
        // col lucchetto del lettore in mano: l'annuncio ha bisogno dello
        // stesso lucchetto, e non può leggerlo prima.
        *ASCOLTO_DA_CONTINUARE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ripresa.track_id);
        // La pausa **dopo** `suona`, non prima: `suona` abbassa il bit da sé
        // (vedi `Motore::pausa` per l'ordine), quindi non rialzarlo qui vuol
        // dire ripartire — ed è il ramo normale, da quando la riapertura è
        // automatica. Il ramo fermo serve a chi era in pausa — o a chi ha
        // aspettato le cuffie troppo a lungo: per lui `suona` avrebbe appena
        // acceso una musica che non aveva chiesto.
        if !riparte {
            lettore.motore.pausa();
        }
        prepara_prossimo(app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()` direbbe
        // zero. Stessa ragione di [`riprova_corrente`].
        manda_stato_con_posizione(app, lettore, ripresa.ms);
        Ok(true)
    });
    let Ok(rimessa) = esito else {
        return false;
    };
    if rimessa {
        nota!(
            "[riproduzione] posizione ripresa dopo la riapertura ms={} suonando={}",
            ripresa.ms,
            riparte
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
/// `(async)`: riapre un file che può stare su una cartella di rete, e quella è
/// esattamente l'attesa da cui la finestra va tenuta fuori.
#[tauri::command(async)]
pub fn riprova_corrente(app: tauri::AppHandle, stato: State<'_, StatoLettore>) -> Esito<()> {
    let annotato = *stato
        .ripresa
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Il formato dell'uscita si legge sotto lucchetto e il lucchetto si lascia
    // subito: sono due interi da una struttura `Copy`, non un'apertura di file.
    let (formato, perso, con_brano) = con_lettore(&stato, |lettore| {
        Ok((
            lettore.motore.formato(),
            lettore.motore.dispositivo_perso(),
            lettore.motore.posizione().track_id.is_some(),
        ))
    })
    .map_err(errore)?;
    let ripresa = match annotato {
        Some(ripresa) if !perso => ripresa,
        _ if perso || !con_brano => return super::riprendi(app, stato),
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
    let Ok(aprendo) = super::Aprendo::prendi(&stato.aprendo, ripresa.track_id) else {
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
        lettore.suona(brano);
        lettore.motore.vai_a(ripresa.ms);
        prepara_prossimo(&app, lettore);
        // Con la posizione richiesta e non con quella del motore: il salto lo
        // fa il filo della decodifica, e fino ad allora `posizione()`
        // direbbe zero. Stessa ragione di [`vai_a`].
        manda_stato_con_posizione(&app, lettore, ripresa.ms);
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
