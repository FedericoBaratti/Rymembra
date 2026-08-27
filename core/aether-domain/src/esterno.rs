//! Quel che una fonte esterna racconta di un brano, prima che diventi
//! qualcosa di nostro.
//!
//! Sono tipi di trasporto: li riempie chi parla con la rete o con un archivio
//! (`aether-catalogo`, `aether-archivio`), li legge chi decide (`abbinamento`,
//! [`crate::scelta`]) e chi scrive (`aether-app`). Stanno qui, e non nel crate
//! che fa le richieste, per la stessa ragione per cui ci sta
//! [`crate::scan_plan`]: il crate che decide non deve dipendere da quello che
//! parla col mondo, o la decisione diventa improvabile senza una rete.
//!
//! # Perché non si chiamano `Spotify*`
//!
//! Perché li riempiono in più d'uno. Un `SpotifyTrack` che arriva da un
//! concerto dell'Internet Archive è un nome che mente, ed è precisamente il
//! genere di bugia che il resto di questo albero si prende la briga di evitare.
//! [`Fonte`] è il campo che dice la verità, e sta dentro il contenuto invece che
//! nel nome del tipo.
//!
//! # Le tre cose che questo modulo ha imparato
//!
//! [`Fonte`], [`Licenza`] e [`Disponibilita`] sono arrivate insieme, quando
//! l'unica fonte d'audio ha smesso di essere una che dava tutto senza chiedere
//! niente. Con un solo posto da cui prendere i byte, «lo posso prendere?» aveva
//! una risposta sola e implicita, e l'implicito era sbagliato. Con dei cataloghi
//! veri la risposta cambia per ogni brano: uno si scarica, uno si ascolta e
//! basta, uno si compra e nient'altro. Tenerla in un tipo invece che in una
//! consuetudine è ciò che impedisce di scaricare, un giorno, qualcosa che non si
//! poteva.
//!
//! # Perché niente `serde`
//!
//! Il dominio non serializza nulla da sé — `Cargo.toml` lo dice, e `serde` ci
//! sta solo fra le dipendenze di sviluppo per i vettori dorati. I tipi che
//! attraversano il confine con l'interfaccia sono altri, e vivono in
//! `aether-app`: tenerli separati è ciò che permette di cambiare la forma di un
//! messaggio IPC senza toccare la definizione di cos'è un brano.

/// Da dove viene un contenuto importato.
///
/// Sta dentro [`ContenutoEsterno`] e non nel nome dei tipi, e finisce nella
/// colonna `desiderati.source_service`. Serve a due cose che senza di lui non si
/// possono fare: riconoscere una playlist già importata **dalla fonte giusta**
/// (vedi `import_esterno::prepara_playlist`) e sapere, davanti a una riga da
/// procurare, se il file era già stato scelto o va ancora cercato.
///
/// # Chi c'è e chi non c'è
///
/// Ci sono le fonti che *nominano* dei brani e quelle che li *danno*, e non
/// sono le stesse. L'archivio di Spotify e un file di playlist dicono soltanto
/// cosa si vuole; i tre cataloghi dicono anche dove prenderlo, e ciascuno con i
/// suoi limiti — quelli che [`Disponibilita`] scrive brano per brano.
///
/// Non c'è la scansione del disco. Un file che è già sul computer di chi
/// ascolta non è un contenuto esterno: entra in libreria da `library::Scan` con
/// `tracks.source = 'scan'`, e non passa mai di qui.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Fonte {
    /// L'archivio che Spotify manda per posta a chi lo chiede.
    ///
    /// È il valore predefinito perché è quello che hanno le righe scritte prima
    /// che le altre fonti esistessero.
    #[default]
    ArchivioSpotify,
    /// Un file di playlist: M3U, PLS, XSPF.
    FilePlaylist,
    /// L'Internet Archive: Live Music Archive, netlabel, pubblico dominio.
    /// Dice i nomi **e** dà i byte, e permette di tenerli.
    InternetArchive,
    /// Jamendo: musica sotto licenza Creative Commons. Solo ascolto — i loro
    /// termini vietano espressamente la cache e l'accesso fuori linea.
    Jamendo,
    /// Audius: la licenza viaggia col brano, e decide l'artista.
    Audius,
}

