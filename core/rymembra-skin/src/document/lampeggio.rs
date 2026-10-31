//! Il lampeggio: la misura che rifiuta, come errore, le animazioni che
//! accendono e spengono una luce troppo in fretta (WCAG 2.3.1).

use super::{SkinIssue, SkinMotion, giu, problema};
use crate::movimento::{AnimDirection, AnimFrame, Curva, Iterazioni};
use crate::parts::Ombra;
use crate::tokens::ColorValue;
use crate::values::Easing;

// ── Il lampeggio ────────────────────────────────────────────────────────────
//
// Una skin si scambia, e chi la indossa non l'ha scritta. Un'animazione che
// accende e spegne una luce dieci volte al secondo, per sempre, non è uno stile
// brutto: in chi soffre di epilessia fotosensibile può scatenare una crisi, ed
// è il motivo per cui WCAG 2.3.1 ne fa un criterio di livello A. Le regole
// d'arresto del compilatore non bastano, perché scattano solo per chi ha già
// chiesto meno movimento — e chi non sa di essere sensibile non l'ha chiesto.
//
// Quindi è un **errore** e non un avviso: una skin che lampeggia non si
// installa, non si prova sull'app, e lo Studio lo dice sulla durata
// dell'animazione.
//
// # Cosa si misura
//
// Il tempo di un lampo: due cambi di luce opposti uno dopo l'altro — sale e
// scende, si accende e si spegne — dall'inizio del primo alla fine del secondo.
// La metà di quel tempo è il **mezzo ciclo**, e sotto [`MEZZO_CICLO_MINIMO_MS`]
// è un lampeggio. Per ogni proprietà che si vede come luce, separatamente,
// perché il motore anima ogni proprietà sulla sua traccia:
//
// - un fotogramma che non dichiara la proprietà non conta; se non la dichiara
//   il primo o l'ultimo, lì vale quel che la parte ha di suo, che non si
//   conosce e quindi è diverso da tutto;
// - due passi di fila nello stesso verso — 0 → 0,5 → 1 — sono un cambio solo:
//   una salita scritta in più fotogrammi non è un tremolio;
// - con `normal` e `reverse`, alla fine di ogni giro la luce **salta** al
//   primo fotogramma: è un cambio istantaneo, e conta;
// - con `alternate` il giro dura due volte, e al ritorno ogni cambio si rifà
//   al contrario;
// - la durata è quella che il foglio userà: con l'intensità `essential` il
//   compilatore la dimezza, e il conto si fa sulla metà;
// - la curva conta quando torna indietro: una `cubic-bezier` che esce da
//   [0, 1] fa andare e venire la luce **dentro** un intervallo, e lì i cambi
//   sono quelli della curva, non quello dei fotogrammi;
// - un ciclo, o un'animazione ripetuta più di tre volte, si giudica dal suo
//   lampo più breve; una corsa che si ferma entro tre giri si giudica
//   contando i lampi rapidi che fa in tutto, e più di tre non passano.
//
// La soglia è più severa di quella di WCAG — tre lampi al secondo vorrebbero
// dire un mezzo ciclo di 166 ms — e di proposito: il criterio misura anche
// l'area e la luminanza, e qui non si sa quanto grande sarà la parte né su
// cosa starà. Un colore che pulsa in due secondi resta lontanissimo dalla
// soglia, un respiro di luminosità anche; un fondo che scatta cento volte al
// minuto no.

/// Il mezzo ciclo più corto che una luce ripetuta può avere, in millisecondi.
///
/// Un terzo di secondo: un lampo intero dura almeno due terzi, cioè non più di
/// un lampo e mezzo al secondo. Vedi «Il lampeggio» in questo file.
pub const MEZZO_CICLO_MINIMO_MS: f64 = 333.0;

