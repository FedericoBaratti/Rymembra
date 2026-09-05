//! L'importazione da un link, dal lato della finestra.
//!
//! Sei comandi, una famiglia di eventi e una cella. Il dialogo con i cataloghi
//! sta in `aether-catalogo`, l'abbinamento in `aether-domain`, la scrittura in
//! `aether-app`: qui si tiene una cella e si prende il lucchetto — due cose che
//! nessuno di quei tre sa fare.
//!
//! # Che cosa è cambiato, e perché il modulo si è accorciato di trecento righe
//!
//! Prima qui c'erano **due** lettori, ciascuno con la sua cascata di livelli, il
//! suo file di configurazione per correggere le costanti che i servizi ruotano,
//! e la sua diagnostica per spiegare quale dei livelli fosse caduto. Tutta
//! quell'architettura esisteva per una ragione sola: i due servizi che si
//! interrogavano non avevano mai promesso a nessuno di farsi interrogare, e
//! quindi cambiavano senza preavviso.
//!
//! I cataloghi liberi hanno un'API pubblica e documentata. Non c'è un file di
//! configurazione da correggere in fretta perché non c'è niente che possa
//! ruotare sotto i piedi, non c'è una cascata di livelli perché non c'è un
//! livello che possa smettere di funzionare all'improvviso. La diagnostica resta
//! — un servizio può essere giù — ma dice una cosa sola e vera: chi risponde.
//!
//! # L'ordine dei lucchetti
//!
//! Due lucchetti, e si prendono sempre in questo ordine:
//!
//! ```text
//! (senza lucchetti)  leggi dal catalogo
//! cella              metti in cella / copiane il contenuto   → rilascia
//! libreria           scrivi                                  → rilascia
//! ```
//!
//! La cella **non** resta presa mentre si scrive nel database, ed è la ragione
//! per cui [`esegui`] ne fa una copia invece di passare un riferimento.

use std::sync::Mutex;
use std::time::Duration;

use crate::spegnimento::Emette as _;
use aether_app::import_esterno::{self, RapportoImport};
use aether_catalogo::Cataloghi;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::{AvanzamentoLettura, ContenutoEsterno, Fonte};
use aether_net::Rete;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Quanto si aspetta una copertina.
///
/// Trenta secondi: è piccola, ma un servizio può impuntarsi, e la lettura di un
/// link non deve fermarsi per un'immagine.
const SCADENZA_COPERTINA: Duration = Duration::from_secs(30);

/// Quanto può pesare una copertina portata dentro.
///
/// Otto mebibyte, lo stesso tetto che `tag_scrittura` applica quando la
/// incorpora in un file: accettarne di più qui vorrebbe dire tenerne in memoria
/// il doppio per buttarli via un passo dopo.
const COPERTINA_MASSIMA: usize = 8 * 1024 * 1024;

/// Quel che si è letto e che si sta per importare.
struct Risolto {
    /// La chiave con cui si riconosce che è lo stesso link.
    chiave: String,
    /// Il contenuto.
    contenuto: ContenutoEsterno,
    /// La copertina già portata dentro come `data:` URI.
    copertina: Option<String>,
}

/// Lo stato dell'importazione da un link.
pub struct StatoImport {
    /// I cataloghi, con la loro riserva di connessioni.
    ///
    /// Separati da quelli della coda di proposito: la coda tiene tre fili
    /// occupati per minuti, e una lettura fatta da chi sta guardando la finestra
    /// non deve mettersi dietro di loro.
    cataloghi: Cataloghi,
    /// La rete con cui si portano dentro le copertine.
    rete: Rete,
    /// L'ultimo contenuto risolto.
    ultimo: Mutex<Option<Risolto>>,
}

impl StatoImport {
    /// Lo stato, vuoto.
    ///
    /// Non prende più né la cartella dati né quella dei binari: non c'è un file
    /// di configurazione da leggere e non c'è nessun binario da trovare.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            cataloghi: Cataloghi::nuovi(),
            rete: Rete::nuova("copertine", SCADENZA_COPERTINA),
            ultimo: Mutex::new(None),
        }
    }
}

