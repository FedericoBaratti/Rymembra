//! Le uscite che ci sono, e quale prendere.
//!
//! # Perché un modulo e non due righe dentro `uscita.rs`
//!
//! Perché la parte che conta — *quale* dispositivo prendere, dato quel che c'è
//! e quel che si voleva — è una funzione pura su una fetta, e una funzione pura
//! si prova. Il resto di `uscita.rs` no: apre una scheda audio, e su una
//! macchina di CI non ce n'è nessuna. Tenere [`scegli`] separata da [`elenco`]
//! è ciò che permette di provare la regola senza avere l'hardware sotto.
//!
//! # L'identità è un nome, e i nomi sono fragili
//!
//! Ricordare «il dispositivo che ho scelto» vuol dire ricordare una stringa, e
//! la stringa che si ricorda è il nome — su WASAPI il `FriendlyName`
//! dell'endpoint, lo stesso che cpal 0.15 restituiva da `name()` e che cpal
//! 0.18 mette in [`cpal::DeviceDescription::name`]. Un nome è:
//!
//! - **tradotto** — «Altoparlanti» su un sistema italiano, «Speakers» su uno
//!   inglese, e cambiare la lingua di Windows cambia la preferenza salvata;
//! - **non unico** — due schede possono chiamarsi tutte e due «Altoparlanti»;
//! - **non eterno** — un aggiornamento del driver può riscriverlo.
//!
//! ## E perché non l'identificatore stabile, adesso che c'è
//!
//! Da cpal 0.17 `Device::id()` esiste, e su WASAPI è l'ID dell'endpoint: non
//! tradotto, unico, e sopravvive ai riavvii. Qui si usa per l'unica domanda a
//! cui un nome non sa rispondere — **quale** dei due «Altoparlanti» è il
//! predefinito, vedi [`elenco`] — e l'apertura prende il `Device` proprio
//! dall'enumerazione in cui è stato scelto, non ripescandolo per nome.
//!
//! L'identità che si **salva** resta però il nome, e non per pigrizia. È già
//! scritta così in ogni `player.output` di chi ha scelto un'uscita prima di
//! questa versione, ed è la stessa stringa che il motore restituisce da
//! `Motore::dispositivo` e che la finestra stampa nell'avviso «il suono si è
//! spostato su…». Spostare l'identità sull'ID vuol dire far viaggiare accanto
//! a lui un nome da mostrare fin dentro il motore, e migrare le preferenze già
//! salvate: un lavoro suo, non un effetto collaterale di un aggiornamento di
//! libreria.
//!
//! Fino ad allora la risposta onesta resta comportarsi bene quando il nome non
//! corrisponde più: [`scegli`] prende **il primo** che corrisponde e, se non
//! corrisponde nessuno, ripiega sul predefinito **senza dirlo come errore**.
//! Una preferenza che non si può più onorare non è un guasto: è un cavo
//! staccato.
//!
//! # Perché non ci sono notifiche, e si guarda invece di essere avvisati
//!
//! `HostTrait`, anche in cpal 0.18, sa enumerare e basta: nessun gancio a cui
//! appendersi per sapere che l'**elenco** è cambiato. Chi vuole accorgersene
//! deve rileggerlo e confrontarlo con quello di prima — ed è quel che fa il filo
//! `rymembra-dispositivi` nell'applicazione. Per questo [`Dispositivo`] è
//! `PartialEq`: il confronto fra due elenchi *è* il rilevamento.
//!
//! Il solo avviso che cpal 0.18 dà riguarda i flussi aperti sul **predefinito
//! virtuale** (`default_output_device`): quando il predefinito cambia, la
//! callback d'errore riceve qualcosa. Ma su WASAPI la 0.18.2 riceve
//! `StreamInvalidated` — «ricostruisci il flusso» — e il suo registro delle
//! modifiche dice espressamente che il flusso **non** segue il dispositivo
//! nuovo da solo. È quindi lo stesso lavoro di ricostruzione che il filo fa già,
//! con in più un secondo modo di arrivarci; e il flusso qui si apre su un
//! dispositivo preciso, che è l'unico modo in cui «ho fissato il DAC» e «seguo
//! il sistema» si possono comportare in modo diverso.

