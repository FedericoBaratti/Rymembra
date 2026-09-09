//! Da quel che il file dichiara e da quel che il percorso lascia intendere, i
//! metadati di un brano — e da dove viene ciascuno.
//!
//! # L'idea, in una riga
//!
//! **Un campo di metadati porta con sé la propria provenienza.**
//!
//! Fin qui Aether salvava una stringa e più tardi *ri-indovinava* se fosse vera:
//! `crate::enrich::titolo_di_ripiego` deduce che un titolo è un ripiego
//! confrontandolo con la radice del nome del file, perché è l'unico indizio che
//! gli resta. Funziona, ed è una supposizione fatta a valle su un dato che a
//! monte si conosceva con certezza.
//!
//! Registrata [`Origine`] al momento della lettura, tutto il resto smette di
//! indovinare:
//!
//! - l'arricchimento sa **cosa può sovrascrivere** — un valore dedotto sì, uno
//!   scritto da una persona mai;
//! - l'interfaccia sa **cosa mostrare come incerto**, e accanto a un testo
//!   riparato può far vedere com'era prima;
//! - l'utente sa **cosa correggere**, invece di cercarlo a occhio.
//!
//! # La scala di prudenza
//!
//! È la stessa di [`crate::enrich::plan_write`], applicata un passo prima:
//!
//! 1. un tag vero vince su tutto;
//! 2. un tag **riparato** vince sul percorso — il file lo diceva, si è solo
//!    corretta la codifica con cui era stato letto;
//! 3. un tag assente o segnaposto ripiega sull'indizio del percorso;
//! 4. i due segnaposto di [`crate::album`] restano l'ultima spiaggia, quando né
//!    il file né la cartella dicono niente.
//!
//! Niente di dedotto sovrascrive mai qualcosa che una persona ha scritto. E
//! niente di tutto questo tocca i file dell'utente: sono valori di libreria, e
//! il file resta come sta.

use crate::album::{UNKNOWN_ALBUM, UNKNOWN_ARTIST};
use crate::codifica::ripara;
use crate::indizi::{Indizi, e_segnaposto};
use crate::text::collapse_whitespace;

/// Da dove viene il valore di un campo.
///
/// Il valore predefinito è [`Origine::Tag`], ed è la lettura più prudente: è
/// quella che l'arricchimento non si sente in diritto di sovrascrivere. Serve
/// alle righe scritte prima che questa provenienza esistesse, che non dicono
/// niente e non devono per questo diventare riscrivibili.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origine {
    /// Lo diceva il file, e si è letto così com'era.
    #[default]
    Tag,
    /// Lo diceva il file, ma con una codifica sbagliata: vedi [`crate::codifica`].
    TagRiparato,
    /// Lo dice la cartella o il nome del file: vedi [`crate::indizi`].
    Percorso,
    /// Non lo diceva nessuno: è un segnaposto.
    Ripiego,
    /// L'ha deciso l'utente, e non si tocca più.
    Manuale,
}

impl Origine {
    /// Il nome che finisce nel database e nell'interfaccia.
    ///
    /// Testo e non un intero, come per `download_state` e `enrich_source`: una
    /// riga letta a mano con `sqlite3` deve dire cosa è successo senza una
    /// tabella di corrispondenze da un'altra parte.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::TagRiparato => "tag-riparato",
            Self::Percorso => "percorso",
            Self::Ripiego => "ripiego",
            Self::Manuale => "manuale",
        }
    }

    /// L'origine da come è scritta, per rileggerla dal database.
    ///
    /// Un nome che non si riconosce vale [`Origine::Tag`]: è la lettura più
    /// prudente, perché è quella che l'arricchimento non si sente in diritto di
    /// sovrascrivere.
    #[must_use]
    pub fn da_str(nome: &str) -> Self {
        match nome {
            "tag-riparato" => Self::TagRiparato,
            "percorso" => Self::Percorso,
            "ripiego" => Self::Ripiego,
            "manuale" => Self::Manuale,
            _ => Self::Tag,
        }
    }

    /// L'arricchimento può sostituire questo valore senza chiedere?
    ///
    /// Sì per quel che abbiamo dedotto noi, mai per quel che ha scritto una
    /// persona. Un tag riparato **è** sostituibile: la codifica l'abbiamo
    /// indovinata noi, e una corrispondenza provata ne sa di più.
    #[must_use]
    pub const fn sostituibile(self) -> bool {
        matches!(self, Self::TagRiparato | Self::Percorso | Self::Ripiego)
    }
}

