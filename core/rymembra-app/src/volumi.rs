//! Dove sta un percorso: su questo computer, o dall'altra parte di un cavo.
//!
//! # La domanda che [`crate::files::percorso_di_rete`] non sa fare
//!
//! Quella funzione riconosce un percorso di rete **dalla forma**: `\\nas\musica`
//! lo è, `Z:\Musica` no. Per scegliere una frase basta, e lo dice lei stessa;
//! per decidere se un file va nel Cestino no. Una lettera mappata su una share è
//! esattamente il caso in cui la forma mente, ed è il caso più comune di chi
//! tiene la musica su un NAS: `net use Z: \\nas\musica` e da lì in poi, per ogni
//! programma che guarda la stringa, quella musica è su un disco locale.
//!
//! Il modo di chiederlo a Windows senza `unsafe` — che la radice del workspace
//! vieta — è [`std::fs::canonicalize`]. Su Windows la funzione apre il percorso e
//! chiede al sistema il nome finale del file, e il sistema risponde con il nome
//! **vero**: per una lettera mappata `\\?\UNC\nas\musica\…`, per un disco locale
//! `\\?\C:\…`, per una lettera fatta con `subst` il percorso locale che le sta
//! sotto. La forma, a quel punto, dice la verità, e
//! [`crate::files::percorso_di_rete`] la riconosce.
//!
//! # Il prezzo, e la scadenza che lo paga
//!
//! `canonicalize` **tocca il disco**, e su una share morta è la chiamata che non
//! torna per i quaranta secondi di SMB. Per questo non si chiama mai nuda: una
//! risposta sola passa da [`crate::scadenza::con_scadenza`], molte da un gruppo
//! di fili che si smette di ascoltare quando tace troppo a lungo. Una risposta
//! che non arriva in tempo è [`Collocazione::Ignota`], e non è né «locale» né
//! «di rete»: è la terza risposta, e ogni chiamante decide da sé da che parte
//! farla cadere. Chi cancella la fa cadere dalla parte della rete, perché il
//! Cestino è la promessa che non si può fare a vuoto.
//!
//! # Che cosa si risolve
//!
//! Non il file, ma la **cartella che lo contiene**, e se nemmeno quella risponde
//! la radice dell'unità. Due ragioni: un album è una cartella e venti file, e
//! risolverla una volta costa un ventesimo; e un file che non c'è più — il caso
//! di chi cancella un brano già sparito — ha comunque una cartella o un'unità a
//! cui chiedere dove stanno.
//!
//! # L'esistenza, che ha la stessa malattia
//!
//! La stessa scadenza serve a una domanda più semplice, «questo file c'è?», che
//! il profilo e il trasloco di una radice fanno per migliaia di percorsi. Con
//! `Path::exists` una share morta risponde «no» — cioè «il file non c'è» — dopo
//! quaranta secondi per file. Qui risponde [`Esistenza::Ignota`], e subito: non
//! sapere non è la stessa cosa di sapere che manca.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::files::{percorso_di_rete, percorso_lungo};

/// Dove sta un percorso, per quel che il disco ha voluto dire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Collocazione {
    /// Su un disco di questo computer.
    Locale,
    /// Su una condivisione di rete: un UNC, o una lettera mappata su uno.
    DiRete,
    /// Non si è saputo: il disco non ha risposto in tempo, o il percorso non
    /// porta a niente che si possa risolvere.
    Ignota,
}

impl Collocazione {
    /// Il Cestino di Windows lo può prendere, con la promessa di ridarlo?
    ///
    /// Solo sul locale. [`Self::Ignota`] risponde di no, e non per pessimismo:
    /// mandare al Cestino un file di rete non fallisce, lo **cancella per
    /// sempre** riportando successo, e chi non sa dove sta un file non può
    /// promettere di ritrovarlo.
    #[must_use]
    pub const fn ha_un_cestino(self) -> bool {
        matches!(self, Self::Locale)
    }
}

