//! I profili dei modelli, e il filo che porta una risposta mentre arriva.
//!
//! Involucri, come tutto il resto di questo crate: il client sta in
//! `aether-ia`, i profili in `settings`, le chiavi nel portachiavi. Qui c'è chi
//! li mette insieme, chi tiene il turno in corso, e chi lo racconta alla
//! finestra.
//!
//! # Cosa questo modulo non sa
//!
//! Cos'è una skin. Le istruzioni che descrivono i token, le parti e il formato
//! delle modifiche le costruisce la finestra — dove il registro e il documento
//! sono già in memoria — e arrivano qui dentro `messaggi` come un messaggio
//! come gli altri. Costruirle qui vorrebbe dire una seconda copia del
//! vocabolario in un secondo linguaggio, cioè la cosa che diverge.
//!
//! # Una conversazione alla volta
//!
//! Due generazioni sullo stesso documento si scriverebbero addosso a vicenda,
//! e chi guarda non saprebbe quale delle due ha vinto. La seconda richiesta
//! riceve `ia.busy`, che è `Info` e ritentabile: non è un guasto, è «aspetta
//! che finisca».
//!
//! # Dove vivono le cose
//!
//! I profili in `settings` come JSON — la tabella è chiave/valore, quindi non
//! serve nessuna migrazione — e le chiavi nel portachiavi del sistema, una voce
//! per profilo. Mai in `settings`, e senza ripiego se il portachiavi non
//! risponde: la regola sta scritta in `portachiavi.rs` e vale anche qui.
//!
//! # Cosa non finisce mai in un log
//!
//! La chiave. Ci finiscono l'id del profilo, il nome del modello e i codici che
//! il servizio restituisce, che servono a capire cosa è successo e non sono
//! segreti. Il testo della conversazione **non** ci finisce: è quel che
//! l'utente ha scritto, e un diario allegato a una segnalazione non deve
//! portarselo dietro.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aether_app::settings;
use aether_domain::errors::{AppError, ErrorCode};
use aether_ia::messaggi::{Fine, Messaggio, Voce};
use aether_ia::{Cliente, Fornitore, Profilo};
use aether_oauth::portachiavi::{DiSistema, Portachiavi as _};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _, State};

use crate::errore::{ErroreIpc, Esito, errore};
use crate::nota;
use crate::spegnimento::{self, Emette as _};
use crate::stato::{Stato, con_libreria};

/// I profili, come JSON, in `settings`.
const CHIAVE_PROFILI: &str = "ia.profili";

/// L'id del profilo scelto, in `settings`.
const CHIAVE_ATTIVO: &str = "ia.profilo";

/// Quanti profili si possono salvare.
///
/// Sedici. Non è una risorsa scarsa — sono quattro righe di JSON l'uno — ma è
/// il numero oltre il quale un elenco a scelta singola smette di essere un
/// elenco, e soprattutto è il tetto che impedisce a un difetto della finestra
/// di riempire `settings` con un profilo per clic.
const PROFILI_MASSIMI: usize = 16;

/// Ogni quanto i pezzi di testo escono verso la finestra.
///
/// Cinquanta millisecondi. Un evento IPC per gettone inonderebbe il canale
/// come faceva lo scarico prima di accorpare l'avanzamento: un modello veloce
/// ne produce cento al secondo, e cento `PostMessageW` al secondo per scrivere
/// duecento caratteri è un costo che si vede nell'animazione di tutto il resto
/// della finestra. Sotto i cinquanta millisecondi la scrittura resta fluida da
/// guardare.
const RESPIRO: Duration = Duration::from_millis(50);

// ── lo stato ────────────────────────────────────────────────────────────────

/// Il turno in corso.
struct Turno {
    /// Il numero che la finestra usa per riconoscere i suoi eventi.
    numero: u64,
    /// Alzata da [`ia_ferma`].
    fermare: Arc<AtomicBool>,
}