use cpal::traits::{DeviceTrait as _, HostTrait as _};

/// Un'uscita audio, come la vede cpal in questo istante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispositivo {
    /// Il nome cpal, che qui fa anche da identità. Vedi la nota in testa al
    /// modulo su quanto poco sia un'identità, e sul perché non è ancora l'ID.
    pub id: String,
    /// Come chiamarlo a schermo. Oggi è `id`, e sono due campi lo stesso:
    /// il giorno che l'identità smetterà di essere il nome — su Android lo è
    /// già, AAudio ha degli interi — questo resta quel che si legge.
    pub nome: String,
    /// È quello che il sistema usa adesso di suo.
    pub predefinito: bool,
}

/// Le uscite che ci sono adesso.
///
/// **Un elenco vuoto non è un errore.** Un computer senza scheda audio esiste —
/// una macchina di CI, un server, un portatile con tutte le uscite disabilitate
/// in Gestione dispositivi — e chi chiama vuole sapere cosa c'è. Anche un
/// errore di enumerazione diventa un elenco vuoto: «non lo so» e «niente»
/// portano chi chiama a fare la stessa cosa, e distinguerli vorrebbe dire un
/// ramo d'errore che nessuno saprebbe cosa farsene.
///
/// I dispositivi che non sanno dire il proprio nome si scartano: senza nome non
/// c'è né identità da salvare né riga da disegnare.
#[must_use]
pub fn elenco() -> Vec<Dispositivo> {
    con_i_device()
        .into_iter()
        .map(|(dispositivo, _)| dispositivo)
        .collect()
}

/// Le uscite che ci sono adesso, ognuna col `Device` di cpal da cui viene.
///
/// È [`elenco`] prima di buttare via i `Device`, e serve all'apertura: chi apre
/// sceglie su questo stesso elenco e apre **il `Device` scelto**, invece di
/// rileggere l'elenco e ripescarlo per nome. Fra le due letture il mondo poteva
/// cambiare, e con due schede dallo stesso nome il ripescaggio prendeva la
/// prima anche quando si era scelta la seconda.
///
/// # Quale è il predefinito
///
/// Per **ID**, non per nome: con due «Altoparlanti» il nome non sa dire quale
/// dei due sia quello del sistema, l'ID dell'endpoint sì. Il predefinito si
/// legge **prima**, una volta sola: chiederlo dentro il ciclo vorrebbe dire una
/// chiamata di sistema per dispositivo, e — peggio — un elenco che potrebbe
/// marcarne due se il sistema cambiasse idea a metà.
///
/// Se l'ID non si riesce a leggere si torna al confronto per nome, e allora
/// vale la regola di prima: **il primo** che porta quel nome, non tutti. Una
/// bandiera sola resta l'invariante — [`scegli`] prende il primo predefinito
/// che trova, e chi sorveglia si fida che ce ne sia uno solo per decidere se
/// vale la pena riaprire.
pub(crate) fn con_i_device() -> Vec<(Dispositivo, cpal::Device)> {
    let host = cpal::default_host();
    let di_sistema = host.default_output_device();
    let id_predefinito = di_sistema.as_ref().and_then(|d| d.id().ok());
    let nome_predefinito = di_sistema
        .as_ref()
        .and_then(|d| d.description().ok())
        .map(|descrizione| descrizione.name().to_owned());

    let Ok(uscite) = host.output_devices() else {
        return Vec::new();
    };

    let mut gia_marcato = false;
    uscite
        .filter_map(|device| {
            let nome = device.description().ok()?.name().to_owned();
            let e_il_predefinito = !gia_marcato
                && match (id_predefinito.as_ref(), device.id().ok()) {
                    (Some(di_sistema), Some(suo)) => *di_sistema == suo,
                    _ => nome_predefinito.as_ref() == Some(&nome),
                };
            gia_marcato |= e_il_predefinito;
            Some((
                Dispositivo {
                    predefinito: e_il_predefinito,
                    id: nome.clone(),
                    nome,
                },
                device,
            ))
        })
        .collect()
}

