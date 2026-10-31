//! Lo Skin Studio: il registro, la validazione, la bozza.
//!
//! # Lo Studio non aggiunge potere
//!
//! Rende visibile un contratto che oggi si scopre leggendo Rust. Tutto quel che
//! serve a un editor di skin esiste già nel crate: i token con la loro
//! descrizione, i loro estremi e il flag di obbligatorietà, i preset che ne
//! scrivono blocchi coerenti, le parti coi loro gruppi, gli effetti col costo
//! dichiarato, un vocabolario chiuso per parte, e `nearest_parts()` per i
//! refusi. Nessuno di questi dati usciva dal processo. (Quanti sono non lo si
//! scrive qui: lo dicono i registri del nucleo, e un numero ricopiato in un
//! commento è il primo a restare indietro — qui diceva «undici effetti» quando
//! erano già dodici.)
//!
//! Questo modulo non decide niente: legge il registro e chiama `parse_skin`,
//! `check_skin`, `compile_skin`. È la stessa regola dei comandi — nessuna
//! decisione fuori dal nucleo — e vale doppio qui, perché la stessa validazione
//! dovrà girare identica dentro l'APK.
//!
//! # Perché la sorgente di verità è il testo JSON
//!
//! Perché `SkinDocument` **non è riserializzabile**, e non è una svista: sta
//! scritto in `document.rs`, sopra la sua definizione. Un colore validato è una
//! quaterna di canali, e riscriverlo produrrebbe un manifest che quella stessa
//! validazione rifiuta — `rgb(9 9 13)` non è una forma che il formato accetta in
//! ingresso.
//!
//! La conseguenza è l'architettura dello Studio: si edita **il testo**, e la
//! vista a controlli produce modifiche su quel testo. Le due viste di
//! `Rymembra - impalcatura` — clicca o programma — non sono due modelli
//! sincronizzati: sono due tastiere sullo stesso documento. E il documento che
//! esce è il file che si mette in git.

mod registro;
mod validazione;

pub use self::registro::*;
pub use self::validazione::*;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use rymembra_app::scrittura::scrivi_atomico;
use rymembra_domain::errors::{AppError, ErrorCode};
use serde::Serialize;
use tauri::State;

use crate::errore::{ErroreIpc, Esito, errore};
use crate::stato::{Stato, con_libreria};

// ── La bozza, il pacchetto, le istantanee ───────────────────────────────────

/// Dove stanno le bozze dello Studio.
fn cartella_bozze(data_dir: &Path) -> PathBuf {
    data_dir.join("skin").join("bozze")
}

/// Le bozze hanno un lucchetto loro.
///
/// Fino alla 3.0 ogni comando dello Studio prendeva quello della libreria —
/// l'unico, quello della connessione al database — per scrivere un file in
/// una cartella che col database non ha niente a che fare. Dietro un
/// salvataggio della bozza si accodava la scansione, e dietro la scansione il
/// salvataggio: su un disco lento, lo Studio che non salva mentre la libreria
/// si aggiorna. Le bozze però un ordine fra loro lo vogliono — una rinomina e
/// un salvataggio sulla stessa cartella non si devono incrociare — e questo è
/// quell'ordine, senza il resto.
static BOZZE: Mutex<()> = Mutex::new(());

/// La cartella dati, senza il lucchetto della libreria.
///
/// Se la libreria non si è aperta non c'è una cartella: allora si chiede a
/// [`con_libreria`], che sa dire il guasto col suo codice. La usa anche
/// `skin.rs`, per i comandi che leggono pacchetti e non il database.
pub(crate) fn cartella_dati(stato: &Stato) -> Result<PathBuf, AppError> {
    match stato.cartella_dati() {
        Some(dati) => Ok(dati.to_path_buf()),
        None => con_libreria(stato, |libreria| Ok(libreria.data_dir.clone())),
    }
}

/// Fa `azione` sulle bozze, col loro lucchetto e senza quello della libreria.
fn con_le_bozze<T>(
    stato: &Stato,
    azione: impl FnOnce(&Path) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let dati = cartella_dati(stato)?;
    let _turno = BOZZE.lock().unwrap_or_else(PoisonError::into_inner);
    azione(&dati)
}

/// L'identificatore è anche un nome di cartella: non deve poter uscire.
///
/// Stesso controllo di `skin.rs`, e ripetuto apposta: quello protegge la
/// cartella delle skin installate, questo quella delle bozze, e sono due porte
/// diverse. Un giorno una delle due potrebbe accettare qualcosa che l'altra no.
fn id_sicuro(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// L'id è uno che il formato accetta: da 2 a 48 fra minuscole, cifre e
/// trattini, e comincia con una lettera.
///
/// È la regola di `rymembra_skin::document` (`identificatore`), che lì è
/// privata: copiata qui, e legata all'originale dalla prova
/// `l_id_dichiarato_segue_la_regola_del_formato`. Serve a [`id_dichiarato`]:
/// con [`id_sicuro`] al suo posto, un «Notturno» scritto come un nome spostava
/// la bozza in `bozze/Notturno` — un id che il formato poi rifiuta — e la
/// correzione in «notturno» trovava su Windows la propria cartella e la
/// scambiava per quella di un altro.
fn id_del_formato(id: &str) -> bool {
    let mut caratteri = id.bytes();
    caratteri.next().is_some_and(|b| b.is_ascii_lowercase())
        && caratteri.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && (2..=48).contains(&id.len())
}

fn non_trovata(id: &str) -> AppError {
    AppError::new(ErrorCode::SkinNotFound { id: id.to_owned() })
}

/// Quel su cui lo Studio sta lavorando: il manifest, e le risorse che
/// l'esportazione dovrà rimettere dentro.
///
/// Il manifest può venire da una bozza; le risorse no. Una bozza è un
/// `skin.json` e basta — i caratteri e le immagini restano dentro il `.ryskin`
/// installato mentre si accorda il documento. Tenere le due cose in una
/// funzione sola è ciò che fa combaciare l'albero della vista Documento con quel
/// che l'esportazione scrive davvero: due letture separate divergerebbero il
/// giorno in cui una delle due impara qualcosa che l'altra no.
struct DaLavorare {
    sorgente: String,
    preview: Option<Vec<u8>>,
    assets: Vec<rymembra_skin::package::SkinAsset>,
}

/// Cerca prima fra le bozze, poi fra quelle di serie, poi fra le installate.
///
/// L'ordine è quello: chi ha lasciato una bozza a metà la ritrova, e non riparte
/// dall'originale che stava modificando.
///
/// «Quelle di serie» sono tutte, non solo Plain: fino alla 3.0 qui si
/// guardava soltanto `plain`, e derivare da Vinile o da Neon — che non stanno
/// fra le installate, perché non si installano — diceva «skin non installata».
///
/// # Il pacchetto si legge senza validarlo
///
/// Con le guardie dell'archivio sì, con la validazione del documento no
/// ([`rymembra_skin::package::leggi_pacchetto_grezzo`]). Fino alla 3.0 qui c'era
/// `read_skin_package` con un `?`: una skin installata che non passava più la
/// validazione — una regola nuova del formato, un budget diventato errore —
/// non si apriva nello Studio, cioè nell'unico posto dove la si poteva
/// aggiustare. E il `?` stava **prima** della bozza: bastava un pacchetto
/// rotto accanto a una bozza buona per perdere l'accesso a tutte e due.
/// Adesso il testo si apre com'è, e la validazione dice riga per riga cosa non
/// va. Un archivio che non si legge nemmeno così resta un guasto da dire —
/// ma solo se non c'è una bozza da aprire al suo posto.
fn da_lavorare(data_dir: &Path, id: &str) -> Result<DaLavorare, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }

    // Un file che non si apre vuol dire «non installata»; un file che si apre e
    // non si legge è un guasto, e va detto invece che scambiato per assenza.
    let installata = data_dir.join("skin").join(format!("{id}.ryskin"));
    let pacchetto = std::fs::read(&installata)
        .ok()
        .map(|bytes| rymembra_skin::package::leggi_pacchetto_grezzo(&bytes));
    let letto = pacchetto.as_ref().and_then(|esito| esito.as_ref().ok());

    let bozza = cartella_bozze(data_dir)
        .join(id)
        .join(rymembra_skin::package::MANIFEST_NAME);
    let sorgente = if let Ok(testo) = std::fs::read_to_string(&bozza) {
        testo
    } else if let Some(di_serie) = rymembra_skin::di_serie(id) {
        di_serie.to_owned()
    } else {
        match pacchetto.as_ref() {
            Some(Ok(letto)) => letto.source.clone(),
            Some(Err(guasto)) => return Err(guasto.clone()),
            None => return Err(non_trovata(id)),
        }
    };

    // Le risorse della bozza comandano quando la bozza ha la sua cartella:
    // è lì che lo Studio aggiunge e toglie.
    let risorse = cartella_risorse(data_dir, id);
    let assets = if risorse.exists() {
        risorse_della_bozza(&risorse)
    } else {
        letto.map_or_else(Vec::new, |letto| letto.assets.clone())
    };
    Ok(DaLavorare {
        sorgente,
        preview: letto.and_then(|letto| letto.preview.clone()),
        assets,
    })
}

