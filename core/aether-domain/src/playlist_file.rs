//! I file di playlist: M3U, PLS, XSPF. Leggerli e scriverli, senza toccare
//! niente.
//!
//! # Perché sta nel dominio
//!
//! Perché un file di playlist non è un file: è un **testo**. Non c'è niente
//! qui dentro che abbia bisogno di un disco, di un orologio o di un database —
//! ci sono tre grammatiche, e le grammatiche si provano con delle stringhe
//! scritte a mano. Chi apre il file, chi decide in che cartella stanno i
//! percorsi relativi e chi cerca i brani in libreria sta in `aether-app`, dove
//! ci sono un filesystem e un database da interrogare.
//!
//! # I percorsi non si risolvono qui, e non è pigrizia
//!
//! [`VocePlaylist::percorso`] è **quello che c'è scritto nel file**: relativo,
//! assoluto, con le barre in un verso o nell'altro, `file:///` o niente. La
//! tentazione è normalizzarlo subito; la ragione per non farlo è che «relativo»
//! non significa niente senza sapere dov'è il file, e questo modulo il file non
//! l'ha aperto. Un percorso normalizzato rispetto alla cartella sbagliata è
//! peggio di uno non normalizzato: il secondo si può ancora correggere, il
//! primo sembra già giusto.
//!
//! # Cosa non si capisce si conta
//!
//! Nessuna delle tre grammatiche fa fallire una lettura. Un M3U con una riga
//! storta in mezzo dà tutte le altre e una riga in [`PlaylistLetta::illeggibili`],
//! che è la stessa regola di `ScanReport.unreadable` per i file musicali: una
//! playlist di duecento brani con un refuso alla riga novanta è una playlist di
//! centonovantanove brani, non un errore.

use std::fmt::Write as _;

/// I formati che sappiamo leggere e scrivere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatoPlaylist {
    /// `.m3u` e `.m3u8`: una riga per brano, i commenti cominciano con `#`.
    M3u,
    /// `.pls`: un file INI con `File1=`, `Title1=`, `Length1=`.
    Pls,
    /// `.xspf`: XML, il formato che nessuno usa e che tutti leggono.
    Xspf,
}

impl FormatoPlaylist {
    /// Il formato da un'estensione, con o senza punto, di qualunque cassa.
    ///
    /// `.m3u` e `.m3u8` sono lo stesso formato: la differenza è la codifica —
    /// il primo storicamente nella pagina di codice del sistema, il secondo
    /// UTF-8 per definizione — e la codifica è un problema di chi legge i byte,
    /// non della grammatica.
    #[must_use]
    pub fn da_estensione(estensione: &str) -> Option<Self> {
        match estensione
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "m3u" | "m3u8" => Some(Self::M3u),
            "pls" => Some(Self::Pls),
            "xspf" => Some(Self::Xspf),
            _ => None,
        }
    }

    /// L'estensione con cui si scrive, senza il punto.
    ///
    /// Sempre `m3u8` e mai `m3u`: quel che questo modulo scrive è UTF-8, e
    /// dichiararlo nell'estensione è l'unico modo che un altro programma ha di
    /// saperlo senza indovinare.
    #[must_use]
    pub const fn estensione(self) -> &'static str {
        match self {
            Self::M3u => "m3u8",
            Self::Pls => "pls",
            Self::Xspf => "xspf",
        }
    }

    /// Come si chiama a schermo.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::M3u => "M3U",
            Self::Pls => "PLS",
            Self::Xspf => "XSPF",
        }
    }
}

/// Una voce di una playlist su file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VocePlaylist {
    /// Il percorso **come sta scritto nel file**. Vedi la nota in testa.
    pub percorso: String,
    /// Il titolo dichiarato, se il formato ne porta uno.
    pub titolo: Option<String>,
    /// L'interprete dichiarato.
    ///
    /// M3U non ha un campo suo: sta dentro `#EXTINF` nella forma
    /// «interprete - titolo», e quel trattino è una convenzione, non una
    /// regola. Vedi [`spezza_extinf`].
    pub artista: Option<String>,
    /// La durata dichiarata, in millisecondi.
    pub durata_ms: Option<u64>,
}

