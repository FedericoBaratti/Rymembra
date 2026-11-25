//! Dove sta la finestra, e dove rimetterla.
//!
//! # Il difetto che questo modulo toglie
//!
//! Con il vassoio acceso la X **distrugge** la finestra (`comparsa`), e «Mostra»,
//! un clic sull'icona, un secondo avvio o un link la ricreano dalla
//! configurazione: `tauri.conf.json` non ha una posizione e dice 1280×820, quindi
//! la finestra rinasceva sempre di quella misura, non ingrandita, con la
//! posizione a cascata di Windows (collaudo in Windows Sandbox, D1). Il vecchio
//! `hide()` teneva la geometria, perché la finestra restava lì; e un riavvio
//! normale partiva a cascata come adesso, perché non c'è mai stato niente che la
//! ricordasse.
//!
//! # Cosa si ricorda
//!
//! La posizione, la misura e se era ingrandita ([`Geometria`]). **Il rettangolo è
//! quello esterno** (`outer_position` e `outer_size`), e **quello normale**, non
//! quello ingrandito: una finestra ingrandita occupa
//! l'area di lavoro, e ricordare quello vorrebbe dire che «ripristina» la lascia
//! grande quanto lo schermo. Per questo si legge dopo ogni spostamento
//! ([`aggiorna`]) e si tiene in memoria finché la finestra c'è: quando si chiude
//! non c'è più niente da leggere, e quel che serve è proprio l'ultimo rettangolo
//! normale.
//!
//! Non si ricorda mentre la finestra è ridotta a icona (Windows le dà la
//! posizione −32000, −32000) né a schermo intero: in quei momenti il rettangolo
//! non dice niente su dove l'ha messa chi la usa.
//!
//! # Dove rimetterla
//!
//! Un monitor può non esserci più — il portatile staccato dallo schermo
//! esterno — e una finestra rimessa dove non c'è nessuno schermo è una finestra
//! persa. [`rimetti_su_uno_schermo`] la riporta in un'area che esiste: la stessa
//! se c'è ancora, la più coperta fra quelle di adesso, o l'area principale se
//! non ne tocca nessuna. Tutto in **pixel fisici** e in `i64`: una riga
//! scritta a mano nelle impostazioni, o corrotta, non deve poter far traboccare
//! una somma.
//!
//! Questo modulo non conosce Tauri: la lettura e la scrittura della finestra
//! sono in `comparsa`, e le prove di qui girano senza finestre né monitor.

use serde::{Deserialize, Serialize};

/// Un rettangolo in pixel fisici: l'area di lavoro di un monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    /// Il bordo sinistro. Può essere negativo: un monitor a sinistra del principale.
    pub x: i32,
    /// Il bordo alto.
    pub y: i32,
    /// La larghezza.
    pub larghezza: u32,
    /// L'altezza.
    pub altezza: u32,
}

/// La posizione e la misura della finestra, come si ricordano nelle impostazioni.
///
/// **Il rettangolo è quello esterno**, lo stesso che dicono `outer_position` e
/// `outer_size` (`GetWindowRect`): posizione e misura vengono dalla stessa
/// cornice, e si rimettono insieme. Fino al collaudo in Windows Sandbox (D1) la
/// misura era quella *interna*, e rimessa su una finestra appena nata cresceva di
/// trenta pixel a ogni ricreazione: vedi [`rimetti_la_misura`]. Le righe scritte
/// da una build di sviluppo di prima si leggono come esterne, e la finestra
/// torna più piccola di 16×9 pixel una volta sola, finché non si richiude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Geometria {
    /// Il bordo sinistro esterno, in pixel fisici.
    pub x: i32,
    /// Il bordo alto esterno.
    pub y: i32,
    /// La larghezza **esterna** (`outer_size`).
    pub larghezza: u32,
    /// L'altezza **esterna**.
    pub altezza: u32,
    /// Era ingrandita. Il rettangolo sopra è quello di quando non lo era.
    pub ingrandita: bool,
}

/// La finestra com'è adesso, letta dal sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lettura {
    /// Il bordo sinistro esterno.
    pub x: i32,
    /// Il bordo alto esterno.
    pub y: i32,
    /// La larghezza esterna.
    pub larghezza: u32,
    /// L'altezza esterna.
    pub altezza: u32,
    /// È ingrandita.
    pub ingrandita: bool,
    /// È ridotta a icona.
    pub ridotta: bool,
    /// È a schermo intero.
    pub schermo_intero: bool,
}

/// Una misura in pixel fisici.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Misura {
    /// La larghezza.
    pub larghezza: u32,
    /// L'altezza.
    pub altezza: u32,
}

impl Misura {
    /// Una misura.
    #[must_use]
    pub const fn nuova(larghezza: u32, altezza: u32) -> Self {
        Self { larghezza, altezza }
    }
}

/// La posizione che Windows dà a una finestra ridotta a icona.
const POSIZIONE_DELLA_RIDUZIONE: i32 = -32_000;

/// Sotto questa misura una geometria ricordata non è una finestra: è una riga
/// scritta male, e si butta invece di aprire una finestra di tre pixel.
const LARGHEZZA_MINIMA: i64 = 320;
/// Vedi [`LARGHEZZA_MINIMA`].
const ALTEZZA_MINIMA: i64 = 240;

