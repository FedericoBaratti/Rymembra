//! Cercare nei cataloghi liberi, dal lato della finestra.
//!
//! La metà della issue #1 che mancava davvero. Il lettore sa suonare byte che
//! arrivano dalla rete dalla 2.2.0, la migrazione 26 ha dato a quei brani un
//! posto in libreria, e qui c'è la superficie da cui si cercano: una frase, i
//! cataloghi, e un tasto per tenersi quel che si è trovato.
//!
//! # Perché i risultati restano di qua
//!
//! La finestra riceve delle righe da mostrare e rimanda **l'indirizzo** di
//! quella che si vuole, non la riga intera. È la differenza che conta: licenza
//! e disponibilità le dichiara il catalogo, e se tornassero indietro dalla
//! finestra basterebbe un errore — o una chiamata costruita a mano, che su un
//! canale IPC è sempre possibile — perché qualcosa entrasse in libreria con un
//! permesso che nessuno gli ha dato. Il cancello di `prelievo` sta prima della
//! rete per la stessa ragione: le decisioni sui permessi si prendono dove i
//! permessi si conoscono.
//!
//! # Perché una ricerca si fa a Invio e non a ogni tasto
//!
//! Perché costa. Misurata: da una a cinque richieste all'Internet Archive e una
//! ad Audius, fra i trecento millisecondi e i tre secondi e mezzo. La casella
//! della libreria cerca a ogni carattere perché interroga un indice SQLite che
//! sta in casa; questa interroga archivi pubblici che ci ospitano gratis, e una
//! ricerca per tasto premuto è il modo di farsi chiudere la porta.
//!
//! # L'ordine dei lucchetti
//!
//! ```text
//! (senza lucchetti)  chiedi ai cataloghi
//! cella              metti via i risultati                  → rilascia
//! libreria           segna quali ci sono già / scrivi       → rilascia
//! ```

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use aether_catalogo::Cataloghi;
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::esterno::Disponibilita;
use aether_domain::scelta::Candidato;
use serde::Serialize;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Lo stato della ricerca nei cataloghi.
pub struct StatoEsplora {
    /// I cataloghi, con la loro riserva di connessioni.
    ///
    /// Separati da quelli della coda **e** da quelli dell'importazione, per la
    /// stessa ragione per cui quei due sono separati fra loro: la coda tiene
    /// tre fili occupati per minuti, e chi sta guardando la finestra e ha
    /// appena premuto Invio non deve mettersi dietro di loro.
    cataloghi: Cataloghi,
    /// L'ultima ricerca, con i candidati come li ha dati il catalogo.
    ultima: Mutex<Vec<Candidato>>,
    /// Alzato quando chi cerca cambia idea.
    ///
    /// I cataloghi lo interrogano fra una richiesta e l'altra: è la forma che
    /// l'annullamento ha in tutto l'albero, perché qui non c'è un runtime
    /// asincrono da cui cancellare un compito.
    annullata: AtomicBool,
}

impl StatoEsplora {
    /// Lo stato, vuoto.
    #[must_use]
    pub fn nuovo() -> Self {
        Self {
            cataloghi: Cataloghi::nuovi(),
            ultima: Mutex::new(Vec::new()),
            annullata: AtomicBool::new(false),
        }
    }
}

impl Default for StatoEsplora {
    fn default() -> Self {
        Self::nuovo()
    }
}

/// Un mutex avvelenato da un panico altrove.
fn avvelenato() -> AppError {
    AppError::new(ErrorCode::InternalUnexpected {
        detail: Some("lo stato della ricerca non è più leggibile".to_owned()),
    })
}

/// Un risultato, come lo mostra la finestra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RisultatoIpc {
    /// L'indirizzo, che è anche il modo di nominarlo nei comandi che seguono.
    pub url: String,
    /// Il titolo.
    pub titolo: String,
    /// Chi lo firma.
    pub autore: Option<String>,
    /// L'album, il concerto o la raccolta da cui viene.
    pub album: Option<String>,
    /// La durata dichiarata, in secondi.
    pub durata_sec: Option<u32>,
    /// Il nome stabile del catalogo.
    pub fonte: String,
    /// Come si chiama quel catalogo davanti a chi ascolta.
    pub fonte_etichetta: String,
    /// Il nome stabile della licenza.
    pub licenza: String,
    /// Che cosa se ne può fare: `scaricabile`, `soloAscolto`, `soloAcquisto`.
    pub disponibilita: String,
    /// La pagina pubblica, da mostrare accanto al brano.
    ///
    /// Non è un ornamento: per certe Creative Commons e per i termini di Audius
    /// il rimando visibile è una condizione d'uso.
    pub pagina: Option<String>,
    /// Sta già in libreria.
    pub in_libreria: bool,
}

