//! Il perché di un problema o di un avviso, nella forma che si traduce.
//!
//! # Perché non basta il messaggio
//!
//! Il validatore parlava solo italiano: `SkinIssue.message` è una frase fatta,
//! e lo Studio con l'interfaccia in inglese mostrava l'elenco dei problemi in
//! italiano (R10, gruppo C). Tradurre la frase già fatta vorrebbe dire
//! rileggerla — riconoscerne il modello e ripescarne i pezzi — ed è il modo in
//! cui la prosa torna a fare da protocollo.
//!
//! Quindi chi trova un problema dice **anche** come lo si dice: una chiave del
//! catalogo delle lingue (`skin.issue.<chiave>` in `apps/desktop/src/lingue`)
//! e i valori dei suoi segnaposto. La frase italiana resta, identica a prima:
//! la leggono il modello della chat, il diario e il dettaglio di un pacchetto
//! rifiutato. Le due non divergono perché nascono dallo stesso modello, scritto
//! una volta sola nella chiamata a [`motivo!`], e una prova dello Studio
//! (`studio/tests.rs`) confronta quel modello con la riga di `it.json` e
//! pretende la riga di `en.json` con gli stessi segnaposto.
//!
//! # Valori che sono a loro volta frasi
//!
//! Alcuni pezzi sono parole e non dati — «sfondo», «in un giro solo», il
//! consiglio dopo un lampo troppo rapido — e un valore italiano dentro una
//! frase inglese è lo stesso difetto di prima, più piccolo. Un valore così è
//! un [`Motivo`] anche lui, e chi traduce lo traduce prima di metterlo al suo
//! posto.

use std::borrow::Cow;
use std::fmt;

/// Un valore di un segnaposto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Valore {
    /// Un dato: un nome, un numero, un elenco di nomi. Si mette com'è.
    Testo(String),
    /// Un pezzo di frase, che si traduce prima di metterlo al suo posto.
    Motivo(Box<Motivo>),
}

impl fmt::Display for Valore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Testo(testo) => f.write_str(testo),
            Self::Motivo(motivo) => f.write_str(&motivo.testo),
        }
    }
}

/// Quel che può diventare un [`Valore`]: i dati che i messaggi del
/// validatore mettono nei segnaposto, e un [`Motivo`].
///
/// Un elenco chiuso e non «tutto quel che è `Display`»: un `Motivo` è
/// `Display` anche lui, e in un'implementazione generica finirebbe appiattito
/// nella sua frase italiana — proprio quel che questo modulo esiste per evitare.
pub trait InValore {
    /// Il valore.
    fn in_valore(self) -> Valore;
}

macro_rules! dati_in_valore {
    ($($tipo:ty),* $(,)?) => {
        $(impl InValore for $tipo {
            fn in_valore(self) -> Valore {
                Valore::Testo(self.to_string())
            }
        })*
    };
}
dati_in_valore!(
    &str, &&str, String, &String, char, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128,
    isize, f32, f64
);

impl InValore for Motivo {
    fn in_valore(self) -> Valore {
        Valore::Motivo(Box::new(self))
    }
}

/// Il perché di un problema, per chi lo legge e per chi lo traduce.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Motivo {
    /// La chiave del catalogo delle lingue, senza il prefisso `skin.issue.`;
    /// o intera, quando comincia per `skin.` — la descrizione di un token,
    /// che ha già la sua riga (`skin.token.<id>`, vedi [`Motivo::dal_catalogo`]).
    ///
    /// Vuota per un testo che non ne ha una: [`From<String>`], cioè una
    /// chiamata che non è ancora passata da [`motivo!`]. Compila — un problema
    /// nuovo si vede subito, in italiano — e la prova dello Studio fallisce.
    pub chiave: Cow<'static, str>,
    /// La frase italiana, già composta: quella di sempre.
    pub testo: String,
    /// I valori dei segnaposto `{nome}` del modello, nell'ordine del modello.
    pub valori: Vec<(&'static str, Valore)>,
    /// I nomi vicini a quello sbagliato, per «Forse intendevi…».
    ///
    /// A parte e non dentro la frase: la frase di chi traduce non ha il
    /// segnaposto, e la finestra aggiunge la sua (`skin.issue.forse`).
    pub forse: Vec<String>,
}

impl Motivo {
    /// Una frase che il catalogo ha già, sotto la sua chiave intera: la
    /// descrizione di un token del registro dentro un avviso.
    #[must_use]
    pub fn dal_catalogo(chiave: String, testo: &str) -> Self {
        Self {
            chiave: Cow::Owned(chiave),
            testo: testo.to_owned(),
            ..Self::default()
        }
    }

    /// I nomi vicini, in coda alla frase italiana come li scriveva
    /// `vicini::forse`, e a parte per chi traduce.
    #[must_use]
    pub fn forse(mut self, candidati: &[&str]) -> Self {
        self.testo.push_str(&crate::vicini::forse(candidati));
        self.forse = candidati.iter().map(|&nome| nome.to_owned()).collect();
        self
    }
}

impl fmt::Display for Motivo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.testo)
    }
}

impl From<String> for Motivo {
    fn from(testo: String) -> Self {
        Self {
            testo,
            ..Self::default()
        }
    }
}

impl From<&str> for Motivo {
    fn from(testo: &str) -> Self {
        Self::from(testo.to_owned())
    }
}

/// Un [`Motivo`]: la chiave, il modello italiano, e un valore per ogni
/// segnaposto.
///
/// ```ignore
/// problema(path, motivo!("lunghezzaTesto", "va da {min} a {max} caratteri", min = min, max = max))
/// ```
///
/// Il modello usa solo segnaposto nominati e senza formato (`{nome}`, non
/// `{nome:.0}` né `{}`): sono gli stessi del catalogo delle lingue, e un valore
/// da arrotondare si arrotonda prima. Un argomento che il modello non usa non
/// compila; un segnaposto senza argomento lo trova la prova dello Studio.
macro_rules! motivo {
    ($chiave:literal, $modello:literal $(, $nome:ident = $valore:expr)* $(,)?) => {{
        $( let $nome = $crate::motivo::InValore::in_valore($valore); )*
        $crate::motivo::Motivo {
            chiave: ::std::borrow::Cow::Borrowed($chiave),
            testo: format!($modello $(, $nome = $nome)*),
            valori: vec![$((stringify!($nome), $nome)),*],
            forse: Vec::new(),
        }
    }};
}