/// Dove stanno le istantanee di una skin.
///
/// **Fuori** dalla bozza: fino alla 2.x stavano in `bozze/<id>/istantanee`, e
/// «Scarta bozza» — che butta la cartella della bozza — le portava via con
/// sé, compresa quella presa un attimo prima proprio per poter tornare
/// indietro. La rete sotto il bottone irreversibile se ne andava col bottone.
fn cartella_istantanee(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join("skin").join("istantanee").join(id)
}

/// Le istantanee di una skin, dopo averle portate fuori dalla bozza se
/// stavano ancora dentro.
///
/// Il trasloco è per voce e non per cartella: se da una versione all'altra ne
/// sono nate anche nella cartella nuova, le due serie si uniscono invece di
/// scavalcarsi. Un file che non si sposta resta dov'era — la prossima volta ci
/// si riprova — e non ferma niente.
fn istantanee_di(data_dir: &Path, id: &str) -> PathBuf {
    let nuova = cartella_istantanee(data_dir, id);
    let vecchia = cartella_bozze(data_dir).join(id).join("istantanee");
    if let Ok(voci) = std::fs::read_dir(&vecchia)
        && std::fs::create_dir_all(&nuova).is_ok()
    {
        for voce in voci.flatten() {
            drop(std::fs::rename(voce.path(), nuova.join(voce.file_name())));
        }
        drop(std::fs::remove_dir(&vecchia));
    }
    nuova
}

/// Dove stanno le risorse della bozza: caratteri e immagini.
///
/// È la cartella che il protocollo `rymembra-skin` serve all'anteprima e alla
/// prova sull'app (`skin::servi_risorsa`), e quella da cui l'esportazione e
/// «Salva e usa» riempiono il pacchetto.
fn cartella_risorse(data_dir: &Path, id: &str) -> PathBuf {
    cartella_bozze(data_dir).join(id).join("assets")
}

/// Le risorse della bozza, in ordine di nome.
///
/// Si rileggono i byte a ogni volta, e quel che non passa il controllo del
/// formato — un nome storto, un `.png` che non è un PNG — si salta: nella
/// cartella qualcuno ci può aver messo le mani, e una risorsa che il pacchetto
/// rifiuterebbe non deve arrivare fino all'esportazione per farla fallire.
fn risorse_della_bozza(cartella: &Path) -> Vec<rymembra_skin::package::SkinAsset> {
    let Ok(voci) = std::fs::read_dir(cartella) else {
        return Vec::new();
    };
    let mut risorse: Vec<rymembra_skin::package::SkinAsset> = voci
        .flatten()
        .filter_map(|voce| {
            let name = voce.file_name().to_str()?.to_owned();
            let bytes = std::fs::read(voce.path()).ok()?;
            let mime = rymembra_skin::riconosci_risorsa(&name, &bytes)?;
            Some(rymembra_skin::package::SkinAsset { name, mime, bytes })
        })
        .collect();
    risorse.sort_by(|a, b| a.name.cmp(&b.name));
    risorse
}

/// Porta nella bozza le risorse del pacchetto.
///
/// Una volta sola: se la cartella c'è già, la bozza ha le sue e quelle
/// comandano. Senza `forza` la cartella nasce solo se c'è qualcosa da
/// metterci — aprire nello Studio una skin senza risorse non deve lasciare
/// cartelle vuote in giro.
fn materializza_risorse(
    data_dir: &Path,
    id: &str,
    del_pacchetto: &[rymembra_skin::package::SkinAsset],
    forza: bool,
) -> Result<(), AppError> {
    let cartella = cartella_risorse(data_dir, id);
    if cartella.exists() || (del_pacchetto.is_empty() && !forza) {
        return Ok(());
    }
    std::fs::create_dir_all(&cartella)
        .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;
    for risorsa in del_pacchetto {
        scrivi_atomico(&cartella.join(&risorsa.name), &risorsa.bytes)?;
    }
    Ok(())
}

/// Il nome che una risorsa prende nel pacchetto, dal nome del file.
///
/// Il formato ammette minuscole, cifre, trattini e trattini bassi, con
/// un'estensione sola: un «Titoli Grassetto.WOFF2» diventa
/// «titoli-grassetto.woff2» invece di essere rifiutato per una maiuscola.
fn nome_risorsa(percorso: &Path) -> String {
    let radice: String = percorso
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let radice = radice.trim_matches(|c: char| c == '-' || c == '_');
    let radice = if radice.is_empty() { "risorsa" } else { radice };
    let estensione = percorso
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let estensione = if estensione == "jpeg" {
        "jpg".to_owned()
    } else {
        estensione
    };
    format!("{radice}.{estensione}")
}

/// Un nome che nella cartella non c'è ancora: `trama.png`, `trama-2.png`, …
fn nome_libero(cartella: &Path, nome: &str) -> String {
    if !cartella.join(nome).exists() {
        return nome.to_owned();
    }
    let (radice, estensione) = nome.rsplit_once('.').unwrap_or((nome, ""));
    (2..)
        .map(|n| format!("{radice}-{n}.{estensione}"))
        .find(|candidato| !cartella.join(candidato).exists())
        .unwrap_or_else(|| nome.to_owned())
}

fn rifiutata(nome: &str, perche: &str) -> AppError {
    AppError::new(ErrorCode::SkinAssetRejected {
        asset: nome.to_owned(),
        reason: perche.to_owned(),
    })
}

