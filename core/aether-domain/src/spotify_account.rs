//! Un account Spotify intero, e cosa se ne può portare in libreria.
//!
//! # Due strade, un solo valore
//!
//! Quel che sta qui dentro lo riempiono due lettori che non si somigliano per
//! niente. Uno parla con la Web API dopo un consenso OAuth e vede l'account
//! adesso; l'altro apre l'archivio che Spotify manda per posta e vede
//! l'account com'era il giorno in cui l'ha impacchettato. Hanno tempi diversi,
//! guasti diversi, e uno dei due arriva in due pezzi separati da settimane.
//!
//! Producono lo stesso [`AccountSnapshot`], ed è l'unica decisione strutturale
//! di tutta questa funzione. Da qui in giù — l'abbinamento, il piano, la
//! scrittura, la coda di scaricamento — il codice è **uno solo**, e una
//! correzione all'abbinamento dei preferiti non può valere per una strada e non
//! per l'altra.
//!
//! # Perché l'abbinamento non è scritto qui
//!
//! Perché esiste già, in [`crate::abbinamento`], ed è la parte più guardata di
//! tutto il sistema: la scala a quattro gradini con la durata a sorvegliare
//! quelli larghi, e il rifiuto di indovinare quando due candidati si somigliano.
//! Un account intero è tanti elenchi, non un problema diverso. Quel che serviva
//! era poterla chiamare molte volte senza rifare l'indice ogni volta, ed è
//! [`crate::abbinamento::Indice`].
//!
//! # La cronologia, e il millisecondo che sposta tutto
//!
//! L'archivio dice quando un brano **ha smesso** di suonare (`ts` nella
//! cronologia estesa, `endTime` in quella breve). `play_history.played_at` di
//! Aether vuole quando ha **cominciato** — lo dice [`crate::listen::Listen`], e
//! il motivo è che è ciò che il protocollo di scrobbling chiede. Convertire è
//! una sottrazione, e non farla sposterebbe ogni ascolto in avanti della propria
//! durata: invisibile su una riga, evidente su una cronologia di dieci anni,
//! e irreparabile una volta scritto. Vedi [`AscoltoSpotify::iniziato_ms`].

use std::collections::HashSet;

use crate::abbinamento::{Indice, LibraryTrack, PianoAbbinamento};
use crate::esterno::BranoEsterno;
use crate::keys::{TrackKey, TrackKeyInput};
use crate::listen::counts_as_play;

/// Da dove è arrivato uno snapshot.
///
/// Non è un'etichetta per curiosità: le due strade hanno **completezze
/// diverse**, e chi legge il rapporto deve poterlo sapere. L'API non dà la
/// cronologia oltre gli ultimi cinquanta ascolti; l'archivio non dà l'ISRC ma dà
/// dieci anni di ascolti. Dire «importati 3 ascolti» senza dire da dove
/// sembrerebbe un guasto invece che il limite dell'endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provenienza {
    /// La Web API, dopo un consenso OAuth.
    Api,
    /// L'archivio che Spotify manda su richiesta.
    Archivio,
}

impl Provenienza {
    /// Il nome stabile che attraversa l'IPC, in italiano come il resto della UI.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Archivio => "archivio",
        }
    }

    /// Questa strada può portare una cronologia degna di quel nome?
    ///
    /// L'API ne dà cinquanta righe e non di più: sono l'ultima mezza giornata di
    /// chi ascolta molto. Serve a decidere se mostrare «cronologia importata» o
    /// «la cronologia completa sta solo nell'archivio».
    #[must_use]
    pub const fn ha_cronologia_completa(self) -> bool {
        matches!(self, Self::Archivio)
    }
}