impl Fonte {
    /// Il nome stabile che finisce in tabella e attraversa l'IPC.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::ArchivioSpotify => "archivio-spotify",
            Self::FilePlaylist => "file-playlist",
            Self::InternetArchive => "internet-archive",
            Self::Jamendo => "jamendo",
            Self::Audius => "audius",
        }
    }

    /// Come si chiama davanti a chi ascolta.
    #[must_use]
    pub const fn etichetta(self) -> &'static str {
        match self {
            Self::ArchivioSpotify => "Archivio Spotify",
            Self::FilePlaylist => "File di playlist",
            Self::InternetArchive => "Internet Archive",
            Self::Jamendo => "Jamendo",
            Self::Audius => "Audius",
        }
    }

    /// Come si rilegge da una colonna.
    ///
    /// Tutto ciò che non si riconosce vale [`Fonte::ArchivioSpotify`]: è il
    /// valore predefinito della migrazione 7, cioè quello che hanno le righe
    /// scritte quando `spotify` era l'unica parola che quella colonna potesse
    /// contenere. Il vecchio `"spotify"` si rilegge quindi da sé.
    #[must_use]
    pub fn da_testo(grezzo: &str) -> Self {
        match grezzo {
            "file-playlist" => Self::FilePlaylist,
            "internet-archive" => Self::InternetArchive,
            "jamendo" => Self::Jamendo,
            "audius" => Self::Audius,
            _ => Self::ArchivioSpotify,
        }
    }

    /// La fonte può consegnare dei byte da tenere sul disco.
    ///
    /// `false` non vuol dire «non si può ascoltare»: vuol dire che l'unico modo
    /// lecito di ascoltarla è mentre arriva. Vedi [`Disponibilita`], che dice la
    /// stessa cosa brano per brano quando la fonte lascia scegliere all'artista.
    #[must_use]
    pub const fn puo_consegnare(self) -> bool {
        match self {
            Self::InternetArchive | Self::Audius => true,
            Self::ArchivioSpotify | Self::FilePlaylist | Self::Jamendo => false,
        }
    }
}

/// Sotto che licenza sta un brano di un catalogo.
///
/// # Perché è un tipo e non un testo libero
///
/// Perché su questa risposta si decide se scrivere dei byte sul disco di
/// qualcuno, e una stringa non si può interrogare. [`Licenza::Sconosciuta`] è la
/// variante che porta il peso: significa «non lo so», e chi legge deve trattarla
/// come un no, non come un sì.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Licenza {
    /// Fuori dal diritto d'autore.
    PubblicoDominio,
    /// Una delle licenze Creative Commons; il codice è quello canonico
    /// (`by`, `by-sa`, `by-nc-nd`…), come lo dichiara il catalogo.
    CreativeCommons(String),
    /// La licenza aperta di Audius, con quel che l'artista ha scelto.
    OpenMusicLicense,
    /// Distribuzione libera ma non commerciale: è il patto del Live Music
    /// Archive, dove l'artista consente lo scambio e non la vendita.
    LiberaNonCommerciale,
    /// L'artista si tiene tutto: si può nominare, non copiare.
    TutteRiservate,
    /// Il catalogo non lo dice.
    #[default]
    Sconosciuta,
}

impl Licenza {
    /// Il nome stabile che attraversa l'IPC.
    #[must_use]
    pub fn nome(&self) -> String {
        match self {
            Self::PubblicoDominio => "pubblicoDominio".to_owned(),
            Self::CreativeCommons(codice) => format!("cc-{codice}"),
            Self::OpenMusicLicense => "oml".to_owned(),
            Self::LiberaNonCommerciale => "liberaNonCommerciale".to_owned(),
            Self::TutteRiservate => "tutteRiservate".to_owned(),
            Self::Sconosciuta => "sconosciuta".to_owned(),
        }
    }

