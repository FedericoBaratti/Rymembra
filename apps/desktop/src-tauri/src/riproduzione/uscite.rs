//! Le uscite audio: quali ci sono, quale è aperta, quale si vuole.
//!
//! Il filo che rilegge l'elenco — perché guardare sia l'unico rilevamento
//! possibile sta scritto su [`avvia_sorveglianza`] — la traduzione dell'elenco
//! per la finestra, e i due comandi con cui la schermata delle impostazioni
//! chiede e sceglie.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui.

use aether_app::playback;
use serde::Serialize;

use crate::spegnimento::Emette as _;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, con_libreria};

use super::riapertura::riapri_su;
use super::{
    ATTESA_MASSIMA_DISPOSITIVI, PASSO_DISPOSITIVI, StatoLettore, annota_ripresa, uscita_scelta,
};

/// Un'uscita audio, come la vede la finestra.
///
/// # Perché `presente` e non «l'elenco è la verità»
///
/// Perché l'elenco dice cosa c'è **adesso**, e la schermata deve poter mostrare
/// anche quel che è stato scelto e non c'è più: un DAC staccato che sparisce
/// dalla lista lascia la scelta apparentemente su «predefinito di sistema»,
/// cioè racconta che la preferenza è stata dimenticata quando invece è ancora
/// scritta e tornerà buona appena si riattacca il cavo. Un rigo spento dice la
/// verità; una riga tolta ne dice un'altra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispositivoAudio {
    /// L'identità, che è il nome cpal. Vedi `aether_play::dispositivi`.
    pub id: String,
    /// Come chiamarlo a schermo.
    pub nome: String,
    /// È quello che il sistema usa di suo.
    pub predefinito: bool,
    /// C'è adesso. Falso solo per la scelta rimasta scritta di un cavo
    /// staccato, che l'elenco porta lo stesso.
    pub presente: bool,
    /// È quello da cui sta uscendo il suono in questo momento.
    pub attivo: bool,
    /// È quello che l'utente ha chiesto, per nome.
    ///
    /// # Perché non si ricava da `attivo` e `predefinito`
    ///
    /// Perché i tre non coincidono, e i casi in cui divergono sono proprio
    /// quelli che contano. Chi ha fissato la scheda che *è anche* la
    /// predefinita di sistema ha fatto una scelta diversa da chi ha lasciato
    /// «segui il sistema», e dall'esterno le due si vedono identiche: stessa
    /// riga attiva, stessa riga predefinita. La differenza sta in
    /// `player.output`, che è di qua, e mandarla è una riga — mentre indovinarla
    /// di là è una riga che sbaglia il giorno in cui qualcuno fissa la sua
    /// scheda abituale.
    pub scelto: bool,
}