/// Quanto la finestra può restare fuori dall'area, per lato.
///
/// Windows dà alle finestre un bordo invisibile di sette o otto pixel — una
/// finestra agganciata a sinistra ha `x = −8` — e rimetterla dentro di forza
/// vorrebbe dire staccarla dal bordo a cui l'utente l'aveva appoggiata. Oltre
/// questo margine la finestra è fuori davvero, e si rimette dentro.
const MARGINE: i64 = 16;

/// Cosa tenere in memoria dopo aver riletto la finestra.
///
/// `ricordata` è quel che si sapeva; si restituisce quel che si sa adesso.
///
/// - **A schermo intero o ridotta a icona** non si cambia niente: il
///   rettangolo non dice dove sta la finestra normale, e `tao` azzera «ingrandita»
///   mentre è ridotta.
/// - **Una misura nulla o la posizione della riduzione** — una lettura fatta
///   a metà di un cambio di stato — non si ricordano.
/// - **Ingrandita**: si tiene il rettangolo di prima e si alza il segno. Se non
///   c'è un rettangolo di prima — la finestra è nata ingrandita — si prende quello
///   di adesso: è l'unica cosa che si sa, e ripristinandola rimane grande.
/// - **Normale**: il rettangolo di adesso.
#[must_use]
pub fn aggiorna(ricordata: Option<Geometria>, adesso: &Lettura) -> Option<Geometria> {
    if adesso.schermo_intero || adesso.ridotta {
        return ricordata;
    }
    let nulla = adesso.larghezza == 0 || adesso.altezza == 0;
    let della_riduzione =
        adesso.x == POSIZIONE_DELLA_RIDUZIONE && adesso.y == POSIZIONE_DELLA_RIDUZIONE;
    if nulla || della_riduzione {
        return ricordata;
    }
    if adesso.ingrandita
        && let Some(prima) = ricordata
    {
        return Some(Geometria {
            ingrandita: true,
            ..prima
        });
    }
    Some(Geometria {
        x: adesso.x,
        y: adesso.y,
        larghezza: adesso.larghezza,
        altezza: adesso.altezza,
        ingrandita: adesso.ingrandita,
    })
}

// ── la misura, fra finestra e impostazioni ─────────────────────────────────

/// Quel che serve a rimettere la misura di una finestra. La finestra vera
/// (`comparsa`) e quella finta delle prove ([`prove`]) lo danno allo stesso modo.
pub trait MisuraDellaFinestra {
    /// La misura esterna adesso (`outer_size`), se il sistema risponde.
    fn esterna(&mut self) -> Option<Misura>;
    /// La misura interna adesso (`inner_size`), se il sistema risponde.
    fn interna(&mut self) -> Option<Misura>;
    /// Chiede una misura **interna**: è l'unica che `tao` sa rimettere.
    ///
    /// # Errori
    ///
    /// Il testo del guasto, se il sistema non l'ha accettata.
    fn chiedi_interna(&mut self, misura: Misura) -> Result<(), String>;
}

/// Che misura si ricorda di una finestra: l'**esterna**, la stessa cornice da cui
/// viene la posizione. Quella interna dipende da com'è la finestra in quel
/// momento — nascosta o visibile — e rimessa su una finestra nata dopo non
/// tornava uguale (D1).
#[must_use]
pub fn misura_da_ricordare<F: MisuraDellaFinestra>(finestra: &mut F) -> Option<Misura> {
    finestra.esterna()
}

/// Quanto la finestra è più grande del suo contenuto, adesso.
fn cornice(esterna: Misura, interna: Misura) -> Misura {
    Misura::nuova(
        esterna.larghezza.saturating_sub(interna.larghezza),
        esterna.altezza.saturating_sub(interna.altezza),
    )
}

/// La misura **interna** da chiedere perché la finestra, con la cornice che ha
/// adesso, risulti grande `voluta` fuori.
///
/// `tao::set_inner_size` somma alla misura chiesta la cornice che legge in quel
/// momento (`GetWindowRect` meno `GetClientRect`) e dà la somma al sistema: se la
/// cornice è quella misurata un istante prima, l'esterna che ne esce è
/// esattamente `voluta`. Una finestra appena costruita e ancora nascosta ha una
/// cornice alta 39 pixel, una visibile 9: chiedere la misura interna ricordata
/// la faceva uscire trenta pixel più alta a ogni ricreazione (L-WSB33, D1).
#[must_use]
pub fn misura_da_chiedere(voluta: Misura, cornice: Misura) -> Misura {
    Misura::nuova(
        voluta.larghezza.saturating_sub(cornice.larghezza).max(1),
        voluta.altezza.saturating_sub(cornice.altezza).max(1),
    )
}

/// Se la finestra ha risposto con un'altra misura esterna, la nuova richiesta:
/// quella di prima spostata di quanto si è sbagliato. `None` se coincide, o se
/// non c'è niente da cambiare.
#[must_use]
pub fn correzione(voluta: Misura, ottenuta: Misura, chiesta: Misura) -> Option<Misura> {
    if ottenuta == voluta {
        return None;
    }
    let sposta = |chiesta: u32, voluta: u32, ottenuta: u32| {
        let nuova = i64::from(chiesta) + i64::from(voluta) - i64::from(ottenuta);
        u32::try_from(nuova.clamp(1, i64::from(u32::MAX))).unwrap_or(chiesta)
    };
    let nuova = Misura::nuova(
        sposta(chiesta.larghezza, voluta.larghezza, ottenuta.larghezza),
        sposta(chiesta.altezza, voluta.altezza, ottenuta.altezza),
    );
    (nuova != chiesta).then_some(nuova)
}

