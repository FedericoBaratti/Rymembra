//! La coda che procura i desiderati, dal lato della finestra.
//!
//! Quattro comandi, sei eventi e un filo di sottofondo. Le decisioni stanno in
//! `aether-domain` (quale file), il dialogo con i cataloghi in
//! `aether-catalogo`, il database in `aether-app`: qui si tiene il tempo e si
//! prende il lucchetto — due cose che nessuno di quei tre sa fare, e che sono
//! anche le due che possono far impuntare la riproduzione.
//!
//! # Si chiama «procura» e non «scarica», ed è la cosa da leggere per prima
//!
//! Perché adesso il prelievo è **un** esito possibile fra tre, e non più quello
//! per cui la coda esiste. Una riga può finire così:
//!
//! - si è trovata in un catalogo che permette di tenerla → si prende;
//! - si è trovata solo dove si ascolta e basta, o da nessuna parte →
//!   `introvabile`, che qui vuol dire **«nessuna fonte lecita ce l'ha»** e
//!   diventa una riga della lista di ciò che resta da comprare;
//! - qualcosa è andato storto → `fallito`, e si riprova.
//!
//! La seconda voce è nuova e non è un ripiego: è la risposta onesta alla
//! domanda che il vecchio scaricamento si permetteva di non fare.
//!
//! # L'ordine dei lucchetti
//!
//! Come `nuvola` dichiara il suo, questo modulo dichiara il proprio:
//!
//! > **Il filo della coda prende il lucchetto della libreria solo in finestre
//! > brevi, e mai mentre un prelievo è in corso.**
//!
//! Il ciclo alterna in modo stretto:
//!
//! ```text
//! con_libreria → leggi un lotto di desiderati  → rilascia
//! (senza lucchetto) cerca nei cataloghi, preleva, scrivi i tag
//! con_libreria → segna l'esito                 → rilascia
//! ```
//!
//! Qui la regola pesa più che altrove. Una richiesta di rete dura un secondo;
//! **un concerto in FLAC dura minuti**, e la coda ne fa cento di fila. Tenere il
//! lucchetto per tutta la coda vorrebbe dire un'applicazione che per un'ora non
//! risponde: niente riproduzione, niente ricerca, niente scansione.
//!
//! Che sia vero non è affidato all'attenzione di chi legge: `aether-catalogo`
//! non riceve mai una `rusqlite::Connection`, quindi il codice che terrebbe il
//! lucchetto durante un prelievo non si può nemmeno scrivere.
//!
//! # Perché tre fili e non uno, e non dieci
//!
//! Uno solo lascia il collegamento vuoto. Dieci si prendono un `429` da un
//! servizio pubblico che ci ospita gratis, e a quel punto aspettano tutti e
//! dieci — con l'aggravante che a rimetterci è chi quel servizio lo regge.
//!
//! # Perché un `AtomicBool` e non un canale
//!
//! Stessa ragione scritta per esteso in [`crate::stato`] a proposito di
//! `annulla_scansione`: chi chiede di fermarsi non deve mettersi in coda dietro
//! ciò che vuole fermare.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use aether_app::desiderati::{self, Desiderato};
use aether_app::settings;
use aether_app::tag_scrittura::scrivi_tag;
use aether_catalogo::Cataloghi;
use aether_domain::destinazione::destinazione;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{Disponibilita, Fonte};
use aether_domain::paths::PathRules;
use aether_domain::scelta::{Candidato, affidabilita, scarto_durata, scegli_candidato};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, State};

use crate::errore::{Esito, errore};
use crate::nota;
use crate::stato::{Stato, con_libreria};

/// Quanti brani si procurano insieme.
const FILI: usize = 3;

/// Quanti desiderati si leggono per volta.
///
/// Un lotto e non tutta la tabella: la coda può essere di migliaia di righe, e
/// tenerle in memoria per rileggerne lo stato subito dopo non serve a niente.
/// Un lotto per giro tiene la finestra del lucchetto corta e permette che un
/// «riprova i falliti» arrivato a metà entri nel giro successivo.
const LOTTO: usize = 24;

/// Ogni quanti punti percentuali si manda un evento per il brano in corso.
///
/// Un prelievo annuncia l'avanzamento a ogni blocco da 64 KiB, cioè decine di
/// volte al secondo. Mandarne uno per annuncio inonderebbe il canale IPC per
/// muovere una barra di meno di un pixel — è la stessa strozzatura di
/// `riordino` e `comandi`, con la stessa ragione.
const PASSO_FRAZIONE: u32 = 2;

// ── lo stato condiviso ──────────────────────────────────────────────────────

/// Lo stato della coda che procura.
pub struct StatoProcura {
    /// Una coda sta girando. Impedisce che due partano insieme.
    ///
    /// Non è pignoleria: due code che leggono lo stesso lotto prenderebbero gli
    /// stessi brani due volte, sullo stesso percorso, con due prelievi che
    /// scrivono lo stesso file.
    in_corso: AtomicBool,
    /// Qualcuno ha chiesto di fermarsi.
    da_fermare: AtomicBool,
    /// Quanti ne restano, per chi chiede lo stato senza aspettare un evento.
    rimasti: AtomicU32,
    /// Quanti ne sono stati presi in questa passata.
    fatti: AtomicU32,
    /// I cataloghi, con la loro riserva di connessioni.
    ///
    /// Vivono qui e non dentro la passata per la stessa ragione per cui i
    /// `Fornitori` dell'arricchimento vivono nello stato: una riserva ricreata a
    /// ogni giro è un saluto TLS per brano invece che uno per sessione, contro
    /// servizi pubblici che ci ospitano gratis.
    cataloghi: Cataloghi,
}