/// Quale prendere, fra quelli che ci sono.
///
/// `voluto` è la preferenza dell'utente: `None` vuol dire «quello di sistema».
///
/// # La regola
///
/// 1. Se si voleva un nome preciso e c'è, quello — **il primo**, se per caso
///    due si chiamano uguale. Sbagliare gemello è meglio che non suonare.
/// 2. Altrimenti il predefinito, che è il ripiego di ogni caso storto: la
///    preferenza punta a una scheda staccata, oppure non c'era preferenza.
/// 3. Se nell'elenco non c'è nemmeno un predefinito — succede: un elenco può
///    arrivare senza che nessuno sia marcato — il primo che c'è.
/// 4. Elenco vuoto: `None`, e chi chiama dirà che non c'è audio.
#[must_use]
pub fn scegli<'a>(elenco: &'a [Dispositivo], voluto: Option<&str>) -> Option<&'a Dispositivo> {
    scegli_posto(elenco, voluto).and_then(|posto| elenco.get(posto))
}

/// Quale prendere per **riaprire** dopo un guasto, fra quelli che ci sono.
///
/// La stessa regola di [`scegli`] con un'eccezione sola: una preferenza
/// scritta che nell'elenco non c'è **non ripiega**, e la risposta è `None`.
///
/// # Perché due regole
///
/// Perché aprire e riaprire sono due domande diverse. All'avvio, con il DAC
/// spento, chi apre Rymembra vuole sentire qualcosa: il ripiego sul predefinito è
/// la risposta giusta, ed è quella di [`scegli`]. Dopo, a musica in corso, la
/// stessa scheda fissata che sparisce — le cuffie Bluetooth che si staccano — è
/// la situazione per cui la si era fissata: chi l'ha scelta non vuole che il
/// suono passi agli altoparlanti dell'ufficio. Qui si aspetta che torni.
///
/// Senza preferenza si segue il sistema, come in [`scegli`]: chi ha lasciato
/// «predefinito» ha chiesto proprio quello.
#[must_use]
pub fn scegli_per_riaprire<'a>(
    elenco: &'a [Dispositivo],
    voluto: Option<&str>,
) -> Option<&'a Dispositivo> {
    match voluto {
        Some(nome) => elenco.iter().find(|d| d.id == nome),
        None => scegli(elenco, None),
    }
}

/// [`scegli`], ma dice **dove** sta il prescelto invece di prestarlo.
///
/// Serve all'apertura, che sceglie su [`con_i_device`] e deve arrivare al
/// `Device` accanto al [`Dispositivo`] scelto: con due gemelli dallo stesso
/// nome, il posto è l'unica cosa che li distingue.
pub(crate) fn scegli_posto(elenco: &[Dispositivo], voluto: Option<&str>) -> Option<usize> {
    if let Some(nome) = voluto
        && let Some(posto) = elenco.iter().position(|d| d.id == nome)
    {
        return Some(posto);
    }
    elenco
        .iter()
        .position(|d| d.predefinito)
        .or_else(|| (!elenco.is_empty()).then_some(0))
}

#[cfg(test)]
mod prove {
    use super::*;

    /// Un elenco finto, per provare la regola senza una scheda audio sotto.
    fn finti(nomi: &[(&str, bool)]) -> Vec<Dispositivo> {
        nomi.iter()
            .map(|(nome, predefinito)| Dispositivo {
                id: (*nome).to_owned(),
                nome: (*nome).to_owned(),
                predefinito: *predefinito,
            })
            .collect()
    }

    #[test]
    fn il_voluto_vince_sul_predefinito() {
        let elenco = finti(&[("Altoparlanti", true), ("DAC USB", false)]);
        let scelto = scegli(&elenco, Some("DAC USB")).expect("c'è");
        assert_eq!(scelto.id, "DAC USB");
    }

    #[test]
    fn un_voluto_che_non_c_e_piu_ripiega_sul_predefinito() {
        // Il caso vero: il DAC è staccato, la preferenza è rimasta scritta.
        // Deve suonare dagli altoparlanti, non tacere.
        let elenco = finti(&[("Altoparlanti", true), ("Cuffie", false)]);
        let scelto = scegli(&elenco, Some("DAC USB")).expect("c'è il ripiego");
        assert_eq!(scelto.id, "Altoparlanti");
    }

