//! Le regole di una playlist intelligente: cosa si può chiedere, e cosa no.
//!
//! # Perché il dominio non scrive la query
//!
//! Perché una regola è una **domanda**, e SQL è un modo di farla. Qui c'è la
//! domanda: «artista contiene Björk», «riproduzioni maggiore di 5», «aggiunto
//! negli ultimi 30 giorni». Chi la traduce in `WHERE t.artist LIKE ?1` sta in
//! `aether-app`, dove c'è un database e dove si sa che quel `?1` è un parametro
//! e non un pezzo di stringa.
//!
//! Non è una divisione di comodo. È l'unica difesa che regge contro
//! l'iniezione: se le regole sapessero produrre SQL, quel codice starebbe nel
//! crate che le prove riempiono di stringhe scritte a mano, e prima o poi
//! qualcuno concatenerebbe un valore invece di legarlo. Le colonne che si
//! possono nominare sono un `enum` chiuso — [`Campo`] — quindi il pezzo di
//! query che varia non arriva mai da fuori: arriva da un `match`.
//!
//! # Perché le colonne esistevano da prima
//!
//! `playlists.is_smart` e `playlists.rules` sono nella migrazione `001`, cioè
//! nella prima riga di schema che questo progetto abbia scritto, e non le ha
//! mai usate nessuno. Erano un'intenzione dichiarata a schema e mai onorata:
//! una playlist con `is_smart = 1` si sarebbe rifiutata di farsi modificare a
//! mano — quel controllo c'è, in `playlists::modificabile` — e nient'altro.

/// Un campo su cui si può porre una condizione.
///
/// Chiuso, e deve restarlo: è la sola ragione per cui la traduzione in SQL non
/// può diventare un'iniezione. Aggiungere un campo qui vuol dire aggiungere un
/// ramo in `aether_app::smart`, e il compilatore lo chiede.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Campo {
    /// `tracks.title`.
    Titolo,
    /// `tracks.artist`.
    Artista,
    /// `tracks.album`.
    Album,
    /// `tracks.genre`.
    Genere,
    /// `tracks.year`.
    Anno,
    /// `tracks.rating`, da 0 a 5.
    Valutazione,
    /// `tracks.liked`.
    Preferito,
    /// `tracks.play_count`.
    Riproduzioni,
    /// `tracks.date_added`, in millisecondi dall'epoca.
    Aggiunto,
    /// `tracks.duration_ms`.
    Durata,
    /// `tracks.last_played_at`, in millisecondi dall'epoca.
    UltimoAscolto,
}

impl Campo {
    /// Che tipo di valore accetta.
    ///
    /// Serve a chi disegna la finestra — un campo di testo, un numero, una
    /// spunta — e a [`Regola::valida`], che è l'unica che decide davvero.
    #[must_use]
    pub const fn genere(self) -> GenereCampo {
        match self {
            Self::Titolo | Self::Artista | Self::Album | Self::Genere => GenereCampo::Testo,
            Self::Anno | Self::Valutazione | Self::Riproduzioni | Self::Durata => {
                GenereCampo::Numero
            }
            Self::Preferito => GenereCampo::Booleano,
            Self::Aggiunto | Self::UltimoAscolto => GenereCampo::Data,
        }
    }

    /// Il nome con cui sta scritto in `playlists.rules`.
    ///
    /// Stabile per definizione: cambiarlo renderebbe illeggibili le playlist
    /// già salvate. È il motivo per cui non si usa il nome della variante.
    #[must_use]
    pub const fn come_testo(self) -> &'static str {
        match self {
            Self::Titolo => "titolo",
            Self::Artista => "artista",
            Self::Album => "album",
            Self::Genere => "genere",
            Self::Anno => "anno",
            Self::Valutazione => "valutazione",
            Self::Preferito => "preferito",
            Self::Riproduzioni => "riproduzioni",
            Self::Aggiunto => "aggiunto",
            Self::Durata => "durata",
            Self::UltimoAscolto => "ultimoAscolto",
        }
    }

    /// Il contrario di [`Self::come_testo`].
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        Some(match testo {
            "titolo" => Self::Titolo,
            "artista" => Self::Artista,
            "album" => Self::Album,
            "genere" => Self::Genere,
            "anno" => Self::Anno,
            "valutazione" => Self::Valutazione,
            "preferito" => Self::Preferito,
            "riproduzioni" => Self::Riproduzioni,
            "aggiunto" => Self::Aggiunto,
            "durata" => Self::Durata,
            "ultimoAscolto" => Self::UltimoAscolto,
            _ => return None,
        })
    }

    /// Tutti, nell'ordine in cui vanno mostrati.
    #[must_use]
    pub const fn tutti() -> &'static [Self] {
        &[
            Self::Titolo,
            Self::Artista,
            Self::Album,
            Self::Genere,
            Self::Anno,
            Self::Valutazione,
            Self::Preferito,
            Self::Riproduzioni,
            Self::Durata,
            Self::Aggiunto,
            Self::UltimoAscolto,
        ]
    }
}

