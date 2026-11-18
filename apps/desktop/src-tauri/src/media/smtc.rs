//! Le `SystemMediaTransportControls` di Windows, via `souvlaki`.
//!
//! È la metà di [`super`] che parla col sistema operativo del computer: la
//! scheda nel riquadro del volume, i tasti multimediali della tastiera e delle
//! cuffie. Il perché di ogni scelta sta nel preambolo di [`super`], che la
//! racconta da prima che le metà fossero due.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{OnceLock, PoisonError};
use std::time::Duration;

use rymembra_app::covers::CoverStore;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::Manager as _;

use super::{Gesto, PASSO_SALTO_MS, StatoMedia, avanti_entro, dentro_il_brano, ms_di, premi};
use crate::nota;
use crate::riproduzione::StatoRiproduzione;
use crate::stato::Stato;

/// I due pezzi che stanno insieme o non stanno affatto, e quel che si è già
/// mostrato.
pub(super) struct Controlli {
    /// La maniglia verso il sistema operativo.
    controlli: MediaControls,
    /// Dove stanno le copertine su disco.
    copertine: CoverStore,
    /// La scheda consegnata a Windows l'ultima volta che la consegna è
    /// riuscita. Vedi [`Scheda`].
    mostrata: Option<Scheda>,
    /// L'ultima scheda che Windows ha rifiutato: serve a scriverlo nel diario
    /// una volta sola invece che a ogni stato.
    rifiutata: Option<Scheda>,
    /// La finestra nativa a cui sono appesi i controlli: vedi [`avvia`].
    ///
    /// **Per ultima**, ed è l'ordine che conta: i campi cadono nell'ordine in cui
    /// sono scritti, e il `Drop` di `controlli` chiama `detach` su questa
    /// finestra, che dev'essere ancora viva. Nessuno la legge: la si tiene perché
    /// viva quanto i controlli.
    _finestra: rymembra_barra::FinestraDeiMessaggi,
}

/// Quel che la scheda di Windows mostra, per sapere se è cambiato.
///
/// # Perché si confronta
///
/// Perché consegnare la scheda costa: `souvlaki` riscrive i quattro testi, la
/// durata, e — se riceve una copertina — **apre il file** e aspetta il sistema
/// finché non l'ha letto. Lo stato del lettore invece parte a ogni pausa, a
/// ogni salto, a ogni ritocco della coda, e prima di questo confronto ognuno
/// di loro riapriva la stessa copertina. Adesso la scheda si consegna solo se
/// qualcosa di quel che mostra è cambiato, e la copertina solo se è cambiata
/// lei: a `souvlaki` arriva `None`, e il sistema tiene quella che ha.
#[derive(Clone, PartialEq, Eq)]
struct Scheda {
    track_id: Option<i64>,
    titolo: String,
    artista: String,
    album: String,
    /// L'indirizzo della copertina, già nella forma di [`url_di_file`].
    copertina: Option<String>,
    durata_ms: u64,
}

