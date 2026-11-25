//! Le corsie: i comandi che valgono nell'ordine in cui arrivano, fuori dal filo
//! della pagina.
//!
//! # Il problema
//!
//! Un comando Tauri scritto senza `async` gira **sul posto**: sul filo della
//! finestra di Windows, sul filo del ponte Java di Android. Per questo, finché
//! erano sincroni, `volume`, `equalizzatore`, `pausa`, `vai_a`, `preferito`,
//! `valutazione` e i salvataggi delle preferenze si eseguivano uno alla volta e
//! **nell'ordine in cui la pagina li aveva mandati** — lo dice anche il commento
//! di `riproduzione::prendi_il_lettore`, che li chiamava «sincroni apposta»:
//! arrivano a raffica da un cursore sotto il dito, e `volume.ts` ed
//! `Equalizzatore.tsx` contano che l'eco di un valore torni dopo quello
//! mandato prima.
//!
//! Il prezzo era che un lucchetto occupato fermava tutta la pagina (vedi il
//! `//!` di [`crate::disparte`]). Un `async fn` che manda il lavoro a
//! [`crate::disparte::in_disparte`] non ferma niente, ma lo manda su un pool di
//! fili: due comandi in fila su un lucchetto occupato si risvegliano nell'ordine
//! che sceglie il sistema operativo, e l'ultimo valore scritto può essere il
//! primo che la pagina aveva mandato.
//!
//! # Cosa fa una corsia
//!
//! Un filo solo, con una coda: i lavori si eseguono **uno alla volta, nell'ordine
//! in cui sono stati messi in coda**, e chi li ha chiesti li aspetta senza
//! occupare né il filo della pagina né un posto del pool. Il filo parte al primo
//! lavoro, e ogni lavoro gira dentro [`crate::stato::sorvegliato`]: un panico
//! diventa `internal.aborted` per chi l'ha chiesto, finisce nella salute dei fili
//! e non porta via la corsia. Una corsia morta sarebbe lo stesso blocco in un
//! altro posto — da lì in poi ogni `volume` aspetterebbe per sempre.
//!
//! Le corsie sono due, una per **risorsa**, e non una sola: [`LETTORE`] e
//! [`LIBRERIA`]. Un lavoro fermo sul lucchetto della libreria non deve tenere
//! indietro un `volume`, che il lucchetto della libreria non lo chiede.
//!
//! # Cosa va in una corsia, e cosa no
//!
//! In una corsia vanno i comandi che **scrivono uno stato dove l'ultimo vince**
//! (un volume, una pausa, una stella, una preferenza, il fattore di zoom): se
//! due si scambiano, resta il valore sbagliato. Le letture, e tutto quel che
//! tocca il portachiavi, il disco, la rete o un processo, vanno per
//! `in_disparte` e non qui: un portachiavi che non risponde non deve fermare le
//! stelle.
//!
//! Un lavoro di una corsia **non chiama mai un comando che passa per una
//! corsia**, né la sua né l'altra: aspetterebbe se stesso. Il lavoro vero sta
//! in un nucleo sincrono (`..._ora`), che i comandi chiamano dalla corsia e che
//! gli altri chiamanti — i tasti multimediali, la sessione, il vassoio — chiamano
//! dai loro fili.
//!
//! # Cosa resta fuori dall'ordine
//!
//! Una corsia ordina **solo i comandi che passano da lei**. Restano sul pool, e
//! fra loro e rispetto alle corsie non hanno nessun ordine (S7-4):
//!
//! - **le riprese e i gesti che aprono un file** — `riprendi`, `alterna` quando
//!   riprende, `prossimo`, `precedente`, `suona`, `radio`, `coda_vai`: non possono
//!   stare in una corsia, perché aprire un brano da un NAS che dorme dura fino a
//!   quindici secondi, e la `pausa` dietro di loro aspetterebbe lo stesso tempo. Il
//!   loro ordine rispetto alla pausa lo tiene `Lettore::pausa_chiesta`, un
//!   contatore che chi apre un brano legge una volta, prima di aspettare, e porta
//!   fino alla consegna (S7-1);
//! - **i gesti sulla coda** — `coda_accoda`, `coda_dopo`, `coda_togli`,
//!   `coda_riordina`: due premuti durante una scansione aspettano la libreria in
//!   `voci_da_libreria` e si svegliano nell'ordine che sceglie il sistema. Premuti
//!   a intervalli umani non si scambiano; solo in una sessione, dove un host che
//!   preme due «Accoda» di fila con la libreria tenuta per secondi può vederli
//!   scambiati;
//! - **i setter delle preferenze** (`latenza`, `dissolvenza`, `spettro_bande_scegli`,
//!   la normalizzazione…): si mettono in fila sul lucchetto `preferenze`, che dà
//!   l'esclusione ma non l'ordine. Non arrivano a raffica da un cursore;
//! - **`eq_preset_salva`** leggeva `lettore.eq` dal pool e poteva passare davanti a
//!   un `equalizzatore` ancora in coda dietro un lettore tenuto: oggi legge la
//!   curva **sulla corsia del lettore**, dopo quelli già arrivati, e scrive nella
//!   libreria dal pool (S7-4).
//!
//! # Cosa non promette
//!
//! L'ordine vale **dal momento in cui il comando comincia a girare**: Tauri
//! manda ogni comando `async` a `async_runtime::spawn`, e due comandi partiti a
//! pochi microsecondi di distanza possono cominciare scambiati. I comandi della
//! pagina arrivano a decine di millisecondi l'uno dall'altro (`PASSO_VOLUME_MS`,
//! `PASSO_INVIO`) o aspettano la risposta del precedente, e il difetto che la
//! corsia toglie — i lavori che si scambiano **mentre aspettano un lucchetto**,
//! cioè per secondi — non ha questo limite.
//!
//! Un lavoro che aspetta un lucchetto tiene fermi quelli che gli stanno dietro:
//! quelli dietro lo vorrebbero comunque, ma `pausa` e `vai_a` passano prima da
//! `strada`, che in sessione può aspettare il filo della sessione fino a venti
//! secondi, e in quel tempo anche i `volume` dietro di loro aspettano — com'era
//! quando giravano tutti sul filo della finestra, uno dopo l'altro.