impl Default for StatoImport {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Un elenco arrivato più corto di quanto il catalogo dichiari.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Troncatura {
    /// Quanti brani sono arrivati.
    pub letti: u32,
    /// Quanti ne dichiara il catalogo.
    pub attesi: u32,
}

/// Cosa si è letto dal catalogo, prima di guardare la libreria.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anteprima {
    /// Il nome stabile della fonte: `internet-archive`, `jamendo`, `audius`.
    ///
    /// Solo il nome stabile, e non anche l'etichetta che si legge: quella la
    /// compone il frontend dal catalogo delle lingue. `Fonte::etichetta()`
    /// resta e serve ancora — nelle cause degli errori e nell'attribuzione che
    /// si scrive nei tag — ma là non la legge chi ascolta.
    pub fonte: String,
    /// `brano`, `album`, `playlist`, `artista` o `collezione`.
    pub genere: String,
    /// L'identificativo presso il catalogo.
    pub id: String,
    /// Il nome.
    pub titolo: String,
    /// Chi lo firma.
    pub autore: Option<String>,
    /// La copertina come `data:` URI, già portata dentro.
    ///
    /// **Non** l'indirizzo del catalogo: la politica dei contenuti della
    /// finestra non ammette domini esterni fra le immagini, e allargarla per
    /// sempre per una miniatura è esattamente quel che il resto
    /// dell'applicazione si rifiuta di fare.
    pub copertina: Option<String>,
    /// Quanti brani sono arrivati.
    pub brani: usize,
    /// Da quale livello.
    pub sorgente: String,
    /// Se l'elenco è monco.
    pub troncato: Option<Troncatura>,
    /// Quanti di quei brani si possono tenere sul disco.
    ///
    /// Il campo che l'anteprima di prima non poteva avere. Un elenco di venti
    /// brani di cui tre si prendono e diciassette si ascoltano e basta è una
    /// cosa che chi conferma deve sapere **prima** di confermare, non scoprire
    /// dalla coda che si riempie di righe introvabili.
    pub scaricabili: usize,
}

/// Lo stato di un catalogo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticaCatalogo {
    /// Il nome stabile. L'etichetta che si legge la mette il frontend.
    pub nome: String,
    /// Risponde, adesso.
    pub risponde: bool,
    /// Da qui si può tenere una copia, o solo ascoltare.
    pub consegna: bool,
}

/// Che aria tira, catalogo per catalogo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostica {
    /// Uno per catalogo compilato in questa build.
    pub cataloghi: Vec<DiagnosticaCatalogo>,
}

/// La chiave con cui la cella riconosce un contenuto già letto.
fn chiave(fonte: Fonte, id: &str) -> String {
    format!("{}:{id}", fonte.nome())
}

/// A che punto è la lettura di un link.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventoLettura {
    /// Il livello che sta rispondendo: `Livello::nome()`, la stessa parola che
    /// l'anteprima poi mostra come provenienza.
    sorgente: String,
    /// Quante pagine sono state lette, da 1.
    pagina: u32,
    /// Quante saranno, quando il catalogo lo dichiara. `None` → indeterminata.
    pagine: Option<u32>,
    /// Quanti brani sono in mano finora.
    brani: u32,
}

/// Inoltra alla finestra quel che il lettore racconta.
fn inoltra(app: &AppHandle, avanzamento: AvanzamentoLettura) {
    app.emetti(
        "import:avanzamento",
        EventoLettura {
            sorgente: avanzamento.livello.nome().to_owned(),
            pagina: avanzamento.pagina,
            pagine: avanzamento.pagine,
            brani: avanzamento.brani,
        },
    );
}