/// Quel che una ricerca ha trovato.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoRicerca {
    /// I risultati, nell'ordine in cui vanno mostrati.
    pub risultati: Vec<RisultatoIpc>,
    /// La ricerca è stata interrotta da chi l'aveva chiesta.
    pub annullata: bool,
    /// I cataloghi che non hanno risposto, coi nomi da mostrare.
    ///
    /// Vuoto è il caso normale. Pieno **e con dei risultati accanto** è il
    /// caso che prima non si vedeva: l'elenco sembra completo e non lo è,
    /// perché uno dei due archivi era giù mentre l'altro rispondeva.
    pub muti: Vec<String>,
}

fn in_ipc(candidato: &Candidato, in_libreria: bool) -> RisultatoIpc {
    RisultatoIpc {
        url: candidato.url.clone(),
        titolo: candidato.titolo.clone(),
        autore: candidato.autore.clone(),
        album: candidato.album.clone(),
        durata_sec: candidato.durata_sec,
        fonte: candidato.fonte.nome().to_owned(),
        fonte_etichetta: candidato.fonte.etichetta().to_owned(),
        licenza: candidato.licenza.nome(),
        disponibilita: candidato.disponibilita.nome().to_owned(),
        pagina: candidato.pagina.clone(),
        in_libreria,
    }
}

/// Cerca una frase nei cataloghi liberi.
///
/// `(async)` come `import_anteprima`, e per la stessa ragione: parla con la
/// rete per qualche secondo, e un comando normale terrebbe fermo il filo
/// principale della finestra per tutto quel tempo.
///
/// # Errori
///
/// `download.externalSearchFailed` se **nessun** catalogo ha risposto — che è
/// una frase diversa da «nessun risultato», e la finestra le distingue perché
/// chi legge la seconda al posto della prima smette di cercare.
#[tauri::command(async)]
pub fn esplora_cerca(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
    testo: String,
) -> Esito<EsitoRicerca> {
    esplora.annullata.store(false, Ordering::Relaxed);
    let annullato = || esplora.annullata.load(Ordering::Relaxed);

    let ricerca = esplora
        .cataloghi
        .cerca_libera(&testo, &annullato)
        .map_err(errore)?;

    // Quali ci sono già: una query sola con tutti i riferimenti, non una per
    // riga. Con otto risultati la differenza non si vede; è la forma a contare,
    // perché il giorno in cui i risultati saranno cinquanta sarebbe cinquanta
    // volte il lucchetto della libreria preso e lasciato.
    let riferimenti: Vec<(String, String)> = ricerca
        .trovati
        .iter()
        .map(|c| (c.fonte.nome().to_owned(), c.url.clone()))
        .collect();
    let presenti = con_libreria(&stato, |libreria| {
        aether_app::catalogo::gia_in_libreria(&libreria.connection, &riferimenti)
    })
    .map_err(errore)?;

    let risultati = ricerca
        .trovati
        .iter()
        .map(|c| in_ipc(c, presenti.contains(&c.url)))
        .collect();

    // E il guasto si dice, invece di ingoiarlo. Un `if let Ok(…)` lasciava
    // passare in silenzio la cella avvelenata: da lì in poi «aggiungi» e
    // «ascolta» avrebbero risposto `catalogo.resolveFailed` su risultati
    // vecchi, senza che niente collegasse le due cose.
    {
        let mut ultima = esplora.ultima.lock().map_err(|_| errore(avvelenato()))?;
        *ultima = ricerca.trovati;
    }

    Ok(EsitoRicerca {
        risultati,
        annullata: annullato(),
        muti: ricerca
            .muti
            .iter()
            .map(|fonte| fonte.etichetta().to_owned())
            .collect(),
    })
}

/// Ferma la ricerca in corso.
///
/// Non aspetta che si fermi: alza la bandiera e torna. Chi sta cercando la
/// guarda fra una richiesta e l'altra, e si ferma lì.
#[tauri::command]
pub fn esplora_annulla(esplora: State<'_, StatoEsplora>) -> Esito<()> {
    esplora.annullata.store(true, Ordering::Relaxed);
    Ok(())
}