/// Una playlist, come Spotify la racconta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistSpotify {
    /// Il nome. È anche ciò da cui si ricava [`crate::keys::PlaylistKey`].
    pub nome: String,
    /// La descrizione, se ce n'è una.
    pub descrizione: Option<String>,
    /// L'identificativo su Spotify.
    ///
    /// Vale la pena portarlo fino al database: è l'unica cosa che resta uguale
    /// quando l'utente rinomina una playlist, e senza di lui la
    /// sincronizzazione successiva ne creerebbe una seconda accanto alla prima.
    pub spotify_id: Option<String>,
    /// I brani, nell'ordine in cui stanno su Spotify.
    pub brani: Vec<BranoEsterno>,
    /// Quanti brani Spotify **dichiara** che ce ne siano.
    ///
    /// Separato da `brani.len()` per la stessa ragione di
    /// [`crate::esterno::ContenutoEsterno::declared_total`]: la differenza fra i
    /// due è l'unico modo di accorgersi che un elenco è arrivato monco.
    pub dichiarati: Option<u32>,
}

impl PlaylistSpotify {
    /// Quanti brani mancano all'appello, se ne mancano.
    ///
    /// `(letti, attesi)`, come [`crate::esterno::ContenutoEsterno::truncation`].
    #[must_use]
    pub fn troncatura(&self) -> Option<(u32, u32)> {
        let attesi = self.dichiarati?;
        let letti = u32::try_from(self.brani.len()).unwrap_or(u32::MAX);
        (letti < attesi).then_some((letti, attesi))
    }
}

/// Un album salvato in «I tuoi album».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlbumSpotify {
    /// Il titolo.
    pub titolo: String,
    /// Chi lo firma.
    pub artista: Option<String>,
    /// L'identificativo su Spotify, per `albums.spotify_id`.
    pub spotify_id: Option<String>,
    /// I brani, se il lettore è riuscito a leggerli.
    ///
    /// Vuoto non è un guasto: l'archivio elenca gli album salvati **senza** le
    /// loro tracce, e chiederle una per una all'API costerebbe una richiesta per
    /// album. Un album senza brani porta comunque il suo `spotify_id`, che è
    /// quel che [`crate::album`] usa per fondere le edizioni.
    pub brani: Vec<BranoEsterno>,
}

/// Un artista seguito.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtistaSpotify {
    /// Il nome, che è anche la chiave primaria di `artists`.
    pub nome: String,
    /// L'identificativo su Spotify, per `artists.spotify_id`.
    pub spotify_id: Option<String>,
}

/// Un ascolto, come lo racconta Spotify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AscoltoSpotify {
    /// Quando il brano ha **smesso** di suonare, in millisecondi dall'epoca.
    ///
    /// La fine e non l'inizio, perché è quel che l'archivio scrive: `ts` è
    /// documentato come «when the track stopped playing», e la cronologia breve
    /// chiama il suo campo `endTime` senza lasciare dubbi. La conversione la fa
    /// [`Self::iniziato_ms`], una volta sola e in un posto solo.
    pub finito_ms: i64,
    /// Quanti millisecondi è stato suonato davvero.
    pub ms_ascoltati: u64,
    /// Che brano era.
    pub brano: BranoEsterno,
}

impl AscoltoSpotify {
    /// Quando l'ascolto è **cominciato**: quel che `play_history.played_at`
    /// vuole.
    ///
    /// Una sottrazione, e tutta la ragione per cui questo tipo non espone
    /// direttamente un `quando_ms` ambiguo.
    ///
    /// Il `.max(0)` non è ridondante rispetto a `saturating_sub`: quella satura
    /// a `i64::MIN`, che è un numero perfettamente valido e perfettamente
    /// sbagliato. Su dati veri la sottrazione non va mai sotto zero — un
    /// timestamp d'epoca è mille miliardi, un ascolto sono milioni — ma un
    /// archivio con un `ms_played` più grande del proprio `ts` esiste, e un
    /// `played_at` negativo si ordinerebbe prima di tutta la cronologia vera
    /// senza che nessuno capisca perché.
    #[must_use]
    pub fn iniziato_ms(&self) -> i64 {
        let durata = i64::try_from(self.ms_ascoltati).unwrap_or(i64::MAX);
        self.finito_ms.saturating_sub(durata).max(0)
    }

