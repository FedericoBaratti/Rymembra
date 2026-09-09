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
//! cpal 0.15 non offre nessun identificatore stabile: `Device` sa dire solo
//! `name() -> Result<String, DeviceNameError>`. Quindi ricordare «il
//! dispositivo che ho scelto» vuol dire per forza ricordare una stringa, e
//! quella stringa su WASAPI è:
//!
//! - **tradotta** — «Altoparlanti» su un sistema italiano, «Speakers» su uno
//!   inglese, e cambiare la lingua di Windows cambia la preferenza salvata;
//! - **non unica** — due schede possono chiamarsi tutte e due «Altoparlanti»;
//! - **non eterna** — un aggiornamento del driver può riscriverla.
//!
//! Non c'è modo di fare meglio restando dentro cpal, e uscirne vorrebbe dire
//! `IMMNotificationClient` — cioè implementare un'interfaccia COM, cioè
//! `unsafe`, che questo workspace vieta (`unsafe_code = "forbid"`). La risposta
//! onesta è quindi comportarsi bene quando il nome non corrisponde più:
//! [`scegli`] prende **il primo** che corrisponde e, se non corrisponde
//! nessuno, ripiega sul predefinito **senza dirlo come errore**. Una
//! preferenza che non si può più onorare non è un guasto: è un cavo staccato.
//!
//! # Perché non ci sono notifiche, e si guarda invece di essere avvisati
//!
//! `HostTrait`, in cpal 0.15.3, ha `devices`, `default_output_device`,
//! `output_devices` e nient'altro: nessun gancio a cui appendersi per sapere
//! che qualcosa è cambiato. Chi vuole accorgersene deve rileggere l'elenco e
//! confrontarlo con quello di prima — ed è quel che fa il filo
//! `aether-dispositivi` nell'applicazione. Per questo [`Dispositivo`] è
//! `PartialEq`: il confronto fra due elenchi *è* il rilevamento.

use cpal::traits::{DeviceTrait as _, HostTrait as _};

/// Un'uscita audio, come la vede cpal in questo istante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispositivo {
    /// Il nome cpal, che qui fa anche da identità. Vedi la nota in testa al
    /// modulo su quanto poco sia un'identità.
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
/// in Gestione dispositivi — e chi chiama vuole sapere cosa c'è. Anche
/// `DevicesError` diventa un elenco vuoto: «non lo so» e «niente» portano chi
/// chiama a fare la stessa cosa, e distinguerli vorrebbe dire un ramo d'errore
/// che nessuno saprebbe cosa farsene.
///
/// I dispositivi che non sanno dire il proprio nome si scartano: senza nome non
/// c'è né identità da salvare né riga da disegnare.
#[must_use]
pub fn elenco() -> Vec<Dispositivo> {
    let host = cpal::default_host();
    // Il predefinito si legge **prima**, una volta sola: chiederlo dentro il
    // ciclo vorrebbe dire una chiamata di sistema per dispositivo, e — peggio —
    // un elenco che potrebbe marcarne due se il sistema cambiasse idea a metà.
    let predefinito = host
        .default_output_device()
        .and_then(|dispositivo| dispositivo.name().ok());

    let Ok(uscite) = host.output_devices() else {
        return Vec::new();
    };

    // **Il primo** che porta il nome del predefinito, non tutti quelli che lo
    // portano. Su WASAPI due schede possono chiamarsi tutte e due
    // «Altoparlanti», e marcarle entrambe non sarebbe un dettaglio estetico:
    // [`scegli`] prende il primo predefinito che trova, mentre chi sorveglia si
    // fida che ce ne sia uno solo per decidere se val la pena riaprire. Due
    // bandiere alzate vorrebbero dire due risposte alla stessa domanda, e la
    // domanda è «da dove deve uscire il suono».
    let mut gia_marcato = false;
    uscite
        .filter_map(|dispositivo| {
            let nome = dispositivo.name().ok()?;
            let e_il_predefinito = !gia_marcato && predefinito.as_ref() == Some(&nome);
            gia_marcato |= e_il_predefinito;
            Some(Dispositivo {
                predefinito: e_il_predefinito,
                id: nome.clone(),
                nome,
            })
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
///    due si chiamano uguale. Sbagliare gemello è meglio che non suonare, e
///    non c'è niente in cpal con cui spareggiarli.
/// 2. Altrimenti il predefinito, che è il ripiego di ogni caso storto: la
///    preferenza punta a una scheda staccata, oppure non c'era preferenza.
/// 3. Se nell'elenco non c'è nemmeno un predefinito — succede: un elenco può
///    arrivare senza che nessuno sia marcato — il primo che c'è.
/// 4. Elenco vuoto: `None`, e chi chiama dirà che non c'è audio.
#[must_use]
pub fn scegli<'a>(elenco: &'a [Dispositivo], voluto: Option<&str>) -> Option<&'a Dispositivo> {
    if let Some(nome) = voluto
        && let Some(trovato) = elenco.iter().find(|d| d.id == nome)
    {
        return Some(trovato);
    }
    elenco
        .iter()
        .find(|d| d.predefinito)
        .or_else(|| elenco.first())
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
        // Su WASAPI succede davvero. Non c'è modo di spareggiarli restando
        // dentro cpal: si prende il primo e si suona.
        let elenco = finti(&[("Altoparlanti", false), ("Altoparlanti", true)]);
        let scelto = scegli(&elenco, Some("Altoparlanti")).expect("c'è");
        assert!(!scelto.predefinito, "il primo, non il predefinito");
    }

    #[test]
    fn un_elenco_vuoto_non_da_niente() {
        assert!(scegli(&[], None).is_none());
        assert!(scegli(&[], Some("DAC USB")).is_none());
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
