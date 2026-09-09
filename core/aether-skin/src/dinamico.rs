//! L'accento che segue la copertina, senza rompere il contrasto.
//!
//! # Il problema, per esteso
//!
//! `--accent` non è una decorazione: è **anche** un colore di testo. Compare
//! come `color:` sul nome del brano in riproduzione, sull'icona di una scheda
//! vuota, sul contorno del fuoco; e compare come `background:` sotto il testo
//! di `.bottone.primario`, che sopra ci scrive `var(--color-surface-0)`. Una
//! tinta presa da una copertina non ha nessun motivo di rispettare né l'una né
//! l'altra: il giallo di un disco reggae contro una superficie chiara sta a
//! 1,4:1, e il testo sparisce.
//!
//! Il vecchio albero lo faceva lo stesso — `useAccentColor.ts` scriveva
//! `--accent` con l'esadecimale della tavolozza e via — e il risultato era che
//! su certi dischi metà interfaccia diventava illeggibile. È la ragione per cui
//! qui il meccanismo è rimasto spento finché non c'è stato un posto in cui
//! metterlo che *non* fosse «scrivi il colore e spera».
//!
//! # La forma della soluzione
//!
//! La tonalità della copertina si tiene, la chiarezza si sposta. In OKLCH e non
//! in HSL, perché in HSL «stessa L, tonalità diversa» significa luminosità
//! percepite molto diverse — un giallo e un blu a `hsl(_, 50%, 50%)` non si
//! somigliano affatto — e finiremmo a cercare a tentoni quel che in OKLCH è una
//! coordinata sola.
//!
//! Si cerca la chiarezza **più vicina** a quella originale che soddisfa tutti i
//! vincoli insieme, e si cerca in entrambe le direzioni: su un tema scuro il
//! risultato sarà quasi sempre più chiaro, su uno chiaro più scuro, ma non è
//! questa funzione a doverlo sapere. Lo decide il rapporto di contrasto.
//!
//! Poi il croma si riduce quanto basta a rientrare in sRGB, perché la stessa
//! tonalità a una chiarezza diversa può non esistere sullo schermo. Tagliare i
//! canali invece che il croma cambierebbe la tonalità, cioè proprio la cosa che
//! stiamo cercando di conservare.
//!
//! # Perché può dire di no
//!
//! Perché a volte è la risposta giusta. Con quattro superfici molto distanti fra
//! loro non esiste nessuna chiarezza che vada bene per tutte, e allora vince
//! l'accento che la skin ha scritto — che è stato controllato da chi l'ha
//! scritta. `None` non è un guasto: è «questa copertina non ha un accento
//! sicuro», e il posto in cui si decide è uno solo, qui, in Rust, accanto a
//! [`contrast_ratio`](crate::values::contrast_ratio).

use crate::values::{Rgba, contrast_ratio};

/// Oltre questo croma un accento smette di essere un accento.
///
/// La copertina di un disco può essere fluorescente; un'interfaccia che ci
/// vive dentro no. Il valore è poco sopra il croma dell'accento di serie
/// (≈0,16), quindi una tinta viva resta viva e una tinta al neon si calma senza
/// perdere la tonalità.
const TETTO_CROMA: f64 = 0.20;

/// Di quanto ci si sposta a ogni passo nella ricerca della chiarezza.
///
/// Un centesimo di OKLCH è sotto la soglia in cui l'occhio distingue due
/// chiarezze: cercare più fine costerebbe di più e darebbe lo stesso colore.
const GRANA: f64 = 0.01;

/// Quanti passi per direzione. Cento per un centesimo copre tutta la scala.
const PASSI: u32 = 100;

/// Fin dove si tollera che un canale esca da sRGB prima di chiamarlo fuori
/// gamut. È l'errore di arrotondamento del giro di conversioni, non un margine.
const TOLLERANZA: f64 = 1e-6;

/// Una tinta in OKLCH: chiarezza, croma, tonalità.
///
/// È lo spazio in cui si ragiona sui colori quando conta *come si vedono* e non
/// come sono codificati. Le tre coordinate sono indipendenti fra loro come in
/// nessuno degli spazi che il CSS aveva prima.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    /// Chiarezza percettiva: 0 è nero, 1 è bianco.
    pub l: f64,
    /// Quanto è colorata. Zero è grigio; oltre ~0,37 non esiste in sRGB.
    pub c: f64,
    /// La tonalità, in gradi da 0 a 360.
    pub h: f64,
}

