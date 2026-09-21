//! Quale risultato di un catalogo è il brano che si sta cercando.
//!
//! Un catalogo risponde a una stringa di ricerca con una manciata di risultati,
//! e fra quelli ci sono la registrazione giusta, un concerto del '77, una cover,
//! un remix e un file che si chiama come la canzone ma dura quaranta minuti.
//!
//! Questo modulo è la decisione in mezzo, e sta qui — nel dominio — perché è
//! precisamente il genere di decisione che [`crate`] descrive: si può sbagliare
//! in silenzio, e sbagliando mette in libreria la registrazione di qualcun altro
//! col nome giusto nei tag. Provarla deve costare una chiamata di funzione, non
//! una rete.
//!
//! # La durata è un cancello, la natura e l'autore sono la scelta
//!
//! L'ordine fra i criteri è **la** decisione di progetto di questo file, e va
//! nell'una direzione e non nell'altra:
//!
//! - un autore ufficiale con la durata sbagliata è la **registrazione**
//!   sbagliata — il live, la versione allungata, il disco intero in un file solo;
//! - la durata giusta su un autore qualunque è almeno la canzone giusta.
//!
//! Quindi la durata elimina, e solo fra i sopravvissuti natura e autore
//! scelgono.
//!
//! # Perché la natura esiste, e perché non è più un filtro secco
//!
//! Nell'albero di prima le parole come `live` e `remix` **scartavano** il
//! candidato, e con una fonte in cui tutto è in studio quella era la scelta
//! giusta. I cataloghi liberi non sono così: il Live Music Archive è fatto
//! *soltanto* di registrazioni dal vivo, e scartarle vorrebbe dire dichiarare
//! introvabile ogni brano di ogni artista che ci sta dentro.
//!
//! Quindi la natura non scarta: **retrocede**. Un candidato in studio batte
//! sempre un'alternativa, ma se l'alternativa è tutto quel che c'è, chi chiama
//! decide — con `alternative_ammesse` — se prenderla o dire «non c'è». E in
//! tutti e due i casi la natura viaggia fino all'interfaccia, perché «ho preso
//! un live» è un'informazione e il silenzio no.

use crate::abbinamento::{primo_artista, senza_decorazioni};
use crate::esterno::{BranoEsterno, Disponibilita, Fonte, Licenza};
use crate::text::{collapse_whitespace, fold_text};

/// Lo scarto di durata entro cui due registrazioni sono la stessa.
///
/// Trenta secondi, per la ragione già scritta in
/// [`crate::abbinamento::TOLLERANZA_MS`]: sono i silenzi di coda e gli stacchi
/// che cambiano fra un rip e l'altro. Qui è più larga che nell'abbinamento in
/// libreria perché un catalogo ci mette del suo — un secondo di silenzio in
/// testa, una coda che sfuma, l'applauso tagliato dove capita.
pub const BANDA_STRETTA_MS: u64 = 30_000;

/// Oltre questo scarto non è la stessa registrazione, e nessun autore lo salva.
pub const BANDA_LARGA_MS: u64 = 60_000;

/// Che registrazione promette di essere un candidato.
///
/// Non è un giudizio di qualità: è la risposta alla domanda «è *questa* la
/// registrazione chiesta, o un'altra dello stesso brano?».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Natura {
    /// Niente nel titolo dice che sia una registrazione diversa da quella chiesta.
    #[default]
    Studio,
    /// Il titolo o la collezione dicono che è dal vivo.
    DalVivo,
    /// Remix, cover, versione acustica, strumentale: un'altra registrazione.
    Alternativa,
}

impl Natura {
    /// Quanto è vicina a ciò che si è chiesto, dal più al meno.
    #[must_use]
    pub const fn peso(self) -> u8 {
        match self {
            Self::Studio => 2,
            Self::DalVivo => 1,
            Self::Alternativa => 0,
        }
    }

    /// Il nome della variante, per chi la deve mostrare.
    ///
    /// Scritto a mano e non derivato: un giorno l'enum si riordina per
    /// leggibilità, e serializzare la forma del tipo vorrebbe dire che il nome
    /// che la finestra riceve dipende da come è scritto il codice.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::Studio => "studio",
            Self::DalVivo => "dalVivo",
            Self::Alternativa => "alternativa",
        }
    }
}

