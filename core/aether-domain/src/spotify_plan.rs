//! Quali brani di Spotify sono già sul disco, e quali no.
//!
//! # Il problema, detto per bene
//!
//! Spotify chiama un brano «Karma Police», il file sul disco lo chiama «Karma
//! Police», e finisce lì una volta su due. L'altra volta Spotify dice «Everything
//! In Its Right Place - 2009 Remaster» dove il file dice «Everything In Its Right
//! Place»; oppure dice «Artista A, Artista B» dove il tag dice solo «Artista A»;
//! oppure il brano sta su un singolo invece che sull'album. Sono lo stesso brano,
//! e un abbinamento che pretende l'uguaglianza esatta li dichiara mancanti tutti.
//!
//! La risposta è una **scala**: si prova prima il confronto stretto, e si scende
//! solo se non ha trovato niente. Ogni gradino allarga di poco e in un modo
//! preciso, e ogni gradino sotto il secondo è guardato a vista dalla durata.
//!
//! # Il gradino zero: l'ISRC
//!
//! Sopra tutti c'è un confronto che non ha bisogno di nessuna di queste
//! astuzie. L'ISRC nomina **la registrazione**, non il modo in cui qualcuno l'ha
//! intitolata: lo stesso codice a dodici caratteri sta sul remaster e
//! sull'originale, sulla versione con l'ospite scritto nel titolo e su quella
//! senza. Dove c'è da tutte e due le parti, la scala qui sotto non serve.
//!
//! Non è il gradino che oggi lavora di più — Pathfinder l'ISRC non lo manda più
//! (vedi la nota in testa a `aether_spotify::pathfinder`), quindi arriva solo
//! dai tag di un file già taggato bene o da un'importazione vecchia. Ma quando
//! c'è **non sbaglia**, ed è l'unico gradino di cui si possa dire. Costa una
//! tabella di hash su una libreria che si sta già scorrendo tre volte.
//!
//! # Perché la durata sorveglia i gradini larghi
//!
//! Senza, il terzo gradino abbinerebbe «Everything In Its Right Place» a
//! «Everything In Its Right Place (Live at Glastonbury)» — che dopo la ripulitura
//! hanno lo stesso identico titolo e lo stesso artista. Sono due registrazioni
//! diverse, e la differenza si vede solo nei secondi. La tolleranza è larga
//! ([`TOLLERANZA_MS`]) perché lo stesso brano ricodificato da sorgenti diverse
//! differisce davvero di qualche secondo — è la ragione per cui la durata non fa
//! parte di [`crate::keys::TrackKey`], spiegata lì.
//!
//! # Cosa succede in caso di dubbio
//!
//! Non si indovina. Se due file in libreria sono candidati ugualmente plausibili
//! e nessuno dei due ha una durata utile a distinguerli, il brano risulta
//! **mancante**: chi legge il piano può andarselo a cercare, mentre un
//! abbinamento sbagliato mette in playlist la canzone di qualcun altro e nessuno
//! se ne accorge.

use std::collections::HashMap;

use crate::keys::{TrackKey, TrackKeyInput, normalize_key};
use crate::spotify::SpotifyTrack;

/// Di quanto possono differire due durate e restare lo stesso brano.
///
/// Dieci secondi. Sembra molto, e lo è: serve a coprire i silenzi di coda
/// diversi fra un rip e l'altro e gli stacchi delle versioni radio, che sono la
/// causa vera degli scarti. Regge perché non lavora da sola — a questo punto
/// artista e titolo coincidono già, e due canzoni diverse dello stesso artista
/// con lo stesso titolo ripulito sono un caso raro quanto un remix live.
pub const TOLLERANZA_MS: u64 = 10_000;

/// Un brano già in libreria, ridotto a quel che serve per riconoscerlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTrack {
    /// L'identificativo della riga.
    pub id: i64,
    /// La chiave d'identità già calcolata, come sta nel database.
    pub track_key: String,
    /// L'interprete, dal tag.
    pub artist: String,
    /// Il titolo, dal tag.
    pub title: String,
    /// La durata in millisecondi. Zero significa «non la so».
    pub duration_ms: i64,
    /// L'ISRC, dal tag o da un'importazione precedente.
    ///
    /// `None` sulla stragrande maggioranza delle righe, ed è previsto: dove c'è
    /// vale più di tutto il resto, dove non c'è non costa niente.
    pub isrc: Option<String>,
}