    /// Questo ascolto conta come ascolto?
    ///
    /// La regola è **la stessa** di quando suona Aether — metà brano o quattro
    /// minuti, mai sotto i trenta secondi di durata — e viene da
    /// [`crate::listen::counts_as_play`]. Una seconda regola qui vorrebbe dire
    /// che lo stesso brano, sentito allo stesso modo, conta su Spotify e non
    /// conta in Aether: la cronologia importata e quella vera non sarebbero più
    /// confrontabili, e `play_count` sommerebbe due misure diverse.
    ///
    /// # La durata, che Spotify non sempre dice
    ///
    /// `durata_in_libreria` è il ripiego per quando `brano.duration_ms` è
    /// `None` — cioè **sempre**, venendo dall'archivio: né `YourLibrary.json`
    /// né i file di playlist né la cronologia estesa hanno un campo per la
    /// durata. Senza quel ripiego `counts_as_play` ricadrebbe sulla sola soglia
    /// dei quattro minuti, e un archivio di dieci anni entrerebbe in libreria
    /// senza nessuna delle canzoni che durano meno di quattro minuti — cioè
    /// quasi tutte, e senza che niente lo dica: gli ascolti scartati finiscono
    /// in [`ScartiCronologia::troppo_brevi`], che è esattamente dove uno si
    /// aspetta di trovare gli skip.
    ///
    /// Non è un'ipotesi: è quel che è successo la prima volta che
    /// `examples/archivio.rs` ha letto un archivio vero, dove di cinque righe
    /// quattro risultavano «troppo brevi» e una sola era davvero uno skip.
    ///
    /// La durata del brano ritrovato in libreria va bene perché è la stessa
    /// canzone — l'abbinamento non ne accetta un'altra. `0`, quando non c'è
    /// nemmeno quella, lascia decidere alla soglia dei quattro minuti, che è già
    /// come `counts_as_play` tratta una durata sconosciuta.
    #[must_use]
    pub fn conta(&self, durata_in_libreria: Option<u64>) -> bool {
        let durata = self.brano.duration_ms.or(durata_in_libreria).unwrap_or(0);
        counts_as_play(self.ms_ascoltati, durata)
    }
}

/// Tutto quel che si è potuto leggere di un account.
///
/// Ogni elenco può essere vuoto, e vuoto non vuol dire guasto: chi importa il
/// solo archivio della cronologia estesa ha `playlist` e `preferiti` vuoti
/// perché stanno nell'**altro** archivio, non perché qualcosa sia andato storto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSnapshot {
    /// Il nome visualizzato dell'account, per il rapporto.
    pub profilo: Option<String>,
    /// L'identificativo dell'utente su Spotify.
    pub spotify_user_id: Option<String>,
    /// Le playlist, con i loro brani.
    pub playlist: Vec<PlaylistSpotify>,
    /// I «Brani che ti piacciono».
    pub preferiti: Vec<BranoEsterno>,
    /// Gli album salvati.
    pub album: Vec<AlbumSpotify>,
    /// Gli artisti seguiti.
    pub artisti: Vec<ArtistaSpotify>,
    /// La cronologia d'ascolto.
    pub cronologia: Vec<AscoltoSpotify>,
    /// Da quale delle due strade è arrivato.
    pub provenienza: Provenienza,
}

impl AccountSnapshot {
    /// Uno snapshot vuoto, di una provenienza dichiarata.
    ///
    /// Non c'è un `Default`: la provenienza non ha un valore di serie
    /// ragionevole, e sceglierne uno vorrebbe dire che un errore di costruzione
    /// si presenta come un'importazione riuscita da un posto sbagliato.
    #[must_use]
    pub fn vuoto(provenienza: Provenienza) -> Self {
        Self {
            profilo: None,
            spotify_user_id: None,
            playlist: Vec::new(),
            preferiti: Vec::new(),
            album: Vec::new(),
            artisti: Vec::new(),
            cronologia: Vec::new(),
            provenienza,
        }
    }

    /// Non c'è proprio niente da importare?
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        self.playlist.is_empty()
            && self.preferiti.is_empty()
            && self.album.is_empty()
            && self.artisti.is_empty()
            && self.cronologia.is_empty()
    }
}