/// Quanto ci si può fidare dei metadati di un brano.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Salute {
    /// Ogni campo viene dai tag del file, o dall'utente.
    Ok,
    /// Almeno un campo è stato dedotto o riparato.
    Dedotto,
    /// Il file non si è potuto leggere: c'è solo quel che dice il percorso.
    Degradato,
}

impl Salute {
    /// Il nome che finisce in `tracks.meta_salute`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Dedotto => "dedotto",
            Self::Degradato => "degradato",
        }
    }

    /// La salute da come è scritta. Un valore ignoto vale [`Salute::Ok`], che è
    /// quel che dice una libreria scritta prima che questa colonna esistesse.
    #[must_use]
    pub fn da_str(nome: &str) -> Self {
        match nome {
            "dedotto" => Self::Dedotto,
            "degradato" => Self::Degradato,
            _ => Self::Ok,
        }
    }
}

/// Che cosa non andava nei metadati di un brano.
///
/// Un elenco chiuso e con un nome stabile per voce, come
/// [`crate::scan_plan::SkipReason`]: serve a dire all'utente **perché** un brano
/// è finito fra quelli da guardare, e una frase composta al volo nell'interfaccia
/// sarebbe una seconda verità da tenere allineata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Problema {
    /// Il file non si è potuto leggere: non c'è nemmeno la durata.
    Illeggibile,
    /// Il file si legge ma non ha nessun tag.
    SenzaTag,
    /// Almeno un campo era scritto in una codifica sbagliata.
    Mojibake,
    /// L'interprete c'era, ma diceva «non lo so».
    SegnapostoArtista,
    /// L'album c'era, ma diceva «non lo so».
    SegnapostoAlbum,
    /// Il titolo c'era, ma diceva «non lo so».
    SegnapostoTitolo,
    /// C'era una copertina incorporata e non si è potuta decodificare.
    CopertinaIlleggibile,
}

impl Problema {
    /// Il nome che finisce in `tracks.meta_problemi` e nell'interfaccia.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Illeggibile => "illeggibile",
            Self::SenzaTag => "senza-tag",
            Self::Mojibake => "mojibake",
            Self::SegnapostoArtista => "segnaposto-artista",
            Self::SegnapostoAlbum => "segnaposto-album",
            Self::SegnapostoTitolo => "segnaposto-titolo",
            Self::CopertinaIlleggibile => "copertina-illeggibile",
        }
    }
}

/// Un campo ricostruito: il valore, da dove viene, e com'era prima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campo {
    /// Il valore da mettere in libreria.
    pub valore: String,
    /// Da dove viene.
    pub origine: Origine,
    /// Quel che il tag diceva prima della riparazione.
    ///
    /// Pieno solo per [`Origine::TagRiparato`], ed è ciò che permette
    /// all'interfaccia di mostrare «prima → dopo» invece di chiedere all'utente
    /// di fidarsi.
    pub prima: Option<String>,
}

impl Campo {
    /// Un campo che viene dai tag così com'erano.
    fn dal_tag(valore: String) -> Self {
        Self {
            valore,
            origine: Origine::Tag,
            prima: None,
        }
    }
}

/// Un numero ricostruito. Non ha bisogno di `prima`: non si ripara un intero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numero<T> {
    /// Il valore.
    pub valore: T,
    /// Da dove viene.
    pub origine: Origine,
}