// ── Le conversioni ──────────────────────────────────────────────────────────
//
// Le matrici sono quelle di Björn Ottosson, che ha definito OKLab; sono scritte
// per esteso invece di essere calcolate perché sono una costante della
// letteratura, non un risultato di questo programma.

/// Un canale sRGB verso la sua intensità lineare.
fn lineare(canale: u8) -> f64 {
    let c = f64::from(canale) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Un'intensità lineare verso il canale sRGB, ancora come numero reale.
fn gamma(intensita: f64) -> f64 {
    if intensita <= 0.003_130_8 {
        12.92 * intensita
    } else {
        1.055 * intensita.powf(1.0 / 2.4) - 0.055
    }
}

/// Un canale da un numero in 0..=1, arrotondato.
fn byte(valore: f64) -> u8 {
    let scalato = (valore.clamp(0.0, 1.0) * 255.0).round();
    // `scalato` è intero e sta in 0..=255 per costruzione: la conversione non
    // perde niente e non cambia segno.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "il valore è già bloccato in 0..=255 e arrotondato dalla riga sopra"
    )]
    let intero = scalato as u8;
    intero
}

/// Da RGB lineare a OKLCH.
fn lineare_in_oklch(r: f64, g: f64, b: f64) -> Oklch {
    let lungo = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let medio = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let corto = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();

    let chiarezza = 0.210_454_255_3 * lungo + 0.793_617_785_0 * medio - 0.004_072_046_8 * corto;
    let verde_rosso = 1.977_998_495_1 * lungo - 2.428_592_205_0 * medio + 0.450_593_709_9 * corto;
    let blu_giallo = 0.025_904_037_1 * lungo + 0.782_771_766_2 * medio - 0.808_675_766_0 * corto;

    let gradi = blu_giallo.atan2(verde_rosso).to_degrees();
    Oklch {
        l: chiarezza,
        c: verde_rosso.hypot(blu_giallo),
        h: if gradi < 0.0 { gradi + 360.0 } else { gradi },
    }
}

/// Da OKLCH a RGB lineare. I canali possono uscire da 0..=1: vuol dire che
/// quella tinta a quella chiarezza non esiste su uno schermo sRGB.
fn oklch_in_lineare(tinta: Oklch) -> (f64, f64, f64) {
    let radianti = tinta.h.to_radians();
    let verde_rosso = tinta.c * radianti.cos();
    let blu_giallo = tinta.c * radianti.sin();

    let lungo = (tinta.l + 0.396_337_777_4 * verde_rosso + 0.215_803_757_3 * blu_giallo).powi(3);
    let medio = (tinta.l - 0.105_561_345_8 * verde_rosso - 0.063_854_172_8 * blu_giallo).powi(3);
    let corto = (tinta.l - 0.089_484_177_5 * verde_rosso - 1.291_485_548_0 * blu_giallo).powi(3);

    (
        4.076_741_662_1 * lungo - 3.307_711_591_3 * medio + 0.230_969_929_2 * corto,
        -1.268_438_004_6 * lungo + 2.609_757_401_1 * medio - 0.341_319_396_5 * corto,
        -0.004_196_086_3 * lungo - 0.703_418_614_7 * medio + 1.707_614_701_0 * corto,
    )
}

/// La tinta esiste in sRGB?
fn dentro_il_gamut(tinta: Oklch) -> bool {
    let (r, g, b) = oklch_in_lineare(tinta);
    let ammesso = |canale: f64| (-TOLLERANZA..=1.0 + TOLLERANZA).contains(&canale);
    ammesso(r) && ammesso(g) && ammesso(b)
}

/// Il colore di una tinta, come lo vede lo schermo.
///
/// Opaco sempre: le varianti trasparenti si costruiscono con
/// [`Rgba::with_alpha`], e un accento semitrasparente non sarebbe misurabile
/// contro le superfici senza sapere quale ha sotto.
#[must_use]
pub(crate) fn in_rgba(tinta: Oklch) -> Rgba {
    let (r, g, b) = oklch_in_lineare(tinta);
    Rgba {
        r: byte(gamma(r)),
        g: byte(gamma(g)),
        b: byte(gamma(b)),
        a: 1.0,
    }
}

