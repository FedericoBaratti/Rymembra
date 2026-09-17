//! LRCLIB: il catalogo dei testi, e l'unico che si interroga.
//!
//! Sta in questo crate e non in uno nuovo perché la descrizione di
//! [`crate`] dice «i servizi di metadati musicali, tutti senza chiave», e
//! LRCLIB è esattamente quello: nessuna registrazione, nessun account, nessun
//! segreto nel portachiavi. Da qui eredita gratis il [`Deposito`](crate::Deposito),
//! la [`Cadenza`](crate::Cadenza), l'interruttore, lo `User-Agent` che dice chi
//! siamo, e — la cosa che conta di più — la distinzione fra «non ce l'ha» e
//! «non si sa», che [`Fornitori::json`] codifica in `Ok(None)` contro `Err`.
//!
//! # Perché questo e non gli altri
//!
//! Perché è l'unico che si può interrogare **senza aggirare niente**. Gli altri
//! cataloghi di testi o vogliono una licenza editoriale che un lettore locale
//! non ha, o si leggono solo raschiando una pagina scritta per un browser: nel
//! primo caso non ce li darebbero, nel secondo ce li prenderemmo. Qui c'è
//! un'API pubblica, documentata, fatta apposta per i lettori musicali — la
//! usano Feishin, Supersonic, Navidrome — e delle richieste HTTP che si leggono
//! nel codice.
//!
//! Va detto per intero, perché è la parte che di solito si tace: LRCLIB è una
//! base dati **alimentata dalla comunità**, non un distributore con contratti
//! di sotto-edizione. Aether non infrange nessuna regola nel leggerla, e non
//! per questo i testi che ne escono sono «licenziati». Chi ospita risponde alle
//! richieste di rimozione; noi non ridistribuiamo niente e non teniamo copie
//! altrove — vedi la nota sul backup in `011_testi.sql`.
//!
//! # I secondi con la virgola
//!
//! `duration` arriva in **secondi, in virgola mobile**: `240.0`, non `240000`.
//! È lo stesso scoglio che `deezer.rs` documenta sui suoi secondi interi, e
//! sbagliarlo produce scarti di tre ordini di grandezza — cioè nessuna
//! corrispondenza, mai, senza nessun messaggio d'errore.

use aether_domain::abbinamento::{primo_artista, senza_decorazioni};
use aether_domain::errors::{AppError, ErrorCode};
use aether_domain::testo::{Candidato, Cercato};
use aether_domain::{enrich::normalize_for_match, testo};
use aether_net::http::{Corpo, Metodo, Richiesta};
use aether_net::percento;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::deposito::VIVE_RICERCA_MS;
use crate::{Fornitori, Memoria};

/// Una voce del catalogo: cosa dice di essere, e cosa porta.
///
/// I due pezzi stanno separati perché servono a due momenti diversi: il
/// [`Candidato`] alla scelta, che è pura e sta nel dominio, e il testo a chi
/// scriverà la riga in tabella. Fonderli vorrebbe dire portare due testi interi
/// dentro la funzione che li deve solo confrontare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voce {
    /// Quel che serve per decidere se è questo il brano.
    pub candidato: Candidato,
    /// Il testo senza tempi.
    pub piatto: Option<String>,
    /// L'LRC, quando la voce ce l'ha.
    pub sincronizzato: Option<String>,
}

impl Voce {
    /// La voce non porta niente da mostrare.
    ///
    /// Capita: una voce può esistere per dire soltanto che il brano è
    /// strumentale, e capita anche che qualcuno abbia caricato una riga vuota.
    /// Il primo caso è una risposta e si tiene; il secondo no.
    #[must_use]
    pub fn e_vuota(&self) -> bool {
        !self.candidato.strumentale
            && self.piatto.as_ref().is_none_or(|t| t.trim().is_empty())
            && self
                .sincronizzato
                .as_ref()
                .is_none_or(|t| t.trim().is_empty())
    }
}

/// Interpreta una risposta, che sia un elenco o una voce sola. Funzione pura.
///
/// Le due forme arrivano da due punti diversi — `/api/search` risponde con un
/// vettore, `/api/get` con un oggetto — e trattarle qui insieme evita a chi
/// chiama di sapere quale delle due sta guardando.
#[must_use]
pub fn interpreta(corpo: &[u8]) -> Vec<Voce> {
    let Ok(letto) = serde_json::from_slice::<Value>(corpo) else {
        return Vec::new();
    };
    match letto {
        Value::Array(voci) => voci.iter().filter_map(una).collect(),
        oggetto => una(&oggetto).into_iter().collect(),
    }
}

/// Una voce sola, se ha almeno un titolo e un artista.
fn una(voce: &Value) -> Option<Voce> {
    let titolo = testo_di(voce, "trackName").or_else(|| testo_di(voce, "name"))?;
    let artista = testo_di(voce, "artistName")?;
    let sincronizzato = testo_di(voce, "syncedLyrics");
    Some(Voce {
        candidato: Candidato {
            id: voce.get("id").and_then(Value::as_i64).unwrap_or(0),
            titolo,
            artista,
            album: testo_di(voce, "albumName"),
            durata_ms: durata_ms(voce),
            strumentale: voce
                .get("instrumental")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            sincronizzato: sincronizzato
                .as_ref()
                .is_some_and(|t: &String| !t.trim().is_empty()),
        },
        piatto: testo_di(voce, "plainLyrics"),
        sincronizzato,
    })
}

/// Un campo di testo non vuoto.
fn testo_di(voce: &Value, campo: &str) -> Option<String> {
    voce.get(campo)
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .map(ToOwned::to_owned)
}

/// La durata in millisecondi, dai secondi in virgola mobile del catalogo.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "il campo è già stato ristretto a un numero finito, positivo e sotto il giorno intero"
)]
fn durata_ms(voce: &Value) -> Option<u64> {
    let secondi = voce.get("duration").and_then(Value::as_f64)?;
    // Un giorno come tetto: oltre non è la durata di un brano, è un dato
    // sbagliato, e lasciarlo passare farebbe fallire il veto di durata per
    // eccesso invece che per merito.
    if !secondi.is_finite() || secondi <= 0.0 || secondi > 86_400.0 {
        return None;
    }
    Some((secondi * 1000.0).round() as u64)
}

/// I secondi interi, arrotondati, che il catalogo si aspetta.
///
/// Una funzione e non due divisioni scritte due volte: le due chiamate — la
/// chiave della cache e il parametro della domanda esatta — devono arrotondare
/// **allo stesso modo**, altrimenti una richiesta e la sua risposta ricordata
/// finiscono sotto due chiavi diverse e la cache non serve a niente.
#[expect(
    clippy::integer_division,
    reason = "arrotondare al secondo è una divisione intera, ed è la forma che il catalogo chiede"
)]
const fn secondi(ms: u64) -> u64 {
    ms.saturating_add(500) / 1000
}

/// La chiave con cui si ricorda la domanda **generosa**.
///
/// Ci va anche la durata, arrotondata al secondo: due edizioni dello stesso
/// brano fanno due domande diverse e devono avere due risposte diverse in
/// cache, altrimenti la prima risposta si applica anche alla seconda — che è
/// proprio l'errore che il veto di durata esiste per fermare.
///
/// L'album invece **non** ci va, e non è una dimenticanza: `/api/search` non lo
/// manda: vedi [`url_ricerca`]. Metterlo qui vorrebbe dire ricordare sotto due
/// chiavi diverse due richieste identiche byte per byte.
fn chiave_ricerca(cercato: &Cercato<'_>) -> String {
    let secondi = secondi(cercato.durata_ms.unwrap_or(0));
    format!(
        "{}|{}|{secondi}",
        normalize_for_match(cercato.artista),
        normalize_for_match(cercato.titolo),
    )
}