/// Lo stato dei modelli.
pub struct StatoIa {
    /// La conversazione in corso, se ce n'è una.
    corrente: Mutex<Option<Turno>>,
    /// Il numero del prossimo turno.
    ///
    /// Cresce e non torna indietro: un numero riusato farebbe arrivare alla
    /// finestra i pezzi di una risposta vecchia con l'etichetta di quella
    /// nuova, ed è il tipo di difetto che si vede una volta ogni cento e non si
    /// riproduce mai.
    prossimo: AtomicU64,
}

impl StatoIa {
    /// Lo stato, vuoto. Non tocca né rete né disco.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            corrente: Mutex::new(None),
            prossimo: AtomicU64::new(1),
        }
    }

    /// Prende il posto per un turno nuovo, se è libero.
    ///
    /// Restituisce il numero del turno e la sua bandiera. `None` quando ce n'è
    /// già uno in corso: la prova e la presa sono un gesto solo, o due
    /// richieste arrivate insieme passerebbero tutte e due.
    fn prendi_posto(&self) -> Option<(u64, Arc<AtomicBool>)> {
        let mut corrente = self
            .corrente
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if corrente.is_some() {
            return None;
        }
        let numero = self.prossimo.fetch_add(1, Ordering::Relaxed);
        let fermare = Arc::new(AtomicBool::new(false));
        *corrente = Some(Turno {
            numero,
            fermare: Arc::clone(&fermare),
        });
        Some((numero, fermare))
    }

    /// Libera il posto, se il turno che lo teneva è ancora questo.
    ///
    /// Il controllo sul numero non è pedanteria: senza, un filo che finisce
    /// tardi libererebbe il posto di un turno cominciato dopo di lui.
    fn lascia_posto(&self, numero: u64) {
        let mut corrente = self
            .corrente
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if corrente.as_ref().is_some_and(|t| t.numero == numero) {
            *corrente = None;
        }
    }

    /// Alza la bandiera di quel turno. `false` se non è più lui.
    fn ferma(&self, numero: u64) -> bool {
        let corrente = self
            .corrente
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match corrente.as_ref() {
            Some(turno) if turno.numero == numero => {
                turno.fermare.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// C'è una conversazione in corso.
    fn occupato(&self) -> bool {
        self.corrente
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }
}

impl Default for StatoIa {
    fn default() -> Self {
        Self::nuovo()
    }
}

// ── quel che la finestra riceve ─────────────────────────────────────────────

/// I profili e quale è scelto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoIaIpc {
    /// Tutti, nell'ordine in cui sono stati salvati.
    pub profili: Vec<Profilo>,
    /// L'id di quello scelto, se ce n'è uno valido.
    pub attivo: Option<String>,
    /// C'è una conversazione in corso.
    pub occupato: bool,
}

/// Un profilo come la finestra lo manda.
///
/// Diverso da [`Profilo`] per due campi, e sono i due che la finestra non ha il
/// diritto di decidere: l'`id`, che qui è `None` per un profilo nuovo e che il
/// nucleo genera; e `conChiave`, che è una **risposta** sul portachiavi e non
/// un desiderio.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfiloIn {
    /// L'id di un profilo che esiste già, o `None` per crearne uno.
    pub id: Option<String>,
    /// Come lo chiama chi lo scrive.
    pub nome: String,
    /// Chi serve il modello.
    pub fornitore: Fornitore,
    /// L'indirizzo di base.
    pub url_base: String,
    /// Il nome del modello.
    pub modello: String,
}

/// Un pezzo di risposta.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PezzoIpc {
    turno: u64,
    testo: String,
    /// È ragionamento e non risposta.
    ///
    /// Un booleano e non il nome della voce, per la stessa ragione di
    /// [`OperazioneIpc::togli`]: sono due sole. Chi lo riceve lo usa per due
    /// decisioni che non si possono sbagliare — dove disegnarlo, e se darlo in
    /// pasto all'estrattore delle modifiche. Il ragionamento non ci va: contiene
    /// i tentativi scartati, e ci si troverebbe applicata la versione che il
    /// modello aveva scritto per poi cambiare idea.
    pensiero: bool,
}

/// La fine di un turno.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FineIpc {
    turno: u64,
    #[serde(flatten)]
    fine: Fine,
}

/// Un turno finito male.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuastoIpc {
    turno: u64,
    errore: ErroreIpc,
}