/// Quante ripetizioni un'animazione può fare prima di essere misurata come un
/// ciclo, e quanti lampi rapidi può fare in tutto quando si ferma prima.
///
/// Tre come i tre lampi che WCAG 2.3.1 ammette. Ma le ripetizioni non sono i
/// lampi: un giro solo di dodici fotogrammi che accendono e spengono ne
/// contiene cinque. Fino alla 3.2.0 di prova questo commento diceva che un'animazione
/// ripetuta al più tre volte «non può farne di più, a qualunque velocità», e il
/// controllo la saltava. Adesso oltre tre ripetizioni l'animazione si misura
/// come un ciclo — decide il lampo più breve, anche a cavallo fra due giri — e
/// fino a tre si **contano** i lampi rapidi di tutta la corsa, che non devono
/// essere più di tre. Vedi «Il lampeggio» in questo file.
pub const RIPETIZIONI_SENZA_LAMPEGGIO: u32 = 3;

/// Un valore che si vede come luce, su un fotogramma.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Luce<'a> {
    /// Quel che la parte ha di suo, dove la traccia non dichiara la proprietà.
    DellaParte,
    /// Un numero: opacità, sfocatura, luminosità, saturazione, tinta.
    Numero(f64),
    /// Un colore.
    Colore(&'a ColorValue),
    /// Un'ombra.
    Ombra(&'a Ombra),
}

impl Luce<'_> {
    /// In che verso va il cambio da qui a lì, quando lo si può dire.
    ///
    /// Solo fra due numeri: un colore che diventa un altro colore non sale e
    /// non scende, e due cambi di colore di fila si contano per due.
    fn verso(self, poi: Self) -> Option<std::cmp::Ordering> {
        match (self, poi) {
            (Self::Numero(da), Self::Numero(a)) => da.partial_cmp(&a),
            _ => None,
        }
    }
}

/// Fra quali estremi il motore taglia un numero animato.
type Taglio = (f64, f64);

/// Nessun estremo: la tinta gira quanto vuole.
const SENZA_TAGLIO: Taglio = (f64::NEG_INFINITY, f64::INFINITY);

/// Da zero in su: sfocatura, luminosità e saturazione negative non esistono.
const DA_ZERO: Taglio = (0.0, f64::INFINITY);

/// Una proprietà che si vede come luce: come si chiama in un messaggio, come
/// si legge da un fotogramma, e fra quali estremi il motore la taglia quando
/// una curva la porta oltre i suoi fotogrammi.
type Tracciato = (
    &'static str,
    for<'a> fn(&'a AnimFrame) -> Option<Luce<'a>>,
    Taglio,
);

/// Le proprietà che, cambiando in fretta, lampeggiano. Le trasformazioni e il
/// ritaglio restano fuori: spostano la parte, non ne cambiano la luce.
///
/// Il taglio dei colori e delle ombre non sta qui: non sono numeri, e il loro
/// si fa sulla curva — vedi [`cambi_dell_intervallo`].
const LUCI: &[Tracciato] = &[
    ("l'opacità", |f| f.opacity.map(Luce::Numero), (0.0, 1.0)),
    (
        "la sfocatura",
        |f| f.blur.map(|l| Luce::Numero(l.value)),
        DA_ZERO,
    ),
    ("la luminosità", |f| f.brightness.map(Luce::Numero), DA_ZERO),
    ("la saturazione", |f| f.saturate.map(Luce::Numero), DA_ZERO),
    ("la tinta", |f| f.hue_rotate.map(Luce::Numero), SENZA_TAGLIO),
    (
        "il colore di fondo",
        |f| f.background_color.as_ref().map(Luce::Colore),
        SENZA_TAGLIO,
    ),
    (
        "il colore del testo",
        |f| f.color.as_ref().map(Luce::Colore),
        SENZA_TAGLIO,
    ),
    (
        "il colore del bordo",
        |f| f.border_color.as_ref().map(Luce::Colore),
        SENZA_TAGLIO,
    ),
    (
        "l'ombra",
        |f| f.shadow.as_ref().map(Luce::Ombra),
        SENZA_TAGLIO,
    ),
];

/// Un cambio di luce dentro un giro, in millisecondi dall'inizio del giro.
#[derive(Debug, Clone, Copy)]
struct Cambio {
    inizio: f64,
    fine: f64,
    verso: Option<std::cmp::Ordering>,
}

/// Quanti punti si prendono da una curva che rimbalza.
///
/// Sessantaquattro bastano a trovare ogni inversione di una cubica, che ne ha
/// al più due, con un errore di tempo sotto il due per cento dell'intervallo.
const CAMPIONI_CURVA: u32 = 64;