    /// La licenza permette di tenere una copia sul proprio disco.
    ///
    /// [`Self::Sconosciuta`] risponde `false`, e non è pignoleria: è l'unico
    /// modo di far sì che una fonte che si dimentica di dichiarare la licenza
    /// non diventi, per silenzio, una fonte da cui si scarica tutto.
    #[must_use]
    pub const fn permette_copia(&self) -> bool {
        matches!(
            self,
            Self::PubblicoDominio
                | Self::CreativeCommons(_)
                | Self::OpenMusicLicense
                | Self::LiberaNonCommerciale
        )
    }
}

/// Che cosa si può fare, in pratica, di questo brano.
///
/// È la risposta strutturata alla domanda che l'albero di prima non si faceva
/// mai, perché con un solo posto da cui prendere l'audio la risposta era sempre
/// la stessa. È il prodotto di due cose — cosa la fonte consente in generale
/// ([`Fonte::puo_consegnare`]) e cosa la licenza di quel brano consente in
/// particolare ([`Licenza::permette_copia`]) — e si calcola con [`Self::decidi`]
/// invece che a mano in ogni catalogo, perché due implementazioni della stessa
/// regola darebbero due risposte diverse sullo stesso brano.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Disponibilita {
    /// Si può prendere e tenere.
    Scaricabile,
    /// Si può ascoltare mentre arriva, e nient'altro.
    SoloAscolto,
    /// Nessuna fonte lecita lo dà: resta il negozio.
    #[default]
    SoloAcquisto,
}

impl Disponibilita {
    /// Il nome stabile che attraversa l'IPC.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Scaricabile => "scaricabile",
            Self::SoloAscolto => "soloAscolto",
            Self::SoloAcquisto => "soloAcquisto",
        }
    }

    /// Come si rilegge da una colonna.
    #[must_use]
    pub fn da_testo(grezzo: &str) -> Self {
        match grezzo {
            "scaricabile" => Self::Scaricabile,
            "soloAscolto" => Self::SoloAscolto,
            _ => Self::SoloAcquisto,
        }
    }

    /// Che cosa si può fare, date la fonte e la licenza.
    ///
    /// `consegna_concessa` è il permesso che alcune fonti danno brano per brano
    /// — su Audius lo decide l'artista con un interruttore. Le fonti che non
    /// hanno quell'interruttore passano `true` e lasciano decidere alla licenza.
    #[must_use]
    pub fn decidi(fonte: Fonte, licenza: &Licenza, consegna_concessa: bool) -> Self {
        if !fonte.puo_consegnare() {
            // Una fonte che nomina e basta non rende nessun brano ascoltabile
            // da sé: quel brano si troverà altrove, o si comprerà.
            return if matches!(fonte, Fonte::Jamendo) {
                Self::SoloAscolto
            } else {
                Self::SoloAcquisto
            };
        }
        if consegna_concessa && licenza.permette_copia() {
            Self::Scaricabile
        } else {
            Self::SoloAscolto
        }
    }
}

/// Che cosa nomina un riferimento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenereContenuto {
    /// Un brano singolo.
    Track,
    /// Un album, con i suoi brani in ordine.
    Album,
    /// Una playlist.
    Playlist,
    /// Un artista, di cui si prende la discografia.
    Artist,
    /// Una collezione: un *item* dell'Internet Archive, cioè un concerto o un
    /// disco intero con i suoi file, o una raccolta di un catalogo.
    Collezione,
}

impl GenereContenuto {
    /// La parola che compare nei percorsi e negli indirizzi.
    ///
    /// In inglese perché è il protocollo, non il nostro vocabolario: questa
    /// stringa va dentro un indirizzo che deve arrivare al catalogo così com'è.
    #[must_use]
    pub const fn path_word(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Artist => "artist",
            Self::Collezione => "details",
        }
    }

    /// Il nome stabile che attraversa l'IPC, in italiano come il resto della UI.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Track => "brano",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Artist => "artista",
            Self::Collezione => "collezione",
        }
    }

    /// Riconosce la parola del protocollo.
    #[must_use]
    pub fn from_path_word(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "track" | "tracks" => Some(Self::Track),
            "album" | "albums" => Some(Self::Album),
            "playlist" | "playlists" => Some(Self::Playlist),
            "artist" | "artists" => Some(Self::Artist),
            "details" | "collection" => Some(Self::Collezione),
            _ => None,
        }
    }
}