/// Quel che un file di playlist conteneva.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaylistLetta {
    /// Il nome dichiarato dentro il file, se c'è.
    ///
    /// Solo XSPF ne ha uno vero (`<title>`); PLS e M3U no, e per loro il nome
    /// lo dà il file. Chi legge decide: vedi `import_playlist`.
    pub nome: Option<String>,
    /// Le voci, nell'ordine in cui stavano.
    pub voci: Vec<VocePlaylist>,
    /// Quante righe non si sono capite.
    pub illeggibili: usize,
}

/// Legge dei byte come playlist.
///
/// Toglie il BOM e interpreta come UTF-8 sostituendo quel che non lo è: un
/// M3U scritto vent'anni fa in una pagina di codice locale è pieno di byte che
/// UTF-8 non ammette, e rifiutarlo vorrebbe dire rifiutare esattamente i file
/// che questo modulo esiste per leggere. Un carattere sbagliato in un titolo si
/// vede e si corregge; un file che non si apre no.
#[must_use]
pub fn leggi(byte: &[u8], formato: FormatoPlaylist) -> PlaylistLetta {
    let senza_bom = byte.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(byte);
    let testo = String::from_utf8_lossy(senza_bom);
    leggi_testo(&testo, formato)
}

/// Legge un testo già decodificato.
#[must_use]
pub fn leggi_testo(testo: &str, formato: FormatoPlaylist) -> PlaylistLetta {
    match formato {
        FormatoPlaylist::M3u => leggi_m3u(testo),
        FormatoPlaylist::Pls => leggi_pls(testo),
        FormatoPlaylist::Xspf => leggi_xspf(testo),
    }
}

/// Scrive una playlist nel formato dato.
///
/// Sempre con `\n` e mai con `\r\n`, anche su Windows: tutti e tre i formati lo
/// accettano, e un file con le terminazioni miste è quel che si ottiene appena
/// qualcuno lo apre con un editor diverso.
#[must_use]
pub fn scrivi(nome: &str, voci: &[VocePlaylist], formato: FormatoPlaylist) -> String {
    match formato {
        FormatoPlaylist::M3u => scrivi_m3u(voci),
        FormatoPlaylist::Pls => scrivi_pls(voci),
        FormatoPlaylist::Xspf => scrivi_xspf(nome, voci),
    }
}

/// I millisecondi in secondi arrotondati, o `-1` per «non la so».
///
/// `-1` e non `0`: tutti e tre i formati lo usano per dire che la durata non è
/// nota, e uno zero vorrebbe dire «dura niente» — che è una cosa diversa, e che
/// un lettore altrui disegna come una barra già arrivata in fondo.
#[expect(
    clippy::integer_division,
    reason = "sono secondi: la parte decimale è esattamente quel che si butta, e il +500 la arrotonda"
)]
fn secondi_di(durata_ms: Option<u64>) -> i64 {
    durata_ms.map_or(-1, |ms| {
        i64::try_from(ms.saturating_add(500) / 1000).unwrap_or(-1)
    })
}

// ── M3U ─────────────────────────────────────────────────────────────────────

fn leggi_m3u(testo: &str) -> PlaylistLetta {
    let mut letta = PlaylistLetta::default();
    // L'`#EXTINF` vale per la **riga di percorso successiva**, e solo per
    // quella: due `#EXTINF` di fila sono un file storto, e il secondo vince —
    // che è quel che fa ogni lettore in circolazione.
    let mut in_attesa: Option<(Option<u64>, Option<String>, Option<String>)> = None;

    for riga in testo.lines() {
        let riga = riga.trim_end_matches('\r').trim();
        if riga.is_empty() {
            continue;
        }
        if let Some(resto) = riga.strip_prefix("#EXTINF:") {
            match spezza_extinf(resto) {
                Some(pezzi) => in_attesa = Some(pezzi),
                None => letta.illeggibili += 1,
            }
            continue;
        }
        // Ogni altra direttiva — `#EXTM3U`, `#PLAYLIST`, `#EXTGRP`, i commenti
        // di chi l'ha scritto — si salta senza contarla illeggibile: sono righe
        // legittime che questo modulo non usa, e contarle come guasti farebbe
        // sembrare rotto ogni file scritto bene.
        if riga.starts_with('#') {
            if let Some(nome) = riga.strip_prefix("#PLAYLIST:") {
                let nome = nome.trim();
                if !nome.is_empty() {
                    letta.nome = Some(nome.to_owned());
                }
            }
            continue;
        }
        let (durata_ms, artista, titolo) = in_attesa.take().unwrap_or((None, None, None));
        letta.voci.push(VocePlaylist {
            percorso: riga.to_owned(),
            titolo,
            artista,
            durata_ms,
        });
    }
    letta
}