impl StatoProcura {
    /// Lo stato, con i cataloghi di serie.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            in_corso: AtomicBool::new(false),
            da_fermare: AtomicBool::new(false),
            rimasti: AtomicU32::new(0),
            fatti: AtomicU32::new(0),
            cataloghi: Cataloghi::nuovi(),
        }
    }

    /// Prova a prendere il posto della coda. `false` se ce n'è già una.
    fn prendi_posto(&self) -> bool {
        self.in_corso
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Lascia il posto.
    fn lascia_posto(&self) {
        self.in_corso.store(false, Ordering::Release);
    }

    /// Chiede alla coda in corso di fermarsi.
    pub fn ferma(&self) {
        self.da_fermare.store(true, Ordering::Relaxed);
    }

    /// Se qualcuno ha chiesto di fermarsi.
    fn fermata(&self) -> bool {
        self.da_fermare.load(Ordering::Relaxed)
    }
}

impl Default for StatoProcura {
    fn default() -> Self {
        Self::nuovo()
    }
}

// ── quel che attraversa l'IPC ───────────────────────────────────────────────

/// Un catalogo attivo, come lo vede chi guarda.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogoAttivo {
    /// Il nome stabile: `internet-archive`, `jamendo`, `audius`.
    ///
    /// L'etichetta che si legge non viaggia: la mette il frontend, dal
    /// catalogo delle lingue, perché è testo dell'interfaccia.
    pub nome: &'static str,
    /// Da qui si può tenere una copia, o solo ascoltare.
    pub consegna: bool,
}

/// Come sta la coda.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoCoda {
    /// Sta girando.
    pub attiva: bool,
    /// Quanti sono stati presi in questa passata.
    pub fatti: u32,
    /// Quanti ne restano da prendere.
    pub rimasti: u32,
    /// I conteggi per stato, dal database.
    pub conteggi: desiderati::Conteggi,
    /// Gli stessi conteggi, ma una riga per importazione.
    ///
    /// Il totale della coda non basta a chi ha importato due elenchi: dice
    /// «31 su 74» e nasconde quale dei due sta scendendo. Vedi
    /// [`desiderati::per_sorgente`].
    pub sorgenti: Vec<desiderati::Sorgente>,
    /// Da dove Aether prende la musica, adesso.
    ///
    /// Sostituisce il vecchio `ytdlp: bool`, e la differenza non è cosmetica:
    /// là c'era una cosa che poteva **mancare**, e mezza interfaccia esisteva
    /// per dirlo. Qui non manca niente — i cataloghi sono compilati dentro — e
    /// l'elenco serve a un'altra cosa, che prima non si poteva fare: dire da
    /// dove arriva quel che si sta scaricando.
    pub cataloghi: Vec<CatalogoAttivo>,
    /// Si accettano registrazioni diverse da quella chiesta.
    pub alternative: bool,
}

/// L'avanzamento della coda nel suo insieme, e di ogni importazione.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Avanzamento {
    fatti: u32,
    totale: u32,
    sorgenti: Vec<desiderati::Sorgente>,
}

/// Il file che la coda ha scelto per un brano di cui aveva solo i nomi.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Scelto {
    /// Il titolo, come sta nel catalogo.
    titolo: String,
    /// Chi lo pubblica. `null` quando il catalogo non lo dà.
    autore: Option<String>,
    /// Da quale catalogo: il nome stabile di [`Fonte`].
    fonte: &'static str,
    /// Sotto che licenza sta.
    licenza: String,
    /// Che registrazione è: `studio`, `dalVivo`, `alternativa`.
    ///
    /// È il campo che rende onesta l'intera funzione. Un catalogo di concerti
    /// risponde con dei concerti, e prenderne uno per la versione in studio
    /// senza dirlo sarebbe scrivere in libreria una cosa per un'altra.
    natura: &'static str,
    /// Quanto è affidabile chi pubblica: `nomeAutore`, `verificata`, `ignota`.
    affidabilita: &'static str,
    /// Scarto fra la durata del file e quella dichiarata, in ms.
    ///
    /// **Firmato**: positivo se il file è più lungo. Un `+8 s` è
    /// un'introduzione o una coda che sfuma; un `−8 s` è una versione tagliata.
    /// Il valore assoluto direbbe la metà della cosa. `null` quando una delle
    /// due durate non si sa.
    scarto_ms: Option<i64>,
    /// La pagina d'origine, da mostrare accanto al brano.
    pagina: Option<String>,
}

/// Cosa sta succedendo a un brano.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventoBrano {
    /// Il titolo secondo la fonte che l'ha nominato.
    titolo: String,
    /// L'interprete.
    artista: Option<String>,
    /// Da 0 a 1 mentre arriva; `None` quando sta ancora cercando.
    frazione: Option<f32>,
    /// `cerco`, `prendo`, `fatto`, `fallito`, `introvabile`.
    esito: &'static str,
    /// Il codice del catalogo degli errori, quando è andata male.
    codice: Option<String>,
    /// Da quale importazione viene: l'identificativo del contenitore.
    ///
    /// Senza, un elenco di importazioni non saprebbe sotto quale riga mettere
    /// «sto cercando…», e lo metterebbe sotto tutte.
    sorgente_id: String,
    /// Il nome di quel contenitore.
    provenienza: String,
    /// Il file scelto, e perché quello.
    ///
    /// `null` in due casi che chi legge deve distinguere: da un link di un
    /// catalogo la scelta non c'è stata — il file era già noto e la coda salta
    /// la ricerca — e mentre `esito` è `cerco` non è ancora stata fatta.
    scelto: Option<Scelto>,
}

// ── i comandi ───────────────────────────────────────────────────────────────

/// Avvia la coda.
///
/// Torna subito: la coda gira su un filo suo. Chi la mostra segue gli eventi
/// `scarico:avanzamento` e `scarico:brano`.
#[tauri::command]
pub fn scarica_desiderati(app: AppHandle) -> Esito<StatoCoda> {
    avvia(&app);
    scarico_stato(app.state(), app.state())
}