/// Il candidato con questo indirizzo, fra quelli dell'ultima ricerca.
fn candidato_di(esplora: &StatoEsplora, url: &str) -> Result<Candidato, AppError> {
    let Ok(ultima) = esplora.ultima.lock() else {
        return Err(avvelenato());
    };
    ultima
        .iter()
        .find(|c| c.url == url)
        .cloned()
        .ok_or_else(|| {
            AppError::new(ErrorCode::CatalogoResolveFailed).with_cause(
                "questo risultato non è più fra quelli cercati: rifai la ricerca".to_owned(),
            )
        })
}

/// Mette in libreria un brano trovato, come riferimento e non come file.
///
/// # Errori
///
/// `catalogo.resolveFailed` se quell'indirizzo non è fra i risultati
/// dell'ultima ricerca; `download.notPermitted` se nessuna fonte lecita lo
/// consegna; `db.*` se la scrittura fallisce.
#[tauri::command(async)]
pub fn esplora_aggiungi(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
    url: String,
) -> Esito<aether_app::catalogo::Aggiunti> {
    let candidato = candidato_di(&esplora, &url).map_err(errore)?;
    let esito = con_libreria(&stato, |libreria| {
        aether_app::catalogo::aggiungi(&mut libreria.connection, std::slice::from_ref(&candidato))
    })
    .map_err(errore)?;
    if esito.senza_byte > 0 {
        return Err(errore(aether_app::catalogo::nessuno_lo_consegna()));
    }
    if esito.scartati > 0 {
        return Err(errore(aether_app::catalogo::riga_illeggibile()));
    }
    Ok(esito)
}

/// Mette nella lista della spesa un risultato che nessun catalogo libero dà.
///
/// # Perché un comando e non «aggiungi» che si arrangia
///
/// Perché sono due gesti diversi e vanno detti in due modi diversi: «tienilo»
/// mette in libreria qualcosa che si potrà ascoltare, questo annota che quel
/// brano esiste e che per averlo bisogna comprarlo. Finisce fra i
/// [`Stato::Introvabile`](aether_app::desiderati::Stato::Introvabile) di
/// `desiderati`, che è la riga che nessuna passata di scaricamento riprova e
/// che la lista della spesa raccoglie.
///
/// # Errori
///
/// `catalogo.resolveFailed` se quell'indirizzo non è fra i risultati
/// dell'ultima ricerca; `db.*` se la scrittura fallisce.
#[tauri::command(async)]
pub fn esplora_nella_lista(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
    url: String,
    frase: String,
) -> Esito<bool> {
    let candidato = candidato_di(&esplora, &url).map_err(errore)?;
    let scritte = con_libreria(&stato, |libreria| {
        aether_app::catalogo::nella_lista_della_spesa(
            &mut libreria.connection,
            std::slice::from_ref(&candidato),
            &frase,
        )
    })
    .map_err(errore)?;
    Ok(scritte > 0)
}

/// Mette in coda di prelievo un risultato che la licenza permette di tenere.
///
/// # Perché non è «aggiungi» con un interruttore
///
/// Perché sono due cose diverse e finiscono in due posti diversi. «Aggiungi»
/// scrive in libreria un **riferimento**: il brano c'è e suona arrivando dalla
/// rete. Questo scrive in `desiderati` una riga che chiede un **file**, e il
/// prelievo la porterà giù con l'indirizzo esatto che chi cercava aveva
/// davanti — non un file simile scelto da un algoritmo.
///
/// Non avvia la passata: quella prende tutte le righe in attesa, e accenderla
/// da qui vorrebbe dire far partire scaricamenti che nessuno ha chiesto adesso.
/// Si accende da Importazioni, come per ogni altra riga desiderata.
///
/// # Errori
///
/// `catalogo.resolveFailed` se quell'indirizzo non è fra i risultati
/// dell'ultima ricerca; `download.notPermitted` se la licenza di quel brano non
/// permette di tenerne una copia; `db.*` se la scrittura fallisce.
#[tauri::command(async)]
pub fn esplora_tieni(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
    url: String,
    frase: String,
) -> Esito<bool> {
    let candidato = candidato_di(&esplora, &url).map_err(errore)?;
    let scritte = con_libreria(&stato, |libreria| {
        aether_app::catalogo::da_tenere(
            &mut libreria.connection,
            std::slice::from_ref(&candidato),
            &frase,
        )
    })
    .map_err(errore)?;
    // Zero righe scritte ha due cause, e vanno distinte: il candidato non è
    // tenibile — ed è un no del catalogo, da dire — oppure c'era già, che è un
    // «fatto» tiepido. Il cancello sta nel nucleo, quindi qui si guarda la
    // stessa condizione per scegliere la frase, non per decidere.
    if scritte == 0 && candidato.disponibilita != Disponibilita::Scaricabile {
        return Err(errore(aether_app::catalogo::non_si_tiene()));
    }
    Ok(scritte > 0)
}