/// La chiave con cui si ricorda la domanda **esatta**.
///
/// È quella della ricerca più l'album, perché l'album sta nell'URL di
/// `/api/get` e quindi **fa parte della domanda**: la stessa canzone chiesta
/// una volta come «Habemus Capa» e una come «Greatest Hits» sono due richieste
/// diverse, e il catalogo risponde a una delle due `404`. Senza l'album qui, la
/// prima delle due risposte si applicava anche alla seconda — un `404`
/// ricordato per tre giorni su un'edizione che il catalogo ce l'aveva, o
/// peggio il testo di un'edizione mostrato sull'altra.
///
/// La regola generale, che vale anche per chi aggiungerà una terza domanda:
/// **quel che cambia l'URL deve cambiare la chiave.**
fn chiave_esatta(cercato: &Cercato<'_>) -> String {
    format!(
        "{}|{}",
        chiave_ricerca(cercato),
        normalize_for_match(cercato.album.unwrap_or_default()),
    )
}

/// L'URL della domanda esatta.
///
/// `album_name` **si omette** quando non c'è, invece di mandarlo vuoto: per
/// LRCLIB un parametro assente è «non lo so», mentre `album_name=` è «l'album è
/// la stringa vuota», e nessuna voce del catalogo ha l'album vuoto. Mandarlo
/// vuoto voleva dire un `404` garantito su ogni file senza tag `album` — cioè
/// spendere la richiesta esatta per non ottenere mai niente, proprio sui file
/// taggati peggio, che sono quelli che il testo non ce l'hanno.
fn url_esatta(cercato: &Cercato<'_>, durata_ms: u64) -> String {
    let mut url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}&duration={}",
        percento(cercato.artista),
        percento(cercato.titolo),
        secondi(durata_ms),
    );
    if let Some(album) = cercato.album.map(str::trim).filter(|a| !a.is_empty()) {
        url.push_str("&album_name=");
        url.push_str(&percento(album));
    }
    url
}

/// Con quale titolo si sta facendo il giro: quello del file, o quello sfrondato.
///
/// Serve a **due** cose, e la seconda è quella che conta: dà un nome di
/// servizio diverso ai due giri, quindi due voci di deposito diverse. Senza,
/// le due domande finirebbero sotto la stessa chiave — `chiave_ricerca` passa
/// da `normalize_for_match`, che le parentesi le toglie già, quindi
/// «Wattershed (Live at Reading…)» e «Wattershed» danno la **stessa** stringa
/// — e la risposta della prima si applicherebbe alla seconda. È la regola
/// scritta su [`chiave_esatta`]: quel che cambia l'URL deve cambiare la
/// chiave, e qui l'URL cambia eccome.
///
/// Il guadagno collaterale è la memoria del «non c'è»: due servizi vuol dire
/// che il deposito ricorda di aver provato **tutt'e due** i titoli, e riaprire
/// il pannello domani non costa nessuna richiesta invece di costarne due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Giro {
    /// Il titolo come sta nei tag.
    ComeSta,
    /// Il titolo senza le decorazioni.
    Sfrondato,
    /// Il solo primo interprete, quando i tag ne elencano più d'uno.
    ArtistaPrincipale,
}

impl Giro {
    /// Il nome del servizio per la domanda esatta di questo giro.
    const fn esatta(self) -> &'static str {
        match self {
            Self::ComeSta => "lrclib-esatta",
            Self::Sfrondato => "lrclib-esatta-sfrondata",
            Self::ArtistaPrincipale => "lrclib-esatta-principale",
        }
    }

    /// Il nome del servizio per la domanda generosa di questo giro.
    const fn ricerca(self) -> &'static str {
        match self {
            Self::ComeSta => "lrclib-ricerca",
            Self::Sfrondato => "lrclib-ricerca-sfrondata",
            Self::ArtistaPrincipale => "lrclib-ricerca-principale",
        }
    }
}

/// L'URL della domanda generosa: solo titolo e artista.
fn url_ricerca(cercato: &Cercato<'_>) -> String {
    format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        percento(cercato.titolo),
        percento(cercato.artista),
    )
}

/// Il testo di questo brano, se il catalogo ce l'ha.
///
/// # Le due domande, in quest'ordine
///
/// 1. `/api/get` con titolo, artista, album e durata. È la domanda esatta: il
///    catalogo confronta lui i quattro campi e risponde `404` se non li ha
///    tutti e quattro uguali. Quando risponde, la risposta è **del brano
///    giusto** — che non è la stessa cosa che «è la risposta migliore»: vedi
///    sotto.
/// 2. `/api/search` con titolo e artista. È la domanda generosa, e serve
///    esattamente ai casi in cui la prima fallisce per un dettaglio che non
///    conta — un album taggato «Greatest Hits» invece del disco originale, un
///    secondo di differenza nella durata. A restringere ci pensa
///    [`aether_domain::testo::scegli`], che è pura e provata senza rete.
///
/// # Una risposta esatta senza tempi non chiude la ricerca
///
/// LRCLIB tiene **più voci per lo stesso brano** — una per ogni edizione che
/// qualcuno ha caricato — e `/api/get` ne restituisce una sola: quella la cui
/// firma (titolo, artista, album, durata al secondo) coincide con la domanda.
/// Non è detto che sia quella con i tempi. Capita, e capita spesso, che la voce
/// più vecchia porti il solo testo piatto mentre altre cinque voci dello stesso
/// brano — stessa durata, album scritto in un altro modo — l'LRC ce l'hanno.
///
/// Fermarsi lì è il difetto che questa funzione aveva: il pannello mostrava un
/// testo che non scorre, con l'etichetta «senza tempi», per un brano di cui il
/// catalogo i tempi ce li aveva. E il sintomo era il peggiore possibile per chi
/// guarda — capitava «solo con alcune canzoni», senza nessuna regola visibile,
/// perché la regola stava in quale delle voci era stata caricata per prima.
///
/// Quindi: una risposta esatta **con** i tempi (o uno strumentale, che è una
/// risposta a sé) chiude la ricerca; una senza tempi si tiene da parte e si fa
/// comunque la domanda generosa. Se fra quel che torna c'è una voce con i tempi
/// che passa i veti di [`aether_domain::testo::scegli`], vince lei; altrimenti
/// si restituisce la piatta di prima, che resta pur sempre la voce del brano
/// giusto. Il prezzo è una richiesta in più sui brani che il catalogo conosce
/// solo in piatto — e per una settimana soltanto, che è quanto il deposito
/// ricorda una ricerca.
///
/// # Errori
///
/// L'errore di rete così com'è: chi chiama lo tratta come «non si sa», che è
/// diverso da `Ok(None)` — «il catalogo non ce l'ha». Confondere i due
/// significherebbe marchiare come introvabili tutti i brani cercati mentre il
/// portatile era staccato dal wifi.
pub fn cerca(fornitori: &Fornitori, cercato: &Cercato<'_>) -> Result<Option<Voce>, AppError> {
    cerca_con(fornitori, cercato, Memoria::Vale)
}

/// Come [`cerca`], ma **senza rileggere** quel che il deposito ricorda.
///
/// È il gesto di chi ha davanti un pannello vuoto e preme «Cerca di nuovo».
/// Senza questa via il pulsante non avrebbe niente da fare: un «non ce l'ho»
/// resta in deposito tre giorni e una risposta una settimana, quindi il
/// ritentativo avrebbe riletto la stessa risposta di prima e chi ha premuto
/// avrebbe visto lo stesso vuoto, concludendo che il pulsante è finto.
///
/// Il deposito si **riscrive** lo stesso: saltarlo in lettura non vuol dire
/// smettere di ricordare, vuol dire non fidarsi di quel che si ricordava
/// adesso. La cadenza e l'interruttore valgono identici — sono proprietà del
/// servizio, non della domanda — quindi anche questa via aspetta il suo quarto
/// di secondo e si ferma davanti a un interruttore aperto.
///
/// # Errori
///
/// Gli stessi di [`cerca`].
pub fn cerca_di_nuovo(
    fornitori: &Fornitori,
    cercato: &Cercato<'_>,
) -> Result<Option<Voce>, AppError> {
    cerca_con(fornitori, cercato, Memoria::Salta)
}