/// Chiede alla coda in corso di fermarsi.
///
/// Non chiede il lucchetto della libreria — e non può: quel lucchetto ce l'ha
/// la coda, a intermittenza, per tutta la sua durata. Alza un bit, e la coda lo
/// legge fra un brano e l'altro e mentre un prelievo scende.
#[tauri::command]
pub fn annulla_scarico(procura: State<'_, StatoProcura>) -> Esito<()> {
    procura.ferma();
    Ok(())
}

/// Come sta la coda.
///
/// **Non tocca la rete.** L'elenco dei cataloghi è quello compilato dentro, non
/// quello che risponde adesso: questo comando lo chiama ogni finestra che si
/// apre, e farne una sonda di rete vorrebbe dire un'interfaccia che aspetta
/// `archive.org` per disegnare un pannello. Chi vuole sapere chi risponde chiama
/// `import_diagnostica`, che è fatto apposta e lo dichiara.
#[tauri::command]
pub fn scarico_stato(
    stato: State<'_, Stato>,
    procura: State<'_, StatoProcura>,
) -> Esito<StatoCoda> {
    // Le tre letture sotto lo **stesso** lucchetto: prenderlo due volte
    // lascerebbe passare in mezzo un brano che si conclude, e i totali
    // direbbero una cosa mentre le righe per importazione ne dicono un'altra.
    let (conteggi, sorgenti, alternative) = con_libreria(&stato, |libreria| {
        Ok((
            desiderati::conteggi(&libreria.connection)?,
            desiderati::per_sorgente(&libreria.connection)?,
            alternative_ammesse(&libreria.connection),
        ))
    })
    .map_err(errore)?;
    Ok(StatoCoda {
        attiva: procura.in_corso.load(Ordering::Acquire),
        fatti: procura.fatti.load(Ordering::Relaxed),
        rimasti: procura.rimasti.load(Ordering::Relaxed),
        conteggi,
        sorgenti,
        cataloghi: cataloghi_attivi(),
        alternative,
    })
}

/// Accetta o rifiuta le registrazioni diverse da quella chiesta.
///
/// # Perché è un interruttore e non una decisione presa una volta
///
/// Perché non c'è una risposta giusta per tutti. I cataloghi liberi sono fatti
/// in gran parte di concerti: chi vuole ascoltare quella canzone li accetta
/// volentieri, chi sta ricostruendo un disco preciso no. Spento, la coda torna
/// severa e trova molto meno — ed è quel che alcune persone vogliono.
///
/// **Non tocca quel che è già stato preso.** Un live scaricato ieri resta in
/// libreria: questo decide cosa fa la coda da adesso in poi, e un interruttore
/// che cancellasse dei file sarebbe un interruttore che nessuno oserebbe
/// toccare.
#[tauri::command]
pub fn alternative_ammettile(
    stato: State<'_, Stato>,
    procura: State<'_, StatoProcura>,
    ammesse: bool,
) -> Esito<StatoCoda> {
    con_libreria(&stato, |libreria| {
        settings::write(
            &libreria.connection,
            settings::CHIAVE_ALTERNATIVE,
            if ammesse { "1" } else { "0" },
        )
    })
    .map_err(errore)?;
    scarico_stato(stato, procura)
}

/// Quel che resta da comprare.
///
/// # Perché è un comando e non un campo di [`StatoCoda`]
///
/// Perché `scarico_stato` lo chiama ogni finestra che si apre e ogni volta che
/// un brano si conclude — decine di volte in una coda — e questa lista può
/// essere lunga come la libreria di qualcuno. Attaccarla lì vorrebbe dire
/// rileggerla e serializzarla intera a ogni brano, per mostrarla in una sezione
/// che di norma è chiusa.
///
/// Il `limite` è obbligatorio per la stessa ragione per cui lo è in
/// `import_rapporti`: `desiderati` non cancella mai niente, quindi questo
/// elenco cresce per sempre, e chiedere solo quel che si mostra è l'unica
/// disciplina che regge nel tempo.
#[tauri::command]
pub fn da_comprare(stato: State<'_, Stato>, limite: u32) -> Esito<Vec<desiderati::DaComprare>> {
    con_libreria(&stato, |libreria| {
        desiderati::da_comprare(&libreria.connection, limite)
    })
    .map_err(errore)
}

/// Dove si compra un brano che nessun catalogo libero ha.
///
/// # Perché l'indirizzo si costruisce **qui** e non nella finestra
///
/// Perché aprire un indirizzo nel browser di sistema è l'unica capacità
/// pericolosa che questa applicazione concede alla sua pagina
/// (`opener:allow-open-url` in `capabilities/default.json`). Se l'indirizzo
/// arrivasse dalla finestra, una pagina compromessa potrebbe farne aprire uno
/// qualunque; arrivando da qui, quel che si può aprire sono i tre domini di
/// questo elenco e nient'altro — la finestra manda **cosa cercare**, non dove.
///
/// # Perché questi tre
///
/// **Bandcamp** per primo perché è quello in cui la quota che arriva
/// all'artista è più alta, e questa lista esiste esattamente per mandare i
/// soldi a chi ha fatto la musica. **Qobuz** perché consegna file — FLAC, che
/// entrano in libreria e restano — e non un abbonamento che scade. **Discogs**
/// per ultimo perché è quello che trova le cose fuori catalogo, che sono
/// precisamente quelle che nessun catalogo libero aveva.
const NEGOZI: &[(&str, &str)] = &[
    ("bandcamp", "https://bandcamp.com/search?q="),
    ("qobuz", "https://www.qobuz.com/it-it/search?q="),
    ("discogs", "https://www.discogs.com/search/?type=release&q="),
];