/// I tag come il file li dichiara, prima di qualunque ripiego.
///
/// Un tipo di prestiti e non di `String`: chi chiama ha già letto il file e non
/// deve pagare una copia di ogni campo per porre una domanda.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagsGrezzi<'a> {
    /// Titolo.
    pub titolo: Option<&'a str>,
    /// Interprete.
    pub artista: Option<&'a str>,
    /// Album.
    pub album: Option<&'a str>,
    /// Artista dell'album.
    pub album_artist: Option<&'a str>,
    /// Genere.
    pub genere: Option<&'a str>,
    /// Anno.
    pub anno: Option<i32>,
    /// Numero di traccia.
    pub traccia: Option<u32>,
    /// Numero di disco.
    pub disco: Option<u32>,
}

/// I metadati di un brano, con la provenienza di ognuno.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ricostruzione {
    /// Titolo. Non è mai vuoto: lo schema lo vuole `NOT NULL`.
    pub titolo: Campo,
    /// Interprete. Non è mai vuoto.
    pub artista: Campo,
    /// Album. Non è mai vuoto.
    pub album: Campo,
    /// Artista dell'album, quando il file lo dichiara o la cartella lo dice.
    pub album_artist: Option<Campo>,
    /// Genere: solo dai tag, perché nessuna cartella lo sa.
    pub genere: Option<Campo>,
    /// Anno.
    pub anno: Option<Numero<i32>>,
    /// Numero di traccia.
    pub traccia: Option<Numero<u32>>,
    /// Numero di disco.
    pub disco: Option<Numero<u32>>,
    /// Quanto ci si può fidare dell'insieme.
    pub salute: Salute,
    /// Perché, in ordine e senza ripetizioni.
    pub problemi: Vec<Problema>,
}

impl Ricostruzione {
    /// Marca la ricostruzione come degradata, con la ragione.
    ///
    /// Serve a chi ha provato ad aprire il file e non ci è riuscito: i campi
    /// restano quelli che il percorso ha dato, e la riga entra in libreria lo
    /// stesso. Un brano che non compare è un brano che l'utente non sa di avere
    /// perso; un brano che compare marcato è un brano su cui può decidere.
    #[must_use]
    pub fn degradata(mut self, perche: Problema) -> Self {
        self.salute = Salute::Degradato;
        if !self.problemi.contains(&perche) {
            self.problemi.push(perche);
        }
        self
    }

    /// Aggiunge un problema senza cambiare la salute.
    pub fn segnala(&mut self, problema: Problema) {
        if !self.problemi.contains(&problema) {
            self.problemi.push(problema);
        }
        if self.salute == Salute::Ok {
            self.salute = Salute::Dedotto;
        }
    }
}

/// Il valore di un tag, ripulito e riparato, se dice qualcosa.
///
/// Restituisce `None` in due casi che vanno tenuti distinti da chi chiama: il
/// tag non c'è, e il tag c'è ma è un segnaposto. Il secondo è un **problema** da
/// segnalare, il primo è solo un'assenza.
fn dal_tag(valore: Option<&str>) -> Option<Campo> {
    let grezzo = collapse_whitespace(valore?);
    if grezzo.is_empty() || e_segnaposto(Some(&grezzo)) {
        return None;
    }
    match ripara(&grezzo) {
        Some(riparato) => Some(Campo {
            valore: riparato.testo,
            origine: Origine::TagRiparato,
            prima: Some(riparato.originale),
        }),
        None => Some(Campo::dal_tag(grezzo)),
    }
}

/// Il tag c'era, ma diceva «non lo so»?
fn e_un_segnaposto(valore: Option<&str>) -> bool {
    valore.is_some_and(|v| !v.trim().is_empty() && e_segnaposto(Some(v)))
}

/// Un campo dal percorso, quando il tag non ha detto niente.
fn dal_percorso(indizio: Option<&String>) -> Option<Campo> {
    let valore = collapse_whitespace(indizio?);
    if valore.is_empty() {
        return None;
    }
    Some(Campo {
        valore,
        origine: Origine::Percorso,
        prima: None,
    })
}