/// Cosa l'utente ha scelto di portarsi dietro.
///
/// Tutto acceso di serie: chi preme «importa il mio account» vuole il suo
/// account, non un sottoinsieme che deve andare a cercare. Gli interruttori
/// esistono per il caso opposto — chi ha già importato le playlist e vuole solo
/// aggiungere la cronologia arrivata dopo, e a cui rifare tutto non servirebbe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scelte {
    /// Creare o riempire una playlist per ognuna di quelle su Spotify.
    pub playlist: bool,
    /// Segnare come preferiti i «Brani che ti piacciono».
    pub preferiti: bool,
    /// Registrare gli album salvati.
    pub album: bool,
    /// Registrare gli artisti seguiti.
    pub artisti: bool,
    /// Portare dentro la cronologia d'ascolto.
    pub cronologia: bool,
}

impl Default for Scelte {
    fn default() -> Self {
        Self {
            playlist: true,
            preferiti: true,
            album: true,
            artisti: true,
            cronologia: true,
        }
    }
}

/// Il piano di una playlist: come si chiama e cosa ci si ritrova dentro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PianoPlaylist {
    /// La posizione nell'elenco dello snapshot, per ritrovare i brani.
    pub indice: usize,
    /// Il nome della playlist su Spotify.
    pub nome: String,
    /// Quali brani ci sono già e quali no.
    pub piano: PianoAbbinamento,
}

/// Un ascolto della cronologia che è stato ricondotto a un brano in libreria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AscoltoAbbinato {
    /// La riga di libreria.
    pub track_id: i64,
    /// Quando l'ascolto è cominciato: già convertito, pronto per
    /// `play_history.played_at`.
    pub iniziato_ms: i64,
    /// Quanto è stato suonato.
    pub ms_ascoltati: u64,
}

/// Perché una riga di cronologia non è diventata un ascolto.
///
/// Contate e non buttate via in silenzio: «di 41.203 righe ne sono entrate
/// 12.980» è una frase che si può controllare, mentre «importati 12.980
/// ascolti» lascia chi legge senza sapere se ne mancano trenta o trentamila.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScartiCronologia {
    /// Righe uguali a un'altra: l'archivio ripete lo stesso ascolto fra un file
    /// e l'altro.
    pub doppioni: usize,
    /// Ascoltate troppo poco per contare, secondo
    /// [`crate::listen::counts_as_play`].
    pub troppo_brevi: usize,
    /// Brani che in libreria non ci sono.
    ///
    /// **Non** finiscono fra i desiderati: un brano sentito una volta nel 2017
    /// non è una cosa che l'utente ha chiesto di avere. I desiderati nascono
    /// dalle playlist e dai preferiti, dove l'intenzione c'è.
    pub non_in_libreria: usize,
}

impl ScartiCronologia {
    /// Quante righe non sono diventate un ascolto, in tutto.
    #[must_use]
    pub const fn totale(&self) -> usize {
        self.doppioni
            .saturating_add(self.troppo_brevi)
            .saturating_add(self.non_in_libreria)
    }
}

/// Cosa porterebbe l'importazione di un account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountPlan {
    /// Una voce per playlist scelta.
    pub playlist: Vec<PianoPlaylist>,
    /// I preferiti ritrovati in libreria, e quelli no.
    pub preferiti: PianoAbbinamento,
    /// I brani degli album salvati.
    pub album: Vec<PianoPlaylist>,
    /// Gli ascolti pronti da scrivere, in ordine di tempo.
    pub cronologia: Vec<AscoltoAbbinato>,
    /// Quel che della cronologia è rimasto fuori, e perché.
    pub scarti: ScartiCronologia,
    /// Quanti artisti seguiti si porterebbero dentro.
    pub artisti: usize,
}

impl AccountPlan {
    /// Quanti brani in tutto risultano mancanti dalla libreria.
    ///
    /// La cronologia non ci entra: vedi
    /// [`ScartiCronologia::non_in_libreria`].
    #[must_use]
    pub fn mancanti(&self) -> usize {
        self.playlist
            .iter()
            .chain(self.album.iter())
            .map(|p| p.piano.mancanti.len())
            .sum::<usize>()
            .saturating_add(self.preferiti.mancanti.len())
    }