/// I caratteri che si possono lasciar passare in una stringa di ricerca.
///
/// Tutto il resto va in percentuale. La lista è corta di proposito: allargarla
/// per far stare un apostrofo vorrebbe dire ragionare, una volta per carattere,
/// su cosa quel carattere fa in un indirizzo — e il costo di sbagliarne uno è
/// una ricerca che va in un posto che non è quello che si voleva.
const fn sicuro(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

/// Apre la ricerca di un brano su un negozio.
///
/// Il negozio si nomina, non si indirizza: un nome che non è nell'elenco è un
/// rifiuto, non un indirizzo aperto per caso.
#[tauri::command]
pub fn cerca_dove_comprare(negozio: String, cosa: String) -> Esito<()> {
    let Some((_, base)) = NEGOZI.iter().find(|(nome, _)| *nome == negozio) else {
        return Err(errore(
            AppError::new(ErrorCode::DownloadUnrecognizedUrl)
                .with_cause(format!("negozio sconosciuto: {negozio}")),
        ));
    };

    let mut url = String::with_capacity(base.len() + cosa.len() * 3);
    url.push_str(base);
    for byte in cosa.trim().bytes() {
        if sicuro(byte) {
            url.push(char::from(byte));
        } else if byte == b' ' {
            url.push('+');
        } else {
            // `{:02X}` e non `{:X}`: un byte sotto 0x10 scritto con una cifra
            // sola mangerebbe il carattere successivo dentro la sua sequenza.
            url.push_str(&format!("%{byte:02X}"));
        }
    }

    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|err| {
        errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura del browser".to_owned()),
            })
            .with_cause(err.to_string()),
        )
    })
}

/// Rimette in coda i falliti e riavvia.
#[tauri::command]
pub fn riprova_falliti(app: AppHandle, stato: State<'_, Stato>) -> Esito<u32> {
    let quanti = con_libreria(&stato, |libreria| {
        desiderati::riprova_falliti(&libreria.connection)
    })
    .map_err(errore)?;
    if quanti > 0 {
        avvia(&app);
    }
    Ok(quanti)
}

/// I cataloghi compilati in questa build.
///
/// Scritto a mano e non dedotto: quali cataloghi ci siano dipende dalle feature
/// di `aether-catalogo`, e un elenco che si costruisse da sé nasconderebbe il
/// fatto che spegnerne uno è una decisione — quella di Jamendo, per esempio, è
/// legata a una clausola non commerciale e va presa da chi distribuisce.
fn cataloghi_attivi() -> Vec<CatalogoAttivo> {
    vec![
        CatalogoAttivo {
            nome: Fonte::InternetArchive.nome(),
            consegna: Fonte::InternetArchive.puo_consegnare(),
        },
        // `consegna` è `true` per Audius, e vuol dire «può, in generale»: là
        // decide l'artista brano per brano, quindi questa riga dice cosa la
        // fonte permette e non cosa permetterà quel singolo pezzo. È la stessa
        // distinzione che `Disponibilita::decidi` fa un gradino più sotto.
        CatalogoAttivo {
            nome: Fonte::Audius.nome(),
            consegna: Fonte::Audius.puo_consegnare(),
        },
    ]
}

/// Si accettano registrazioni diverse da quella chiesta.
///
/// Assente vale sì: vedi [`settings::CHIAVE_ALTERNATIVE`].
fn alternative_ammesse(connection: &rusqlite::Connection) -> bool {
    settings::read(connection, settings::CHIAVE_ALTERNATIVE)
        .ok()
        .flatten()
        .is_none_or(|valore: String| valore != "0" && valore != "false")
}

// ── il filo ─────────────────────────────────────────────────────────────────

/// Avvia la coda, se non ce n'è già una e se c'è qualcosa da fare.
///
/// Pubblico perché lo chiama anche [`crate::importa::import_esegui`]: è l'«in
/// modo automatico» chiesto — confermata un'importazione, i brani mancanti
/// cominciano a essere cercati da soli.
pub fn avvia(app: &AppHandle) {
    let procura = app.state::<StatoProcura>();
    if !procura.prendi_posto() {
        // Ce n'è già una. I brani appena importati li prenderà lei al prossimo
        // lotto: la coda rilegge la tabella a ogni giro, quindi non si perde
        // niente e non serve accodare una seconda passata.
        //
        // Si annuncia lo stesso, però: chi ha appena confermato la seconda
        // importazione deve vederla comparire adesso, non fra un minuto quando
        // finisce il brano che era in corso. Un elenco che non nomina quel che
        // hai appena fatto assomiglia a un'importazione non riuscita.
        annuncia(app);
        return;
    }
    // Prima di partire: un annullamento arrivato dopo la fine della coda
    // precedente fermerebbe questa al primo brano.
    procura.da_fermare.store(false, Ordering::Relaxed);
    procura.fatti.store(0, Ordering::Relaxed);

    let manico = app.clone();
    let avviato = std::thread::Builder::new()
        .name("aether-procura".to_owned())
        .spawn(move || {
            passata(&manico);
            manico.state::<StatoProcura>().lascia_posto();
            let _ = manico.emit("scarico:finito", ());
        });
    if let Err(err) = avviato {
        nota!("[procura] il filo della coda non è partito: {err}");
        procura.lascia_posto();
    }
}

