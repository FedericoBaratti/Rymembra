//! I tre pulsanti nell'anteprima della barra delle applicazioni.
//!
//! Passando il puntatore sull'icona di Aether nella barra, Windows mostra
//! l'anteprima della finestra. Sotto l'anteprima un programma può mettere fino
//! a sette pulsanti suoi — è quel che fa foobar2000 — e Aether ne mette tre:
//! brano precedente, riproduci o pausa, brano successivo.
//!
//! # Perché un crate a parte
//!
//! Perché per farlo bisogna parlare col sistema operativo a mano, e la radice
//! del workspace vieta `unsafe` con `forbid`. Le altre volte che serviva
//! qualcosa del genere — i controlli multimediali, il Cestino — c'era una
//! dipendenza che lo faceva già, provata e usata da altri. Qui non c'è: l'unico
//! pacchetto che lo offre per Tauri è alla prima versione, ha un solo commit, e
//! perde lo stato dei pulsanti quando la barra li ricrea.
//!
//! Così il codice `unsafe` sta tutto in un modulo, [`finestra`], dentro un
//! crate che dichiara l'eccezione nel suo `Cargo.toml` e nel `Cargo.toml` della
//! radice. Il resto di questo crate, e tutto il resto di Aether, resta senza.
//!
//! # Cosa si vede da fuori
//!
//! Due cose: [`aggancia`], una volta, sul filo della finestra; e
//! [`Barra::mostra`], da qualunque filo, ogni volta che lo stato del lettore
//! cambia. Quel che succede quando si preme un pulsante lo decide chi aggancia,
//! con la chiusura che passa: questo crate non sa niente della musica.

mod aspetto;
#[cfg(windows)]
mod finestra;
mod glifi;

pub use aspetto::{Aspetto, Etichette};

/// Un pulsante premuto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tasto {
    /// Il brano prima.
    Precedente,
    /// Pausa se suona, riproduci se è fermo.
    Alterna,
    /// Il brano dopo.
    Successivo,
}

/// I pulsanti agganciati a una finestra.
///
/// Si può tenere in uno stato condiviso e chiamare da qualunque filo: chi
/// disegna davvero è sempre il filo della finestra, a cui [`Barra::mostra`]
/// manda soltanto un messaggio. Lasciarla cadere non stacca niente — i pulsanti
/// vivono quanto la finestra, ed è la finestra a portarseli via quando si
/// chiude.
pub struct Barra {
    #[cfg(windows)]
    dentro: finestra::Barra,
}

impl Barra {
    /// Come devono apparire i pulsanti adesso.
    ///
    /// Se è uguale a quel che si mostra già non fa niente; altrimenti lo
    /// ricorda e chiede al filo della finestra di ridisegnare. Il valore
    /// ricordato è anche quello che si usa quando la barra ricrea i pulsanti
    /// da sé — la finestra che torna dal vassoio, Esplora risorse riavviato —
    /// così non si torna mai a un aspetto vecchio.
    pub fn mostra(&self, aspetto: Aspetto) {
        #[cfg(windows)]
        self.dentro.mostra(aspetto);
        #[cfg(not(windows))]
        let _ = aspetto;
    }
}

/// Mette i tre pulsanti sotto l'anteprima della finestra `hwnd`.
///
/// Va chiamata **sul filo che ha creato la finestra**, e una volta sola: è lì
/// che Windows recapita i clic, e una seconda chiamata trova la finestra già
/// agganciata e lo dice. `hwnd` è l'`HWND` della finestra; un valore che non è
/// una finestra di questo filo non fa danni, fa un errore.
///
/// `su_tasto` gira sul filo della finestra, dentro la gestione del messaggio:
/// deve tornare subito, e per qualunque cosa che possa aspettare deve passare
/// il lavoro a un altro filo. `diario` riceve una riga quando i pulsanti
/// compaiono e quando il sistema li rifiuta — la stessa riga non si ripete.
///
/// I pulsanti non compaiono subito: compaiono quando la finestra ha un'icona
/// nella barra, cioè quando si mostra. Fino al primo [`Barra::mostra`] sono
/// spenti e senza suggerimento.
///
/// # Errors
///
/// Una frase che dice perché non si è agganciato: la finestra non c'è, è di un
/// altro filo, è già agganciata, o non si è su Windows.
pub fn aggancia(
    hwnd: *mut std::ffi::c_void,
    su_tasto: impl Fn(Tasto) + Send + Sync + 'static,
    diario: impl Fn(&str) + Send + Sync + 'static,
) -> Result<Barra, String> {
    #[cfg(windows)]
    {
        finestra::aggancia(hwnd, Box::new(su_tasto), Box::new(diario))
            .map(|dentro| Barra { dentro })
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, su_tasto, diario);
        Err("i pulsanti nell'anteprima esistono solo su Windows".to_owned())
    }
}