/// Registra i controlli e collega i tasti.
///
/// # A quale finestra si appendono
///
/// Su Windows le SMTC si appendono a una finestra, e non a quella
/// dell'applicazione: con la X sul vassoio quella si chiude e il motore resta,
/// e con lei i tasti multimediali morirebbero proprio quando chi ascolta si
/// aspetta di poter ancora mettere in pausa dalla tastiera. Si appendono a una
/// finestra **nativa nostra**, mai mostrata, che nasce qui e vive quanto i
/// controlli: vedi `rymembra_barra::FinestraDeiMessaggi`. Va chiamata sul filo
/// della finestra — dove la finestra dei messaggi deve nascere, perché è lì che
/// i messaggi si pompano — e non dipende più da `main`.
///
/// Ogni motivo per cui potrebbe non riuscire — la finestra che non nasce, il
/// sistema che rifiuta — finisce nella stessa riga di registro e lascia
/// l'applicazione intera: un lettore senza scheda nel riquadro del volume è un
/// lettore, un lettore che non si apre no.
pub fn avvia(app: &tauri::AppHandle) {
    let finestra = match rymembra_barra::FinestraDeiMessaggi::crea() {
        Ok(finestra) => finestra,
        Err(err) => {
            nota!(
                "[media] la finestra dei messaggi non nasce, i tasti multimediali restano spenti: {err}"
            );
            return;
        }
    };
    let punt: *mut c_void = finestra.hwnd();

    let config = PlatformConfig {
        // Il nome che il sistema mostra accanto ai comandi. Non tradotto: è
        // il nome del programma, e i nomi dei programmi non si traducono.
        display_name: "Rymembra",
        // Serve solo a MPRIS su Linux, che qui non si compila mai. Sta qui
        // perché il tipo lo pretende, non perché faccia qualcosa.
        dbus_name: "rymembra",
        hwnd: Some(punt),
    };

    let mut controlli = match MediaControls::new(config) {
        Ok(controlli) => controlli,
        Err(err) => {
            nota!("[media] il sistema non concede i controlli: {err:?}");
            return;
        }
    };

    let mano = app.clone();
    if let Err(err) = controlli.attach(move |evento| su_evento(&mano, evento)) {
        nota!("[media] i tasti non si collegano: {err:?}");
        return;
    }

    // Lo `CoverStore` si prende una volta sola, qui: la cartella non cambia
    // per tutta la vita del processo. E si prende **accanto** al mutex, non
    // dentro: questa funzione gira mentre la finestra si costruisce, e mettersi
    // in coda dietro il database per copiare un percorso vorrebbe dire ritardare
    // la comparsa della finestra di quanto dura la prima passata di qualcun
    // altro.
    let stato_app = app.state::<Stato>();
    let Ok(copertine) = stato_app.copertine() else {
        nota!("[media] la libreria non è aperta: niente copertine nella scheda");
        return;
    };

    let stato = app.state::<StatoMedia>();
    let mut dentro = stato
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    *dentro = Some(Controlli {
        controlli,
        copertine,
        mostrata: None,
        rifiutata: None,
        _finestra: finestra,
    });
}

/// Stacca la scheda del sistema, mentre la finestra c'è ancora.
///
/// # Perché non basta lasciarli cadere da soli
///
/// Perché i controlli stanno in uno stato **gestito**, e uno stato gestito
/// viene lasciato cadere alla fine di tutto — dopo che il ciclo degli eventi si
/// è dichiarato distrutto e `DestroyWindow` ha smontato le finestre. Ma questi
/// controlli non sono un oggetto qualunque: su Windows sono agganciati
/// all'`HWND` della finestra dei messaggi, che `souvlaki` si è fatto dare
/// all'avvio (`GetForWindow`), e il loro `Drop` chiama `detach` su quella
/// finestra. Cioè parla a una finestra che non c'è più.
///
/// Chiamata dal `RunEvent` d'uscita in `main`, il distacco succede quando la
/// finestra è ancora al suo posto e la domanda ha ancora una risposta.
/// Lasciarli cadere una seconda volta dopo non fa niente: il posto è già vuoto.
///
/// # L'ordine, e il filo
///
/// Cadono i controlli, **poi** la finestra dei messaggi (`Controlli::_finestra`
/// è l'ultimo campo). E la finestra si distrugge solo sul filo che l'ha creata:
/// la sequenza di spegnimento gira sul filo della finestra per «Esci» e per
/// la chiusura, ma l'aggiornatore la chiama dal suo filo, e lì la finestra
/// dei messaggi non si può distruggere — si chiude con un messaggio, e se il
/// suo filo non pompa se ne va con il processo, che è sull'uscire.
pub fn stacca(app: &tauri::AppHandle) {
    let Some(stato) = app.try_state::<StatoMedia>() else {
        return;
    };
    let mut dentro = stato
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // `take` e non `as_mut().detach()`: il `Drop` di `MediaControls` fa già il
    // distacco, e farlo due volte vorrebbe dire fidarsi di come `souvlaki`
    // tratta il secondo. Portarli via è l'unico modo che non dipende da quello.
    drop(dentro.take());
}

/// La parte di [`super::aggiorna`] che tocca Windows: la scheda e lo stato
/// della riproduzione. Posizione e durata le ha già segnate chi chiama.
pub(super) fn aggiorna(app: &tauri::AppHandle, media: &StatoMedia, stato: &StatoRiproduzione) {
    let mut dentro = media
        .controlli
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(controlli) = dentro.as_mut() else {
        return;
    };

    let scheda = scheda_di(app, stato, &controlli.copertine);
    if controlli.mostrata.as_ref() != Some(&scheda) {
        consegna(controlli, scheda);
    }

    let dove = Some(MediaPosition(Duration::from_millis(stato.posizione_ms)));
    let riproduzione = if stato.brano.is_none() {
        MediaPlayback::Stopped
    } else if stato.in_pausa {
        MediaPlayback::Paused { progress: dove }
    } else {
        MediaPlayback::Playing { progress: dove }
    };
    let _ = controlli.controlli.set_playback(riproduzione);
}

