//! La provenienza dei metadati, come si scrive in una riga e come si rilegge.
//!
//! `aether_domain::ricostruzione` decide da dove viene ogni campo; qui quella
//! decisione diventa le tre colonne che la migrazione 016 ha aggiunto a
//! `tracks`, e le correzioni che l'utente scrive tornano indietro.
//!
//! # Perché si scrive solo quel che non è ovvio
//!
//! [`json_origini`] restituisce `None` quando ogni campo viene dai tag, che è il
//! caso di una libreria taggata bene — cioè della stragrande maggioranza delle
//! righe. La colonna resta nulla, e non si paga un oggetto JSON per brano per
//! dire «tutto normale».

use aether_domain::enrich::OrigineCampi;
use aether_domain::ricostruzione::{Origine, Ricostruzione};
use serde::{Deserialize, Serialize};

/// Da dove viene un campo, e com'era prima se è stato riparato.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Voce {
    /// Il nome dell'origine: `percorso`, `tag-riparato`, `ripiego`, `manuale`.
    pub da: String,
    /// Il testo che il tag conteneva prima della riparazione.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prima: Option<String>,
}

impl Voce {
    /// La voce di un campo di testo, se c'è qualcosa da dire.
    ///
    /// `None` per [`Origine::Tag`]: il caso normale non si annota.
    fn del_campo(campo: &aether_domain::ricostruzione::Campo) -> Option<Self> {
        (campo.origine != Origine::Tag).then(|| Self {
            da: campo.origine.as_str().to_owned(),
            prima: campo.prima.clone(),
        })
    }

    /// La voce di un numero. Un intero non si ripara, quindi `prima` non c'è.
    fn del_numero(origine: Origine) -> Option<Self> {
        (origine != Origine::Tag).then(|| Self {
            da: origine.as_str().to_owned(),
            prima: None,
        })
    }

    /// L'origine che questa voce dichiara.
    #[must_use]
    pub fn origine(&self) -> Origine {
        Origine::da_str(&self.da)
    }
}

/// La provenienza di ogni campo di un brano.
///
/// I nomi sono in `camelCase` come tutto ciò che attraversa l'IPC: la stessa
/// forma serve al database e alla finestra, e due forme diverse per la stessa
/// cosa sono due cose da tenere allineate.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Origini {
    /// Titolo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titolo: Option<Voce>,
    /// Interprete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artista: Option<Voce>,
    /// Album.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<Voce>,
    /// Artista dell'album.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<Voce>,
    /// Genere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genere: Option<Voce>,
    /// Anno.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anno: Option<Voce>,
    /// Numero di traccia.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traccia: Option<Voce>,
    /// Numero di disco.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disco: Option<Voce>,
}

impl Origini {
    /// Le origini di una ricostruzione.
    #[must_use]
    pub fn da(r: &Ricostruzione) -> Self {
        Self {
            titolo: Voce::del_campo(&r.titolo),
            artista: Voce::del_campo(&r.artista),
            album: Voce::del_campo(&r.album),
            album_artist: r.album_artist.as_ref().and_then(Voce::del_campo),
            genere: r.genere.as_ref().and_then(Voce::del_campo),
            anno: r.anno.and_then(|n| Voce::del_numero(n.origine)),
            traccia: r.traccia.and_then(|n| Voce::del_numero(n.origine)),
            disco: r.disco.and_then(|n| Voce::del_numero(n.origine)),
        }
    }

    /// Nessun campo ha niente da dichiarare: vengono tutti dai tag.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        *self == Self::default()
    }
}

impl Origini {
    /// Le quattro origini su cui l'arricchimento decide.
    ///
    /// Un campo che non compare nel JSON viene dai tag: è il caso normale, e
    /// [`Origine::default`] lo dice già.
    #[must_use]
    pub fn per_arricchimento(&self) -> OrigineCampi {
        let leggi = |voce: Option<&Voce>| voce.map_or(Origine::Tag, Voce::origine);
        OrigineCampi {
            titolo: leggi(self.titolo.as_ref()),
            artista: leggi(self.artista.as_ref()),
            album: leggi(self.album.as_ref()),
            album_artist: leggi(self.album_artist.as_ref()),
        }
    }

    /// Le origini di una riga, dal JSON di `tracks.meta_origine`.
    ///
    /// Un JSON che non si legge — scritto da una versione futura, o rovinato —
    /// vale «tutto dai tag»: è la lettura prudente, quella che l'arricchimento
    /// non si sente in diritto di sovrascrivere.
    #[must_use]
    pub fn da_json(json: Option<&str>) -> Self {
        json.and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default()
    }
}

/// Le origini come JSON, o `None` se non c'è niente da annotare.
///
/// # Errori
///
/// Nessuno: `Origini` è fatto di stringhe e opzioni, e la serializzazione non
/// può fallire. Se fallisse — un giorno, per un campo aggiunto male — la riga
/// entra comunque in libreria senza la provenienza: perdere l'annotazione è
/// molto meglio che perdere il brano.
#[must_use]
pub fn json_origini(r: &Ricostruzione) -> Option<String> {
    let origini = Origini::da(r);
    if origini.e_vuoto() {
        return None;
    }
    serde_json::to_string(&origini).ok()
}

/// I problemi come JSON, o `None` se non ce n'è.
#[must_use]
pub fn json_problemi(r: &Ricostruzione) -> Option<String> {
    if r.problemi.is_empty() {
        return None;
    }
    let nomi: Vec<&str> = r.problemi.iter().map(|p| p.as_str()).collect();
    serde_json::to_string(&nomi).ok()
}

/// Quel che l'utente ha corretto a mano, come sta in `track_overrides.campi`.
///
/// Un campo assente vuol dire «non l'ho toccato», mai «svuotalo». Non esiste un
/// modo di dire «cancellalo» e non deve esistere, per la stessa ragione per cui
/// non esiste in `aether_domain::enrich::Fields`: perdere un dato per averlo
/// cercato e non trovato sarebbe il modo peggiore di correggere.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Correzioni {
    /// Titolo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titolo: Option<String>,
    /// Interprete.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artista: Option<String>,
    /// Album.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Artista dell'album.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    /// Genere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genere: Option<String>,
    /// Anno.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anno: Option<i64>,
    /// Numero di traccia.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traccia: Option<i64>,
    /// Numero di disco.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disco: Option<i64>,
}

impl Correzioni {
    /// Non c'è niente da correggere.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        *self == Self::default()
    }
}