/// Il contenuto in cella è già quello di questo link.
fn gia_in_cella(stato: &State<'_, StatoImport>, chiave: &str) -> Result<bool, AppError> {
    let Ok(cella) = stato.ultimo.lock() else {
        return Err(avvelenato());
    };
    Ok(cella.as_ref().is_some_and(|r| r.chiave == chiave))
}

/// Legge il contenuto, riusando quello già in cella se è lo stesso link.
///
/// `forza` salta il riuso: vedi [`import_anteprima`] sul perché una lettura
/// riuscita male deve poter essere rifatta.
///
/// **Non** prende il lucchetto della libreria: vedi la nota in testa al modulo.
fn risolvi(
    app: &AppHandle,
    stato: &State<'_, StatoImport>,
    url: &str,
    forza: bool,
) -> Result<(), AppError> {
    let Some(riferimento) = aether_catalogo::riconosci(url) else {
        return Err(AppError::new(ErrorCode::DownloadUnrecognizedUrl)
            .with_message("non è un link di un catalogo che Aether sappia leggere".to_owned()));
    };

    let chiave = chiave(riferimento.fonte, &riferimento.id);
    if !forza && gia_in_cella(stato, &chiave)? {
        return Ok(());
    }

    let contenuto = stato.cataloghi.risolvi(&riferimento)?;
    // Un solo evento, alla fine: l'Internet Archive risponde con la scheda
    // intera in una richiesta, quindi non ci sono pagine da contare. Emetterlo
    // lo stesso serve a chi disegna — la barra si chiude invece di restare
    // indeterminata per sempre.
    inoltra(
        app,
        AvanzamentoLettura {
            livello: contenuto.source,
            pagina: 1,
            pagine: Some(1),
            brani: u32::try_from(contenuto.tracks.len()).unwrap_or(u32::MAX),
        },
    );

    let copertina = contenuto
        .cover_url
        .as_deref()
        .and_then(|indirizzo| porta_dentro(&stato.rete, indirizzo));

    let Ok(mut cella) = stato.ultimo.lock() else {
        return Err(avvelenato());
    };
    *cella = Some(Risolto {
        chiave,
        contenuto,
        copertina,
    });
    Ok(())
}

/// Porta dentro un'immagine come `data:` URI.
///
/// `None` per qualunque intoppo: un contenuto senza copertina è un contenuto, e
/// far fallire la lettura di una playlist perché una miniatura non arriva
/// sarebbe assurdo.
fn porta_dentro(rete: &Rete, url: &str) -> Option<String> {
    let mut byte = Vec::new();
    rete.preleva(url, &[], &mut byte, &|| false, &mut |_, _| {})
        .ok()?;
    aether_net::immagine::data_uri(&byte, COPERTINA_MASSIMA)
}

/// Fa qualcosa con il contenuto in cella.
fn con_contenuto<T>(
    stato: &State<'_, StatoImport>,
    cosa: impl FnOnce(&Risolto) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let Ok(cella) = stato.ultimo.lock() else {
        return Err(avvelenato());
    };
    let risolto = cella
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::CatalogoResolveFailed))?;
    cosa(risolto)
}

/// Un mutex avvelenato da un panico altrove.
fn avvelenato() -> AppError {
    AppError::new(ErrorCode::InternalUnexpected {
        detail: Some("lo stato dell'importazione non è più leggibile".to_owned()),
    })
}