use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

use rymembra_domain::errors::{AppError, ErrorCode};

use crate::errore::{Esito, errore};

/// Un lavoro in coda: già impacchettato con il suo canale di risposta.
type Lavoro = Box<dyn FnOnce() + Send + 'static>;

/// Una coda di lavori che si eseguono uno alla volta, nell'ordine di arrivo. Vedi
/// il `//!` del modulo.
pub struct Corsia {
    /// Il nome del filo: nel diario dei panici e nella salute dei fili
    /// (`stato::fili_salute`).
    nome: &'static str,
    /// Dove si mettono i lavori. `None` se il filo non è potuto partire.
    ///
    /// Si crea al primo lavoro: chi non tocca mai un cursore non paga un filo.
    coda: OnceLock<Option<Sender<Lavoro>>>,
}

/// La corsia del lettore: `volume`, `equalizzatore`, `pausa`, `vai_a`,
/// `spettro`, `coda_svuota`.
pub static LETTORE: Corsia = Corsia::nuova("rymembra-corsia-lettore");

/// La corsia della libreria: i comandi che scrivono una preferenza, un voto,
/// una playlist, dove l'ultimo vince.
pub static LIBRERIA: Corsia = Corsia::nuova("rymembra-corsia-libreria");

impl Corsia {
    /// Una corsia vuota. Il filo parte al primo lavoro.
    #[must_use]
    pub const fn nuova(nome: &'static str) -> Self {
        Self {
            nome,
            coda: OnceLock::new(),
        }
    }