/// Il corpo delle due, con la sola differenza che le distingue.
///
/// # I due giri, e perché il secondo esiste
///
/// «Wattershed (Live at Reading Festival, London, UK - August 1995)» è il
/// titolo che sta nei tag di un file vero, ed è il caso da cui questo pezzo è
/// nato. Quella parentesi descrive **un'esecuzione**: dice dove e quando è
/// stata suonata. Il testo cantato però è quello della canzone, e nel catalogo
/// nessuno ha caricato una voce con quel nome per esteso — quindi tutt'e due le
/// domande del primo giro tornano vuote, e il pannello resta bianco per un
/// brano di cui LRCLIB il testo ce l'ha.
///
/// Da qui il secondo giro, con il titolo sfrondato da
/// [`senza_decorazioni`]: gruppi fra parentesi via, e la coda dopo un trattino
/// isolato quando parla di un'edizione.
///
/// # Perché non si sfronda subito, e perché non si tocca `titolo_da_cercare`
///
/// Sono la stessa domanda, e hanno la stessa risposta: **l'ordine è
/// l'informazione**. Se il catalogo ha una voce per quella esecuzione esatta,
/// quella è la voce giusta — è il testo di *quel* concerto, con gli
/// intercalari e i versi cambiati che un live ha — e prenderla vince su
/// qualunque ripiego. Il secondo giro parte solo dopo un vuoto.
///
/// E `aether_domain::enrich::titolo_da_cercare` resta com'è, che è il fatto
/// nuovo da mettere per iscritto: **le due dottrine sono opposte, e hanno
/// tutt'e due ragione, perché cercano due cose diverse.** Quella funzione serve
/// all'arricchimento, che cerca una **pubblicazione**: là togliere `(Live)` è
/// un errore, perché farebbe trovare il disco di studio e attaccare a un file
/// dal vivo i metadati di un'altra incisione. Qui si cerca un **testo cantato**,
/// che di quella distinzione non sa niente: le parole di «Wattershed» sono le
/// parole di «Wattershed», al Reading Festival come in studio.
///
/// La difesa contro l'abuso non è quindi il titolo: è il **veto di durata** di
/// [`aether_domain::testo::scegli`], che resta identico nei due giri. Sfrondando
/// si allarga il campo, quindi quel veto conta più di prima, non meno — ed è
/// giusto che una registrazione dal vivo lunga il doppio dello studio faccia
/// rinunciare invece di agganciare il testo sbagliato.
fn cerca_con(
    fornitori: &Fornitori,
    cercato: &Cercato<'_>,
    memoria: Memoria,
) -> Result<Option<Voce>, AppError> {
    if cercato.titolo.trim().is_empty() {
        return Ok(None);
    }
    // Senza artista la domanda esatta non si può fare — `/api/get` lo vuole — e
    // quella generosa per titolo e artista vuoto risponde con niente. Resta la
    // ricerca libera, che guarda il titolo da solo: vedi [`senza_artista`].
    if cercato.artista.trim().is_empty() {
        return senza_artista(fornitori, cercato, memoria);
    }
    // Un `Err` non passa al secondo giro: «non si sa» non è «non c'è», e
    // chiedere una seconda volta a una rete che non ha risposto costa due
    // scadenze invece di una per lo stesso silenzio.
    if let Some(voce) = un_giro(fornitori, cercato, memoria, Giro::ComeSta)? {
        return Ok(Some(voce));
    }

    // Si riusa `senza_decorazioni` invece di scriverne una seconda: è la
    // funzione che `normalize_for_match` compone già dentro di sé, quindi lo
    // sfrondamento della domanda e quello della chiave **non possono**
    // divergere. Due normalizzazioni che si allontanano di un carattere sono
    // il difetto che le chiavi di cache qui sopra esistono per non avere.
    let sfrondato = senza_decorazioni(cercato.titolo);
    let sfrondato = sfrondato.trim();
    // Niente da sfrondare, o non è rimasto niente: il secondo giro sarebbe la
    // stessa richiesta sotto un'altra chiave, cioè traffico per nulla.
    let titolo = if sfrondato.is_empty() || sfrondato == cercato.titolo.trim() {
        cercato.titolo.trim()
    } else {
        if let Some(voce) = un_giro(
            fornitori,
            &Cercato {
                titolo: sfrondato,
                ..*cercato
            },
            memoria,
            Giro::Sfrondato,
        )? {
            return Ok(Some(voce));
        }
        sfrondato
    };

    // ── il terzo giro: l'artista principale ──
    // «Caparezza, Diego Perrone» nei tag, «Caparezza» nel catalogo: la domanda
    // esatta fallisce sull'artista e la generosa ha bisogno che l'artista
    // somigli. Si riprova con il solo primo interprete, e con il titolo più
    // corto dei due giri di prima — quel che ha più probabilità di esserci.
    let principale = primo_artista(cercato.artista);
    if principale.is_empty() || principale == cercato.artista.trim() {
        return Ok(None);
    }
    un_giro(
        fornitori,
        &Cercato {
            titolo,
            artista: principale,
            ..*cercato
        },
        memoria,
        Giro::ArtistaPrincipale,
    )
}

/// La ricerca per titolo soltanto, quando l'artista non si sa.
///
/// È il caso dei file senza tag, che sono quelli che il testo non ce l'hanno: il
/// titolo arriva dal nome del file e l'artista non c'è. Prima la domanda partiva
/// lo stesso con «Artista sconosciuto» come artista, cioè una richiesta spesa per
/// una risposta vuota garantita, e il brano si segnava come cercato per due
/// settimane.
///
/// `q=` guarda titolo, artista e album insieme, e risponde con parecchio: a
/// restringere ci pensa [`testo::scegli`], che con l'artista vuoto non gli dà
/// punti e quindi pretende titolo e durata quasi perfetti. Senza la durata non
/// si prova nemmeno: un titolo solo, come «Home», è di mille canzoni.
fn senza_artista(
    fornitori: &Fornitori,
    cercato: &Cercato<'_>,
    memoria: Memoria,
) -> Result<Option<Voce>, AppError> {
    if cercato.durata_ms.is_none_or(|d| d == 0) {
        return Ok(None);
    }
    let corpo = fornitori.json_con_memoria(
        &fornitori.lrclib,
        "lrclib-ricerca-titolo",
        &chiave_ricerca(cercato),
        &url_ricerca_libera(cercato.titolo),
        VIVE_RICERCA_MS,
        memoria,
    )?;
    let Some(corpo) = corpo else {
        return Ok(None);
    };
    let voci = interpreta(&corpo);
    let candidati: Vec<Candidato> = voci.iter().map(|v| v.candidato.clone()).collect();
    Ok(testo::scegli(&candidati, cercato)
        .and_then(|scelto| voci.into_iter().nth(scelto))
        .filter(|voce| !voce.e_vuota()))
}

/// L'URL della ricerca libera.
fn url_ricerca_libera(testo: &str) -> String {
    format!("https://lrclib.net/api/search?q={}", percento(testo))
}

/// Quante voci al massimo si mostrano a chi sceglie a mano.
///
/// Venti: una ricerca per un titolo comune ne restituisce anche cento — cover,
/// karaoke, remix — e oltre la ventesima, ordinate come le ordina [`candidati`],
/// non c'è più niente che somigli al brano.
const CANDIDATI_MASSIMI: usize = 20;

