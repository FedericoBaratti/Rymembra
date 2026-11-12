//! La chiusura che aspetta lo Studio, da qualunque strada arrivi (R14-7).
//!
//! # Il difetto
//!
//! Lo Studio salva 800 ms dopo l'ultima modifica. Un nome scritto e subito
//! dopo la chiusura, e riaprendo il nome non c'era. La X della barra del
//! titolo lo sa già: aspetta, al più tre secondi, che lo Studio abbia scritto
//! (`studio/primaDiChiudere.ts`), e solo dopo chiede la chiusura. Le altre tre
//! strade no: Alt+F4, «Chiudi finestra» dalla barra delle applicazioni ed
//! «Esci» dal vassoio chiudono dal nucleo, e la pagina non lo sapeva nemmeno.
//!
//! # Come si aspetta
//!
//! La pagina dice al nucleo quando ha qualcosa da scrivere prima di chiudere
//! ([`chiusura_da_scrivere`]: lo Studio è aperto). Solo allora una chiusura che
//! arriva dal nucleo si **rinvia**: la si impedisce, si manda alla pagina
//! `finestra:scrivi`, e la pagina — finito di scrivere — risponde
//! [`chiusura_scritto`]; il nucleo rifà allora il gesto rinviato, chiudere o
//! uscire. Senza lo Studio aperto non cambia niente: nessun giro in più.
//!
//! # La chiusura vera non si perde
//!
//! Una pagina che non risponde — bloccata, ricaricata a metà, caduta — non
//! deve tenere aperta una finestra che si è chiesto di chiudere. Ogni rinvio ha
//! il suo numero e il suo ripiego: dopo [`TETTO`] il nucleo rifà il gesto da
//! sé, se **quel** rinvio è ancora in attesa. E una pagina che si ricarica
//! dimentica di avere qualcosa da scrivere ([`pagina_nuova`]): altrimenti ogni
//! chiusura dopo aspetterebbe il tetto intero.
//!
//! Le regole stanno in [`Chiusura`], senza Tauri, perché si possano provare.
//! Solo sul computer: sul telefono la finestra che si chiude è l'attività, e
//! la musica continua nel servizio (vedi `run` in `lib.rs`).

use std::sync::Mutex;
use std::time::Duration;

use crate::spegnimento::Emette as _;

/// Quanto il nucleo aspetta la pagina prima di rifare il gesto da sé: i tre
/// secondi che la pagina concede allo Studio (`tuttoScritto`), più uno per il
/// giro dell'evento e della risposta.
pub const TETTO: Duration = Duration::from_secs(4);

/// L'evento con cui il nucleo chiede alla pagina di scrivere. Il carico è il
/// numero del rinvio, che la pagina non deve restituire: lo tiene il nucleo.
pub const EVENTO_SCRIVI: &str = "finestra:scrivi";

/// Il gesto che si è rinviato.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    mobile,
    allow(
        dead_code,
        reason = "«Esci» sta nel vassoio, che c'è solo sul computer"
    )
)]
pub enum Gesto {
    /// Chiudere la finestra: Alt+F4, la barra delle applicazioni.
    Chiudi,
    /// Uscire dal programma: «Esci» dal vassoio.
    Esci,
}

/// Che cosa fa una richiesta di chiusura della finestra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decisione {
    /// Passa: la finestra si chiude.
    Passa,
    /// La si impedisce e la finestra si nasconde: il secondo piano è acceso.
    /// La pagina resta viva, e lo Studio finisce di scrivere da sé.
    Nascondi,
    /// La si impedisce, e si chiede alla pagina di scrivere. Il numero è
    /// quello del ripiego.
    Rinvia(u64),
    /// La si impedisce: un rinvio è già in corso, e decide lui.
    Aspetta,
}

#[derive(Debug)]
struct Stato {
    /// La pagina ha qualcosa da scrivere prima di chiudere.
    da_scrivere: bool,
    /// Il rinvio in corso: il suo numero e il gesto da rifare.
    in_attesa: Option<(u64, Gesto)>,
    /// La prossima richiesta di chiusura passa: chi la fa ha già aspettato.
    concessa: bool,
    /// Il numero dell'ultimo rinvio.
    giro: u64,
}

/// Le regole della chiusura rinviata. Una sola per il processo:
/// [`CHIUSURA`].
#[derive(Debug)]
pub struct Chiusura(Mutex<Stato>);

/// Quella dell'applicazione.
pub static CHIUSURA: Chiusura = Chiusura::nuova();

impl Chiusura {
    /// Niente da scrivere, niente in attesa.
    #[must_use]
    pub const fn nuova() -> Self {
        Self(Mutex::new(Stato {
            da_scrivere: false,
            in_attesa: None,
            concessa: false,
            giro: 0,
        }))
    }

