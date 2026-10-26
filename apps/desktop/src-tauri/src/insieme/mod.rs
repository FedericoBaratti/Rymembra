//! «Ascolta insieme»: la sessione d'ascolto fra PC della stessa rete.
//!
//! # Chi fa che cosa
//!
//! - [`fasi`]: il riduttore puro. Decide, e basta.
//! - [`filo`]: il filo della sessione. Esegue quel che il riduttore decide —
//!   rete, lettore, libreria, finestra — e tiene la situazione.
//! - [`voci`]: le voci sul filo nei due versi, i gettoni (di chi ospita, e
//!   di chi segue per i brani che aggiunge), le copertine arrivate, i
//!   descrittori delle copie (fase 3).
//! - [`gesti`]: fase 2, i gesti di chi segue tradotti in azioni per `uid`, e
//!   le azioni tradotte di nuovo, da chi ospita, nei comandi del lettore.
//! - [`flusso`]: i byte di una voce altrui letti in rete, come `Sorgente`.
//! - [`segui`]: il regolatore di chi segue, a dieci colpi al secondo.
//! - [`fiducia`]: l'identità nel portachiavi, i fidati, i vicini.
//! - [`strada`]: che strada prende un gesto del lettore.
//!
//! # Le porte verso il resto
//!
//! Le funzioni di questo modulo sono le sole che il resto del programma
//! chiama: `riproduzione` ([`ruolo`], [`instrada`], [`lettore_cambiato`],
//! [`copertina`], [`nel_lettore`], [`uscita_cambiata`]), `main`
//! ([`StatoInsieme`], [`avvia_filo`], i comandi) e `spegnimento` ([`ferma`]).
//! Nessuna aspetta la rete, e nessuna prende il lucchetto del lettore: chi le
//! chiama può averlo in mano.
//!
//! # I comandi
//!
//! Ognuno passa dalla cassetta del filo e aspetta la risposta, al più
//! [`ATTESA_COMANDO`]; quelli che restituiscono il pannello lo leggono dalla
//! vetrina, che il filo aggiorna **prima** di rispondere. I due sincroni —
//! [`insieme_stato`] e [`insieme_cerca`] — girano sul filo della finestra, e
//! non aspettano niente: una lettura della vetrina e un messaggio imbucato.

mod fasi;
mod fiducia;
mod filo;
mod flusso;
mod gesti;
mod ipc;
mod preparazione;
mod segui;
pub mod strada;
mod voci;

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_insieme::messaggi::Diritti;
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{Esito, errore};
use filo::{Comune, Posta, Richiesta};

pub use ipc::{
    AssociazioneIpc, AvvisoIpc, FaseIpc, FidatoIpc, InsiemeNelLettore, InvitoIpc, PersonaIpc,
    ProprietarioIpc, RichiestaIpc, SessioneIpc, SincroniaIpc, StatoInsiemeIpc, VicinoIpc,
};
pub use strada::{Gesto, Instradato, Regola, Ruolo};

/// Quanto un comando aspetta il filo della sessione.
///
/// Venti secondi: il filo può essere dentro un'apertura del lettore — entrare
/// come host riapre la voce corrente, magari da un disco di rete — e un
/// comando che rinuncia prima direbbe «non riuscito» per una cosa che riesce.
/// Oltre, il filo è fermo davvero, e la finestra merita una risposta.
pub const ATTESA_COMANDO: Duration = Duration::from_secs(20);

/// Quanto [`ferma`] aspetta il filo, al più.
const ATTESA_FERMA: Duration = Duration::from_millis(400);

/// Lo stato di «Ascolta insieme», gestito da Tauri.
pub struct StatoInsieme {
    /// Quel che il filo condivide con chi legge senza passare dalla cassetta:
    /// il ruolo (vedi [`ruolo`]), il pannello, il campo del lettore, le
    /// copertine.
    comune: Arc<Comune>,
    /// Il campanello del lettore: [`lettore_cambiato`].
    sveglia: Sender<()>,
    /// La cassetta del filo della sessione.
    posta: Sender<Posta>,
    /// Il capo da cui il filo legge la cassetta, finché non parte.
    cassetta: Mutex<Option<Receiver<Posta>>>,
}