/// Tutte le voci che il catalogo ha per questo brano, per sceglierne una a mano.
///
/// # Quando serve
///
/// Quando [`cerca`] ha scelto male, o non ha scelto: la scelta automatica passa
/// dai veti di [`testo::scegli`], che sono prudenti apposta — meglio nessun testo
/// che quello di un'altra canzone — e la prudenza qualche volta scarta la voce
/// giusta. Chi ascolta la riconosce a colpo d'occhio; il programma no.
///
/// # Cosa torna
///
/// Le voci **non vuote** delle ricerche generose con il titolo com'è, sfrondato,
/// e con il solo artista principale; senza artista, la ricerca libera. Senza
/// doppioni, con quelle che hanno i tempi prima, e a parità la durata più
/// vicina a quella del file. Nessun veto: è chi guarda che decide.
///
/// Salta sempre il deposito in lettura: chi apre questo elenco lo fa perché
/// quel che si ricordava non gli è andato bene.
///
/// # Errori
///
/// Gli stessi di [`cerca`]. Un giro che fallisce dopo che un altro ha già
/// risposto non fa perdere quel che si era trovato.
pub fn candidati(fornitori: &Fornitori, cercato: &Cercato<'_>) -> Result<Vec<Voce>, AppError> {
    let titolo = cercato.titolo.trim();
    if titolo.is_empty() {
        return Ok(Vec::new());
    }
    let sfrondato = senza_decorazioni(titolo);
    let principale = primo_artista(cercato.artista);

    // Le domande, come coppie (servizio, cercato, url): le stesse chiavi dei giri
    // di `cerca`, così una risposta presa qui vale anche là, e viceversa.
    let mut domande: Vec<(&str, Cercato<'_>, String)> = Vec::new();
    if cercato.artista.trim().is_empty() {
        domande.push((
            "lrclib-ricerca-titolo",
            *cercato,
            url_ricerca_libera(titolo),
        ));
    } else {
        domande.push((Giro::ComeSta.ricerca(), *cercato, url_ricerca(cercato)));
        if !sfrondato.trim().is_empty() && sfrondato.trim() != titolo {
            let domanda = Cercato {
                titolo: sfrondato.trim(),
                ..*cercato
            };
            domande.push((Giro::Sfrondato.ricerca(), domanda, url_ricerca(&domanda)));
        }
        if !principale.is_empty() && principale != cercato.artista.trim() {
            let domanda = Cercato {
                artista: principale,
                ..*cercato
            };
            domande.push((
                Giro::ArtistaPrincipale.ricerca(),
                domanda,
                url_ricerca(&domanda),
            ));
        }
    }

    let mut voci: Vec<Voce> = Vec::new();
    let mut guasto: Option<AppError> = None;
    for (servizio, domanda, url) in &domande {
        let risposta = fornitori.json_con_memoria(
            &fornitori.lrclib,
            servizio,
            &chiave_ricerca(domanda),
            url,
            VIVE_RICERCA_MS,
            Memoria::Salta,
        );
        match risposta {
            Ok(Some(corpo)) => {
                for voce in interpreta(&corpo) {
                    let nuova = !voci.iter().any(|v| v.candidato.id == voce.candidato.id);
                    if nuova && !voce.e_vuota() {
                        voci.push(voce);
                    }
                }
            }
            Ok(None) => {}
            Err(err) => guasto = Some(err),
        }
    }
    if voci.is_empty()
        && let Some(err) = guasto
    {
        return Err(err);
    }

    voci.sort_by_key(|voce| {
        (
            !voce.candidato.sincronizzato,
            match (cercato.durata_ms, voce.candidato.durata_ms) {
                (Some(nostra), Some(sua)) => nostra.abs_diff(sua),
                _ => u64::MAX,
            },
        )
    });
    voci.truncate(CANDIDATI_MASSIMI);
    Ok(voci)
}

/// Un giro di domande — l'esatta, poi la generosa — con un titolo solo.
fn un_giro(
    fornitori: &Fornitori,
    cercato: &Cercato<'_>,
    memoria: Memoria,
    giro: Giro,
) -> Result<Option<Voce>, AppError> {
    let chiedi = |servizio: &str, chiave: &str, url: &str| {
        fornitori.json_con_memoria(
            &fornitori.lrclib,
            servizio,
            chiave,
            url,
            VIVE_RICERCA_MS,
            memoria,
        )
    };

    // ── la domanda esatta ───────────────────────────────────────────────────
    // Quel che l'esatta ha risposto quando la risposta non porta i tempi: si
    // tiene in mano fino alla fine invece di restituirlo subito, perché è la
    // risposta di riserva e non ancora la risposta.
    let mut piatta: Option<Voce> = None;
    if let Some(durata_ms) = cercato.durata_ms.filter(|d| *d > 0) {
        let url = url_esatta(cercato, durata_ms);
        let corpo = chiedi(giro.esatta(), &chiave_esatta(cercato), &url)?;
        if let Some(corpo) = corpo
            && let Some(voce) = interpreta(&corpo).into_iter().next()
            && !voce.e_vuota()
        {
            // Uno strumentale è una risposta compiuta come una con i tempi: non
            // c'è nessun LRC migliore da andare a cercare per un brano che le
            // parole non le ha.
            if voce.candidato.sincronizzato || voce.candidato.strumentale {
                return Ok(Some(voce));
            }
            piatta = Some(voce);
        }
    }

    // ── la domanda generosa ─────────────────────────────────────────────────
    let url = url_ricerca(cercato);
    let risposta = chiedi(giro.ricerca(), &chiave_ricerca(cercato), &url);
    let corpo = match risposta {
        Ok(Some(corpo)) => corpo,
        Ok(None) => return Ok(piatta),
        // Un guasto qui non deve cancellare quel che l'esatta aveva già
        // risposto: la seconda domanda serviva a **migliorare** una risposta
        // che si aveva in mano, e un `Err` la butterebbe via — chi chiama lo
        // legge come «non si sa» e non scrive niente, cioè un brano senza testo
        // invece di un brano con il testo piatto. Quando invece non c'era
        // niente da migliorare, l'errore è tutto quel che si ha, e si propaga.
        Err(err) => return piatta.map_or(Err(err), |voce| Ok(Some(voce))),
    };

    let voci = interpreta(&corpo);
    let candidati: Vec<Candidato> = voci.iter().map(|v| v.candidato.clone()).collect();
    let scelta = testo::scegli(&candidati, cercato)
        .and_then(|scelto| voci.into_iter().nth(scelto))
        .filter(|voce| !voce.e_vuota());
    match scelta {
        // Con i tempi vince sempre: è tutto il motivo per cui questa seconda
        // domanda si fa anche quando la prima aveva già risposto. Senza tempi
        // vince solo se non c'era niente prima — fra due testi piatti quello
        // dell'esatta è del brano giusto per costruzione, questo ci somiglia e
        // basta.
        Some(voce) if voce.candidato.sincronizzato || piatta.is_none() => Ok(Some(voce)),
        _ => Ok(piatta),
    }
}

// ── restituire ──────────────────────────────────────────────────────────────

/// Quanti tentativi al massimo per la prova di lavoro.
///
/// Il bersaglio di LRCLIB è `000000FF…`: servono in media una manciata di
/// milioni di hash, cioè qualche secondo. Cinquanta milioni sono un tetto largo
/// dieci volte, e servono a non lasciare un filo a girare per sempre se un
/// giorno il bersaglio si stringesse.
const TENTATIVI_MASSIMI: u64 = 50_000_000;

/// Un testo da restituire al catalogo.
#[derive(Debug, Clone, Copy)]
pub struct DaPubblicare<'a> {
    /// Il titolo, come sta nei tag del file.
    pub titolo: &'a str,
    /// L'artista.
    pub artista: &'a str,
    /// L'album.
    pub album: &'a str,
    /// La durata del file, in millisecondi.
    pub durata_ms: u64,
    /// Il testo senza tempi.
    pub piatto: &'a str,
    /// L'LRC.
    pub sincronizzato: &'a str,
}