    /// Mette `lavoro` in fondo alla coda. `Err` rende il lavoro se la coda non
    /// c'è: il filo non è partito, o è caduto portandosi via il ricevitore.
    fn metti_in_coda(&self, lavoro: Lavoro) -> Result<(), Lavoro> {
        let coda = self.coda.get_or_init(|| {
            let (mittente, ricevitore) = channel::<Lavoro>();
            std::thread::Builder::new()
                .name(self.nome.to_owned())
                .spawn(move || {
                    for lavoro in ricevitore {
                        lavoro();
                    }
                })
                .ok()
                .map(|_| mittente)
        });
        match coda {
            Some(mittente) => mittente.send(lavoro).map_err(|errore| errore.0),
            None => Err(lavoro),
        }
    }

    /// Mette il lavoro in coda e rende la risposta che arriverà. È la metà
    /// sincrona di [`Corsia::in_fila`]: separata perché la posizione in coda si
    /// decide **qui**, e una prova può provarla senza un runtime.
    fn accoda<T: Send + 'static>(
        &self,
        nome: &'static str,
        lavoro: impl FnOnce() -> T + Send + 'static,
    ) -> Result<tauri::async_runtime::Receiver<Option<T>>, AppError> {
        // Un posto, e l'unico invio: `try_send` non aspetta mai.
        let (consegna, risposta) = tauri::async_runtime::channel::<Option<T>>(1);
        let filo = self.nome;
        let impacchettato: Lavoro = Box::new(move || {
            // `None` se il lavoro è caduto in panico: l'ha già scritto
            // `sorvegliato`, e chi aspetta riceve un errore e non un silenzio.
            let esito = crate::stato::sorvegliato(filo, lavoro);
            let _ = consegna.try_send(esito);
        });
        self.metti_in_coda(impacchettato).map_err(|_| {
            AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause(format!("la corsia «{}» non è partita", self.nome))
        })?;
        Ok(risposta)
    }

    /// Esegue `lavoro` sulla corsia, dietro a quelli arrivati prima, e ne aspetta
    /// l'esito.
    ///
    /// `nome` è quel che finisce nella causa dell'errore, scritto per essere letto
    /// in un diario: «volume», non «lavoro 3».
    ///
    /// # Errori
    ///
    /// `internal.aborted` se il lavoro è caduto in panico, o se la corsia non
    /// c'è più. Un errore e non un panico nostro: chi ha chiesto il comando
    /// merita una risposta.
    pub async fn in_fila<T: Send + 'static>(
        &self,
        nome: &'static str,
        lavoro: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, AppError> {
        let mut risposta = self.accoda(nome, lavoro)?;
        match risposta.recv().await {
            Some(Some(valore)) => Ok(valore),
            Some(None) => Err(AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause("il lavoro è caduto in panico")),
            None => Err(AppError::new(ErrorCode::InternalAborted {
                what: Some(nome.to_owned()),
            })
            .with_cause(format!(
                "la corsia «{}» ha lasciato cadere il lavoro",
                self.nome
            ))),
        }
    }

    /// [`Corsia::in_fila`] per un comando: il lavoro restituisce un [`Esito`] e
    /// la caduta del lavoro diventa `internal.aborted`. È la gemella di
    /// [`crate::disparte::comanda`], con l'ordine di arrivo in più.
    ///
    /// # Errori
    ///
    /// Quelli del lavoro; `internal.aborted` se cade o se la corsia non c'è.
    pub async fn comanda<T: Send + 'static>(
        &self,
        nome: &'static str,
        lavoro: impl FnOnce() -> Esito<T> + Send + 'static,
    ) -> Esito<T> {
        self.in_fila(nome, lavoro).await.map_err(errore)?
    }
}

#[cfg(test)]
mod prove {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;