/// Quanto chi pubblica somiglia alla fonte del brano.
///
/// L'ordine delle varianti **è** l'ordine di preferenza, e [`Affidabilita::peso`]
/// lo rende esplicito invece di lasciarlo dedurre da un `derive(Ord)`: un giorno
/// qualcuno riordinerà l'enum per leggibilità, e senza il peso scritto a mano
/// cambierebbe il comportamento senza cambiare nessuna riga che sembri decidere
/// qualcosa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Affidabilita {
    /// Chi pubblica si chiama come l'interprete.
    NomeAutore,
    /// Il catalogo lo segna come caricamento verificato o curato.
    Verificata,
    /// Tutto il resto: potrebbe essere chiunque.
    #[default]
    Ignota,
}

impl Affidabilita {
    /// Quanto vale, dal più al meno affidabile.
    #[must_use]
    pub const fn peso(self) -> u8 {
        match self {
            Self::NomeAutore => 2,
            Self::Verificata => 1,
            Self::Ignota => 0,
        }
    }

    /// Il nome della variante, per chi la deve mostrare.
    #[must_use]
    pub const fn nome(self) -> &'static str {
        match self {
            Self::NomeAutore => "nomeAutore",
            Self::Verificata => "verificata",
            Self::Ignota => "ignota",
        }
    }
}

/// Un risultato di un catalogo, ridotto a quel che serve per sceglierlo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidato {
    /// Come si torna a prenderlo. Per l'Internet Archive è l'indirizzo del file.
    pub url: String,
    /// Il titolo, come lo scrive chi lo ha caricato.
    pub titolo: String,
    /// Chi lo pubblica: l'autore, la band, la collezione.
    pub autore: Option<String>,
    /// Il catalogo dichiara il caricamento verificato o curato.
    pub autore_verificato: bool,
    /// La durata dichiarata, in secondi. `None` quando il catalogo non la dà.
    pub durata_sec: Option<u32>,
    /// Che registrazione promette di essere.
    pub natura: Natura,
    /// Da quale catalogo arriva.
    pub fonte: Fonte,
    /// Sotto che licenza sta, secondo quel catalogo.
    pub licenza: Licenza,
    /// Che cosa se ne può fare.
    ///
    /// Sta qui e non solo su [`BranoEsterno`] perché è un criterio di scelta
    /// come gli altri: chi cerca un file da tenere non ha nessun motivo di
    /// preferire quello che non può tenere, e senza il campo dovrebbe andare a
    /// richiederlo alla fonte una seconda volta per scoprirlo.
    pub disponibilita: Disponibilita,
    /// L'estensione del file, quando il catalogo la dichiara.
    ///
    /// Serve al momento di scrivere su disco, e serve **prima**: un `.ogg` su
    /// una libreria che non sa aprirlo è un file inutile scaricato per intero.
    pub estensione: Option<String>,
    /// La pagina pubblica del brano.
    ///
    /// Da mostrare, non da leggere: i termini di certi cataloghi obbligano a un
    /// rimando visibile accanto al brano.
    pub pagina: Option<String>,
    /// L'album, il concerto o la raccolta da cui viene, quando il catalogo lo
    /// dice.
    ///
    /// Non serve all'abbinamento — [`scegli`] non lo guarda, perché il brano
    /// cercato porta già il proprio album e confrontarli darebbe un criterio in
    /// più che sbaglia sulle compilation. Serve a **mostrarlo**: su un item
    /// dell'Internet Archive dieci risultati sono dieci pezzi dello stesso
    /// concerto, e senza il nome di quel concerto accanto sono dieci righe che
    /// si somigliano e basta. Ed è quel che finisce in `tracks.album` quando un
    /// brano di catalogo entra in libreria.
    pub album: Option<String>,
}

impl Candidato {
    /// Se ne può tenere una copia sul disco.
    #[must_use]
    pub fn si_puo_tenere(&self) -> bool {
        self.disponibilita == Disponibilita::Scaricabile
    }
}

/// Le parole che rivelano un'altra registrazione dal vivo.
const PAROLE_DAL_VIVO: &[&str] = &["live", "dal vivo", "concert", "concerto", "unplugged"];

/// Le parole che rivelano un'altra registrazione, punto.
///
/// Non sono «parole brutte»: sono parole che, se stanno nel titolo del risultato
/// e **non** in quello chiesto, dicono che quel file non è il brano chiesto. La
/// condizione doppia è essenziale — chi cerca un remix deve poterlo trovare, e
/// un brano che si chiama davvero «Karaoke» non deve diventare introvabile.
const PAROLE_ALTERNATIVA: &[&str] = &[
    "cover",
    "remix",
    "mashup",
    "karaoke",
    "reaction",
    "reazione",
    "nightcore",
    "sped up",
    "slowed",
    "8d",
    "instrumental",
    "strumentale",
    "acoustic",
    "acustica",
    "tutorial",
    "demo",
    "rehearsal",
];