/// Restituisce al catalogo un testo sincronizzato.
///
/// # Perché c'è una prova di lavoro, e perché va bene così
///
/// LRCLIB non chiede un account: chiunque può contribuire, ed è quel che lo
/// rende utile. Il prezzo è che chiunque potrebbe anche riempirlo di spazzatura,
/// e la difesa è far costare **qualche secondo di calcolo** ogni invio. È una
/// difesa onesta — non tocca chi manda un testo alla volta, e rende impraticabile
/// mandarne centomila.
///
/// Da qui la conseguenza che va detta a chi preme il pulsante: la pubblicazione
/// non è istantanea, e non perché la rete è lenta.
///
/// # Errori
///
/// `metadata.lyricsPublishRefused` se il catalogo rifiuta il contenuto o se la
/// prova di lavoro non si chiude nei tentativi concessi; l'errore di trasporto
/// così com'è quando la rete non risponde.
pub fn pubblica(fornitori: &Fornitori, cosa: &DaPubblicare<'_>) -> Result<(), AppError> {
    let (prefisso, bersaglio) = sfida(fornitori)?;
    let Some(nonce) = risolvi(&prefisso, &bersaglio) else {
        return Err(AppError::new(ErrorCode::MetadataLyricsPublishRefused {
            detail: Some("la prova di lavoro non si è chiusa".to_owned()),
        }));
    };
    let gettone = format!("{prefisso}:{nonce}");

    let corpo = serde_json::json!({
        "trackName": cosa.titolo,
        "artistName": cosa.artista,
        "albumName": cosa.album,
        "duration": secondi(cosa.durata_ms),
        "plainLyrics": cosa.piatto,
        "syncedLyrics": cosa.sincronizzato,
    })
    .to_string();

    fornitori.lrclib.attendi();
    let risposta = fornitori.rete.esegui(Richiesta {
        metodo: Metodo::Post,
        url: "https://lrclib.net/api/publish",
        intestazioni: &[
            ("Content-Type", "application/json"),
            ("X-Publish-Token", &gettone),
        ],
        corpo: Corpo::Byte {
            tipo: "application/json",
            dati: corpo.as_bytes(),
        },
    });
    let risposta = match risposta {
        Ok(risposta) => risposta,
        Err(err) => {
            fornitori.lrclib.guasto();
            return Err(err);
        }
    };
    fornitori.lrclib.riuscita();

    if risposta.e_andata() {
        return Ok(());
    }
    // Un rifiuto sul contenuto non è un guasto del servizio: il servizio ha
    // funzionato benissimo, ha solo detto di no. Distinguerli conta perché
    // l'interruttore della cadenza si apre sui guasti, e aprirlo qui
    // spegnerebbe anche le **ricerche** per cinque minuti.
    Err(AppError::new(ErrorCode::MetadataLyricsPublishRefused {
        detail: Some(format!("il catalogo ha risposto {}", risposta.stato)),
    })
    .with_cause(risposta.testo()))
}

/// Chiede una sfida: il prefisso da cui partire e il bersaglio da battere.
fn sfida(fornitori: &Fornitori) -> Result<(String, String), AppError> {
    fornitori.lrclib.attendi();
    let risposta = match fornitori.rete.esegui(Richiesta {
        metodo: Metodo::Post,
        url: "https://lrclib.net/api/request-challenge",
        intestazioni: &[("Accept", "application/json")],
        corpo: Corpo::Niente,
    }) {
        Ok(risposta) => risposta,
        Err(err) => {
            fornitori.lrclib.guasto();
            return Err(err);
        }
    };
    if !risposta.e_andata() {
        let errore = fornitori
            .rete
            .stato_a_errore(&risposta, "https://lrclib.net/api/request-challenge");
        if errore.is_retryable() {
            fornitori.lrclib.guasto();
        } else {
            fornitori.lrclib.riuscita();
        }
        return Err(errore);
    }
    fornitori.lrclib.riuscita();

    let letto: Value = serde_json::from_slice(&risposta.corpo).map_err(|err| {
        AppError::new(ErrorCode::MetadataLyricsPublishRefused {
            detail: Some("la sfida non si è letta".to_owned()),
        })
        .with_cause(err.to_string())
    })?;
    let prefisso = letto.get("prefix").and_then(Value::as_str);
    let bersaglio = letto.get("target").and_then(Value::as_str);
    match (prefisso, bersaglio) {
        (Some(prefisso), Some(bersaglio)) => Ok((prefisso.to_owned(), bersaglio.to_owned())),
        _ => Err(AppError::new(ErrorCode::MetadataLyricsPublishRefused {
            detail: Some("la sfida non aveva prefisso e bersaglio".to_owned()),
        })),
    }
}

/// Cerca il numero che, unito al prefisso, dà un'impronta sotto il bersaglio.
///
/// # La regola, per esteso
///
/// Si prova `SHA-256(prefisso ‖ nonce)` per `nonce` = 0, 1, 2… e si confronta
/// l'impronta con il bersaglio **byte per byte, dal più significativo**: è un
/// confronto fra due numeri a 256 bit scritti in ordine di rete, e in Rust è
/// esattamente l'ordinamento naturale di due `[u8; 32]`.
///
/// `None` se il bersaglio non è trentadue byte in esadecimale, o se
/// [`TENTATIVI_MASSIMI`] non bastano.
///
/// # Perché è pura
///
/// Perché è l'unica parte di questo modulo che si può sbagliare in silenzio: un
/// confronto al contrario troverebbe subito un nonce che il catalogo rifiuta, e
/// il sintomo sarebbe «la pubblicazione non funziona» senza nessun errore da
/// leggere. Separata dalla rete, si prova con un bersaglio facile.
#[must_use]
pub fn risolvi(prefisso: &str, bersaglio_hex: &str) -> Option<String> {
    let bersaglio = da_esadecimale(bersaglio_hex)?;
    let mut nonce = 0_u64;
    while nonce < TENTATIVI_MASSIMI {
        let mut impronta = Sha256::new();
        impronta.update(prefisso.as_bytes());
        impronta.update(nonce.to_string().as_bytes());
        let uscita: [u8; 32] = impronta.finalize().into();
        if uscita < bersaglio {
            return Some(nonce.to_string());
        }
        nonce = nonce.saturating_add(1);
    }
    None
}