/// Com'è andata la rimessa della misura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EsitoMisura {
    /// La misura interna chiesta la prima volta.
    pub chiesta: Misura,
    /// La misura esterna che la finestra ha alla fine.
    pub ottenuta: Misura,
    /// C'è voluta una seconda richiesta.
    pub corretta: bool,
}

/// Rimette la misura **esterna** `voluta`: misura la cornice della finestra
/// adesso, chiede la misura interna che la fa uscire giusta, rilegge quel che la
/// finestra ha e corregge **una volta** se differisce. Non insegue: se il sistema
/// non concede la misura — una minima sotto cui non si scende — si ferma lì.
///
/// # Errori
///
/// Il testo del guasto, se la finestra non risponde o rifiuta la misura.
pub fn rimetti_la_misura<F: MisuraDellaFinestra>(
    finestra: &mut F,
    voluta: Misura,
) -> Result<EsitoMisura, String> {
    let non_risponde = || "la finestra non risponde".to_owned();
    let esterna = finestra.esterna().ok_or_else(non_risponde)?;
    let interna = finestra.interna().ok_or_else(non_risponde)?;
    let chiesta = misura_da_chiedere(voluta, cornice(esterna, interna));
    finestra.chiedi_interna(chiesta)?;
    let ottenuta = finestra.esterna().ok_or_else(non_risponde)?;
    let Some(ancora) = correzione(voluta, ottenuta, chiesta) else {
        return Ok(EsitoMisura {
            chiesta,
            ottenuta,
            corretta: false,
        });
    };
    finestra.chiedi_interna(ancora)?;
    Ok(EsitoMisura {
        chiesta,
        ottenuta: finestra.esterna().ok_or_else(non_risponde)?,
        corretta: true,
    })
}

/// Quanto un rettangolo copre di un'area, in pixel quadrati.
fn copertura(x: i64, y: i64, larghezza: i64, altezza: i64, area: &Area) -> i64 {
    let (ax, ay) = (i64::from(area.x), i64::from(area.y));
    let (aw, ah) = (i64::from(area.larghezza), i64::from(area.altezza));
    let largo = (x + larghezza).min(ax + aw) - x.max(ax);
    let alto = (y + altezza).min(ay + ah) - y.max(ay);
    if largo <= 0 || alto <= 0 {
        0
    } else {
        largo.saturating_mul(alto)
    }
}

/// Come rimettere la geometria ricordata sulla finestra di adesso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rimessa {
    /// Tutta: posizione, misura e ingrandimento.
    Subito,
    /// La posizione e la misura, **non** l'ingrandimento: la finestra sta per
    /// comparire ridotta a icona (l'avvio automatico di Windows). Ingrandita ora,
    /// ancora nascosta, `tao` alla `show` farebbe `SW_MAXIMIZE`: a chi sta
    /// arrivando al desktop comparirebbe una finestra a schermo intero un istante
    /// prima della riduzione. L'ingrandimento aspetta che qualcuno la ripristini.
    SenzaIngrandire,
    /// Niente adesso: la finestra è ridotta a icona. `SetWindowPos` su una
    /// finestra ridotta non cambia il rettangolo con cui si ripristina, e
    /// `maximize()` la ripristinerebbe sullo schermo. Aspetta che qualcuno la
    /// ripristini.
    Dopo,
}

/// Come rimettere la geometria, dato come sta la finestra.
///
/// `ridotta`: è ridotta a icona adesso. `in_riduzione`: sta per comparire
/// ridotta (la comparsa dell'avvio automatico non è ancora stata consumata).
#[must_use]
pub fn come_rimetterla(ridotta: bool, in_riduzione: bool) -> Rimessa {
    if ridotta {
        Rimessa::Dopo
    } else if in_riduzione {
        Rimessa::SenzaIngrandire
    } else {
        Rimessa::Subito
    }
}

/// È il momento di rimettere quel che aspettava (`Rimessa::SenzaIngrandire` o
/// `Rimessa::Dopo`)? Quando la finestra si vede, non è ridotta a icona e non è
/// a schermo intero: dopo che qualcuno l'ha ripristinata. **Nascosta no**: è
/// com'è prima che l'avvio automatico la mostri ridotta, e ingrandirla lì
/// vorrebbe dire il lampo che si voleva evitare.
#[must_use]
pub fn e_ora_di_rimettere(visibile: bool, ridotta: bool, schermo_intero: bool) -> bool {
    visibile && !ridotta && !schermo_intero
}

/// Mette per prima l'area del monitor principale, lasciando l'ordine degli altri.
///
/// [`rimetti_su_uno_schermo`] centra sulla prima area quando la finestra non
/// tocca nessuno schermo, e la prima deve essere la principale: l'elenco dei
/// monitor che dà il sistema non ha un ordine che lo garantisca. Se la
/// principale non si sa, o non è fra le aree, l'ordine resta com'è.
#[must_use]
pub fn principale_per_prima(mut aree: Vec<Area>, principale: Option<Area>) -> Vec<Area> {
    let posto = principale.and_then(|p| aree.iter().position(|area| *area == p));
    if let Some(posto) = posto
        && let Some(fino_a_lei) = aree.get_mut(..=posto)
    {
        fino_a_lei.rotate_right(1);
    }
    aree
}

