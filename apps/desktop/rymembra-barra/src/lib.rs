//! I tre pulsanti nell'anteprima della barra delle applicazioni.
//!
//! Passando il puntatore sull'icona di Rymembra nella barra, Windows mostra
//! l'anteprima della finestra. Sotto l'anteprima un programma può mettere fino
//! a sette pulsanti suoi — è quel che fa foobar2000 — e Rymembra ne mette tre:
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
//! Così il codice `unsafe` sta tutto in cinque posti, il modulo `finestra`, una
//! funzione di [`unita`](crate::unita_di), le tre chiamate di
//! [`sottofondo`](crate::Sottofondo), le quattro di
//! [`espulsione`](crate::osserva_volume) e la lettura di Maiusc di
//! [`maiusc`](crate::maiusc_giu), dentro un crate che dichiara
//! l'eccezione nel suo `Cargo.toml` e nel `Cargo.toml` della radice. Il resto
//! di questo crate, e tutto il resto di Rymembra, resta senza.
//!
//! # L'altra domanda al sistema: il genere di un'unità
//!
//! Il secondo posto è venuto dopo, per la stessa ragione: «Elimina dal disco»
//! deve sapere se un file sta su una chiavetta, una scheda SD, un CD o un RAM
//! disk, dove Windows un Cestino non ce l'ha, e lo dice soltanto
//! `GetDriveTypeW` — sul volume che `GetVolumePathNameW` trova, perché una
//! scheda SD può stare montata in una cartella di `C:`. Nessuna dipendenza
//! seria lo espone senza `unsafe`, e
//! portarne dentro una per una chiamata sola costerebbe più di questa
//! funzione: vedi [`unita_di`].
//!
//! # La terza: il lavoro di sottofondo, e la batteria
//!
//! Le passate dopo la scansione — impronte e volume di tutta la libreria —
//! girano a priorità di sottofondo, e a batteria più piano. La modalità la
//! mette `SetThreadPriority(…, THREAD_MODE_BACKGROUND_BEGIN)`, la batteria la
//! dice `GetSystemPowerStatus`: vedi [`Sottofondo`] e [`a_batteria`].
//!
//! # La quarta: il volume che Windows sta per espellere
//!
//! La sorveglianza delle cartelle tiene aperto un handle su ogni cartella
//! della libreria, e su una chiavetta o un disco USB «Espelli» di Esplora
//! risorse risponde «Il dispositivo è in uso» finché Rymembra è aperto
//! (R24-1). Windows chiede a chi ha il dispositivo aperto di lasciarlo con una
//! notifica di dispositivo, `CM_Register_Notification`, che non si fa senza
//! `unsafe`: vedi [`osserva_volume`], [`volume_di`] e [`Avviso`].
//!
//! # La quinta: Maiusc tenuto mentre l'applicazione si apre
//!
//! L'uscita d'emergenza da una skin che rende la finestra inservibile (D-48)
//! non può dipendere dalla pagina: la finestra compare prima che la pagina sia
//! pronta, e un tasto arriva alla pagina solo se ha il fuoco. Lo stato del tasto
//! lo dice `GetAsyncKeyState`, una chiamata sola, e la regola che lo usa — quanto
//! tenuto, entro quanto dalla comparsa della finestra — è pura: vedi [`Ascolto`]
//! e [`maiusc_giu`].
//!
//! # La sesta: dove sta la cartella privata del pacchetto MSIX
//!
//! Nella copia del Microsoft Store i file che l'app crea in `%APPDATA%` e in
//! `%LOCALAPPDATA%` stanno nella cartella privata del pacchetto, che Esplora
//! risorse non vede dove l'app la vede. Non serve `unsafe` — è l'API WinRT
//! sicura del `windows` — ma è un'altra domanda al sistema, e sta con le
//! altre: vedi [`cartella_cache_del_pacchetto`].
//!
//! # Cosa si vede da fuori
//!
//! Per i pulsanti, due cose: [`aggancia`], una volta, sul filo della finestra;
//! e [`Barra::mostra`], da qualunque filo, ogni volta che lo stato del lettore
//! cambia. Quel che succede quando si preme un pulsante lo decide chi aggancia,
//! con la chiusura che passa: questo crate non sa niente della musica. Per le
//! unità, [`unita_di`] e il suo [`Unita`]. Per il sottofondo, la guardia
//! [`Sottofondo`] e la domanda [`a_batteria`]. Per l'espulsione, [`volume_di`]
//! dice su quale volume sta un percorso e [`osserva_volume`] avvisa quando
//! Windows vuole toglierlo: quel che si lascia e quando si riprende lo decide
//! chi chiama.

// Fuori da Windows i pulsanti non si disegnano, e quel che serve a disegnarli
// — gli identificativi, i suggerimenti, i glifi — non lo legge nessuno. Resta
// compilato lo stesso, e non dietro `cfg(windows)`: le sue prove sono pure, e
// girano anche dove la barra non c'è.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "lo legge soltanto `finestra`, che c'è solo su Windows"
    )
)]
mod aspetto;
mod espulsione;
#[cfg(windows)]
mod finestra;
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "lo legge soltanto `finestra`, che c'è solo su Windows"
    )
)]
mod glifi;
mod maiusc;
mod pacchetto;
mod sottofondo;
mod unita;

pub use aspetto::{Aspetto, Etichette};
pub use espulsione::{Avviso, Registrazione, osserva_volume, volume_di};
pub use maiusc::{Ascolto, Esito, FINESTRA, PASSO, TENUTO, maiusc_giu};
pub use pacchetto::cartella_cache_del_pacchetto;
pub use sottofondo::{Sottofondo, a_batteria};
pub use unita::{Unita, unita_di};

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

/// Fa in modo che un **Alt da solo** (o F10) non mandi la finestra `hwnd` in un
/// menu di sistema invisibile, che si mangia il tasto seguente e con una
/// WebView2 dentro si blocca (reperto 1 di FALT). Alt+Spazio, che apre il menu
/// di sistema vero, resta com'era.
///
/// Va chiamata **sul filo che ha creato la finestra**, e una volta sola, il
/// prima possibile — dal `setup`: non dipende dai pulsanti, che si agganciano
/// solo quando la libreria è aperta, e la finestra può comparire prima. Vedi
/// `finestra.rs`.
///
/// # Errors
///
/// Una frase che dice perché non si è montata: la finestra non c'è, è di un
/// altro filo, è già montata, o non si è su Windows.
pub fn ferma_alt_da_solo(hwnd: *mut std::ffi::c_void) -> Result<(), String> {
    #[cfg(windows)]
    {
        finestra::ferma_alt_da_solo(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("l'Alt da solo non è un problema fuori da Windows".to_owned())
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