    /// Una corsia tutta per la prova: le due vere sono statiche e condivise
    /// con ogni altra prova del processo.
    fn corsia(nome: &'static str) -> Corsia {
        Corsia::nuova(nome)
    }

    /// Aspetta una risposta senza un'app: il runtime globale di Tauri esiste
    /// anche nelle prove.
    fn aspetta<T>(mut risposta: tauri::async_runtime::Receiver<Option<T>>) -> Option<T> {
        tauri::async_runtime::block_on(risposta.recv()).flatten()
    }

    /// I lavori in coda su un lucchetto occupato escono nell'ordine in cui sono
    /// stati messi in coda, e uno alla volta.
    ///
    /// È il difetto che la corsia toglie. Sul pool bloccante, con un lucchetto
    /// tenuto da fuori, i lavori si risvegliano tutti insieme e quello che prende
    /// il lucchetto per primo lo sceglie il sistema operativo: l'ultimo volume
    /// può essere il primo.
    #[test]
    fn i_lavori_escono_nell_ordine_e_uno_alla_volta() {
        let corsia = corsia("rymembra-prova-ordine");
        let lucchetto = Arc::new(Mutex::new(()));
        let usciti = Arc::new(Mutex::new(Vec::<usize>::new()));
        let insieme = Arc::new(AtomicUsize::new(0));
        let massimo = Arc::new(AtomicUsize::new(0));

        let tenuto = lucchetto.lock().unwrap();
        let risposte: Vec<_> = (0..12)
            .map(|numero| {
                let lucchetto = Arc::clone(&lucchetto);
                let usciti = Arc::clone(&usciti);
                let insieme = Arc::clone(&insieme);
                let massimo = Arc::clone(&massimo);
                corsia
                    .accoda("prova", move || {
                        let ora = insieme.fetch_add(1, Ordering::SeqCst) + 1;
                        massimo.fetch_max(ora, Ordering::SeqCst);
                        // Il lucchetto occupato da fuori: tutti ci arrivano
                        // mentre è chiuso.
                        drop(lucchetto.lock().unwrap());
                        std::thread::sleep(Duration::from_millis(2));
                        usciti.lock().unwrap().push(numero);
                        insieme.fetch_sub(1, Ordering::SeqCst);
                        numero
                    })
                    .expect("la corsia parte")
            })
            .collect();
        std::thread::sleep(Duration::from_millis(50));
        drop(tenuto);

        let risposte: Vec<usize> = risposte
            .into_iter()
            .map(|r| aspetta(r).expect("risponde"))
            .collect();
        assert_eq!(risposte, (0..12).collect::<Vec<_>>());
        assert_eq!(*usciti.lock().unwrap(), (0..12).collect::<Vec<_>>());
        assert_eq!(
            massimo.load(Ordering::SeqCst),
            1,
            "due lavori della stessa corsia girano insieme"
        );
    }