    /// Quanti brani in tutto sono stati ritrovati.
    #[must_use]
    pub fn abbinati(&self) -> usize {
        self.playlist
            .iter()
            .chain(self.album.iter())
            .map(|p| p.piano.abbinati.len())
            .sum::<usize>()
            .saturating_add(self.preferiti.abbinati.len())
    }
}

/// Decide cosa l'importazione di un account porterebbe in libreria.
///
/// Puro: non guarda l'orologio, non tocca il disco, e su una libreria di
/// cinquantamila righe e un account da duecento playlist costruisce **un**
/// indice invece di duecento.
#[must_use]
pub fn plan_account_import(
    snapshot: &AccountSnapshot,
    libreria: &[LibraryTrack],
    scelte: &Scelte,
) -> AccountPlan {
    let indice = Indice::nuovo(libreria);
    let mut piano = AccountPlan::default();

    if scelte.playlist {
        piano.playlist = snapshot
            .playlist
            .iter()
            .enumerate()
            .map(|(indice_playlist, playlist)| PianoPlaylist {
                indice: indice_playlist,
                nome: playlist.nome.clone(),
                piano: indice.piano(&playlist.brani),
            })
            .collect();
    }

    if scelte.preferiti {
        piano.preferiti = indice.piano(&snapshot.preferiti);
    }

    if scelte.album {
        piano.album = snapshot
            .album
            .iter()
            .enumerate()
            .map(|(indice_album, album)| PianoPlaylist {
                indice: indice_album,
                nome: album.titolo.clone(),
                piano: indice.piano(&album.brani),
            })
            .collect();
    }

    if scelte.artisti {
        piano.artisti = snapshot.artisti.len();
    }

    if scelte.cronologia {
        let (ascolti, scarti) = pianifica_cronologia(&snapshot.cronologia, &indice);
        piano.cronologia = ascolti;
        piano.scarti = scarti;
    }

    piano
}