/// Su quale gradino della scala è avvenuto l'abbinamento.
///
/// Va nel rapporto, e non per curiosità: «180 abbinati, di cui 24 per solo
/// titolo» è un'informazione su cui l'utente può decidere di controllare, mentre
/// «180 abbinati» non lo è.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gradino {
    /// Stesso ISRC: è la stessa registrazione, e non c'è altro da guardare.
    Isrc,
    /// Chiave d'identità completa: artista, titolo e album coincidono.
    ChiaveEsatta,
    /// Artista e titolo coincidono, l'album no. La durata conferma.
    ArtistaTitolo,
    /// Coincidono dopo aver tolto le decorazioni. La durata conferma.
    Ripulito,
}

/// Un brano di Spotify ritrovato in libreria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abbinato {
    /// La posizione del brano nell'elenco di Spotify.
    pub indice: usize,
    /// La riga di libreria che gli corrisponde.
    pub track_id: i64,
    /// Come lo si è ritrovato.
    pub gradino: Gradino,
}

/// Cosa si può importare, e cosa no.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpotifyPlan {
    /// I brani ritrovati, in ordine di comparsa su Spotify.
    pub abbinati: Vec<Abbinato>,
    /// Le posizioni dei brani che in libreria non ci sono.
    pub mancanti: Vec<usize>,
}

impl SpotifyPlan {
    /// Quanti sono stati ritrovati su ciascun gradino.
    #[must_use]
    pub fn per_gradino(&self, gradino: Gradino) -> usize {
        self.abbinati
            .iter()
            .filter(|a| a.gradino == gradino)
            .count()
    }
}

/// Decide, per ogni brano di Spotify, se è già in libreria.
///
/// Lo stesso brano di libreria può essere abbinato più volte: una playlist può
/// contenere due volte la stessa canzone, e `playlist_tracks` lo permette
/// apposta. Impedirlo qui vorrebbe dire far sparire la seconda occorrenza.
///
/// Per un elenco solo va benissimo. Chi ne ha molti — l'importazione di un
/// account intero ne ha una per playlist, più i preferiti, più la cronologia —
/// costruisca un [`Indice`] una volta e lo riusi: qui dentro si ricostruisce a
/// ogni chiamata, e su duecento playlist vorrebbe dire scorrere duecento volte
/// tutta la libreria per rifare le stesse quattro tabelle.
#[must_use]
pub fn plan_spotify_import(brani: &[SpotifyTrack], libreria: &[LibraryTrack]) -> SpotifyPlan {
    Indice::nuovo(libreria).piano(brani)
}

/// La libreria messa in tabella, pronta a rispondere «questo brano ce l'ho?».
///
/// # Perché è un tipo e non quattro variabili locali
///
/// Perché costruirlo costa quanto scorrere la libreria quattro volte, e chi
/// importa un account intero fa la stessa domanda a duecento elenchi diversi. Le
/// quattro tabelle non dipendono da cosa si sta cercando: dipendono solo da cosa
/// c'è sul disco, che per tutta la durata di un'importazione non cambia.
///
/// Il tempo di vita è quello della libreria che l'ha generato — le due tabelle
/// larghe tengono riferimenti alle righe invece di copiarle, perché una riga
/// duplicata per ognuno dei due indici sarebbe la libreria in memoria tre volte.
#[derive(Debug)]
pub struct Indice<'a> {
    per_isrc: HashMap<String, i64>,
    per_chiave: HashMap<String, i64>,
    per_artista_titolo: HashMap<(String, String), Vec<&'a LibraryTrack>>,
    per_ripulito: HashMap<(String, String), Vec<&'a LibraryTrack>>,
    durate: HashMap<i64, i64>,
}

impl<'a> Indice<'a> {
    /// Mette in tabella una libreria.
    #[must_use]
    pub fn nuovo(libreria: &'a [LibraryTrack]) -> Self {
        Self {
            per_isrc: indicizza_per_isrc(libreria),
            per_chiave: indicizza_per_chiave(libreria),
            per_artista_titolo: indicizza(libreria, |t| {
                (
                    normalize_key(Some(&t.artist)),
                    normalize_key(Some(&t.title)),
                )
            }),
            per_ripulito: indicizza(libreria, |t| {
                (
                    normalize_key(Some(primo_artista(&t.artist))),
                    normalize_key(Some(&senza_decorazioni(&t.title))),
                )
            }),
            durate: libreria.iter().map(|t| (t.id, t.duration_ms)).collect(),
        }
    }