/// Aggiunge una risorsa alla bozza, da un file scelto sul disco.
///
/// Il tipo si decide dai byte, come dentro un pacchetto: un file chiamato
/// `.png` che non lo è si ferma qui, col suo nome, invece che all'esportazione.
///
/// E così la misura: la risorsa entra solo se il pacchetto che ne verrebbe sta
/// nei limiti della lettura (`entra_in_un_pacchetto_con_nomi`). Fino alla 3.0
/// si contavano soltanto le voci, e una bozza poteva raccogliere trenta
/// megabyte di immagini che nessun `.ryskin` avrebbe mai contenuto — lo si
/// scopriva esportando, cioè dopo averci lavorato sopra. La misura è quella
/// coi nomi veri, compreso quello che la risorsa nuova prende davvero
/// (`trama-2.png` se `trama.png` c'è già): la stima senza nomi sbaglia per
/// difetto con i nomi oltre gli ottanta caratteri, e una risorsa entrava nella
/// bozza per poi fermare l'esportazione.
fn aggiungi_risorsa(data_dir: &Path, id: &str, percorso: &Path) -> Result<VoceFileIpc, AppError> {
    use rymembra_skin::package::limits::MAX_ENTRY_BYTES;

    let da = da_lavorare(data_dir, id)?;
    materializza_risorse(data_dir, id, &da.assets, true)?;
    let cartella = cartella_risorse(data_dir, id);

    let nome = nome_risorsa(percorso);
    let byte = std::fs::metadata(percorso)
        .map_err(|err| rymembra_app::files::io_error(&percorso.display().to_string(), &err))?
        .len();
    if byte > MAX_ENTRY_BYTES {
        return Err(AppError::new(ErrorCode::SkinTooLarge {
            bytes: byte,
            limit_bytes: MAX_ENTRY_BYTES,
        }));
    }
    let bytes = std::fs::read(percorso)
        .map_err(|err| rymembra_app::files::io_error(&percorso.display().to_string(), &err))?;
    if rymembra_skin::riconosci_risorsa(&nome, &bytes).is_none() {
        return Err(rifiutata(
            &nome,
            "una risorsa è un'immagine png, jpg o webp, o un carattere woff2, e i byte devono dire lo stesso dell'estensione",
        ));
    }
    let quanto = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let libero = nome_libero(&cartella, &nome);
    let gia = risorse_della_bozza(&cartella);
    rymembra_skin::package::entra_in_un_pacchetto_con_nomi(
        quanto(da.sorgente.len()),
        da.preview.as_ref().map(|p| quanto(p.len())),
        gia.iter()
            .map(|r| (r.name.as_str(), quanto(r.bytes.len())))
            .chain([(libero.as_str(), byte)]),
    )?;
    scrivi_atomico(&cartella.join(&libero), &bytes)?;
    Ok(VoceFileIpc {
        nome: format!("assets/{libero}"),
        byte,
        genere: "risorsa",
    })
}

/// Aggiunge una risorsa alla bozza. Restituisce la voce, col nome che ha preso.
#[tauri::command(async)]
pub fn studio_risorsa_aggiungi(
    stato: State<'_, Stato>,
    id: String,
    percorso: String,
) -> Esito<VoceFileIpc> {
    con_le_bozze(&stato, |dati| {
        aggiungi_risorsa(dati, &id, Path::new(&percorso))
    })
    .map_err(errore)
}

/// Toglie una risorsa dalla bozza.
#[tauri::command(async)]
pub fn studio_risorsa_togli(stato: State<'_, Stato>, id: String, nome: String) -> Esito<()> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) || rymembra_skin::tipo_risorsa(&nome).is_none() {
            return Err(non_trovata(&id));
        }
        let da = da_lavorare(dati, &id)?;
        materializza_risorse(dati, &id, &da.assets, true)?;
        let file = cartella_risorse(dati, &id).join(&nome);
        match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(rymembra_app::files::io_error(
                &file.display().to_string(),
                &err,
            )),
        }
    })
    .map_err(errore)
}

/// Quel che serve per mettere la bozza in un pacchetto: la miniatura del
/// pacchetto installato, e le risorse della bozza.
///
/// Lo usa anche «Salva e usa» in `skin.rs`: prima impacchettava il solo
/// manifest, e una skin col suo carattere installata da qui perdeva il
/// carattere e la miniatura. Prende da sé il lucchetto delle bozze.
pub fn corredo_della_bozza(
    data_dir: &Path,
    id: &str,
) -> Result<(Option<Vec<u8>>, Vec<rymembra_skin::package::SkinAsset>), AppError> {
    let _turno = BOZZE.lock().unwrap_or_else(PoisonError::into_inner);
    corredo(data_dir, id)
}

/// [`corredo_della_bozza`], per chi il lucchetto delle bozze l'ha già.
fn corredo(
    data_dir: &Path,
    id: &str,
) -> Result<(Option<Vec<u8>>, Vec<rymembra_skin::package::SkinAsset>), AppError> {
    let da = da_lavorare(data_dir, id)?;
    Ok((da.preview, da.assets))
}

/// Il testo di una skin da aprire nello Studio.
///
/// Una skin del formato 1 si apre **migrata**: il testo che lo Studio mostra
/// è quello del formato di oggi, e la migrazione si scrive subito come bozza —
/// così alla prossima apertura non si rifà, e non si fotografa due volte. Il
/// testo di prima resta in un'istantanea di causa `migrata`, che è da dove si
/// riparte se la migrazione ha tradito qualcosa.
#[tauri::command(async)]
pub fn studio_documento(stato: State<'_, Stato>, id: String) -> Esito<String> {
    con_le_bozze(&stato, |dati| apri_documento(dati, &id, true)).map_err(errore)
}

/// Quel che fa [`studio_documento`], senza lo stato di Tauri attorno.
///
/// `fotografa` dice se la migrazione di un testo del formato 1 si prende la
/// sua istantanea. Sì all'apertura; no dopo «Scarta bozza», che riapre lo
/// stesso pacchetto: la fotografia di quel testo c'è già dalla prima
/// apertura, e ogni «Scarta» ne aggiungeva un'altra uguale — la serie delle
/// istantanee si riempiva di copie dello stesso file, e le venti che si
/// tengono spingevano fuori quelle che contavano.
fn apri_documento(data_dir: &Path, id: &str, fotografa: bool) -> Result<String, AppError> {
    let da = da_lavorare(data_dir, id)?;
    materializza_risorse(data_dir, id, &da.assets, false)?;
    match rymembra_skin::migra_testo(&da.sorgente) {
        Ok(migrato) if migrato.da < u64::from(rymembra_skin::SKIN_FORMAT_VERSION) => {
            if fotografa {
                prendi_istantanea(&istantanee_di(data_dir, id), &da.sorgente, Causa::Migrata)?;
            }
            let file = cartella_bozze(data_dir)
                .join(id)
                .join(rymembra_skin::package::MANIFEST_NAME);
            scrivi_atomico(&file, migrato.sorgente.as_bytes())?;
            Ok(migrato.sorgente)
        }
        // Già del formato di oggi, o non ancora JSON: si apre com'è, e la
        // validazione dirà dove.
        _ => Ok(da.sorgente),
    }
}

/// Una voce del pacchetto, come la mostra la colonna sinistra dello Studio.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceFileIpc {
    /// Il nome dentro l'archivio: `skin.json`, `preview.png`, `assets/x.woff2`.
    pub nome: String,
    /// Quanto pesa, decompresso.
    pub byte: u64,
    /// `manifest`, `miniatura` o `risorsa`.
    pub genere: &'static str,
}