/// Quel che «aggiungi tutti» ha prodotto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EsitoAggiuntaTutti {
    /// I conteggi: quanti nuovi, quanti c'erano già, quanti rifiutati.
    pub conteggi: aether_app::catalogo::Aggiunti,
    /// Gli indirizzi che **adesso** stanno in libreria.
    ///
    /// Non è ridondante rispetto ai conteggi: è l'unica cosa con cui la
    /// finestra può aggiornare le righe dicendo il vero. Senza, segnava
    /// «in libreria» tutto quel che non fosse di solo acquisto — comprese le
    /// righe che il nucleo aveva appena scartato — e bastava premere
    /// «ascolta» su una di quelle per ricevere un errore su un tasto che
    /// diceva di aver già funzionato.
    pub in_libreria: Vec<String>,
}

/// Mette in libreria tutti i risultati dell'ultima ricerca che si possono
/// ascoltare.
///
/// # Errori
///
/// `db.*` se la scrittura fallisce. Un risultato che nessuno consegna non è un
/// errore: si conta, e gli altri entrano lo stesso.
#[tauri::command(async)]
pub fn esplora_aggiungi_tutti(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
) -> Esito<EsitoAggiuntaTutti> {
    let candidati = {
        let Ok(ultima) = esplora.ultima.lock() else {
            return Err(errore(avvelenato()));
        };
        ultima.clone()
    };
    let riferimenti: Vec<(String, String)> = candidati
        .iter()
        .map(|c| (c.fonte.nome().to_owned(), c.url.clone()))
        .collect();

    // Scrittura e rilettura sotto lo stesso lucchetto: due prese separate
    // lascerebbero in mezzo una finestra in cui un'altra scrittura cambia
    // la risposta, e la risposta serve a disegnare i tasti.
    con_libreria(&stato, |libreria| {
        let conteggi = aether_app::catalogo::aggiungi(&mut libreria.connection, &candidati)?;
        let presenti = aether_app::catalogo::gia_in_libreria(&libreria.connection, &riferimenti)?;
        Ok(EsitoAggiuntaTutti {
            conteggi,
            in_libreria: presenti.into_iter().collect(),
        })
    })
    .map_err(errore)
}

/// Toglie dalla libreria un brano di catalogo, per indirizzo.
///
/// # Errori
///
/// `catalogo.resolveFailed` se quell'indirizzo non è fra i risultati
/// dell'ultima ricerca; `db.*` se la scrittura fallisce.
#[tauri::command(async)]
pub fn esplora_togli(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
    url: String,
) -> Esito<bool> {
    let candidato = candidato_di(&esplora, &url).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        aether_app::catalogo::togli(&mut libreria.connection, candidato.fonte.nome(), &url)
    })
    .map_err(errore)
}

/// Il brano di libreria che corrisponde a un risultato, per poterlo suonare.
///
/// # Errori
///
/// `catalogo.resolveFailed` se quell'indirizzo non è fra i risultati
/// dell'ultima ricerca; `db.*` se la lettura fallisce.
#[tauri::command(async)]
pub fn esplora_identificativo(
    stato: State<'_, Stato>,
    esplora: State<'_, StatoEsplora>,
    url: String,
) -> Esito<Option<i64>> {
    let candidato = candidato_di(&esplora, &url).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        aether_app::catalogo::id_di(&libreria.connection, candidato.fonte.nome(), &url)
    })
    .map_err(errore)
}