/// Legge un link e dice cosa c'è dietro, senza toccare la libreria.
///
/// # Perché esiste `forza`
///
/// Perché il contenuto letto resta in una cella, e senza una via per svuotarla
/// una lettura andata male sarebbe **definitiva**: lo stesso link darebbe quella
/// stessa risposta magra per tutto il tempo in cui l'applicazione resta aperta,
/// e «Riprova» non riproverebbe niente.
///
/// `(async)`: parla con la rete — più pagine, e una copertina da scaricare — e
/// un comando normale terrebbe fermo il filo principale della finestra per
/// tutto quel tempo.
#[tauri::command(async)]
pub fn import_anteprima(
    app: AppHandle,
    importa: State<'_, StatoImport>,
    url: String,
    forza: bool,
) -> Esito<Anteprima> {
    risolvi(&app, &importa, &url, forza).map_err(errore)?;
    con_contenuto(&importa, |risolto| {
        let c = &risolto.contenuto;
        Ok(Anteprima {
            fonte: c.fonte.nome().to_owned(),
            genere: c.kind.nome().to_owned(),
            id: c.id.clone(),
            titolo: c.title.clone(),
            autore: c.author.clone(),
            copertina: risolto.copertina.clone(),
            brani: c.tracks.len(),
            sorgente: c.source.nome().to_owned(),
            troncato: c
                .truncation()
                .map(|(letti, attesi)| Troncatura { letti, attesi }),
            scaricabili: c
                .tracks
                .iter()
                .filter(|b| b.disponibilita == aether_domain::esterno::Disponibilita::Scaricabile)
                .count(),
        })
    })
    .map_err(errore)
}

/// Cosa porterebbe l'importazione, senza scrivere niente.
///
/// `(async)` come [`import_anteprima`]: può dover risolvere il link.
#[tauri::command(async)]
pub fn import_piano(
    app: AppHandle,
    stato: State<'_, Stato>,
    importa: State<'_, StatoImport>,
    url: String,
    crea_playlist: bool,
) -> Esito<RapportoImport> {
    esegui(&app, &stato, &importa, &url, crea_playlist, false)
}

/// Importa per davvero.
///
/// # E poi la coda parte
///
/// I brani che in libreria non ci sono finiscono in `desiderati`, e da lì parte
/// la coda — subito, senza dover premere altro. È l'«in modo automatico»: chi
/// importa un elenco vuole ascoltarlo, non ottenere una lista di cose che gli
/// mancano.
///
/// Non è bloccante: [`crate::procura::avvia`] mette in piedi un filo e torna, e
/// questo comando risponde con il rapporto mentre il primo brano si sta già
/// cercando. Se una coda sta già girando non ne parte una seconda: quella in
/// corso rilegge la tabella a ogni lotto.
///
/// `(async)` come [`import_anteprima`]: la risoluzione del link e la scrittura
/// di trecento righe non stanno sul filo della finestra — è la ragione per cui
/// `import:avanzamento` esiste.
#[tauri::command(async)]
pub fn import_esegui(
    app: AppHandle,
    stato: State<'_, Stato>,
    importa: State<'_, StatoImport>,
    url: String,
    crea_playlist: bool,
) -> Esito<RapportoImport> {
    let esito = crate::nuvola::se_riuscito(
        &app,
        esegui(&app, &stato, &importa, &url, crea_playlist, true),
    );
    if esito.as_ref().is_ok_and(|rapporto| rapporto.missing > 0) {
        crate::procura::avvia(&app);
    }
    esito
}

fn esegui(
    app: &AppHandle,
    stato: &State<'_, Stato>,
    importa: &State<'_, StatoImport>,
    url: &str,
    crea_playlist: bool,
    conferma: bool,
) -> Esito<RapportoImport> {
    // Prima la rete, fuori da ogni lucchetto… e senza `forza`: il piano e la
    // conferma parlano del contenuto che si è appena visto, e rileggerlo
    // vorrebbe dire poter importare qualcosa di diverso da quel che mostrava
    // l'anteprima.
    risolvi(app, importa, url, false).map_err(errore)?;
    // …poi una copia del contenuto, così la cella non resta presa mentre si
    // scrive nel database: sono due lucchetti diversi, e prenderli in ordine
    // fisso è quel che rende impossibile incrociarli.
    let contenuto =
        con_contenuto(importa, |risolto| Ok(risolto.contenuto.clone())).map_err(errore)?;

    con_libreria(stato, |libreria| {
        if conferma {
            import_esterno::import(&mut libreria.connection, &contenuto, crea_playlist)
        } else {
            import_esterno::plan(&mut libreria.connection, &contenuto, crea_playlist)
        }
    })
    .map_err(errore)
}

