//! Gli stati del DOM che una skin può vestire e animare.
//!
//! # Il confine
//!
//! Le skin animano **stati del DOM, non eventi dell'applicazione**: la frase
//! stava in cima a `movimento.rs` dal formato 1, e il formato 2 la tiene. Quel
//! che cambia è quanti stati il DOM dichiara. Nel formato 1 erano quattro —
//! passaggio, attivo, fuoco, disabilitato — e `active` voleva dire due cose
//! diverse a seconda della parte: la voce della navigazione della pagina
//! aperta, la riga del brano selezionato. Qui ogni stato ha un nome solo e un
//! selettore solo, e ognuno è un **attributo che il componente scrive già per
//! la propria ragione**: `aria-selected` perché un lettore di schermo sappia
//! cosa è scelto, `aria-current` perché sappia dove si trova, `data-playing`
//! perché la riga che suona si distingua dalle altre. Una skin legge quel che
//! l'interfaccia dice già; non chiede all'interfaccia di dire qualcosa in più
//! solo per lei.
//!
//! # «Premuto» e «attivato» sono due stati diversi
//!
//! `pressed` è il dito sul tasto, `:active`: dura quanto la pressione. `checked`
//! è l'interruttore acceso, il cuore pieno, la stella scelta: dura finché
//! qualcuno non lo spegne. Nel formato 1 nessuno dei due aveva un nome, e
//! `active` — che suona come il primo — era un terzo significato ancora.
//!
//! # L'ordine
//!
//! A parità di specificità vince la regola che viene dopo, e tutte le regole di
//! stato hanno la stessa specificità della parte (`:where(…)`). L'ordine di
//! [`PartState::ALL`] è quindi una decisione: il passaggio e la pressione
//! prima, gli stati «di contenuto» nel mezzo, e in fondo il fuoco e il
//! disabilitato — il fuoco perché chi arriva col tabulatore deve vedere dov'è
//! anche sopra una riga selezionata, il disabilitato perché un comando spento
//! resta spento qualunque altra cosa gli stia succedendo. È l'ordine relativo
//! del formato 1 (passaggio, attivo, fuoco, disabilitato) con i nuovi inseriti
//! al posto di `active`, così una skin migrata si comporta come prima.

/// Uno stato del DOM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartState {
    /// Il puntatore è sopra.
    Hover,
    /// È premuto adesso: `:active`.
    Pressed,
    /// È fra quelli scelti: una riga in una selezione multipla.
    Selected,
    /// È quello corrente: la pagina aperta, il brano che suona nella coda.
    ///
    /// È l'erede dell'`active` del formato 1, e il suo selettore contiene
    /// ancora quello di allora: la migrazione rinomina e basta.
    Current,
    /// È aperto: un gruppo espanso, una barra larga.
    Open,
    /// È acceso: il cuore pieno, l'interruttore, la stella.
    Checked,
    /// Suona: la copertina, la riga, il lettore di quel che si sta ascoltando.
    Playing,
    /// È fermo in pausa sullo stesso brano.
    Paused,
    /// Ha il fuoco da tastiera.
    Focus,
    /// È disabilitato.
    Disabled,
}

/// Dove sta il fuoco che accende lo stato `focus` di una parte.
///
/// Quasi sempre sulla parte stessa. Alcune parti però sono **involucri**: il
/// campo di ricerca è una `.cerca` con dentro un `<input>`, il volume una
/// striscia con dentro un cursore. `:focus-visible` sull'involucro non si
/// accende mai, perché il fuoco ce l'ha il figlio — e uno stato che non si
/// accende mai è un campo che la skin scrive e che non fa niente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fuoco {
    /// Il fuoco lo prende la parte: `:focus-visible`.
    Proprio,
    /// Il fuoco lo prende un figlio, o la parte stessa quando è lei il
    /// controllo: `:focus-visible, :has(:focus-visible)`. Il volume è un
    /// involucro nella colonna e il cursore nudo nella barra del lettore, e
    /// una skin non deve sapere quale dei due sta guardando.
    Dentro,
}