/// L'ultima spiaggia: un segnaposto, dichiarato come tale.
fn ripiego(valore: &str) -> Campo {
    Campo {
        valore: valore.to_owned(),
        origine: Origine::Ripiego,
        prima: None,
    }
}

/// Un numero dal tag se c'è, altrimenti dall'indizio del percorso.
fn numero<T: Copy>(dal_tag: Option<T>, dal_percorso: Option<T>) -> Option<Numero<T>> {
    dal_tag
        .map(|valore| Numero {
            valore,
            origine: Origine::Tag,
        })
        .or_else(|| {
            dal_percorso.map(|valore| Numero {
                valore,
                origine: Origine::Percorso,
            })
        })
}

/// I metadati di un brano, dai suoi tag e dal suo percorso.
///
/// `radice_del_file` è il nome del file senza estensione
/// ([`crate::paths::file_stem`]): è l'ultima cosa che si sa di un brano di cui
/// non si sa niente, e resta il ripiego del titolo com'era prima di questo
/// modulo — meglio `gd1977-05-08d1t04` che «senza titolo», perché almeno
/// identifica il file.
///
/// ```
/// use aether_domain::indizi::Indizi;
/// use aether_domain::ricostruzione::{Origine, Salute, TagsGrezzi, ricostruisci};
///
/// // Un file senza tag, dentro una cartella che dice tutto.
/// let indizi = Indizi {
///     artista: Some("Pink Floyd".to_owned()),
///     album: Some("The Dark Side of the Moon".to_owned()),
///     titolo: Some("Speak to Me".to_owned()),
///     anno: Some(1973),
///     traccia: Some(1),
///     artista_da_cartella: true,
///     ..Indizi::default()
/// };
/// let r = ricostruisci(&TagsGrezzi::default(), &indizi, "01 - Speak to Me");
/// assert_eq!(r.artista.valore, "Pink Floyd");
/// assert_eq!(r.artista.origine, Origine::Percorso);
/// assert_eq!(r.salute, Salute::Dedotto);
///
/// // Un tag vero non si tocca, nemmeno quando la cartella dice altro.
/// let tags = TagsGrezzi { artista: Some("Roger Waters"), ..TagsGrezzi::default() };
/// let r = ricostruisci(&tags, &indizi, "01 - Speak to Me");
/// assert_eq!(r.artista.valore, "Roger Waters");
/// assert_eq!(r.artista.origine, Origine::Tag);
/// ```
#[must_use]
pub fn ricostruisci(
    tags: &TagsGrezzi<'_>,
    indizi: &Indizi,
    radice_del_file: &str,
) -> Ricostruzione {
    let titolo = dal_tag(tags.titolo)
        .or_else(|| dal_percorso(indizi.titolo.as_ref()))
        // La radice del nome del file non è un ripiego elegante, ma è l'unica
        // cosa che identifica quel brano quando nessuno dice niente. Se è vuota
        // pure lei — un file che si chiama solo `.mp3` — si prende il
        // segnaposto, perché lo schema vuole `NOT NULL` e una stringa vuota in
        // libreria è una riga che non si può nemmeno cercare.
        .unwrap_or_else(|| {
            let radice = collapse_whitespace(radice_del_file);
            if radice.is_empty() {
                ripiego(UNKNOWN_ALBUM)
            } else {
                ripiego(radice.as_str())
            }
        });

    let artista = dal_tag(tags.artista)
        .or_else(|| dal_percorso(indizi.artista.as_ref()))
        .unwrap_or_else(|| ripiego(UNKNOWN_ARTIST));

    let album = dal_tag(tags.album)
        .or_else(|| dal_percorso(indizi.album.as_ref()))
        .unwrap_or_else(|| ripiego(UNKNOWN_ALBUM));

    // L'artista dell'album si deduce da una cartella, e a due condizioni.
    //
    // **Solo da una cartella, mai dal nome del file.** È il campo che
    // `rebuild_aggregates` usa per raggruppare un disco in una uscita sola: su
    // una raccolta, scriverci l'interprete della singola traccia spezzerebbe
    // l'album in tante uscite quante sono le tracce.
    //
    // **Solo se anche l'interprete è dedotto.** Questa è la condizione che a
    // prima vista non serve, e serve: `album_artist` ha la precedenza su
    // `artist` in tutta la libreria — è `ARTISTA_EFFETTIVO`, in `library.rs` —
    // quindi riempirlo dalla cartella su un file che l'interprete ce l'ha
    // **scritto nei tag** vuol dire far vincere il nome della cartella su
    // quello vero. Una libreria in `Cure/Disintegration/` con i tag a posto
    // mostrerebbe l'artista «Cure» invece di «The Cure», e non perché manchi
    // qualcosa: perché abbiamo dedotto sopra a un dato che c'era.
    //
    // Quando l'interprete è dedotto, invece, i due vengono dalla stessa
    // cartella e dicono per forza la stessa cosa.
    let album_artist = dal_tag(tags.album_artist).or_else(|| {
        (indizi.artista_da_cartella && artista.origine == Origine::Percorso)
            .then(|| dal_percorso(indizi.artista.as_ref()))
            .flatten()
    });

    let genere = dal_tag(tags.genere);

    let mut problemi = Vec::new();
    if tags.titolo.is_none() && tags.artista.is_none() && tags.album.is_none() {
        problemi.push(Problema::SenzaTag);
    }
    let riparato = [&titolo, &artista, &album]
        .into_iter()
        .chain(album_artist.iter())
        .chain(genere.iter())
        .any(|campo| campo.origine == Origine::TagRiparato);
    if riparato {
        problemi.push(Problema::Mojibake);
    }
    if e_un_segnaposto(tags.artista) {
        problemi.push(Problema::SegnapostoArtista);
    }
    if e_un_segnaposto(tags.album) {
        problemi.push(Problema::SegnapostoAlbum);
    }
    if e_un_segnaposto(tags.titolo) {
        problemi.push(Problema::SegnapostoTitolo);
    }

    // La salute guarda i tre campi obbligatori, non tutti. Dedurre
    // `album_artist` da una cartella d'artista è quasi sempre giusto ed è quasi
    // sempre possibile: contarlo qui metterebbe nell'elenco «da sistemare»
    // mezza libreria taggata bene, e un elenco che contiene tutto non si guarda.
    // Un guasto di codifica in quel campo continua invece a segnalarsi, perché
    // finisce in `problemi` insieme a tutti gli altri.
    let dedotto = [&titolo, &artista, &album]
        .into_iter()
        .any(|campo| campo.origine != Origine::Tag);

    Ricostruzione {
        titolo,
        artista,
        album,
        album_artist,
        genere,
        anno: numero(tags.anno.filter(|a| *a != 0), indizi.anno),
        traccia: numero(tags.traccia.filter(|t| *t != 0), indizi.traccia),
        disco: numero(tags.disco.filter(|d| *d != 0), indizi.disco),
        salute: if dedotto || !problemi.is_empty() {
            Salute::Dedotto
        } else {
            Salute::Ok
        },
        problemi,
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn indizi_pieni() -> Indizi {
        Indizi {
            artista: Some("Pink Floyd".to_owned()),
            album: Some("The Dark Side of the Moon".to_owned()),
            titolo: Some("Speak to Me".to_owned()),
            anno: Some(1973),
            traccia: Some(1),
            artista_da_cartella: true,
            ..Indizi::default()
        }
    }

    #[test]
    fn senza_tag_parla_il_percorso() {
        let r = ricostruisci(&TagsGrezzi::default(), &indizi_pieni(), "01 - Speak to Me");
        assert_eq!(r.titolo.valore, "Speak to Me");
        assert_eq!(r.artista.valore, "Pink Floyd");
        assert_eq!(r.album.valore, "The Dark Side of the Moon");
        assert_eq!(r.titolo.origine, Origine::Percorso);
        assert_eq!(r.anno.map(|a| a.valore), Some(1973));
        assert_eq!(r.traccia.map(|t| t.valore), Some(1));
        assert_eq!(r.salute, Salute::Dedotto);
        assert!(r.problemi.contains(&Problema::SenzaTag));
    }

    #[test]
    fn un_tag_vero_vince_sempre() {
        let tags = TagsGrezzi {
            titolo: Some("Speak to Me / Breathe"),
            artista: Some("Roger Waters"),
            album: Some("Un altro disco"),
            anno: Some(1979),
            ..TagsGrezzi::default()
        };
        let r = ricostruisci(&tags, &indizi_pieni(), "01 - Speak to Me");
        assert_eq!(r.titolo.valore, "Speak to Me / Breathe");
        assert_eq!(r.artista.valore, "Roger Waters");
        assert_eq!(r.album.valore, "Un altro disco");
        assert_eq!(r.anno.map(|a| a.valore), Some(1979));
        assert_eq!(
            r.salute,
            Salute::Ok,
            "niente di dedotto, niente da guardare"
        );
        assert!(r.problemi.is_empty());
    }

    #[test]
    fn un_segnaposto_inglese_non_e_un_nome() {
        // Il guasto che questa prova impedisce: `Unknown Artist` prende una
        // scheda artista tutta sua, e l'arricchimento non lo guarda nemmeno
        // perché per lui quel campo è pieno.
        let tags = TagsGrezzi {
            artista: Some("Unknown Artist"),
            album: Some("<unknown>"),
            titolo: Some("Track 01"),
            ..TagsGrezzi::default()
        };
        let r = ricostruisci(&tags, &indizi_pieni(), "01 - Speak to Me");
        assert_eq!(r.artista.valore, "Pink Floyd");
        assert_eq!(r.album.valore, "The Dark Side of the Moon");
        assert_eq!(r.titolo.valore, "Speak to Me");
        assert!(r.problemi.contains(&Problema::SegnapostoArtista));
        assert!(r.problemi.contains(&Problema::SegnapostoAlbum));
        assert!(r.problemi.contains(&Problema::SegnapostoTitolo));
    }

    #[test]
    fn un_tag_riparato_vince_sul_percorso() {
        // Il file lo diceva: si è solo corretta la codifica con cui era stato
        // letto. La cartella non ne sa di più.
        let tags = TagsGrezzi {
            artista: Some("BjÃ¶rk"),
            ..TagsGrezzi::default()
        };
        let r = ricostruisci(&tags, &indizi_pieni(), "01 - Speak to Me");
        assert_eq!(r.artista.valore, "Björk");
        assert_eq!(r.artista.origine, Origine::TagRiparato);
        assert_eq!(r.artista.prima.as_deref(), Some("BjÃ¶rk"));
        assert!(r.problemi.contains(&Problema::Mojibake));
    }

    #[test]
    fn lartista_dellalbum_lo_da_solo_una_cartella() {
        // Su una raccolta l'interprete viene dal nome del file, ed è di quella
        // traccia soltanto: scriverlo come artista del disco lo spezzerebbe in
        // una uscita per traccia.
        let indizi = Indizi {
            artista: Some("Blur".to_owned()),
            album: Some("Best of 90s".to_owned()),
            artista_da_cartella: false,
            ..Indizi::default()
        };
        let r = ricostruisci(&TagsGrezzi::default(), &indizi, "04 - Blur - Song 2");
        assert_eq!(r.artista.valore, "Blur");
        assert_eq!(r.album_artist, None, "una raccolta non ha un artista solo");

        // Da una cartella d'artista, invece, sì.
        let r = ricostruisci(&TagsGrezzi::default(), &indizi_pieni(), "01");
        assert_eq!(
            r.album_artist.map(|c| c.valore),
            Some("Pink Floyd".to_owned())
        );
    }

    #[test]
    fn un_interprete_scritto_nei_tag_non_si_fa_scavalcare_dalla_cartella() {
        // `album_artist` ha la precedenza su `artist` in tutta la libreria. Su
        // un file taggato bene dentro `Cure/Disintegration/`, dedurlo dalla
        // cartella mostrerebbe l'artista «Cure» al posto di «The Cure» — e non
        // perché manchi qualcosa, ma perché abbiamo dedotto sopra a un dato che
        // c'era.
        let indizi = Indizi {
            artista: Some("Cure".to_owned()),
            album: Some("Disintegration".to_owned()),
            artista_da_cartella: true,
            ..Indizi::default()
        };
        let tags = TagsGrezzi {
            artista: Some("The Cure"),
            album: Some("Disintegration"),
            titolo: Some("Lovesong"),
            ..TagsGrezzi::default()
        };
        let r = ricostruisci(&tags, &indizi, "02");
        assert_eq!(r.artista.valore, "The Cure");
        assert_eq!(
            r.album_artist, None,
            "la cartella non ha niente da aggiungere"
        );
        assert_eq!(r.salute, Salute::Ok);
    }

    #[test]
    fn quando_nessuno_dice_niente_restano_i_segnaposto() {
        let r = ricostruisci(
            &TagsGrezzi::default(),
            &Indizi::default(),
            "gd77-05-08d1t04",
        );
        assert_eq!(r.titolo.valore, "gd77-05-08d1t04");
        assert_eq!(r.artista.valore, UNKNOWN_ARTIST);
        assert_eq!(r.album.valore, UNKNOWN_ALBUM);
        assert_eq!(r.titolo.origine, Origine::Ripiego);
    }

    #[test]
    fn i_campi_obbligatori_non_sono_mai_vuoti() {
        // Lo schema li vuole `NOT NULL`, e una stringa vuota in libreria è una
        // riga che non si può nemmeno cercare.
        let r = ricostruisci(&TagsGrezzi::default(), &Indizi::default(), "   ");
        assert!(!r.titolo.valore.is_empty());
        assert!(!r.artista.valore.is_empty());
        assert!(!r.album.valore.is_empty());
    }

    #[test]
    fn un_anno_zero_non_e_un_anno() {
        let tags = TagsGrezzi {
            anno: Some(0),
            traccia: Some(0),
            ..TagsGrezzi::default()
        };
        let r = ricostruisci(&tags, &Indizi::default(), "x");
        assert_eq!(r.anno, None);
        assert_eq!(r.traccia, None);
    }

    #[test]
    fn quel_che_e_dedotto_si_puo_sostituire_e_quel_che_e_scritto_no() {
        assert!(Origine::Percorso.sostituibile());
        assert!(Origine::Ripiego.sostituibile());
        assert!(Origine::TagRiparato.sostituibile());
        assert!(!Origine::Tag.sostituibile());
        assert!(!Origine::Manuale.sostituibile());
    }

    #[test]
    fn i_nomi_fanno_andata_e_ritorno() {
        for origine in [
            Origine::Tag,
            Origine::TagRiparato,
            Origine::Percorso,
            Origine::Ripiego,
            Origine::Manuale,
        ] {
            assert_eq!(Origine::da_str(origine.as_str()), origine);
        }
        for salute in [Salute::Ok, Salute::Dedotto, Salute::Degradato] {
            assert_eq!(Salute::da_str(salute.as_str()), salute);
        }
        // Una libreria scritta prima che queste colonne esistessero legge
        // «niente», e «niente» deve voler dire «a posto».
        assert_eq!(Salute::da_str(""), Salute::Ok);
        assert_eq!(Origine::da_str(""), Origine::Tag);
    }

    #[test]
    fn un_file_illeggibile_resta_una_riga() {
        let r = ricostruisci(&TagsGrezzi::default(), &indizi_pieni(), "01 - Speak to Me")
            .degradata(Problema::Illeggibile);
        assert_eq!(r.salute, Salute::Degradato);
        assert!(r.problemi.contains(&Problema::Illeggibile));
        assert_eq!(
            r.artista.valore, "Pink Floyd",
            "il percorso parla lo stesso"
        );
    }
}