/// La tinta di un colore.
#[must_use]
pub(crate) fn in_oklch(colore: Rgba) -> Oklch {
    lineare_in_oklch(lineare(colore.r), lineare(colore.g), lineare(colore.b))
}

/// La stessa tinta, col croma ridotto quanto basta a esistere in sRGB.
///
/// Per bisezione e non a tentativi: il gamut di sRGB in OKLCH non ha una forma
/// che si scriva in chiuso, ma è **convesso lungo il croma** a chiarezza e
/// tonalità fisse — se un croma ci sta, ci stanno tutti quelli sotto. Venti
/// bisezioni portano l'errore sotto un milionesimo, cioè molto sotto un canale.
#[must_use]
pub(crate) fn dentro_srgb(tinta: Oklch) -> Oklch {
    if dentro_il_gamut(tinta) {
        return tinta;
    }
    let mut basso = 0.0;
    let mut alto = tinta.c;
    for _ in 0..20 {
        let meta = (basso + alto) / 2.0;
        if dentro_il_gamut(Oklch { c: meta, ..tinta }) {
            basso = meta;
        } else {
            alto = meta;
        }
    }
    Oklch { c: basso, ..tinta }
}

/// Il colore regge il contrasto contro tutto ciò che gli sta attorno?
fn regge(colore: Rgba, superfici: &[Rgba], sopra: Rgba, minimo: f64) -> bool {
    // Come testo, su ognuna delle superfici su cui può capitare.
    superfici
        .iter()
        .all(|fondo| contrast_ratio(colore, *fondo) >= minimo)
        // E come fondo, sotto il testo che ci si scrive sopra: `.bottone.primario`
        // mette `color: var(--color-surface-0)` su `background: var(--accent)`.
        // Controllare solo un verso vorrebbe dire spostare l'illeggibilità dal
        // testo in accento al testo sull'accento.
        && contrast_ratio(sopra, colore) >= minimo
}

/// L'accento sicuro più vicino a una tinta, o niente.
///
/// Conserva la tonalità di `tinta` e le sposta la chiarezza fino a quando
/// regge `minimo` contro tutte le `superfici` — dove finisce come testo — e
/// sotto `sopra`, il colore che ci si scrive sopra quando fa da fondo.
///
/// Restituisce `None` quando nessuna chiarezza va bene per tutte insieme. Chi
/// chiama deve tenersi l'accento della skin: è stato scelto da una persona che
/// l'ha guardato, ed è sempre meglio di un colore che qui non ha passato il
/// controllo.
#[must_use]
pub fn accento_sicuro(tinta: Rgba, superfici: &[Rgba], sopra: Rgba, minimo: f64) -> Option<Rgba> {
    let base = in_oklch(tinta);
    let croma = base.c.min(TETTO_CROMA);

    // Si prova prima la chiarezza originale, poi le due vicine, poi le due dopo:
    // la prima che passa è quella che somiglia di più alla copertina. Cercare in
    // una direzione sola vorrebbe dire scegliere in anticipo se il tema è scuro
    // o chiaro, che è precisamente la cosa che questa funzione non deve sapere.
    for passo in 0..=PASSI {
        let scarto = f64::from(passo) * GRANA;
        for chiarezza in [base.l + scarto, base.l - scarto] {
            if !(0.0..=1.0).contains(&chiarezza) {
                continue;
            }
            let colore = in_rgba(dentro_srgb(Oklch {
                l: chiarezza,
                c: croma,
                h: base.h,
            }));
            if regge(colore, superfici, sopra, minimo) {
                return Some(colore);
            }
        }
    }
    None
}

// ── Dalla copertina alla finestra ───────────────────────────────────────────

/// I token che si muovono insieme all'accento.
///
/// Sono quelli che **sono** l'accento scritto in un'altra opacità: il fondo di
/// una pillola selezionata, il suo alone, il primo velo ambientale dietro la
/// schermata di riproduzione. Se si muovesse solo `--accent` la finestra
/// resterebbe per metà del colore di prima, che è peggio che non muoversi.
///
/// `color.accent.like` e `color.hero` non sono in elenco e seguono lo stesso:
/// il compilatore li scrive come `var(--accent)` e `var(--accent-rgb)`, quindi
/// cambiano da soli. Vale anche per ogni token che una skin abbia dichiarato
/// con `{"$source": "albumArt.…"}` — è la ragione per cui il compilatore fa
/// puntare tutte e quattro le sorgenti allo stesso posto.
///
/// `color.ambient.2` invece resta dov'è, e non è una dimenticanza: è il
/// **secondo** alone, quello che una skin sceglie diverso dal primo per dare
/// profondità al velo. Tingerlo dello stesso colore ridurrebbe due aloni a uno.
const FAMIGLIA: &[&str] = &[
    "color.accent",
    "color.accent.soft",
    "color.accent.glow",
    "color.ambient.1",
];

