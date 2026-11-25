//! I valori che un documento può portare, e cosa se ne fa quando ne escono.
//!
//! # Perché un posto solo
//!
//! [`crate::documento::interpreta`] è indulgente sui record **malformati** —
//! un voto che non è un voto cade, il documento resta — ma non guardava quelli
//! **fuori dominio**: un voto da sei stelle, un conteggio presso `i64::MAX`,
//! una decisione datata fra dieci anni. Sono JSON validissimo, e ognuno rompeva
//! qualcosa di diverso a valle:
//!
//! - sei stelle violano il `CHECK` di `tracks.rating`, e la transazione della
//!   sincronia cadeva intera **a ogni passata**, finché il file restava lì;
//! - un conteggio enorme fa traboccare la `SUM` con cui la libreria rifà
//!   `play_count` (in SQLite la somma di interi dà errore, non satura): prima
//!   la passata, poi ogni ascolto di quel brano;
//! - un istante nel futuro vince per sempre il «vince il più recente»: ogni
//!   decisione presa qui è più vecchia, e la copia che gli altri ripubblicano
//!   la rimette. Una lapide di playlist dal futuro non lascia più ricreare la
//!   playlist, su nessun dispositivo.
//!
//! I documenti arrivano da due porte — la cartella o il Drive della sincronia,
//! e il `.ryprofile` — e la regola è una: [`nel_dominio`]. Chi la chiama sono
//! [`crate::Motore::passata`], sui documenti degli altri, e il profilo, sul
//! documento che porta. Chi scrive nel database dopo non deve controllare
//! niente, e non controlla. C'è una terza porta, il backup su Drive, che ha un
//! formato suo: applica le stesse regole, con [`ascolti_ammessi`] e il futuro
//! tagliato come per il profilo, in `rymembra_app::backup::nel_dominio`.
//!
//! # Due modi di trattare il futuro, e perché sono due
//!
//! Il profilo si importa una volta, e lì un istante nel futuro si **taglia** ad
//! adesso ([`Futuro::Taglia`]): la decisione entra, datata quando entra, e una
//! decisione presa qui domani la batte.
//!
//! La sincronia invece rilegge lo stesso documento a ogni passata, e tagliare ad
//! «adesso» vorrebbe dire datarlo ogni volta un po' più avanti: vincerebbe a
//! ogni passata su quel che si è deciso qui in mezzo — è lo stesso congelamento,
//! solo più lento — e la libreria si riscriverebbe ogni volta. Lì quel che sta
//! oltre [`MARGINE_FUTURO_MS`] si **rinvia** ([`Futuro::Rinvia`]): non entra in
//! questa fusione, e il documento resta com'è nella memoria del motore. Quando
//! l'orologio di qui arriva a quell'istante, entra da sé. Il margine è per gli
//! orologi che vanno un po' avanti: una decisione presa adesso su un telefono
//! di cinque minuti avanti non deve aspettare cinque minuti.
//!
//! # Cosa non sta qui
//!
//! I numeri degli elementi delle sequenze: quelli li controlla
//! [`crate::Sequenza`] stessa, perché una sequenza si rilegge anche dal
//! database e la regola deve valere in tutti e due i posti.

use crate::documento::Contenuto;

/// Il voto più alto: cinque stelle, come il `CHECK` di `tracks.rating`.
pub const VOTO_MASSIMO: u8 = 5;

/// Il conteggio d'ascolto più alto che si accetta per un brano da un
/// dispositivo: un miliardo.
///
/// Nessuno ascolta un brano un miliardo di volte — sono duemila anni a ciclo
/// continuo — e tanto basta perché la somma non trabocchi mai: anche milioni di
/// voci tutte al tetto, quante ne stanno in un documento, restano lontane da
/// `i64::MAX`.
pub const ASCOLTI_MASSIMI: i64 = 1_000_000_000;