/// Da quale livello del lettore è arrivata una risposta.
///
/// Non è statistica: è la differenza fra un elenco completo e uno che potrebbe
/// non esserlo, e va portata fino all'utente. Vedi `catalogo.listaMonca` nel
/// catalogo degli errori.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Livello {
    /// Lo zip che Spotify manda per posta. Completo per definizione: è tutto
    /// quel che c'è, e non c'è una pagina dopo.
    Archivio,
    /// Un file di playlist letto dal disco.
    FilePlaylist,
    /// L'Internet Archive: la ricerca avanzata più i metadati dell'item.
    ArchivioOrg,
    /// L'API pubblica di Jamendo.
    Jamendo,
    /// L'API pubblica di Audius.
    Audius,
}

impl Livello {
    /// Il nome stabile che attraversa l'IPC.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Archivio => "archivio",
            Self::FilePlaylist => "file-playlist",
            Self::ArchivioOrg => "archive.org",
            Self::Jamendo => "jamendo",
            Self::Audius => "audius",
        }
    }
}

/// A che punto è la lettura di un elenco.
///
/// # Perché esiste
///
/// Perché la lettura era **muta**: la si avviava e si aspettava, e un elenco da
/// trecento brani sono decine di richieste dietro un «Lettura in corso…» che non
/// cambia mai. Chi aspettava non poteva distinguere una lettura lunga da una
/// impiantata, e l'unica risposta possibile era chiudere e riprovare — cioè
/// rifare da capo la cosa che stava quasi finendo.
///
/// # Perché le pagine e non i secondi
///
/// I secondi li può contare chi disegna, e non vogliono dire niente: due letture
/// della stessa durata hanno numeri di pagine diversissimi. L'unica domanda che
/// ci si fa aspettando è «sta andando avanti?», e una pagina in più è
/// esattamente quella risposta.
///
/// # Perché `pagine` è opzionale
///
/// Perché quante saranno **non sempre si sa**: la ricerca dell'Internet Archive
/// dichiara `numFound` nella prima risposta, un file di playlist non ha pagine
/// affatto. `None` è il permesso di disegnare una barra indeterminata invece di
/// una previsione, ed è il motivo per cui non si è messo `0`: uno zero è un
/// numero, e un numero è una promessa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvanzamentoLettura {
    /// Quale livello sta rispondendo.
    pub livello: Livello,
    /// Quante pagine sono state lette, contando da 1.
    pub pagina: u32,
    /// Quante ne saranno in tutto, quando la fonte lo dichiara.
    pub pagine: Option<u32>,
    /// Quanti brani sono in mano finora.
    pub brani: u32,
}