/// La stringa di ricerca stretta: interprete e titolo come la fonte li scrive.
#[must_use]
pub fn query_stretta(brano: &BranoEsterno) -> String {
    let artista = brano.artist.as_deref().unwrap_or("");
    collapse_whitespace(&format!("{artista} {}", brano.title))
}

/// La stringa di ricerca larga, per quando la stretta non trova niente.
///
/// Solo il primo interprete e il titolo senza decorazioni, riusando le due
/// funzioni con cui [`crate::abbinamento`] ripulisce le stesse stringhe per
/// l'abbinamento in libreria. Riusarle e non riscriverle non è economia di
/// righe: è ciò che impedisce che «cercato nel catalogo» e «cercato in libreria»
/// vogliano dire due cose diverse.
///
/// `None` quando non aggiunge niente — coincide con la stretta o è vuota — così
/// chi chiama non paga una seconda richiesta identica alla prima.
#[must_use]
pub fn query_larga(brano: &BranoEsterno) -> Option<String> {
    let artista = brano.artist.as_deref().map(primo_artista).unwrap_or("");
    let titolo = senza_decorazioni(&brano.title);
    let larga = collapse_whitespace(&format!("{artista} {titolo}"));
    if larga.is_empty() || larga == query_stretta(brano) {
        return None;
    }
    Some(larga)
}

/// La parola compare nel testo come parola intera.
///
/// Il confronto per sottostringa non basta e il caso che lo dimostra è in
/// classifica da cinquant'anni: «Stayin' Alive» contiene `live`, e con un
/// `contains` diventerebbe un concerto.
fn contiene_parola(piegato: &str, parola: &str) -> bool {
    piegato.match_indices(parola).any(|(inizio, _)| {
        let prima = piegato
            .get(..inizio)
            .and_then(|p| p.chars().next_back())
            .is_none_or(|c| !c.is_alphanumeric());
        let dopo = piegato
            .get(inizio.saturating_add(parola.len())..)
            .and_then(|p| p.chars().next())
            .is_none_or(|c| !c.is_alphanumeric());
        prima && dopo
    })
}

/// Che registrazione promette un titolo, dato quello che si è chiesto.
///
/// Le fonti che sanno già la risposta — una collezione di soli concerti — non
/// devono chiamarla: mettono [`Natura::DalVivo`] e basta. Serve a chi ha in mano
/// soltanto una stringa.
#[must_use]
pub fn natura_dal_titolo(titolo: &str, titolo_atteso: &str) -> Natura {
    let piegato = fold_text(titolo);
    let atteso = fold_text(titolo_atteso);
    let nuova = |parole: &[&str]| {
        parole
            .iter()
            .any(|p| contiene_parola(&piegato, p) && !contiene_parola(&atteso, p))
    };
    if nuova(PAROLE_ALTERNATIVA) {
        Natura::Alternativa
    } else if nuova(PAROLE_DAL_VIVO) {
        Natura::DalVivo
    } else {
        Natura::Studio
    }
}

/// Quanto chi pubblica somiglia alla fonte del brano.
#[must_use]
pub fn affidabilita(autore: Option<&str>, verificato: bool, artista: Option<&str>) -> Affidabilita {
    let Some(autore) = autore.map(str::trim).filter(|c| !c.is_empty()) else {
        return if verificato {
            Affidabilita::Verificata
        } else {
            Affidabilita::Ignota
        };
    };

    // Il confronto col nome dell'interprete passa dalla piegatura di
    // `aether_domain::text`, la stessa che usa l'abbinamento in libreria: senza,
    // «Bjork» e «Björk» sarebbero due artisti diversi e l'autore ufficiale
    // varrebbe come uno qualunque.
    if let Some(artista) = artista {
        let atteso = fold_text(primo_artista(artista));
        if !atteso.is_empty() && fold_text(autore) == atteso {
            return Affidabilita::NomeAutore;
        }
    }

    if verificato {
        Affidabilita::Verificata
    } else {
        Affidabilita::Ignota
    }
}

