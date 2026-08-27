//! Il conteggio d'ascolto, ripartito fra i dispositivi che lo hanno prodotto.
//!
//! # Perché non basta un numero
//!
//! Perché con un numero solo la fusione ha due sole scelte, e sono entrambe
//! sbagliate. La somma raddoppia la storia ogni volta che si rifà una passata —
//! ed è il motivo per cui [`aether_domain::merge::merge_stats`] prende il
//! massimo, che su un'importazione ripetuta è la scelta giusta. Il massimo però,
//! fra due dispositivi, **perde**: cinque ascolti sul portatile e tre sul
//! telefono fanno cinque.
//!
//! Ripartire il conteggio per dispositivo scioglie il nodo senza compromessi. Il
//! numero di ciascuno sale soltanto per mano sua, quindi fra due versioni dello
//! stesso numero vince il massimo; e il totale è la somma di numeri che non si
//! sovrappongono. Commutativa, associativa, idempotente — e giusta.
//!
//! ```text
//!   portatile: 5      telefono: 3      importazione: 40
//!   ────────────────────────────────────────────────────
//!   totale: 48
//! ```
//!
//! # Perché l'importazione è un dispositivo
//!
//! Perché lo storico che arriva dal vecchio database, o dall'archivio che Spotify
//! spedisce, è un conteggio come gli altri: nessuno lo farà mai salire, e
//! rifarlo due volte deve dare lo stesso numero. Metterlo sotto un nome riservato
//! lo fa entrare nella somma senza casi speciali, e la sua idempotenza è la stessa
//! di tutti gli altri invece di essere una regola a parte da ricordarsi.

use std::collections::BTreeMap;

use crate::registro::Registro;

/// Il nome sotto cui entra lo storico che non è stato prodotto qui.
///
/// Non è un identificativo di dispositivo vero — quelli sono otto byte casuali in
/// base64url, e nessuno di essi può somigliare a questa parola.
pub const IMPORTAZIONE: &str = "importazione";

/// Quante volte un brano è stato ascoltato, per dispositivo.
///
/// Vuoto vale zero: un brano senza voce nella mappa non è un brano mai ascoltato
/// da nessuno, è un brano di cui non abbiamo notizie — e le due cose danno lo
/// stesso totale, che è il solo motivo per cui non serve distinguerle.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Contatore(BTreeMap<String, i64>);

impl Contatore {
    /// Un contatore vuoto.
    #[must_use]
    pub fn nuovo() -> Self {
        Self(BTreeMap::new())
    }

    /// Il contatore di un dispositivo solo.
    #[must_use]
    pub fn di_uno(dispositivo: &str, quanti: i64) -> Self {
        let mut mappa = BTreeMap::new();
        if quanti > 0 {
            mappa.insert(dispositivo.to_owned(), quanti);
        }
        Self(mappa)
    }

    /// Quante volte in tutto.
    ///
    /// La somma è **satura**: `i64` regge un numero di ascolti che nessuna vita
    /// umana produce, ma un documento arrivato da fuori non è tenuto a essere
    /// ragionevole, e in un albero dove `panic` è vietato un traboccamento non può
    /// essere lasciato all'aritmetica di serie.
    #[must_use]
    pub fn totale(&self) -> i64 {
        self.0
            .values()
            .fold(0_i64, |somma, quanti| somma.saturating_add(*quanti))
    }

    /// Quante volte su un dispositivo preciso.
    #[must_use]
    pub fn di(&self, dispositivo: &str) -> i64 {
        self.0.get(dispositivo).copied().unwrap_or(0)
    }

    /// Dichiara il conteggio di un dispositivo.
    ///
    /// Dichiara, non aggiunge: il numero di un dispositivo è un fatto che quel
    /// dispositivo possiede, e chi lo riceve lo registra. Un valore più basso di
    /// quello che si sa già si ignora — un conteggio non scende, e una passata che
    /// lo facesse scendere avrebbe appena buttato via degli ascolti.
    pub fn segna(&mut self, dispositivo: &str, quanti: i64) {
        if quanti <= 0 {
            return;
        }
        self.0
            .entry(dispositivo.to_owned())
            .and_modify(|gia| *gia = (*gia).max(quanti))
            .or_insert(quanti);
    }