/// Un brano, come una fonte esterna lo descrive.
///
/// Quasi tutto è opzionale perché quasi tutto lo è davvero: un item
/// dell'Internet Archive non dà l'anno né il numero di traccia, e certi non
/// danno nemmeno l'interprete. Il titolo no — un brano senza titolo non è un
/// brano, ed è l'unica cosa che ogni fonte restituisce sempre.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranoEsterno {
    /// Il titolo.
    pub title: String,
    /// Gli interpreti, uniti da virgole.
    pub artist: Option<String>,
    /// L'album di provenienza.
    pub album: Option<String>,
    /// L'interprete dell'album, che raggruppa le compilation sotto un nome solo.
    pub album_artist: Option<String>,
    /// Il numero del disco, per gli album multipli.
    pub disc_number: Option<u32>,
    /// Il numero di traccia.
    pub track_number: Option<u32>,
    /// L'anno, ricavato dalla data di pubblicazione.
    pub year: Option<i32>,
    /// La durata in millisecondi.
    pub duration_ms: Option<u64>,
    /// Il codice ISRC.
    ///
    /// È l'unica identità davvero affidabile fra cataloghi diversi, ed è il
    /// gradino zero dell'abbinamento: stessa registrazione senza guardare i
    /// nomi. L'archivio di Spotify lo porta, i cataloghi liberi quasi mai.
    pub isrc: Option<String>,
    /// L'identificativo Spotify del brano, quando viene dall'archivio.
    pub spotify_track_id: Option<String>,
    /// L'identificativo Spotify dell'album, quando viene dall'archivio.
    ///
    /// Vale la pena portarlo fino al database: [`crate::album`] lo usa già come
    /// aggancio autorevole per fondere le edizioni dello stesso album.
    pub spotify_album_id: Option<String>,
    /// L'indirizzo della copertina.
    pub cover_url: Option<String>,
    /// L'indirizzo da cui prendere l'audio, quando si sa già.
    ///
    /// Da un archivio o da un file di playlist è sempre `None`: là si sa *cosa*
    /// si vuole e non dove prenderlo, e trovarlo è tutto il mestiere di
    /// [`crate::scelta`]. Da un catalogo è pieno, perché il brano **è** quel
    /// file: chi ha importato ha visto quell'elenco e da lì ha deciso.
    ///
    /// La differenza non è cosmetica. Finisce in `desiderati.fonte_url`, e la
    /// coda che procura lo legge per **saltare la ricerca**: senza, si
    /// rifarebbe da capo una scelta già fatta, con la possibilità concreta di
    /// prendere una registrazione diversa da quella che si aveva davanti.
    pub fonte_url: Option<String>,
    /// Sotto che licenza sta, quando la fonte lo dichiara.
    pub licenza: Licenza,
    /// Che cosa se ne può fare.
    pub disponibilita: Disponibilita,
    /// La pagina pubblica del brano presso la fonte.
    ///
    /// Non è ornamento: i termini di Jamendo e di Audius **obbligano** a
    /// mostrare un rimando alla pagina d'origine accanto al brano. Un campo che
    /// non si riempie è un obbligo che non si rispetta.
    pub pagina_url: Option<String>,
}

/// Un contenuto risolto: la playlist, l'album o il brano, con i suoi brani.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContenutoEsterno {
    /// Da quale fonte.
    pub fonte: Fonte,
    /// Cosa è.
    pub kind: GenereContenuto,
    /// L'identificativo del contenitore presso quella fonte.
    pub id: String,
    /// Il nome della playlist, dell'album o del brano.
    pub title: String,
    /// Chi lo firma: l'interprete dell'album, o chi possiede la playlist.
    pub author: Option<String>,
    /// La copertina del contenitore.
    pub cover_url: Option<String>,
    /// I brani, nell'ordine in cui stanno lassù.
    pub tracks: Vec<BranoEsterno>,
    /// Quanti brani la fonte **dichiara** che ce ne siano.
    ///
    /// Separato da `tracks.len()` di proposito: la differenza fra i due è
    /// l'unico modo di accorgersi che un elenco è arrivato monco, ed è
    /// precisamente il guasto che non deve poter passare in silenzio. `None`
    /// quando il livello che ha risposto non dichiara un totale.
    pub declared_total: Option<u32>,
    /// Da dove è arrivato.
    pub source: Livello,
}