impl PartState {
    /// Come si scrive nel documento.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Pressed => "pressed",
            Self::Selected => "selected",
            Self::Current => "current",
            Self::Open => "open",
            Self::Checked => "checked",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Focus => "focus",
            Self::Disabled => "disabled",
        }
    }

    /// Il selettore che lo riconosce, per una parte che prende il fuoco nel
    /// modo dato.
    ///
    /// `focus` è `:focus-visible` e non `:focus`: lo stato deve apparire a chi
    /// naviga da tastiera, non a ogni clic del mouse. `current` accetta
    /// `aria-current` scritto da React per un `false` come stringa `"false"`, e
    /// lo esclude: altrimenti ogni voce della navigazione sarebbe corrente.
    #[must_use]
    pub const fn selector(self, fuoco: Fuoco) -> &'static str {
        match self {
            Self::Hover => ":hover",
            Self::Pressed => ":active",
            Self::Selected => "[aria-selected=\"true\"], [data-selected]",
            Self::Current => {
                "[aria-current]:not([aria-current=\"false\"]), [data-active], .is-active"
            }
            Self::Open => "[aria-expanded=\"true\"], [data-open]",
            Self::Checked => "[aria-pressed=\"true\"], [aria-checked=\"true\"], :checked",
            Self::Playing => "[data-playing]",
            Self::Paused => "[data-paused]",
            Self::Focus => match fuoco {
                Fuoco::Proprio => ":focus-visible",
                Fuoco::Dentro => ":focus-visible, :has(:focus-visible)",
            },
            Self::Disabled => ":disabled, [aria-disabled=\"true\"]",
        }
    }

    /// Tutti, nell'ordine in cui il compilatore li emette. Vedi il `//!`.
    pub const ALL: &'static [Self] = &[
        Self::Hover,
        Self::Pressed,
        Self::Selected,
        Self::Current,
        Self::Open,
        Self::Checked,
        Self::Playing,
        Self::Paused,
        Self::Focus,
        Self::Disabled,
    ];

    /// Dal nome scritto nel documento.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.as_str() == raw)
    }

    /// I nomi ammessi, per un messaggio d'errore.
    #[must_use]
    pub fn nomi() -> Vec<&'static str> {
        Self::ALL.iter().map(|s| s.as_str()).collect()
    }

    /// Il posto dell'indice in [`Self::ALL`].
    #[must_use]
    pub fn posto(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l_ordine_relativo_del_formato_1_resta() {
        // Passaggio prima di corrente, corrente prima di fuoco, fuoco prima di
        // disabilitato: è l'ordine di hover/active/focus/disabled del formato
        // 1, con `current` al posto di `active`. Una skin migrata vince e perde
        // gli stessi conflitti di prima.
        let posto = |s: PartState| s.posto();
        assert!(posto(PartState::Hover) < posto(PartState::Current));
        assert!(posto(PartState::Current) < posto(PartState::Focus));
        assert!(posto(PartState::Focus) < posto(PartState::Disabled));
        assert_eq!(PartState::ALL.last(), Some(&PartState::Disabled));
    }

    #[test]
    fn corrente_contiene_ancora_l_attivo_del_formato_1() {
        let selettore = PartState::Current.selector(Fuoco::Proprio);
        assert!(selettore.contains("[data-active]"));
        assert!(selettore.contains(".is-active"));
        // E il `false` che React scrive come stringa non vale.
        assert!(selettore.contains(":not([aria-current=\"false\"])"));
    }

    #[test]
    fn il_fuoco_e_quello_da_tastiera_e_segue_l_involucro() {
        assert_eq!(PartState::Focus.selector(Fuoco::Proprio), ":focus-visible");
        assert_eq!(
            PartState::Focus.selector(Fuoco::Dentro),
            ":focus-visible, :has(:focus-visible)"
        );
        // Gli altri stati non cambiano con l'involucro.
        assert_eq!(
            PartState::Hover.selector(Fuoco::Dentro),
            PartState::Hover.selector(Fuoco::Proprio)
        );
    }

    #[test]
    fn i_nomi_sono_unici_e_tornano_indietro() {
        let mut visti = std::collections::HashSet::new();
        for stato in PartState::ALL {
            assert!(visti.insert(stato.as_str()), "{} ripetuto", stato.as_str());
            assert_eq!(PartState::parse(stato.as_str()), Some(*stato));
        }
        // `active` non c'è più: la migrazione lo porta a `current`.
        assert_eq!(PartState::parse("active"), None);
    }
}