/// Le variabili da scrivere sulla radice perché l'accento segua una copertina.
///
/// Restituisce coppie `(proprietà, valore)` già in CSS: chi chiama le scrive e
/// basta. È deliberato che non esca di qui un colore grezzo — il punto di tutto
/// questo modulo è che nessuno fuori debba avere un'opinione sui colori.
///
/// # Quando dice di no
///
/// - La skin non lo vuole (`capabilities.dynamicAccent` a `false`). È una
///   dichiarazione dell'autore, e vale più di una preferenza dell'utente:
///   una skin può essere costruita attorno al suo accento.
/// - Le superfici non si leggono, e allora non c'è niente contro cui misurare.
/// - Nessuna chiarezza di quella tonalità regge il contrasto — vedi
///   [`accento_sicuro`].
///
/// # Le superfici
///
/// Le stesse che [`check_skin`](crate::check_skin) usa per validare l'accento
/// **scritto a mano**, più la regola inversa del testo che sta sopra di esso.
/// Non una lista propria: due definizioni di «accento leggibile» nello stesso
/// programma vorrebbero dire che una delle due è sbagliata e non si sa quale.
#[must_use]
pub fn accento_dinamico(
    skin: &crate::document::SkinDocument,
    tinta: Rgba,
    chiaro: bool,
) -> Option<Vec<(String, String)>> {
    use crate::document::{CONTRASTO_MINIMO, DIETRO, colore_di};
    use crate::values::{format_color, format_rgb_triple};

    if !skin.capabilities.dynamic_accent {
        return None;
    }

    // La stessa cascata del CSS: nel tema chiaro si guarda prima l'insieme
    // chiaro e poi quello base. È la ragione per cui `colore_di` prende due
    // insiemi invece di uno.
    let (attivo, base) = match (chiaro, skin.light.as_ref()) {
        (true, Some(light)) => (light, Some(&skin.tokens)),
        _ => (&skin.tokens, None),
    };
    let leggi = |id: &str| colore_di(id, attivo, base, &skin.palette);

    let superfici: Vec<Rgba> = DIETRO.iter().filter_map(|id| leggi(id)).collect();
    // Il testo che sta **sopra** l'accento quando l'accento fa da fondo. Senza
    // di lui si sposterebbe l'illeggibilità invece di toglierla, ed è anche
    // l'unico modo di accorgersi che non c'è nessuna superficie da misurare.
    let sopra = leggi("color.surface.0")?;

    let sicuro = accento_sicuro(tinta, &superfici, sopra, CONTRASTO_MINIMO)?;

    let mut variabili = Vec::with_capacity(FAMIGLIA.len() + 1);
    for id in FAMIGLIA {
        // L'opacità la decide la skin, non noi: `--accent-soft` è l'accento al
        // 16% in `plain` e potrebbe essere al 30% altrove. Si prende quella che
        // c'è scritta e si cambia solo il colore sotto.
        let Some(quello_di_prima) = leggi(id) else {
            continue;
        };
        let Some(def) = crate::tokens::token(id) else {
            continue;
        };
        let nuovo = sicuro.with_alpha(quello_di_prima.a);
        // `color.hero` esiste solo come tripla, e scriverci dentro un `rgb(…)`
        // romperebbe le `rgba()` che lo usano. La regola è la stessa del
        // compilatore, e per lo stesso motivo.
        if def.rgb_triple == Some(def.css) {
            variabili.push((def.css.to_owned(), format_rgb_triple(nuovo)));
            continue;
        }
        variabili.push((def.css.to_owned(), format_color(nuovo)));
        if let Some(tripla) = def.rgb_triple {
            variabili.push((tripla.to_owned(), format_rgb_triple(nuovo)));
        }
    }

    // Nessuna variabile significa che la skin non dichiara nemmeno l'accento:
    // restituire un elenco vuoto farebbe credere a chi chiama di aver applicato
    // qualcosa.
    if variabili.is_empty() {
        return None;
    }
    Some(variabili)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CONTRASTO_MINIMO;

    /// Le quattro superfici del tema scuro di serie, dal foglio generato.
    const SCURE: [Rgba; 4] = [
        Rgba {
            r: 9,
            g: 9,
            b: 13,
            a: 1.0,
        },
        Rgba {
            r: 14,
            g: 14,
            b: 20,
            a: 1.0,
        },
        Rgba {
            r: 22,
            g: 22,
            b: 31,
            a: 1.0,
        },
        Rgba {
            r: 30,
            g: 30,
            b: 42,
            a: 1.0,
        },
    ];

    /// E quelle del tema chiaro.
    const CHIARE: [Rgba; 4] = [
        Rgba {
            r: 251,
            g: 251,
            b: 253,
            a: 1.0,
        },
        Rgba {
            r: 243,
            g: 243,
            b: 247,
            a: 1.0,
        },
        Rgba {
            r: 235,
            g: 235,
            b: 241,
            a: 1.0,
        },
        Rgba {
            r: 226,
            g: 226,
            b: 236,
            a: 1.0,
        },
    ];

    fn colore(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 1.0 }
    }

    /// La distanza fra due tonalità, tenendo conto che 359° e 1° distano 2.
    fn scarto_tonalita(uno: f64, due: f64) -> f64 {
        let grezzo = (uno - due).abs() % 360.0;
        grezzo.min(360.0 - grezzo)
    }

    #[test]
    fn il_giro_di_oklch_torna_al_punto_di_partenza() {
        // Se andata e ritorno non coincidono, tutto il resto misura un colore
        // che non è quello che si vede.
        for campione in [
            colore(0, 0, 0),
            colore(255, 255, 255),
            colore(139, 124, 246),
            colore(229, 72, 77),
            colore(52, 211, 153),
            colore(250, 204, 21),
            colore(9, 9, 13),
            colore(1, 2, 3),
        ] {
            let ritorno = in_rgba(in_oklch(campione));
            assert_eq!(ritorno, campione, "andata e ritorno su {campione:?}");
        }
    }

    #[test]
    fn il_grigio_non_ha_tonalita_e_il_bianco_e_in_cima() {
        let grigio = in_oklch(colore(128, 128, 128));
        assert!(grigio.c < 0.001, "un grigio non è colorato: {}", grigio.c);
        let bianco = in_oklch(colore(255, 255, 255));
        assert!(
            (bianco.l - 1.0).abs() < 0.001,
            "il bianco è 1: {}",
            bianco.l
        );
        let nero = in_oklch(colore(0, 0, 0));
        assert!(nero.l.abs() < 0.001, "il nero è 0: {}", nero.l);
    }

    #[test]
    fn una_tinta_impossibile_rientra_senza_cambiare_colore() {
        // Croma 0,4 non esiste in sRGB a nessuna tonalità: si deve ridurre, e
        // la tonalità deve restare quella.
        let assurda = Oklch {
            l: 0.6,
            c: 0.4,
            h: 150.0,
        };
        let ridotta = dentro_srgb(assurda);
        assert!(ridotta.c < assurda.c, "il croma doveva scendere");
        assert!(dentro_il_gamut(ridotta), "e finire dentro sRGB");
        assert!(
            scarto_tonalita(ridotta.h, assurda.h) < 0.001,
            "la tonalità non si tocca"
        );
    }

    #[test]
    fn su_un_tema_scuro_una_tinta_cupa_viene_schiarita() {
        // Un blu notte preso da una copertina scura: contro `--color-surface-3`
        // sta sotto 2:1, e come testo sparirebbe.
        let cupa = colore(24, 28, 74);
        assert!(contrast_ratio(cupa, SCURE[3]) < 2.0, "premessa della prova");

        let sicuro = accento_sicuro(cupa, &SCURE, SCURE[0], CONTRASTO_MINIMO).unwrap();
        assert!(
            in_oklch(sicuro).l > in_oklch(cupa).l,
            "su un fondo scuro si sale"
        );
        for superficie in SCURE {
            assert!(
                contrast_ratio(sicuro, superficie) >= CONTRASTO_MINIMO,
                "{sicuro:?} su {superficie:?} sta a {}",
                contrast_ratio(sicuro, superficie)
            );
        }
    }

    #[test]
    fn sul_tema_chiaro_la_stessa_tinta_va_dall_altra_parte() {
        // Il giallo è il caso che rompeva il vecchio albero: acceso quanto basta
        // per essere invisibile su bianco.
        let giallo = colore(250, 204, 21);
        assert!(
            contrast_ratio(giallo, CHIARE[0]) < 1.6,
            "premessa della prova"
        );

        let scuro = accento_sicuro(giallo, &SCURE, SCURE[0], CONTRASTO_MINIMO).unwrap();
        let chiaro = accento_sicuro(giallo, &CHIARE, CHIARE[0], CONTRASTO_MINIMO).unwrap();
        assert!(
            in_oklch(chiaro).l < in_oklch(scuro).l,
            "la stessa tinta è più scura sul tema chiaro: {chiaro:?} contro {scuro:?}"
        );
        for superficie in CHIARE {
            assert!(contrast_ratio(chiaro, superficie) >= CONTRASTO_MINIMO);
        }
    }

    #[test]
    fn la_tonalita_della_copertina_si_riconosce_ancora() {
        // È tutto il punto: se si potesse cambiare tonalità, la risposta sicura
        // sarebbe sempre bianco o nero, e l'accento non seguirebbe più niente.
        for campione in [
            colore(229, 72, 77),
            colore(52, 211, 153),
            colore(250, 204, 21),
            colore(24, 28, 74),
            colore(200, 90, 200),
        ] {
            let sicuro = accento_sicuro(campione, &SCURE, SCURE[0], CONTRASTO_MINIMO).unwrap();
            assert!(
                scarto_tonalita(in_oklch(sicuro).h, in_oklch(campione).h) < 2.0,
                "{campione:?} è diventato {sicuro:?}"
            );
        }
    }

    #[test]
    fn qualunque_tonalita_trova_una_risposta_sui_temi_di_serie() {
        // Il giro completo delle tonalità a croma pieno: su entrambi i temi non
        // deve esserci un disco che spegne il meccanismo per caso.
        for grado in 0..360 {
            let tinta = in_rgba(dentro_srgb(Oklch {
                l: 0.55,
                c: 0.30,
                h: f64::from(grado),
            }));
            for (superfici, sopra) in [(SCURE, SCURE[0]), (CHIARE, CHIARE[0])] {
                let sicuro = accento_sicuro(tinta, &superfici, sopra, CONTRASTO_MINIMO)
                    .unwrap_or_else(|| panic!("{grado}° non ha un accento sicuro"));
                for superficie in superfici {
                    assert!(
                        contrast_ratio(sicuro, superficie) >= CONTRASTO_MINIMO,
                        "{grado}°: {sicuro:?} su {superficie:?}"
                    );
                }
                assert!(
                    contrast_ratio(sopra, sicuro) >= CONTRASTO_MINIMO,
                    "{grado}°: il testo sopra l'accento"
                );
            }
        }
    }

    #[test]
    fn l_accento_non_diventa_fluorescente() {
        let neon = in_rgba(dentro_srgb(Oklch {
            l: 0.7,
            c: 0.35,
            h: 30.0,
        }));
        let sicuro = accento_sicuro(neon, &SCURE, SCURE[0], CONTRASTO_MINIMO).unwrap();
        assert!(
            in_oklch(sicuro).c <= TETTO_CROMA + 0.001,
            "croma {}",
            in_oklch(sicuro).c
        );
    }

    #[test]
    fn quando_non_si_puo_non_si_inventa() {
        // 21:1 è il contrasto fra nero e bianco: è il massimo che esista, e
        // nessun colore lo tiene contro quattro superfici diverse. La risposta
        // giusta è `None`, non il colore meno peggio.
        assert!(
            accento_sicuro(colore(250, 204, 21), &SCURE, SCURE[0], 21.0).is_none(),
            "un vincolo impossibile deve restare senza risposta"
        );
    }

    // ── Sulla skin di serie vera ────────────────────────────────────────────

    fn plain() -> crate::document::SkinDocument {
        crate::parse_skin_json(crate::PLAIN_SOURCE).expect("la skin di serie si legge")
    }

    fn cerca<'a>(variabili: &'a [(String, String)], nome: &str) -> Option<&'a str> {
        variabili
            .iter()
            .find(|(chiave, _)| chiave == nome)
            .map(|(_, valore)| valore.as_str())
    }

    #[test]
    fn la_famiglia_dell_accento_si_muove_tutta_insieme() {
        let variabili =
            accento_dinamico(&plain(), colore(230, 120, 30), false).expect("plain lo permette");

        // Il colore, la sua tripla, e le due varianti con l'opacità della skin.
        let pieno = cerca(&variabili, "--accent").expect("--accent");
        let tripla = cerca(&variabili, "--accent-rgb").expect("--accent-rgb");
        let soft = cerca(&variabili, "--accent-soft").expect("--accent-soft");
        let glow = cerca(&variabili, "--accent-glow").expect("--accent-glow");
        let ambiente = cerca(&variabili, "--ambient-1").expect("--ambient-1");

        assert!(pieno.starts_with("rgb(") && !pieno.contains('/'), "{pieno}");
        assert!(!tripla.contains("rgb"), "la tripla è nuda: {tripla}");
        // Le opacità sono quelle che `plain` ha scritto, non numeri nostri.
        assert!(soft.ends_with("/ 0.16)"), "{soft}");
        assert!(glow.ends_with("/ 0.35)"), "{glow}");
        assert!(ambiente.ends_with("/ 0.1)"), "{ambiente}");
    }

    #[test]
    fn l_accento_scritto_dalla_copertina_regge_le_stesse_soglie_di_quello_a_mano() {
        // Il controllo che rende il meccanismo accettabile: qualunque disco, in
        // entrambi i temi, la finestra resta leggibile quanto prima.
        let skin = plain();
        let chiare = skin.light.as_ref().expect("plain ha un tema chiaro");
        for grado in (0..360).step_by(5) {
            let tinta = in_rgba(dentro_srgb(Oklch {
                l: 0.6,
                c: 0.28,
                h: f64::from(grado),
            }));
            for chiaro in [false, true] {
                let (attivo, base) = if chiaro {
                    (chiare, Some(&skin.tokens))
                } else {
                    (&skin.tokens, None)
                };
                let variabili =
                    accento_dinamico(&skin, tinta, chiaro).expect("plain risponde sempre");
                let scritto = cerca(&variabili, "--accent").expect("--accent");
                let accento = crate::values::parse_color(scritto).expect("è un colore");

                // Le superfici vere di `plain`, lette dal documento: se domani la
                // skin cambia fondo, questa prova cambia con lei.
                for id in crate::document::DIETRO {
                    let superficie = crate::document::colore_di(id, attivo, base, &skin.palette)
                        .unwrap_or_else(|| panic!("{id} si legge"));
                    let misurato = contrast_ratio(accento, superficie);
                    assert!(
                        misurato >= CONTRASTO_MINIMO,
                        "{grado}° chiaro={chiaro}: l'accento su {id} sta a {misurato}"
                    );
                }
                let sopra =
                    crate::document::colore_di("color.surface.0", attivo, base, &skin.palette)
                        .expect("surface.0 si legge");
                let misurato = contrast_ratio(sopra, accento);
                assert!(
                    misurato >= CONTRASTO_MINIMO,
                    "{grado}° chiaro={chiaro}: il testo sopra l'accento sta a {misurato}"
                );
            }
        }
    }

    #[test]
    fn una_skin_che_non_lo_vuole_non_lo_riceve() {
        // `sala` dichiara `dynamicAccent: false`: è una scelta dell'autore, ed è
        // più forte di qualunque preferenza — la skin può essere costruita
        // attorno al suo accento.
        let mut skin = plain();
        skin.capabilities.dynamic_accent = false;
        assert!(accento_dinamico(&skin, colore(230, 120, 30), false).is_none());
    }

    #[test]
    fn senza_superfici_resta_il_vincolo_del_testo_sopra() {
        // Un elenco vuoto non deve diventare «va bene tutto»: l'accento fa
        // ancora da fondo a `--color-surface-0`.
        let sicuro = accento_sicuro(colore(250, 204, 21), &[], SCURE[0], CONTRASTO_MINIMO).unwrap();
        assert!(contrast_ratio(SCURE[0], sicuro) >= CONTRASTO_MINIMO);
    }
}