/// Apre nel browser la pagina pubblica di un risultato.
///
/// # Perché l'indirizzo non arriva dalla finestra
///
/// È la stessa regola di `procura::cerca_dove_comprare`, scritta là e valida
/// qui: aprire un indirizzo nel browser di sistema è **l'unica capacità
/// pericolosa** che la finestra ha, e lasciarle scegliere quale indirizzo
/// vorrebbe dire dargliela intera. Qui la finestra nomina un risultato; la
/// pagina è quella che il catalogo aveva dichiarato per quel risultato, e prima
/// di aprirla passa comunque dall'allowlist di `aether_catalogo::riconosci` —
/// che è lo stesso cancello da cui passano i byte da suonare.
///
/// # Errori
///
/// `catalogo.resolveFailed` se quell'indirizzo non è fra i risultati
/// dell'ultima ricerca; `download.unrecognizedUrl` se quel risultato non ha una
/// pagina pubblica, o se la pagina non è in un posto dove Aether sia invitato;
/// `internal.aborted` se il browser non si apre.
#[tauri::command(async)]
pub fn esplora_apri_pagina(esplora: State<'_, StatoEsplora>, url: String) -> Esito<()> {
    let candidato = candidato_di(&esplora, &url).map_err(errore)?;
    let pagina = candidato.pagina.ok_or_else(|| {
        errore(
            AppError::new(ErrorCode::DownloadUnrecognizedUrl)
                .with_cause("questo brano non dichiara una pagina pubblica".to_owned()),
        )
    })?;
    if aether_catalogo::riconosci(&pagina).is_none() {
        return Err(errore(
            AppError::new(ErrorCode::DownloadUnrecognizedUrl)
                .with_cause("questa pagina non è in un posto dove Aether sia invitato".to_owned()),
        ));
    }
    tauri_plugin_opener::open_url(&pagina, None::<&str>).map_err(|err| {
        errore(
            AppError::new(ErrorCode::InternalAborted {
                what: Some("apertura del browser".to_owned()),
            })
            .with_cause(err.to_string()),
        )
    })
}

#[cfg(test)]
mod prove {
    use super::*;
    use aether_domain::esterno::{Disponibilita, Fonte, Licenza};

    fn candidato() -> Candidato {
        Candidato {
            url: "https://archive.org/download/x/y.mp3".to_owned(),
            titolo: "Scarlet Begonias".to_owned(),
            autore: Some("Grateful Dead".to_owned()),
            album: Some("Barton Hall 1977".to_owned()),
            durata_sec: Some(754),
            fonte: Fonte::InternetArchive,
            licenza: Licenza::LiberaNonCommerciale,
            disponibilita: Disponibilita::SoloAscolto,
            pagina: Some("https://archive.org/details/x".to_owned()),
            ..Candidato::default()
        }
    }

    /// Quel che va alla finestra porta licenza, disponibilità e pagina.
    ///
    /// Le tre cose senza cui la schermata non può rispettare i termini dei
    /// cataloghi: dire cosa si può fare, e da dove viene.
    #[test]
    fn un_risultato_porta_con_se_i_suoi_permessi() {
        let riga = in_ipc(&candidato(), false);
        assert_eq!(riga.licenza, "liberaNonCommerciale");
        assert_eq!(riga.disponibilita, "soloAscolto");
        assert_eq!(riga.fonte, "internet-archive");
        assert_eq!(riga.fonte_etichetta, "Internet Archive");
        assert_eq!(riga.album.as_deref(), Some("Barton Hall 1977"));
        assert!(
            riga.pagina.is_some(),
            "senza la pagina l'attribuzione manca"
        );
    }

    /// Un indirizzo che non è fra i risultati non si aggiunge.
    ///
    /// È il cancello che tiene i permessi dalla parte del catalogo: senza,
    /// basterebbe una chiamata costruita a mano per far entrare in libreria un
    /// indirizzo qualunque con la licenza che si preferisce.
    #[test]
    fn un_indirizzo_che_nessuno_ha_cercato_non_si_aggiunge() {
        let esplora = StatoEsplora::nuovo();
        if let Ok(mut ultima) = esplora.ultima.lock() {
            *ultima = vec![candidato()];
        }
        let esito = candidato_di(&esplora, "https://archive.org/download/altro/z.mp3");
        assert!(
            matches!(
                esito.as_ref().map_err(AppError::code),
                Err(ErrorCode::CatalogoResolveFailed)
            ),
            "invece di rifiutare ha risposto: {:?}",
            esito.map(|c| c.url)
        );
        assert!(candidato_di(&esplora, &candidato().url).is_ok());
    }

    /// L'annullamento è una bandiera, e si rialza a ogni ricerca nuova.
    ///
    /// Se restasse alzata, la ricerca dopo un annullamento partirebbe già
    /// annullata e risponderebbe «nessun risultato» senza chiedere niente a
    /// nessuno — il guasto che si vede una volta su dieci e non si riproduce.
    #[test]
    fn annullare_non_avvelena_la_ricerca_dopo() {
        let esplora = StatoEsplora::nuovo();
        assert!(!esplora.annullata.load(Ordering::Relaxed));
        esplora.annullata.store(true, Ordering::Relaxed);
        assert!(esplora.annullata.load(Ordering::Relaxed));
        // È la prima riga di `esplora_cerca`.
        esplora.annullata.store(false, Ordering::Relaxed);
        assert!(!esplora.annullata.load(Ordering::Relaxed));
    }
}