/// Spezza il corpo di un `#EXTINF:`.
///
/// La forma è `durata_in_secondi,interprete - titolo`. Due punti deboli, e
/// tutti e due sono nel formato e non nel codice:
///
/// - la durata è in **secondi** e può essere `-1` per «non la so»;
/// - il trattino fra interprete e titolo è una convenzione. Un brano che si
///   chiama «Ballad of a Well-Known Gun» ne contiene uno, e spezzare sul primo
///   darebbe interprete «Ballad of a Well». Perciò si spezza su ` - ` con gli
///   spazi, e **una volta sola**, dalla prima occorrenza: è la stessa regola
///   che usano i lettori che sbagliano di meno.
fn spezza_extinf(corpo: &str) -> Option<(Option<u64>, Option<String>, Option<String>)> {
    let (durata, resto) = corpo.split_once(',')?;
    let durata_ms = durata
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|secondi| *secondi >= 0)
        .and_then(|secondi| u64::try_from(secondi).ok())
        .map(|secondi| secondi.saturating_mul(1000));
    let resto = resto.trim();
    if resto.is_empty() {
        return Some((durata_ms, None, None));
    }
    match resto.split_once(" - ") {
        Some((artista, titolo)) if !artista.trim().is_empty() && !titolo.trim().is_empty() => {
            Some((
                durata_ms,
                Some(artista.trim().to_owned()),
                Some(titolo.trim().to_owned()),
            ))
        }
        // Nessun trattino: è tutto titolo. Inventare un interprete dividendo a
        // metà sarebbe peggio che non averlo.
        _ => Some((durata_ms, None, Some(resto.to_owned()))),
    }
}

fn scrivi_m3u(voci: &[VocePlaylist]) -> String {
    let mut fuori = String::from("#EXTM3U\n");
    for voce in voci {
        let secondi = secondi_di(voce.durata_ms);
        let etichetta = match (&voce.artista, &voce.titolo) {
            (Some(artista), Some(titolo)) => format!("{artista} - {titolo}"),
            (None, Some(titolo)) => titolo.clone(),
            // Senza titolo si scrive comunque l'`#EXTINF`: la durata da sola
            // serve a chi legge per mostrare la lunghezza della playlist senza
            // aprire i file.
            _ => String::new(),
        };
        let _ = writeln!(fuori, "#EXTINF:{secondi},{etichetta}");
        fuori.push_str(&voce.percorso);
        fuori.push('\n');
    }
    fuori
}

// ── PLS ─────────────────────────────────────────────────────────────────────