/// Una modifica proposta da un modello.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperazioneIpc {
    /// Toglie invece di scrivere.
    ///
    /// Un booleano e non il nome dell'operazione: sono due sole, e un `op:
    /// "scrivi" | "togli"` costringerebbe la finestra a confrontare stringhe
    /// per decidere fra due rami.
    pub togli: bool,
    /// Dove, un passo per livello.
    pub percorso: Vec<String>,
    /// Cosa scrivere. Assente per una che toglie.
    pub valore: Option<serde_json::Value>,
}

/// Quel che si è capito, e quel che no.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperazioniIpc {
    /// Le modifiche ben formate.
    pub operazioni: Vec<OperazioneIpc>,
    /// Perché il resto è stato scartato, una riga per pezzo.
    ///
    /// Servono a due cose: mostrarle a chi guarda, e rimandarle al modello in
    /// modalità agent — dove sono esattamente l'informazione che gli serve per
    /// correggersi.
    pub ragioni: Vec<String>,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// I profili configurati. Istantaneo: non tocca la rete.
#[tauri::command]
pub fn ia_profili(stato: State<'_, Stato>, ia: State<'_, StatoIa>) -> Esito<StatoIaIpc> {
    stato_ipc(&stato, &ia).map_err(errore)
}

/// Salva un profilo, nuovo o esistente, e lo sceglie.
///
/// # Cosa fa `chiave`
///
/// `None` lascia stare quel che c'è nel portachiavi — è il caso di chi cambia
/// il modello senza reincollare il segreto — `Some("")` lo cancella, e
/// `Some(k)` lo sostituisce. Tre casi e non due: senza il primo, ogni modifica
/// a un profilo costringerebbe a riscrivere la chiave, e la finestra dovrebbe
/// tenerla in memoria per poterla rimandare.
///
/// # Perché l'indirizzo si controlla qui e non alla prima domanda
///
/// Perché un profilo che non potrà mai funzionare non deve poter essere
/// salvato: la scoperta arriverebbe dopo aver scritto la prima richiesta, in
/// una schermata diversa da quella in cui è stato commesso l'errore.
#[tauri::command]
pub fn ia_salva_profilo(
    stato: State<'_, Stato>,
    ia: State<'_, StatoIa>,
    profilo: ProfiloIn,
    chiave: Option<String>,
) -> Esito<StatoIaIpc> {
    let esito = (|| {
        let nome = profilo.nome.trim();
        let url_base = profilo.url_base.trim().to_owned();
        let modello = profilo.modello.trim().to_owned();
        aether_ia::controlla_indirizzo(&url_base)?;

        let mut profili = leggi_profili(&stato)?;
        let quale = profilo
            .id
            .as_deref()
            .and_then(|id| profili.iter().position(|p| p.id == id));
        if quale.is_none() && profili.len() >= PROFILI_MASSIMI {
            return Err(
                AppError::new(ErrorCode::IaNotConfigured).with_message(format!(
                    "non si possono salvare più di {PROFILI_MASSIMI} profili"
                )),
            );
        }

        let id = match quale.and_then(|i| profili.get(i)) {
            Some(esistente) => esistente.id.clone(),
            None => {
                let presi: Vec<String> = profili.iter().map(|p| p.id.clone()).collect();
                aether_ia::id_da_nome(nome, &presi)
            }
        };

        // Il segreto **prima** dell'elenco: se il portachiavi non risponde, un
        // profilo salvato senza la sua chiave direbbe di essere pronto e non lo
        // sarebbe. Al contrario, una chiave scritta e un elenco non salvato
        // lascia solo una voce orfana, che il salvataggio successivo riusa.
        let con_chiave = match chiave.as_deref() {
            None => quale
                .and_then(|i| profili.get(i))
                .is_some_and(|p| p.con_chiave),
            Some("") => {
                portachiavi().cancella(&aether_ia::voce_portachiavi(&id))?;
                false
            }
            Some(valore) => {
                portachiavi().scrivi(&aether_ia::voce_portachiavi(&id), valore.trim())?;
                true
            }
        };

        let salvato = Profilo {
            id: id.clone(),
            nome: if nome.is_empty() {
                modello.clone()
            } else {
                nome.to_owned()
            },
            fornitore: profilo.fornitore,
            url_base,
            modello,
            con_chiave,
        };
        match quale.and_then(|i| profili.get_mut(i)) {
            Some(posto) => *posto = salvato,
            None => profili.push(salvato),
        }

        scrivi_profili(&stato, &profili)?;
        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, &id)
        })?;
        nota!("[ia] profilo salvato: {id}");
        Ok(())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &ia).map_err(errore)
}