/// Quanto avanti può andare l'orologio di un altro dispositivo: dieci minuti.
///
/// Abbastanza per un telefono o un computer che non sincronizzano l'ora da un
/// po'; poco abbastanza perché una decisione «dal futuro» scritta a mano non
/// congeli niente per più di così.
pub const MARGINE_FUTURO_MS: i64 = 10 * 60 * 1000;

/// Cosa fare di un istante oltre adesso: vedi il `//!`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Futuro {
    /// La sincronia: quel che sta oltre adesso più [`MARGINE_FUTURO_MS`] non
    /// entra in questa fusione, e entrerà quando sarà ora.
    Rinvia,
    /// Il profilo: quel che sta oltre adesso entra datato adesso.
    Taglia,
}

impl Futuro {
    /// L'istante da usare al posto di `at`, o `None` se il record non entra.
    #[must_use]
    pub fn istante(self, at: i64, adesso_ms: i64) -> Option<i64> {
        match self {
            Self::Rinvia => (at <= adesso_ms.saturating_add(MARGINE_FUTURO_MS)).then_some(at),
            Self::Taglia => Some(at.min(adesso_ms)),
        }
    }
}

/// Un conteggio d'ascolto che si può scrivere nella libreria.
#[must_use]
pub fn ascolti_ammessi(quanti: i64) -> bool {
    (0..=ASCOLTI_MASSIMI).contains(&quanti)
}