/// Quel che la scheda deve mostrare per questo stato.
///
/// A coda finita la scheda è **vuota**, non assente: quattro stringhe vuote e
/// la copertina di ripiego. `souvlaki` scrive soltanto i campi che riceve, e
/// `MediaMetadata::default()` — che era quel che si mandava — non ne conteneva
/// nessuno: la scheda restava lì a dire il titolo dell'ultimo brano.
fn scheda_di(app: &tauri::AppHandle, stato: &StatoRiproduzione, copertine: &CoverStore) -> Scheda {
    let Some(brano) = stato.brano.as_ref() else {
        return Scheda {
            track_id: None,
            titolo: String::new(),
            artista: String::new(),
            album: String::new(),
            copertina: copertina_di_ripiego(),
            durata_ms: 0,
        };
    };
    let copertina = brano
        .cover_art_hash
        .as_ref()
        .and_then(|hash| file_della_copertina(app, copertine, hash))
        .and_then(|percorso| url_di_file(&percorso))
        // Un brano senza copertina mostra quella di ripiego, e non è un
        // ornamento: senza, la scheda terrebbe la copertina del brano di prima
        // — `souvlaki` non ha un modo di toglierla.
        .or_else(copertina_di_ripiego);
    Scheda {
        track_id: Some(brano.id),
        titolo: brano.title.clone(),
        artista: brano.artist.clone(),
        album: brano.album.clone(),
        copertina,
        durata_ms: stato.durata_ms,
    }
}

/// Il file della copertina di un brano, se c'è.
///
/// Un brano di questo PC la ha nello store, sotto la sua impronta. Un brano
/// che suona da un altro dispositivo ha invece una chiave
/// (`lontano-<dispositivo>-<impronta>`) e la sua copertina sta nella cache
/// delle copertine lontane: dove non c'è ancora la si chiede in disparte, e
/// quando arriva la scheda si ripubblica, con lei. Vedi `lontane`.
fn file_della_copertina(
    app: &tauri::AppHandle,
    copertine: &CoverStore,
    hash: &str,
) -> Option<PathBuf> {
    if crate::lontano::e_una_copertina(hash) {
        return super::lontane::file_di(
            app,
            hash,
            true,
            crate::riproduzione::insieme::aggiorna_finestra,
        );
    }
    Some(copertine.path_for(hash)).filter(|percorso| percorso.exists())
}

/// Consegna la scheda a Windows, e ricorda com'è andata.
///
/// La copertina si passa solo se è cambiata: con `None` `souvlaki` lascia
/// quella che c'è, ed è l'unico modo di non riaprire lo stesso file a ogni
/// pausa.
///
/// # Quando Windows rifiuta
///
/// `souvlaki` consegna la copertina **prima** del resto e, se il sistema la
/// rifiuta, esce senza consegnare niente: un file che non si apre costava
/// titolo, artista e durata. Qui si riprova subito senza copertina, così la
/// scheda almeno dice cosa suona; e la si dà per consegnata, perché riaprire lo
/// stesso file a ogni stato non lo farebbe aprire meglio.
///
/// Ogni rifiuto si scrive nel diario una volta per scheda: prima veniva
/// scartato in silenzio, ed è così che la scheda vuota è rimasta nascosta per
/// tre versioni.
fn consegna(controlli: &mut Controlli, scheda: Scheda) {
    let copertina_nuova = controlli
        .mostrata
        .as_ref()
        .is_none_or(|prima| prima.copertina != scheda.copertina);
    let copertina = if copertina_nuova {
        scheda.copertina.as_deref()
    } else {
        None
    };
    let metti = |controlli: &mut MediaControls, copertina: Option<&str>| {
        controlli.set_metadata(MediaMetadata {
            title: Some(&scheda.titolo),
            album: Some(&scheda.album),
            artist: Some(&scheda.artista),
            cover_url: copertina,
            duration: Some(Duration::from_millis(scheda.durata_ms)),
        })
    };
    let mut esito = metti(&mut controlli.controlli, copertina);
    if let Err(err) = &esito
        && copertina.is_some()
    {
        if controlli.rifiutata.as_ref() != Some(&scheda) {
            nota!(
                "[media] la scheda di Windows non accetta la copertina del brano {:?}: {err:?}",
                scheda.track_id
            );
        }
        esito = metti(&mut controlli.controlli, None);
    }
    match esito {
        Ok(()) => {
            controlli.mostrata = Some(scheda);
            controlli.rifiutata = None;
        }
        Err(err) => {
            if controlli.rifiutata.as_ref() != Some(&scheda) {
                nota!(
                    "[media] la scheda di Windows non accetta il brano {:?}: {err:?}",
                    scheda.track_id
                );
            }
            // Quel che è stato consegnato a metà non si sa: la prossima volta
            // si riparte da zero, copertina compresa.
            controlli.mostrata = None;
            controlli.rifiutata = Some(scheda);
        }
    }
}