impl StatoInsieme {
    /// Lo stato, e l'orecchio del suo filo.
    #[must_use]
    pub fn nuovo() -> (Self, Receiver<()>) {
        let (sveglia, orecchio) = channel();
        let (posta, cassetta) = channel();
        (
            Self {
                comune: Arc::new(Comune::nuovo()),
                sveglia,
                posta,
                cassetta: Mutex::new(Some(cassetta)),
            },
            orecchio,
        )
    }
}

/// Fa partire il filo della sessione.
///
/// Da spento il filo dorme: legge le impostazioni e i fidati, e non apre
/// niente in rete finché l'interruttore non è acceso.
pub fn avvia_filo(app: AppHandle, orecchio: Receiver<()>) {
    let Some(stato) = app.try_state::<StatoInsieme>() else {
        return;
    };
    let Some(cassetta) = stato
        .cassetta
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return;
    };
    let comune = Arc::clone(&stato.comune);
    let posta = stato.posta.clone();
    filo::avvia(app.clone(), comune, posta, cassetta, orecchio);
}

/// Rimette la rete com'era all'avvio, se l'interruttore è acceso: si chiama
/// quando un aggiornamento non è riuscito dopo che [`ferma`] l'aveva spenta, e
/// l'applicazione resta viva. Non aspetta.
pub fn riaccendi(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoInsieme>() {
        let _ = stato.posta.send(Posta::Riaccendi);
    }
}

/// Chiude la rete uscendo: si chiama da `spegnimento::spegni`, prima di
/// salvare la coda, e dagli aggiornamenti prima di fermare la musica.
///
/// Chi ospita manda la fine a tutti, chi segue esce; il lettore torna alla
/// coda di questo PC, la rete si ferma. Aspetta al più [`ATTESA_FERMA`] e
/// senza niente in mano: si chiama dal filo principale, e il filo della
/// sessione potrebbe aver bisogno proprio di lui (per mostrare la finestra,
/// per dire) — oltre quel tempo si esce lo stesso. Le impostazioni non si
/// toccano: alla prossima apertura l'interruttore è com'era.
///
/// Ripetibile: la seconda volta trova tutto spento e torna subito.
pub fn ferma(app: &AppHandle) {
    let Some(stato) = app.try_state::<StatoInsieme>() else {
        return;
    };
    let (risposta, attesa) = channel();
    if stato.posta.send(Posta::Ferma { risposta }).is_err() {
        return;
    }
    let _ = attesa.recv_timeout(ATTESA_FERMA);
}

/// Il ruolo di questo PC, senza lucchetti.
#[must_use]
pub fn ruolo(app: &AppHandle) -> Ruolo {
    filo::ruolo_dal_byte(
        app.try_state::<StatoInsieme>()
            .map_or(0, |stato| stato.comune.ruolo.load(Ordering::Acquire)),
    )
}

/// La strada di un gesto: vedi [`strada`]. Si chiama in testa al corpo del
/// comando, **senza** il lucchetto del lettore in mano.
///
/// Per [`Regola::LocaleConVoci`] i brani di libreria diventano voci della
/// sessione qui, prima che il comando li metta in coda: il comando prosegue
/// con le maniglie al posto degli id. Per [`Regola::Inoltra`] (fase 2) il
/// gesto diventa una o più azioni per l'host ([`gesti::traduci`]), che il filo
/// della sessione manda: il comando torna subito, senza aspettare l'esito —
/// `pausa` gira sul filo della finestra.
#[must_use]
pub fn instrada(app: &AppHandle, gesto: &Gesto) -> Instradato {
    match strada::regola(ruolo(app), gesto) {
        Regola::Locale => Instradato::Locale,
        Regola::LocaleConVoci => {
            let ids: &[i64] = match gesto {
                Gesto::Suona { ids, .. } | Gesto::Accoda { ids } | Gesto::Dopo { ids } => ids,
                _ => &[],
            };
            match crate::riproduzione::insieme::voci_da_libreria(app, ids) {
                Ok(maniglie) => Instradato::ConVoci(maniglie),
                Err(err) => Instradato::Rifiutato(err),
            }
        }
        Regola::Inoltra => inoltra(app, gesto),
        Regola::NonConcesso => Instradato::Rifiutato(AppError::new(ErrorCode::InsiemeNonConcesso)),
        Regola::Rifiuta => {
            Instradato::Rifiutato(AppError::new(ErrorCode::InsiemeGestoNonDisponibile {
                gesto: nome_del_gesto(gesto).to_owned(),
            }))
        }
    }
}