/// Che genere di valore vuole un campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenereCampo {
    /// Una stringa.
    Testo,
    /// Un intero.
    Numero,
    /// Sì o no.
    Booleano,
    /// Un istante, che nelle regole si esprime **in giorni indietro**.
    Data,
}

/// Come si confronta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operatore {
    /// Il testo contiene.
    Contiene,
    /// Il testo non contiene.
    NonContiene,
    /// Uguale, che per il testo vuol dire «uguale ignorando la cassa».
    Uguale,
    /// Diverso.
    Diverso,
    /// Il testo comincia con.
    Inizia,
    /// Il testo finisce con.
    Finisce,
    /// Maggiore, per numeri e date.
    Maggiore,
    /// Minore.
    Minore,
    /// Negli ultimi N giorni. Solo per le date.
    NegliUltimi,
    /// Non negli ultimi N giorni: la regola delle canzoni dimenticate.
    NonNegliUltimi,
    /// Vuoto: nessun genere scritto, nessun anno, mai ascoltato.
    ///
    /// Un operatore suo e non «uguale a niente», perché nel database quel
    /// «niente» è a volte `NULL` e a volte stringa vuota — la scansione scrive
    /// l'uno o l'altra a seconda di com'era il tag — e chi scrive una regola
    /// non deve conoscere quella differenza.
    Vuoto,
    /// Non vuoto.
    NonVuoto,
}

impl Operatore {
    /// Il nome con cui sta scritto in `playlists.rules`.
    #[must_use]
    pub const fn come_testo(self) -> &'static str {
        match self {
            Self::Contiene => "contiene",
            Self::NonContiene => "nonContiene",
            Self::Uguale => "uguale",
            Self::Diverso => "diverso",
            Self::Inizia => "inizia",
            Self::Finisce => "finisce",
            Self::Maggiore => "maggiore",
            Self::Minore => "minore",
            Self::NegliUltimi => "negliUltimi",
            Self::NonNegliUltimi => "nonNegliUltimi",
            Self::Vuoto => "vuoto",
            Self::NonVuoto => "nonVuoto",
        }
    }

    /// Il contrario di [`Self::come_testo`].
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        Some(match testo {
            "contiene" => Self::Contiene,
            "nonContiene" => Self::NonContiene,
            "uguale" => Self::Uguale,
            "diverso" => Self::Diverso,
            "inizia" => Self::Inizia,
            "finisce" => Self::Finisce,
            "maggiore" => Self::Maggiore,
            "minore" => Self::Minore,
            "negliUltimi" => Self::NegliUltimi,
            "nonNegliUltimi" => Self::NonNegliUltimi,
            "vuoto" => Self::Vuoto,
            "nonVuoto" => Self::NonVuoto,
            _ => return None,
        })
    }

    /// Ha bisogno di un valore accanto.
    #[must_use]
    pub const fn vuole_valore(self) -> bool {
        !matches!(self, Self::Vuoto | Self::NonVuoto)
    }

    /// Quelli che hanno senso per questo genere di campo.
    #[must_use]
    pub const fn per(genere: GenereCampo) -> &'static [Self] {
        match genere {
            GenereCampo::Testo => &[
                Self::Contiene,
                Self::NonContiene,
                Self::Uguale,
                Self::Diverso,
                Self::Inizia,
                Self::Finisce,
                Self::Vuoto,
                Self::NonVuoto,
            ],
            GenereCampo::Numero => &[
                Self::Uguale,
                Self::Diverso,
                Self::Maggiore,
                Self::Minore,
                Self::Vuoto,
                Self::NonVuoto,
            ],
            GenereCampo::Booleano => &[Self::Uguale],
            GenereCampo::Data => &[
                Self::NegliUltimi,
                Self::NonNegliUltimi,
                Self::Vuoto,
                Self::NonVuoto,
            ],
        }
    }
}

/// Il valore accanto a un operatore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Valore {
    /// Per i campi di testo.
    Testo(String),
    /// Per numeri, booleani (0 o 1) e giorni.
    Numero(i64),
    /// Per gli operatori che non ne vogliono.
    Nessuno,
}

/// Una condizione sola.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regola {
    /// Su cosa.
    pub campo: Campo,
    /// Come.
    pub operatore: Operatore,
    /// Con quale valore.
    pub valore: Valore,
}