    fn stato(&self) -> std::sync::MutexGuard<'_, Stato> {
        // Un lucchetto avvelenato non deve impedire di chiudere: lo stato è
        // fatto di tre valori, e ognuno resta sensato a metà di un giro.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// La pagina dice se ha qualcosa da scrivere prima di chiudere.
    pub fn da_scrivere(&self, si: bool) {
        self.stato().da_scrivere = si;
    }

    /// La pagina è nata, o rinata: quel che aveva da scrivere non c'è più.
    pub fn pagina_nuova(&self) {
        self.stato().da_scrivere = false;
    }

    /// La X della pagina ha già aspettato lo Studio: la sua chiusura passa.
    pub fn concedi(&self) {
        self.stato().concessa = true;
    }

    /// Un rinvio è in corso: chi spegne il programma deve aspettare lui.
    pub fn rinviata(&self) -> bool {
        self.stato().in_attesa.is_some()
    }

    /// Arriva una richiesta di chiusura della finestra. `nasconde` è il
    /// secondo piano (`vassoio::nasconde`).
    pub fn chiede_chiudi(&self, nasconde: bool) -> Decisione {
        let mut stato = self.stato();
        // Si consuma comunque: una concessione rimasta lì salterebbe l'attesa
        // della chiusura dopo, che nessuno aveva concesso.
        let concessa = std::mem::take(&mut stato.concessa);
        if stato.in_attesa.is_some() {
            return Decisione::Aspetta;
        }
        if nasconde {
            return Decisione::Nascondi;
        }
        if concessa || !stato.da_scrivere {
            return Decisione::Passa;
        }
        stato.giro += 1;
        stato.in_attesa = Some((stato.giro, Gesto::Chiudi));
        Decisione::Rinvia(stato.giro)
    }

    /// Arriva «Esci». `Passa` vuol dire uscire adesso.
    #[cfg_attr(
        mobile,
        allow(
            dead_code,
            reason = "«Esci» sta nel vassoio, che c'è solo sul computer"
        )
    )]
    pub fn chiede_esci(&self) -> Decisione {
        let mut stato = self.stato();
        if let Some((numero, _)) = stato.in_attesa {
            // Uscire vale più che chiudere: quando la pagina avrà scritto, si
            // esce.
            stato.in_attesa = Some((numero, Gesto::Esci));
            return Decisione::Aspetta;
        }
        if !stato.da_scrivere {
            return Decisione::Passa;
        }
        stato.giro += 1;
        stato.in_attesa = Some((stato.giro, Gesto::Esci));
        Decisione::Rinvia(stato.giro)
    }

    /// La pagina ha scritto: il gesto da rifare, se ce n'era uno in attesa.
    pub fn scritto(&self) -> Option<Gesto> {
        let mut stato = self.stato();
        let (_, gesto) = stato.in_attesa.take()?;
        stato.concessa = gesto == Gesto::Chiudi;
        Some(gesto)
    }

    /// Il tempo del rinvio `numero` è finito: il gesto, se quel rinvio è
    /// ancora in attesa. Un rinvio già risolto, o uno più nuovo, non si tocca.
    pub fn scaduto(&self, numero: u64) -> Option<Gesto> {
        let mut stato = self.stato();
        let (in_corso, gesto) = stato.in_attesa?;
        if in_corso != numero {
            return None;
        }
        stato.in_attesa = None;
        stato.concessa = gesto == Gesto::Chiudi;
        Some(gesto)
    }
}

/// Rifà il gesto rinviato.
fn rifai(app: &tauri::AppHandle, gesto: Gesto) {
    use tauri::Manager as _;
    match gesto {
        Gesto::Chiudi => {
            if let Some(finestra) = app.get_webview_window("main") {
                let _ = finestra.close();
            }
        }
        Gesto::Esci => esci_adesso(app),
    }
}

/// Esce davvero: la bandiera **prima** dell'uscita (vedi `vassoio::esci`).
fn esci_adesso(app: &tauri::AppHandle) {
    crate::spegnimento::chiedi();
    app.exit(0);
}

/// La finestra chiede di chiudersi: che cosa fa. Un rinvio parte da qui.
///
/// `nasconde` è il secondo piano (`vassoio::nasconde`). Sul telefono non c'è
/// mai niente da scrivere — la barra del titolo, che lo direbbe, non c'è — e
/// la risposta è sempre quella di prima.
pub fn alla_richiesta(app: &tauri::AppHandle, nasconde: bool) -> Decisione {
    let decisione = CHIUSURA.chiede_chiudi(nasconde);
    if let Decisione::Rinvia(numero) = decisione {
        rinvia(app, numero);
    }
    decisione
}