/// La curva di un'animazione, campionata, quando può tornare indietro.
///
/// Il compilatore la scrive nello shorthand `animation`, e il motore la applica
/// a **ogni** intervallo fra due fotogrammi. Una `cubic-bezier` con un punto di
/// controllo fuori da [0, 1] in y esce dall'intervallo e ci rientra: dentro lo
/// stesso passo la luce va oltre la meta, torna indietro, e magari ci rivà.
/// Fino alla 3.2.0 di prova il conto la ignorava, e `(0.5, 5, 0.5, -4)` su un'opacità da
/// 350 ms per metà giro faceva sei cambi pieni ogni 700 ms senza che nessuno
/// lo dicesse.
///
/// `None` per tutte le altre: le parole chiave, i gradini e le cubiche con le y
/// in [0, 1] vanno in un verso solo, e per loro un intervallo resta un cambio
/// solo — il conto di sempre, intatto. Una curva richiamata per nome si cerca
/// in `motion.easings`. Le coppie sono (x, y) per il parametro da 0 a 1: la x
/// cresce con lui, perché il formato tiene le x fra 0 e 1.
fn curva_che_rimbalza(motion: &SkinMotion, curva: &Curva) -> Option<Vec<(f64, f64)>> {
    let easing = match curva {
        Curva::Valore(easing) => *easing,
        Curva::Nome(nome) => motion
            .easings
            .iter()
            .find(|(n, _)| n == nome)
            .map(|(_, easing)| easing)
            .copied()?,
    };
    let Easing::CubicBezier([x1, y1, x2, y2]) = easing else {
        return None;
    };
    if (0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2) {
        return None;
    }
    let cubica = |a: f64, b: f64, t: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    Some(
        (0..=CAMPIONI_CURVA)
            .map(|passo| {
                let t = f64::from(passo) / f64::from(CAMPIONI_CURVA);
                (cubica(x1, x2, t), cubica(y1, y2, t))
            })
            .collect(),
    )
}

/// I cambi di luce fra due fotogrammi, in millisecondi.
///
/// Senza una curva che rimbalza è un cambio solo, quello di sempre. Con una
/// curva che rimbalza si segue il valore vero lungo la curva e si spezza dove
/// cambia verso. Un numero si taglia dove il motore lo taglia — l'opacità fra
/// 0 e 1, sfocatura, luminosità e saturazione da 0 in su — perché la molla
/// che porta un'opacità a 1,1 e la riporta a 1 non cambia niente sullo
/// schermo. Un colore o un'ombra non hanno un numero da tagliare: si taglia la
/// curva fra 0 e 1, cioè si contano i ritorni che attraversano il passo e non
/// il piccolo sorpasso oltre la meta, che esce dalla gamma e lì si ferma.
fn cambi_dell_intervallo<'a>(
    (da, prima): (f64, Luce<'a>),
    (a, dopo): (f64, Luce<'a>),
    curva: Option<&[(f64, f64)]>,
    taglio: Taglio,
    cambi: &mut Vec<Cambio>,
) {
    if prima == dopo {
        return;
    }
    let Some(campioni) = curva else {
        cambi.push(Cambio {
            inizio: da,
            fine: a,
            verso: prima.verso(dopo),
        });
        return;
    };
    let numeri = match (prima, dopo) {
        (Luce::Numero(v0), Luce::Numero(v1)) => Some((v0, v1)),
        _ => None,
    };
    let valore = |y: f64| match numeri {
        Some((v0, v1)) => (v0 + (v1 - v0) * y).clamp(taglio.0, taglio.1),
        None => y.clamp(0.0, 1.0),
    };
    let tempo = |x: f64| da + (a - da) * x;

    // Un tratto in corso: dove comincia, dove arriva, in che verso.
    let mut tratto: Option<(f64, f64, std::cmp::Ordering)> = None;
    let mut chiudi = |tratto: (f64, f64, std::cmp::Ordering)| {
        cambi.push(Cambio {
            inizio: tempo(tratto.0),
            fine: tempo(tratto.1),
            verso: numeri.map(|_| tratto.2),
        });
    };
    for coppia in campioni.windows(2) {
        let [(x0, y0), (x1, y1)] = coppia else {
            continue;
        };
        let (prima, poi) = (valore(*y0), valore(*y1));
        if (poi - prima).abs() <= f64::EPSILON {
            continue;
        }
        let verso = prima.total_cmp(&poi);
        tratto = match tratto {
            Some((inizio, _, corrente)) if corrente == verso => Some((inizio, *x1, verso)),
            Some(finito) => {
                chiudi(finito);
                Some((*x0, *x1, verso))
            }
            None => Some((*x0, *x1, verso)),
        };
    }
    if let Some(finito) = tratto {
        chiudi(finito);
    }
}

/// Un giro di una proprietà: i suoi cambi in avanti, in millisecondi, e la
/// luce con cui comincia e finisce.
struct Giro<'a> {
    cambi: Vec<Cambio>,
    primo: Luce<'a>,
    ultimo: Luce<'a>,
}