/// Cosa c'è nel pacchetto su cui si sta lavorando.
///
/// Non è l'elenco di una cartella: è l'elenco delle voci che il **formato**
/// ammette, cioè esattamente quel che `studio_esporta` rimetterà nell'archivio.
/// Mostrare un file che l'esportazione butterebbe via sarebbe peggio che non
/// mostrare niente.
#[tauri::command(async)]
pub fn studio_pacchetto(stato: State<'_, Stato>, id: String) -> Esito<Vec<VoceFileIpc>> {
    con_le_bozze(&stato, |dati| {
        let da = da_lavorare(dati, &id)?;
        let quanto = |byte: usize| u64::try_from(byte).unwrap_or(u64::MAX);
        let mut voci = vec![VoceFileIpc {
            nome: rymembra_skin::package::MANIFEST_NAME.to_owned(),
            byte: quanto(da.sorgente.len()),
            genere: "manifest",
        }];
        if let Some(miniatura) = &da.preview {
            voci.push(VoceFileIpc {
                nome: rymembra_skin::package::PREVIEW_NAME.to_owned(),
                byte: quanto(miniatura.len()),
                genere: "miniatura",
            });
        }
        voci.extend(da.assets.iter().map(|risorsa| VoceFileIpc {
            nome: format!("assets/{}", risorsa.name),
            byte: quanto(risorsa.bytes.len()),
            genere: "risorsa",
        }));
        Ok(voci)
    })
    .map_err(errore)
}

/// Com'è andato un salvataggio della bozza.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvataggioIpc {
    /// L'id sotto cui la bozza sta adesso: quello nuovo, se il documento ha
    /// cambiato `id` e la rinomina è passata.
    pub id: String,
    /// Perché la rinomina non è passata, quando il documento dichiara un id
    /// che la bozza non ha potuto prendere. Il testo è salvato lo stesso,
    /// sotto l'id di prima.
    pub rinomina: Option<ErroreIpc>,
}

/// L'`id` che il testo dichiara, se è uno che il formato accetta.
///
/// Letto dal JSON e non dal documento validato: una bozza si salva anche a
/// metà, e un token sbagliato altrove non toglie niente al suo `id`. Ma l'id
/// stesso deve passare la regola del formato ([`id_del_formato`]): uno che la
/// validazione rifiuta non è un posto in cui spostare la bozza.
fn id_dichiarato(sorgente: &str) -> Option<String> {
    let valore: serde_json::Value = serde_json::from_str(sorgente).ok()?;
    let id = valore.get("id")?.as_str()?;
    id_del_formato(id).then(|| id.to_owned())
}

/// Salva la bozza, dopo averla spostata sotto l'id che il documento dichiara.
///
/// # Perché la rinomina sta qui
///
/// Fino alla 3.0 erano due comandi, e la finestra li chiamava a un secondo e
/// mezzo l'uno dall'altro: il salvataggio scriveva nella cartella vecchia, la
/// rinomina — più tardi, se il testo era valido, se nessuno aveva annullato —
/// la spostava. In mezzo c'erano la prova sull'app e l'esportazione, che
/// cercavano le risorse sotto l'id del documento in una cartella che ancora
/// non c'era, e un rifiuto della rinomina che nessuno vedeva: lo si buttava
/// con un `catch` vuoto. Adesso è un passo solo, sotto il lucchetto delle
/// bozze, e quel che è successo torna indietro: dove sta la bozza, e perché
/// non si è spostata se non si è spostata.
///
/// # Prima si scrive, poi si sposta
///
/// Il testo si scrive nella cartella che la finestra conosce, e solo dopo la
/// cartella si sposta. Al contrario, una scrittura fallita dopo una rinomina
/// riuscita (disco pieno, `skin.json` tenuto aperto) restituiva un errore con
/// la bozza già in `bozze/seconda`: la finestra restava su «prima», il
/// salvataggio dopo scambiava la propria cartella spostata per quella di
/// un'altra bozza, e il testo finiva in una `bozze/prima` nuova, senza risorse
/// né conversazione.
fn salva_bozza(
    data_dir: &Path,
    id: &str,
    sorgente: &str,
    origine: Option<&str>,
) -> Result<SalvataggioIpc, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let file = cartella_bozze(data_dir)
        .join(id)
        .join(rymembra_skin::package::MANIFEST_NAME);
    scrivi_atomico(&file, sorgente.as_bytes())?;
    let mut dove = id.to_owned();
    let mut rinomina = None;
    if let Some(dichiarato) = id_dichiarato(sorgente).filter(|d| d != id) {
        match rinomina_bozza(data_dir, id, &dichiarato, origine) {
            Ok(()) => dove = dichiarato,
            Err(rifiuto) => rinomina = Some(ErroreIpc::from(rifiuto)),
        }
    }
    Ok(SalvataggioIpc { id: dove, rinomina })
}

/// Salva la bozza.
///
/// Si salva **anche se non è valida**: una bozza è un lavoro in corso, e
/// rifiutare di scrivere un documento a metà vorrebbe dire perderlo chiudendo la
/// finestra. È la validazione a dire cosa non va, non il salvataggio.
///
/// Se il documento ha cambiato `id` la bozza lo segue: vedi [`salva_bozza`].
/// `origine` è l'id con cui lo Studio è stato aperto.
#[tauri::command(async)]
pub fn studio_salva(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    origine: Option<String>,
) -> Esito<SalvataggioIpc> {
    let esito = con_le_bozze(&stato, |dati| {
        salva_bozza(dati, &id, &sorgente, origine.as_deref())
    })
    .map_err(errore);
    // Una bozza è lavoro non ancora salvato da nessuna parte: è precisamente la
    // cosa che un backup deve portare via per prima.
    crate::nuvola::se_riuscito(&app, esito)
}

/// Butta la bozza e torna a quel che dice il pacchetto.
///
/// # Perché le istantanee restano
///
/// Sono due cose diverse e non si cancellano insieme. La bozza è «dove sono
/// arrivato»; le istantanee sono «dove sono passato», e sono l'unica rete sotto
/// questo bottone — che è irreversibile per natura. Per questo non stanno più
/// dentro la cartella della bozza ma in `skin/istantanee/<id>/`: la 2.x le
/// teneva lì, e questo comando se le portava via.
///
/// Restituisce la sorgente del pacchetto, cioè quel che l'editor deve mostrare
/// da qui in poi: farsela ridire con una seconda chiamata lascerebbe un istante
/// in cui la finestra mostra un documento che non esiste più da nessuna parte.
#[tauri::command(async)]
pub fn studio_scarta(stato: State<'_, Stato>, id: String) -> Esito<String> {
    con_le_bozze(&stato, |dati| scarta_bozza(dati, &id)).map_err(errore)
}