/// Una passata completa: legge lotti finché ce n'è, o finché non si ferma.
fn passata(app: &AppHandle) {
    let procura = app.state::<StatoProcura>();

    let Some(cartella) = cartella_download(app) else {
        segnala_guasto(
            app,
            &AppError::new(ErrorCode::FsNotFound {
                path: "cartella di scaricamento".to_owned(),
            })
            .with_cause(
                "nessuna cartella sorvegliata: i file presi non sarebbero trovati da nessuna scansione"
                    .to_owned(),
            ),
        );
        return;
    };
    let temporanea = cartella_temporanea(app);
    let alternative = con_libreria(&app.state::<Stato>(), |libreria| {
        Ok(alternative_ammesse(&libreria.connection))
    })
    .unwrap_or(true);

    // Un annuncio prima di cominciare. Serve a chi apre l'elenco subito dopo un
    // riavvio con una coda rimasta in sospeso: senza, i numeri resterebbero
    // quelli dell'interrogazione iniziale finché non finisce il primo brano.
    annuncia(app);

    let mut presi_qualcosa = false;
    loop {
        if procura.fermata() {
            break;
        }
        let lotto = match con_libreria(&app.state::<Stato>(), |libreria| {
            desiderati::leggi_da_scaricare(&libreria.connection, LOTTO)
        }) {
            Ok(lotto) => lotto,
            Err(guasto) => {
                segnala_guasto(app, &guasto);
                break;
            }
        };
        if lotto.is_empty() {
            break;
        }
        presi_qualcosa = true;
        esegui_lotto(app, &cartella, &temporanea, alternative, lotto);
    }

    // La scansione finale, e solo se qualcosa è arrivato: è ciò che fa entrare i
    // file in libreria da soli. Senza, si vedrebbe la coda finire e la libreria
    // ferma, e toccherebbe indovinare che bisogna premere «Scansiona».
    if presi_qualcosa && procura.fatti.load(Ordering::Relaxed) > 0 {
        rientra_in_libreria(app);
    }
}

/// Procura un lotto, con [`FILI`] brani insieme.
fn esegui_lotto(
    app: &AppHandle,
    cartella: &Path,
    temporanea: &Path,
    alternative: bool,
    lotto: Vec<Desiderato>,
) {
    let procura = app.state::<StatoProcura>();
    let totale = totale_da_fare(app);
    procura.rimasti.store(totale, Ordering::Relaxed);

    // Una coda condivisa e non una fetta per filo: i brani non durano uguale, e
    // dividerli in tre parti uguali lascerebbe due fili fermi ad aspettare il
    // terzo che è capitato sui file lunghi.
    let coda = Arc::new(Mutex::new(lotto));
    let quanti = FILI.min(coda.lock().map_or(1, |c| c.len()).max(1));

    std::thread::scope(|ambito| {
        for _ in 0..quanti {
            let coda = Arc::clone(&coda);
            ambito.spawn(move || {
                loop {
                    if app.state::<StatoProcura>().fermata() {
                        break;
                    }
                    let Some(desiderato) = coda.lock().ok().and_then(|mut c| c.pop()) else {
                        break;
                    };
                    un_brano(app, cartella, temporanea, alternative, &desiderato);
                }
            });
        }
    });
}

/// Un brano solo, dall'inizio alla fine.
///
/// Ogni esito passa da uno dei tre `segna_*`: una riga che resta in `attesa`
/// senza che nessuno l'abbia toccata tornerebbe nel lotto successivo, e la coda
/// girerebbe su di lei per sempre.
fn un_brano(
    app: &AppHandle,
    cartella: &Path,
    temporanea: &Path,
    alternative: bool,
    desiderato: &Desiderato,
) {
    let procura = app.state::<StatoProcura>();
    let fermare = || app.state::<StatoProcura>().fermata();
    emetti_brano(app, desiderato, None, "cerco", None, None);

    // ── la scelta, fuori da ogni lucchetto ──────────────────────────────────
    let scelto = match trova(app, desiderato, alternative, &fermare) {
        Ok(Some(scelto)) => scelto,
        Ok(None) => {
            // «Ho cercato e nessuna fonte lecita ce l'ha» è terminale, e va
            // distinto da «non sono riuscito a cercare», che è passeggero.
            // Questa riga finisce nella lista di ciò che resta da comprare.
            let motivo = AppError::new(ErrorCode::DownloadNoResults);
            segna(app, desiderato, |connection| {
                desiderati::segna_introvabile(connection, desiderato.id, &codice_e_causa(&motivo))
            });
            emetti_brano(app, desiderato, None, "introvabile", Some(&motivo), None);
            annuncia(app);
            return;
        }
        Err(guasto) => return chiudi_male(app, desiderato, &guasto),
    };

    let cartellino = cartellino(&scelto, desiderato);
    emetti_brano(
        app,
        desiderato,
        Some(0.0),
        "prendo",
        None,
        Some(cartellino.clone()),
    );

    // ── il prelievo, fuori da ogni lucchetto ────────────────────────────────
    // La posizione si conta da 1: `posizione` è l'indice nel contenitore.
    let numero = desiderato.posizione.saturating_add(1);
    let dove = destinazione(&desiderato.brano, numero);
    let mut ultima = 0_u32;
    let esito = procura.cataloghi.preleva(
        &scelto,
        &aether_catalogo::Richiesta {
            cartella_download: cartella,
            cartella_temporanea: temporanea,
            base_relativa: &dove.relativo(),
        },
        &fermare,
        &mut |frazione| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "la frazione arriva già limitata fra 0 e 1"
            )]
            let percento = (frazione * 100.0).clamp(0.0, 100.0) as u32;
            if percento >= ultima.saturating_add(PASSO_FRAZIONE) || percento >= 100 {
                ultima = percento;
                emetti_brano(
                    app,
                    desiderato,
                    Some(frazione),
                    "prendo",
                    None,
                    Some(cartellino.clone()),
                );
            }
        },
    );

    let preso = match esito {
        Ok(preso) => preso,
        // Un «no» non è un guasto: la licenza non cambierà riprovando, e la
        // riga va in fondo alla lista di ciò che si compra invece di bruciare
        // tre tentativi per sentirsi dire tre volte la stessa cosa.
        Err(guasto) if matches!(guasto.code(), ErrorCode::DownloadNotPermitted { .. }) => {
            segna(app, desiderato, |connection| {
                desiderati::segna_introvabile(connection, desiderato.id, &codice_e_causa(&guasto))
            });
            emetti_brano(
                app,
                desiderato,
                None,
                "introvabile",
                Some(&guasto),
                Some(cartellino),
            );
            annuncia(app);
            return;
        }
        Err(guasto) => return chiudi_male(app, desiderato, &guasto),
    };

    // ── i tag di chi l'ha nominato, sopra quelli del catalogo ───────────────
    // Un guasto qui **non** annulla il prelievo: il file c'è ed è ascoltabile, e
    // rimetterlo in coda vorrebbe dire riprenderlo per riscrivere un campo. Si
    // annota e si prosegue.
    let copertina = copertina(app, desiderato);
    let da_scrivere = con_attribuzione(&desiderato.brano, preso.attribuzione.as_deref());
    if let Err(guasto) = scrivi_tag(&preso.percorso, &da_scrivere, numero, copertina.as_deref()) {
        // L'estensione e non il percorso: il diario si spedisce, e un percorso
        // di download porta dentro il nome dell'account di Windows e — visto
        // come `riordino` costruisce le cartelle — l'artista e l'album. Quel
        // che serve a capire perché un tag non si scrive è **quale formato**
        // era, che è la differenza fra un FLAC e un contenitore che `lofty` non
        // sa riscrivere. Il resto lo dice `{guasto}`.
        nota!(
            "[procura] tag non scritti su un .{}: {guasto}",
            preso
                .percorso
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("senza estensione")
        );
    }

    segna(app, desiderato, |connection| {
        desiderati::segna_fatto(
            connection,
            desiderato.id,
            &preso.percorso.display().to_string(),
            &scelto.url,
            &preso.licenza,
        )
    });
    procura.fatti.fetch_add(1, Ordering::Relaxed);
    emetti_brano(app, desiderato, Some(1.0), "fatto", None, Some(cartellino));
    annuncia(app);
}