impl Regola {
    /// La regola ha senso.
    ///
    /// # Perché una regola senza senso non è un errore da propagare
    ///
    /// Perché il posto in cui nasce è un menù a tendina, e cambiare campo dopo
    /// aver scelto un operatore produce quasi sempre una coppia illegale per un
    /// istante — «anno» con «contiene», mentre il secondo menù non è ancora
    /// stato ritoccato. Le regole che non stanno in piedi si **saltano** nella
    /// traduzione, e la finestra le segna; farle fallire vorrebbe dire una
    /// playlist che smette di funzionare mentre la si scrive.
    #[must_use]
    pub fn valida(&self) -> bool {
        if !Operatore::per(self.campo.genere()).contains(&self.operatore) {
            return false;
        }
        match (self.operatore.vuole_valore(), &self.valore) {
            (false, Valore::Nessuno) => true,
            (false, _) => false,
            (true, Valore::Testo(t)) => {
                self.campo.genere() == GenereCampo::Testo && !t.trim().is_empty()
            }
            (true, Valore::Numero(n)) => match self.campo.genere() {
                GenereCampo::Testo => false,
                // Zero giorni indietro è una finestra vuota: sarebbe una regola
                // che non seleziona mai niente, scritta da chi voleva «oggi».
                GenereCampo::Data => *n > 0,
                GenereCampo::Booleano => *n == 0 || *n == 1,
                GenereCampo::Numero => true,
            },
            (true, Valore::Nessuno) => false,
        }
    }
}

/// Come si mettono insieme le condizioni.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Combinazione {
    /// Tutte devono valere: `AND`.
    #[default]
    Tutte,
    /// Ne basta una: `OR`.
    Qualsiasi,
}

impl Combinazione {
    /// Il nome con cui sta scritta in `playlists.rules`.
    #[must_use]
    pub const fn come_testo(self) -> &'static str {
        match self {
            Self::Tutte => "tutte",
            Self::Qualsiasi => "qualsiasi",
        }
    }

    /// Il contrario.
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        match testo {
            "tutte" => Some(Self::Tutte),
            "qualsiasi" => Some(Self::Qualsiasi),
            _ => None,
        }
    }
}

/// In che ordine escono i brani.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ordinamento {
    /// Artista, album, disco, traccia: l'ordine di uno scaffale.
    #[default]
    Scaffale,
    /// I più ascoltati in cima.
    PiuAscoltati,
    /// I meno ascoltati in cima: la playlist «quel che non ho mai sentito».
    MenoAscoltati,
    /// I più recenti in cima, per data di aggiunta.
    Recenti,
    /// A caso.
    ///
    /// Il seme lo dà chi esegue, non queste regole: il dominio non ha un
    /// orologio, ed è la stessa disciplina della coda mescolata.
    Casuale,
}

impl Ordinamento {
    /// Il nome con cui sta scritto in `playlists.rules`.
    #[must_use]
    pub const fn come_testo(self) -> &'static str {
        match self {
            Self::Scaffale => "scaffale",
            Self::PiuAscoltati => "piuAscoltati",
            Self::MenoAscoltati => "menoAscoltati",
            Self::Recenti => "recenti",
            Self::Casuale => "casuale",
        }
    }

    /// Il contrario.
    #[must_use]
    pub fn da_testo(testo: &str) -> Option<Self> {
        Some(match testo {
            "scaffale" => Self::Scaffale,
            "piuAscoltati" => Self::PiuAscoltati,
            "menoAscoltati" => Self::MenoAscoltati,
            "recenti" => Self::Recenti,
            "casuale" => Self::Casuale,
            _ => return None,
        })
    }
}

/// Tutte le regole di una playlist intelligente.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Insieme {
    /// `AND` o `OR`.
    pub combinazione: Combinazione,
    /// Le condizioni.
    pub regole: Vec<Regola>,
    /// Quanti brani al massimo. `None` = tutti.
    pub limite: Option<u32>,
    /// In che ordine.
    pub ordinamento: Ordinamento,
}

impl Insieme {
    /// Le regole che si possono davvero tradurre.
    ///
    /// Quelle storte si saltano: vedi [`Regola::valida`].
    #[must_use]
    pub fn valide(&self) -> Vec<&Regola> {
        self.regole.iter().filter(|r| r.valida()).collect()
    }

    /// Quante ne sono state scartate.
    #[must_use]
    pub fn scartate(&self) -> usize {
        self.regole.len().saturating_sub(self.valide().len())
    }

    /// L'insieme seleziona **tutta** la libreria.
    ///
    /// Nessuna regola valida con `Tutte` vuol dire «nessuna condizione», cioè
    /// ogni brano — che è un risultato legittimo e quasi sempre non voluto. Chi
    /// esegue deve poterlo dire prima, non dopo aver mostrato millequattrocento
    /// righe sotto il nome «Preferiti del 2019».
    #[must_use]
    pub fn prende_tutto(&self) -> bool {
        self.valide().is_empty() && self.combinazione == Combinazione::Tutte
    }