/// Un file c'è?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Esistenza {
    /// C'è.
    Presente,
    /// Il sistema ha risposto «non c'è», e non per un guasto di rete.
    Assente,
    /// Non si è saputo: la share non risponde, l'accesso è negato, il tempo è
    /// scaduto. **Non** vuol dire «non c'è».
    Ignota,
}

/// Quanto si aspetta una risoluzione prima di dichiararla ignota.
///
/// Cinque secondi, come l'apertura di un brano: un disco locale risponde in
/// microsecondi e una share viva in millisecondi, una share morta ci mette una
/// quarantina di secondi a rinunciare da sola. Il numero deve solo stare largo
/// sul caso buono e stretto sul caso cattivo.
pub const SCADENZA_RISOLUZIONE: Duration = Duration::from_secs(5);

/// Quanti fili lavorano insieme su un elenco di percorsi.
///
/// Otto: SMB serve bene richieste in parallelo, e su una share lenta è la
/// latenza di ogni giro — non la banda — a fare il tempo totale. Di più non
/// accorcia niente su un disco locale e moltiplica i fili che una share morta
/// lascia appesi.
const PARALLELISMO: usize = 8;

/// Il prefisso verbatim tolto, per confrontare e per mostrare.
///
/// `\\?\UNC\nas\musica` → `\\nas\musica`, `\\?\C:\Musica` → `C:\Musica`. È la
/// forma che `canonicalize` restituisce su Windows, ed è una forma che nessuno
/// scrive a mano: confrontarla così com'è con `\\nas\musica` direbbe «due posti
/// diversi» per lo stesso posto.
#[must_use]
pub fn senza_verbatim(testo: &str) -> String {
    if let Some(resto) = testo
        .get(..8)
        .filter(|testa| testa.eq_ignore_ascii_case(r"\\?\UNC\"))
        .and_then(|_| testo.get(8..))
    {
        return format!(r"\\{resto}");
    }
    testo.strip_prefix(r"\\?\").unwrap_or(testo).to_owned()
}

/// La collocazione di un percorso che il sistema ha già risolto.
///
/// Pura: è la lettura dell'uscita di `canonicalize`, e così si prova senza
/// una share. Un UNC — verbatim o no — è di rete; tutto il resto è locale.
#[must_use]
pub fn da_canonico(canonico: &Path) -> Collocazione {
    if percorso_di_rete(canonico) {
        Collocazione::DiRete
    } else {
        Collocazione::Locale
    }
}

/// La collocazione che si legge dalla sola forma, senza toccare il disco.
///
/// `Some` per un UNC, che è di rete qualunque cosa ne dica il disco, e per un
/// percorso del cloud (`nuvola://…`), che il disco non conosce affatto. Il
/// secondo vale «di rete» per le stesse due decisioni: niente Cestino — un
/// brano del servizio non si cancella da qui — e niente sorveglianza col
/// `notify`. Chiederlo al sistema sarebbe peggio che inutile: `nuvola:` non è
/// una lettera di unità, il percorso risulterebbe relativo, e la cartella di
/// lavoro di Rymembra — che sta sul disco locale — lo farebbe sembrare locale.
///
/// Tutto il resto — una lettera, un percorso relativo — va chiesto al sistema,
/// perché la forma non basta a dirlo.
#[must_use]
pub fn dalla_forma(percorso: &Path) -> Option<Collocazione> {
    let del_cloud = percorso
        .to_str()
        .is_some_and(rymembra_domain::nuvola::e_nuvola);
    (del_cloud || percorso_di_rete(percorso)).then_some(Collocazione::DiRete)
}

/// Il nome vero di un percorso del cloud: lui stesso.
///
/// `canonicalize` su `nuvola://…` chiederebbe a Windows di una cartella
/// relativa `nuvola:` nella cartella di lavoro, e risponderebbe «non c'è» — o
/// peggio, se qualcuno l'avesse creata. Il nome di una cartella del cloud è già
/// quello vero: il conto è un'impronta dell'account, e i segmenti vengono dallo
/// specchio.
fn del_cloud(percorso: &str) -> Option<String> {
    rymembra_domain::nuvola::e_nuvola(percorso).then(|| percorso.to_owned())
}

/// Le cartelle a cui chiedere, in ordine: quella che contiene il file, e poi la
/// radice della sua unità.
///
/// Un percorso relativo si risolve rispetto alla cartella di lavoro, che è quel
/// che farebbe il sistema aprendolo: la sua «radice» è quindi `.`.
///
/// # La radice di un'unità
///
/// `Z:\` non ha una madre, e qui si chiedeva allora a `.` — cioè alla cartella
/// di lavoro di Rymembra, che sta sul disco locale. Una lettera mappata su un NAS
/// usata **intera** come cartella sorvegliata usciva così «locale», subito e
/// senza aver chiesto niente alla rete: col modo di serie la share riceveva una
/// sorveglianza viva invece di essere lasciata stare, e il filo della
/// sorveglianza restava appeso un minuto ogni minuto finché il NAS non c'era.
/// Un percorso senza madre ma con una radice **è** la sua radice, ed è lei che
/// va interrogata; `.` resta solo per un nome nudo, che una radice non ce l'ha.
fn da_interrogare(percorso: &Path) -> Vec<PathBuf> {
    let mut radice = PathBuf::new();
    for pezzo in percorso.components() {
        match pezzo {
            Component::Prefix(_) | Component::RootDir => radice.push(pezzo.as_os_str()),
            _ => break,
        }
    }
    let mut cartelle = Vec::with_capacity(2);
    match percorso.parent() {
        Some(genitore) if !genitore.as_os_str().is_empty() => {
            cartelle.push(genitore.to_path_buf());
        }
        _ if !radice.as_os_str().is_empty() => cartelle.push(radice.clone()),
        _ => cartelle.push(PathBuf::from(".")),
    }
    if radice.as_os_str().is_empty() {
        radice.push(".");
    }
    if cartelle.first() != Some(&radice) {
        cartelle.push(radice);
    }
    cartelle
}

/// La collocazione chiesta al disco, **senza scadenza**: chi la chiama la tiene
/// su un filo che si può abbandonare.
fn chiedi_al_disco(percorso: &Path) -> Collocazione {
    if let Some(dalla_forma) = dalla_forma(percorso) {
        return dalla_forma;
    }
    for cartella in da_interrogare(percorso) {
        if let Ok(canonico) = std::fs::canonicalize(&cartella) {
            return da_canonico(&canonico);
        }
    }
    Collocazione::Ignota
}

/// Dove sta un percorso solo, con la scadenza addosso.
///
/// Per molti percorsi insieme c'è [`collocazioni`], che risolve ogni cartella
/// una volta sola e lavora in parallelo.
#[must_use]
pub fn collocazione(percorso: &str, scadenza: Duration) -> Collocazione {
    if let Some(dalla_forma) = dalla_forma(Path::new(percorso)) {
        return dalla_forma;
    }
    let posseduto = PathBuf::from(percorso);
    crate::scadenza::con_scadenza("volumi", scadenza, move || chiedi_al_disco(&posseduto))
        .unwrap_or(Collocazione::Ignota)
}

/// Dove stanno molti percorsi, nello stesso ordine.
///
/// Si risolve una volta per **cartella**, non per file: la selezione tipica è un
/// album, e venti file nella stessa cartella stanno per forza nello stesso
/// posto. I percorsi che si riconoscono dalla forma non toccano il disco.
///
/// `scadenza` è il silenzio massimo fra due risposte, non la durata totale:
/// vedi [`in_parallelo`].
#[must_use]
pub fn collocazioni(percorsi: &[String], scadenza: Duration) -> Vec<Collocazione> {
    // La cartella di ciascuno, o `None` se la forma ha già risposto.
    let chiavi: Vec<Option<PathBuf>> = percorsi
        .iter()
        .map(|p| {
            let percorso = Path::new(p);
            if dalla_forma(percorso).is_some() {
                return None;
            }
            Some(
                percorso
                    .parent()
                    .filter(|g| !g.as_os_str().is_empty())
                    .map_or_else(|| percorso.to_path_buf(), Path::to_path_buf),
            )
        })
        .collect();

    let mut distinte: Vec<PathBuf> = Vec::new();
    for chiave in chiavi.iter().flatten() {
        if !distinte.contains(chiave) {
            distinte.push(chiave.clone());
        }
    }
    // Si chiede per la cartella come se fosse un file al suo interno: così
    // `chiedi_al_disco` interroga proprio lei, e dopo la radice della sua unità.
    let risposte = in_parallelo(distinte.clone(), scadenza, |cartella: &PathBuf| {
        chiedi_al_disco(&cartella.join("_"))
    });

    chiavi
        .iter()
        .map(|chiave| match chiave {
            None => Collocazione::DiRete,
            // Una cartella che non ha risposto in tempo è «non si sa», che è
            // anche la risposta prudente per il caso — impossibile — di una
            // chiave che non sta fra le distinte.
            Some(cartella) => distinte
                .iter()
                .position(|d| d == cartella)
                .and_then(|i| risposte.get(i).copied().flatten())
                .unwrap_or(Collocazione::Ignota),
        })
        .collect()
}

/// Il nome vero di un percorso, senza prefisso verbatim, se il sistema l'ha dato
/// in tempo.
///
/// È la risposta a «questa cartella e quella sono lo stesso posto?», che la
/// forma non sa dare: `Z:\` e `\\nas\musica` possono esserlo, e lo si scopre
/// solo risolvendoli tutti e due. `None` se il percorso non c'è, se il sistema
/// rifiuta, o se la scadenza passa.
#[must_use]
pub fn risolvi(percorso: &str, scadenza: Duration) -> Option<String> {
    if let Some(com_e) = del_cloud(percorso) {
        return Some(com_e);
    }
    let posseduto = percorso_lungo(Path::new(percorso));
    crate::scadenza::con_scadenza("volumi-risolvi", scadenza, move || {
        std::fs::canonicalize(&posseduto).ok()
    })
    .flatten()
    .map(|canonico| senza_verbatim(&canonico.to_string_lossy()))
}

/// [`risolvi`] per molti percorsi insieme, nello stesso ordine.
///
/// Le cartelle del cloud tornano come sono, senza chiedere niente al disco:
/// vedi [`del_cloud`].
#[must_use]
pub fn risolvi_tutti(percorsi: &[String], scadenza: Duration) -> Vec<Option<String>> {
    in_parallelo(
        percorsi.to_vec(),
        scadenza,
        |percorso: &String| -> Option<String> {
            match rymembra_domain::nuvola::Radice::da(percorso) {
                rymembra_domain::nuvola::Radice::Nuvola { .. } => del_cloud(percorso),
                rymembra_domain::nuvola::Radice::Locale(locale) => {
                    std::fs::canonicalize(percorso_lungo(Path::new(locale)))
                        .ok()
                        .map(|canonico| senza_verbatim(&canonico.to_string_lossy()))
                }
            }
        },
    )
    .into_iter()
    .map(Option::flatten)
    .collect()
}

/// Un file c'è? Chiesto al disco **senza scadenza**.
///
/// La distinzione che conta è fra `NotFound` e tutto il resto. Ma prima di
/// guardare il genere si guarda se il guasto è di rete: Windows racconta
/// `ERROR_BAD_NETPATH` e `ERROR_BAD_NET_NAME` — la share che non c'è — come
/// `NotFound`, e senza quel controllo una share spenta direbbe «file mancante»
/// per ogni brano che contiene.
///
/// Un percorso del cloud è [`Esistenza::Ignota`], senza toccare il disco: se
/// c'è lo sa lo specchio del servizio, non il filesystem, e il «non c'è» che il
/// filesystem darebbe farebbe proporre di rimappare una cartella che c'è.
#[must_use]
pub fn esistenza(percorso: &str) -> Esistenza {
    if rymembra_domain::nuvola::e_nuvola(percorso) {
        return Esistenza::Ignota;
    }
    match std::fs::metadata(percorso_lungo(Path::new(percorso))) {
        Ok(_) => Esistenza::Presente,
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && !rymembra_domain::errors::rete::e_di_rete(&err) =>
        {
            Esistenza::Assente
        }
        Err(_) => Esistenza::Ignota,
    }
}

/// [`esistenza`] per un percorso solo, con la scadenza addosso.
#[must_use]
pub fn esistenza_entro(percorso: &str, scadenza: Duration) -> Esistenza {
    let posseduto = percorso.to_owned();
    crate::scadenza::con_scadenza("volumi-esiste", scadenza, move || esistenza(&posseduto))
        .unwrap_or(Esistenza::Ignota)
}

/// [`esistenza`] per molti percorsi, in parallelo, nello stesso ordine.
///
/// Quel che non risponde prima che il gruppo taccia per `silenzio` è
/// [`Esistenza::Ignota`]. Vedi [`in_parallelo`] per cosa vuol dire.
#[must_use]
pub fn esistenze(percorsi: Vec<String>, silenzio: Duration) -> Vec<Esistenza> {
    in_parallelo(percorsi, silenzio, |percorso: &String| esistenza(percorso))
        .into_iter()
        .map(|esito| esito.unwrap_or(Esistenza::Ignota))
        .collect()
}

/// Esegue `lavoro` su ogni elemento con al più [`PARALLELISMO`] fili, e smette
/// di aspettare quando per `silenzio` non arriva più niente.
///
/// # Perché il silenzio e non una scadenza per elemento
///
/// Perché una scadenza per elemento, con un gruppo di fili che si passano gli
/// elementi, vorrebbe sapere quale filo sta lavorando su quale elemento e da
/// quando — un registro in più per rispondere a una domanda che il silenzio
/// risponde da solo. Su una share viva le risposte arrivano di continuo, anche
/// se una singola resta indietro; su una share morta **tutti** i fili si
/// fermano insieme e il gruppo tace. Tacere per `silenzio` è quindi la stessa
/// cosa di «ogni elemento ha avuto la sua scadenza»: un file che si appende
/// resta indietro da solo, e gli altri fili continuano a prendere lavoro dalla
/// coda comune finché ce n'è.
///
/// Gli elementi che non hanno risposto sono `None`. I fili rimasti dentro una
/// chiamata che non torna finiscono da soli quando torna: la loro `send`
/// trova il canale chiuso, e il ciclo esce. È il patto di
/// [`crate::scadenza`], applicato a un gruppo.
fn in_parallelo<T, R>(elementi: Vec<T>, silenzio: Duration, lavoro: fn(&T) -> R) -> Vec<Option<R>>
where
    T: Send + Sync + 'static,
    R: Send + 'static,
{
    let quanti = elementi.len();
    let mut esiti: Vec<Option<R>> = (0..quanti).map(|_| None).collect();
    if quanti == 0 {
        return esiti;
    }
    let elementi = Arc::new(elementi);
    let prossimo = Arc::new(AtomicUsize::new(0));
    let (manda, ricevi) = mpsc::channel::<(usize, R)>();
    for numero in 0..PARALLELISMO.min(quanti) {
        let elementi = Arc::clone(&elementi);
        let prossimo = Arc::clone(&prossimo);
        let manda = manda.clone();
        // Un filo che non parte non è un guasto: quelli partiti prendono il
        // suo lavoro dalla coda comune. Se non ne parte nessuno, il `recv`
        // qui sotto trova il canale chiuso e tutto resta «non si sa».
        let _ = std::thread::Builder::new()
            .name(format!("rymembra-volumi-{numero}"))
            .spawn(move || {
                loop {
                    let indice = prossimo.fetch_add(1, Ordering::Relaxed);
                    let Some(elemento) = elementi.get(indice) else {
                        break;
                    };
                    if manda.send((indice, lavoro(elemento))).is_err() {
                        break;
                    }
                }
            });
    }
    // Il mittente di qui va lasciato cadere: finché c'è, il canale non si
    // chiude mai, e un gruppo di fili tutti finiti sembrerebbe un gruppo che
    // tace.
    drop(manda);

    let mut arrivati = 0;
    while arrivati < quanti {
        match ricevi.recv_timeout(silenzio) {
            Ok((indice, esito)) => {
                if let Some(posto) = esiti.get_mut(indice) {
                    *posto = Some(esito);
                    arrivati += 1;
                }
            }
            // Scaduto, o tutti i fili finiti: in tutti e due i casi quel che
            // non è arrivato non arriverà in tempo.
            Err(_) => break,
        }
    }
    esiti
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_prefisso_verbatim_si_toglie() {
        assert_eq!(senza_verbatim(r"\\?\UNC\nas\musica\a"), r"\\nas\musica\a");
        assert_eq!(senza_verbatim(r"\\?\unc\nas\musica"), r"\\nas\musica");
        assert_eq!(senza_verbatim(r"\\?\C:\Musica"), r"C:\Musica");
        // Quel che non è verbatim resta com'è.
        assert_eq!(senza_verbatim(r"\\nas\musica"), r"\\nas\musica");
        assert_eq!(senza_verbatim(r"C:\Musica"), r"C:\Musica");
        assert_eq!(senza_verbatim("Musica/a.flac"), "Musica/a.flac");
    }

    #[test]
    fn la_risposta_del_sistema_si_legge_dalla_forma() {
        // Quel che `canonicalize` restituisce per una lettera mappata e per un
        // disco locale: è la lettura che decide se un file va nel Cestino.
        assert_eq!(
            da_canonico(Path::new(r"\\?\UNC\nas\musica\Album")),
            Collocazione::DiRete
        );
        assert_eq!(
            da_canonico(Path::new(r"\\?\C:\Users\tizio\Musica")),
            Collocazione::Locale
        );
    }

    #[test]
    fn un_unc_e_di_rete_senza_chiedere_al_disco() {
        // Le due forme, verbatim e no: si riconoscono dalla stringa, e non si
        // tocca il disco — un server che non esiste non fa aspettare niente.
        let prima = std::time::Instant::now();
        let dove = collocazioni(
            &[
                r"\\server-che-non-esiste\musica\a.flac".to_owned(),
                r"\\?\UNC\server-che-non-esiste\musica\b.flac".to_owned(),
            ],
            SCADENZA_RISOLUZIONE,
        );
        assert_eq!(dove, vec![Collocazione::DiRete, Collocazione::DiRete]);
        assert!(
            prima.elapsed() < Duration::from_secs(1),
            "la forma basta, e non deve costare una risoluzione"
        );
        assert_eq!(
            collocazione(r"\\server-che-non-esiste\m\a.flac", SCADENZA_RISOLUZIONE),
            Collocazione::DiRete
        );
    }

    #[test]
    fn un_file_locale_e_locale_anche_se_non_c_e_piu() {
        let cartella = tempfile::tempdir().expect("cartella temporanea");
        let vivo = cartella.path().join("a.flac");
        std::fs::write(&vivo, b"x").expect("file");
        // Il file sparito: si chiede alla cartella, che c'è ancora.
        let sparito = cartella.path().join("sparito.flac");
        // E quello la cui cartella è sparita: si chiede alla radice del disco.
        let orfano = cartella.path().join("non").join("c-e").join("b.flac");

        let percorsi: Vec<String> = [&vivo, &sparito, &orfano]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let dove = collocazioni(&percorsi, SCADENZA_RISOLUZIONE);
        assert_eq!(
            dove,
            vec![
                Collocazione::Locale,
                Collocazione::Locale,
                Collocazione::Locale
            ]
        );
        assert!(dove.iter().all(|c| c.ha_un_cestino()));
    }

    #[test]
    fn un_percorso_relativo_si_risolve_dalla_cartella_di_lavoro() {
        // Le prove girano nella cartella della cassa, che sta su un disco
        // locale: un relativo — esistente o no — sta dove sta lei.
        assert_eq!(
            collocazione("Cargo.toml", SCADENZA_RISOLUZIONE),
            Collocazione::Locale
        );
        assert_eq!(
            collocazione("non-esiste/a.flac", SCADENZA_RISOLUZIONE),
            Collocazione::Locale
        );
    }

    #[test]
    fn chi_non_si_sa_non_ha_un_cestino() {
        assert!(Collocazione::Locale.ha_un_cestino());
        assert!(!Collocazione::DiRete.ha_un_cestino());
        // La scelta che conta: un file di cui non si sa dove sta non va nel
        // Cestino, perché su una share il Cestino lo cancellerebbe per sempre.
        assert!(!Collocazione::Ignota.ha_un_cestino());
    }

    #[test]
    fn le_cartelle_da_interrogare_sono_la_madre_e_la_radice() {
        if cfg!(windows) {
            assert_eq!(
                da_interrogare(Path::new(r"Z:\Musica\Album\a.flac")),
                vec![PathBuf::from(r"Z:\Musica\Album"), PathBuf::from(r"Z:\")]
            );
            // Un file in radice: la madre **è** la radice, e non si chiede due
            // volte la stessa cosa.
            assert_eq!(
                da_interrogare(Path::new(r"Z:\a.flac")),
                vec![PathBuf::from(r"Z:\")]
            );
            // La radice di un'unità usata intera come cartella sorvegliata: si
            // chiede a lei, e non alla cartella di lavoro di Rymembra — che è
            // locale, e diceva «locale» anche di una lettera mappata su un NAS.
            assert_eq!(
                da_interrogare(Path::new(r"Z:\")),
                vec![PathBuf::from(r"Z:\")]
            );
            assert_eq!(
                da_interrogare(Path::new(r"\\nas\musica\")),
                vec![PathBuf::from(r"\\nas\musica\")]
            );
        }
        assert_eq!(
            da_interrogare(Path::new("a.flac")),
            vec![PathBuf::from(".")]
        );
        assert_eq!(
            da_interrogare(Path::new("Musica/a.flac")),
            vec![PathBuf::from("Musica"), PathBuf::from(".")]
        );
    }

    #[test]
    fn l_esistenza_distingue_il_file_che_manca() {
        let cartella = tempfile::tempdir().expect("cartella temporanea");
        let vivo = cartella.path().join("a.flac");
        std::fs::write(&vivo, b"x").expect("file");
        let vivo = vivo.to_string_lossy().into_owned();
        let sparito = cartella
            .path()
            .join("sparito.flac")
            .to_string_lossy()
            .into_owned();

        assert_eq!(esistenza(&vivo), Esistenza::Presente);
        assert_eq!(esistenza(&sparito), Esistenza::Assente);
        assert_eq!(
            esistenza_entro(&vivo, SCADENZA_RISOLUZIONE),
            Esistenza::Presente
        );
        assert_eq!(
            esistenze(vec![vivo, sparito], SCADENZA_RISOLUZIONE),
            vec![Esistenza::Presente, Esistenza::Assente]
        );
    }

    #[cfg(windows)]
    #[test]
    fn una_share_che_non_c_e_non_e_un_file_che_manca() {
        // `ERROR_BAD_NETPATH` e compagni arrivano da Windows come `NotFound`: senza
        // il controllo dei codici di rete, un NAS spento direbbe «mancante»
        // per ogni brano che contiene, e il profilo li conterebbe fra gli
        // irrintracciabili. Qui la risposta dev'essere «non si sa».
        //
        // Un server che non risolve: la risposta arriva (dopo il giro del
        // nome) come guasto di rete, oppure la scadenza la ferma. In tutti e
        // due i casi, mai «assente».
        let esito = esistenza_entro(
            r"\\rymembra-server-che-non-esiste.invalid\musica\a.flac",
            Duration::from_secs(3),
        );
        assert_ne!(esito, Esistenza::Assente);
    }

    #[test]
    fn un_lavoro_che_si_appende_diventa_ignoto_e_gli_altri_arrivano() {
        // Il patto del gruppo: un elemento che non torna non ferma quelli che
        // tornano, e il gruppo non aspetta la sua fine.
        fn lento_il_tre(n: &u64) -> u64 {
            if *n == 3 {
                std::thread::sleep(Duration::from_millis(600));
            }
            n * 10
        }
        let prima = std::time::Instant::now();
        let esiti = in_parallelo((0..20).collect(), Duration::from_millis(150), lento_il_tre);
        assert!(
            prima.elapsed() < Duration::from_millis(550),
            "ha aspettato l'elemento appeso: {:?}",
            prima.elapsed()
        );
        assert_eq!(esiti.get(3).copied().flatten(), None, "l'appeso è ignoto");
        for (n, esito) in (0..20_u64).zip(&esiti) {
            if n != 3 {
                assert_eq!(*esito, Some(n * 10), "l'elemento {n}");
            }
        }
        // Il filo appeso finisce da sé e trova il canale chiuso.
        std::thread::sleep(Duration::from_millis(700));
    }

    #[test]
    fn un_elenco_vuoto_non_assume_nessuno() {
        assert!(esistenze(Vec::new(), SCADENZA_RISOLUZIONE).is_empty());
        assert!(collocazioni(&[], SCADENZA_RISOLUZIONE).is_empty());
    }

    #[test]
    fn le_cartelle_del_cloud_tornano_come_sono_senza_chiedere_al_disco() {
        // Il nome vero di una cartella del cloud è lei stessa, accanto a una
        // del disco che invece si risolve come sempre.
        let cartella = tempfile::tempdir().expect("cartella temporanea");
        let nuvola = "nuvola://onedrive-abcdefgh/Musica";
        let locale = cartella.path().display().to_string();
        let risolte = risolvi_tutti(&[nuvola.to_owned(), locale.clone()], SCADENZA_RISOLUZIONE);
        assert_eq!(risolte.first().cloned().flatten().as_deref(), Some(nuvola));
        assert!(risolte.get(1).cloned().flatten().is_some(), "il disco sì");
        assert_eq!(
            risolvi(nuvola, SCADENZA_RISOLUZIONE).as_deref(),
            Some(nuvola)
        );
        // «Non si sa» e non «non c'è»: non si propone di rimapparla.
        assert_eq!(esistenza(nuvola), Esistenza::Ignota);
        assert_eq!(
            esistenze(vec![nuvola.to_owned()], SCADENZA_RISOLUZIONE),
            [Esistenza::Ignota]
        );
        // E non è un disco di questo computer: niente Cestino.
        assert_eq!(dalla_forma(Path::new(nuvola)), Some(Collocazione::DiRete));
        assert_eq!(
            collocazioni(&[format!("{nuvola}/a.flac")], SCADENZA_RISOLUZIONE),
            [Collocazione::DiRete]
        );
        assert!(!collocazione(nuvola, SCADENZA_RISOLUZIONE).ha_un_cestino());
    }
}