/// Quel che fa [`studio_scarta`], senza lo stato di Tauri attorno.
///
/// # Prima di cancellare, un posto a cui tornare
///
/// Una bozza che porta un id senza pacchetto — il risultato normale di
/// cambiare l'id di una skin derivata — non ha niente dietro di sé: la
/// cartella se ne andava con le risorse e la conversazione, e poi
/// `apri_documento` rispondeva «skin non trovata» con l'editor che mostrava
/// ancora, già segnato come salvato, un testo che sul disco non c'era più. Si
/// guarda quindi prima: una skin di serie, o un pacchetto che si legge — la
/// stessa lettura di [`da_lavorare`] — e senza nessuno dei due non si tocca
/// niente.
fn scarta_bozza(data_dir: &Path, id: &str) -> Result<String, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let pacchetto = data_dir.join("skin").join(format!("{id}.ryskin"));
    let si_torna = rymembra_skin::di_serie(id).is_some()
        || std::fs::read(&pacchetto)
            .is_ok_and(|bytes| rymembra_skin::package::leggi_pacchetto_grezzo(&bytes).is_ok());
    if !si_torna {
        return Err(non_trovata(id).with_cause(
            "questa bozza non ha un pacchetto a cui tornare: buttarla vorrebbe dire perderla"
                .to_owned(),
        ));
    }
    let cartella = cartella_bozze(data_dir).join(id);
    if cartella.exists() {
        std::fs::remove_dir_all(&cartella)
            .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;
    }
    // Dopo la cancellazione `da_lavorare` non trova più la bozza e ricade
    // sul pacchetto: è la stessa funzione che decide all'apertura, quindi
    // non c'è un secondo posto in cui la regola «la bozza vince» possa
    // divergere. Le istantanee non stanno più qui dentro, e restano — e non
    // se ne aggiunge un'altra per la migrazione: vedi `apri_documento`.
    apri_documento(data_dir, id, false)
}

/// Scrive un `.ryskin` da una sorgente, con dentro le risorse che aveva.
///
/// Gli **errori** bloccano, gli avvisi no: è la regola scritta nel disegno, e
/// qui è per costruzione — `parse_skin_json` fallisce sugli errori e non sa
/// niente degli avvisi.
///
/// Il manifest è quello dell'editor, le risorse quelle del pacchetto: chi aveva
/// una skin con un carattere dentro lo perdeva esportandola da qui, e l'albero
/// della vista Documento avrebbe mostrato file che l'esportazione buttava via.
///
/// Il percorso arriva dalla pagina, e si scrive solo se finisce in `.ryskin`:
/// fino alla 2.4 questo comando scriveva un pacchetto sotto qualunque nome.
/// Vedi [`crate::destinazioni`].
///
/// # Errori
///
/// `fs.extensionRefused` o `fs.pathInvalid` per un percorso che la finestra non
/// può chiedere, **prima** di prendere il lucchetto; quelli della validazione e
/// della scrittura per il resto.
#[tauri::command(async)]
pub fn studio_esporta(
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    percorso: String,
) -> Esito<()> {
    let file =
        crate::destinazioni::file_da_scrivere(&percorso, crate::destinazioni::ESTENSIONI_SKIN)
            .map_err(errore)?;
    con_le_bozze(&stato, |dati| {
        let da = da_lavorare(dati, &id)?;
        // `write_skin_package` valida da sé prima di scrivere — passa dalle
        // stesse guardie della lettura — quindi qui non serve un `parse` in più:
        // sarebbe una seconda validazione che può divergere dalla prima.
        let bytes =
            rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
                source: &sorgente,
                preview: da.preview.as_deref(),
                assets: &da.assets,
            })?;
        // Intero o niente anche qui: un `.ryskin` a metà sul disco di chi lo
        // voleva mandare a qualcuno è un pacchetto che si scopre rotto
        // dall'altra parte.
        scrivi_atomico(&file.percorso, &bytes)
    })
    .map_err(errore)
}

/// Sposta la bozza e le sue istantanee sotto un altro identificatore.
///
/// Serve quando nel documento cambia `id`: la bozza vive in una cartella col
/// nome dell'id, e un documento che dice «sono nocturne» dentro la cartella di
/// «plain» si ritroverebbe riaprendo plain. Non si scavalca niente: se sotto
/// il nome nuovo c'è già una bozza, una skin di serie o una skin **installata**
/// si dice di no. L'ultima mancava fino alla 3.0: una bozza rinominata come
/// una skin installata di un altro, alla prima «Salva e usa», ne prendeva il
/// posto. Il rifiuto di qui però ferma la rinomina, non l'installazione: il
/// testo resta nella bozza col suo `"id"`, e a non lasciarlo installare sopra
/// l'altra skin è `skin::installa_sorgente`, con la stessa regola.
///
/// `origine` è la skin da cui lo Studio è partito: tornare al suo id non è
/// prendere il posto di un'altra, è rimettere a posto il proprio. Senza,
/// chi cambiava l'`id` e ci ripensava restava chiuso fuori dalla sua skin.
///
/// # Le istantanee di un altro
///
/// Anche una serie di istantanee sotto il nome nuovo è un conflitto, come per
/// [`occupati`]: quelle di una skin buttata e disinstallata restano, e la
/// bozza che ne prendeva il nome si ritrovava nell'elenco le istantanee di
/// un'altra skin mentre le sue restavano indietro. Tornando all'`origine` le
/// istantanee che ci sono sono sue, e le due serie si uniscono voce per voce.
///
/// # Solo le maiuscole
///
/// `Notturno` → `notturno` su Windows è la stessa cartella, ed `exists()`
/// sull'arrivo risponde sì: era un falso «id già preso» sulla propria bozza.
/// Si riconosce dalla cartella stessa, e si rinomina passando per un nome che
/// nessun id può avere.
///
/// # Dopo che la cartella si è spostata
///
/// La bozza sta dove sta la sua cartella, e lì la deve cercare la finestra:
/// una volta spostata, le istantanee la seguono se ci riescono, e un loro
/// guasto non fa più tornare un errore che [`salva_bozza`] leggerebbe come
/// «non spostata».
fn rinomina_bozza(
    data_dir: &Path,
    da: &str,
    a: &str,
    origine: Option<&str>,
) -> Result<(), AppError> {
    if !id_sicuro(da) || !id_sicuro(a) {
        return Err(non_trovata(a));
    }
    if da == a {
        return Ok(());
    }
    let partenza = cartella_bozze(data_dir).join(da);
    let arrivo = cartella_bozze(data_dir).join(a);
    if da.eq_ignore_ascii_case(a) && stessa_cartella(&partenza, &arrivo) {
        return rinomina_le_maiuscole(data_dir, da, a);
    }
    let sua = origine == Some(a);
    let installata = !sua && data_dir.join("skin").join(format!("{a}.ryskin")).exists();
    let istantanee_altrui = !sua && cartella_istantanee(data_dir, a).exists();
    if rymembra_skin::di_serie(a).is_some() || arrivo.exists() || installata || istantanee_altrui {
        return Err(
            AppError::new(ErrorCode::SkinIdConflict { id: a.to_owned() }).with_cause(
                "sotto questo identificatore c'è già una skin di serie, una skin installata, \
                 un'altra bozza o le istantanee di un'altra skin"
                    .to_owned(),
            ),
        );
    }
    if partenza.exists() {
        std::fs::create_dir_all(cartella_bozze(data_dir))
            .map_err(|err| rymembra_app::files::io_error(&arrivo.display().to_string(), &err))?;
        std::fs::rename(&partenza, &arrivo)
            .map_err(|err| rymembra_app::files::io_error(&arrivo.display().to_string(), &err))?;
    }
    sposta_istantanee(data_dir, da, a);
    Ok(())
}