/// La copertina per quando non ce n'è una: l'icona di Rymembra.
///
/// # Perché un file nella cartella temporanea
///
/// Perché la scheda di Windows vuole un file vero — vedi [`StatoMedia`] — e
/// l'icona sta dentro l'eseguibile, non accanto. Si scrive una volta per
/// processo, e si riscrive se nel frattempo qualcuno ha pulito la cartella.
/// Nella cartella temporanea e non accanto al database, perché non è un dato:
/// è un'immagine che l'applicazione sa rifare quando vuole.
///
/// `None` se non si riesce a scriverla: la scheda resta con la copertina che
/// aveva, che è quel che succedeva prima.
fn copertina_di_ripiego() -> Option<String> {
    const ICONA: &[u8] = include_bytes!("../../icons/128x128.png");
    static PERCORSO: OnceLock<PathBuf> = OnceLock::new();
    let percorso =
        PERCORSO.get_or_init(|| std::env::temp_dir().join("rymembra-scheda-senza-copertina.png"));
    if !percorso.exists() && rymembra_app::scrittura::scrivi_atomico(percorso, ICONA).is_err() {
        return None;
    }
    url_di_file(percorso)
}

/// L'indirizzo di una copertina, nella forma che `souvlaki` sa aprire.
///
/// # Non è un URL, ed è voluto
///
/// `souvlaki` 0.8.3, su Windows, fa una cosa sola con un indirizzo che
/// comincia per `file://`: **toglie il prefisso** (`trim_start_matches`) e
/// passa quel che resta a `StorageFile::GetFileFromPathAsync`, che vuole un
/// percorso di Windows vero — lettera di unità e barre rovesce. Qui si
/// costruiva un URL da manuale, `file:///C:/…`: tolto il prefisso restava
/// `/C:/…`, che il sistema rifiuta, e con quel rifiuto saltava la scheda
/// intera — vedi il preambolo del modulo.
///
/// La forma giusta è quindi il prefisso seguito dal percorso **così com'è**:
/// `file://C:\Users\…`, che tolto il prefisso torna a essere
/// `C:\Users\…`. Niente codifica percentuale, per la stessa ragione: spazi e
/// lettere accentate devono arrivare al sistema tali e quali.
///
/// Il prefisso `\\?\` dei percorsi lunghi si toglie: `GetFileFromPathAsync`
/// non lo accetta, e per una copertina — che sta in una cartella dell'utente —
/// non serve.
fn url_di_file(percorso: &std::path::Path) -> Option<String> {
    let testo = percorso.to_str()?;
    let testo = if let Some(resto) = testo.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{resto}")
    } else {
        testo.strip_prefix(r"\\?\").unwrap_or(testo).to_owned()
    };
    Some(format!("file://{testo}"))
}