/// «Esci» dal vassoio: adesso, o quando la pagina avrà scritto.
#[cfg_attr(
    mobile,
    allow(
        dead_code,
        reason = "«Esci» sta nel vassoio, che c'è solo sul computer"
    )
)]
pub fn esci(app: &tauri::AppHandle) {
    match CHIUSURA.chiede_esci() {
        Decisione::Passa => esci_adesso(app),
        Decisione::Rinvia(numero) => rinvia(app, numero),
        Decisione::Aspetta | Decisione::Nascondi => {}
    }
}

/// Un rinvio è partito: lo si dice alla pagina, e si arma il ripiego.
///
/// Prima l'evento, poi il resto: `emetti` tace a uscita cominciata, e per
/// «Esci» l'uscita comincia solo quando la pagina ha scritto.
pub fn rinvia(app: &tauri::AppHandle, numero: u64) {
    app.emetti(EVENTO_SCRIVI, numero);
    let mano = app.clone();
    let partito = std::thread::Builder::new()
        .name("rymembra-chiusura".to_owned())
        .spawn(move || {
            std::thread::sleep(TETTO);
            // Un'uscita già cominciata da un'altra strada fa da sé.
            if crate::spegnimento::in_uscita() {
                return;
            }
            if let Some(gesto) = CHIUSURA.scaduto(numero) {
                crate::nota!(
                    "[chiusura] la pagina non ha detto di aver scritto entro {} s: {gesto:?} lo stesso",
                    TETTO.as_secs()
                );
                rifai(&mano, gesto);
            }
        });
    // Senza un filo per il ripiego il rinvio non si può tenere: lo si
    // risolve subito, come se il tempo fosse finito.
    if let Err(err) = partito
        && let Some(gesto) = CHIUSURA.scaduto(numero)
    {
        crate::nota!("[chiusura] nessun filo per aspettare la pagina ({err}): {gesto:?} subito");
        rifai(app, gesto);
    }
}

/// La pagina dice se ha qualcosa da scrivere prima di chiudere: lo Studio è
/// aperto (`studio/primaDiChiudere.ts`).
#[tauri::command]
pub fn chiusura_da_scrivere(si: bool) {
    CHIUSURA.da_scrivere(si);
}