/// Riporta una geometria ricordata su uno schermo che esiste adesso.
///
/// `aree` sono le aree di lavoro dei monitor di adesso, **la principale per
/// prima**. Restituisce:
///
/// - `None` se non c'è nessun monitor — non si tocca niente, la finestra resta
///   dov'è — o se la misura ricordata non è quella di una finestra;
/// - la stessa geometria se sta dentro un'area, con la tolleranza del bordo
///   invisibile ([`MARGINE`]);
/// - altrimenti la stessa **ridotta all'area più coperta** — più piccola se
///   non ci sta, riportata dentro se sporge — o **centrata sull'area
///   principale** se non ne tocca nessuna: il suo monitor non c'è più.
///
/// «Ingrandita» passa com'è: la finestra ingrandita riempie l'area su cui
/// ricade, e quella la sceglie il rettangolo qui sotto.
#[expect(
    clippy::integer_division,
    reason = "centrare: se lo spazio che avanza è dispari il pixel in più resta a destra e in basso, e non si vede"
)]
#[must_use]
pub fn rimetti_su_uno_schermo(g: &Geometria, aree: &[Area]) -> Option<Geometria> {
    let (x, y) = (i64::from(g.x), i64::from(g.y));
    let (larghezza, altezza) = (i64::from(g.larghezza), i64::from(g.altezza));
    if larghezza < LARGHEZZA_MINIMA || altezza < ALTEZZA_MINIMA {
        return None;
    }
    // La più coperta; a parità la prima, cioè la principale.
    let mut scelta: Option<(&Area, i64)> = None;
    for area in aree {
        let coperta = copertura(x, y, larghezza, altezza, area);
        if scelta.is_none_or(|(_, meglio)| coperta > meglio) {
            scelta = Some((area, coperta));
        }
    }
    let (area, coperta) = scelta?;
    let (ax, ay) = (i64::from(area.x), i64::from(area.y));
    let (aw, ah) = (i64::from(area.larghezza), i64::from(area.altezza));

    let larghezza = larghezza.min(aw);
    let altezza = altezza.min(ah);
    let (nuovo_x, nuovo_y) = if coperta == 0 {
        // Il suo schermo non c'è più: al centro della principale.
        (ax + (aw - larghezza) / 2, ay + (ah - altezza) / 2)
    } else {
        (
            x.clamp(ax - MARGINE, ax + aw - larghezza + MARGINE),
            y.clamp(ay - MARGINE, ay + ah - altezza + MARGINE),
        )
    };
    Some(Geometria {
        x: i32::try_from(nuovo_x).ok()?,
        y: i32::try_from(nuovo_y).ok()?,
        larghezza: u32::try_from(larghezza).ok()?,
        altezza: u32::try_from(altezza).ok()?,
        ingrandita: g.ingrandita,
    })
}

#[cfg(test)]
mod prove {
    use super::*;

    const PRINCIPALE: Area = Area {
        x: 0,
        y: 0,
        larghezza: 1920,
        altezza: 1040,
    };

    /// Un secondo monitor a sinistra del principale: coordinate negative.
    const A_SINISTRA: Area = Area {
        x: -1280,
        y: 0,
        larghezza: 1280,
        altezza: 1024,
    };

    fn finestra(x: i32, y: i32, larghezza: u32, altezza: u32) -> Geometria {
        Geometria {
            x,
            y,
            larghezza,
            altezza,
            ingrandita: false,
        }
    }

    fn lettura(x: i32, y: i32) -> Lettura {
        Lettura {
            x,
            y,
            larghezza: 1296,
            altezza: 829,
            ingrandita: false,
            ridotta: false,
            schermo_intero: false,
        }
    }

    // ── dove rimetterla ─────────────────────────────────────────────────────

    #[test]
    fn una_finestra_dentro_lo_schermo_resta_dov_e() {
        let g = finestra(200, 120, 1296, 829);
        assert_eq!(rimetti_su_uno_schermo(&g, &[PRINCIPALE]), Some(g));
    }

    #[test]
    fn ingrandita_resta_ingrandita() {
        let g = Geometria {
            ingrandita: true,
            ..finestra(200, 120, 1296, 829)
        };
        assert_eq!(rimetti_su_uno_schermo(&g, &[PRINCIPALE]), Some(g));
    }

    #[test]
    fn il_bordo_invisibile_di_windows_non_la_stacca_dal_bordo() {
        // Una finestra agganciata a sinistra ha x = −8, e in basso il bordo
        // invisibile la fa sporgere di qualche pixel: non va spostata.
        let a_sinistra = finestra(-8, 0, 960, 1040);
        assert_eq!(
            rimetti_su_uno_schermo(&a_sinistra, &[PRINCIPALE]),
            Some(a_sinistra)
        );
        let in_basso = finestra(0, 1040 - 829 + 8, 1296, 829);
        assert_eq!(
            rimetti_su_uno_schermo(&in_basso, &[PRINCIPALE]),
            Some(in_basso)
        );
    }

    #[test]
    fn una_finestra_piu_grande_dello_schermo_si_riduce() {
        let g = finestra(0, 0, 3000, 2000);
        assert_eq!(
            rimetti_su_uno_schermo(&g, &[PRINCIPALE]),
            Some(finestra(0, 0, 1920, 1040))
        );
    }