    /// Quanto dura, sul disco, il brano che [`Self::abbina`] ha ritrovato.
    ///
    /// Esiste per un caso solo, e vale la pena dire quale: l'archivio di Spotify
    /// **non porta mai la durata** — né `YourLibrary.json`, né i file di
    /// playlist, né la cronologia estesa hanno un campo per dirla. Chi deve
    /// decidere se un ascolto conta si ritroverebbe con `duration_ms: None`, e
    /// [`crate::listen::counts_as_play`] senza durata ricade sulla sola soglia
    /// dei quattro minuti: cioè butterebbe via ogni ascolto di ogni canzone che
    /// dura meno di quattro minuti, che è quasi tutta la musica.
    ///
    /// Il brano ritrovato in libreria la durata ce l'ha, e per definizione è la
    /// stessa canzone. Da lì la si prende.
    #[must_use]
    pub fn durata(&self, track_id: i64) -> Option<u64> {
        self.durate
            .get(&track_id)
            .copied()
            .and_then(|ms| u64::try_from(ms).ok())
    }

    /// Cosa porterebbe l'importazione di questo elenco.
    #[must_use]
    pub fn piano(&self, brani: &[SpotifyTrack]) -> SpotifyPlan {
        let mut piano = SpotifyPlan::default();
        for (indice, brano) in brani.iter().enumerate() {
            match self.abbina(brano) {
                Some((track_id, gradino)) => piano.abbinati.push(Abbinato {
                    indice,
                    track_id,
                    gradino,
                }),
                None => piano.mancanti.push(indice),
            }
        }
        piano
    }

    /// La scala, un gradino alla volta.
    #[must_use]
    pub fn abbina(&self, brano: &SpotifyTrack) -> Option<(i64, Gradino)> {
        // Il gradino zero: nessuna normalizzazione, nessuna durata a
        // sorvegliare. Due registrazioni con lo stesso ISRC *sono* la stessa
        // registrazione.
        if let Some(codice) = brano.isrc.as_deref().and_then(normalizza_isrc)
            && let Some(id) = self.per_isrc.get(&codice)
        {
            return Some((*id, Gradino::Isrc));
        }

        let chiave = TrackKey::compute(TrackKeyInput {
            artist: brano.artist.as_deref(),
            title: Some(&brano.title),
            album: brano.album.as_deref(),
        });
        if let Some(id) = self.per_chiave.get(chiave.as_str()) {
            return Some((*id, Gradino::ChiaveEsatta));
        }

        let secondo = (
            normalize_key(brano.artist.as_deref()),
            normalize_key(Some(&brano.title)),
        );
        if let Some(candidati) = self.per_artista_titolo.get(&secondo)
            && let Some(id) = scegli(candidati, brano.duration_ms)
        {
            return Some((id, Gradino::ArtistaTitolo));
        }

        let terzo = (
            normalize_key(brano.artist.as_deref().map(primo_artista)),
            normalize_key(Some(&senza_decorazioni(&brano.title))),
        );
        if let Some(candidati) = self.per_ripulito.get(&terzo)
            && let Some(id) = scegli(candidati, brano.duration_ms)
        {
            return Some((id, Gradino::Ripulito));
        }

        None
    }
}