/// Il giro di una proprietà, o `None` se nessun fotogramma la dichiara.
///
/// `durata` è quella di un giro, già scalata; `curva` è quella di
/// [`curva_che_rimbalza`].
fn giro<'a>(
    fotogrammi: &'a [AnimFrame],
    durata: f64,
    leggi: for<'b> fn(&'b AnimFrame) -> Option<Luce<'b>>,
    curva: Option<&[(f64, f64)]>,
    taglio: Taglio,
) -> Option<Giro<'a>> {
    let mut punti: Vec<(f64, Luce<'a>)> = fotogrammi
        .iter()
        .filter_map(|f| leggi(f).map(|luce| (f.at, luce)))
        .collect();
    let (&(primo, _), &(ultimo, _)) = (punti.first()?, punti.last()?);
    if primo > 0.0 {
        punti.insert(0, (0.0, Luce::DellaParte));
    }
    if ultimo < 100.0 {
        punti.push((100.0, Luce::DellaParte));
    }
    let tempo = |at: f64| durata * at / 100.0;

    let mut cambi: Vec<Cambio> = Vec::new();
    for coppia in punti.windows(2) {
        if let [(da, prima), (a, dopo)] = coppia {
            cambi_dell_intervallo(
                (tempo(*da), *prima),
                (tempo(*a), *dopo),
                curva,
                taglio,
                &mut cambi,
            );
        }
    }
    Some(Giro {
        cambi,
        primo: punti.first().map_or(Luce::DellaParte, |(_, luce)| *luce),
        ultimo: punti.last().map_or(Luce::DellaParte, |(_, luce)| *luce),
    })
}

/// Due cambi di fila nello stesso verso sono uno solo: una salita scritta in
/// più fotogrammi non è un tremolio.
fn unisci(corsa: Vec<Cambio>) -> Vec<Cambio> {
    let mut cambi: Vec<Cambio> = Vec::with_capacity(corsa.len());
    for cambio in corsa {
        match cambi.last_mut() {
            Some(prima) if prima.verso.is_some() && prima.verso == cambio.verso => {
                prima.fine = cambio.fine;
            }
            _ => cambi.push(cambio),
        }
    }
    cambi
}

/// Il mezzo ciclo più corto di una proprietà che si ripete senza fermarsi, o
/// `None` se non cambia mai.
///
/// `alterna` vale per `alternate` e `alternate-reverse`. `reverse` è `normal`
/// guardato allo specchio: gli intervalli fra un cambio e l'altro sono gli
/// stessi.
fn mezzo_ciclo(giro: &Giro<'_>, durata: f64, alterna: bool) -> Option<f64> {
    let mut corsa = giro.cambi.clone();
    let periodo = if alterna {
        // Il ritorno: gli stessi cambi, al contrario e nel verso opposto.
        let periodo = durata * 2.0;
        let ritorno: Vec<Cambio> = giro
            .cambi
            .iter()
            .rev()
            .map(|c| Cambio {
                inizio: periodo - c.fine,
                fine: periodo - c.inizio,
                verso: c.verso.map(std::cmp::Ordering::reverse),
            })
            .collect();
        corsa.extend(ritorno);
        periodo
    } else {
        // Il salto dall'ultimo fotogramma al primo, fra un giro e l'altro.
        if giro.ultimo != giro.primo {
            corsa.push(Cambio {
                inizio: durata,
                fine: durata,
                verso: giro.ultimo.verso(giro.primo),
            });
        }
        durata
    };

    // Due cambi di fila nello stesso verso sono uno solo, anche a cavallo fra
    // la fine di un giro e l'inizio del successivo.
    let mut cambi = unisci(corsa);
    if cambi.len() > 1
        && let (Some(&primo), Some(&ultimo)) = (cambi.first(), cambi.last())
        && primo.verso.is_some()
        && primo.verso == ultimo.verso
    {
        cambi.pop();
        if let Some(testa) = cambi.first_mut() {
            testa.inizio = ultimo.inizio - periodo;
        }
    }

    // Ogni cambio col successivo — l'ultimo col primo del giro dopo — è un
    // lampo. Il più breve decide.
    let quanti = cambi.len();
    (0..quanti)
        .filter_map(|indice| {
            let questo = cambi.get(indice)?;
            let (poi, oltre) = match cambi.get(indice + 1) {
                Some(poi) => (poi, 0.0),
                None => (cambi.first()?, periodo),
            };
            Some((poi.fine + oltre - questo.inizio) / 2.0)
        })
        .reduce(f64::min)
}