/// Trentadue byte da una stringa esadecimale, o niente.
fn da_esadecimale(grezzo: &str) -> Option<[u8; 32]> {
    let grezzo = grezzo.trim();
    if grezzo.len() != 64 {
        return None;
    }
    let mut fuori = [0_u8; 32];
    let cifre: Vec<u8> = grezzo
        .bytes()
        .map(|b| (b as char).to_digit(16))
        .collect::<Option<Vec<u32>>>()?
        .into_iter()
        .map(|c| u8::try_from(c).unwrap_or(0))
        .collect();
    for (posto, coppia) in fuori.iter_mut().zip(cifre.as_chunks::<2>().0) {
        let alto = coppia.first().copied().unwrap_or(0);
        let basso = coppia.get(1).copied().unwrap_or(0);
        *posto = alto.saturating_mul(16).saturating_add(basso);
    }
    Some(fuori)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Una risposta come quella vera, con i campi che il catalogo manda
    /// davvero. Le parole sono inventate: qui si prova la forma, non il testo.
    const RISPOSTA: &str = r#"[
      {"id": 1, "name": "Titolo", "trackName": "Titolo", "artistName": "Artista",
       "albumName": "Album", "duration": 240.0, "instrumental": false,
       "plainLyrics": "prima riga\nseconda riga",
       "syncedLyrics": "[00:01.00]prima riga\n[00:02.00]seconda riga"},
      {"id": 2, "name": "Titolo", "trackName": "Titolo", "artistName": "Artista",
       "albumName": "Album", "duration": 240.0, "instrumental": false,
       "plainLyrics": "prima riga\nseconda riga", "syncedLyrics": null}
    ]"#;

    #[test]
    fn i_secondi_con_la_virgola_diventano_millisecondi() {
        let voci = interpreta(RISPOSTA.as_bytes());
        assert_eq!(voci.len(), 2);
        assert_eq!(
            voci.first().and_then(|v| v.candidato.durata_ms),
            Some(240_000)
        );
    }

    #[test]
    fn una_voce_sola_si_legge_come_un_elenco_di_uno() {
        let uno = r#"{"id": 7, "trackName": "Titolo", "artistName": "Artista",
                      "duration": 12.5, "instrumental": false,
                      "syncedLyrics": "[00:01.00]prima riga"}"#;
        let voci = interpreta(uno.as_bytes());
        assert_eq!(voci.len(), 1);
        assert_eq!(voci.first().map(|v| v.candidato.id), Some(7));
        assert_eq!(
            voci.first().and_then(|v| v.candidato.durata_ms),
            Some(12_500)
        );
        assert!(voci.first().is_some_and(|v| v.candidato.sincronizzato));
    }

    #[test]
    fn chi_non_ha_i_tempi_lo_dichiara() {
        let voci = interpreta(RISPOSTA.as_bytes());
        assert!(voci.last().is_some_and(|v| !v.candidato.sincronizzato));
        assert!(voci.last().is_some_and(|v| v.sincronizzato.is_none()));
    }

    #[test]
    fn una_risposta_che_non_e_json_non_fa_cadere_niente() {
        assert!(interpreta(b"non json").is_empty());
        assert!(interpreta(b"").is_empty());
        assert!(interpreta(b"[]").is_empty());
        // Un oggetto senza i campi che contano non è una voce.
        assert!(interpreta(br#"{"code": 404, "name": "TrackNotFound"}"#).is_empty());
    }

    #[test]
    fn una_durata_impossibile_vale_come_nessuna_durata() {
        let strane = r#"[
          {"trackName": "A", "artistName": "B", "duration": 0.0},
          {"trackName": "A", "artistName": "B", "duration": -3.0},
          {"trackName": "A", "artistName": "B", "duration": 999999.0},
          {"trackName": "A", "artistName": "B"}
        ]"#;
        for voce in interpreta(strane.as_bytes()) {
            assert_eq!(voce.candidato.durata_ms, None);
        }
    }

    #[test]
    fn uno_strumentale_senza_testo_non_e_una_voce_vuota() {
        let voci = interpreta(
            br#"{"trackName": "A", "artistName": "B", "instrumental": true,
                 "plainLyrics": null, "syncedLyrics": null}"#,
        );
        assert!(voci.first().is_some_and(|v| !v.e_vuota()));
        // Mentre una voce che non è strumentale e non porta niente lo è.
        let vuote = interpreta(br#"{"trackName": "A", "artistName": "B"}"#);
        assert!(vuote.first().is_some_and(Voce::e_vuota));
    }

    #[test]
    fn la_prova_di_lavoro_trova_un_nonce_sotto_il_bersaglio() {
        // Bersaglio facilissimo: quasi ogni impronta ci sta sotto, e la prova
        // resta veloce. Quel che si verifica è il **verso** del confronto.
        let facile = "F0".to_owned() + &"00".repeat(31);
        let nonce = risolvi("prova", &facile).expect("un nonce");
        let mut impronta = Sha256::new();
        impronta.update(b"prova");
        impronta.update(nonce.as_bytes());
        let uscita: [u8; 32] = impronta.finalize().into();
        let bersaglio = da_esadecimale(&facile).expect("bersaglio");
        assert!(
            uscita < bersaglio,
            "l'impronta deve stare sotto il bersaglio"
        );
    }

    #[test]
    fn un_bersaglio_impossibile_non_gira_per_sempre() {
        // Tutto zero: nessuna impronta ci sta sotto, e la ricerca deve
        // arrendersi invece di occupare un filo all'infinito. Con il tetto vero
        // ci metterebbe minuti, quindi qui si prova la strada dell'esadecimale
        // storto, che è l'altro modo in cui questa funzione dice di no.
        assert_eq!(risolvi("prova", "non esadecimale"), None);
        assert_eq!(risolvi("prova", ""), None);
        assert_eq!(risolvi("prova", "00"), None);
    }

    #[test]
    fn il_bersaglio_si_legge_dal_byte_piu_significativo() {
        assert_eq!(
            da_esadecimale("000000FF"),
            None,
            "un bersaglio corto non è un bersaglio"
        );

        let intero = "000000FF".to_owned() + &"00".repeat(28);
        let letto = da_esadecimale(&intero).expect("trentadue byte");
        assert_eq!(letto.first(), Some(&0));
        assert_eq!(letto.get(3), Some(&255));
        assert_eq!(letto.get(31), Some(&0));
    }

    // ── le due domande, senza rete ──────────────────────────────────────────

    /// Cosa il deposito finto sa di un servizio.
    ///
    /// Le tre voci sono i tre esiti che [`Fornitori::json`] distingue prima di
    /// toccare la rete, e servono tutt'e tre: `Corpo` è la risposta ricordata,
    /// `Niente` è il «non ce l'ho» ricordato, `Sconosciuta` è l'unica che
    /// lascerebbe partire una richiesta vera — ed è quella con cui si prova
    /// cosa succede quando la seconda domanda non si può fare.
    enum Finta {
        Corpo(&'static str),
        Niente,
        Sconosciuta,
    }

    /// Un deposito che risponde quel che gli si è messo dentro.
    ///
    /// Serve a provare [`cerca`] **senza rete**: `json` guarda prima qui, e una
    /// voce trovata — anche `Niente` — gli fa saltare la richiesta. Un servizio
    /// che non sta nella mappa vale `Niente`, non `Sconosciuta`: così una prova
    /// scritta male fallisce invece di andare a bussare a LRCLIB.
    struct Finto {
        risposte: std::collections::HashMap<&'static str, Finta>,
        chieste: std::sync::Mutex<Vec<String>>,
    }

    impl Finto {
        /// I servizi a cui si è chiesto, nell'ordine.
        fn chieste(&self) -> Vec<String> {
            self.chieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl crate::Deposito for std::sync::Arc<Finto> {
        fn leggi(&self, servizio: &str, _chiave: &str) -> Option<crate::deposito::Voce> {
            self.chieste
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(servizio.to_owned());
            match self.risposte.get(servizio) {
                Some(Finta::Corpo(corpo)) => {
                    Some(crate::deposito::Voce::Corpo(corpo.as_bytes().to_vec()))
                }
                Some(Finta::Sconosciuta) => None,
                Some(Finta::Niente) | None => Some(crate::deposito::Voce::Niente),
            }
        }

        fn scrivi(&self, _servizio: &str, _chiave: &str, _voce: &crate::deposito::Voce, _ms: i64) {}
    }

    /// I fornitori attorno a un deposito finto, e il finto per interrogarlo dopo.
    fn finti(risposte: Vec<(&'static str, Finta)>) -> (Fornitori, std::sync::Arc<Finto>) {
        let finto = std::sync::Arc::new(Finto {
            risposte: risposte.into_iter().collect(),
            chieste: std::sync::Mutex::new(Vec::new()),
        });
        (
            Fornitori::nuovo(Box::new(std::sync::Arc::clone(&finto))),
            finto,
        )
    }

    /// Il brano che si sta cercando in tutte le prove qui sotto.
    const fn brano() -> Cercato<'static> {
        Cercato {
            titolo: "Titolo",
            artista: "Artista",
            album: Some("Album"),
            durata_ms: Some(240_000),
        }
    }

    /// Una voce sola, come la manda `/api/get`.
    const ESATTA_PIATTA: &str = r#"{"id": 1, "trackName": "Titolo", "artistName": "Artista",
         "albumName": "Album", "duration": 240.0, "instrumental": false,
         "plainLyrics": "prima riga\nseconda riga", "syncedLyrics": null}"#;

    /// La stessa voce, ma con i tempi.
    const ESATTA_CON_TEMPI: &str = r#"{"id": 1, "trackName": "Titolo", "artistName": "Artista",
         "albumName": "Album", "duration": 240.0, "instrumental": false,
         "plainLyrics": "prima riga", "syncedLyrics": "[00:01.00]prima riga"}"#;

    /// Un elenco come lo manda `/api/search`: la stessa canzone caricata due
    /// volte, con l'album scritto in un altro modo, e la seconda ha l'LRC. È il
    /// caso vero — LRCLIB tiene più voci per brano — e la ragione di tutto
    /// quello che segue.
    const RICERCA_CON_TEMPI: &str = r#"[
      {"id": 2, "trackName": "Titolo", "artistName": "Artista", "albumName": "Album",
       "duration": 240.0, "instrumental": false, "plainLyrics": "prima riga", "syncedLyrics": null},
      {"id": 3, "trackName": "Titolo", "artistName": "Artista", "albumName": "Albumm",
       "duration": 240.3, "instrumental": false, "plainLyrics": "prima riga",
       "syncedLyrics": "[00:01.00]prima riga\n[00:02.00]seconda riga"}
    ]"#;

    #[test]
    fn una_risposta_esatta_senza_tempi_non_ferma_la_ricerca() {
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(ESATTA_PIATTA)),
            ("lrclib-ricerca", Finta::Corpo(RICERCA_CON_TEMPI)),
        ]);
        let voce = cerca(&fornitori, &brano())
            .expect("nessun guasto")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 3, "vince la voce che porta i tempi");
        assert!(voce.sincronizzato.is_some());
        assert!(
            finto.chieste().iter().any(|s| s == "lrclib-ricerca"),
            "la seconda domanda si deve fare"
        );
    }

    #[test]
    fn una_risposta_esatta_con_i_tempi_chiude_la_ricerca() {
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(ESATTA_CON_TEMPI)),
            ("lrclib-ricerca", Finta::Corpo(RICERCA_CON_TEMPI)),
        ]);
        let voce = cerca(&fornitori, &brano())
            .expect("nessun guasto")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 1);
        assert!(
            !finto.chieste().iter().any(|s| s == "lrclib-ricerca"),
            "non c'è niente di meglio da cercare, e la richiesta non si spende"
        );
    }

    #[test]
    fn uno_strumentale_esatto_chiude_la_ricerca() {
        let strumentale = r#"{"id": 1, "trackName": "Titolo", "artistName": "Artista",
             "albumName": "Album", "duration": 240.0, "instrumental": true,
             "plainLyrics": null, "syncedLyrics": null}"#;
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(strumentale)),
            ("lrclib-ricerca", Finta::Corpo(RICERCA_CON_TEMPI)),
        ]);
        let voce = cerca(&fornitori, &brano())
            .expect("nessun guasto")
            .expect("una voce");
        assert!(voce.candidato.strumentale);
        assert!(!finto.chieste().iter().any(|s| s == "lrclib-ricerca"));
    }

    #[test]
    fn se_la_ricerca_non_porta_tempi_resta_la_piatta_dell_esatta() {
        // La ricerca risponde con una voce che somiglia ma non ha i tempi: fra
        // due testi piatti si tiene quello della domanda esatta, che è del
        // brano giusto per costruzione.
        let solo_piatte = r#"[
          {"id": 2, "trackName": "Titolo", "artistName": "Artista", "albumName": "Album",
           "duration": 240.0, "instrumental": false, "plainLyrics": "altra riga",
           "syncedLyrics": null}
        ]"#;
        let (fornitori, _) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(ESATTA_PIATTA)),
            ("lrclib-ricerca", Finta::Corpo(solo_piatte)),
        ]);
        let voce = cerca(&fornitori, &brano())
            .expect("nessun guasto")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 1);
    }

    #[test]
    fn se_la_ricerca_non_trova_niente_resta_la_piatta_dell_esatta() {
        let (fornitori, _) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(ESATTA_PIATTA)),
            ("lrclib-ricerca", Finta::Niente),
        ]);
        let voce = cerca(&fornitori, &brano())
            .expect("nessun guasto")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 1);
    }

    #[test]
    fn un_guasto_sulla_seconda_domanda_non_butta_via_la_prima_risposta() {
        let (fornitori, _) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(ESATTA_PIATTA)),
            ("lrclib-ricerca", Finta::Sconosciuta),
        ]);
        // L'interruttore aperto è il guasto più facile da mettere in scena, e
        // fa fallire `json` prima di qualunque richiesta vera.
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.lrclib.guasto();
        }
        let voce = cerca(&fornitori, &brano())
            .expect("il testo piatto si tiene lo stesso")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 1);
    }

    #[test]
    fn il_caso_da_cui_e_nata_la_regola() {
        // Le due risposte vere del catalogo per un brano vero, ridotte ai campi
        // che contano: i testi sono sostituiti, i metadati no — sono loro che
        // decidono, e cambiarli vorrebbe dire provare un'altra cosa.
        //
        // `/api/get` con la firma del file — «The Auditels Family», Caparezza,
        // «Habemus Capa», 247 secondi — trova la voce caricata per prima, che è
        // senza tempi. Le altre sei voci dello stesso brano, stessa durata al
        // decimo, l'LRC ce l'hanno: ci si arriva solo con la seconda domanda.
        let esatta = r#"{"id": 670078, "trackName": "The auditels family",
             "artistName": "Caparezza", "albumName": "Habemus Capa", "duration": 247.0,
             "instrumental": false, "plainLyrics": "le parole", "syncedLyrics": null}"#;
        let ricerca = r#"[
          {"id": 670078, "trackName": "The auditels family", "artistName": "Caparezza",
           "albumName": "Habemus Capa", "duration": 247.0, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": null},
          {"id": 27142794, "trackName": "The Auditels Family", "artistName": "Caparezza",
           "albumName": "Greatest Hits", "duration": 247.069325, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": null},
          {"id": 30729488, "trackName": "The Auditels Family", "artistName": "CAPAREZZA",
           "albumName": "Epocalisse", "duration": 247.0, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": "[00:12.00]le parole"},
          {"id": 15787178, "trackName": "The Auditels Family", "artistName": "CapaRezza",
           "albumName": "Hamebus Capa", "duration": 247.3357, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": "[00:12.00]le parole"},
          {"id": 22323216, "trackName": "13 - The Auditels family", "artistName": "Caparezza",
           "albumName": "Habemus Capa", "duration": 247.327347, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": "[00:12.00]le parole"}
        ]"#;
        let (fornitori, _) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(esatta)),
            ("lrclib-ricerca", Finta::Corpo(ricerca)),
        ]);
        let voce = cerca(
            &fornitori,
            &Cercato {
                titolo: "The Auditels Family",
                artista: "Caparezza",
                album: Some("Habemus Capa"),
                durata_ms: Some(247_327),
            },
        )
        .expect("nessun guasto")
        .expect("una voce");
        assert!(
            voce.candidato.sincronizzato,
            "il catalogo i tempi ce li ha: vanno trovati"
        );
        assert_ne!(voce.candidato.id, 670078);
    }

    #[test]
    fn senza_niente_in_mano_il_guasto_si_propaga() {
        let (fornitori, _) = finti(vec![
            ("lrclib-esatta", Finta::Niente),
            ("lrclib-ricerca", Finta::Sconosciuta),
        ]);
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.lrclib.guasto();
        }
        assert!(
            cerca(&fornitori, &brano()).is_err(),
            "«non si sa» non è «non c'è»"
        );
    }

    #[test]
    fn la_chiave_distingue_due_edizioni_dello_stesso_brano() {
        let corta = Cercato {
            titolo: "Titolo",
            artista: "Artista",
            album: None,
            durata_ms: Some(240_000),
        };
        let lunga = Cercato {
            durata_ms: Some(300_000),
            ..corta
        };
        assert_ne!(chiave_ricerca(&corta), chiave_ricerca(&lunga));
        // …e non distingue due scritture della stessa cosa.
        let storta = Cercato {
            titolo: "  titolo  ",
            artista: "ARTISTA",
            ..corta
        };
        assert_eq!(chiave_ricerca(&corta), chiave_ricerca(&storta));
    }

    #[test]
    fn due_edizioni_non_condividono_la_risposta_ricordata() {
        // Stesso titolo, stesso artista, stessa durata al secondo: due
        // ristampe dello stesso brano differiscono **solo** per l'album, e
        // l'album sta nell'URL della domanda esatta.
        let originale = Cercato {
            titolo: "The Auditels Family",
            artista: "Caparezza",
            album: Some("Habemus Capa"),
            durata_ms: Some(247_000),
        };
        let raccolta = Cercato {
            album: Some("Greatest Hits"),
            ..originale
        };
        assert_ne!(
            chiave_esatta(&originale),
            chiave_esatta(&raccolta),
            "due domande diverse non possono avere una risposta sola in cache"
        );
        // E la domanda generosa, che l'album non lo manda, resta una sola: due
        // chiavi per la stessa richiesta sarebbero due richieste dove ne basta
        // una.
        assert_eq!(chiave_ricerca(&originale), chiave_ricerca(&raccolta));
    }

    #[test]
    fn un_album_assente_non_finisce_nell_url() {
        let senza = Cercato {
            titolo: "Titolo",
            artista: "Artista",
            album: None,
            durata_ms: Some(240_000),
        };
        let url = url_esatta(&senza, 240_000);
        assert!(
            !url.contains("album_name"),
            "un album che non c'è non è un album vuoto: {url}"
        );
        // Uno fatto di soli spazi conta come assente: nei tag capita, e
        // mandarlo sarebbe lo stesso `404` garantito.
        let spazi = Cercato {
            album: Some("   "),
            ..senza
        };
        assert!(!url_esatta(&spazi, 240_000).contains("album_name"));

        // Quando invece c'è, ci va — con il percento al posto giusto.
        let con = Cercato {
            album: Some("Habemus Capa"),
            ..senza
        };
        assert!(url_esatta(&con, 240_000).contains("album_name=Habemus%20Capa"));
        // La durata resta in secondi interi, come il catalogo la vuole.
        assert!(url_esatta(&con, 240_400).contains("duration=240"));
    }

    // ── il secondo giro, col titolo sfrondato ───────────────────────────────

    /// Il titolo come sta nei tag del file da cui è nato il punto #4.
    ///
    /// La parentesi descrive **un'esecuzione**: dove e quando è stata suonata.
    /// Nessuno ha caricato in catalogo una voce con quel nome per esteso.
    const DAL_VIVO: &str = "Wattershed (Live at Reading Festival, London, UK - August 1995)";

    /// Il brano dal vivo: tre minuti e cinquantuno, come la registrazione.
    const fn concerto() -> Cercato<'static> {
        Cercato {
            titolo: DAL_VIVO,
            artista: "Foo Fighters",
            album: Some("Live at Reading"),
            durata_ms: Some(231_000),
        }
    }

    #[test]
    fn un_titolo_dal_vivo_ripiega_sul_titolo_sfrondato() {
        // Il catalogo, sotto il titolo per esteso, non ha niente: né la
        // domanda esatta né quella generosa. È il pannello bianco che il
        // punto #4 doveva chiudere.
        //
        // Sotto «Wattershed» invece la voce c'è, con la durata di **questa**
        // esecuzione: qualcuno l'ha caricata dal disco dal vivo, col titolo
        // scritto in modo semplice.
        let sfrondata = r#"[
          {"id": 991, "trackName": "Wattershed", "artistName": "Foo Fighters",
           "albumName": "Live at Reading", "duration": 231.0, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": "[00:12.00]le parole"}
        ]"#;
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Niente),
            ("lrclib-ricerca", Finta::Niente),
            ("lrclib-esatta-sfrondata", Finta::Niente),
            ("lrclib-ricerca-sfrondata", Finta::Corpo(sfrondata)),
        ]);

        let voce = cerca(&fornitori, &concerto())
            .expect("nessun guasto")
            .expect("il catalogo il testo ce l'ha, sotto l'altro nome");
        assert_eq!(voce.candidato.id, 991);
        assert!(voce.candidato.sincronizzato);

        // E i due giri hanno chiesto a **quattro** servizi diversi, non a due:
        // è quel che tiene separate le due risposte in deposito. Con una
        // chiave sola il «non c'è» del primo giro avrebbe risposto anche al
        // secondo — `normalize_for_match` le parentesi le toglie già, quindi i
        // due titoli danno la stessa chiave.
        let chieste = finto.chieste();
        for servizio in [
            "lrclib-esatta",
            "lrclib-ricerca",
            "lrclib-esatta-sfrondata",
            "lrclib-ricerca-sfrondata",
        ] {
            assert!(
                chieste.iter().any(|s| s == servizio),
                "manca la domanda a {servizio}: {chieste:?}"
            );
        }
    }

    #[test]
    fn il_ripiego_non_scavalca_un_esito_esatto() {
        // Il catalogo ha la voce di **questa** esecuzione, col titolo per
        // esteso: è il testo di quel concerto, con gli intercalari e i versi
        // cambiati che un live ha, e vince su qualunque ripiego.
        let esatta_dal_vivo = r#"{"id": 100, "trackName": "Wattershed (Live at Reading Festival, London, UK - August 1995)",
             "artistName": "Foo Fighters", "albumName": "Live at Reading", "duration": 231.0,
             "instrumental": false, "plainLyrics": "le parole del concerto",
             "syncedLyrics": "[00:12.00]le parole del concerto"}"#;
        // Lo studio esiste, ha la stessa durata al secondo, e non deve
        // vincere: l'ordine è l'informazione.
        let studio = r#"[
          {"id": 200, "trackName": "Wattershed", "artistName": "Foo Fighters",
           "albumName": "The Colour and the Shape", "duration": 231.0, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": "[00:12.00]le parole"}
        ]"#;
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(esatta_dal_vivo)),
            ("lrclib-ricerca", Finta::Corpo(studio)),
            ("lrclib-esatta-sfrondata", Finta::Corpo(studio)),
            ("lrclib-ricerca-sfrondata", Finta::Corpo(studio)),
        ]);

        let voce = cerca(&fornitori, &concerto())
            .expect("nessun guasto")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 100, "vince l'esecuzione esatta");
        let chieste = finto.chieste();
        assert!(
            !chieste.iter().any(|s| s.ends_with("-sfrondata")),
            "il secondo giro non si fa nemmeno: {chieste:?}"
        );
    }

    #[test]
    fn il_ripiego_rispetta_il_veto_sulla_durata() {
        // Sotto «Wattershed» il catalogo ha la sola versione di studio: due
        // minuti e cinquantaquattro contro i tre e cinquantuno del concerto,
        // cioè cinquantasette secondi di scarto. Sfrondare allarga il campo, e
        // il veto di durata è l'unica difesa che resta contro l'agganciare il
        // testo di un'altra incisione: qui deve tenere.
        let solo_studio = r#"[
          {"id": 200, "trackName": "Wattershed", "artistName": "Foo Fighters",
           "albumName": "The Colour and the Shape", "duration": 174.0, "instrumental": false,
           "plainLyrics": "le parole", "syncedLyrics": "[00:12.00]le parole"}
        ]"#;
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Niente),
            ("lrclib-ricerca", Finta::Niente),
            ("lrclib-esatta-sfrondata", Finta::Niente),
            ("lrclib-ricerca-sfrondata", Finta::Corpo(solo_studio)),
        ]);

        assert_eq!(
            cerca(&fornitori, &concerto()).expect("nessun guasto"),
            None,
            "meglio nessun testo che il testo di un'altra incisione"
        );
        // Il secondo giro **si è fatto**: la rinuncia viene dal veto, non dal
        // non aver provato. Senza questa riga la prova passerebbe anche se il
        // ripiego non esistesse.
        assert!(
            finto
                .chieste()
                .iter()
                .any(|s| s == "lrclib-ricerca-sfrondata")
        );
    }

    #[test]
    fn un_titolo_senza_decorazioni_non_fa_un_secondo_giro() {
        // Niente da sfrondare: il secondo giro sarebbe la stessa identica
        // richiesta sotto un'altra chiave, cioè traffico per nulla contro un
        // servizio che ci ospita gratis.
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Niente),
            ("lrclib-ricerca", Finta::Niente),
        ]);
        assert_eq!(cerca(&fornitori, &brano()).expect("nessun guasto"), None);
        assert!(!finto.chieste().iter().any(|s| s.ends_with("-sfrondata")));
    }

    #[test]
    fn saltare_la_memoria_non_rilegge_ma_riscrive() {
        // Il deposito finto risponde `Corpo` a chiunque legga: se la lettura
        // avvenisse, la domanda esatta troverebbe una voce con i tempi e la
        // ricerca non si farebbe. Saltandola, la richiesta vera parte — e qui
        // non c'è rete, quindi cade sull'interruttore aperto.
        let (fornitori, finto) = finti(vec![
            ("lrclib-esatta", Finta::Corpo(ESATTA_CON_TEMPI)),
            ("lrclib-ricerca", Finta::Corpo(RICERCA_CON_TEMPI)),
        ]);
        for _ in 0..crate::cadenza::GUASTI_PER_APRIRE {
            fornitori.lrclib.guasto();
        }
        assert!(
            cerca_di_nuovo(&fornitori, &brano()).is_err(),
            "il ritentativo non si accontenta di quel che si ricordava"
        );
        assert!(
            finto.chieste().is_empty(),
            "il deposito non è stato nemmeno interrogato"
        );

        // E la stessa domanda per la via normale la risposta ricordata la
        // trova, interruttore aperto o no.
        let voce = cerca(&fornitori, &brano())
            .expect("la memoria basta")
            .expect("una voce");
        assert_eq!(voce.candidato.id, 1);
        assert!(!finto.chieste().is_empty());
    }
}