/// Il file da prendere: quello già scelto, o il migliore che i cataloghi danno.
///
/// `Ok(None)` è l'esito terminale «nessuna fonte lecita ce l'ha», distinto da un
/// `Err`, che è «non sono riuscito a chiedere».
fn trova(
    app: &AppHandle,
    desiderato: &Desiderato,
    alternative: bool,
    fermare: &dyn Fn() -> bool,
) -> Result<Option<Candidato>, AppError> {
    // Chi ha importato ha visto *quell'elenco*: rifare la scelta vorrebbe dire
    // prendere una registrazione diversa da quella che aveva davanti.
    if let Some(url) = desiderato
        .brano
        .fonte_url
        .as_deref()
        .filter(|u| !u.is_empty())
        && desiderato.brano.disponibilita == Disponibilita::Scaricabile
    {
        return Ok(Some(Candidato {
            url: url.to_owned(),
            titolo: desiderato.brano.title.clone(),
            autore: desiderato.brano.artist.clone(),
            durata_sec: desiderato.brano.duration_ms.and_then(|ms| {
                #[expect(
                    clippy::integer_division,
                    reason = "i millisecondi che avanzano non cambiano una fascia di durata"
                )]
                let secondi = ms / 1000;
                u32::try_from(secondi).ok()
            }),
            fonte: desiderato.fonte,
            licenza: desiderato.brano.licenza.clone(),
            disponibilita: desiderato.brano.disponibilita,
            pagina: desiderato.brano.pagina_url.clone(),
            ..Candidato::default()
        }));
    }

    let procura = app.state::<StatoProcura>();
    let candidati = procura.cataloghi.cerca(&desiderato.brano, fermare)?;
    // Solo quel che si può tenere: un file che si ascolta e basta non è una
    // risposta a «mettimelo in libreria», e sceglierlo per poi rifiutarlo un
    // passo dopo brucerebbe un tentativo per niente.
    let prendibili: Vec<Candidato> = candidati
        .into_iter()
        .filter(Candidato::si_puo_tenere)
        .collect();
    Ok(scegli_candidato(&prendibili, &desiderato.brano, alternative).cloned())
}

/// Che cosa dire a chi guarda, del file scelto.
fn cartellino(scelto: &Candidato, desiderato: &Desiderato) -> Scelto {
    Scelto {
        titolo: scelto.titolo.clone(),
        autore: scelto.autore.clone(),
        fonte: scelto.fonte.nome(),
        licenza: scelto.licenza.nome(),
        natura: scelto.natura.nome(),
        affidabilita: affidabilita(
            scelto.autore.as_deref(),
            scelto.autore_verificato,
            desiderato.brano.artist.as_deref(),
        )
        .nome(),
        scarto_ms: scarto_durata(desiderato.brano.duration_ms, scelto.durata_sec),
        pagina: scelto.pagina.clone(),
    }
}

/// I metadati da scrivere nel file, con l'attribuzione dentro.
///
/// L'attribuzione finisce nel commento del brano e non solo a schermo, e non è
/// pedanteria: il file sopravvive all'applicazione. Fra due anni, su un altro
/// computer, quel campo è l'unica cosa che dice sotto quale patto quel brano è
/// finito lì.
fn con_attribuzione(
    brano: &aether_domain::BranoEsterno,
    attribuzione: Option<&str>,
) -> aether_domain::BranoEsterno {
    let mut copia = brano.clone();
    if let Some(riga) = attribuzione {
        copia.pagina_url = copia.pagina_url.or_else(|| Some(riga.to_owned()));
    }
    copia
}

/// Chiude una riga andata male, distinguendo i due modi.
fn chiudi_male(app: &AppHandle, desiderato: &Desiderato, guasto: &AppError) {
    // L'annullamento non è un guasto e non consuma tentativi: chi ha premuto
    // «Annulla» si aspetta di ritrovare la coda dov'era, non accorciata di tre
    // tentativi per ogni brano che stava scendendo.
    if matches!(guasto.code(), ErrorCode::InternalAborted { .. }) {
        return;
    }

    let dettaglio = codice_e_causa(guasto);
    let ritentabile = guasto.is_retryable();
    segna(app, desiderato, |connection| {
        desiderati::segna_fallito(connection, desiderato.id, &dettaglio, ritentabile)
    });
    emetti_brano(app, desiderato, None, "fallito", Some(guasto), None);
    annuncia(app);
}