    #[test]
    fn una_finestra_che_sporge_rientra() {
        // Sporge di 800 pixel a destra e di 300 in basso: si riporta dentro
        // fino al margine del bordo invisibile.
        let g = finestra(1400, 700, 1296, 829);
        let messa = rimetti_su_uno_schermo(&g, &[PRINCIPALE]).expect("c'è un monitor");
        assert_eq!(messa.larghezza, 1296);
        assert_eq!(messa.altezza, 829);
        assert_eq!(messa.x, 1920 - 1296 + 16);
        assert_eq!(messa.y, 1040 - 829 + 16);
        // E la barra del titolo, in alto, non resta mai oltre il margine.
        let sopra = finestra(300, -500, 1296, 829);
        let messa = rimetti_su_uno_schermo(&sopra, &[PRINCIPALE]).expect("c'è un monitor");
        assert_eq!(messa.y, -16);
    }

    #[test]
    fn un_monitor_sparito_porta_la_finestra_al_centro_del_principale() {
        // Stava su uno schermo a destra (x da 1920) che non c'è più.
        let g = finestra(2500, 100, 1296, 829);
        let messa = rimetti_su_uno_schermo(&g, &[PRINCIPALE]).expect("c'è un monitor");
        // 1920 − 1296 = 624, metà 312; 1040 − 829 = 211, metà 105 (il pixel in
        // più resta in basso).
        assert_eq!(messa.x, 312);
        assert_eq!(messa.y, 105);
        assert_eq!((messa.larghezza, messa.altezza), (1296, 829));
        // Anche se lo schermo che resta è più piccolo: si riduce e si centra.
        let piccolo = Area {
            x: 0,
            y: 0,
            larghezza: 1024,
            altezza: 700,
        };
        let messa = rimetti_su_uno_schermo(&g, &[piccolo]).expect("c'è un monitor");
        assert_eq!((messa.larghezza, messa.altezza), (1024, 700));
        assert_eq!((messa.x, messa.y), (0, 0));
    }

    #[test]
    fn con_due_monitor_vince_quello_che_la_copre_di_piu() {
        // Tutta dentro il monitor a sinistra (coordinate negative): resta com'è.
        let sul_secondo = finestra(-1200, 50, 1000, 700);
        assert_eq!(
            rimetti_su_uno_schermo(&sul_secondo, &[PRINCIPALE, A_SINISTRA]),
            Some(sul_secondo)
        );
        // 1000 pixel sul monitor a sinistra e 296 sul principale: vince il
        // monitor a sinistra, e la finestra — più larga di lui — si riduce a
        // quello che ci sta.
        let g = finestra(-1000, 100, 1296, 829);
        assert_eq!(
            rimetti_su_uno_schermo(&g, &[PRINCIPALE, A_SINISTRA]),
            Some(finestra(-1264, 100, 1280, 829))
        );
        // Con quel monitor staccato, la stessa riga si riporta sul principale.
        let sola = rimetti_su_uno_schermo(&g, &[PRINCIPALE]).expect("monitor");
        assert_eq!((sola.x, sola.larghezza), (-16, 1296));
    }

    #[test]
    fn a_parita_di_copertura_vince_il_principale() {
        // 648 pixel su ciascuno dei due: una finestra a cavallo si riporta su un
        // monitor solo, e a parità è il principale, che è il primo.
        let a_meta = finestra(-648, 100, 1296, 829);
        let messa = rimetti_su_uno_schermo(&a_meta, &[PRINCIPALE, A_SINISTRA]).expect("monitor");
        assert_eq!((messa.x, messa.larghezza), (-16, 1296));
    }

    #[test]
    fn senza_monitor_non_si_tocca_niente() {
        let g = finestra(200, 120, 1296, 829);
        assert_eq!(rimetti_su_uno_schermo(&g, &[]), None);
    }

    #[test]
    fn una_misura_che_non_e_di_una_finestra_si_butta() {
        for (larghezza, altezza) in [
            (0, 0),
            (1296, 0),
            (0, 829),
            (10, 10),
            (319, 829),
            (1296, 239),
        ] {
            let g = finestra(100, 100, larghezza, altezza);
            assert_eq!(
                rimetti_su_uno_schermo(&g, &[PRINCIPALE]),
                None,
                "{larghezza}×{altezza}"
            );
        }
    }

    #[test]
    fn i_valori_estremi_non_fanno_traboccare_niente() {
        // Una riga corrotta o scritta a mano: nessun panico, e il risultato sta
        // comunque nell'area.
        for g in [
            finestra(i32::MAX, i32::MAX, u32::MAX, u32::MAX),
            finestra(i32::MIN, i32::MIN, u32::MAX, u32::MAX),
            finestra(i32::MAX, i32::MIN, 1296, 829),
            finestra(i32::MIN, i32::MAX, 1296, 829),
        ] {
            let messa = rimetti_su_uno_schermo(&g, &[PRINCIPALE]).expect("c'è un monitor");
            assert!(
                messa.larghezza <= PRINCIPALE.larghezza,
                "{g:?} -> {messa:?}"
            );
            assert!(messa.altezza <= PRINCIPALE.altezza, "{g:?} -> {messa:?}");
            assert!(messa.x >= -16 && messa.y >= -16, "{g:?} -> {messa:?}");
            let destra = i64::from(messa.x) + i64::from(messa.larghezza);
            let sotto = i64::from(messa.y) + i64::from(messa.altezza);
            assert!(
                destra <= 1920 + 16 && sotto <= 1040 + 16,
                "{g:?} -> {messa:?}"
            );
        }
        // Un'area ai limiti del tipo: o una risposta dentro i tipi, o `None`.
        let enorme = Area {
            x: i32::MAX - 100,
            y: i32::MAX - 100,
            larghezza: u32::MAX,
            altezza: u32::MAX,
        };
        let _ = rimetti_su_uno_schermo(&finestra(0, 0, 1296, 829), &[enorme]);
    }