/// Fra più candidati, quello che la durata conferma.
///
/// Restituisce `None` quando la durata non basta a decidere: è la regola
/// spiegata in testa al modulo, e il caso in cui un abbinamento sbagliato
/// costerebbe più di un mancante in più.
fn scegli(candidati: &[&LibraryTrack], durata: Option<u64>) -> Option<i64> {
    if candidati.is_empty() {
        return None;
    }

    if let Some(attesa) = durata {
        let mut migliore: Option<(i64, u64)> = None;
        let mut con_durata = 0_usize;
        for candidato in candidati {
            let Ok(sua) = u64::try_from(candidato.duration_ms) else {
                continue;
            };
            if sua == 0 {
                continue;
            }
            con_durata = con_durata.saturating_add(1);
            let scarto = attesa.abs_diff(sua);
            if scarto > TOLLERANZA_MS {
                continue;
            }
            let meglio = migliore.is_none_or(|(id, precedente)| {
                scarto < precedente || (scarto == precedente && candidato.id < id)
            });
            if meglio {
                migliore = Some((candidato.id, scarto));
            }
        }
        if let Some((id, _)) = migliore {
            return Some(id);
        }
        // Tutti i candidati avevano una durata e nessuna combaciava: non sono
        // quella registrazione. Vale anche quando il candidato è **uno solo** —
        // è il caso della versione dal vivo di sei minuti al posto del taglio
        // radiofonico di tre, dove «è l'unico che somiglia» non è una ragione
        // sufficiente per metterlo in playlist.
        if con_durata == candidati.len() {
            return None;
        }
    }

    // Nessuna durata utile da una delle due parti: resta solo l'assenza di
    // ambiguità. Un candidato unico è lui; due no, e non si indovina.
    match candidati {
        [solo] => Some(solo.id),
        _ => None,
    }
}

/// L'ISRC ridotto alla sua forma confrontabile, se è un ISRC.
///
/// Dodici caratteri alfanumerici in maiuscolo. I trattini con cui a volte lo si
/// scrive — «GB-AYE-97-00426» — non fanno parte del codice, e la maiuscola non
/// è garantita da nessuno dei due lati.
///
/// La lunghezza si **verifica** invece di accettare quel che c'è: nel campo
/// `isrc` di un file taggato a mano finisce di tutto, e un valore corto che
/// combacia per caso con un altro valore corto abbinerebbe due brani che non
/// c'entrano niente — sul gradino che per costruzione non passa dal controllo
/// della durata, cioè quello dove un errore non ha nessuna rete sotto.
#[must_use]
pub fn normalizza_isrc(grezzo: &str) -> Option<String> {
    let pulito: String = grezzo
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (pulito.len() == 12).then_some(pulito)
}

/// Dall'ISRC alla riga, tenendo la più vecchia.
///
/// Stessa regola di [`indicizza_per_chiave`], e per la stessa ragione: due file
/// con lo stesso ISRC sono lo stesso brano in due formati, e quello con
/// l'identificativo più basso è quello a cui puntano già le playlist.
fn indicizza_per_isrc(libreria: &[LibraryTrack]) -> HashMap<String, i64> {
    let mut mappa: HashMap<String, i64> = HashMap::new();
    for brano in libreria {
        let Some(codice) = brano.isrc.as_deref().and_then(normalizza_isrc) else {
            continue;
        };
        mappa
            .entry(codice)
            .and_modify(|id| *id = (*id).min(brano.id))
            .or_insert(brano.id);
    }
    mappa
}

/// Dalla chiave d'identità alla riga, tenendo la più vecchia.
///
/// Una chiave può nominare più righe — lo stesso brano in due formati. Si tiene
/// l'identificativo più basso, per la stessa ragione scritta in
/// `import_legacy`: è la riga a cui puntano più probabilmente le playlist e la
/// cronologia già esistenti.
fn indicizza_per_chiave(libreria: &[LibraryTrack]) -> HashMap<String, i64> {
    let mut mappa: HashMap<String, i64> = HashMap::with_capacity(libreria.len());
    for brano in libreria {
        mappa
            .entry(brano.track_key.clone())
            .and_modify(|id| *id = (*id).min(brano.id))
            .or_insert(brano.id);
    }
    mappa
}

/// Raggruppa la libreria per una chiave qualsiasi.
fn indicizza<C: Fn(&LibraryTrack) -> (String, String)>(
    libreria: &[LibraryTrack],
    chiave: C,
) -> HashMap<(String, String), Vec<&LibraryTrack>> {
    let mut mappa: HashMap<(String, String), Vec<&LibraryTrack>> = HashMap::new();
    for brano in libreria {
        mappa.entry(chiave(brano)).or_default().push(brano);
    }
    mappa
}

/// I separatori dopo i quali comincia un artista che non è il principale.
///
/// Lo spazio davanti a `feat` e `ft` fa da confine di parola senza bisogno di
/// un'espressione regolare: `feat` catturato così non può essere la fine di
/// un'altra parola, e `featuring` comincia per `feat` quindi è già coperto.
const SEPARATORI_ARTISTA: &[&str] = &[",", " & ", " feat", " ft.", " ft ", " with ", " vs "];