fn leggi_pls(testo: &str) -> PlaylistLetta {
    // Le chiavi sono numerate da 1 e **non sono garantite contigue**: un file
    // con `File1`, `File3` e nessun `File2` esiste, e i lettori che ciclano
    // `1..=NumberOfEntries` si fermano al buco. Si raccoglie per numero e si
    // ordina alla fine.
    let mut per_numero: Vec<(u32, VocePlaylist)> = Vec::new();
    let mut letta = PlaylistLetta::default();

    let trova = |elenco: &mut Vec<(u32, VocePlaylist)>, numero: u32| -> usize {
        match elenco.iter().position(|(n, _)| *n == numero) {
            Some(posto) => posto,
            None => {
                elenco.push((numero, VocePlaylist::default()));
                elenco.len().saturating_sub(1)
            }
        }
    };

    for riga in testo.lines() {
        let riga = riga.trim_end_matches('\r').trim();
        if riga.is_empty() || riga.starts_with('[') || riga.starts_with(';') {
            continue;
        }
        let Some((chiave, valore)) = riga.split_once('=') else {
            letta.illeggibili += 1;
            continue;
        };
        let chiave = chiave.trim();
        let valore = valore.trim();
        let minuscola = chiave.to_ascii_lowercase();
        // `NumberOfEntries` e `Version` si leggono e si buttano: il primo è
        // ridondante — le voci si contano — e fidarsene è esattamente il modo
        // di perdere le righe oltre il numero dichiarato quando è sbagliato.
        if minuscola == "numberofentries" || minuscola == "version" {
            continue;
        }
        let Some((campo, numero)) = spezza_chiave_pls(&minuscola) else {
            letta.illeggibili += 1;
            continue;
        };
        let posto = trova(&mut per_numero, numero);
        let Some((_, voce)) = per_numero.get_mut(posto) else {
            continue;
        };
        match campo {
            "file" => voce.percorso = valore.to_owned(),
            "title" => {
                if !valore.is_empty() {
                    voce.titolo = Some(valore.to_owned());
                }
            }
            "length" => {
                voce.durata_ms = valore
                    .parse::<i64>()
                    .ok()
                    .filter(|secondi| *secondi >= 0)
                    .and_then(|secondi| u64::try_from(secondi).ok())
                    .map(|secondi| secondi.saturating_mul(1000));
            }
            _ => letta.illeggibili += 1,
        }
    }

    per_numero.sort_by_key(|(numero, _)| *numero);
    // Una voce senza `File` non è una voce: è un `Title` orfano, e portarselo
    // dietro darebbe una riga che non punta a niente.
    letta.voci = per_numero
        .into_iter()
        .filter_map(|(_, voce)| (!voce.percorso.is_empty()).then_some(voce))
        .collect();
    letta
}

/// `file12` → `("file", 12)`.
fn spezza_chiave_pls(chiave: &str) -> Option<(&str, u32)> {
    let taglio = chiave.find(|c: char| c.is_ascii_digit())?;
    let (campo, numero) = chiave.split_at(taglio);
    Some((campo, numero.parse().ok()?))
}

fn scrivi_pls(voci: &[VocePlaylist]) -> String {
    let mut fuori = String::from("[playlist]\n");
    for (indice, voce) in voci.iter().enumerate() {
        let numero = indice.saturating_add(1);
        let _ = writeln!(fuori, "File{numero}={}", voce.percorso);
        if let Some(titolo) = &voce.titolo {
            let etichetta = match &voce.artista {
                Some(artista) => format!("{artista} - {titolo}"),
                None => titolo.clone(),
            };
            let _ = writeln!(fuori, "Title{numero}={etichetta}");
        }
        let secondi = secondi_di(voce.durata_ms);
        let _ = writeln!(fuori, "Length{numero}={secondi}");
    }
    let _ = writeln!(fuori, "NumberOfEntries={}", voci.len());
    // `Version=2` in fondo e non in cima: lo vuole la specifica, ed è l'unica
    // riga di questo formato che abbia un ordine obbligatorio.
    fuori.push_str("Version=2\n");
    fuori
}

// ── XSPF ────────────────────────────────────────────────────────────────────

/// Legge un XSPF.
///
/// # Perché non c'è un parser XML
///
/// Perché `aether-domain` non ha dipendenze oltre alla normalizzazione
/// Unicode, e portarne una intera per un formato che si incontra due volte
/// l'anno vorrebbe dire farla compilare anche dentro l'APK. Quel che serve qui
/// è un sottoinsieme minuscolo e completamente fissato: i `<track>` dentro
/// `<trackList>`, e di ognuno quattro elementi di testo.
///
/// Il prezzo è dichiarato: **questo non valida niente**. Un XSPF malformato non
/// dà errore, dà meno voci. È la stessa scelta del resto del modulo — quel che
/// non si capisce si conta — e vale solo perché nessuna decisione irreversibile
/// dipende dal risultato: le voci lette diventano un piano che l'utente guarda
/// prima di confermare.
fn leggi_xspf(testo: &str) -> PlaylistLetta {
    let mut letta = PlaylistLetta::default();

    // Il titolo della playlist è il primo `<title>` **fuori** da `<trackList>`:
    // dentro, `<title>` è il titolo di un brano. Cercarlo prima della lista è
    // sufficiente e non richiede di ricostruire l'annidamento.
    let fine_intestazione = trova_senza_cassa(testo, "<tracklist").unwrap_or(testo.len());
    if let Some(intestazione) = testo.get(..fine_intestazione)
        && let Some(titolo) = contenuto_tag(intestazione, "title")
        && !titolo.is_empty()
    {
        letta.nome = Some(titolo);
    }

    let mut da = fine_intestazione;
    while let Some(inizio) = testo
        .get(da..)
        .and_then(|resto| trova_apertura(resto, "track"))
    {
        let inizio = da.saturating_add(inizio);
        let Some(fine) = testo
            .get(inizio..)
            .and_then(|resto| trova_senza_cassa(resto, "</track>"))
        else {
            // Un `<track>` che non si chiude: è la fine utile del file.
            letta.illeggibili += 1;
            break;
        };
        let fine = inizio.saturating_add(fine);
        let Some(blocco) = testo.get(inizio..fine) else {
            break;
        };
        da = fine.saturating_add("</track>".len());

        let percorso = contenuto_tag(blocco, "location").unwrap_or_default();
        if percorso.is_empty() {
            letta.illeggibili += 1;
            continue;
        }
        letta.voci.push(VocePlaylist {
            percorso,
            titolo: contenuto_tag(blocco, "title").filter(|t| !t.is_empty()),
            artista: contenuto_tag(blocco, "creator").filter(|a| !a.is_empty()),
            durata_ms: contenuto_tag(blocco, "duration").and_then(|d| d.trim().parse().ok()),
        });
    }
    letta
}