/// La pagina ha scritto quel che `finestra:scrivi` le chiedeva: si rifà il
/// gesto rinviato, se è ancora in attesa.
#[tauri::command]
pub fn chiusura_scritto(app: tauri::AppHandle) {
    if let Some(gesto) = CHIUSURA.scritto() {
        rifai(&app, gesto);
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn con_lo_studio() -> Chiusura {
        let c = Chiusura::nuova();
        c.da_scrivere(true);
        c
    }

    #[test]
    fn senza_niente_da_scrivere_la_chiusura_passa_subito() {
        let c = Chiusura::nuova();
        assert_eq!(c.chiede_chiudi(false), Decisione::Passa);
        assert_eq!(c.chiede_esci(), Decisione::Passa);
        assert!(!c.rinviata());
        // Il secondo piano nasconde, come prima.
        assert_eq!(c.chiede_chiudi(true), Decisione::Nascondi);
    }

    #[test]
    fn alt_f4_con_lo_studio_aperto_aspetta_che_la_pagina_abbia_scritto() {
        // R14-7: Alt+F4 e la barra delle applicazioni chiudevano dal nucleo
        // senza aspettare il salvataggio dello Studio.
        let c = con_lo_studio();
        let Decisione::Rinvia(numero) = c.chiede_chiudi(false) else {
            panic!("la chiusura passava senza aspettare lo Studio");
        };
        assert!(c.rinviata());
        // Una seconda richiesta, mentre si aspetta, non ne apre un'altra.
        assert_eq!(c.chiede_chiudi(false), Decisione::Aspetta);
        // La pagina ha scritto: si chiude, e quella chiusura passa.
        assert_eq!(c.scritto(), Some(Gesto::Chiudi));
        assert!(!c.rinviata());
        assert_eq!(c.chiede_chiudi(false), Decisione::Passa);
        // E la concessione vale una volta sola.
        assert!(matches!(c.chiede_chiudi(false), Decisione::Rinvia(n) if n > numero));
    }

    #[test]
    fn esci_dal_vassoio_aspetta_lo_studio_e_poi_esce() {
        let c = con_lo_studio();
        assert!(matches!(c.chiede_esci(), Decisione::Rinvia(_)));
        assert_eq!(c.scritto(), Some(Gesto::Esci));
        assert!(!c.rinviata());
    }

    #[test]
    fn esci_durante_una_chiusura_rinviata_vale_piu_di_chiudere() {
        let c = con_lo_studio();
        assert!(matches!(c.chiede_chiudi(false), Decisione::Rinvia(_)));
        assert_eq!(c.chiede_esci(), Decisione::Aspetta);
        assert_eq!(c.scritto(), Some(Gesto::Esci));
    }

    #[test]
    fn una_pagina_che_non_risponde_non_tiene_aperta_la_finestra() {
        let c = con_lo_studio();
        let Decisione::Rinvia(numero) = c.chiede_chiudi(false) else {
            panic!("rinvio atteso");
        };
        // Il ripiego scatta, e la chiusura che rifà passa.
        assert_eq!(c.scaduto(numero), Some(Gesto::Chiudi));
        assert_eq!(c.chiede_chiudi(false), Decisione::Passa);
        // Una risposta della pagina arrivata dopo non rifà niente.
        assert_eq!(c.scritto(), None);
    }

    #[test]
    fn il_ripiego_di_un_rinvio_vecchio_non_tocca_quello_nuovo() {
        let c = con_lo_studio();
        let Decisione::Rinvia(primo) = c.chiede_chiudi(false) else {
            panic!("rinvio atteso");
        };
        assert_eq!(c.scritto(), Some(Gesto::Chiudi));
        // La chiusura rifatta passa (la concessione), e il programma resta
        // aperto per un'altra ragione — poi una nuova richiesta.
        assert_eq!(c.chiede_chiudi(false), Decisione::Passa);
        let Decisione::Rinvia(secondo) = c.chiede_chiudi(false) else {
            panic!("rinvio atteso");
        };
        assert_ne!(primo, secondo);
        assert_eq!(
            c.scaduto(primo),
            None,
            "il ripiego del primo non chiude il secondo"
        );
        assert!(c.rinviata());
        assert_eq!(c.scaduto(secondo), Some(Gesto::Chiudi));
    }

    #[test]
    fn la_x_della_pagina_ha_gia_aspettato_e_non_aspetta_di_nuovo() {
        let c = con_lo_studio();
        c.concedi();
        assert_eq!(c.chiede_chiudi(false), Decisione::Passa);
        // Col secondo piano la X nasconde, e consuma la concessione lo stesso.
        c.concedi();
        assert_eq!(c.chiede_chiudi(true), Decisione::Nascondi);
        assert!(matches!(c.chiede_chiudi(false), Decisione::Rinvia(_)));
    }

    /// Le tre strade che chiudono dal nucleo passano tutte di qui, e lo
    /// spegnimento non parte sotto una chiusura rinviata.
    #[test]
    fn alt_f4_la_barra_delle_applicazioni_ed_esci_passano_da_qui() {
        let lib = include_str!("lib.rs");
        let finestra = &lib[lib.find(".on_window_event(").expect("il gestore")..];
        let finestra = &finestra[..finestra.find("})").expect("la fine del gestore")];
        assert!(
            finestra.contains("chiusura::alla_richiesta(mano, vassoio::nasconde(mano))"),
            "Alt+F4 e la barra delle applicazioni non aspettano lo Studio"
        );
        let giro = &lib[lib.find("app.run(|mano, evento|").expect("il ciclo")..];
        assert!(
            giro.contains("crate::chiusura::CHIUSURA.rinviata()"),
            "una chiusura rinviata spegnerebbe tutto sotto la pagina"
        );
        let chiudi = &lib[lib.find("fn finestra_chiudi(").expect("la X")..];
        assert!(chiudi[..chiudi.find('}').expect("fine")].contains("CHIUSURA.concedi()"));
        let pronto = &lib[lib.find("fn pronto(").expect("pronto")..];
        assert!(pronto[..pronto.find('}').expect("fine")].contains("CHIUSURA.pagina_nuova()"));
        for comando in [
            "chiusura::chiusura_da_scrivere,",
            "chiusura::chiusura_scritto,",
        ] {
            assert!(lib.contains(comando), "{comando} non è registrato");
        }
        let vassoio = include_str!("vassoio.rs");
        let esci = &vassoio[vassoio.find("fn esci(").expect("Esci")..];
        assert!(
            esci[..esci.find('}').expect("fine")].contains("crate::chiusura::esci(app)"),
            "«Esci» dal vassoio non aspetta lo Studio"
        );
    }

    #[test]
    fn una_pagina_ricaricata_non_ha_piu_niente_da_scrivere() {
        let c = con_lo_studio();
        c.pagina_nuova();
        assert_eq!(c.chiede_chiudi(false), Decisione::Passa);
    }
}