    /// L'insieme non seleziona **niente**, e non per come è fatta la libreria.
    ///
    /// Nessuna regola valida con `Qualsiasi` è un `OR` vuoto: falso sempre.
    #[must_use]
    pub fn prende_niente(&self) -> bool {
        self.valide().is_empty() && self.combinazione == Combinazione::Qualsiasi
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn regola(campo: Campo, operatore: Operatore, valore: Valore) -> Regola {
        Regola {
            campo,
            operatore,
            valore,
        }
    }

    #[test]
    fn i_nomi_fanno_andata_e_ritorno() {
        // Sono i nomi con cui le regole stanno su disco: se `da_testo` e
        // `come_testo` divergessero, ogni playlist intelligente salvata prima
        // diventerebbe illeggibile — e in silenzio, perché una regola che non
        // si interpreta si salta.
        for campo in Campo::tutti() {
            assert_eq!(Campo::da_testo(campo.come_testo()), Some(*campo));
        }
        for genere in [
            GenereCampo::Testo,
            GenereCampo::Numero,
            GenereCampo::Booleano,
            GenereCampo::Data,
        ] {
            for operatore in Operatore::per(genere) {
                assert_eq!(
                    Operatore::da_testo(operatore.come_testo()),
                    Some(*operatore)
                );
            }
        }
        for ordine in [
            Ordinamento::Scaffale,
            Ordinamento::PiuAscoltati,
            Ordinamento::MenoAscoltati,
            Ordinamento::Recenti,
            Ordinamento::Casuale,
        ] {
            assert_eq!(Ordinamento::da_testo(ordine.come_testo()), Some(ordine));
        }
    }

    #[test]
    fn un_operatore_di_testo_su_un_campo_numerico_non_vale() {
        // Il caso che succede davvero: si cambia il primo menù e il secondo
        // resta com'era. Non è un errore da propagare, è una regola da saltare.
        assert!(
            !regola(
                Campo::Anno,
                Operatore::Contiene,
                Valore::Testo("199".to_owned())
            )
            .valida()
        );
        assert!(regola(Campo::Anno, Operatore::Maggiore, Valore::Numero(1990)).valida());
    }

    #[test]
    fn un_valore_vuoto_non_e_una_regola() {
        // «artista contiene ""» selezionerebbe tutto, sotto un nome che promette
        // il contrario.
        assert!(
            !regola(
                Campo::Artista,
                Operatore::Contiene,
                Valore::Testo("   ".to_owned())
            )
            .valida()
        );
    }

    #[test]
    fn zero_giorni_indietro_non_e_una_finestra() {
        assert!(!regola(Campo::Aggiunto, Operatore::NegliUltimi, Valore::Numero(0)).valida());
        assert!(regola(Campo::Aggiunto, Operatore::NegliUltimi, Valore::Numero(30)).valida());
    }

    #[test]
    fn vuoto_non_vuole_un_valore_accanto() {
        assert!(regola(Campo::Genere, Operatore::Vuoto, Valore::Nessuno).valida());
        assert!(
            !regola(
                Campo::Genere,
                Operatore::Vuoto,
                Valore::Testo("rock".to_owned())
            )
            .valida()
        );
    }

    #[test]
    fn un_preferito_e_zero_o_uno() {
        assert!(regola(Campo::Preferito, Operatore::Uguale, Valore::Numero(1)).valida());
        assert!(!regola(Campo::Preferito, Operatore::Uguale, Valore::Numero(7)).valida());
    }

    #[test]
    fn nessuna_regola_significa_due_cose_diverse() {
        // È la distinzione che permette di avvisare prima invece di mostrare
        // millequattrocento righe sotto un nome che ne prometteva dieci.
        let tutte = Insieme::default();
        assert!(tutte.prende_tutto());
        assert!(!tutte.prende_niente());

        let qualsiasi = Insieme {
            combinazione: Combinazione::Qualsiasi,
            ..Insieme::default()
        };
        assert!(qualsiasi.prende_niente());
        assert!(!qualsiasi.prende_tutto());
    }

    #[test]
    fn le_regole_storte_si_contano_e_le_altre_restano() {
        let insieme = Insieme {
            combinazione: Combinazione::Tutte,
            regole: vec![
                regola(
                    Campo::Artista,
                    Operatore::Contiene,
                    Valore::Testo("Björk".to_owned()),
                ),
                regola(Campo::Anno, Operatore::Contiene, Valore::Numero(1997)),
            ],
            limite: Some(50),
            ordinamento: Ordinamento::Recenti,
        };
        assert_eq!(insieme.valide().len(), 1);
        assert_eq!(insieme.scartate(), 1);
        assert!(!insieme.prende_tutto(), "una regola valida c'è");
    }
}