/// Chi segue: il gesto tradotto e affidato al filo della sessione.
///
/// Lo specchio si legge qui, senza lucchetti in mano (è la regola di
/// `instrada`), e i brani da aggiungere si descrivono qui: `brani_di_questo`
/// legge il database e il disco, e i comandi che aggiungono girano già in
/// disparte, mentre il filo della sessione deve restare libero per le ancore.
fn inoltra(app: &AppHandle, gesto: &Gesto) -> Instradato {
    let Some(stato) = app.try_state::<StatoInsieme>() else {
        return Instradato::Rifiutato(AppError::new(ErrorCode::InternalAborted {
            what: Some("insieme".to_owned()),
        }));
    };
    // Senza lo specchio (il lettore a metà di un'entrata o di un'uscita) un
    // gesto che nomina una voce non si saprebbe dire per `uid`: vedi
    // `gesti::traduci_senza_specchio`.
    let traduzione = match crate::riproduzione::insieme::istantanea(app) {
        Some(specchio) => gesti::traduci(gesto, &specchio),
        None => gesti::traduci_senza_specchio(gesto),
    };
    match traduzione {
        Err(err) => Instradato::Rifiutato(err),
        Ok(gesti::Traduzione::Azioni(azioni)) => {
            if !azioni.is_empty() {
                let _ = stato.posta.send(Posta::Gesti(azioni));
            }
            Instradato::Inoltrato
        }
        Ok(gesti::Traduzione::Aggiungi { ids, dove, suona }) => {
            match crate::riproduzione::insieme::brani_di_questo(app, &ids) {
                // Non è rimasto niente da aggiungere (righe sparite, file che
                // non si leggono): un no, invece di un gesto che non fa niente.
                Ok(brani) if brani.is_empty() => {
                    Instradato::Rifiutato(AppError::new(ErrorCode::LibraryTrackNotFound {
                        track_id: ids.first().copied(),
                    }))
                }
                Ok(brani) => {
                    // Parte solo il brano cliccato: se è lui che manca, gli
                    // altri si aggiungono e basta (vedi `gesti::traduci`).
                    let suona = suona && brani.first().map(|b| b.track_id) == ids.first().copied();
                    let _ = stato.posta.send(Posta::Aggiungi { brani, dove, suona });
                    Instradato::Inoltrato
                }
                Err(err) => Instradato::Rifiutato(err),
            }
        }
    }
}

/// Il nome stabile di un gesto, per l'errore: senza gli id e le posizioni che
/// porta con sé, che nel diario non servono a nessuno.
#[must_use]
pub const fn nome_del_gesto(gesto: &Gesto) -> &'static str {
    match gesto {
        Gesto::Pausa => "pausa",
        Gesto::Riprendi => "riprendi",
        Gesto::Alterna => "alterna",
        Gesto::Prossimo => "prossimo",
        Gesto::Precedente => "precedente",
        Gesto::VaiA { .. } => "vaiA",
        Gesto::Suona { .. } => "suona",
        Gesto::Radio => "radio",
        Gesto::Accoda { .. } => "accoda",
        Gesto::Dopo { .. } => "dopo",
        Gesto::CodaVai { .. } => "codaVai",
        Gesto::CodaTogli { .. } => "codaTogli",
        Gesto::CodaRiordina { .. } => "codaRiordina",
        Gesto::CodaSvuota => "codaSvuota",
        Gesto::CodaRipristina => "codaRipristina",
        Gesto::Shuffle => "shuffle",
        Gesto::Ripeti => "ripeti",
        Gesto::Autoplay => "autoplay",
        Gesto::Timer => "timer",
    }
}

/// Chi ospita: la coda, la voce corrente o la pausa sono cambiate, e gli altri
/// vanno avvisati. Non blocca: suona il campanello del filo della sessione.
pub fn lettore_cambiato(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoInsieme>()
        && stato.comune.ruolo.load(Ordering::Acquire) == filo::ruolo_in_byte(Ruolo::Host)
    {
        let _ = stato.sveglia.send(());
    }
}

/// L'uscita audio di questo PC è cambiata: chi segue rimette il regolatore da
/// capo, perché la deriva che aveva imparato era di quell'altra scheda. Non
/// blocca. Vedi `richieste-sessione.md`: la chiama il lettore dove l'uscita
/// cambia davvero.
pub fn uscita_cambiata(app: &AppHandle) {
    if let Some(stato) = app.try_state::<StatoInsieme>() {
        let _ = stato.posta.send(Posta::UscitaCambiata);
    }
}