/// Il rapporto di un'importazione già fatta.
///
/// `null` quando non ce n'è uno: un'importazione più vecchia della persistenza,
/// o una di cui il rapporto sia stato potato. **Non** è un errore, e chi lo
/// mostra deve dirlo come una nota — un guasto manderebbe qualcuno a cercare una
/// riparazione che non esiste.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
#[tauri::command]
pub fn import_rapporto(
    stato: State<'_, Stato>,
    source_id: String,
) -> Esito<Option<RapportoImport>> {
    con_libreria(&stato, |libreria| {
        import_esterno::rapporto(&libreria.connection, &source_id)
    })
    .map_err(errore)
}

/// Gli ultimi rapporti, il più recente per primo.
///
/// `limite` non è una comodità: la pagina ne mostra un elenco e se ne apre uno,
/// e chiedere tutto vorrebbe dire deserializzare migliaia di elenchi di brani
/// mancanti per buttarne via quasi tutti.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
#[tauri::command]
pub fn import_rapporti(stato: State<'_, Stato>, limite: u32) -> Esito<Vec<RapportoImport>> {
    con_libreria(&stato, |libreria| {
        import_esterno::rapporti(&libreria.connection, limite)
    })
    .map_err(errore)
}

/// Chi risponde, adesso.
///
/// **Fa richieste vere.** È l'unico modo di distinguere «il brano non c'è» da
/// «il catalogo è giù», e una diagnostica che non chiede niente non diagnostica
/// niente. Per la stessa ragione non la si chiama a ogni apertura di finestra:
/// `scarico_stato` dà l'elenco dei cataloghi senza toccare la rete.
///
/// `(async)`, proprio perché fa richieste vere: due cataloghi che non
/// rispondono sarebbero due timeout consumati sul filo della finestra.
#[tauri::command(async)]
pub fn import_diagnostica(importa: State<'_, StatoImport>) -> Esito<Diagnostica> {
    let d = importa.cataloghi.diagnostica();
    Ok(Diagnostica {
        cataloghi: vec![
            DiagnosticaCatalogo {
                nome: Fonte::InternetArchive.nome().to_owned(),
                risponde: d.internet_archive,
                consegna: Fonte::InternetArchive.puo_consegnare(),
            },
            DiagnosticaCatalogo {
                nome: Fonte::Audius.nome().to_owned(),
                risponde: d.audius,
                consegna: Fonte::Audius.puo_consegnare(),
            },
            // Jamendo compare solo se compilato: una riga «non risponde» per un
            // catalogo che non esiste in questa versione manderebbe a cercare
            // un guasto di rete dove c'è una decisione di distribuzione.
            #[cfg(feature = "jamendo")]
            DiagnosticaCatalogo {
                nome: Fonte::Jamendo.nome().to_owned(),
                risponde: d.jamendo,
                consegna: Fonte::Jamendo.puo_consegnare(),
            },
        ],
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn la_chiave_della_cella_porta_la_fonte() {
        // Senza la fonte, due contenuti di due cataloghi con lo stesso
        // identificativo si scambierebbero di posto in cella: chi incolla il
        // secondo si vedrebbe l'anteprima del primo.
        assert_eq!(
            chiave(Fonte::InternetArchive, "abc"),
            "internet-archive:abc"
        );
        assert_ne!(
            chiave(Fonte::InternetArchive, "abc"),
            chiave(Fonte::Jamendo, "abc")
        );
    }

    #[test]
    fn quel_che_non_e_di_un_catalogo_non_si_prova_nemmeno() {
        // Il confine, e la ragione per cui è scritto: un link che non si
        // riconosce è un link a cui non si bussa.
        for url in [
            "https://www.youtube.com/watch?v=4IJI6soiQhI",
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
        ] {
            assert!(aether_catalogo::riconosci(url).is_none(), "su «{url}»");
        }
        assert!(aether_catalogo::riconosci("https://archive.org/details/gd1977-05-08").is_some());
    }
}