/// Cerca l'apertura di un tag, e **solo** quel tag.
///
/// `<trackList>` comincia con `<track` anche lui, e cercare la sottostringa
/// nuda faceva sparire il primo brano di ogni playlist: la lista intera veniva
/// presa per un brano, il suo blocco arrivava fino al `</track>` del primo, e
/// il ciclo ripartiva dopo. Nessuna prova di grammatica l'avrebbe visto — il
/// file era corretto e il risultato era «un brano in meno», sempre lo stesso.
/// Il nome deve finire dove finisce: `>`, uno spazio, o `/` di un tag vuoto.
fn trova_apertura(pagliaio: &str, tag: &str) -> Option<usize> {
    let minuscolo = pagliaio.to_ascii_lowercase();
    let ago = format!("<{tag}");
    let mut da = 0usize;
    while let Some(trovato) = minuscolo.get(da..).and_then(|resto| resto.find(&ago)) {
        let inizio = da.saturating_add(trovato);
        let dopo = inizio.saturating_add(ago.len());
        match minuscolo.get(dopo..).and_then(|resto| resto.chars().next()) {
            Some('>' | '/') | Some(' ' | '\t' | '\n' | '\r') => return Some(inizio),
            // Un altro tag che comincia allo stesso modo: si va avanti.
            Some(_) => da = dopo,
            None => return None,
        }
    }
    None
}

/// Cerca un ago in un pagliaio ignorando maiuscole e minuscole.
///
/// Restituisce un indice di **byte** valido su `pagliaio`, perché è così che si
/// affetta una `&str`. L'ago è sempre ASCII qui dentro, quindi la conversione
/// in minuscolo non sposta nessun confine di carattere.
fn trova_senza_cassa(pagliaio: &str, ago: &str) -> Option<usize> {
    pagliaio.to_ascii_lowercase().find(ago)
}

/// Il testo dentro il primo `<tag>…</tag>`, con le entità sciolte.
fn contenuto_tag(blocco: &str, tag: &str) -> Option<String> {
    let minuscolo = blocco.to_ascii_lowercase();
    let apertura = minuscolo.find(&format!("<{tag}"))?;
    // Gli attributi si saltano: `<location foo="bar">` è legale, e a noi
    // interessa solo cosa c'è dopo il `>`.
    let dopo_attributi = blocco.get(apertura..)?.find('>')?;
    let inizio = apertura.saturating_add(dopo_attributi).saturating_add(1);
    let chiusura = minuscolo.get(inizio..)?.find(&format!("</{tag}>"))?;
    let dentro = blocco.get(inizio..inizio.saturating_add(chiusura))?;
    Some(sciogli_entita(dentro.trim()))
}