/// I byte di una copertina di sessione, per il protocollo `rymembra-cover`.
/// Solo le chiavi `insieme-<uid>`, e solo da chi segue: sono in memoria, mai
/// nell'archivio delle copertine.
#[must_use]
pub fn copertina(app: &AppHandle, chiave: &str) -> Option<Arc<[u8]>> {
    voci::uid_della_chiave(chiave)?;
    let stato = app.try_state::<StatoInsieme>()?;
    stato
        .comune
        .copertine
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .byte(chiave)
}

/// Il campo `insieme` di `StatoRiproduzione`: una fotografia che il filo
/// della sessione tiene aggiornata. Nessun lucchetto del lettore: lo si legge
/// col suo in mano.
#[must_use]
pub fn nel_lettore(app: &AppHandle) -> Option<InsiemeNelLettore> {
    app.try_state::<StatoInsieme>()?
        .comune
        .nel_lettore
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Un comando al filo della sessione, e la sua risposta.
fn chiedi(app: &AppHandle, richiesta: Richiesta) -> Result<(), AppError> {
    let fermo = || {
        AppError::new(ErrorCode::InternalAborted {
            what: Some("insieme".to_owned()),
        })
    };
    let stato = app.try_state::<StatoInsieme>().ok_or_else(fermo)?;
    let (risposta, attesa) = channel();
    stato
        .posta
        .send(Posta::Comando {
            richiesta,
            risposta,
        })
        .map_err(|_| fermo())?;
    attesa
        .recv_timeout(ATTESA_COMANDO)
        .map_err(|_| fermo())?
        .map_err(senza_causa_di_rete)
}

/// Un errore della sessione senza la sua causa: `errore()` la scrive nel
/// diario, e la causa di un guasto di rete può contenere un indirizzo. Il
/// codice basta alla finestra per la frase, e al diario per capire. Fa
/// eccezione `identitaPersa`, la cui causa la scrive `fiducia` e dice soltanto
/// quale delle tre strade è mancata.
fn senza_causa_di_rete(err: AppError) -> AppError {
    let kind = err.code().kind();
    if kind.domain() == rymembra_domain::errors::Domain::Insieme
        && kind != rymembra_domain::errors::ErrorCodeKind::InsiemeIdentitaPersa
    {
        AppError::new(err.code().clone())
    } else {
        err
    }
}

/// Il pannello com'è adesso nella vetrina.
fn vetrina(app: &AppHandle) -> StatoInsiemeIpc {
    app.try_state::<StatoInsieme>()
        .map_or_else(filo::vetrina_spenta, |stato| {
            stato
                .comune
                .vetrina
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        })
}

/// Un comando che risponde col pannello aggiornato.
fn e_poi_il_pannello(app: &AppHandle, richiesta: Richiesta) -> Esito<StatoInsiemeIpc> {
    chiedi(app, richiesta).map_err(errore)?;
    Ok(vetrina(app))
}

/// Il pannello, adesso.
#[tauri::command]
pub fn insieme_stato(stato: State<'_, StatoInsieme>) -> Esito<StatoInsiemeIpc> {
    Ok(stato
        .comune
        .vetrina
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone())
}

/// Accende o spegne «Ascolta insieme».
#[tauri::command(async)]
pub fn insieme_attiva(app: AppHandle, attivo: bool) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Attiva(attivo))
}

/// Cambia il nome di questo PC.
#[tauri::command(async)]
pub fn insieme_nome(app: AppHandle, nome: String) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Nome(nome))
}

/// Il dialogo sul firewall è stato visto.
#[tauri::command(async)]
pub fn insieme_firewall_visto(app: AppHandle) -> Esito<()> {
    chiedi(&app, Richiesta::FirewallVisto).map_err(errore)
}

/// Comincia a ospitare una sessione.
#[tauri::command(async)]
pub fn insieme_ospita(app: AppHandle) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Ospita)
}

/// Chi ospita: termina la sessione per tutti.
#[tauri::command(async)]
pub fn insieme_termina(app: AppHandle) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Termina)
}

/// Chi segue: esce dalla sessione.
#[tauri::command(async)]
pub fn insieme_esci(app: AppHandle) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Esci)
}