/// Dalla cronologia grezza agli ascolti scrivibili.
///
/// # L'ordine dei tre filtri, e perché non è quello che verrebbe da sé
///
/// Verrebbe da sé mettere prima i due filtri che costano meno — i doppioni sono
/// un confronto di stringhe, la soglia è un'aritmetica — e lasciare in fondo
/// l'abbinamento, che è l'unico a interrogare le tabelle. La prima versione
/// faceva così, ed era sbagliata: **la soglia ha bisogno di sapere quanto dura
/// il brano**, e dall'archivio quel dato non arriva mai. Ricadeva sui quattro
/// minuti e scartava come «troppo brevi» quattro righe su cinque, comprese
/// quelle ascoltate per intero.
///
/// Quindi: doppioni, abbinamento, soglia. Il costo non cambia di molto — un
/// abbinamento è una manciata di ricerche in tabelle di hash, non una scansione
/// — e la deduplicazione resta comunque per prima, che sull'archivio è il filtro
/// che toglie di mezzo più righe.
fn pianifica_cronologia(
    cronologia: &[AscoltoSpotify],
    indice: &Indice<'_>,
) -> (Vec<AscoltoAbbinato>, ScartiCronologia) {
    let mut scarti = ScartiCronologia::default();
    let mut visti: HashSet<(String, i64)> = HashSet::with_capacity(cronologia.len());
    let mut ascolti = Vec::new();

    for ascolto in cronologia {
        let chiave = TrackKey::compute(TrackKeyInput {
            artist: ascolto.brano.artist.as_deref(),
            title: Some(&ascolto.brano.title),
            album: ascolto.brano.album.as_deref(),
        });
        if !visti.insert((chiave.into_string(), ascolto.finito_ms)) {
            scarti.doppioni = scarti.doppioni.saturating_add(1);
            continue;
        }
        let Some((track_id, _)) = indice.abbina(&ascolto.brano) else {
            scarti.non_in_libreria = scarti.non_in_libreria.saturating_add(1);
            continue;
        };
        if !ascolto.conta(indice.durata(track_id)) {
            scarti.troppo_brevi = scarti.troppo_brevi.saturating_add(1);
            continue;
        }
        ascolti.push(AscoltoAbbinato {
            track_id,
            iniziato_ms: ascolto.iniziato_ms(),
            ms_ascoltati: ascolto.ms_ascoltati,
        });
    }

    // In ordine di tempo, e non nell'ordine in cui i file dell'archivio
    // capitavano: `play_history` non lo impone, ma chi va a guardare la tabella
    // con uno strumento qualunque si aspetta una cronologia, e un archivio
    // arriva spezzato in file che non sono per forza ordinati fra loro.
    ascolti.sort_by_key(|a| (a.iniziato_ms, a.track_id));
    (ascolti, scarti)
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::keys::TrackKey;

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

    fn sp(artista: &str, titolo: &str, album: &str, durata: u64) -> BranoEsterno {
        BranoEsterno {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            album: Some(album.to_owned()),
            duration_ms: Some(durata),
            ..BranoEsterno::default()
        }
    }

    fn ascolto(brano: BranoEsterno, finito_ms: i64, ms: u64) -> AscoltoSpotify {
        AscoltoSpotify {
            finito_ms,
            ms_ascoltati: ms,
            brano,
        }
    }

    /// Una libreria di due brani da tre minuti.
    fn libreria() -> Vec<LibraryTrack> {
        vec![
            lib(1, "Blur", "Song 2", "Blur", 180_000),
            lib(2, "Gorillaz", "Feel Good Inc", "Demon Days", 180_000),
        ]
    }

    #[test]
    fn l_ascolto_si_registra_da_quando_e_cominciato_non_da_quando_e_finito() {
        // Il difetto che questa conversione impedisce: senza, ogni ascolto
        // scivolerebbe in avanti della propria durata. Invisibile su una riga,
        // evidente su dieci anni, e irreparabile una volta scritto.
        let a = ascolto(sp("Blur", "Song 2", "Blur", 180_000), 1_000_000, 120_000);
        assert_eq!(a.iniziato_ms(), 880_000);
    }

    #[test]
    fn un_archivio_incoerente_non_scrive_un_istante_negativo() {
        let a = ascolto(sp("Blur", "Song 2", "Blur", 180_000), 5_000, 90_000);
        assert_eq!(a.iniziato_ms(), 0, "satura invece di andare sotto zero");
    }

    #[test]
    fn la_soglia_e_la_stessa_di_quando_suona_aether() {
        // Metà brano. Non una regola nuova: `counts_as_play`, la stessa che
        // decide quando `play_count` sale ascoltando in Aether.
        let brano = sp("Blur", "Song 2", "Blur", 180_000);
        assert!(!ascolto(brano.clone(), 0, 89_999).conta(None));
        assert!(ascolto(brano, 0, 90_000).conta(None));
    }

    #[test]
    fn la_durata_che_l_archivio_non_dice_la_dice_la_libreria() {
        // Il difetto vero, trovato facendo girare `examples/archivio.rs` su un
        // archivio: l'archivio la durata non la scrive **mai**, e senza il
        // ripiego `counts_as_play` ricade sui quattro minuti — cioè scarta come
        // «troppo breve» una canzone di tre minuti ascoltata per intero.
        let senza_durata = BranoEsterno {
            title: "Ignoto".to_owned(),
            ..BranoEsterno::default()
        };
        let tre_minuti = ascolto(senza_durata.clone(), 0, 180_000);
        assert!(
            !tre_minuti.conta(None),
            "senza nessuna durata restano i quattro minuti"
        );
        assert!(
            tre_minuti.conta(Some(180_000)),
            "con la durata di libreria è un brano ascoltato per intero"
        );
        assert!(
            !ascolto(senza_durata, 0, 3_000).conta(Some(180_000)),
            "e uno skip di tre secondi resta uno skip"
        );
    }

    #[test]
    fn quel_che_spotify_dice_batte_il_ripiego() {
        // Il ripiego è un ripiego: quando la durata arriva dalla Web API è
        // quella del brano su Spotify, e va usata quella.
        let brano = sp("Blur", "Song 2", "Blur", 180_000);
        assert!(
            ascolto(brano, 0, 90_000).conta(Some(3_600_000)),
            "mezz'ora di libreria non deve far perdere un ascolto vero"
        );
    }

    #[test]
    fn un_ascolto_dall_archivio_entra_davvero() {
        // La prova dall'alto della stessa cosa: uno snapshot fatto come lo fa
        // `aether-archivio` — nessuna durata da nessuna parte — deve produrre un
        // ascolto, non uno scarto.
        let senza_durata = BranoEsterno {
            title: "Song 2".to_owned(),
            artist: Some("Blur".to_owned()),
            album: Some("Blur".to_owned()),
            ..BranoEsterno::default()
        };
        let snapshot = AccountSnapshot {
            cronologia: vec![ascolto(senza_durata, 1_000_000, 180_000)],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };
        let piano = plan_account_import(&snapshot, &libreria(), &Scelte::default());
        assert_eq!(piano.cronologia.len(), 1);
        assert_eq!(piano.scarti.troppo_brevi, 0);
    }

    #[test]
    fn i_doppioni_dell_archivio_non_diventano_due_ascolti() {
        // L'archivio ripete la stessa riga fra un file e l'altro. Senza la
        // deduplicazione `play_count` conterebbe due volte lo stesso pomeriggio.
        let brano = sp("Blur", "Song 2", "Blur", 180_000);
        let snapshot = AccountSnapshot {
            cronologia: vec![
                ascolto(brano.clone(), 1_000_000, 120_000),
                ascolto(brano.clone(), 1_000_000, 120_000),
                // Stesso brano, un'altra volta: questo è un ascolto vero.
                ascolto(brano, 2_000_000, 120_000),
            ],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let piano = plan_account_import(&snapshot, &libreria(), &Scelte::default());
        assert_eq!(piano.cronologia.len(), 2);
        assert_eq!(piano.scarti.doppioni, 1);
        assert_eq!(piano.scarti.totale(), 1);
    }

    #[test]
    fn quel_che_non_e_in_libreria_si_conta_e_non_si_desidera() {
        // Un brano sentito una volta nel 2017 non è una cosa che l'utente ha
        // chiesto di avere: va contato nel rapporto, non messo in coda.
        let snapshot = AccountSnapshot {
            cronologia: vec![ascolto(
                sp("Chi", "Questo Non Ce L'Ho", "Mai", 180_000),
                1_000_000,
                120_000,
            )],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let piano = plan_account_import(&snapshot, &libreria(), &Scelte::default());
        assert!(piano.cronologia.is_empty());
        assert_eq!(piano.scarti.non_in_libreria, 1);
        assert_eq!(
            piano.mancanti(),
            0,
            "la cronologia non alimenta i desiderati"
        );
    }

    #[test]
    fn la_cronologia_esce_in_ordine_di_tempo() {
        // I file dell'archivio non sono ordinati fra loro.
        let brano = sp("Blur", "Song 2", "Blur", 180_000);
        let snapshot = AccountSnapshot {
            cronologia: vec![
                ascolto(brano.clone(), 3_000_000, 120_000),
                ascolto(brano.clone(), 1_000_000, 120_000),
                ascolto(brano, 2_000_000, 120_000),
            ],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let piano = plan_account_import(&snapshot, &libreria(), &Scelte::default());
        let istanti: Vec<i64> = piano.cronologia.iter().map(|a| a.iniziato_ms).collect();
        assert_eq!(istanti, vec![880_000, 1_880_000, 2_880_000]);
    }

    #[test]
    fn i_preferiti_e_le_playlist_passano_dalla_stessa_scala() {
        let snapshot = AccountSnapshot {
            playlist: vec![PlaylistSpotify {
                nome: "La mia".to_owned(),
                brani: vec![
                    // Album diverso: si abbina sul secondo gradino.
                    sp("Blur", "Song 2", "Song 2 - Single", 181_000),
                    sp("Nessuno", "Niente", "Nulla", 100_000),
                ],
                ..PlaylistSpotify::default()
            }],
            preferiti: vec![sp("Gorillaz", "Feel Good Inc", "Demon Days", 180_000)],
            ..AccountSnapshot::vuoto(Provenienza::Api)
        };

        let piano = plan_account_import(&snapshot, &libreria(), &Scelte::default());
        assert_eq!(piano.playlist.len(), 1);
        assert_eq!(
            piano.playlist.first().map(|p| p.nome.as_str()),
            Some("La mia")
        );
        assert_eq!(piano.abbinati(), 2, "uno in playlist, uno fra i preferiti");
        assert_eq!(piano.mancanti(), 1);
    }

    #[test]
    fn le_scelte_spente_non_producono_niente() {
        // L'interruttore serve a chi ha già importato le playlist e vuole solo
        // aggiungere la cronologia arrivata dopo.
        let brano = sp("Blur", "Song 2", "Blur", 180_000);
        let snapshot = AccountSnapshot {
            playlist: vec![PlaylistSpotify {
                nome: "La mia".to_owned(),
                brani: vec![brano.clone()],
                ..PlaylistSpotify::default()
            }],
            preferiti: vec![brano.clone()],
            artisti: vec![ArtistaSpotify {
                nome: "Blur".to_owned(),
                spotify_id: None,
            }],
            cronologia: vec![ascolto(brano, 1_000_000, 120_000)],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };

        let solo_cronologia = Scelte {
            playlist: false,
            preferiti: false,
            album: false,
            artisti: false,
            cronologia: true,
        };
        let piano = plan_account_import(&snapshot, &libreria(), &solo_cronologia);
        assert!(piano.playlist.is_empty());
        assert!(piano.preferiti.abbinati.is_empty());
        assert_eq!(piano.artisti, 0);
        assert_eq!(piano.cronologia.len(), 1);
    }

    #[test]
    fn un_elenco_monco_si_vede_anche_qui() {
        let playlist = PlaylistSpotify {
            nome: "Lunga".to_owned(),
            brani: vec![BranoEsterno::default(); 142],
            dichiarati: Some(300),
            ..PlaylistSpotify::default()
        };
        assert_eq!(playlist.troncatura(), Some((142, 300)));

        let intera = PlaylistSpotify {
            dichiarati: Some(2),
            brani: vec![BranoEsterno::default(); 2],
            ..PlaylistSpotify::default()
        };
        assert_eq!(intera.troncatura(), None);
    }

    #[test]
    fn la_provenienza_dice_se_la_cronologia_e_completa() {
        // Cinquanta righe dall'API non sono «la cronologia»: dirlo evita che il
        // limite di un endpoint sembri un guasto dell'importazione.
        assert!(Provenienza::Archivio.ha_cronologia_completa());
        assert!(!Provenienza::Api.ha_cronologia_completa());
        assert_eq!(Provenienza::Api.nome(), "api");
        assert_eq!(Provenienza::Archivio.nome(), "archivio");
    }

    #[test]
    fn uno_snapshot_vuoto_lo_dice() {
        let vuoto = AccountSnapshot::vuoto(Provenienza::Api);
        assert!(vuoto.e_vuoto());
        let piano = plan_account_import(&vuoto, &libreria(), &Scelte::default());
        assert_eq!(piano.abbinati(), 0);
        assert_eq!(piano.mancanti(), 0);
        assert_eq!(piano.scarti.totale(), 0);
    }

    #[test]
    fn su_una_libreria_vuota_la_cronologia_non_entra_ma_si_conta() {
        // È la strada di chi importa l'account prima di aver scansionato il
        // disco. Non è un guasto, ma il rapporto deve dire perché non è entrato
        // niente.
        let snapshot = AccountSnapshot {
            cronologia: vec![ascolto(
                sp("Blur", "Song 2", "Blur", 180_000),
                1_000_000,
                120_000,
            )],
            ..AccountSnapshot::vuoto(Provenienza::Archivio)
        };
        let piano = plan_account_import(&snapshot, &[], &Scelte::default());
        assert!(piano.cronologia.is_empty());
        assert_eq!(piano.scarti.non_in_libreria, 1);
    }
}