/// Le due cartelle sono la stessa sul disco: il caso di due nomi che
/// differiscono solo per le maiuscole su un file system che non le distingue.
fn stessa_cartella(una: &Path, altra: &Path) -> bool {
    match (std::fs::canonicalize(una), std::fs::canonicalize(altra)) {
        (Ok(una), Ok(altra)) => una == altra,
        _ => false,
    }
}

/// Porta la bozza e le sue istantanee da `da` ad `a`, che differiscono solo
/// per le maiuscole: vedi [`rinomina_bozza`]. Il passaggio intermedio ha un
/// punto nel nome, che [`id_sicuro`] non ammette: non può essere la cartella
/// di nessuna bozza.
fn rinomina_le_maiuscole(data_dir: &Path, da: &str, a: &str) -> Result<(), AppError> {
    let bozze = cartella_bozze(data_dir);
    let di_passaggio = bozze.join(format!("{a}.maiuscole"));
    std::fs::rename(bozze.join(da), &di_passaggio)
        .map_err(|err| rymembra_app::files::io_error(&di_passaggio.display().to_string(), &err))?;
    let arrivo = bozze.join(a);
    std::fs::rename(&di_passaggio, &arrivo)
        .map_err(|err| rymembra_app::files::io_error(&arrivo.display().to_string(), &err))?;
    let istantanee = cartella_istantanee(data_dir, da);
    if istantanee.exists() {
        let di_passaggio = istantanee.with_file_name(format!("{a}.maiuscole"));
        if std::fs::rename(&istantanee, &di_passaggio).is_ok() {
            drop(std::fs::rename(
                &di_passaggio,
                cartella_istantanee(data_dir, a),
            ));
        }
    }
    Ok(())
}

/// Le istantanee di `da` passano ad `a`: la cartella intera se sotto `a` non
/// ce n'è, se no voce per voce come in [`istantanee_di`]. Per quel che si
/// riesce: un file che non si sposta resta dov'era, e la bozza resta comunque
/// dove si è spostata.
fn sposta_istantanee(data_dir: &Path, da: &str, a: &str) {
    let istantanee = istantanee_di(data_dir, da);
    if !istantanee.exists() {
        return;
    }
    let nuove = cartella_istantanee(data_dir, a);
    if !nuove.exists() {
        if let Some(sopra) = nuove.parent() {
            drop(std::fs::create_dir_all(sopra));
        }
        if std::fs::rename(&istantanee, &nuove).is_ok() {
            return;
        }
    }
    let Ok(voci) = std::fs::read_dir(&istantanee) else {
        return;
    };
    if std::fs::create_dir_all(&nuove).is_err() {
        return;
    }
    for voce in voci.flatten() {
        drop(std::fs::rename(voce.path(), nuove.join(voce.file_name())));
    }
    drop(std::fs::remove_dir(&istantanee));
}

// ── Un tema nuovo ───────────────────────────────────────────────────────────

/// Gli id che hanno già una cartella nello Studio: bozze e istantanee.
///
/// Un tema nuovo non li deve prendere anche se nessuna skin li porta più. Una
/// bozza rimasta da una skin disinstallata ha dentro le sue risorse e la sua
/// conversazione, e fino alla 3.0 il tema nuovo che ne ereditava il nome le
/// ereditava tutte e due — la conversazione su un'altra skin, e caratteri che
/// il manifest nuovo non nominava. Le istantanee idem: comparivano nell'elenco
/// del tema nuovo come se fossero sue.
fn occupati(data_dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = [
        cartella_bozze(data_dir),
        data_dir.join("skin").join("istantanee"),
    ]
    .iter()
    .filter_map(|cartella| std::fs::read_dir(cartella).ok())
    .flatten()
    .flatten()
    .filter(|voce| voce.path().is_dir())
    .filter_map(|voce| voce.file_name().to_str().map(ToOwned::to_owned))
    .filter(|id| id_sicuro(id))
    .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Gli id che un tema nuovo non può prendere, oltre a quelli delle skin
/// elencate: vedi [`occupati`].
#[tauri::command(async)]
pub fn studio_occupati(stato: State<'_, Stato>) -> Esito<Vec<String>> {
    con_le_bozze(&stato, |dati| Ok(occupati(dati))).map_err(errore)
}

/// Un tema nuovo: il pacchetto con le risorse della base, installato, e la
/// bozza che parte da lì.
///
/// # Perché un comando solo
///
/// Fino alla 3.0 la finestra lo componeva da tre: installa il manifest,
/// salva la bozza, scegli. L'installazione impacchettava il **solo**
/// manifest, quindi un tema nato da una skin con un carattere o un'immagine
/// si fermava lì — il manifest nominava una risorsa che il pacchetto non
/// aveva — e quello nato da una skin con la miniatura la perdeva. E la bozza
/// finiva nella cartella che c'era già, se c'era: vedi [`occupati`].
///
/// Qui le risorse e la miniatura sono quelle della base — della sua bozza, se
/// la base è aperta nello Studio con qualcosa di aggiunto — e un id già preso
/// da una skin, da una bozza o da una serie di istantanee si rifiuta prima di
/// scrivere qualunque cosa.
fn crea_tema(
    data_dir: &Path,
    base: &str,
    sorgente: &str,
) -> Result<crate::skin::VoceSkin, AppError> {
    let documento = rymembra_skin::parse_skin_json(sorgente)?;
    let id = documento.id;
    if !id_sicuro(&id) {
        return Err(non_trovata(&id));
    }
    // Senza badare alle maiuscole: su Windows `bozze/Notte` è la cartella di
    // «notte», e il tema nuovo ne erediterebbe chat e risorse.
    let preso = rymembra_skin::di_serie(&id).is_some()
        || data_dir.join("skin").join(format!("{id}.ryskin")).exists()
        || occupati(data_dir)
            .iter()
            .any(|occupato| occupato.eq_ignore_ascii_case(&id));
    if preso {
        return Err(AppError::new(ErrorCode::SkinIdConflict { id }));
    }
    let (preview, assets) = corredo(data_dir, base)?;
    let bytes = rymembra_skin::write_skin_package(&rymembra_skin::package::WritePackageInput {
        source: sorgente,
        preview: preview.as_deref(),
        assets: &assets,
    })?;
    let voce = crate::skin::installa(data_dir, &bytes)?;
    // La bozza parte allineata all'installato: riaprendo lo Studio, la
    // pillola in testa non dice «da salvare» su un lavoro che nessuno ha
    // ancora toccato. Le risorse la bozza le prende dal pacchetto alla prima
    // apertura, come per ogni skin installata.
    let file = cartella_bozze(data_dir)
        .join(&id)
        .join(rymembra_skin::package::MANIFEST_NAME);
    scrivi_atomico(&file, sorgente.as_bytes())?;
    Ok(voce)
}

/// Crea un tema nuovo da `base`: vedi [`crea_tema`].
///
/// # Errori
///
/// `skin.idConflict` se l'id è già preso; quelli della validazione, del
/// pacchetto e della scrittura per il resto.
#[tauri::command(async)]
pub fn studio_crea(
    app: tauri::AppHandle,
    stato: State<'_, Stato>,
    base: String,
    sorgente: String,
) -> Esito<crate::skin::VoceSkin> {
    let esito = con_le_bozze(&stato, |dati| crea_tema(dati, &base, &sorgente)).map_err(errore);
    crate::nuvola::se_riuscito(&app, esito)
}

// ── La conversazione con la chat ────────────────────────────────────────────

/// Il nome del file della conversazione, dentro la cartella della bozza.
const FILE_CHAT: &str = "chat.json";

/// Quanto può pesare una conversazione salvata.
///
/// Due megabyte sono qualche centinaio di battute con i loro blocchi di
/// modifiche: le immagini non ci finiscono — si mandano e basta — e il testo
/// di una chat vera sta molto sotto. Il tetto è per la finestra che si rompe e
/// scrive in un ciclo, non per chi chiacchiera a lungo.
const CHAT_MASSIMA_MB: usize = 2;
const CHAT_MASSIMA: usize = CHAT_MASSIMA_MB * 1024 * 1024;

/// La conversazione salvata di una bozza, o `None` se non ce n'è.
///
/// Sta dentro la cartella della bozza, accanto a `skin.json`: è una
/// conversazione **su quella bozza**, e «Scarta» se la porta via insieme — le
/// modifiche di cui parla non esistono più. Una rinomina la sposta con la
/// cartella. Il backup su Drive (`rymembra_cloud::pacchetti`) prende soltanto il
/// manifest: quel che si è scritto al modello resta su questo computer.
fn leggi_chat(data_dir: &Path, id: &str) -> Result<Option<String>, AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let file = cartella_bozze(data_dir).join(id).join(FILE_CHAT);
    match std::fs::read_to_string(&file) {
        Ok(testo) => Ok(Some(testo)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(rymembra_app::files::io_error(
            &file.display().to_string(),
            &err,
        )),
    }
}

/// Scrive la conversazione di una bozza. Vuota, la toglie.
///
/// Il contenuto lo decide la finestra, e qui si controlla solo che sia un
/// oggetto JSON di un peso ragionevole: il file lo rilegge lei, e un formato
/// scritto in due posti sarebbe un formato che diverge.
fn scrivi_chat(data_dir: &Path, id: &str, testo: &str) -> Result<(), AppError> {
    if !id_sicuro(id) {
        return Err(non_trovata(id));
    }
    let cartella = cartella_bozze(data_dir).join(id);
    let file = cartella.join(FILE_CHAT);
    if testo.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(
                rymembra_app::files::io_error(&file.display().to_string(), &err),
            ),
            _ => Ok(()),
        };
    }
    // Due rifiuti con due frasi: «troppo lunga» e «non è un oggetto JSON» sono
    // due guasti diversi della finestra, e una frase sola per tutti e due
    // mandava a cercare nel posto sbagliato. Il peso prima, così un testo
    // enorme non si analizza nemmeno.
    let rifiuto = |detail: String| {
        Err(AppError::new(ErrorCode::IpcPayloadInvalid {
            channel: "studio_chat_scrivi".to_owned(),
            detail: Some(detail),
        }))
    };
    if testo.len() > CHAT_MASSIMA {
        return rifiuto(format!(
            "la conversazione supera i {} MB ({} byte)",
            CHAT_MASSIMA_MB,
            testo.len()
        ));
    }
    if !serde_json::from_str::<serde_json::Value>(testo).is_ok_and(|valore| valore.is_object()) {
        return rifiuto("la conversazione non è un oggetto JSON".to_owned());
    }
    scrivi_atomico(&file, testo.as_bytes())
}