/// Prende il lucchetto per il tempo di una `UPDATE`.
fn segna(
    app: &AppHandle,
    desiderato: &Desiderato,
    azione: impl FnOnce(&rusqlite::Connection) -> Result<(), AppError>,
) {
    let esito = con_libreria(&app.state::<Stato>(), |libreria| {
        azione(&libreria.connection)
    });
    if let Err(guasto) = esito {
        // Non si può fare altro che dirlo: se il database non risponde, il
        // brano resterà in `attesa` e verrà ritentato al prossimo giro — che è
        // il comportamento meno sbagliato fra quelli disponibili.
        // L'identificativo e non il titolo, per la ragione del `nota!` qui
        // sopra: un diario che elenca i brani che qualcuno sta procurando è
        // l'elenco di cosa ascolta. Il numero basta a ritrovare la riga nel
        // database di chi segnala, ed è l'unico che possa servire.
        nota!(
            "[procura] esito non registrato per il desiderato {}: {guasto}",
            desiderato.id
        );
    }
}

/// Porta dentro la copertina, se c'è.
///
/// Fuori da ogni lucchetto, come tutto il resto della rete. `None` per qualunque
/// intoppo: un brano senza copertina è un brano, e far fallire un prelievo
/// riuscito perché una miniatura non si scarica sarebbe assurdo.
fn copertina(app: &AppHandle, desiderato: &Desiderato) -> Option<Vec<u8>> {
    let url = desiderato.brano.cover_url.as_deref()?;
    let mut byte = Vec::new();
    let rete = app
        .state::<StatoProcura>()
        .cataloghi
        .archivio()
        .rete()
        .clone();
    rete.preleva(url, &[], &mut byte, &|| false, &mut |_, _| {})
        .ok()?;
    // Otto mebibyte, lo stesso limite che `tag_scrittura` applica quando la
    // incorpora: scaricarne di più vorrebbe dire buttarli via un passo dopo.
    aether_net::immagine::plausibile(&byte, 8 * 1024 * 1024).then_some(byte)
}

// ── gli eventi ──────────────────────────────────────────────────────────────

/// Manda lo stato di un brano alla finestra.
fn emetti_brano(
    app: &AppHandle,
    desiderato: &Desiderato,
    frazione: Option<f32>,
    esito: &'static str,
    guasto: Option<&AppError>,
    scelto: Option<Scelto>,
) {
    let _ = app.emit(
        "scarico:brano",
        EventoBrano {
            titolo: desiderato.brano.title.clone(),
            artista: desiderato.brano.artist.clone(),
            frazione,
            esito,
            codice: guasto.map(|g| g.code().kind().code().to_owned()),
            sorgente_id: desiderato.sorgente_id.clone(),
            provenienza: desiderato.provenienza.clone(),
            scelto,
        },
    );
}

/// Manda l'avanzamento complessivo e quello di ogni importazione.
///
/// Pubblica perché la chiama anche [`avvia`] quando il posto della coda è già
/// preso: è il solo modo perché un'importazione confermata a coda in corsa
/// compaia subito nell'elenco invece che alla fine del brano in corso.
pub fn annuncia(app: &AppHandle) {
    let procura = app.state::<StatoProcura>();
    let (rimasti, sorgenti) = situazione(app);
    procura.rimasti.store(rimasti, Ordering::Relaxed);
    let fatti = procura.fatti.load(Ordering::Relaxed);
    let _ = app.emit(
        "scarico:avanzamento",
        Avanzamento {
            fatti,
            totale: fatti.saturating_add(rimasti),
            sorgenti,
        },
    );
}

/// Fa sapere alla finestra che la coda non può partire.
fn segnala_guasto(app: &AppHandle, guasto: &AppError) {
    nota!("[procura] la coda non parte: {guasto}");
    let _ = app.emit("scarico:guasto", crate::errore::errore(guasto.clone()));
}

/// Quanti ne restano in tabella.
fn totale_da_fare(app: &AppHandle) -> u32 {
    con_libreria(&app.state::<Stato>(), |libreria| {
        desiderati::conta_in_attesa(&libreria.connection)
    })
    .unwrap_or(0)
}

/// Quanti ne restano e come stanno le importazioni, con un lucchetto solo.
///
/// Due prese separate lascerebbero passare in mezzo un brano concluso da un
/// altro filo, e i due numeri racconterebbero due momenti diversi.
fn situazione(app: &AppHandle) -> (u32, Vec<desiderati::Sorgente>) {
    con_libreria(&app.state::<Stato>(), |libreria| {
        Ok((
            desiderati::conta_in_attesa(&libreria.connection)?,
            desiderati::per_sorgente(&libreria.connection)?,
        ))
    })
    .unwrap_or_default()
}

/// Il codice del catalogo più la causa, per la colonna `download_error`.
///
/// Il codice e non la sola frase: è quello che la finestra sa tradurre, e senza
/// di lui un errore salvato oggi diventerebbe testo opaco al prossimo cambio di
/// lingua.
fn codice_e_causa(guasto: &AppError) -> String {
    match guasto.cause() {
        Some(causa) => format!("{}: {causa}", guasto.code().kind().code()),
        None => guasto.code().kind().code().to_owned(),
    }
}

// ── le cartelle ─────────────────────────────────────────────────────────────