    #[test]
    fn la_principale_va_per_prima_e_gli_altri_restano_in_ordine() {
        let terzo = Area {
            x: 1920,
            y: 0,
            larghezza: 1600,
            altezza: 900,
        };
        let elenco = vec![A_SINISTRA, terzo, PRINCIPALE];
        assert_eq!(
            principale_per_prima(elenco.clone(), Some(PRINCIPALE)),
            vec![PRINCIPALE, A_SINISTRA, terzo]
        );
        // Già per prima, o non si sa quale sia, o non è fra le aree: com'è.
        assert_eq!(
            principale_per_prima(vec![PRINCIPALE, A_SINISTRA], Some(PRINCIPALE)),
            vec![PRINCIPALE, A_SINISTRA]
        );
        assert_eq!(principale_per_prima(elenco.clone(), None), elenco);
        assert_eq!(
            principale_per_prima(elenco.clone(), Some(Area { x: 5, ..PRINCIPALE })),
            elenco
        );
        assert_eq!(
            principale_per_prima(Vec::new(), Some(PRINCIPALE)),
            Vec::new()
        );
    }

    // ── quando rimetterla ───────────────────────────────────────────────────

    #[test]
    fn una_finestra_normale_si_rimette_tutta_subito() {
        assert_eq!(come_rimetterla(false, false), Rimessa::Subito);
    }

    /// L'avvio automatico: la finestra sta per comparire ridotta, e ingrandirla
    /// prima farebbe un lampo a schermo intero (`tao` alla `show` fa
    /// `SW_MAXIMIZE` se il segno c'è).
    #[test]
    fn se_sta_per_comparire_ridotta_non_si_ingrandisce_ancora() {
        assert_eq!(come_rimetterla(false, true), Rimessa::SenzaIngrandire);
    }

    /// Una finestra già ridotta (l'apertura lenta: la rete di sicurezza l'ha
    /// mostrata ridotta prima che la libreria fosse aperta) non si tocca: né la
    /// posizione, che non cambierebbe il rettangolo con cui si ripristina, né
    /// l'ingrandimento, che la ripristinerebbe sullo schermo.
    #[test]
    fn una_finestra_gia_ridotta_aspetta_che_la_si_ripristini() {
        assert_eq!(come_rimetterla(true, false), Rimessa::Dopo);
        assert_eq!(come_rimetterla(true, true), Rimessa::Dopo);
    }

    #[test]
    fn quel_che_aspettava_si_rimette_solo_a_finestra_visibile_normale() {
        // Visibile, né ridotta né a schermo intero: ripristinata da qualcuno.
        assert!(e_ora_di_rimettere(true, false, false));
        // Nascosta (prima che l'avvio automatico la mostri ridotta), ridotta o a
        // schermo intero: ancora no.
        assert!(!e_ora_di_rimettere(false, false, false));
        assert!(!e_ora_di_rimettere(true, true, false));
        assert!(!e_ora_di_rimettere(true, false, true));
        assert!(!e_ora_di_rimettere(false, true, true));
    }

    // ── cosa ricordare ──────────────────────────────────────────────────────

    #[test]
    fn una_finestra_normale_si_ricorda_com_e() {
        let adesso = lettura(300, 200);
        assert_eq!(
            aggiorna(None, &adesso),
            Some(Geometria {
                x: 300,
                y: 200,
                larghezza: 1296,
                altezza: 829,
                ingrandita: false,
            })
        );
        // E lo spostamento seguente sostituisce il rettangolo di prima.
        let prima = aggiorna(None, &adesso);
        let dopo = aggiorna(prima, &lettura(10, 20));
        assert_eq!(dopo.map(|g| (g.x, g.y)), Some((10, 20)));
    }

    #[test]
    fn ingrandita_si_ricorda_il_rettangolo_di_prima_e_il_segno() {
        let normale = aggiorna(None, &lettura(300, 200));
        // Ingrandita, la finestra occupa l'area di lavoro: quel rettangolo non
        // è quello da rimettere quando la si ripristina.
        let ingrandita = Lettura {
            x: -8,
            y: -8,
            larghezza: 1936,
            altezza: 1056,
            ingrandita: true,
            ..lettura(0, 0)
        };
        let dopo = aggiorna(normale, &ingrandita).expect("una geometria");
        assert!(dopo.ingrandita);
        assert_eq!(
            (dopo.x, dopo.y, dopo.larghezza, dopo.altezza),
            (300, 200, 1296, 829)
        );
        // Ripristinata, il segno si abbassa e il rettangolo è quello nuovo.
        let ripristinata = aggiorna(Some(dopo), &lettura(310, 210)).expect("una geometria");
        assert!(!ripristinata.ingrandita);
        assert_eq!((ripristinata.x, ripristinata.y), (310, 210));
    }

    #[test]
    fn nata_ingrandita_si_ricorda_quel_che_c_e() {
        // Non c'è un rettangolo di prima: è l'unica cosa che si sa.
        let ingrandita = Lettura {
            ingrandita: true,
            ..lettura(-8, -8)
        };
        let dopo = aggiorna(None, &ingrandita).expect("una geometria");
        assert!(dopo.ingrandita);
        assert_eq!((dopo.x, dopo.y), (-8, -8));
    }