    #[test]
    fn per_riaprire_una_scheda_fissata_che_non_c_e_non_ripiega() {
        // Le cuffie fissate si sono staccate: si aspetta che tornino, e il
        // suono non passa agli altoparlanti.
        let elenco = finti(&[("Altoparlanti", true), ("DAC USB", false)]);
        assert!(scegli_per_riaprire(&elenco, Some("Cuffie")).is_none());
        // Tornate: si riapre su di loro.
        let tornate = finti(&[("Altoparlanti", true), ("Cuffie", false)]);
        assert_eq!(
            scegli_per_riaprire(&tornate, Some("Cuffie")).map(|d| d.id.as_str()),
            Some("Cuffie")
        );
        // Senza preferenza si segue il sistema, come all'apertura.
        assert_eq!(
            scegli_per_riaprire(&elenco, None).map(|d| d.id.as_str()),
            Some("Altoparlanti")
        );
        // E l'apertura a freddo ripiega ancora: meglio suonare che nascere muti.
        assert_eq!(
            scegli(&elenco, Some("Cuffie")).map(|d| d.id.as_str()),
            Some("Altoparlanti")
        );
    }

    #[test]
    fn senza_preferenza_si_prende_il_predefinito() {
        let elenco = finti(&[("Cuffie", false), ("Altoparlanti", true)]);
        let scelto = scegli(&elenco, None).expect("c'è");
        assert_eq!(scelto.id, "Altoparlanti");
    }

    #[test]
    fn senza_nessun_predefinito_marcato_si_prende_il_primo() {
        let elenco = finti(&[("Cuffie", false), ("Altoparlanti", false)]);
        let scelto = scegli(&elenco, None).expect("c'è");
        assert_eq!(scelto.id, "Cuffie");
    }

    #[test]
    fn due_dispositivi_con_lo_stesso_nome_danno_il_primo() {
        // Su WASAPI succede davvero. Col nome come identità non c'è modo di
        // spareggiarli: si prende il primo e si suona.
        let elenco = finti(&[("Altoparlanti", false), ("Altoparlanti", true)]);
        let scelto = scegli(&elenco, Some("Altoparlanti")).expect("c'è");
        assert!(!scelto.predefinito, "il primo, non il predefinito");
    }

    #[test]
    fn fra_due_gemelli_il_predefinito_si_apre_per_posto() {
        // Senza preferenza, invece, il predefinito è marcato per ID — quindi
        // quello giusto dei due — e l'apertura deve arrivare a **lui**, non al
        // primo che porta il suo nome. Il posto è l'unica cosa che li distingue.
        let elenco = finti(&[("Altoparlanti", false), ("Altoparlanti", true)]);
        assert_eq!(scegli_posto(&elenco, None), Some(1));
    }

    #[test]
    fn il_posto_e_scegli_dicono_la_stessa_cosa() {
        // Due funzioni e una regola: se si scostassero, l'uscita aperta non
        // sarebbe quella che il sorvegliante crede scelta, e si riaprirebbe in
        // cerchio.
        let elenco = finti(&[("Cuffie", false), ("Altoparlanti", true), ("DAC", false)]);
        for voluto in [None, Some("DAC"), Some("Cuffie"), Some("sparito")] {
            let per_posto = scegli_posto(&elenco, voluto).and_then(|p| elenco.get(p));
            assert_eq!(per_posto, scegli(&elenco, voluto), "voluto {voluto:?}");
        }
    }

    #[test]
    fn un_elenco_vuoto_non_da_niente() {
        assert!(scegli(&[], None).is_none());
        assert!(scegli(&[], Some("DAC USB")).is_none());
        assert!(scegli_posto(&[], None).is_none());
    }

    #[test]
    fn l_elenco_vero_non_cade_nemmeno_senza_scheda_audio() {
        // Non si può affermare *cosa* torna — dipende dalla macchina, e in CI
        // è quasi sempre vuoto. Si afferma che chiederlo è sempre lecito, che
        // è la promessa scritta su `elenco`.
        let visti = elenco();
        // E che al massimo uno è il predefinito: è l'invariante su cui
        // `scegli` costruisce il ripiego.
        assert!(visti.iter().filter(|d| d.predefinito).count() <= 1);
    }
}