/// Solo il primo interprete.
///
/// Spotify unisce tutti gli interpreti in un campo solo («A, B & C») mentre nei
/// tag di un file c'è spesso solo il principale. Tagliare al primo separatore è
/// quel che rende confrontabili le due forme.
#[must_use]
pub fn primo_artista(artista: &str) -> &str {
    // `to_ascii_lowercase` conserva la lunghezza in byte, quindi una posizione
    // trovata nella copia minuscola vale anche nell'originale.
    let minuscolo = artista.to_ascii_lowercase();
    let taglio = SEPARATORI_ARTISTA
        .iter()
        .filter_map(|sep| minuscolo.find(sep))
        .min();
    match taglio {
        Some(dove) => artista.get(..dove).unwrap_or(artista).trim(),
        None => artista.trim(),
    }
}

/// Le parole che rivelano una coda di edizione invece di un titolo.
const PAROLE_EDIZIONE: &[&str] = &[
    "version",
    "versione",
    "remaster",
    "rimaster",
    "mix",
    "edit",
    "mono",
    "stereo",
    "live",
    "radio",
    "single",
    "remix",
    "acoustic",
    "acustic",
    "demo",
    "deluxe",
    "anniversary",
    "bonus",
    "instrumental",
    "strumentale",
];

/// Ogni forma di trattino che può separare un titolo dalla sua coda.
const TRATTINI: [char; 4] = ['-', '\u{2013}', '\u{2014}', '\u{2212}'];

/// Il titolo senza le decorazioni che Spotify ci attacca.
///
/// Toglie i gruppi fra parentesi — `(feat. X)`, `[Live]`, `{Bonus Track}` — e la
/// coda dopo un trattino isolato quando quella coda parla di un'edizione e non
/// del brano. La seconda condizione è essenziale: senza, «Us - Live» e «Marvin
/// Gaye - What's Going On» verrebbero tagliati allo stesso modo, e il secondo
/// perderebbe metà del titolo.
#[must_use]
pub fn senza_decorazioni(titolo: &str) -> String {
    let senza_parentesi = togli_parentesi(titolo);
    togli_coda_edizione(&senza_parentesi)
}

/// Toglie i gruppi fra parentesi, quadre o graffe.
///
/// Se le parentesi non si chiudono, il titolo torna com'era: un titolo che
/// contiene una parentesi aperta per errore è comunque un titolo, e mangiarne la
/// metà finale sarebbe peggio che lasciarlo intero.
fn togli_parentesi(titolo: &str) -> String {
    let mut fuori = String::with_capacity(titolo.len());
    let mut profondita = 0_usize;
    for c in titolo.chars() {
        match c {
            '(' | '[' | '{' => {
                profondita = profondita.saturating_add(1);
                fuori.push(' ');
            }
            ')' | ']' | '}' => {
                if profondita == 0 {
                    return titolo.to_owned();
                }
                profondita = profondita.saturating_sub(1);
                fuori.push(' ');
            }
            altro if profondita == 0 => fuori.push(altro),
            _ => {}
        }
    }
    if profondita == 0 {
        fuori.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        titolo.to_owned()
    }
}

/// Toglie la coda dopo un trattino isolato, se parla di un'edizione.
fn togli_coda_edizione(titolo: &str) -> String {
    let caratteri: Vec<(usize, char)> = titolo.char_indices().collect();
    for (n, (posizione, c)) in caratteri.iter().enumerate() {
        if !TRATTINI.contains(c) {
            continue;
        }
        // Deve essere un trattino isolato fra due spazi: quello dentro
        // «Jean-Michel» o «Post-Punk» non separa niente.
        let prima = n
            .checked_sub(1)
            .and_then(|p| caratteri.get(p))
            .map(|(_, c)| *c);
        let dopo = caratteri.get(n.saturating_add(1)).map(|(_, c)| *c);
        if !prima.is_some_and(char::is_whitespace) || !dopo.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(coda) = titolo.get(*posizione..) else {
            continue;
        };
        let coda = coda.to_lowercase();
        if PAROLE_EDIZIONE.iter().any(|parola| coda.contains(parola)) {
            return titolo.get(..*posizione).unwrap_or(titolo).trim().to_owned();
        }
    }
    titolo.trim().to_owned()
}

#[cfg(test)]
mod prove {
    use super::*;