    #[test]
    fn ridotta_o_a_schermo_intero_non_cambia_niente() {
        let prima = aggiorna(None, &lettura(300, 200));
        // Windows dà −32000 alla finestra ridotta, e `tao` azzera «ingrandita».
        let ridotta = Lettura {
            ridotta: true,
            ..lettura(POSIZIONE_DELLA_RIDUZIONE, POSIZIONE_DELLA_RIDUZIONE)
        };
        assert_eq!(aggiorna(prima, &ridotta), prima);
        assert_eq!(aggiorna(None, &ridotta), None);
        let intera = Lettura {
            schermo_intero: true,
            ..lettura(0, 0)
        };
        assert_eq!(aggiorna(prima, &intera), prima);
    }

    #[test]
    fn una_lettura_a_meta_di_un_cambio_di_stato_non_si_ricorda() {
        let prima = aggiorna(None, &lettura(300, 200));
        // La posizione della riduzione senza il segno di «ridotta» (la lettura
        // arriva prima dell'aggiornamento di `tao`), e una misura nulla.
        let a_meta = lettura(POSIZIONE_DELLA_RIDUZIONE, POSIZIONE_DELLA_RIDUZIONE);
        assert_eq!(aggiorna(prima, &a_meta), prima);
        let nulla = Lettura {
            larghezza: 0,
            ..lettura(300, 200)
        };
        assert_eq!(aggiorna(prima, &nulla), prima);
    }

    // ── la misura: il giro salva-rimetti è un punto fisso (D1) ──────────────
    //
    // Collaudo in Windows Sandbox (L-WSB33, D1), finestra senza decorazioni con
    // l'ombra di `tao`:
    //
    // - una finestra **visibile** ha la cornice invisibile (8, 1, 8, 8): la misura
    //   esterna è quella interna più 16×9 (1296×829 con 1280×820 dentro);
    // - una finestra **appena nata e ancora nascosta** ha, per `tao`, una cornice
    //   alta 39 pixel (31 di barra del titolo e 8 di bordo): `set_inner_size` ci
    //   aggiunge quella, e la finestra esce più alta di trenta pixel quando si
    //   mostra e la cornice torna a 9.
    //
    // Rimettere la misura *interna* ricordata su una finestra nata, quindi,
    // faceva 829 → 859 → 889 → 910 a ogni riavvio (910 = 871 di area di lavoro più
    // 39). La finestra finta qui sotto è fatta di quei due numeri, e basta.

    /// L'area di lavoro della Sandbox: schermo 1520×919, barra da 48 pixel.
    const SANDBOX: Area = Area {
        x: 0,
        y: 0,
        larghezza: 1520,
        altezza: 871,
    };
    const CORNICE_NASCOSTA: Misura = Misura::nuova(16, 39);
    const CORNICE_VISIBILE: Misura = Misura::nuova(16, 9);

    /// Una finestra come l'hanno misurata le prove dal vivo.
    struct FinestraFinta {
        esterna: Misura,
        cornice: Misura,
        /// Quel che il sistema aggiunge di suo alla misura chiesta (un'altra
        /// ipotesi su come `tao` ricompone la cornice: la correzione deve reggere
        /// anche a questa).
        in_piu: Misura,
        /// La misura esterna sotto cui il sistema non scende.
        minima: Misura,
        richieste: u32,
    }

    impl FinestraFinta {
        /// Nata dalla configurazione (1280×820 interni), ancora nascosta.
        fn nata() -> Self {
            Self {
                esterna: Misura::nuova(1296, 829),
                cornice: CORNICE_NASCOSTA,
                in_piu: Misura::nuova(0, 0),
                minima: Misura::nuova(0, 0),
                richieste: 0,
            }
        }

        fn mostra(&mut self) {
            self.cornice = CORNICE_VISIBILE;
        }
    }

    impl MisuraDellaFinestra for FinestraFinta {
        fn esterna(&mut self) -> Option<Misura> {
            Some(self.esterna)
        }

        fn interna(&mut self) -> Option<Misura> {
            Some(Misura::nuova(
                self.esterna.larghezza - self.cornice.larghezza,
                self.esterna.altezza - self.cornice.altezza,
            ))
        }

        fn chiedi_interna(&mut self, misura: Misura) -> Result<(), String> {
            self.richieste += 1;
            // Come `tao::set_inner_size` con l'ombra: ci somma la cornice che legge
            // adesso, e poi chiede quella misura esterna al sistema.
            self.esterna = Misura::nuova(
                (misura.larghezza + self.cornice.larghezza + self.in_piu.larghezza)
                    .max(self.minima.larghezza),
                (misura.altezza + self.cornice.altezza + self.in_piu.altezza)
                    .max(self.minima.altezza),
            );
            Ok(())
        }
    }

    /// Una ricreazione intera: la X (si ricorda quel che si legge), le
    /// impostazioni (scritte e rilette come JSON), la finestra nuova (la posizione
    /// e la misura rimesse come fa `comparsa::applica`) e «Mostra».
    fn ricrea(prima: &mut FinestraFinta, x: i32, y: i32) -> (FinestraFinta, Geometria) {
        let misura = misura_da_ricordare(prima).expect("la finestra risponde");
        let letta = Lettura {
            larghezza: misura.larghezza,
            altezza: misura.altezza,
            ..lettura(x, y)
        };
        let ricordata = aggiorna(None, &letta).expect("una finestra normale si ricorda");
        let scritta = serde_json::to_string(&ricordata).expect("si scrive");
        let riletta: Geometria = serde_json::from_str(&scritta).expect("si rilegge");
        let messa = rimetti_su_uno_schermo(&riletta, &[SANDBOX]).expect("c'è un monitor");
        let mut nuova = FinestraFinta::nata();
        rimetti_la_misura(&mut nuova, Misura::nuova(messa.larghezza, messa.altezza))
            .expect("la finestra risponde");
        nuova.mostra();
        (nuova, messa)
    }