/// `&amp;` → `&`, e le altre quattro. Più le numeriche, decimali ed esadecimali.
fn sciogli_entita(testo: &str) -> String {
    if !testo.contains('&') {
        return testo.to_owned();
    }
    let mut fuori = String::with_capacity(testo.len());
    let mut resto = testo;
    while let Some(e) = resto.find('&') {
        fuori.push_str(resto.get(..e).unwrap_or_default());
        let coda = resto.get(e..).unwrap_or_default();
        match coda.find(';').filter(|fine| *fine <= 10) {
            Some(fine) => {
                let entita = coda.get(1..fine).unwrap_or_default();
                match entita {
                    "amp" => fuori.push('&'),
                    "lt" => fuori.push('<'),
                    "gt" => fuori.push('>'),
                    "quot" => fuori.push('"'),
                    "apos" => fuori.push('\''),
                    numerica if numerica.starts_with('#') => {
                        let cifre = numerica.get(1..).unwrap_or_default();
                        let punto = if let Some(esa) = cifre.strip_prefix(['x', 'X']) {
                            u32::from_str_radix(esa, 16).ok()
                        } else {
                            cifre.parse().ok()
                        };
                        match punto.and_then(char::from_u32) {
                            Some(c) => fuori.push(c),
                            // Un'entità numerica che non è un carattere resta
                            // com'era scritta: buttarla perderebbe del testo,
                            // e nessuno sa cosa volesse dire.
                            None => fuori.push_str(coda.get(..=fine).unwrap_or_default()),
                        }
                    }
                    _ => fuori.push_str(coda.get(..=fine).unwrap_or_default()),
                }
                resto = coda.get(fine.saturating_add(1)..).unwrap_or_default();
            }
            // Una `&` che non apre un'entità è una `&` e basta: capita, e non è
            // un errore da propagare.
            None => {
                fuori.push('&');
                resto = coda.get(1..).unwrap_or_default();
            }
        }
    }
    fuori.push_str(resto);
    fuori
}

/// Il contrario: i cinque caratteri che in XML non si possono scrivere così.
fn proteggi(testo: &str) -> String {
    let mut fuori = String::with_capacity(testo.len());
    for c in testo.chars() {
        match c {
            '&' => fuori.push_str("&amp;"),
            '<' => fuori.push_str("&lt;"),
            '>' => fuori.push_str("&gt;"),
            '"' => fuori.push_str("&quot;"),
            '\'' => fuori.push_str("&apos;"),
            altro => fuori.push(altro),
        }
    }
    fuori
}