/// Un giro di scoperta adesso: il pannello si è appena aperto. Non aspetta:
/// le risposte arrivano col prossimo `insieme:stato`.
#[tauri::command]
pub fn insieme_cerca(app: AppHandle) -> Esito<()> {
    if let Some(stato) = app.try_state::<StatoInsieme>() {
        let _ = stato.posta.send(Posta::Cerca);
    }
    Ok(())
}

/// Comincia l'associazione con un PC visto in rete.
#[tauri::command(async)]
pub fn insieme_associa(app: AppHandle, vicino: String) -> Esito<()> {
    chiedi(&app, Richiesta::Associa(vicino)).map_err(errore)
}

/// La risposta al codice di quattro cifre.
#[tauri::command(async)]
pub fn insieme_codice(app: AppHandle, associazione: u64, uguale: bool) -> Esito<()> {
    chiedi(
        &app,
        Richiesta::Codice {
            associazione,
            uguale,
        },
    )
    .map_err(errore)
}

/// Chi ospita: invita un PC associato e collegato, con quel che potrà fare.
#[tauri::command(async)]
pub fn insieme_invita(app: AppHandle, pari: i64, diritti: Diritti) -> Esito<()> {
    chiedi(&app, Richiesta::Invita { pari, diritti }).map_err(errore)
}

/// La risposta a un invito.
#[tauri::command(async)]
pub fn insieme_rispondi(app: AppHandle, invito: u64, accetta: bool) -> Esito<()> {
    chiedi(&app, Richiesta::Rispondi { invito, accetta }).map_err(errore)
}

/// Chiede a un PC associato che ospita di entrare nella sua sessione.
#[tauri::command(async)]
pub fn insieme_chiedi(app: AppHandle, pari: i64) -> Esito<()> {
    chiedi(&app, Richiesta::Chiedi(pari)).map_err(errore)
}

/// Chi ospita: la risposta a una richiesta d'ingresso, con quel che potrà
/// fare chi entra. Per un no, `diritti` non conta.
#[tauri::command(async)]
pub fn insieme_ammetti(
    app: AppHandle,
    richiesta: u64,
    accetta: bool,
    diritti: Diritti,
) -> Esito<()> {
    chiedi(
        &app,
        Richiesta::Ammetti {
            richiesta,
            accetta,
            diritti,
        },
    )
    .map_err(errore)
}

/// Chi ospita: toglie un membro dalla sessione.
#[tauri::command(async)]
pub fn insieme_allontana(app: AppHandle, membro: u32) -> Esito<()> {
    chiedi(&app, Richiesta::Allontana(membro)).map_err(errore)
}

/// Chi ospita: cambia che cosa può fare un membro.
#[tauri::command(async)]
pub fn insieme_concedi(app: AppHandle, membro: u32, diritti: Diritti) -> Esito<()> {
    chiedi(&app, Richiesta::Concedi { membro, diritti }).map_err(errore)
}

/// Rinomina un PC associato, per questo PC.
#[tauri::command(async)]
pub fn insieme_pari_rinomina(app: AppHandle, id: i64, nome: String) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Rinomina { id, nome })
}

/// Dimentica un PC associato: la prossima volta si riassocia.
#[tauri::command(async)]
pub fn insieme_pari_dimentica(app: AppHandle, id: i64) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Dimentica(id))
}

/// Blocca o sblocca un PC associato.
#[tauri::command(async)]
pub fn insieme_pari_blocca(app: AppHandle, id: i64, bloccato: bool) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Blocca { id, bloccato })
}

/// Gli indirizzi scritti a mano, `ip:porta`.
#[tauri::command(async)]
pub fn insieme_indirizzi(app: AppHandle, indirizzi: Vec<String>) -> Esito<StatoInsiemeIpc> {
    e_poi_il_pannello(&app, Richiesta::Indirizzi(indirizzi))
}

/// Accende o spegne la voce di calibrazione: clic identici su tutti i PC della
/// sessione, nel percorso di sincronia normale.
#[tauri::command(async)]
pub fn insieme_calibra(app: AppHandle, attiva: bool) -> Esito<()> {
    chiedi(&app, Richiesta::Calibra(attiva)).map_err(errore)
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_nome_del_gesto_non_porta_id_ne_posizioni() {
        let gesto = Gesto::Suona {
            ids: vec![12_345],
            da: 7,
        };
        assert_eq!(nome_del_gesto(&gesto), "suona");
        assert!(!nome_del_gesto(&Gesto::CodaTogli { indici: vec![99] }).contains("99"));
    }
}