/// Un tasto premuto, o un comando arrivato dalla scheda del sistema.
///
/// Gira sul filo del sistema operativo che ha ricevuto il tasto, e ci resta il
/// meno possibile: traduce il tasto in un [`Gesto`] e lo consegna a [`premi`].
/// I conti dei salti relativi si fanno qui, perché leggono due atomici e basta.
fn su_evento(app: &tauri::AppHandle, evento: MediaControlEvent) {
    let gesto = match evento {
        MediaControlEvent::Play => Gesto::Riprendi,
        MediaControlEvent::Pause => Gesto::Pausa,
        MediaControlEvent::Toggle => Gesto::Alterna,
        MediaControlEvent::Next => Gesto::Prossimo,
        MediaControlEvent::Previous => Gesto::Precedente,
        // `Stop` come pausa, di proposito: fermare davvero butterebbe la
        // posizione, e il tasto stop di una tastiera lo si preme per
        // interrompere, non per perdere il segno.
        MediaControlEvent::Stop => Gesto::Pausa,
        // Anche questo passa dal tetto, che pure arriva già come posizione
        // assoluta: il numero lo sceglie il sistema operativo — una barra
        // trascinata nel riquadro del volume, un'auto collegata in Bluetooth —
        // e la durata che quel sistema crede di avere è quella dell'ultimo
        // [`aggiorna`], che può essere del brano di prima.
        MediaControlEvent::SetPosition(MediaPosition(dove)) => {
            Gesto::VaiA(dentro_il_brano(app, ms_di(dove)))
        }
        // Un «avanti» già dentro l'ultimo tratto del brano non salta: vedi
        // [`avanti_entro`].
        MediaControlEvent::Seek(verso) => match salto(app, verso, PASSO_SALTO_MS) {
            Some(ms) => Gesto::VaiA(ms),
            None => return,
        },
        MediaControlEvent::SeekBy(verso, quanto) => match salto(app, verso, ms_di(quanto)) {
            Some(ms) => Gesto::VaiA(ms),
            None => return,
        },
        // Il volume del sistema non è il volume del lettore, e `OpenUri`
        // chiederebbe di suonare qualcosa che non è in libreria. `Raise` e
        // `Quit` sono gesti della finestra che la scheda non deve avere: da
        // lì si comanda la musica, non il programma.
        MediaControlEvent::SetVolume(_)
        | MediaControlEvent::OpenUri(_)
        | MediaControlEvent::Raise
        | MediaControlEvent::Quit => return,
    };
    premi(app, gesto);
}