/// Di quanto il candidato è più lungo di quel che la fonte dichiara, in ms.
///
/// `None` quando una delle due durate non si sa: è il caso che [`banda`] tiene
/// in gara nella banda larga, e dirlo «0» qui vorrebbe dire disegnare un
/// combaciare che nessuno ha verificato.
///
/// # Perché firmato, e perché non lo usa [`banda`]
///
/// Perché al momento di **scegliere** il verso non conta — trenta secondi in
/// più e trenta in meno sono ugualmente lontani — mentre al momento di
/// **mostrare** conta tutto: `+8 s` su un autore ufficiale è un'introduzione
/// parlata o una coda che sfuma, `−8 s` è una versione tagliata. Sono la stessa
/// sottrazione per due domande diverse, e tenerle separate costa tre righe
/// mentre unirle costerebbe un `unsigned_abs` dentro il criterio di scelta.
#[must_use]
pub fn scarto_durata(attesa_ms: Option<u64>, durata_sec: Option<u32>) -> Option<i64> {
    let (attesa, sua_sec) = (attesa_ms?, durata_sec?);
    let sua_ms = i64::from(sua_sec).saturating_mul(1000);
    Some(sua_ms.saturating_sub(i64::try_from(attesa).ok()?))
}

/// La banda di durata in cui cade un candidato: 0 stretta, 1 larga.
///
/// Restituisce `None` quando la durata lo esclude del tutto.
fn banda(attesa_ms: Option<u64>, durata_sec: Option<u32>) -> Option<(u8, u64)> {
    let (Some(attesa), Some(sua_sec)) = (attesa_ms, durata_sec) else {
        // Una delle due durate non si sa: il candidato resta in gara, ma nella
        // banda larga e con lo scarto peggiore possibile, così perde ogni pari
        // merito contro uno di cui la durata si conosce e combacia.
        return Some((1, u64::MAX));
    };
    let scarto = attesa.abs_diff(u64::from(sua_sec).saturating_mul(1000));
    if scarto <= BANDA_STRETTA_MS {
        Some((0, scarto))
    } else if scarto <= BANDA_LARGA_MS {
        Some((1, scarto))
    } else {
        None
    }
}

/// Sceglie il file da prendere, o nessuno.
///
/// L'ordine dei criteri, dal più forte al più debole:
///
/// 1. la banda di durata — stretta prima di larga, e fuori dalla larga si scarta;
/// 2. la [`Natura`] — la registrazione chiesta prima di un'altra dello stesso brano;
/// 3. l'[`Affidabilita`] di chi pubblica;
/// 4. lo scarto di durata;
/// 5. l'ordine in cui il catalogo li ha restituiti, che è la sua idea di
///    pertinenza e vale come ultimo spareggio invece di un `first()` arbitrario.
///
/// Con `alternative_ammesse` a `false` tutto ciò che non è [`Natura::Studio`]
/// esce di gara: è il comportamento giusto per chi sta ricostruendo un disco, e
/// quello sbagliato per chi cerca in un archivio di concerti. La scelta non è di
/// questo modulo.
///
/// `None` quando non ne resta nessuno: è un esito legittimo e **terminale**, da
/// non confondere con «la ricerca è fallita». Chi chiama deve poter distinguere
/// il brano che nel catalogo non c'è — inutile ritentarlo per sempre — da quello
/// per cui la richiesta è andata storta.
#[must_use]
pub fn scegli_candidato<'a>(
    candidati: &'a [Candidato],
    brano: &BranoEsterno,
    alternative_ammesse: bool,
) -> Option<&'a Candidato> {
    let mut migliore: Option<(&Candidato, u8, u8, u8, u64)> = None;

    for candidato in candidati {
        if candidato.url.trim().is_empty() {
            continue;
        }
        if !alternative_ammesse && candidato.natura != Natura::Studio {
            continue;
        }
        let Some((banda_sua, scarto)) = banda(brano.duration_ms, candidato.durata_sec) else {
            continue;
        };
        let natura = candidato.natura.peso();
        let fiducia = affidabilita(
            candidato.autore.as_deref(),
            candidato.autore_verificato,
            brano.artist.as_deref(),
        )
        .peso();

        let meglio = migliore.is_none_or(|(_, banda_ora, natura_ora, fiducia_ora, scarto_ora)| {
            (
                banda_sua,
                std::cmp::Reverse(natura),
                std::cmp::Reverse(fiducia),
                scarto,
            ) < (
                banda_ora,
                std::cmp::Reverse(natura_ora),
                std::cmp::Reverse(fiducia_ora),
                scarto_ora,
            )
        });
        if meglio {
            migliore = Some((candidato, banda_sua, natura, fiducia, scarto));
        }
    }

    migliore.map(|(candidato, ..)| candidato)
}

#[cfg(test)]
mod prove {
    use super::*;

    fn brano(artista: &str, titolo: &str, durata_ms: Option<u64>) -> BranoEsterno {
        BranoEsterno {
            title: titolo.to_owned(),
            artist: Some(artista.to_owned()),
            duration_ms: durata_ms,
            ..BranoEsterno::default()
        }
    }