/// Dove finiscono i brani presi.
///
/// L'impostazione se c'è, altrimenti la **prima cartella sorvegliata**. `None`
/// quando non ce n'è nessuna: scrivere in una cartella non sorvegliata
/// produrrebbe file che nessuna scansione trova mai, cioè un prelievo riuscito
/// che non si vede comparire — indistinguibile, da fuori, da uno fallito. Meglio
/// non partire e dirlo.
fn cartella_download(app: &AppHandle) -> Option<PathBuf> {
    con_libreria(&app.state::<Stato>(), |libreria| {
        let scelta: Option<String> =
            settings::read(&libreria.connection, settings::CHIAVE_CARTELLA_DOWNLOAD)?;
        if let Some(scelta) = scelta.filter(|s| !s.trim().is_empty()) {
            return Ok(Some(PathBuf::from(scelta)));
        }
        let cartelle: Vec<String> =
            settings::read_json(&libreria.connection, settings::CHIAVE_CARTELLE)?
                .unwrap_or_default();
        Ok(cartelle.into_iter().next().map(PathBuf::from))
    })
    .ok()
    .flatten()
}

/// Dove stanno i file mentre arrivano.
///
/// Nella cartella dati e **non** dentro quella sorvegliata: un file a metà che
/// la scansione ingoiasse entrerebbe in libreria come un brano normale che non
/// si sente. È la stessa ragione per cui `MIN_TRACK_BYTES` esiste, presa
/// dall'altro capo.
fn cartella_temporanea(app: &AppHandle) -> PathBuf {
    let dati = con_libreria(&app.state::<Stato>(), |libreria| {
        Ok(libreria.data_dir.clone())
    })
    .unwrap_or_else(|_| std::env::temp_dir());
    dati.join("procura")
}

/// Una scansione delle cartelle sorvegliate, così i file entrano in libreria.
///
/// E subito dopo il **viaggio di ritorno**: i brani appena entrati vanno rimessi
/// nelle playlist da cui mancavano. Qui e non altrove perché è l'unico momento in
/// cui esistono tutti e due i capi — la riga di `desiderati` che dice «questa
/// playlist, questo posto» e la riga di `tracks` che il file ha appena creato.
/// Senza questo passo la coda finisce, i file ci sono, e la playlist è ancora
/// quella con i soli brani che c'erano già.
fn rientra_in_libreria(app: &AppHandle) {
    let esito = con_libreria(&app.state::<Stato>(), |libreria| {
        let roots: Vec<String> =
            settings::read_json(&libreria.connection, settings::CHIAVE_CARTELLE)?
                .unwrap_or_default();
        let scan = aether_app::library::Scan {
            files: &aether_app::files::LocalFiles,
            covers: &libreria.covers,
            roots: &roots,
            rules: PathRules::for_current_platform(),
        };
        scan.run(&mut libreria.connection, |_, _| {
            std::ops::ControlFlow::Continue(())
        })
    });
    match esito {
        Ok(report) => {
            let _ = app.emit("scarico:in_libreria", report.inserted);
        }
        Err(guasto) => nota!("[procura] la scansione finale è fallita: {guasto}"),
    }

    // `None`: la scansione ha guardato tutte le cartelle, quindi possono essere
    // arrivati brani di qualunque importazione — anche di una vecchia, se i file
    // sono stati messi a mano nel frattempo. Un guasto qui si annota e basta: i
    // file sono al loro posto, e la passata successiva riprova.
    let ritorno = con_libreria(&app.state::<Stato>(), |libreria| {
        aether_app::desiderati::riconcilia(&libreria.connection, None)
    });
    match ritorno {
        Ok(fatto) if fatto.voci_rimesse > 0 || fatto.righe_chiuse > 0 => {
            let _ = app.emit("scarico:riconciliato", fatto);
        }
        Ok(_) => {}
        Err(guasto) => nota!("[procura] il ritorno nelle playlist è fallito: {guasto}"),
    }
    // Brani nuovi sono statistiche nuove da ancorare, per la stessa ragione
    // scritta in `comandi::scansiona`.
    let _ = crate::nuvola::se_riuscito(app, Ok::<(), ()>(()));
    // E sono anche i brani che hanno più bisogno di essere arricchiti: un file
    // di un catalogo entra col titolo che gli ha dato chi l'ha caricato, che
    // spesso è il nome del file.
    crate::arricchimento::sporca(app);
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn una_coda_alla_volta() {
        // Due code insieme leggerebbero lo stesso lotto e scriverebbero lo
        // stesso file da due prelievi.
        let stato = StatoProcura::nuovo();
        assert!(stato.prendi_posto());
        assert!(!stato.prendi_posto(), "la seconda non deve entrare");
        stato.lascia_posto();
        assert!(stato.prendi_posto(), "finita la prima, si riparte");
    }

    #[test]
    fn lannullamento_si_alza_e_si_legge() {
        let stato = StatoProcura::nuovo();
        assert!(!stato.fermata());
        stato.ferma();
        assert!(stato.fermata());
    }

    #[test]
    fn il_codice_derrore_sopravvive_alla_scrittura() {
        // È quel che la finestra traduce: senza il codice, `download_error`
        // conterrebbe una frase in italiano che resta in italiano per sempre.
        let guasto = AppError::new(ErrorCode::DownloadUnavailable).with_cause("l'item non c'è più");
        assert_eq!(
            codice_e_causa(&guasto),
            "download.unavailable: l'item non c'è più"
        );
        assert_eq!(
            codice_e_causa(&AppError::new(ErrorCode::DownloadNoResults)),
            "download.noResults"
        );
    }

    #[test]
    fn i_cataloghi_dichiarati_sanno_dire_se_consegnano() {
        // L'elenco che sostituisce il vecchio «manca yt-dlp»: non dice se
        // qualcosa manca — non può mancare niente — ma da dove arriva la musica.
        let attivi = cataloghi_attivi();
        assert!(
            !attivi.is_empty(),
            "senza cataloghi la coda non serve a niente"
        );
        assert!(
            attivi.iter().any(|c| c.consegna),
            "almeno uno deve poter consegnare, o non si scarica mai niente"
        );
    }
}