/// Dove finisce un salto relativo, senza uscire dal brano. `None`: nessun
/// salto da fare.
///
/// I due versi non sono simmetrici, e non è una svista: indietro il fondo è lo
/// zero, e `saturating_sub` ce lo tiene da sé. Avanti il fondo è la fine del
/// brano, che `saturating_add` non conosce — protegge dal trabocco di un `u64`,
/// cioè da un brano lungo cinquecento milioni di anni, non dalla fine di questo
/// — e la tiene [`avanti_entro`], con un margine: un «avanti di cinque
/// secondi» premuto a tre secondi dalla fine non è una richiesta di cambiare
/// canzone.
fn salto(app: &tauri::AppHandle, verso: SeekDirection, quanto_ms: u64) -> Option<u64> {
    let media = app.state::<StatoMedia>();
    let adesso = media.posizione_ms.load(Ordering::Relaxed);
    match verso {
        SeekDirection::Forward => {
            avanti_entro(adesso, quanto_ms, media.durata_ms.load(Ordering::Relaxed))
        }
        SeekDirection::Backward => Some(adesso.saturating_sub(quanto_ms)),
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Quel che `souvlaki` 0.8.3 fa di un indirizzo `file://` su Windows,
    /// riga per riga (`platform/windows/mod.rs`, `set_metadata`): se comincia
    /// per `file://` toglie il prefisso e passa il resto a
    /// `StorageFile::GetFileFromPathAsync`. `None` se non comincia così — lì
    /// `souvlaki` prende un'altra strada, `Uri::CreateUri`, che per un file
    /// locale non serve a niente.
    fn percorso_che_souvlaki_consegna(url: &str) -> Option<&str> {
        url.starts_with("file://")
            .then(|| url.trim_start_matches("file://"))
    }

    /// Il sorgente di questo file senza le prove.
    fn sorgente() -> &'static str {
        let tutto = include_str!("smtc.rs");
        tutto.split("#[cfg(test)]").next().unwrap_or(tutto)
    }

    /// Le SMTC non dipendono dalla finestra dell'applicazione: con la X sul
    /// vassoio quella si chiude, e i tasti multimediali devono restare.
    #[test]
    fn le_smtc_non_si_appendono_alla_finestra_dell_applicazione() {
        let avvia = sorgente()
            .split("pub fn avvia(")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        assert!(!avvia.is_empty(), "avvia non si trova");
        assert!(
            avvia.contains("FinestraDeiMessaggi::crea()"),
            "le SMTC non nascono sulla finestra dei messaggi"
        );
        assert!(
            !avvia.contains("get_webview_window") && !avvia.contains(".hwnd()?"),
            "avvia chiede ancora la finestra dell'applicazione"
        );
    }

    /// Lo smontaggio: la finestra dei messaggi è **l'ultimo** campo, così cade
    /// dopo i controlli, il cui `Drop` chiama `detach` su di lei.
    #[test]
    fn la_finestra_dei_messaggi_cade_dopo_i_controlli() {
        let struttura = sorgente()
            .split("pub(super) struct Controlli {")
            .nth(1)
            .and_then(|resto| resto.split("\n}\n").next())
            .unwrap_or_default();
        let campi: Vec<&str> = struttura
            .lines()
            .map(str::trim)
            .filter(|riga| !riga.starts_with("//") && riga.contains(": "))
            .collect();
        assert_eq!(
            campi
                .first()
                .map(|riga| riga.split(':').next().unwrap_or("")),
            Some("controlli"),
            "i controlli devono essere il primo campo"
        );
        assert_eq!(
            campi
                .last()
                .map(|riga| riga.split(':').next().unwrap_or("")),
            Some("_finestra"),
            "la finestra dei messaggi dev'essere l'ultimo campo: cade dopo i controlli"
        );
    }

    #[test]
    fn souvlaki_consegna_a_windows_il_percorso_della_copertina_tale_e_quale() {
        // Il difetto che la prova tiene chiuso: `file:///C:/…` diventava
        // `/C:/…`, che Windows rifiuta, e con lui saltava la scheda intera.
        // Quel che arriva al sistema dev'essere il percorso di partenza,
        // lettera di unità e barre rovesce comprese.
        let percorso = r"C:\Users\ada\AppData\Roaming\rymembra\copertine\ab\abcdef.jpg";
        let url = url_di_file(std::path::Path::new(percorso)).expect("un percorso UTF-8");
        assert_eq!(url, format!("file://{percorso}"));
        assert_eq!(percorso_che_souvlaki_consegna(&url), Some(percorso));
    }

    #[test]
    fn il_percorso_consegnato_e_assoluto_per_windows() {
        // `GetFileFromPathAsync` vuole un percorso assoluto: è la proprietà che
        // `/C:/…` perdeva. Su Windows lo sa dire `Path::is_absolute`.
        let url = url_di_file(std::path::Path::new(r"D:\Musica\copertina.jpg"))
            .expect("un percorso UTF-8");
        let consegnato = percorso_che_souvlaki_consegna(&url).expect("comincia per file://");
        assert!(
            !consegnato.starts_with('/'),
            "resta la barra davanti: {consegnato}"
        );
        #[cfg(windows)]
        assert!(
            std::path::Path::new(consegnato).is_absolute(),
            "Windows non lo prende per assoluto: {consegnato}"
        );
    }

    #[test]
    fn gli_spazi_e_i_caratteri_non_ascii_si_lasciano_stare() {
        // Niente codifica percentuale: `souvlaki` non la toglie, e un `%20`
        // arriverebbe a Windows come tre caratteri veri di un nome che non
        // esiste. Spazi e lettere accentate passano tali e quali.
        let percorso = r"D:\Musica mia\Björk\è così.png";
        let url = url_di_file(std::path::Path::new(percorso)).expect("un percorso UTF-8");
        assert_eq!(percorso_che_souvlaki_consegna(&url), Some(percorso));
    }

    #[test]
    fn un_percorso_lungo_perde_il_prefisso_che_windows_non_accetta() {
        let url = url_di_file(std::path::Path::new(r"\\?\C:\copertine\ab.jpg"))
            .expect("un percorso UTF-8");
        assert_eq!(
            percorso_che_souvlaki_consegna(&url),
            Some(r"C:\copertine\ab.jpg")
        );
        let rete = url_di_file(std::path::Path::new(r"\\?\UNC\nas\dati\ab.jpg"))
            .expect("un percorso UTF-8");
        assert_eq!(
            percorso_che_souvlaki_consegna(&rete),
            Some(r"\\nas\dati\ab.jpg")
        );
    }
}