/// Elimina un profilo e la sua chiave.
///
/// # Perché il segreto si cancella per primo, e se non ci riesce ci si ferma
///
/// `cancella` è idempotente e sbaglia solo quando il portachiavi non risponde.
/// Proseguire lì vorrebbe dire togliere dall'elenco l'unico posto in cui era
/// scritto il nome della voce, e lasciare nel Credential Manager una
/// credenziale che nessuno saprà più a cosa apparteneva. Il profilo resta, e la
/// schermata dice perché.
#[tauri::command]
pub fn ia_elimina_profilo(
    stato: State<'_, Stato>,
    ia: State<'_, StatoIa>,
    id: String,
) -> Esito<StatoIaIpc> {
    let esito = (|| {
        portachiavi().cancella(&aether_ia::voce_portachiavi(&id))?;
        let mut profili = leggi_profili(&stato)?;
        profili.retain(|p| p.id != id);
        scrivi_profili(&stato, &profili)?;
        con_libreria(&stato, |libreria| {
            // L'id scelto si dimentica solo se era questo: un `forget` cieco
            // scollegherebbe il profilo attivo eliminandone un altro.
            if settings::read(&libreria.connection, CHIAVE_ATTIVO)?.as_deref() == Some(id.as_str())
            {
                settings::forget(&libreria.connection, CHIAVE_ATTIVO)?;
            }
            Ok(())
        })?;
        nota!("[ia] profilo eliminato: {id}");
        Ok(())
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &ia).map_err(errore)
}

/// Sceglie quale profilo usa la chat.
#[tauri::command]
pub fn ia_scegli_profilo(
    stato: State<'_, Stato>,
    ia: State<'_, StatoIa>,
    id: String,
) -> Esito<StatoIaIpc> {
    let esito = (|| {
        let profili = leggi_profili(&stato)?;
        if !profili.iter().any(|p| p.id == id) {
            return Err(AppError::new(ErrorCode::IaNotConfigured)
                .with_message(format!("nessun profilo si chiama «{id}»")));
        }
        con_libreria(&stato, |libreria| {
            settings::write(&libreria.connection, CHIAVE_ATTIVO, &id)
        })
    })();
    esito.map_err(errore)?;
    stato_ipc(&stato, &ia).map_err(errore)
}

/// I modelli che quel profilo può usare.
///
/// È anche la prova di connessione: un servizio che risponde a questa risponde
/// a tutto. Vedi [`Cliente::modelli`].
///
/// `(async)` perché va in rete: un comando bloccante sul filo della finestra la
/// fermerebbe per tutta la durata della richiesta.
#[tauri::command(async)]
pub fn ia_modelli(stato: State<'_, Stato>, id: String) -> Esito<Vec<aether_ia::Modello>> {
    (|| {
        let profilo = trova(&stato, Some(&id))?;
        cliente(&profilo)?.modelli()
    })()
    .map_err(errore)
}

/// Manda la conversazione al modello e restituisce subito il numero del turno.
///
/// # Perché torna un numero e non la risposta
///
/// Perché la risposta arriva a pezzi, e un comando che aspettasse la fine
/// darebbe una schermata ferma per il minuto che ci vuole. I pezzi escono come
/// eventi `ia:pezzo`, e il numero è quel che permette alla finestra di
/// riconoscere i suoi: una risposta cominciata prima di un «Ferma» può ancora
/// consegnare un blocco dopo, e senza il numero finirebbe dentro la
/// conversazione successiva.
///
/// Le istruzioni di sistema arrivano dentro `messaggi`, costruite dalla
/// finestra: vedi il preambolo.
#[tauri::command(async)]
pub fn ia_conversa(
    app: AppHandle,
    stato: State<'_, Stato>,
    ia: State<'_, StatoIa>,
    messaggi: Vec<Messaggio>,
) -> Esito<u64> {
    // Il client si costruisce **prima** di prendere il posto: un profilo
    // incompleto deve dare il suo errore subito, non occupare il turno per il
    // tempo di scoprirlo.
    let profilo = trova(&stato, None).map_err(errore)?;
    let cliente = cliente(&profilo).map_err(errore)?;
    if messaggi.is_empty() {
        return Err(errore(AppError::new(ErrorCode::IaBadResponse {
            detail: Some("nessun messaggio da mandare".to_owned()),
        })));
    }

    let Some((numero, fermare)) = ia.prendi_posto() else {
        return Err(errore(AppError::new(ErrorCode::IaBusy)));
    };

    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-ia".to_owned())
        .spawn(move || {
            parla(&manico, &cliente, &messaggi, numero, &fermare);
            manico.state::<StatoIa>().lascia_posto(numero);
        });
    if let Err(err) = avviato {
        ia.lascia_posto(numero);
        nota!("[ia] il filo della conversazione non è partito: {err}");
        return Err(errore(
            AppError::new(ErrorCode::InternalUnexpected {
                detail: Some("filo della conversazione".to_owned()),
            })
            .with_cause(err.to_string()),
        ));
    }
    Ok(numero)
}

/// Le modifiche ben formate dentro un testo scritto da un modello.
///
/// # Perché è un comando e non tre righe in TypeScript
///
/// Perché è la funzione che riceve l'ingresso meno prevedibile
/// dell'applicazione, e `npm run verify` non ha prove TypeScript: scritta di
/// là non avrebbe nessuna prova, e i suoi modi di sbagliare sono una dozzina —
/// un blocco mai chiuso, tre apici in mezzo a una riga, un «togli» con dentro
/// un valore. Scritta qui ne ha una per ognuno.
///
/// Non tocca niente: né rete, né disco, né documento. Le operazioni le applica
/// la finestra con le stesse funzioni pure che usano i controlli, e finiscono
/// quindi nello stesso annullo.
#[tauri::command]
pub fn ia_operazioni(testo: String) -> Esito<OperazioniIpc> {
    let (operazioni, ragioni) = aether_ia::operazioni::estrai(&testo);
    Ok(OperazioniIpc {
        operazioni: operazioni
            .into_iter()
            .map(|o| OperazioneIpc {
                togli: o.op == aether_ia::Op::Togli,
                percorso: o.percorso,
                valore: o.valore,
            })
            .collect(),
        ragioni,
    })
}

/// Ferma il turno, se è ancora quello.
///
/// Non è un errore fermare un turno già finito: chi preme «Ferma» un istante
/// dopo l'ultimo gettone ha fatto la cosa giusta con un tempismo sfortunato, e
/// un guasto rosso sarebbe una bugia.
#[tauri::command]
pub fn ia_ferma(ia: State<'_, StatoIa>, turno: u64) -> Esito<bool> {
    Ok(ia.ferma(turno))
}

// ── il filo ─────────────────────────────────────────────────────────────────

/// Una conversazione intera, dal filo di sottofondo.
///
/// # Le due bandiere dell'annullamento
///
/// Quella del turno e [`spegnimento::in_uscita`]. Senza la seconda, chiudere la
/// finestra durante una generazione lascerebbe questo filo attaccato a un
/// socket per i dieci minuti della scadenza: `emetti` tacerebbe da solo, ma il
/// filo resterebbe, e con lui il processo.
fn parla(
    app: &AppHandle,
    cliente: &Cliente,
    messaggi: &[Messaggio],
    numero: u64,
    fermare: &AtomicBool,
) {
    let annullato = || fermare.load(Ordering::Relaxed) || spegnimento::in_uscita();

    // I pezzi si accorpano prima di uscire: vedi [`RESPIRO`]. `ultimo` nasce
    // indietro di un respiro apposta, perché il primo carattere arrivi subito —
    // è quello che dice a chi guarda che il modello ha cominciato a rispondere.
    let mut cesto = String::new();
    let mut voce = Voce::Risposta;
    let mut ultimo = Instant::now()
        .checked_sub(RESPIRO)
        .unwrap_or_else(Instant::now);

    let esito = cliente.conversa(messaggi, &annullato, &mut |quale, testo| {
        // Cambiare voce svuota il cesto subito, senza aspettare il respiro: il
        // cesto porta **una** voce, e accorpare l'ultimo pensiero con la prima
        // parola della risposta li marchierebbe tutti e due allo stesso modo —
        // cioè, in un verso, ragionamento dentro il blocco delle modifiche.
        if quale != voce && !cesto.is_empty() {
            app.emetti(
                "ia:pezzo",
                PezzoIpc {
                    turno: numero,
                    testo: std::mem::take(&mut cesto),
                    pensiero: voce == Voce::Pensiero,
                },
            );
            ultimo = Instant::now();
        }
        voce = quale;
        cesto.push_str(testo);
        if ultimo.elapsed() >= RESPIRO {
            app.emetti(
                "ia:pezzo",
                PezzoIpc {
                    turno: numero,
                    testo: std::mem::take(&mut cesto),
                    pensiero: voce == Voce::Pensiero,
                },
            );
            ultimo = Instant::now();
        }
    });

    // Quel che è rimasto nel cesto esce comunque, e prima della fine: senza,
    // l'ultima frase di ogni risposta sparirebbe — cioè proprio la parte in cui
    // sta il blocco delle modifiche.
    if !cesto.is_empty() {
        app.emetti(
            "ia:pezzo",
            PezzoIpc {
                turno: numero,
                testo: cesto,
                pensiero: voce == Voce::Pensiero,
            },
        );
    }

    match esito {
        Ok(fine) => {
            nota!(
                "[ia] turno {numero} finito: {:?} gettoni={:?}/{:?}",
                fine.motivo,
                fine.gettoni_in,
                fine.gettoni_out
            );
            app.emetti(
                "ia:fine",
                FineIpc {
                    turno: numero,
                    fine,
                },
            );
        }
        Err(err) => {
            // `errore` e non `ErroreIpc::from`: è la riga che mette il guasto
            // nel diario, ed è l'unica differenza fra una segnalazione che dice
            // «non funziona» e una che dice quale codice, quando.
            let ipc = errore(err);
            app.emetti(
                "ia:errore",
                GuastoIpc {
                    turno: numero,
                    errore: *ipc,
                },
            );
        }
    }
}

// ── il contorno ─────────────────────────────────────────────────────────────

/// Il portachiavi del sistema.
const fn portachiavi() -> DiSistema {
    DiSistema
}

/// I profili salvati.
///
/// Un elenco illeggibile vale un elenco vuoto invece di un errore: è la stessa
/// scelta di `read_json`, e la ragione è che un `ia.profili` corrotto non deve
/// impedire di aprire le impostazioni — che è l'unico posto da cui lo si può
/// riscrivere.
fn leggi_profili(stato: &State<'_, Stato>) -> Result<Vec<Profilo>, AppError> {
    con_libreria(stato, |libreria| {
        settings::read_json::<Vec<Profilo>>(&libreria.connection, CHIAVE_PROFILI)
    })
    .map(Option::unwrap_or_default)
}

/// Scrive l'elenco.
fn scrivi_profili(stato: &State<'_, Stato>, profili: &[Profilo]) -> Result<(), AppError> {
    con_libreria(stato, |libreria| {
        settings::write_json(&libreria.connection, CHIAVE_PROFILI, &profili)
    })
}

/// Il profilo chiesto, o quello attivo.
///
/// # Errori
///
/// `ia.notConfigured` quando non ce n'è nessuno, o quando l'id non esiste più —
/// che è la condizione di chi ha eliminato il profilo scelto da un'altra
/// finestra.
fn trova(stato: &State<'_, Stato>, id: Option<&str>) -> Result<Profilo, AppError> {
    let profili = leggi_profili(stato)?;
    let scelto = match id {
        Some(id) => Some(id.to_owned()),
        None => con_libreria(stato, |libreria| {
            settings::read(&libreria.connection, CHIAVE_ATTIVO)
        })?,
    };
    // Senza un id scelto vale il primo: chi ha salvato un profilo solo non deve
    // doverlo anche selezionare per usarlo.
    let profilo = match scelto {
        Some(id) => profili.into_iter().find(|p| p.id == id),
        None => profili.into_iter().next(),
    };
    profilo.ok_or_else(|| AppError::new(ErrorCode::IaNotConfigured))
}

/// Un client per questo profilo, con la chiave presa un istante prima.
fn cliente(profilo: &Profilo) -> Result<Cliente, AppError> {
    let chiave = if profilo.con_chiave {
        portachiavi().leggi(&profilo.voce_portachiavi())?
    } else {
        None
    };
    Cliente::nuovo(profilo, chiave)
}

/// Lo stato completo, come lo restituisce ogni comando che cambia qualcosa.
///
/// Tutti lo restituiscono apposta, come nello scrobbling: non esiste un istante
/// in cui la schermata mostra la situazione di prima.
fn stato_ipc(stato: &State<'_, Stato>, ia: &State<'_, StatoIa>) -> Result<StatoIaIpc, AppError> {
    let profili = leggi_profili(stato)?;
    let scelto = con_libreria(stato, |libreria| {
        settings::read(&libreria.connection, CHIAVE_ATTIVO)
    })?;
    // Un id che non corrisponde più a niente vale come nessuno: mostrare
    // «attivo: claude» quando quel profilo è stato eliminato altrove farebbe
    // credere che la chat sia pronta.
    let attivo = scelto
        .filter(|id| profili.iter().any(|p| &p.id == id))
        .or_else(|| profili.first().map(|p| p.id.clone()));
    Ok(StatoIaIpc {
        profili,
        attivo,
        occupato: ia.occupato(),
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn una_conversazione_alla_volta() {
        let ia = StatoIa::nuovo();
        let (primo, _) = ia.prendi_posto().expect("il primo turno prende il posto");
        assert!(ia.occupato());
        assert!(
            ia.prendi_posto().is_none(),
            "il secondo turno non deve passare"
        );
        ia.lascia_posto(primo);
        assert!(!ia.occupato());
        assert!(ia.prendi_posto().is_some(), "dopo, il posto è libero");
    }

    /// Senza il controllo sul numero, un filo che finisce tardi libererebbe il
    /// posto di un turno cominciato dopo di lui.
    #[test]
    fn un_turno_finito_tardi_non_libera_il_posto_di_un_altro() {
        let ia = StatoIa::nuovo();
        let (primo, _) = ia.prendi_posto().expect("primo");
        ia.lascia_posto(primo);
        let (secondo, _) = ia.prendi_posto().expect("secondo");
        ia.lascia_posto(primo);
        assert!(ia.occupato(), "il secondo turno tiene ancora il posto");
        ia.lascia_posto(secondo);
        assert!(!ia.occupato());
    }

    #[test]
    fn i_numeri_dei_turni_non_si_riusano() {
        let ia = StatoIa::nuovo();
        let mut visti = Vec::new();
        for _ in 0..5_u8 {
            let (numero, _) = ia.prendi_posto().expect("il posto è libero ogni volta");
            assert!(!visti.contains(&numero), "{numero} già visto");
            visti.push(numero);
            ia.lascia_posto(numero);
        }
    }

    #[test]
    fn fermare_vale_solo_sul_turno_in_corso() {
        let ia = StatoIa::nuovo();
        let (numero, bandiera) = ia.prendi_posto().expect("un turno");
        assert!(
            !ia.ferma(numero.wrapping_add(1)),
            "un altro numero non ferma"
        );
        assert!(!bandiera.load(Ordering::Relaxed));
        assert!(ia.ferma(numero));
        assert!(bandiera.load(Ordering::Relaxed));

        ia.lascia_posto(numero);
        assert!(!ia.ferma(numero), "un turno finito non si ferma più");
    }
}