    fn lib(id: i64, artista: &str, titolo: &str, album: &str, durata: i64) -> LibraryTrack {
        LibraryTrack {
            id,
            track_key: TrackKey::compute(TrackKeyInput {
                artist: Some(artista),
                title: Some(titolo),
                album: Some(album),
            })
            .into_string(),
            artist: artista.to_owned(),
            title: titolo.to_owned(),
            duration_ms: durata,
            isrc: None,
        }
    }

    /// Una riga di libreria che porta anche l'ISRC.
    fn lib_isrc(
        id: i64,
        artista: &str,
        titolo: &str,
        album: &str,
        durata: i64,
        isrc: &str,
    ) -> LibraryTrack {
        LibraryTrack {
            isrc: Some(isrc.to_owned()),
            ..lib(id, artista, titolo, album, durata)
        }
    }

    fn sp(artista: &str, titolo: &str, album: &str, durata: Option<u64>) -> SpotifyTrack {
        SpotifyTrack {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            album: Some(album.to_owned()),
            duration_ms: durata,
            ..SpotifyTrack::default()
        }
    }

    #[test]
    fn lisrc_abbina_quel_che_nessun_titolo_farebbe_abbinare() {
        // Titolo diverso, album diverso, artista scritto in un altro modo, e
        // una durata fuori tolleranza: sui tre gradini sotto non si abbinerebbe
        // niente. È la stessa registrazione, e l'ISRC è l'unico a saperlo.
        let libreria = [lib_isrc(
            9,
            "Radiohead",
            "Karma Police",
            "OK Computer",
            264_000,
            "GBAYE9700426",
        )];
        let brani = [SpotifyTrack {
            title: "Karma Police - 2017 Remaster".to_owned(),
            artist: Some("Radiohead & Friends".to_owned()),
            album: Some("OKNOTOK 1997 2017".to_owned()),
            duration_ms: Some(400_000),
            isrc: Some("GBAYE9700426".to_owned()),
            ..SpotifyTrack::default()
        }];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.per_gradino(Gradino::Isrc), 1);
        assert_eq!(piano.abbinati.first().map(|a| a.track_id), Some(9));
    }

    #[test]
    fn lisrc_si_confronta_a_meno_di_trattini_e_maiuscole() {
        let libreria = [lib_isrc(
            4,
            "Tizio",
            "Canzone",
            "Album",
            200_000,
            "gb-aye-97-00426",
        )];
        let brani = [SpotifyTrack {
            title: "Tutt'altro titolo".to_owned(),
            artist: Some("Tutt'altro artista".to_owned()),
            isrc: Some("GBAYE9700426".to_owned()),
            ..SpotifyTrack::default()
        }];
        assert_eq!(
            plan_spotify_import(&brani, &libreria).per_gradino(Gradino::Isrc),
            1
        );
    }

    #[test]
    fn un_isrc_storto_non_abbina_niente() {
        // Il campo `isrc` di un file taggato a mano contiene di tutto. Due
        // valori corti uguali non sono una ragione per abbinare, e su questo
        // gradino non c'è la durata a rimediare.
        let libreria = [lib_isrc(1, "Tizio", "Una", "Album", 200_000, "n/d")];
        let brani = [SpotifyTrack {
            title: "Un'altra".to_owned(),
            artist: Some("Caio".to_owned()),
            isrc: Some("n/d".to_owned()),
            ..SpotifyTrack::default()
        }];
        let piano = plan_spotify_import(&brani, &libreria);
        assert!(piano.abbinati.is_empty(), "{piano:?}");
        assert_eq!(piano.mancanti, vec![0]);
        assert_eq!(
            normalizza_isrc("GBAYE9700426").as_deref(),
            Some("GBAYE9700426")
        );
        assert_eq!(normalizza_isrc("troppo-corto"), None);
        assert_eq!(normalizza_isrc("GBAYE9700426XXXX"), None);
    }

    #[test]
    fn senza_isrc_la_scala_resta_quella_di_prima() {
        // Il gradino zero non deve poter cambiare l'esito quando il campo non
        // c'è — che è il caso di oggi, visto che Pathfinder non lo manda più.
        let libreria = [lib(7, "Radiohead", "Karma Police", "OK Computer", 264_000)];
        let brani = [sp(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            Some(264_066),
        )];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.per_gradino(Gradino::ChiaveEsatta), 1);
        assert_eq!(piano.per_gradino(Gradino::Isrc), 0);
    }

    #[test]
    fn il_primo_gradino_e_la_chiave_esatta() {
        let libreria = [lib(7, "Radiohead", "Karma Police", "OK Computer", 264_000)];
        let brani = [sp(
            "Radiohead",
            "Karma Police",
            "OK Computer",
            Some(264_066),
        )];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(
            piano.abbinati,
            vec![Abbinato {
                indice: 0,
                track_id: 7,
                gradino: Gradino::ChiaveEsatta
            }]
        );
        assert!(piano.mancanti.is_empty());
    }

    #[test]
    fn il_secondo_gradino_perdona_lalbum() {
        // Su Spotify il brano sta sul singolo, sul disco sta sulla raccolta.
        let libreria = [lib(3, "Blur", "Song 2", "Blur", 122_000)];
        let brani = [sp("Blur", "Song 2", "Song 2 - Single", Some(121_000))];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.abbinati.len(), 1);
        assert_eq!(piano.per_gradino(Gradino::ArtistaTitolo), 1);
    }

    #[test]
    fn il_terzo_gradino_toglie_le_decorazioni() {
        let libreria = [lib(
            11,
            "Radiohead",
            "Everything In Its Right Place",
            "Kid A",
            271_000,
        )];
        let brani = [sp(
            "Radiohead",
            "Everything In Its Right Place - 2009 Remaster",
            "Kid A (Collector's Edition)",
            Some(271_400),
        )];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.per_gradino(Gradino::Ripulito), 1);
    }

    #[test]
    fn il_terzo_gradino_toglie_gli_artisti_ospiti() {
        let libreria = [lib(2, "Gorillaz", "Feel Good Inc", "Demon Days", 222_000)];
        let brani = [sp(
            "Gorillaz, De La Soul",
            "Feel Good Inc. (feat. De La Soul)",
            "Demon Days",
            Some(222_640),
        )];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.abbinati.len(), 1, "{piano:?}");
        assert_eq!(piano.abbinati.first().map(|a| a.track_id), Some(2));
    }

    #[test]
    fn la_durata_impedisce_di_prendere_la_versione_dal_vivo() {
        // Stesso artista, stesso titolo dopo la ripulitura: solo i secondi li
        // distinguono. Questo è il caso per cui esiste la tolleranza.
        // Nessuno dei due combacia sui primi due gradini — i titoli in libreria
        // sono decorati entrambi — e sul terzo diventano identici.
        let libreria = [
            lib(
                1,
                "Radiohead",
                "Creep (Live at Glastonbury)",
                "Live",
                402_000,
            ),
            lib(
                2,
                "Radiohead",
                "Creep (Album Version)",
                "Pablo Honey",
                238_000,
            ),
        ];
        let brani = [sp(
            "Radiohead",
            "Creep",
            "Pablo Honey (Remastered)",
            Some(238_640),
        )];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.per_gradino(Gradino::Ripulito), 1);
        assert_eq!(
            piano.abbinati.first().map(|a| a.track_id),
            Some(2),
            "ha preso quella in studio"
        );
    }

    #[test]
    fn nel_dubbio_si_dichiara_mancante() {
        // Due candidati identici dopo la ripulitura, nessuna durata da Spotify:
        // indovinare metterebbe in playlist la canzone sbagliata.
        let libreria = [
            lib(1, "Tizio", "Canzone (Live)", "Uno", 200_000),
            lib(2, "Tizio", "Canzone (Demo)", "Due", 300_000),
        ];
        let brani = [sp("Tizio", "Canzone", "Terzo", None)];
        let piano = plan_spotify_import(&brani, &libreria);
        assert!(piano.abbinati.is_empty());
        assert_eq!(piano.mancanti, vec![0]);
    }

    #[test]
    fn una_durata_troppo_lontana_non_abbina() {
        let libreria = [lib(1, "Tizio", "Canzone (Live)", "Uno", 600_000)];
        let brani = [sp("Tizio", "Canzone - Radio Edit", "Due", Some(180_000))];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(
            piano.mancanti,
            vec![0],
            "sei minuti contro tre non sono la stessa registrazione"
        );
    }

    #[test]
    fn lo_stesso_brano_puo_comparire_due_volte() {
        let libreria = [lib(5, "Tizio", "Canzone", "Album", 200_000)];
        let brani = [
            sp("Tizio", "Canzone", "Album", Some(200_000)),
            sp("Tizio", "Canzone", "Album", Some(200_000)),
        ];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(
            piano.abbinati.len(),
            2,
            "una playlist può ripetere un brano"
        );
        assert_eq!(piano.mancanti.len(), 0);
    }

    #[test]
    fn quel_che_non_ce_resta_mancante() {
        let libreria = [lib(1, "Tizio", "Canzone", "Album", 200_000)];
        let brani = [
            sp("Tizio", "Canzone", "Album", Some(200_000)),
            sp("Caio", "Altra", "Altro", Some(150_000)),
        ];
        let piano = plan_spotify_import(&brani, &libreria);
        assert_eq!(piano.abbinati.len(), 1);
        assert_eq!(piano.mancanti, vec![1]);
    }

    #[test]
    fn su_una_libreria_vuota_manca_tutto() {
        let brani = [sp("Tizio", "Canzone", "Album", Some(200_000))];
        let piano = plan_spotify_import(&brani, &[]);
        assert!(piano.abbinati.is_empty());
        assert_eq!(piano.mancanti, vec![0]);
    }

    #[test]
    fn un_indice_riusato_da_gli_stessi_piani_di_tanti_indici() {
        // È la proprietà su cui poggia l'importazione di un account intero:
        // costruire l'indice una volta e interrogarlo per ogni elenco deve dare
        // esattamente quel che darebbe `plan_spotify_import` chiamata a ripetizione.
        let libreria = [
            lib(1, "Blur", "Song 2", "Blur", 122_000),
            lib(2, "Gorillaz", "Feel Good Inc", "Demon Days", 222_000),
        ];
        let elenchi = [
            vec![sp("Blur", "Song 2", "Song 2 - Single", Some(121_000))],
            vec![
                sp(
                    "Gorillaz, De La Soul",
                    "Feel Good Inc.",
                    "Demon Days",
                    Some(222_640),
                ),
                sp("Nessuno", "Niente", "Nulla", Some(1_000)),
            ],
            vec![],
        ];

        let indice = Indice::nuovo(&libreria);
        for elenco in &elenchi {
            assert_eq!(
                indice.piano(elenco),
                plan_spotify_import(elenco, &libreria),
                "l'indice riusato ha deciso diversamente su {elenco:?}"
            );
        }
    }

    #[test]
    fn le_decorazioni_si_tolgono_ma_i_titoli_veri_no() {
        assert_eq!(
            senza_decorazioni("Feel Good Inc. (feat. De La Soul)"),
            "Feel Good Inc."
        );
        assert_eq!(senza_decorazioni("Karma Police [Live]"), "Karma Police");
        assert_eq!(
            senza_decorazioni("Everything In Its Right Place - 2009 Remaster"),
            "Everything In Its Right Place"
        );
        assert_eq!(
            senza_decorazioni("What's Going On"),
            "What's Going On",
            "niente da togliere"
        );
        assert_eq!(
            senza_decorazioni("Marvin Gaye - What's Going On"),
            "Marvin Gaye - What's Going On",
            "la coda non parla di un'edizione: il trattino fa parte del titolo"
        );
        assert_eq!(
            senza_decorazioni("Jean-Michel"),
            "Jean-Michel",
            "il trattino attaccato non separa niente"
        );
        assert_eq!(
            senza_decorazioni("Canzone (senza chiusura"),
            "Canzone (senza chiusura",
            "una parentesi non chiusa lascia il titolo intero"
        );
    }

    #[test]
    fn il_primo_artista_e_solo_il_primo() {
        assert_eq!(primo_artista("Gorillaz, De La Soul"), "Gorillaz");
        assert_eq!(primo_artista("Calvin Harris & Dua Lipa"), "Calvin Harris");
        assert_eq!(primo_artista("Eminem feat. Rihanna"), "Eminem");
        assert_eq!(primo_artista("Eminem ft. Rihanna"), "Eminem");
        assert_eq!(primo_artista("Queen"), "Queen");
        assert_eq!(
            primo_artista("Simon & Garfunkel"),
            "Simon",
            "è il costo del taglio: «Simon» basta comunque ad abbinare, \
             perché a decidere resta poi la durata"
        );
    }
}