/// Avvia il filo che si accorge da solo dei dispositivi che vanno e vengono.
///
/// # Perché guardare, invece di farsi avvisare
///
/// Perché non c'è nessuno che avvisi. `cpal` 0.15 espone l'enumerazione e
/// basta: `HostTrait` non ha nessun gancio per i cambi di dispositivo, e
/// l'unico modo di saperlo dal sistema sarebbe `IMMNotificationClient`, cioè
/// implementare un'interfaccia COM, cioè `unsafe` — che il workspace vieta.
/// Rileggere l'elenco e confrontarlo con quello di prima **è** il rilevamento.
///
/// # Perché un filo suo e non quello dell'orologio
///
/// Perché l'orologio muove il cursore quattro volte al secondo, e
/// un'enumerazione WASAPI è una chiamata di sistema che con un driver che si
/// comporta male può prendersi centinaia di millisecondi. Un cursore che si
/// impunta a ogni passata sarebbe un difetto peggiore di quelli che questo filo
/// è qui per riparare.
///
/// # Perché un canale e non un `sleep`
///
/// Perché chi apre le impostazioni deve vedere l'elenco di **adesso**, non
/// quello di due secondi fa: [`dispositivi_audio`] pungola questo filo, che
/// rilegge subito. È la stessa forma di `crate::aggiornamenti::avvia_filo`.
pub fn avvia_sorveglianza(app: tauri::AppHandle, orecchio: std::sync::mpsc::Receiver<()>) {
    let avviato = std::thread::Builder::new()
        .name("aether-dispositivi".to_owned())
        .spawn(move || {
            // L'elenco dell'ultima passata. Locale al filo: non lo guarda
            // nessun altro, e il confronto con questo è tutto il rilevamento.
            //
            // Parte **vuoto** e non con l'elenco di adesso, così la prima
            // passata annuncia quel che c'è: la finestra che si è appena
            // aperta non deve aspettare che qualcosa cambi per sapere quali
            // uscite esistono.
            let mut visti: Vec<aether_play::Dispositivo> = Vec::new();
            let mut attesa = PASSO_DISPOSITIVI;
            loop {
                // Prima di qualunque cosa che tocchi l'`AppHandle`: se la
                // finestra si sta chiudendo, un `emit` da qui fa panicare il
                // ciclo degli eventi. Vedi `crate::spegnimento`.
                if crate::spegnimento::in_uscita() {
                    return;
                }
                attesa = match passata_dispositivi(&app, &mut visti) {
                    Passo::Riposo => PASSO_DISPOSITIVI,
                    // Il raddoppio, non un'attesa fissa: riaprire ha appena
                    // parlato col driver e non ha funzionato, e insistere allo
                    // stesso ritmo vuol dire insistere per ore.
                    Passo::Insisti => attesa.saturating_mul(2).min(ATTESA_MASSIMA_DISPOSITIVI),
                };
                match orecchio.recv_timeout(attesa) {
                    // L'estremo da cui si manda è morto insieme allo stato: non
                    // c'è più niente da sorvegliare.
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    // Una pungolata azzera l'attesa cresciuta: chi ha appena
                    // aperto le impostazioni non deve pagare i tentativi
                    // falliti di mezz'ora fa.
                    Ok(()) => attesa = PASSO_DISPOSITIVI,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
    if let Err(err) = avviato {
        nota!("[avvio] il filo dei dispositivi audio non è partito: {err}");
    }
}

/// Come si aspetta prima della prossima passata.
enum Passo {
    /// Tutto a posto: si ricomincia al ritmo normale.
    Riposo,
    /// C'è qualcosa che non è riuscito: si allunga l'attesa.
    Insisti,
}

/// Una passata del sorvegliante. Non fallisce mai rumorosamente.
fn passata_dispositivi(app: &tauri::AppHandle, visti: &mut Vec<aether_play::Dispositivo>) -> Passo {
    // `try_state` e non `state`: `state` panica se lo stato non c'è, e questo
    // filo gira anche mentre lo stato viene lasciato cadere.
    let Some(stato) = app.try_state::<StatoLettore>() else {
        return Passo::Riposo;
    };

    let adesso = aether_play::dispositivi::elenco();
    if adesso != *visti {
        visti.clone_from(&adesso);
        app.emetti(
            "riproduzione:dispositivi",
            elenco_per_finestra(&stato, &adesso),
        );
    }

    // Nessuna uscita al mondo: non c'è niente su cui riaprire, e chiederlo di
    // nuovo fra due secondi per trent’anni non serve a nessuno. La fascia rossa
    // resta dov'è, ed è la risposta giusta.
    if adesso.is_empty() {
        return Passo::Insisti;
    }

    let Some(perche) = da_riaprire(&stato, &adesso) else {
        return Passo::Riposo;
    };
    nota!("[riproduzione] riapertura automatica: {perche}");
    // Dov'era la musica, segnato **adesso**: il motore che sta per nascere non
    // saprà niente di quel che questo stava suonando. Nel caso del dispositivo
    // sparito l'orologio l'ha già fatto un istante fa e questa è una seconda
    // scrittura dello stesso valore — i fotogrammi non avanzano più, la
    // posizione è ferma — ma nel caso del predefinito **cambiato** non l'ha
    // fatto nessuno: lì `cpal` non segnala niente, e senza questa riga la
    // musica ripartirebbe da capo.
    annota_ripresa(&stato);
    if let Err(err) = riapri_su(app, &stato, None) {
        nota!(
            "[riproduzione] riapertura automatica fallita codice={} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
        return Passo::Insisti;
    }

    // Ha funzionato — ma ha funzionato **come si voleva**? La domanda non è
    // pedanteria: se il motore si fosse riaperto su un'uscita diversa da quella
    // che `da_riaprire` chiedeva, alla prossima passata la richiesta sarebbe
    // identica, e identica quella dopo. Sarebbero riaperture riuscite ogni due
    // secondi — cioè il brano che riparte da capo ogni due secondi, che è un
    // guasto molto peggiore di quello che si stava riparando.
    //
    // L'elenco si rilegge invece di riusare `adesso`: fra le due letture c'è
    // un'apertura di dispositivo, che si è presa il suo tempo, e confrontare
    // con la fotografia di prima direbbe «non converge» ogni volta che qualcuno
    // stacca un cavo proprio in quell'istante.
    if let Some(ancora) = da_riaprire(&stato, &aether_play::dispositivi::elenco()) {
        nota!("[riproduzione] la riapertura non ha portato dove doveva: {ancora}");
        return Passo::Insisti;
    }
    Passo::Riposo
}

/// Perché varrebbe la pena riaprire, se ne vale la pena.
///
/// # La regola, in una riga
///
/// Si riapre quando **quel che si aprirebbe adesso non è quel che è aperto**.
/// Il «quel che si aprirebbe adesso» è `dispositivi::scegli`, la stessa
/// funzione che decide all'apertura: usarne una seconda, scritta apposta per il
/// confronto, vorrebbe dire due idee di quale sia l'uscita giusta e un lettore
/// che riapre in cerchio perché le due non si mettono d'accordo.
///
/// Da quella riga sola escono tutti e quattro i casi:
///
/// - il DAC scelto è tornato → `scegli` lo sceglie, il motore è sul ripiego;
/// - il DAC scelto è stato staccato → `scegli` ripiega, il motore è ancora lì;
/// - nessuna preferenza e il predefinito di sistema è cambiato → `scegli` dà
///   quello nuovo. **È il caso che prima passava inosservato**: `cpal` non
///   invalida il flusso quando cambia il predefinito, quindi nessun
///   `StreamError`, quindi nessun guasto da vedere;
/// - nessuna preferenza e non è cambiato niente → i due nomi coincidono, e non
///   si fa niente.
///
/// Il guasto vero — `perso` alzato dalla callback — resta un ramo suo, perché
/// lì il nome del dispositivo non è cambiato affatto: è il dispositivo a non
/// funzionare più.
fn da_riaprire(stato: &StatoLettore, adesso: &[aether_play::Dispositivo]) -> Option<String> {
    let voluta = stato
        .uscita_voluta
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let guardia = stato
        .lettore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Ok(lettore) = guardia.as_ref() else {
        // Il motore non si è mai aperto — avvio senza scheda audio — e adesso
        // una scheda c'è. È il caso che prima costringeva a riavviare.
        return Some("il motore non si era mai aperto".to_owned());
    };
    if lettore.motore.dispositivo_perso() {
        // «Il motore» e non «l'uscita»: la bandiera che si legge qui la alzano
        // in due — la callback quando il dispositivo sparisce, e il filo della
        // decodifica quando cade. Nel secondo caso l'uscita funziona benissimo,
        // e scrivere nel diario che ha smesso manderebbe chi lo legge a
        // cercare un cavo che non c'entra. Quale dei due sia lo dice
        // `causa_perdita`, che finisce nella riga di `fili.rs`.
        return Some(format!(
            "il motore su «{}» non suona più",
            lettore.motore.dispositivo()
        ));
    }
    let giusta = aether_play::dispositivi::scegli(adesso, voluta.as_deref())?;
    (giusta.id != lettore.motore.dispositivo()).then(|| {
        format!(
            "si suonava su «{}», adesso tocca a «{}»",
            lettore.motore.dispositivo(),
            giusta.id
        )
    })
}

/// L'elenco come lo vuole la finestra.
///
/// Non è `adesso` tradotto riga per riga: ci si aggiunge la scelta che non c'è
/// più. Un DAC staccato che sparisse dall'elenco lascerebbe la schermata a
/// mostrare «predefinito di sistema» selezionato, cioè a raccontare che la
/// preferenza è stata dimenticata quando invece è ancora scritta e tornerà buona
/// appena si riattacca il cavo. Un rigo spento dice la verità.
fn elenco_per_finestra(
    stato: &StatoLettore,
    adesso: &[aether_play::Dispositivo],
) -> Vec<DispositivoAudio> {
    let voluta = stato
        .uscita_voluta
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    // Il lucchetto del lettore si prende e si lascia subito: è la lettura di
    // una stringa corta, non un'apertura di file.
    let aperta = {
        let guardia = stato
            .lettore
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guardia
            .as_ref()
            .ok()
            .map(|lettore| lettore.motore.dispositivo().to_owned())
    };

    let mut righe: Vec<DispositivoAudio> = adesso
        .iter()
        .map(|d| DispositivoAudio {
            attivo: aperta.as_deref() == Some(d.id.as_str()),
            scelto: voluta.as_deref() == Some(d.id.as_str()),
            id: d.id.clone(),
            nome: d.nome.clone(),
            predefinito: d.predefinito,
            presente: true,
        })
        .collect();

    if let Some(scelta) = voluta
        && !righe.iter().any(|r| r.id == scelta)
    {
        righe.push(DispositivoAudio {
            nome: scelta.clone(),
            id: scelta,
            predefinito: false,
            presente: false,
            attivo: false,
            // Scelto eppure assente: è tutta la ragione per cui questa riga
            // esiste. Senza, il pallino tornerebbe su «predefinito di sistema»
            // e la schermata direbbe che la preferenza è stata dimenticata.
            scelto: true,
        });
    }
    righe
}

// ── i comandi delle uscite ──────────────────────────────────────────────────

/// Le uscite audio che ci sono, più quella scelta che non c'è.
///
/// Serve al primo disegno della schermata: dopo, l'elenco arriva da sé
/// sull'evento `riproduzione:dispositivi`, e non c'è nessun tasto «aggiorna» da
/// premere. La chiamata pungola anche il sorvegliante, così se qualcosa era
/// cambiato nell'ultimo paio di secondi lo si vede subito invece che al
/// prossimo giro.
///
/// `(async)`: enumerare i dispositivi parla col sistema audio. Sul filo
/// principale sarebbe la finestra ferma.
#[tauri::command(async)]
pub fn dispositivi_audio(stato: State<'_, StatoLettore>) -> Esito<Vec<DispositivoAudio>> {
    // Se il filo è morto la pungolata si perde, e va bene: l'elenco qui sotto
    // lo si legge comunque: è il filo a servire i **prossimi** cambiamenti, non
    // questa risposta.
    let _ = stato.sveglia_dispositivi.send(());
    Ok(elenco_per_finestra(
        &stato,
        &aether_play::dispositivi::elenco(),
    ))
}

/// Sceglie da quale uscita far sentire Aether. `None`: quella di sistema.
///
/// # Cosa succede subito
///
/// Il dispositivo si riapre **adesso**, senza aspettare il sorvegliante: chi ha
/// appena cliccato su una riga deve sentire il cambiamento, non scoprirlo due
/// secondi dopo. E riparte come stava — vedi `riapertura::riprendi_dov_era` —
/// così cambiare uscita a metà di una canzone non costa la canzone.
///
/// # Perché la preferenza si rilegge dal database
///
/// Perché la copia in memoria non deve poter dire una cosa diversa da quel che
/// c'è scritto sul disco. Si scrive, si rilegge, e la copia prende il valore
/// riletto: se la scrittura fallisse, la copia resterebbe quella di prima
/// invece di raccontare una preferenza che al prossimo avvio non ci sarà.
///
/// `(async)`: scrive sul database e apre un dispositivo audio, due cose che si
/// prendono il loro tempo.
#[tauri::command(async)]
pub fn scegli_dispositivo_audio(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    id: Option<String>,
) -> Esito<()> {
    let libreria = app.state::<Stato>();
    con_libreria(&libreria, |db| {
        playback::save_uscita(&db.connection, id.as_deref())
    })
    .map_err(errore)?;
    let scelta = uscita_scelta(&app);
    *stato
        .uscita_voluta
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = scelta.clone();

    // `Some(scelta)` e non `None`: la copia in memoria è appena stata scritta,
    // ma passarla per esteso toglie di mezzo la domanda «e se nel frattempo
    // qualcun altro l'avesse cambiata», che su una preferenza che l'utente
    // sta cliccando adesso avrebbe una risposta sola sbagliata.
    riapri_su(&app, &stato, Some(scelta)).map_err(errore)?;

    // E l'elenco alla finestra, con il segno di «attivo» spostato: senza
    // questa riga il pallino resterebbe sulla riga di prima fino alla prossima
    // passata del sorvegliante.
    app.emetti(
        "riproduzione:dispositivi",
        elenco_per_finestra(&stato, &aether_play::dispositivi::elenco()),
    );
    Ok(())
}