/// La conversazione salvata della bozza `id`, o `null`.
#[tauri::command(async)]
pub fn studio_chat_leggi(stato: State<'_, Stato>, id: String) -> Esito<Option<String>> {
    con_le_bozze(&stato, |dati| leggi_chat(dati, &id)).map_err(errore)
}

/// Salva la conversazione della bozza `id`; un testo vuoto la cancella.
#[tauri::command(async)]
pub fn studio_chat_scrivi(stato: State<'_, Stato>, id: String, testo: String) -> Esito<()> {
    con_le_bozze(&stato, |dati| scrivi_chat(dati, &id, &testo)).map_err(errore)
}

// ── Il generatore di tavolozze ──────────────────────────────────────────────

/// Un colore generato, per un token.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoceTavolozzaIpc {
    /// `color.surface.0`.
    pub token: &'static str,
    /// `#rrggbb` o `#rrggbbaa`.
    pub valore: String,
}

/// Una tavolozza intera da una tinta: un colore scritto, o la copertina.
///
/// La copertina è quella di un brano della libreria, per impronta: la tinta
/// la sa già il magazzino delle copertine, che la calcola per l'accento che
/// segue la musica. Niente si decodifica qui.
#[tauri::command(async)]
pub fn studio_tavolozza_da(
    stato: State<'_, Stato>,
    colore: Option<String>,
    copertina: Option<String>,
    scura: bool,
) -> Esito<Vec<VoceTavolozzaIpc>> {
    let tinta = match (colore, copertina) {
        (Some(scritto), _) => rymembra_skin::values::parse_color(&scritto)
            .ok_or_else(|| errore(rifiutata("colore", "non è un colore")))?,
        (None, Some(impronta)) => {
            if !crate::copertine::e_un_impronta(&impronta) {
                return Err(errore(rifiutata("copertina", "non è un'impronta")));
            }
            let copertine =
                con_libreria(&stato, |libreria| Ok(libreria.covers.clone())).map_err(errore)?;
            let [r, g, b] = copertine
                .tinta(&impronta)
                .ok_or_else(|| errore(rifiutata("copertina", "la copertina non ha una tinta")))?;
            rymembra_skin::values::Rgba { r, g, b, a: 1.0 }
        }
        (None, None) => return Err(errore(rifiutata("tinta", "manca da dove partire"))),
    };
    Ok(rymembra_skin::dinamico::tavolozza_da(tinta, scura)
        .into_iter()
        .map(|(token, valore)| VoceTavolozzaIpc { token, valore })
        .collect())
}

// ── Le istantanee ───────────────────────────────────────────────────────────

/// Perché un'istantanea è stata presa.
///
/// «Senza nomi da inventare»: un'istantanea non si battezza, si prende. Il nome
/// che porta dice **perché**, che è l'unica cosa che chi scorre l'elenco vuole
/// sapere — e siccome è un insieme chiuso, è un enum e non una stringa: così a
/// respingere quel che non è previsto è il confine dell'IPC, e non un controllo
/// che qualcuno prima o poi dimentica di scrivere. È la prima regola dello
/// Studio applicata allo Studio stesso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Causa {
    /// La prima apertura di una skin derivata da un'altra.
    Derivata,
    /// Prima di «Salva e usa».
    Salvata,
    /// Prima di un'esportazione.
    Esportata,
    /// Chiesta a mano.
    Manuale,
    /// Il testo del formato 1, prima della migrazione.
    Migrata,
}

impl Causa {
    const fn nome(self) -> &'static str {
        match self {
            Self::Derivata => "derivata",
            Self::Salvata => "salvata",
            Self::Esportata => "esportata",
            Self::Manuale => "manuale",
            Self::Migrata => "migrata",
        }
    }

    fn da(nome: &str) -> Option<Self> {
        [
            Self::Derivata,
            Self::Salvata,
            Self::Esportata,
            Self::Manuale,
            Self::Migrata,
        ]
        .into_iter()
        .find(|causa| causa.nome() == nome)
    }
}

/// Quante istantanee si tengono per bozza, oltre a quella della migrazione.
///
/// Oltre, la cartella di una bozza cresce senza fine e nessuno se ne accorge — e
/// le venti più recenti sono già più di quante se ne guardino.
const QUANTE_ISTANTANEE: usize = 20;