fn scrivi_xspf(nome: &str, voci: &[VocePlaylist]) -> String {
    let mut fuori = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    fuori.push_str("<playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n");
    let _ = writeln!(fuori, "  <title>{}</title>", proteggi(nome));
    fuori.push_str("  <trackList>\n");
    for voce in voci {
        fuori.push_str("    <track>\n");
        let _ = writeln!(
            fuori,
            "      <location>{}</location>",
            proteggi(&voce.percorso)
        );
        if let Some(titolo) = &voce.titolo {
            let _ = writeln!(fuori, "      <title>{}</title>", proteggi(titolo));
        }
        if let Some(artista) = &voce.artista {
            let _ = writeln!(fuori, "      <creator>{}</creator>", proteggi(artista));
        }
        if let Some(ms) = voce.durata_ms {
            let _ = writeln!(fuori, "      <duration>{ms}</duration>");
        }
        fuori.push_str("    </track>\n");
    }
    fuori.push_str("  </trackList>\n</playlist>\n");
    fuori
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn le_estensioni_che_conosciamo() {
        assert_eq!(
            FormatoPlaylist::da_estensione("m3u8"),
            Some(FormatoPlaylist::M3u)
        );
        assert_eq!(
            FormatoPlaylist::da_estensione(".M3U"),
            Some(FormatoPlaylist::M3u)
        );
        assert_eq!(
            FormatoPlaylist::da_estensione("PLS"),
            Some(FormatoPlaylist::Pls)
        );
        assert_eq!(FormatoPlaylist::da_estensione("txt"), None);
    }

    #[test]
    fn un_m3u_con_extinf() {
        let testo = "#EXTM3U\n\
                     #EXTINF:245,Björk - Jóga\n\
                     Björk/Homogenic/02 Jóga.flac\n\
                     #EXTINF:-1,Senza durata\n\
                     altro.mp3\n";
        let letta = leggi_testo(testo, FormatoPlaylist::M3u);
        assert_eq!(letta.voci.len(), 2);
        assert_eq!(letta.voci[0].artista.as_deref(), Some("Björk"));
        assert_eq!(letta.voci[0].titolo.as_deref(), Some("Jóga"));
        assert_eq!(letta.voci[0].durata_ms, Some(245_000));
        // `-1` vuol dire «non la so», non «zero».
        assert_eq!(letta.voci[1].durata_ms, None);
        assert_eq!(letta.voci[1].titolo.as_deref(), Some("Senza durata"));
    }

    #[test]
    fn un_titolo_col_trattino_non_diventa_un_interprete() {
        // Il caso vero: «Ballad of a Well-Known Gun». Spezzare sul primo
        // trattino darebbe interprete «Ballad of a Well».
        let testo = "#EXTINF:283,Elton John - Ballad of a Well-Known Gun\nx.mp3\n";
        let letta = leggi_testo(testo, FormatoPlaylist::M3u);
        assert_eq!(letta.voci[0].artista.as_deref(), Some("Elton John"));
        assert_eq!(
            letta.voci[0].titolo.as_deref(),
            Some("Ballad of a Well-Known Gun")
        );
    }

    #[test]
    fn un_m3u_senza_extinf_e_solo_percorsi() {
        let testo = "musica/uno.mp3\r\nmusica/due.mp3\r\n";
        let letta = leggi_testo(testo, FormatoPlaylist::M3u);
        assert_eq!(letta.voci.len(), 2);
        // Il `\r` non deve finire nel percorso: un file scritto su Windows e
        // letto altrove darebbe percorsi che non esistono, per un byte.
        assert_eq!(letta.voci[0].percorso, "musica/uno.mp3");
        assert!(letta.voci[0].titolo.is_none());
    }

    #[test]
    fn le_direttive_che_non_usiamo_non_sono_guasti() {
        let testo = "#EXTM3U\n#EXTGRP:Rock\n# un commento\n#PLAYLIST:Serata\nuno.mp3\n";
        let letta = leggi_testo(testo, FormatoPlaylist::M3u);
        assert_eq!(letta.voci.len(), 1);
        assert_eq!(letta.illeggibili, 0, "un file scritto bene non ha guasti");
        assert_eq!(letta.nome.as_deref(), Some("Serata"));
    }

    #[test]
    fn un_pls_con_i_numeri_fuori_ordine_e_con_un_buco() {
        // Il caso che rompe i lettori che ciclano `1..=NumberOfEntries`.
        let testo = "[playlist]\n\
                     File3=terzo.mp3\n\
                     Title3=Terzo\n\
                     File1=primo.mp3\n\
                     Title1=Primo\n\
                     Length1=200\n\
                     NumberOfEntries=2\n\
                     Version=2\n";
        let letta = leggi_testo(testo, FormatoPlaylist::Pls);
        assert_eq!(letta.voci.len(), 2);
        assert_eq!(letta.voci[0].percorso, "primo.mp3");
        assert_eq!(letta.voci[1].percorso, "terzo.mp3");
        assert_eq!(letta.voci[0].durata_ms, Some(200_000));
        assert_eq!(letta.illeggibili, 0);
    }

    #[test]
    fn un_title_senza_file_non_diventa_una_voce() {
        let testo = "[playlist]\nTitle1=Orfano\nFile2=vero.mp3\n";
        let letta = leggi_testo(testo, FormatoPlaylist::Pls);
        assert_eq!(letta.voci.len(), 1);
        assert_eq!(letta.voci[0].percorso, "vero.mp3");
    }

    #[test]
    fn un_xspf_con_entita_e_attributi() {
        let testo = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>Sam &amp; Dave</title>
  <trackList>
    <track>
      <location>file:///C:/Musica/Hold%20On.mp3</location>
      <title>Hold On, I&apos;m Comin&apos;</title>
      <creator>Sam &amp; Dave</creator>
      <duration>151000</duration>
    </track>
  </trackList>
</playlist>"#;
        let letta = leggi_testo(testo, FormatoPlaylist::Xspf);
        assert_eq!(letta.nome.as_deref(), Some("Sam & Dave"));
        assert_eq!(letta.voci.len(), 1);
        assert_eq!(letta.voci[0].artista.as_deref(), Some("Sam & Dave"));
        assert_eq!(letta.voci[0].titolo.as_deref(), Some("Hold On, I'm Comin'"));
        assert_eq!(letta.voci[0].durata_ms, Some(151_000));
    }

    #[test]
    fn il_titolo_della_playlist_non_e_quello_del_primo_brano() {
        // Senza il taglio prima di `<trackList>`, il nome della playlist
        // diventerebbe il titolo del primo brano — e nessuno se ne
        // accorgerebbe finché la playlist non si chiama come una canzone.
        let testo = "<playlist><trackList><track>\
                     <location>a.mp3</location><title>Una canzone</title>\
                     </track></trackList></playlist>";
        let letta = leggi_testo(testo, FormatoPlaylist::Xspf);
        assert_eq!(letta.nome, None);
        assert_eq!(letta.voci[0].titolo.as_deref(), Some("Una canzone"));
    }

    #[test]
    fn un_xspf_troncato_da_quel_che_ha_letto() {
        let testo = "<playlist><trackList>\
                     <track><location>a.mp3</location></track>\
                     <track><location>b.mp3</locat";
        let letta = leggi_testo(testo, FormatoPlaylist::Xspf);
        assert_eq!(letta.voci.len(), 1);
        assert_eq!(letta.illeggibili, 1, "il troncamento si dichiara");
    }

    #[test]
    fn il_bom_non_finisce_nel_primo_percorso() {
        // Il difetto classico: il primo brano di ogni playlist esportata da
        // Windows non si trova mai, per tre byte invisibili.
        let mut byte = vec![0xEF, 0xBB, 0xBF];
        byte.extend_from_slice(b"uno.mp3\n");
        let letta = leggi(&byte, FormatoPlaylist::M3u);
        assert_eq!(letta.voci[0].percorso, "uno.mp3");
    }

    #[test]
    fn i_byte_che_non_sono_utf8_non_fermano_la_lettura() {
        // Un M3U scritto in una pagina di codice locale. I titoli avranno un
        // carattere sbagliato; i percorsi ASCII no, ed è quel che serve per
        // ritrovare i brani.
        let byte = b"#EXTINF:100,Bj\xF6rk - J\xF3ga\nmusica/joga.mp3\n";
        let letta = leggi(byte, FormatoPlaylist::M3u);
        assert_eq!(letta.voci.len(), 1);
        assert!(letta.voci[0].artista.is_some());
    }

    #[test]
    fn i_tre_formati_fanno_andata_e_ritorno() {
        let voci = vec![
            VocePlaylist {
                percorso: "C:\\Musica\\Björk\\Jóga.flac".to_owned(),
                titolo: Some("Jóga".to_owned()),
                artista: Some("Björk".to_owned()),
                durata_ms: Some(245_000),
            },
            VocePlaylist {
                percorso: "relativo/altro & ancora.mp3".to_owned(),
                titolo: Some("Altro <e> ancora".to_owned()),
                artista: None,
                durata_ms: None,
            },
        ];
        for formato in [
            FormatoPlaylist::M3u,
            FormatoPlaylist::Pls,
            FormatoPlaylist::Xspf,
        ] {
            let scritta = scrivi("La mia", &voci, formato);
            let riletta = leggi_testo(&scritta, formato);
            assert_eq!(
                riletta.voci.len(),
                2,
                "{}: le voci si perdono nel giro",
                formato.nome()
            );
            assert_eq!(
                riletta.voci[0].percorso,
                voci[0].percorso,
                "{}: il percorso non torna",
                formato.nome()
            );
            assert_eq!(
                riletta.voci[1].percorso,
                voci[1].percorso,
                "{}: le e commerciali nel percorso",
                formato.nome()
            );
            assert_eq!(
                riletta.voci[0].durata_ms,
                Some(245_000),
                "{}: la durata non torna",
                formato.nome()
            );
        }
    }

    #[test]
    fn una_playlist_vuota_si_scrive_e_si_rilegge_vuota() {
        for formato in [
            FormatoPlaylist::M3u,
            FormatoPlaylist::Pls,
            FormatoPlaylist::Xspf,
        ] {
            let scritta = scrivi("Vuota", &[], formato);
            let riletta = leggi_testo(&scritta, formato);
            assert!(riletta.voci.is_empty(), "{}", formato.nome());
            assert_eq!(riletta.illeggibili, 0, "{}", formato.nome());
        }
    }
}