    /// Un ascolto in più su questo dispositivo.
    pub fn conta_uno(&mut self, dispositivo: &str) {
        let quanti = self.di(dispositivo).saturating_add(1);
        self.0.insert(dispositivo.to_owned(), quanti);
    }

    /// I dispositivi che hanno contribuito, in ordine.
    pub fn dispositivi(&self) -> impl Iterator<Item = (&str, i64)> {
        self.0.iter().map(|(chi, quanti)| (chi.as_str(), *quanti))
    }

    /// Nessun dispositivo ha mai ascoltato questo brano.
    #[must_use]
    pub fn e_vuoto(&self) -> bool {
        self.0.is_empty()
    }
}

impl Registro for Contatore {
    fn fondi(&self, altro: &Self) -> Self {
        let mut fuso = self.clone();
        for (chi, quanti) in &altro.0 {
            fuso.segna(chi, *quanti);
        }
        fuso
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn due_dispositivi_sommano_invece_di_sovrascriversi() {
        // Il difetto che questo modulo esiste per togliere: con un numero solo e
        // il massimo, cinque e tre farebbero cinque.
        let portatile = Contatore::di_uno("portatile", 5);
        let telefono = Contatore::di_uno("telefono", 3);
        assert_eq!(portatile.fondi(&telefono).totale(), 8);
        assert_eq!(telefono.fondi(&portatile).totale(), 8);
    }

    #[test]
    fn rifare_una_passata_non_raddoppia_niente() {
        // La proprietà per cui `merge_stats` prende il massimo, conservata: una
        // fusione la si rifà, perché è andata storta a metà o perché nessuno si
        // ricorda di averla fatta.
        let mio = Contatore::di_uno("portatile", 5);
        let suo = Contatore::di_uno("telefono", 3);
        let una = mio.fondi(&suo);
        let due = una.fondi(&suo);
        let tre = due.fondi(&suo).fondi(&mio);
        assert_eq!(una.totale(), 8);
        assert_eq!(due, una);
        assert_eq!(tre, una);
    }

    #[test]
    fn lo_storico_importato_entra_nella_somma() {
        let mut contatore = Contatore::di_uno(IMPORTAZIONE, 40);
        contatore.segna("portatile", 5);
        contatore.segna("telefono", 3);
        assert_eq!(contatore.totale(), 48);
        // E rifare l'importazione con lo stesso numero non cambia niente.
        contatore.segna(IMPORTAZIONE, 40);
        assert_eq!(contatore.totale(), 48);
    }

    #[test]
    fn un_conteggio_non_scende_mai() {
        // Un documento vecchio che arriva in ritardo non deve poter riportare
        // indietro un numero: quel che è stato ascoltato è stato ascoltato.
        let mut contatore = Contatore::di_uno("portatile", 12);
        contatore.segna("portatile", 4);
        assert_eq!(contatore.di("portatile"), 12);
    }

    #[test]
    fn fondere_e_associativo() {
        let a = Contatore::di_uno("a", 1);
        let b = Contatore::di_uno("b", 2);
        let c = Contatore::di_uno("c", 4);
        assert_eq!(a.fondi(&b).fondi(&c), a.fondi(&b.fondi(&c)));
        assert_eq!(a.fondi(&b).fondi(&c).totale(), 7);
    }

    #[test]
    fn contare_un_ascolto_fa_salire_solo_il_proprio_numero() {
        let mut contatore = Contatore::di_uno("telefono", 3);
        contatore.segna("portatile", 5);
        contatore.conta_uno("telefono");
        assert_eq!(contatore.di("telefono"), 4);
        assert_eq!(contatore.di("portatile"), 5);
        assert_eq!(contatore.totale(), 9);
    }

    #[test]
    fn un_totale_assurdo_non_fa_cadere_l_app() {
        // Il documento arriva da fuori: «l'abbiamo scritto noi» non è una garanzia
        // su cosa ci sia dentro adesso.
        let mut contatore = Contatore::nuovo();
        contatore.segna("uno", i64::MAX);
        contatore.segna("due", i64::MAX);
        assert_eq!(contatore.totale(), i64::MAX);
    }

    #[test]
    fn uno_zero_non_occupa_posto() {
        let contatore = Contatore::di_uno("portatile", 0);
        assert!(contatore.e_vuoto());
        assert_eq!(contatore.totale(), 0);
    }
}