    /// Il controllo della prova qui sopra: gli stessi lavori sul pool bloccante
    /// **si sovrappongono**, tutti dentro insieme mentre il lucchetto è chiuso.
    /// È la ragione per cui `volume` e compagni non possono passare da
    /// [`crate::disparte::in_disparte`]: sul pool l'ordine in cui escono lo
    /// sceglie il sistema operativo.
    #[test]
    fn sul_pool_gli_stessi_lavori_si_sovrappongono() {
        let lucchetto = Arc::new(Mutex::new(()));
        let insieme = Arc::new(AtomicUsize::new(0));
        let massimo = Arc::new(AtomicUsize::new(0));

        let tenuto = lucchetto.lock().unwrap();
        let lavori: Vec<_> = (0..12)
            .map(|_| {
                let lucchetto = Arc::clone(&lucchetto);
                let insieme = Arc::clone(&insieme);
                let massimo = Arc::clone(&massimo);
                tauri::async_runtime::spawn_blocking(move || {
                    let ora = insieme.fetch_add(1, Ordering::SeqCst) + 1;
                    massimo.fetch_max(ora, Ordering::SeqCst);
                    drop(lucchetto.lock().unwrap());
                    insieme.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(100));
        drop(tenuto);
        for lavoro in lavori {
            tauri::async_runtime::block_on(lavoro).expect("il lavoro finisce");
        }
        assert!(
            massimo.load(Ordering::SeqCst) > 1,
            "il pool non ha fatto girare due lavori insieme: la prova qui sopra non prova niente"
        );
    }

    /// Un lavoro che cade in panico dà `internal.aborted` a chi l'ha chiesto, e
    /// la corsia va avanti: il lavoro dopo di lui gira.
    #[test]
    fn un_panico_non_porta_via_la_corsia() {
        let corsia = corsia("rymembra-prova-panico");
        let cadente = corsia
            .accoda("prova", || -> u8 { panic!("caduto di proposito") })
            .expect("la corsia parte");
        let dopo = corsia
            .accoda("prova", || 7_u8)
            .expect("la corsia c'è ancora");
        assert_eq!(aspetta(cadente), None, "chi ha chiesto lo viene a sapere");
        assert_eq!(aspetta(dopo), Some(7), "il lavoro dopo gira");

        let esito = tauri::async_runtime::block_on(
            corsia.in_fila("prova", || -> u8 { panic!("caduto di nuovo") }),
        );
        let errore = esito.expect_err("il panico è un errore");
        assert_eq!(errore.code().kind().code(), "internal.aborted");
        // Ancora viva.
        assert_eq!(
            tauri::async_runtime::block_on(corsia.in_fila("prova", || 3_u8)).ok(),
            Some(3)
        );
    }

    /// Una corsia senza coda — il filo non è partito, o è caduto — risponde
    /// `internal.aborted` subito, e non lascia una promessa in sospeso.
    #[test]
    fn una_corsia_morta_risponde_con_un_errore() {
        let corsia = corsia("rymembra-prova-morta");
        let (mittente, ricevitore) = channel::<Lavoro>();
        drop(ricevitore);
        corsia
            .coda
            .set(Some(mittente))
            .unwrap_or_else(|_| panic!("la corsia di prova era già partita"));
        let esito = tauri::async_runtime::block_on(corsia.in_fila("volume", || 1_u8));
        let errore = esito.expect_err("la corsia non c'è");
        assert_eq!(errore.code().kind().code(), "internal.aborted");

        let senza_filo = corsia_senza_filo();
        let esito = tauri::async_runtime::block_on(senza_filo.in_fila("volume", || 1_u8));
        assert!(
            esito.is_err(),
            "senza filo è un errore, non un lavoro perso"
        );
    }

    /// Una corsia il cui filo non è potuto partire.
    fn corsia_senza_filo() -> Corsia {
        let corsia = corsia("rymembra-prova-senza-filo");
        corsia
            .coda
            .set(None)
            .unwrap_or_else(|_| panic!("la corsia di prova era già partita"));
        corsia
    }

    /// Due corsie non si aspettano: un lavoro fermo sulla prima non tiene
    /// indietro uno della seconda. È il perché sono due.
    #[test]
    fn le_corsie_non_si_aspettano() {
        let ferma = corsia("rymembra-prova-ferma");
        let libera = corsia("rymembra-prova-libera");
        let lucchetto = Arc::new(Mutex::new(()));
        let tenuto = lucchetto.lock().unwrap();
        let bloccata = {
            let lucchetto = Arc::clone(&lucchetto);
            ferma
                .accoda("prova", move || drop(lucchetto.lock().unwrap()))
                .expect("la corsia parte")
        };
        let passata = libera.accoda("prova", || 9_u8).expect("la corsia parte");
        assert_eq!(aspetta(passata), Some(9));
        drop(tenuto);
        assert_eq!(aspetta(bloccata), Some(()));
    }
}