/// I lampi rapidi di una proprietà in una corsa che si ferma dopo `volte`
/// giri, e il mezzo ciclo del più breve.
///
/// La corsa si srotola per intero: con `normal` e `reverse` il salto al primo
/// fotogramma avviene fra un giro e l'altro, cioè `volte - 1` volte — dopo
/// l'ultimo vale `fill` — e con `alternate` i giri pari tornano indietro. Un
/// lampo rapido è una coppia di cambi di fila con un mezzo ciclo sotto
/// [`MEZZO_CICLO_MINIMO_MS`]; le coppie non si sovrappongono, perché un lampo è
/// un'andata e un ritorno e un cambio non fa da ritorno a uno e da andata al
/// successivo. È il conto di WCAG: tre lampi sono sei cambi.
fn lampi_rapidi(giro: &Giro<'_>, durata: f64, direzione: AnimDirection, volte: u32) -> (u32, f64) {
    let specchio: Vec<Cambio> = giro
        .cambi
        .iter()
        .rev()
        .map(|c| Cambio {
            inizio: durata - c.fine,
            fine: durata - c.inizio,
            verso: c.verso.map(std::cmp::Ordering::reverse),
        })
        .collect();
    let salta = matches!(direzione, AnimDirection::Normal | AnimDirection::Reverse);
    let mut corsa: Vec<Cambio> = Vec::new();
    for volta in 0..volte {
        let avanti = match direzione {
            AnimDirection::Normal => true,
            AnimDirection::Reverse => false,
            AnimDirection::Alternate => volta % 2 == 0,
            AnimDirection::AlternateReverse => volta % 2 == 1,
        };
        let inizio = durata * f64::from(volta);
        if volta > 0 && salta {
            let (da, a) = if avanti {
                (giro.ultimo, giro.primo)
            } else {
                (giro.primo, giro.ultimo)
            };
            if da != a {
                corsa.push(Cambio {
                    inizio,
                    fine: inizio,
                    verso: da.verso(a),
                });
            }
        }
        let questo = if avanti { &giro.cambi } else { &specchio };
        corsa.extend(questo.iter().map(|c| Cambio {
            inizio: c.inizio + inizio,
            fine: c.fine + inizio,
            verso: c.verso,
        }));
    }

    let cambi = unisci(corsa);
    let mut lampi = 0_u32;
    let mut piu_breve = f64::INFINITY;
    let mut indice = 0;
    while let (Some(questo), Some(poi)) = (cambi.get(indice), cambi.get(indice + 1)) {
        let mezzo = (poi.fine - questo.inizio) / 2.0;
        if mezzo < MEZZO_CICLO_MINIMO_MS {
            lampi += 1;
            piu_breve = piu_breve.min(mezzo);
            indice += 2;
        } else {
            indice += 1;
        }
    }
    (lampi, piu_breve)
}