    /// Una finestra visibile come quella della Sandbox: 1296×829 esterni.
    fn finestra_visibile() -> FinestraFinta {
        let mut f = FinestraFinta::nata();
        f.mostra();
        f
    }

    #[test]
    fn la_cornice_di_una_finestra_visibile_e_di_16_per_9() {
        // I numeri del collaudo: 1296×829 esterni, 1280×820 dentro.
        let mut f = finestra_visibile();
        assert_eq!(f.interna(), Some(Misura::nuova(1280, 820)));
    }

    #[test]
    fn il_giro_salva_rimetti_e_un_punto_fisso() {
        // Dentro l'area di lavoro, così la guardia dello schermo non c'entra.
        let (x, y) = (156, 20);
        let mut finestra = finestra_visibile();
        let partenza = finestra.esterna;
        for giro in 1..=6 {
            let (nuova, messa) = ricrea(&mut finestra, x, y);
            assert_eq!(
                nuova.esterna, partenza,
                "al giro {giro} la finestra è cambiata di misura"
            );
            assert_eq!((messa.x, messa.y), (x, y), "e di posto, al giro {giro}");
            finestra = nuova;
        }
    }

    #[test]
    fn un_riavvio_dopo_l_altro_la_finestra_non_cresce() {
        // 829 → 859 → 889 → 910: il collaudo dal vivo, riavvio per riavvio.
        let mut finestra = finestra_visibile();
        let altezze: Vec<u32> = (0..5)
            .map(|_| {
                let (nuova, _) = ricrea(&mut finestra, 100, 4);
                let altezza = nuova.esterna.altezza;
                finestra = nuova;
                altezza
            })
            .collect();
        assert_eq!(altezze, vec![829; 5]);
    }

    #[test]
    fn una_finestra_sotto_la_barra_sale_la_prima_volta_e_poi_resta() {
        // La finestra del collaudo stava a y = 90 con 829 di altezza: il bordo
        // basso a 919, 48 pixel sotto l'area di lavoro (871). La guardia dello
        // schermo la riporta dentro — 871 − 829 + 16 = 58 — ed è giusto; quel che
        // non deve succedere è che salga ancora, o che cambi misura.
        let mut finestra = finestra_visibile();
        let (prima, messa) = ricrea(&mut finestra, 156, 90);
        assert_eq!((messa.y, prima.esterna), (58, Misura::nuova(1296, 829)));
        let mut finestra = prima;
        for _ in 0..4 {
            let (nuova, messa) = ricrea(&mut finestra, 156, 58);
            assert_eq!((messa.x, messa.y), (156, 58));
            assert_eq!(nuova.esterna, Misura::nuova(1296, 829));
            finestra = nuova;
        }
    }

    #[test]
    fn una_finestra_che_risponde_con_un_altra_misura_si_corregge_una_volta() {
        // Un'altra ipotesi su `tao`: il sistema aggiunge trenta pixel di suo.
        let mut f = FinestraFinta {
            in_piu: Misura::nuova(0, 30),
            ..FinestraFinta::nata()
        };
        let voluta = Misura::nuova(1296, 829);
        let esito = rimetti_la_misura(&mut f, voluta).expect("la finestra risponde");
        assert_eq!(esito.ottenuta, voluta);
        assert!(esito.corretta);
        assert_eq!(f.richieste, 2, "una richiesta e una sola correzione");
    }

    #[test]
    fn una_misura_che_il_sistema_non_concede_non_si_insegue() {
        let mut f = FinestraFinta {
            minima: Misura::nuova(1000, 700),
            ..FinestraFinta::nata()
        };
        let esito = rimetti_la_misura(&mut f, Misura::nuova(900, 600)).expect("risponde");
        assert_eq!(esito.ottenuta, Misura::nuova(1000, 700));
        assert!(
            f.richieste <= 2,
            "si è insistito: {} richieste",
            f.richieste
        );
    }

    // ── come sta nelle impostazioni ─────────────────────────────────────────

    #[test]
    fn la_geometria_si_scrive_e_si_rilegge_uguale() {
        let g = Geometria {
            ingrandita: true,
            ..finestra(-1280, 40, 1296, 829)
        };
        let testo = serde_json::to_string(&g).expect("si scrive");
        assert_eq!(
            testo,
            r#"{"x":-1280,"y":40,"larghezza":1296,"altezza":829,"ingrandita":true}"#
        );
        assert_eq!(serde_json::from_str::<Geometria>(&testo).ok(), Some(g));
    }

    #[test]
    fn una_riga_storta_non_e_una_geometria() {
        for storta in [
            "",
            "{}",
            "null",
            r#"{"x":"a","y":0,"larghezza":1,"altezza":1,"ingrandita":false}"#,
            r#"{"x":0,"y":0,"larghezza":-5,"altezza":829,"ingrandita":false}"#,
            r#"{"x":99999999999,"y":0,"larghezza":1296,"altezza":829,"ingrandita":false}"#,
        ] {
            assert!(
                serde_json::from_str::<Geometria>(storta).is_err(),
                "letta: {storta:?}"
            );
        }
    }
}