    fn cand(url: &str, titolo: &str, autore: &str, durata_sec: u32) -> Candidato {
        Candidato {
            url: url.to_owned(),
            titolo: titolo.to_owned(),
            autore: Some(autore.to_owned()),
            durata_sec: Some(durata_sec),
            ..Candidato::default()
        }
    }

    #[test]
    fn lautore_che_si_chiama_come_linterprete_vince_a_parita_di_durata() {
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [
            cand("https://a/uno", "Karma Police", "Raccolta Anni 90", 264),
            cand("https://a/due", "Karma Police", "Radiohead", 264),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/due")
        );
    }

    #[test]
    fn la_durata_e_un_cancello_che_lautore_non_apre() {
        // Un'ora di «album completo» sul canale dell'artista non è la canzone.
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [
            cand(
                "https://a/album",
                "OK Computer Full Album",
                "Radiohead",
                3_180,
            ),
            cand("https://a/brano", "Karma Police", "Uno Qualunque", 262),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/brano")
        );
    }

    #[test]
    fn oltre_la_banda_larga_non_resta_nessuno() {
        let b = brano("Radiohead", "Karma Police", Some(264_000));
        let candidati = [cand("https://a/x", "Karma Police", "Radiohead", 400)];
        assert_eq!(scegli_candidato(&candidati, &b, true), None);
    }

    #[test]
    fn lo_studio_batte_il_live_anche_se_il_live_ha_lautore_giusto() {
        let b = brano("Grateful Dead", "Sugaree", Some(300_000));
        let candidati = [
            Candidato {
                natura: Natura::DalVivo,
                ..cand("https://a/live", "Sugaree", "Grateful Dead", 300)
            },
            cand("https://a/studio", "Sugaree", "Raccolta", 300),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/studio")
        );
    }

    #[test]
    fn senza_alternative_ammesse_un_live_non_e_una_risposta() {
        let b = brano("Grateful Dead", "Sugaree", Some(300_000));
        let candidati = [Candidato {
            natura: Natura::DalVivo,
            ..cand("https://a/live", "Sugaree", "Grateful Dead", 300)
        }];
        assert!(scegli_candidato(&candidati, &b, true).is_some());
        assert_eq!(scegli_candidato(&candidati, &b, false), None);
    }

    #[test]
    fn stayin_alive_non_e_un_concerto() {
        // Il caso che il confronto per sottostringa sbaglierebbe.
        assert_eq!(
            natura_dal_titolo("Stayin' Alive", "Stayin' Alive"),
            Natura::Studio
        );
        assert_eq!(
            natura_dal_titolo("Sugaree (Live at Winterland)", "Sugaree"),
            Natura::DalVivo
        );
    }

    #[test]
    fn chi_cerca_un_remix_lo_trova() {
        // La parola sta in tutti e due i titoli: non dice più niente di nuovo.
        assert_eq!(
            natura_dal_titolo("Song (Todd Terje Remix)", "Song - Todd Terje Remix"),
            Natura::Studio
        );
        assert_eq!(
            natura_dal_titolo("Song (Todd Terje Remix)", "Song"),
            Natura::Alternativa
        );
    }

    #[test]
    fn una_durata_ignota_resta_in_gara_ma_perde_i_pari_merito() {
        let b = brano("A", "T", Some(200_000));
        let candidati = [
            Candidato {
                durata_sec: None,
                ..cand("https://a/muta", "T", "A", 0)
            },
            cand("https://a/nota", "T", "A", 200),
        ];
        assert_eq!(
            scegli_candidato(&candidati, &b, true).map(|c| c.url.as_str()),
            Some("https://a/nota")
        );
    }

    #[test]
    fn lo_scarto_e_firmato_perche_il_verso_conta_a_schermo() {
        assert_eq!(scarto_durata(Some(200_000), Some(208)), Some(8_000));
        assert_eq!(scarto_durata(Some(200_000), Some(192)), Some(-8_000));
        assert_eq!(scarto_durata(None, Some(192)), None);
    }

    #[test]
    fn la_query_larga_tace_quando_non_aggiunge_niente() {
        let b = brano("Radiohead", "Karma Police", None);
        assert_eq!(query_stretta(&b), "Radiohead Karma Police");
        assert_eq!(query_larga(&b), None);

        let b = brano("Gorillaz, De La Soul", "Feel Good Inc. (Remastered)", None);
        assert!(query_larga(&b).is_some_and(|q| q.starts_with("Gorillaz ")));
    }
}