/// Le animazioni che lampeggiano, come errori sulla loro durata.
pub(super) fn lampeggi(motion: &SkinMotion) -> Vec<SkinIssue> {
    // La durata che il foglio userà: `essential` la dimezza. `maximum` la
    // allunga, e non si conta — lo Studio fa scorrere le animazioni alla loro
    // durata scritta, e il conto deve valere anche lì. `none` le ferma tutte,
    // e nemmeno questo si conta: una scala a zero non ferma un ciclo, lo rende
    // infinitamente veloce, e a fermarlo è una regola del foglio che non deve
    // essere l'unica cosa fra una skin e un lampeggio.
    let scala = match motion.intensity.scale() {
        s if s > 0.0 => s.min(1.0),
        _ => 1.0,
    };
    let mut errori = Vec::new();
    for (nome, animazione) in &motion.animations {
        let alterna = matches!(
            animazione.direction,
            AnimDirection::Alternate | AnimDirection::AlternateReverse
        );
        let durata = animazione.duration.ms * scala;
        let curva = curva_che_rimbalza(motion, &animazione.easing);
        let giri = LUCI.iter().filter_map(|(cosa, leggi, taglio)| {
            giro(
                &animazione.frames,
                durata,
                *leggi,
                curva.as_deref(),
                *taglio,
            )
            .map(|giro| (*cosa, giro))
        });

        let (cosa, mezzo, quanto, consiglio) = match animazione.iterations {
            // Una corsa che si ferma presto: si contano i lampi.
            Iterazioni::Volte(volte) if volte <= RIPETIZIONI_SENZA_LAMPEGGIO => {
                let peggiore = giri
                    .map(|(cosa, giro)| {
                        let (lampi, mezzo) =
                            lampi_rapidi(&giro, durata, animazione.direction, volte);
                        (cosa, lampi, mezzo)
                    })
                    .filter(|(_, lampi, _)| *lampi > RIPETIZIONI_SENZA_LAMPEGGIO)
                    .max_by_key(|(_, lampi, _)| *lampi);
                let Some((cosa, lampi, mezzo)) = peggiore else {
                    continue;
                };
                let dove = if volte == 1 {
                    "in un giro solo".to_owned()
                } else {
                    format!("in {volte} giri")
                };
                (
                    cosa,
                    mezzo,
                    format!("fa {lampi} lampi {dove}"),
                    format!(
                        "o anima una trasformazione invece della luce: più di \
                         {RIPETIZIONI_SENZA_LAMPEGGIO} lampi rapidi non passano nemmeno in \
                         un'animazione che si ferma."
                    ),
                )
            }
            // Un ciclo, o quasi: decide il lampo più breve.
            iterazioni => {
                let ripete = match iterazioni {
                    Iterazioni::Infinite => "si ripete all'infinito".to_owned(),
                    Iterazioni::Volte(n) => format!("si ripete {n} volte"),
                };
                let peggiore = giri
                    .filter_map(|(cosa, giro)| {
                        mezzo_ciclo(&giro, durata, alterna).map(|m| (cosa, m))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let Some((cosa, mezzo)) = peggiore.filter(|(_, m)| *m < MEZZO_CICLO_MINIMO_MS)
                else {
                    continue;
                };
                (
                    cosa,
                    mezzo,
                    ripete,
                    format!(
                        "anima una trasformazione invece della luce, o fermala a \
                         {RIPETIZIONI_SENZA_LAMPEGGIO} ripetizioni."
                    ),
                )
            }
        };
        let dimezzata = if scala < 1.0 {
            format!(
                " Con l'intensità «{}» il foglio moltiplica ogni durata per {scala}, e il conto \
                 si fa sulla durata che il foglio userà.",
                motion.intensity.as_str()
            )
        } else {
            String::new()
        };
        errori.push(problema(
            &giu(&giu(&giu("motion", "animations"), nome), "duration"),
            format!(
                "«{nome}» lampeggia: {cosa} cambia ogni {mezzo:.0} ms e {quanto}, sotto i \
                 {MEZZO_CICLO_MINIMO_MS:.0} ms per mezzo ciclo. Una luce che cambia così in fretta \
                 e si ripete può scatenare una crisi in chi soffre di epilessia fotosensibile \
                 (WCAG 2.3.1). Allunga la durata, togli i fotogrammi che vanno e vengono, \
                 {consiglio}{dimezzata}"
            ),
        ));
    }
    errori
}
