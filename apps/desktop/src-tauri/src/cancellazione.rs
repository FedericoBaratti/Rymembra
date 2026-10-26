//! Togliere un brano dalla libreria, e — se lo si chiede — anche dal disco.
//!
//! Due comandi e non uno, perché sono due domande diverse.
//!
//! [`brani_togli`] dice «questo non lo voglio in elenco»: la riga se ne va, il
//! file resta dov'è. È la risposta giusta per quel che è entrato in libreria per
//! sbaglio — una cartella sorvegliata di troppo, un disco esterno scansionato
//! una volta — e ha un limite che non si nasconde: se il file sta ancora sotto
//! una cartella sorvegliata, la prossima scansione lo rimette. La libreria è un
//! indice del disco, non il disco, e nessun indice può dire a un file di non
//! esistere. Per questo [`rymembra_app::library::Cancellazione`] porta indietro
//! `torneranno`: è la sola cosa onesta da mostrare a chi ha appena premuto.
//!
//! [`brani_elimina`] dice «questo non lo voglio più»: il file va nel Cestino, e
//! la riga con lui. Nel Cestino e non cancellato, perché una voce di menù che
//! distrugge senza ritorno è una voce di menù sbagliata; e per la stessa ragione
//! la riga si toglie **dopo**, e solo per i file che ci sono davvero arrivati —
//! una libreria che dichiara vuoto quel che sul disco c'è ancora è peggio di una
//! che dichiara troppo.
//!
//! # Dove un Cestino non c'è
//!
//! Su un'unità di rete — `\\nas\musica`, o la lettera `Z:` mappata sopra — il
//! Cestino di Windows non esiste. `IFileOperation` con «annullabile» non se ne
//! lamenta: **cancella per sempre** e riporta successo, e se proprio decide di
//! avvisare apre un dialogo della shell da un filo senza finestra. Per chi
//! teneva la musica su un NAS, «Elimina dal disco» voleva dire quindi una
//! cancellazione senza ritorno raccontata come un viaggio nel Cestino.
//!
//! Adesso si chiede prima dove sta ogni file ([`rymembra_app::volumi`], che
//! riconosce anche le lettere mappate), la finestra lo dice nella conferma —
//! «questi N brani saranno cancellati per sempre» — e solo con quella
//! conferma in mano i file di rete si tolgono con `remove_file`, senza passare
//! dalla shell. Senza conferma il nucleo rifiuta con `fs.noRecycleBin`, prima
//! di aver toccato la coda o un file. Un file di cui non si è saputo dove sta
//! vale come uno di rete: la promessa del Cestino non si fa a vuoto.
//!
//! # L'ordine, in [`brani_elimina`]
//!
//! Prima si sgombera la coda **fermando il motore**, poi si cestina, poi si
//! scrive nel database. Non è prudenza generica: su Windows un file aperto non
//! si può mandare nel Cestino, e il motore audio il suo file ce l'ha aperto per
//! tutta la durata del brano — compreso quello «in canna», che il gapless apre
//! in anticipo. Cestinare prima di aver lasciato andare quei due handle vuol
//! dire fallire proprio sul brano che si sta ascoltando, cioè su quello che si
//! sta guardando mentre si preme.
//!
//! # Le immagini CD
//!
//! Un brano d'immagine — una traccia di un FLAC o di un APE intero, tagliato
//! da un `.cue` — non ha un file suo, e dal disco si toglie solo insieme a
//! tutti gli altri brani del suo disco: allora vanno nel Cestino l'immagine e
//! il `.cue`. Una parte sola si rifiuta, e vale come una parte anche la
//! vecchia riga del disco intero, che ha per file l'immagine. Il perché sta su
//! `da_togliere`.
//!
//! # Perché un file suo
//!
//! Per la stessa ragione di [`crate::playlist`]: `comandi.rs` è già il più
//! lungo, e questi due comandi hanno un solo argomento in comune.

use std::path::Path;

use rymembra_app::library::{self, Cancellazione};
use rymembra_app::settings::{self, CHIAVE_CARTELLE};
use rymembra_app::volumi::{self, Collocazione};
use rymembra_domain::errors::{AppError, ErrorCode};
use rymembra_domain::paths::{PathRules, path_key};
use tauri::Manager as _;

use crate::disparte::in_disparte;
use crate::errore::{Esito, errore};
use crate::nota;
use crate::riproduzione::{StatoLettore, sgombra_dalla_coda};
use crate::stato::{Stato, con_libreria};

/// La libreria, o il motivo per cui questo filo non ce l'ha più.
///
/// `try_state` e non `state`: `state` panica se lo stato non c'è, e questi corpi
/// girano sul pool bloccante, dove possono ritrovarsi vivi mentre la finestra si
/// chiude e gli stati gestiti vengono lasciati cadere. È la stessa guardia di
/// `comandi::scansiona_ora`.
fn stato_di<'a>(
    app: &'a tauri::AppHandle,
    cosa: &'static str,
) -> Result<tauri::State<'a, Stato>, AppError> {
    app.try_state::<Stato>().ok_or_else(|| {
        AppError::new(ErrorCode::InternalAborted {
            what: Some(cosa.to_owned()),
        })
        .with_cause("la libreria non è più fra gli stati gestiti")
    })
}

/// Le cartelle sorvegliate, per sapere chi tornerà alla prossima scansione.
fn sorvegliate(stato: &Stato) -> Result<Vec<String>, AppError> {
    con_libreria(stato, |libreria| {
        settings::read_json(&libreria.connection, CHIAVE_CARTELLE).map(Option::unwrap_or_default)
    })
}