/// Un contenuto arrivato da fuori, riportato dentro il dominio dei suoi valori.
///
/// Cade il **record** fuori dominio, mai il documento, come per quelli
/// malformati: un voto da sei stelle non porta via gli altri voti. Gli istanti
/// nel futuro si trattano come dice `futuro`.
///
/// Non tocca quel che non ha un dominio da rispettare: gli inventari di skin e
/// bozze, e la data di creazione di una playlist nella sincronia — fra due
/// creazioni vince la più **vecchia**, quindi una dal futuro perde da sola.
#[must_use]
pub fn nel_dominio(mut contenuto: Contenuto, adesso_ms: i64, futuro: Futuro) -> Contenuto {
    contenuto
        .ascolti
        .retain(|_, quanti| ascolti_ammessi(*quanti));
    contenuto
        .ereditati
        .retain(|_, quanti| ascolti_ammessi(*quanti));
    contenuto.voti.retain(|_, voto| voto.v <= VOTO_MASSIMO);

    let istante = |at: &mut i64| match futuro.istante(*at, adesso_ms) {
        Some(ammesso) => {
            *at = ammesso;
            true
        }
        None => false,
    };
    contenuto.ultimo.retain(|_, at| istante(at));
    contenuto.voti.retain(|_, voto| istante(&mut voto.at));
    contenuto
        .preferiti
        .retain(|_, stato| istante(&mut stato.at));
    contenuto
        .posizioni
        .retain(|_, momento| istante(&mut momento.at));
    contenuto.cartelle.retain(|_, stato| istante(&mut stato.at));
    contenuto.lapidi.brani.retain(|_, at| istante(at));
    contenuto.lapidi.playlist.retain(|_, at| istante(at));
    // Di una playlist si rinvia la **testata** — nome, descrizione, regole,
    // l'«automatica» — e non i brani. Rinviarla intera congelava la sua
    // sequenza su ogni altro dispositivo finché l'orologio non arrivava a
    // quella data, e il documento di chi l'ha scritta non passa mai dal
    // dominio (R02-3). La testata rinviata entra con l'orologio a zero: perde
    // contro qualunque testata vera, e anche contro una lapide; quando sarà
    // ora entrerà con la sua data. Una playlist rinviata senza brani da
    // fondere non porta niente, e resta fuori come prima.
    contenuto.playlist.retain(|_, playlist| {
        if istante(&mut playlist.at) {
            return true;
        }
        playlist.at = 0;
        !playlist.sequenza.e_vuota()
    });
    if futuro == Futuro::Taglia {
        for playlist in contenuto.playlist.values_mut() {
            playlist.creata_at = playlist.creata_at.min(adesso_ms);
            playlist.sequenza.taglia_i_tolti(adesso_ms);
        }
    }
    if contenuto
        .skin_attiva
        .as_mut()
        .is_some_and(|scelta| !istante(&mut scelta.at))
    {
        contenuto.skin_attiva = None;
    }
    contenuto
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::registro::{Interruttore, Momento, Scelta, Voto};

    const ADESSO: i64 = 1_700_000_000_000;
    const DOMANI: i64 = ADESSO + 24 * 60 * 60 * 1000;

    fn con_tutto_a(at: i64) -> Contenuto {
        let mut contenuto = Contenuto::default();
        contenuto.ultimo.insert("k".to_owned(), at);
        contenuto.voti.insert("k".to_owned(), Voto { v: 3, at });
        contenuto
            .preferiti
            .insert("k".to_owned(), Interruttore { on: true, at });
        contenuto
            .posizioni
            .insert("k".to_owned(), Momento { ms: 5, at });
        contenuto
            .cartelle
            .insert("D:/Musica".to_owned(), Interruttore { on: true, at });
        contenuto.lapidi.brani.insert("k".to_owned(), at);
        contenuto.lapidi.playlist.insert("p".to_owned(), at);
        contenuto.playlist.insert(
            "q".to_owned(),
            crate::PlaylistSincronizzata {
                at,
                creata_at: at,
                ..crate::PlaylistSincronizzata::default()
            },
        );
        contenuto.skin_attiva = Some(Scelta {
            id: "notte".to_owned(),
            at,
        });
        contenuto
    }

    #[test]
    fn i_valori_fuori_scala_cadono_da_soli() {
        let mut contenuto = Contenuto::default();
        contenuto
            .voti
            .insert("sei".to_owned(), Voto { v: 6, at: 1 });
        contenuto
            .voti
            .insert("cinque".to_owned(), Voto { v: 5, at: 1 });
        contenuto
            .voti
            .insert("zero".to_owned(), Voto { v: 0, at: 1 });
        contenuto.ascolti.insert("enorme".to_owned(), i64::MAX);
        contenuto.ascolti.insert("negativo".to_owned(), -3);
        contenuto
            .ascolti
            .insert("al tetto".to_owned(), ASCOLTI_MASSIMI);
        contenuto
            .ereditati
            .insert("enorme".to_owned(), ASCOLTI_MASSIMI + 1);

        let sano = nel_dominio(contenuto, ADESSO, Futuro::Rinvia);
        let voti: Vec<&str> = sano.voti.keys().map(String::as_str).collect();
        assert_eq!(voti, vec!["cinque", "zero"]);
        let ascolti: Vec<&str> = sano.ascolti.keys().map(String::as_str).collect();
        assert_eq!(ascolti, vec!["al tetto"]);
        assert!(sano.ereditati.is_empty());
    }

    #[test]
    fn il_segno_di_capacita_passa_dal_dominio_intatto() {
        // La copia `sano` che il motore fa di ogni documento altrui è il punto
        // su cui, in una 3.3.x, si leggerà il segno dei pari: se qui cadesse,
        // ogni pari sembrerebbe un PC della 3.2.0.
        let contenuto = Contenuto {
            capacita: crate::documento::capacita_di_questa_versione(),
            ..con_tutto_a(DOMANI)
        };
        let sano = nel_dominio(contenuto, ADESSO, Futuro::Rinvia);
        assert_eq!(
            sano.capacita,
            crate::documento::capacita_di_questa_versione()
        );
        assert!(sano.voti.is_empty(), "e il resto cade come prima");
    }

    #[test]
    fn la_sincronia_rinvia_il_futuro_e_lo_prende_quando_arriva() {
        let rinviato = nel_dominio(con_tutto_a(DOMANI), ADESSO, Futuro::Rinvia);
        assert_eq!(
            rinviato,
            Contenuto::default(),
            "niente dal futuro: {rinviato:?}"
        );

        // Lo stesso contenuto, il giorno dopo: entra tutto, con le sue date.
        let arrivato = nel_dominio(con_tutto_a(DOMANI), DOMANI, Futuro::Rinvia);
        assert_eq!(arrivato, con_tutto_a(DOMANI));

        // E un orologio di qualche minuto avanti non aspetta.
        let avanti = ADESSO + MARGINE_FUTURO_MS;
        assert_eq!(
            nel_dominio(con_tutto_a(avanti), ADESSO, Futuro::Rinvia),
            con_tutto_a(avanti)
        );
    }

    #[test]
    fn il_profilo_taglia_il_futuro_ad_adesso() {
        let tagliato = nel_dominio(con_tutto_a(DOMANI), ADESSO, Futuro::Taglia);
        assert_eq!(tagliato, con_tutto_a(ADESSO));
        // E il passato resta com'è.
        assert_eq!(
            nel_dominio(con_tutto_a(42), ADESSO, Futuro::Taglia),
            con_tutto_a(42)
        );
    }

    #[test]
    fn il_profilo_taglia_anche_le_date_dei_brani_tolti() {
        let mut contenuto = Contenuto::default();
        let mut sequenza = crate::Sequenza::dalla_lista("uno", &["a".to_owned(), "b".to_owned()]);
        sequenza.togli(0, DOMANI);
        contenuto.playlist.insert(
            "q".to_owned(),
            crate::PlaylistSincronizzata {
                at: 1,
                sequenza,
                ..crate::PlaylistSincronizzata::default()
            },
        );
        let tagliato = nel_dominio(contenuto, ADESSO, Futuro::Taglia);
        let tolti: Vec<Option<i64>> = tagliato
            .playlist
            .get("q")
            .map(|p| p.sequenza.elementi().map(|(_, e)| e.tolto).collect())
            .unwrap_or_default();
        assert_eq!(tolti, vec![Some(ADESSO), None]);
    }

    #[test]
    fn la_sincronia_rinvia_la_testata_di_una_playlist_ma_non_i_suoi_brani() {
        // R02-3: una playlist con la data nel futuro — un orologio sballato, o
        // uno stato già sporco di prima — si rinviava intera, sequenza
        // compresa. Il documento di chi l'ha scritta non passa dal dominio, e
        // finché quella data restava avanti nessun altro dispositivo vedeva
        // più un brano aggiunto o tolto in quella playlist.
        let fra_dieci_anni = ADESSO + 10 * 365 * 24 * 60 * 60 * 1000;
        let mut contenuto = Contenuto::default();
        contenuto.playlist.insert(
            "q".to_owned(),
            crate::PlaylistSincronizzata {
                nome: "Q".to_owned(),
                at: fra_dieci_anni,
                creata_at: 5,
                sequenza: crate::Sequenza::dalla_lista("uno", &["a".to_owned(), "b".to_owned()]),
                ..crate::PlaylistSincronizzata::default()
            },
        );
        let rinviato = nel_dominio(contenuto, ADESSO, Futuro::Rinvia);
        let q = rinviato.playlist.get("q").expect("i brani entrano");
        assert_eq!(q.sequenza.ordine(), vec!["a", "b"]);
        assert_eq!(q.at, 0, "la testata perde contro qualunque testata vera");

        // Quando l'orologio ci arriva, entra anche la testata, con la sua data.
        let mut contenuto = Contenuto::default();
        contenuto.playlist.insert(
            "q".to_owned(),
            crate::PlaylistSincronizzata {
                at: fra_dieci_anni,
                sequenza: crate::Sequenza::dalla_lista("uno", &["a".to_owned()]),
                ..crate::PlaylistSincronizzata::default()
            },
        );
        let arrivato = nel_dominio(contenuto, fra_dieci_anni, Futuro::Rinvia);
        assert_eq!(
            arrivato.playlist.get("q").map(|q| q.at),
            Some(fra_dieci_anni)
        );
    }
}