/// Un'istantanea, come la mostra l'elenco.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IstantaneaIpc {
    /// Millisecondi dall'epoca. È anche la sua identità.
    pub quando: u64,
    /// Il nome di una [`Causa`] (`Causa::nome`): `derivata`, `salvata`,
    /// `esportata`, `manuale` o `migrata`.
    pub causa: &'static str,
    /// Quante parti ridisegnava.
    pub parti: usize,
    /// Quanti token dichiarava.
    pub token: usize,
}

/// Il nome di un'istantanea porta tutto quel che l'elenco deve mostrare.
///
/// `<quando>-<causa>-<parti>-<token>.json`, e il contenuto è il manifest e
/// basta. Nessun file d'indice accanto: un indice è una seconda verità che prima
/// o poi non combacia con la cartella, e qui il costo di evitarlo è dividere una
/// stringa in quattro pezzi. Il contenuto resta un `skin.json` vero, quindi si
/// può copiare fuori e aprire senza passare da qui.
fn istantanea_da(nome: &str) -> Option<IstantaneaIpc> {
    let radice = nome.strip_suffix(".json")?;
    let mut pezzi = radice.split('-');
    let quando = pezzi.next()?.parse().ok()?;
    let causa = Causa::da(pezzi.next()?)?.nome();
    let parti = pezzi.next()?.parse().ok()?;
    let token = pezzi.next()?.parse().ok()?;
    if pezzi.next().is_some() {
        return None;
    }
    Some(IstantaneaIpc {
        quando,
        causa,
        parti,
        token,
    })
}

/// Il nome del file di un'istantanea: l'inverso esatto di `istantanea_da`.
fn nome_istantanea(istantanea: &IstantaneaIpc) -> String {
    format!(
        "{}-{}-{}-{}.json",
        istantanea.quando, istantanea.causa, istantanea.parti, istantanea.token
    )
}

/// Le istantanee di una bozza, la più recente per prima.
fn elenco_istantanee(cartella: &Path) -> Vec<IstantaneaIpc> {
    let Ok(voci) = std::fs::read_dir(cartella) else {
        // Nessuna cartella vuol dire nessuna istantanea, non un guasto: è il
        // caso di ogni bozza prima della prima prova.
        return Vec::new();
    };
    let mut elenco: Vec<IstantaneaIpc> = voci
        .flatten()
        .filter_map(|voce| {
            let nome = voce.file_name();
            istantanea_da(nome.to_str()?)
        })
        .collect();
    // Dalla più recente: `std::cmp::Reverse` invece di scambiare i due lati del
    // confronto, che è la forma che clippy chiede e che si legge meglio.
    elenco.sort_unstable_by_key(|istantanea| std::cmp::Reverse(istantanea.quando));
    elenco
}

/// Le istantanee di una bozza.
#[tauri::command(async)]
pub fn studio_istantanee(stato: State<'_, Stato>, id: String) -> Esito<Vec<IstantaneaIpc>> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        Ok(elenco_istantanee(&istantanee_di(dati, &id)))
    })
    .map_err(errore)
}

/// Prende un'istantanea, e pota le più vecchie.
///
/// Sta fuori dal comando perché il comando è un guscio: prende il lucchetto,
/// compone un percorso e chiama questa. Provare l'una senza costruire l'altro è
/// la stessa divisione che `skin.rs` fa con `installa`.
fn prendi_istantanea(cartella: &Path, sorgente: &str, causa: Causa) -> Result<(), AppError> {
    {
        std::fs::create_dir_all(cartella)
            .map_err(|err| rymembra_app::files::io_error(&cartella.display().to_string(), &err))?;

        // I conteggi si prendono adesso e non a ogni lettura dell'elenco: sono
        // ciò che il documento **era**, e riparsare venti manifest per riempire
        // una colonna sarebbe lavoro rifatto a ogni apertura. Un documento a
        // metà di una parentesi si conta zero e zero — è una bozza salvata
        // apposta anche quando non è valida, e l'elenco lo dice invece di
        // rifiutarsi di prenderla.
        let (parti, token) = rymembra_skin::parse_skin_json(sorgente).map_or((0, 0), |documento| {
            (documento.parts.len(), documento.tokens.iter().count())
        });

        // Due istantanee nello stesso millisecondo si sovrascriverebbero: il
        // salvataggio e l'esportazione possono capitare a un battito di
        // distanza. Si cerca il primo istante libero invece di perderne una.
        let mut presa = IstantaneaIpc {
            quando: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |da_allora| {
                    u64::try_from(da_allora.as_millis()).unwrap_or(u64::MAX)
                }),
            causa: causa.nome(),
            parti,
            token,
        };
        while cartella.join(nome_istantanea(&presa)).exists() {
            presa.quando = presa.quando.saturating_add(1);
        }

        scrivi_atomico(&cartella.join(nome_istantanea(&presa)), sorgente.as_bytes())?;

        // Quella della migrazione non gira con le altre: è l'unica copia del
        // testo del formato 1, e una ventina di salvataggi la spingevano fuori
        // proprio quando ci si accorge che la migrazione ha tradito qualcosa.
        // Se ne prende una ogni volta che un testo del formato 1 si apre (vedi
        // `apri_documento`), cioè di norma una per skin: tenerle fuori dal
        // conto non fa crescere la cartella senza fine.
        for vecchia in elenco_istantanee(cartella)
            .into_iter()
            .filter(|istantanea| istantanea.causa != Causa::Migrata.nome())
            .skip(QUANTE_ISTANTANEE)
        {
            // Un file che non si cancella non è un motivo per far fallire il
            // salvataggio: il lavoro è già su disco, ed è quello che conta.
            drop(std::fs::remove_file(
                cartella.join(nome_istantanea(&vecchia)),
            ));
        }
        Ok(())
    }
}

/// Prende un'istantanea del documento com'è adesso.
#[tauri::command(async)]
pub fn studio_istantanea(
    stato: State<'_, Stato>,
    id: String,
    sorgente: String,
    causa: Causa,
) -> Esito<()> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        prendi_istantanea(&istantanee_di(dati, &id), &sorgente, causa)
    })
    .map_err(errore)
}

/// Il testo di un'istantanea.
///
/// `quando` è un numero, non un pezzo di percorso: il file si trova scorrendo la
/// cartella e confrontando, quindi non c'è nessun nome che arriva da fuori e
/// finisce dentro un `join`.
fn ripristina_istantanea(cartella: &Path, quando: u64) -> Result<String, AppError> {
    let trovata = elenco_istantanee(cartella)
        .into_iter()
        .find(|istantanea| istantanea.quando == quando)
        .ok_or_else(|| non_trovata(&quando.to_string()))?;
    let file = cartella.join(nome_istantanea(&trovata));
    std::fs::read_to_string(&file)
        .map_err(|err| rymembra_app::files::io_error(&file.display().to_string(), &err))
}

/// Riporta il documento a com'era in un'istantanea.
#[tauri::command(async)]
pub fn studio_ripristina(stato: State<'_, Stato>, id: String, quando: u64) -> Esito<String> {
    con_le_bozze(&stato, |dati| {
        if !id_sicuro(&id) {
            return Err(non_trovata(&id));
        }
        ripristina_istantanea(&istantanee_di(dati, &id), quando)
    })
    .map_err(errore)
}

#[cfg(test)]
mod tests;