/// Toglie dalla coda dei brani che stanno per sparire, senza farne un guasto.
///
/// La coda è una comodità, non la verità: se il lettore non c'è — nessun
/// dispositivo audio, la finestra che si sta chiudendo — la cancellazione deve
/// riuscire lo stesso. Quel che non si è potuto fare finisce nel diario, che è
/// il posto dove si va a cercarlo, invece di diventare un errore rosso sopra un
/// gesto che è andato a buon fine.
pub(crate) fn sgombera(app: &tauri::AppHandle, brani: &[i64], ferma: bool) {
    let Some(lettore) = app.try_state::<StatoLettore>() else {
        return;
    };
    if let Err(err) = sgombra_dalla_coda(app, &lettore, brani, ferma) {
        nota!(
            "[cancellazione] la coda non si è potuta sgomberare: {} causa={}",
            err.code().kind().code(),
            err.cause().unwrap_or("—")
        );
    }
}

/// Toglie i brani dalla libreria. **Non tocca i file.**
///
/// # Dove gira
///
/// In disparte: la transazione rifà gli aggregati, che su una libreria grande
/// sono una lettura intera di `tracks`, e il filo della finestra ha quattro
/// secondi di pazienza.
///
/// # Errori
///
/// `internal.aborted` se lo stato non c'è più, `db.*` se la scrittura fallisce.
#[tauri::command]
pub async fn brani_togli(app: tauri::AppHandle, brani: Vec<i64>) -> Esito<Cancellazione> {
    // Una voce di sessione non è una riga: si rifiuta il gesto intero, invece
    // di farne metà. Vedi `riproduzione::ospiti::righe_di_libreria`.
    crate::riproduzione::righe_di_libreria(&brani).map_err(errore)?;
    let mano = app.clone();
    let esito = in_disparte("rimozione di brani dalla libreria", move || {
        let stato = stato_di(&mano, "rimozione di brani")?;
        let radici = sorvegliate(&stato)?;
        let fatto = con_libreria(&stato, |libreria| {
            library::cancella_brani(
                &mut libreria.connection,
                &brani,
                &radici,
                PathRules::for_current_platform(),
            )
        })?;
        // Dopo il database e non prima: se la scrittura fallisce, la coda non
        // deve aver perso brani che in libreria ci sono ancora. Senza `ferma`,
        // perché i file restano leggibili — togliere dalla coda vuol dire «non
        // risuonarlo», non «zitto adesso».
        sgombera(&mano, &brani, false);
        Ok(fatto)
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Quel che «Elimina dal disco» toglie per una parte dei brani scelti.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DaTogliere {
    /// I file veri da togliere, nell'ordine in cui vanno tolti: il file del
    /// brano; o l'immagine CD e poi il suo `.cue`. Vuoto per un brano di
    /// catalogo, che sul disco non c'è.
    file: Vec<String>,
    /// Le righe che se ne vanno appena il primo file se n'è andato.
    brani: Vec<i64>,
}

/// Che cosa togliere dal disco per questi brani, e quali righe con loro.
///
/// # I brani d'immagine CD
///
/// Un brano d'immagine non ha un file suo: il suo percorso in tabella,
/// `…\Album.cue|03`, non si apre, e il file vero — l'immagine, nella colonna
/// `immagine` — è di tutti i brani del disco insieme. Toglierlo per un brano
/// solo vorrebbe dire toglierlo agli altri, che nessuno ha scelto. Quindi:
///
/// - con `tutti_i_brani`, se dei brani di un'immagine ne è scelta solo una
///   parte, si rifiuta tutto con `library.imageTrackPartialDelete`, prima di
///   aver toccato qualunque cosa;
/// - se sono scelti tutti, si tolgono l'immagine e il suo `.cue` — l'immagine
///   per prima: se il `.cue` poi non se ne va, alla scansione dopo resta un
///   foglio senza immagine, che si ignora; nell'ordine opposto resterebbe
///   un'immagine senza foglio, che tornerebbe come un brano lungo un disco —
///   e le righe se ne vanno con l'immagine.
///
/// Senza `tutti_i_brani` non si rifiuta niente: l'immagine e il `.cue` stanno
/// in una voce sola anche quando i brani scelti di quel disco sono dodici.
/// [`brani_su_rete`] chiede invece con `tutti_i_brani`, così che il rifiuto
/// arrivi **prima** della conferma e non dopo: una finestrella che chiede
/// «sicuro?» per un gesto che poi verrà rifiutato fa rispondere a una domanda
/// che non c'era.
///
/// L'immagine si riconosce dalla colonna così com'è scritta, la stessa stringa
/// per tutti i brani di quel disco: è anche quel che la riproduzione confronta
/// per sapere se due brani sono contigui.
///
/// # La vecchia riga del disco intero
///
/// Prima che la scansione sapesse leggere i `.cue`, l'immagine era un brano
/// solo, lungo un disco, con il percorso del file grande e senza `immagine`. La
/// scansione che la taglia toglie quella riga, ma per un po' può restare
/// accanto ai brani nuovi: la guardia contro le stragi ne ha rinviato la
/// rimozione, o chi guardava non l'ha confermata. In quel tempo il suo file è
/// l'immagine degli altri, e toglierlo per lei sola vorrebbe dire cestinare i
/// dodici brani che nessuno ha scelto. Quindi quella riga vale come un brano
/// del disco: da sola si rifiuta come una parte; con tutti i brani d'immagine
/// sta nella loro voce. E con loro se ne va comunque, scelta o no, perché il
/// suo file se ne va: una riga per un file che non c'è più è un brano che non
/// suona.
///
/// Il suo percorso si confronta con la colonna `immagine` con la chiave della
/// scansione ([`path_key`]), cioè come fa il nucleo in
/// `rymembra_app::import_playlist::brani_dell_immagine`. Non con quella
/// funzione: legge tutti i brani d'immagine a ogni chiamata, e qui servirebbe
/// una volta per ogni file scelto, sotto il lucchetto, due volte per gesto.
/// Le immagini distinte si leggono una volta sola, e solo se fra i brani
/// scelti c'è un file intero.
///
/// # Errori
///
/// `library.imageTrackPartialDelete` come detto; `db.*` se la lettura fallisce.
fn da_togliere(
    connection: &rusqlite::Connection,
    brani: &[i64],
    tutti_i_brani: bool,
) -> Result<Vec<DaTogliere>, AppError> {
    use rusqlite::OptionalExtension as _;
    let fallita = |err: rusqlite::Error| {
        rymembra_app::db::codice_da_sqlite("brani da eliminare dal disco", &err)
    };
    let regole = PathRules::for_current_platform();
    let righe = library::summaries_by_id(connection, brani)?;
    let scelti: std::collections::HashSet<i64> = righe.iter().map(|brano| brano.id).collect();
    let mut immagine_di = connection
        .prepare_cached("SELECT immagine FROM tracks WHERE id = ?1")
        .map_err(fallita)?;
    let mut del_disco = connection
        .prepare_cached("SELECT id, path FROM tracks WHERE immagine = ?1 ORDER BY path")
        .map_err(fallita)?;
    // Le immagini della libreria per chiave di percorso, lette alla prima riga
    // senza `immagine`: vedi «La vecchia riga del disco intero».
    let mut immagini: Option<std::collections::HashMap<String, String>> = None;
    let mut piano = Vec::with_capacity(righe.len());
    let mut immagini_viste = std::collections::HashSet::new();
    for brano in righe {
        // Un brano di catalogo non ha un file: la sua riga se ne va e basta.
        let Some(percorso) = brano.path else {
            piano.push(DaTogliere {
                file: Vec::new(),
                brani: vec![brano.id],
            });
            continue;
        };
        let immagine: Option<String> = immagine_di
            .query_row([brano.id], |riga| riga.get(0))
            .optional()
            .map_err(fallita)?
            .flatten();
        let immagine = match immagine {
            Some(immagine) => immagine,
            None => {
                if immagini.is_none() {
                    immagini = Some(immagini_per_chiave(connection, regole)?);
                }
                // Un file intero che è l'immagine di altri brani è la vecchia
                // riga del disco intero: si tratta come un brano di quel disco.
                let sua = immagini
                    .as_ref()
                    .and_then(|indice| indice.get(&path_key(&percorso, regole)))
                    .cloned();
                match sua {
                    Some(immagine) => immagine,
                    None => {
                        piano.push(DaTogliere {
                            file: vec![percorso],
                            brani: vec![brano.id],
                        });
                        continue;
                    }
                }
            }
        };
        // Un'immagine si guarda una volta, al primo dei suoi brani.
        if !immagini_viste.insert(immagine.clone()) {
            continue;
        }
        let disco: Vec<(i64, Option<String>)> = del_disco
            .query_map([&immagine], |riga| Ok((riga.get(0)?, riga.get(1)?)))
            .map_err(fallita)?
            .collect::<Result<_, _>>()
            .map_err(fallita)?;
        let mancano = disco.iter().filter(|(id, _)| !scelti.contains(id)).count();
        if tutti_i_brani && mancano > 0 {
            return Err(AppError::new(ErrorCode::LibraryImageTrackPartialDelete {
                path: immagine.clone(),
            })
            .with_cause(format!(
                "scelti {} brani su {} dell'immagine CD: o tutti, o nessuno",
                disco.len().saturating_sub(mancano),
                disco.len()
            )));
        }
        // La vecchia riga del disco intero, se c'è, si cerca qui e non si
        // aspetta di incontrarla fra le scelte: arrivata dopo i brani del suo
        // disco, verrebbe saltata come un'immagine già vista. Il suo percorso
        // è quello dell'immagine; quello di un brano d'immagine finisce in
        // `|NN`, e qui non risponde.
        let interi = library::ids_by_path(connection, std::slice::from_ref(&immagine), regole)?;
        let mut file = vec![immagine];
        for (_, percorso) in &disco {
            let foglio = percorso
                .as_deref()
                .and_then(rymembra_domain::cue::brano_d_immagine)
                .map(|(cue, _)| cue);
            if let Some(cue) = foglio
                && !file.iter().any(|gia| gia == cue)
            {
                file.push(cue.to_owned());
            }
        }
        let mut ids: Vec<i64> = disco.into_iter().map(|(id, _)| id).collect();
        for intera in interi.into_iter().flatten() {
            if !ids.contains(&intera) {
                ids.push(intera);
            }
        }
        piano.push(DaTogliere { file, brani: ids });
    }
    Ok(piano)
}

/// Ogni immagine CD della libreria, per chiave di percorso, con la stringa
/// scritta nella colonna `immagine`.
///
/// Le distinte e non i brani: qualche centinaio di righe anche su una libreria
/// con migliaia di brani d'immagine, e le dà l'indice parziale su `immagine`.
/// Se due grafie della stessa immagine stessero insieme in tabella — il tempo
/// di una passata — vince la prima: l'altra ha comunque i suoi brani.
///
/// # Errori
///
/// `db.*` se la lettura fallisce.
fn immagini_per_chiave(
    connection: &rusqlite::Connection,
    regole: PathRules,
) -> Result<std::collections::HashMap<String, String>, AppError> {
    let fallita = |err: rusqlite::Error| {
        rymembra_app::db::codice_da_sqlite("immagini CD della libreria", &err)
    };
    let mut lettura = connection
        .prepare_cached(
            "SELECT DISTINCT immagine FROM tracks WHERE immagine IS NOT NULL ORDER BY immagine",
        )
        .map_err(fallita)?;
    let righe = lettura
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(fallita)?;
    let mut indice = std::collections::HashMap::new();
    for riga in righe {
        let immagine = riga.map_err(fallita)?;
        indice
            .entry(path_key(&immagine, regole))
            .or_insert(immagine);
    }
    Ok(indice)
}

/// Quanti di questi brani se ne vanno per sempre, perché il loro file sta dove
/// un Cestino non c'è.
///
/// È la domanda che la conferma di «Elimina dal disco» fa **prima** di
/// mostrarsi: zero vuol dire la frase di sempre, più di zero vuol dire
/// «questi N brani saranno cancellati per sempre». I file di cui non si è
/// saputo dove stanno si contano insieme a quelli di rete, per la stessa
/// ragione per cui [`brani_elimina`] li tratta così: vedi il `//!`.
///
/// Brani e non file, perché la finestrella mette questo numero accanto a
/// quanti brani si sono scelti, e da lì decide se dire «gli altri vanno nel
/// Cestino». I dodici brani di un'immagine CD sul NAS sono due file — l'immagine
/// e il `.cue` — e contarli come due diceva «2 brani su rete, gli altri nel
/// Cestino» mentre sparivano per sempre tutti e dodici. Vedi
/// [`brani_senza_cestino`].
///
/// # Dove gira
///
/// In disparte e fuori dal lucchetto: chiedere dove sta un file vuol dire
/// risolvere la sua cartella, e su una share che non risponde è un'attesa.
///
/// # Errori
///
/// `library.imageTrackPartialDelete` se dei brani di un'immagine CD ne è scelta
/// solo una parte: la conferma lo mostra al posto del tasto, invece di chiedere
/// un sì che [`brani_elimina`] rifiuterebbe. `internal.aborted` se lo stato non
/// c'è più, `db.*` se la lettura fallisce.
#[tauri::command]
pub async fn brani_su_rete(app: tauri::AppHandle, brani: Vec<i64>) -> Esito<usize> {
    // Una voce di sessione non è una riga: si rifiuta il gesto intero, invece
    // di farne metà. Vedi `riproduzione::ospiti::righe_di_libreria`.
    crate::riproduzione::righe_di_libreria(&brani).map_err(errore)?;
    in_disparte("collocazione dei brani da eliminare", move || {
        let stato = stato_di(&app, "collocazione dei brani")?;
        let piano = con_libreria(&stato, |libreria| {
            da_togliere(&libreria.connection, &brani, true)
        })?;
        let percorsi: Vec<String> = piano
            .iter()
            .flat_map(|voce| voce.file.iter().cloned())
            .collect();
        let collocazioni = volumi::collocazioni(&percorsi, volumi::SCADENZA_RISOLUZIONE);
        // Come in `brani_elimina`: chi non ha una risposta vale come di rete.
        let dove = |percorso: &str| {
            percorsi
                .iter()
                .position(|p| p == percorso)
                .and_then(|i| collocazioni.get(i).copied())
                .unwrap_or(Collocazione::Ignota)
        };
        Ok(brani_senza_cestino(&piano, &brani, |percorso| {
            dove(percorso).ha_un_cestino()
        }))
    })
    .await
    .map_err(errore)?
    .map_err(errore)
}

/// Quanti dei brani `scelti` se ne vanno insieme a un file che non può andare
/// nel Cestino.
///
/// Si contano le righe delle voci di [`da_togliere`] che hanno almeno un file
/// dove il Cestino non c'è: [`brani_elimina`] le toglie tutte insieme col primo
/// file della voce, quindi per chi guarda sono perse per sempre tutte. Una
/// voce di catalogo non ha file e non conta.
///
/// Solo le scelte, perché la finestrella confronta questo numero con quanti
/// brani si sono scelti: la vecchia riga del disco intero, che se ne va con
/// l'immagine anche quando nessuno l'ha scelta, lo farebbe arrivare a «tutti»
/// in una scelta di dodici brani d'immagine sul NAS e un file locale — e la
/// frase direbbe che nemmeno quello va nel Cestino.
///
/// Pura apposta, con `ha_un_cestino` dato da fuori: la domanda vera chiede a
/// Windows dove sta una cartella, e una prova non ha un NAS.
fn brani_senza_cestino(
    piano: &[DaTogliere],
    scelti: &[i64],
    ha_un_cestino: impl Fn(&str) -> bool,
) -> usize {
    let scelti: std::collections::HashSet<i64> = scelti.iter().copied().collect();
    piano
        .iter()
        .filter(|voce| voce.file.iter().any(|percorso| !ha_un_cestino(percorso)))
        .map(|voce| voce.brani.iter().filter(|id| scelti.contains(id)).count())
        .sum()
}

/// Cancella un file **per sempre**, dove un Cestino non c'è.
///
/// Un file che non c'è già più non è un guasto: il gesto voleva che non ci
/// fosse, e la sua riga se ne va con le altre. Ma «non c'è» si crede solo se
/// non è la rete a dirlo: Windows racconta la share sparita con lo stesso
/// genere di errore del file sparito, e lì la riga deve restare.
fn per_sempre(percorso: &str) -> Result<(), AppError> {
    match std::fs::remove_file(rymembra_app::files::percorso_lungo(Path::new(percorso))) {
        Ok(()) => Ok(()),
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && !rymembra_domain::errors::rete::e_di_rete(&err) =>
        {
            Ok(())
        }
        Err(err) => Err(rymembra_app::files::io_error(percorso, &err)),
    }
}

/// Manda i file nel Cestino — o, dove un Cestino non c'è e lo si è confermato,
/// li cancella per sempre — e toglie dalla libreria quelli che se ne sono
/// andati.
///
/// `definitivi` è la conferma della finestra: «sì, anche quelli che non possono
/// andare nel Cestino». Assente vale no, ed è la forma che hanno le chiamate
/// scritte prima che esistesse.
///
/// # Dove gira
///
/// In disparte, e il Cestino si chiama **fuori** dal lucchetto della libreria:
/// `IFileOperation` parla con la shell di Windows, che su una cartella di rete
/// può metterci quanto le pare. Fuori dal lucchetto anche la domanda su dove
/// stanno i file, per la stessa ragione.
///
/// # Errori
///
/// `library.imageTrackPartialDelete` se fra i brani ce n'è qualcuno di
/// un'immagine CD di cui non sono scelti tutti: vedi [`da_togliere`]. Anche
/// lì non si è toccato niente.
/// `fs.noRecycleBin` se qualche file sta dove un Cestino non c'è e `definitivi`
/// non è stato detto: in quel caso non si è toccato niente, né la coda né un
/// file. `fs.trashFailed` — o uno dei `fs.*` più precisi, quando Windows dice
/// un numero che si sa leggere — se il Cestino rifiuta un file; `fs.*` se la
/// cancellazione definitiva fallisce. Lì ci si ferma, dopo aver tolto dalla
/// libreria quelli già andati: una riga che resta per un file che non c'è più è
/// un brano che non suona, una riga tolta per un file che c'è ancora è un brano
/// che nessuno ritrova. `internal.aborted` se lo stato non c'è più, `db.*` se
/// la scrittura fallisce.
#[tauri::command]
pub async fn brani_elimina(
    app: tauri::AppHandle,
    brani: Vec<i64>,
    definitivi: Option<bool>,
) -> Esito<usize> {
    // Una voce di sessione non è una riga: si rifiuta il gesto intero, invece
    // di farne metà. Vedi `riproduzione::ospiti::righe_di_libreria`.
    crate::riproduzione::righe_di_libreria(&brani).map_err(errore)?;
    let mano = app.clone();
    let definitivi = definitivi.unwrap_or(false);
    let esito = in_disparte("eliminazione di brani dal disco", move || {
        let stato = stato_di(&mano, "eliminazione di brani")?;
        // Il piano per primo, e con lui il rifiuto di un'immagine CD scelta a
        // metà: prima di chiedere dove stanno i file, prima di fermare il
        // motore, prima di qualunque cosa che si veda.
        let condannati = con_libreria(&stato, |libreria| {
            da_togliere(&libreria.connection, &brani, true)
        })?;
        if condannati.is_empty() {
            return Ok(0);
        }

        // Dove sta ogni file, chiesto **prima** di fermare il motore: se la
        // risposta è un rifiuto, chi ascoltava deve continuare ad ascoltare.
        let percorsi: Vec<String> = condannati
            .iter()
            .flat_map(|voce| voce.file.iter().cloned())
            .collect();
        let collocazioni = volumi::collocazioni(&percorsi, volumi::SCADENZA_RISOLUZIONE);
        let dove = |percorso: &str| {
            percorsi
                .iter()
                .position(|p| p == percorso)
                .and_then(|i| collocazioni.get(i).copied())
                .unwrap_or(Collocazione::Ignota)
        };
        if !definitivi && let Some(primo) = percorsi.iter().find(|p| !dove(p).ha_un_cestino()) {
            return Err(AppError::new(ErrorCode::FsNoRecycleBin {
                path: primo.clone(),
            })
            .with_cause(
                "il file sta su un'unità di rete, o non si è saputo dove sta, e la \
                 cancellazione definitiva non è stata confermata",
            ));
        }

        // Il motore lascia andare i file che ha in mano — il corrente e quello
        // in canna — prima che il Cestino provi a prenderli. Per un brano
        // d'immagine il file in mano è l'immagine, e i brani da sgomberare
        // sono tutti quelli del disco.
        let ids: Vec<i64> = condannati
            .iter()
            .flat_map(|voce| voce.brani.iter().copied())
            .collect();
        sgombera(&mano, &ids, true);

        let mut andati: Vec<i64> = Vec::with_capacity(ids.len());
        let mut guasto = None;
        'voci: for voce in &condannati {
            // Un brano di catalogo non ha un file da mandare nel Cestino, e la
            // sua riga se ne va lo stesso: «elimina dal disco» su qualcosa che
            // sul disco non c'è vuol dire «toglilo dalla libreria», che è quel
            // che succede qui sotto insieme a tutti gli altri. Fallire sarebbe
            // rispondere a una domanda che nessuno ha fatto.
            if voce.file.is_empty() {
                andati.extend(&voce.brani);
                continue;
            }
            for (quale, percorso) in voce.file.iter().enumerate() {
                // Dove il Cestino c'è, il Cestino; dove non c'è — ed è stato
                // confermato, o non si sarebbe arrivati qui — `remove_file`,
                // senza passare dalla shell e dal suo dialogo senza finestra.
                let fatto = if dove(percorso).ha_un_cestino() {
                    trash::delete(percorso).map_err(|err| nel_cestino(percorso, &err))
                } else {
                    per_sempre(percorso)
                };
                match fatto {
                    // Le righe se ne vanno col primo file: il brano, o
                    // l'immagine. Un `.cue` rimasto dopo non suona niente.
                    Ok(()) if quale == 0 => andati.extend(&voce.brani),
                    Ok(()) => {}
                    Err(err) => {
                        guasto = Some(err);
                        break 'voci;
                    }
                }
            }
        }
        // Anche quando ci si è fermati a metà: le righe di quel che è nel
        // Cestino devono sparire comunque, o la libreria racconta file che non
        // ci sono più.
        let radici = sorvegliate(&stato)?;
        let fatto = con_libreria(&stato, |libreria| {
            library::cancella_brani(
                &mut libreria.connection,
                &andati,
                &radici,
                PathRules::for_current_platform(),
            )
        })?;
        match guasto {
            Some(err) => Err(err),
            None => Ok(fatto.tolti),
        }
    })
    .await
    .map_err(errore)?
    .map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

/// Il codice di sistema dentro un `trash::Error::Os`, sbucciato.
///
/// Su Windows quel numero non è un codice Win32: è l'`HRESULT` che COM ha dato
/// al crate, e `HRESULT_FROM_WIN32` il codice vero se lo tiene nei sedici bit
/// bassi con `FACILITY_WIN32` sopra. «File non trovato» arriva come
/// `0x80070002`, non come `2`.
///
/// Serve dirlo perché il confronto sbagliato non si vede: nessun ramo scatta
/// mai, cade tutto sul generico, e il generico una frase ce l'ha — sbagliata ma
/// plausibile. Qui è costato una prova dal vivo per accorgersene.
///
/// Sulle altre piattaforme il crate ci mette `errno`, che involucro non ne ha:
/// se i bit alti non sono quelli, il numero torna com'era.
fn codice_di_sistema(code: i32) -> i32 {
    /// `FACILITY_WIN32`, dove `HRESULT_FROM_WIN32` la scrive.
    const INVOLUCRO: u32 = 0x8007_0000;
    /// I sedici bit in cui sta il codice vero.
    const BASSI: u32 = 0x0000_FFFF;

    // `to_ne_bytes`/`from_ne_bytes` e non un `as`: è la stessa sequenza di bit
    // riletta senza segno, e non un troncamento che il workspace segnala.
    let grezzo = u32::from_ne_bytes(code.to_ne_bytes());
    if grezzo & !BASSI != INVOLUCRO {
        return code;
    }
    i32::try_from(grezzo & BASSI).unwrap_or(code)
}

/// Traduce il guasto del Cestino in un codice del catalogo.
///
/// Pura apposta: è l'unica parte di questo modulo che si può provare senza un
/// disco, una shell e un file da buttare.
///
/// I numeri sono quelli di Windows, e sono quattro perché quattro sono i casi in
/// cui chi legge può fare qualcosa: il file aperto altrove, il permesso negato,
/// il file già sparito, il disco pieno. Tutto il resto — compresa
/// l'«operazione interrotta» che `IFileOperation` restituisce senza dire da
/// cosa — cade su `fs.trashFailed`, che esiste apposta per non dover indovinare.
///
/// Dal vivo, però, il generico è la regola e non l'eccezione: un file tenuto
/// aperto in esclusiva da un altro programma non fa arrivare `0x80070020`, fa
/// arrivare `Unknown { "Some operations were aborted" }` — la shell prova,
/// rinuncia e non dice perché. È il motivo per cui la frase di `fs.trashFailed`
/// nomina tutte e due le cause invece di sceglierne una: quella distinzione da
/// qui non si può fare. I cinque numeri restano lo stesso, perché costano cinque
/// righe e la shell non promette di passare sempre di lì.
fn nel_cestino(percorso: &str, err: &trash::Error) -> AppError {
    /// `ERROR_FILE_NOT_FOUND`
    const NON_TROVATO: i32 = 2;
    /// `ERROR_PATH_NOT_FOUND`
    const PERCORSO_NON_TROVATO: i32 = 3;
    /// `ERROR_ACCESS_DENIED`
    const NEGATO: i32 = 5;
    /// `ERROR_SHARING_VIOLATION`
    const IN_USO: i32 = 32;
    /// `ERROR_DISK_FULL`
    const DISCO_PIENO: i32 = 112;

    let generico = || ErrorCode::FsTrashFailed {
        path: percorso.to_owned(),
        detail: None,
    };
    let codice = match err {
        trash::Error::Os { code, .. } => match codice_di_sistema(*code) {
            IN_USO => ErrorCode::FsInUse {
                path: percorso.to_owned(),
            },
            NEGATO => ErrorCode::FsPermissionDenied {
                path: percorso.to_owned(),
            },
            NON_TROVATO | PERCORSO_NON_TROVATO => ErrorCode::FsNotFound {
                path: percorso.to_owned(),
            },
            DISCO_PIENO => ErrorCode::FsDiskFull {
                path: Some(percorso.to_owned()),
            },
            _ => generico(),
        },
        // «Non ci si arriva» vuol dire tutte e due le cose insieme, lo dice la
        // documentazione del crate: il file non c'è, oppure non si ha il
        // permesso di guardarlo. Fra le due, «non c'è» è quella che chi legge
        // può verificare da sé in due secondi.
        trash::Error::CouldNotAccess { .. } | trash::Error::CanonicalizePath { .. } => {
            ErrorCode::FsNotFound {
                path: percorso.to_owned(),
            }
        }
        _ => generico(),
    };
    AppError::new(codice).with_cause(err.to_string())
}

#[cfg(test)]
mod prove_per_sempre {
    use super::per_sempre;

    #[test]
    fn per_sempre_toglie_il_file_e_non_si_lamenta_di_quel_che_manca_gia() {
        let cartella = tempfile::tempdir().expect("cartella temporanea");
        let file = cartella.path().join("a.flac");
        std::fs::write(&file, b"x").expect("file");
        let percorso = file.to_string_lossy().into_owned();

        assert!(per_sempre(&percorso).is_ok());
        assert!(!file.exists(), "il file se n'è andato");
        // Già sparito: il gesto voleva che non ci fosse, e non c'è.
        assert!(per_sempre(&percorso).is_ok());
    }
}

#[cfg(test)]
mod prove {
    use super::nel_cestino;

    fn codice(err: &trash::Error) -> String {
        nel_cestino(r"C:\musica\a.mp3", err)
            .code()
            .kind()
            .code()
            .to_owned()
    }

    fn os(code: i32) -> trash::Error {
        trash::Error::Os {
            code,
            description: String::new(),
        }
    }

    /// Lo stesso numero, nell'involucro in cui Windows lo consegna davvero.
    fn hresult(win32: u32) -> trash::Error {
        os(i32::from_ne_bytes((0x8007_0000_u32 | win32).to_ne_bytes()))
    }

    #[test]
    fn i_numeri_di_windows_su_cui_si_puo_fare_qualcosa_hanno_un_codice_loro() {
        assert_eq!(codice(&hresult(32)), "fs.inUse");
        assert_eq!(codice(&hresult(5)), "fs.permissionDenied");
        assert_eq!(codice(&hresult(2)), "fs.notFound");
        assert_eq!(codice(&hresult(3)), "fs.notFound");
        assert_eq!(codice(&hresult(112)), "fs.diskFull");
    }

    #[test]
    fn il_numero_che_il_diario_ha_visto_davvero() {
        // Non un valore costruito qui: è quel che il crate ha restituito
        // cestinando la riga di un file spostato fuori da Rymembra, copiato dal
        // diario. La prima versione confrontava `2` con questo, non trovava mai
        // niente, e mandava a schermo il generico — che una frase ce l'ha, ed
        // è per questo che non si vedeva.
        assert_eq!(codice(&os(-2_147_024_894)), "fs.notFound");
    }

    #[test]
    fn un_errno_arriva_nudo_e_si_legge_lo_stesso() {
        // Le altre piattaforme non impacchettano niente: il crate ci mette
        // `errno`. Sono gli stessi cinque casi, e i numeri bassi non hanno
        // l'involucro da togliere.
        assert_eq!(codice(&os(2)), "fs.notFound");
        assert_eq!(codice(&os(112)), "fs.diskFull");
    }

    #[test]
    fn un_hresult_che_non_viene_da_win32_non_si_sbuccia() {
        // Facility `ITF` e non `WIN32`, con `0x20` nei bit bassi: sbucciarlo
        // senza guardare la facility direbbe «aperto da un altro programma» a
        // chi ha un guasto che con i file aperti non c'entra niente.
        assert_eq!(codice(&os(-2_147_221_472)), "fs.trashFailed");
    }

    #[test]
    fn un_guasto_che_non_si_sa_leggere_non_si_traveste_da_un_altro() {
        // Il caso vero, e il motivo per cui `fs.trashFailed` esiste:
        // `IFileOperation` dice «interrotta» e non dice da cosa. Indovinare «è
        // aperto da un altro programma» manderebbe metà delle persone a
        // chiudere un programma che non c'entra, e l'altra metà a non scoprire
        // mai che quel disco un Cestino non ce l'ha.
        assert_eq!(
            codice(&trash::Error::Unknown {
                description: "Some operations were aborted".to_owned(),
            }),
            "fs.trashFailed"
        );
        assert_eq!(codice(&os(1_234)), "fs.trashFailed");
    }

    #[test]
    fn non_ci_si_arriva_si_racconta_come_un_file_che_non_c_e() {
        assert_eq!(
            codice(&trash::Error::CouldNotAccess {
                target: r"C:\musica\a.mp3".to_owned(),
            }),
            "fs.notFound"
        );
    }

    #[test]
    fn la_causa_tecnica_non_si_perde() {
        // Il codice è per chi legge a schermo, la causa per chi legge il
        // diario: senza, un guasto generico sarebbe indistinguibile da tutti
        // gli altri guasti generici.
        let err = nel_cestino(r"C:\musica\a.mp3", &os(1_234));
        assert!(err.cause().is_some_and(|c| c.contains("1234")));
    }
}

#[cfg(test)]
mod prove_delle_immagini {
    use super::{DaTogliere, da_togliere};
    use crate::stato::Stato;

    /// Un file intero e un disco d'immagine da tre brani, col suo `.cue`.
    fn banco(
        stato: &Stato,
    ) -> std::sync::MutexGuard<'_, Result<crate::stato::Libreria, rymembra_domain::errors::AppError>>
    {
        let guardia = stato.libreria.lock().expect("lucchetto");
        guardia
            .as_ref()
            .expect("libreria aperta")
            .connection
            .execute_batch(
                r"INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (1, 'C:\M\a.flac', 'k1', 'T', 'A', 'Al', 'g', 1000, 1, 0, 0);
                 INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified,
                                     immagine, inizio_cd, fine_cd)
                 VALUES (2, 'C:\M\Disco\Album.cue|01', 'k2', 'Uno', 'A', 'D', 'd', 1000, 9,
                         0, 0, 'C:\M\Disco\Album.ape', 0, 7500),
                        (3, 'C:\M\Disco\Album.cue|02', 'k3', 'Due', 'A', 'D', 'd', 1000, 9,
                         0, 0, 'C:\M\Disco\Album.ape', 7500, 15000),
                        (4, 'C:\M\Disco\Album.cue|03', 'k4', 'Tre', 'A', 'D', 'd', 1000, 9,
                         0, 0, 'C:\M\Disco\Album.ape', 15000, NULL);",
            )
            .expect("banco");
        guardia
    }

    #[test]
    fn una_parte_dei_brani_di_un_immagine_non_si_elimina() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;

        // Due brani del disco su tre, insieme a un file intero: no, per tutti.
        let rifiuto = da_togliere(connection, &[1, 2, 3], true).expect_err("una parte sola");
        assert_eq!(
            rifiuto.code().kind().code(),
            "library.imageTrackPartialDelete"
        );
        assert!(
            rifiuto.cause().is_some_and(|c| c.contains("2 brani su 3")),
            "la causa dice quanti: {:?}",
            rifiuto.cause()
        );
    }

    #[test]
    fn tutti_i_brani_di_un_immagine_tolgono_l_immagine_e_il_suo_foglio() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;

        let mut piano = da_togliere(connection, &[4, 1, 2, 3], true).expect("tutti");
        piano.sort_by_key(|voce| voce.brani.first().copied());
        assert_eq!(
            piano,
            vec![
                DaTogliere {
                    file: vec![r"C:\M\a.flac".to_owned()],
                    brani: vec![1],
                },
                DaTogliere {
                    // Prima l'immagine, poi il foglio: una volta sola
                    // ciascuno, e mai il percorso virtuale.
                    file: vec![
                        r"C:\M\Disco\Album.ape".to_owned(),
                        r"C:\M\Disco\Album.cue".to_owned()
                    ],
                    brani: vec![2, 3, 4],
                },
            ]
        );
    }

    /// La vecchia riga del disco intero accanto ai brani nuovi: il file grande
    /// letto come un brano solo, prima che la scansione sapesse dei `.cue`.
    /// `percorso` è il suo, che può differire dall'immagine nelle maiuscole.
    fn con_la_riga_intera(connection: &rusqlite::Connection, percorso: &str) {
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, album_key,
                                     duration_ms, file_size, date_added, date_modified)
                 VALUES (5, ?1, 'k5', 'Disco', 'A', 'D', 'd', 3000, 9, 0, 0)",
                [percorso],
            )
            .expect("riga intera");
    }

    #[test]
    fn la_vecchia_riga_del_disco_intero_da_sola_non_cestina_l_immagine() {
        // Il difetto: la riga intera ha `immagine` NULL e per file l'immagine,
        // e prima diventava `file: [immagine]` — cioè l'immagine nel Cestino
        // per una riga sola, e i tre brani del disco persi senza averli scelti.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;
        con_la_riga_intera(connection, r"C:\M\Disco\Album.ape");

        for scelti in [&[5][..], &[1, 5], &[5, 2]] {
            let rifiuto = da_togliere(connection, scelti, true).expect_err("una parte sola");
            assert_eq!(
                rifiuto.code().kind().code(),
                "library.imageTrackPartialDelete",
                "scelti {scelti:?}"
            );
        }
    }

    #[test]
    fn la_vecchia_riga_del_disco_intero_se_ne_va_con_il_suo_disco() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;
        con_la_riga_intera(connection, r"C:\M\Disco\Album.ape");

        let disco = DaTogliere {
            file: vec![
                r"C:\M\Disco\Album.ape".to_owned(),
                r"C:\M\Disco\Album.cue".to_owned(),
            ],
            brani: vec![2, 3, 4, 5],
        };
        // Una voce sola, in qualunque ordine arrivino: la riga intera prima
        // dei brani d'immagine o dopo, quando l'immagine è già stata vista.
        for scelti in [&[5, 2, 3, 4][..], &[2, 3, 4, 5], &[3, 5, 4, 2]] {
            assert_eq!(
                da_togliere(connection, scelti, true).expect("tutti"),
                vec![disco.clone()],
                "scelti {scelti:?}"
            );
        }
        // Scelti i soli brani d'immagine, la riga intera se ne va lo stesso:
        // il suo file è l'immagine, e dopo non suonerebbe più.
        assert_eq!(
            da_togliere(connection, &[2, 3, 4], true).expect("tutti"),
            vec![disco]
        );
    }

    #[cfg(windows)]
    #[test]
    fn la_riga_intera_si_riconosce_anche_con_altre_maiuscole() {
        // La chiave della scansione, come nel nucleo: su Windows le maiuscole
        // non distinguono due file.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;
        con_la_riga_intera(connection, r"C:\M\DISCO\album.APE");

        let rifiuto = da_togliere(connection, &[5], true).expect_err("una parte sola");
        assert_eq!(
            rifiuto.code().kind().code(),
            "library.imageTrackPartialDelete"
        );
        let piano = da_togliere(connection, &[2, 3, 4], true).expect("tutti");
        assert_eq!(piano.len(), 1);
        assert_eq!(
            piano.first().map(|voce| voce.brani.clone()),
            Some(vec![2, 3, 4, 5])
        );
    }

    #[test]
    fn la_conferma_conta_i_brani_che_spariscono_non_i_file() {
        // Il difetto: un disco d'immagine sul NAS sono due file, e la
        // finestrella diceva «2 brani su rete, gli altri nel Cestino» mentre
        // sparivano per sempre tutti e tre. Qui il disco sta «in rete» e il
        // file intero no.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;
        let ha_un_cestino = |percorso: &str| !percorso.starts_with(r"C:\M\Disco\");

        let scelti = [1, 2, 3, 4];
        let piano = da_togliere(connection, &scelti, true).expect("tutti");
        assert_eq!(
            super::brani_senza_cestino(&piano, &scelti, ha_un_cestino),
            3
        );
        assert_eq!(super::brani_senza_cestino(&piano, &scelti, |_| false), 4);
        assert_eq!(super::brani_senza_cestino(&piano, &scelti, |_| true), 0);

        // Un brano di catalogo non ha file, e non sparisce da nessun disco.
        let catalogo = [DaTogliere {
            file: Vec::new(),
            brani: vec![9],
        }];
        assert_eq!(super::brani_senza_cestino(&catalogo, &[9], |_| false), 0);
    }

    #[test]
    fn la_riga_intera_non_scelta_non_entra_nel_conto() {
        // Se ne va con l'immagine, ma nessuno l'ha scelta: contarla farebbe
        // superare alla finestrella il numero dei brani scelti, e con un file
        // locale nella scelta direbbe che nemmeno quello va nel Cestino.
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        let guardia = banco(&stato);
        let connection = &guardia.as_ref().expect("libreria").connection;
        con_la_riga_intera(connection, r"C:\M\Disco\Album.ape");
        let ha_un_cestino = |percorso: &str| !percorso.starts_with(r"C:\M\Disco\");

        let scelti = [1, 2, 3, 4];
        let piano = da_togliere(connection, &scelti, true).expect("tutti");
        assert!(
            piano.iter().any(|voce| voce.brani.contains(&5)),
            "la riga intera se ne va con l'immagine"
        );
        assert_eq!(
            super::brani_senza_cestino(&piano, &scelti, ha_un_cestino),
            3
        );
        // Scelta anche lei, conta.
        let tutti = [1, 2, 3, 4, 5];
        let piano = da_togliere(connection, &tutti, true).expect("tutti");
        assert_eq!(super::brani_senza_cestino(&piano, &tutti, ha_un_cestino), 4);
    }
}