impl ContenutoEsterno {
    /// Quanti brani mancano all'appello, se ne mancano.
    ///
    /// Restituisce `(letti, attesi)` — i due numeri che servono a dire «142 su
    /// 300» invece di «alcuni brani». `None` quando l'elenco è completo, quando
    /// la fonte non ha dichiarato un totale, o quando ne sono arrivati **più**
    /// di quanti dichiarati: succede sugli elenchi modificati fra una pagina e
    /// l'altra, e non è un ammanco.
    #[must_use]
    pub fn truncation(&self) -> Option<(u32, u32)> {
        let attesi = self.declared_total?;
        let letti = u32::try_from(self.tracks.len()).unwrap_or(u32::MAX);
        (letti < attesi).then_some((letti, attesi))
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn contenuto(quanti: usize, dichiarati: Option<u32>) -> ContenutoEsterno {
        ContenutoEsterno {
            fonte: Fonte::ArchivioSpotify,
            kind: GenereContenuto::Playlist,
            id: "37i9dQZF1DXcBWIGoYBM5M".to_owned(),
            title: "Prova".to_owned(),
            author: None,
            cover_url: None,
            tracks: vec![BranoEsterno::default(); quanti],
            declared_total: dichiarati,
            source: Livello::Archivio,
        }
    }

    #[test]
    fn un_elenco_completo_non_e_monco() {
        assert_eq!(contenuto(10, Some(10)).truncation(), None);
        assert_eq!(contenuto(10, None).truncation(), None);
    }

    #[test]
    fn un_elenco_monco_dice_i_due_numeri() {
        assert_eq!(contenuto(142, Some(300)).truncation(), Some((142, 300)));
    }

    #[test]
    fn piu_brani_del_dichiarato_non_e_un_ammanco() {
        assert_eq!(contenuto(11, Some(10)).truncation(), None);
    }

    #[test]
    fn i_nomi_delle_fonti_fanno_il_giro() {
        for fonte in [
            Fonte::ArchivioSpotify,
            Fonte::FilePlaylist,
            Fonte::InternetArchive,
            Fonte::Jamendo,
            Fonte::Audius,
        ] {
            assert_eq!(Fonte::da_testo(fonte.nome()), fonte);
        }
    }

    #[test]
    fn il_vecchio_spotify_si_rilegge_come_archivio() {
        // La migrazione 7 ha scritto `'spotify'` in quella colonna, e quelle
        // righe devono continuare a voler dire qualcosa.
        assert_eq!(Fonte::da_testo("spotify"), Fonte::ArchivioSpotify);
        assert_eq!(Fonte::da_testo(""), Fonte::ArchivioSpotify);
        assert_eq!(Fonte::da_testo("youtube"), Fonte::ArchivioSpotify);
    }

    #[test]
    fn una_licenza_sconosciuta_non_permette_di_copiare() {
        // La regola che impedisce a una fonte distratta di diventare, per
        // silenzio, una fonte da cui si scarica tutto.
        assert!(!Licenza::Sconosciuta.permette_copia());
        assert!(!Licenza::TutteRiservate.permette_copia());
        assert!(Licenza::PubblicoDominio.permette_copia());
        assert!(Licenza::CreativeCommons("by-nc".to_owned()).permette_copia());
        assert!(Licenza::LiberaNonCommerciale.permette_copia());
    }

    #[test]
    fn jamendo_si_ascolta_e_non_si_tiene() {
        // Anche con la licenza più permissiva: sono i loro termini a vietare la
        // cache, non la licenza del brano.
        assert_eq!(
            Disponibilita::decidi(
                Fonte::Jamendo,
                &Licenza::CreativeCommons("by".to_owned()),
                true
            ),
            Disponibilita::SoloAscolto
        );
    }

    #[test]
    fn larchivio_di_spotify_nomina_e_basta() {
        assert_eq!(
            Disponibilita::decidi(Fonte::ArchivioSpotify, &Licenza::Sconosciuta, true),
            Disponibilita::SoloAcquisto
        );
    }

    #[test]
    fn su_audius_lartista_puo_dire_di_no_anche_con_una_licenza_aperta() {
        assert_eq!(
            Disponibilita::decidi(Fonte::Audius, &Licenza::OpenMusicLicense, true),
            Disponibilita::Scaricabile
        );
        assert_eq!(
            Disponibilita::decidi(Fonte::Audius, &Licenza::OpenMusicLicense, false),
            Disponibilita::SoloAscolto
        );
    }

    #[test]
    fn il_giro_dei_generi_passa_dalla_parola_del_protocollo() {
        for genere in [
            GenereContenuto::Track,
            GenereContenuto::Album,
            GenereContenuto::Playlist,
            GenereContenuto::Artist,
            GenereContenuto::Collezione,
        ] {
            assert_eq!(
                GenereContenuto::from_path_word(genere.path_word()),
                Some(genere)
            );
        }
        assert_eq!(
            GenereContenuto::from_path_word("TRACK"),
            Some(GenereContenuto::Track)
        );
        assert_eq!(GenereContenuto::from_path_word("episode"), None);
    }
}
