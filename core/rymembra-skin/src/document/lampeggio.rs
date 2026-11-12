//! Il lampeggio: la misura che rifiuta, come errore, le animazioni che
//! accendono e spengono una luce troppo in fretta (WCAG 2.3.1).

use super::{SkinIssue, SkinMotion, giu, problema};
use crate::movimento::{AnimDirection, AnimFrame, Curva, Iterazioni, Ritaglio};
use crate::parts::Ombra;
use crate::tokens::ColorValue;
use crate::values::{Easing, Length, LengthUnit};

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

/// Come si chiama una luce di [`LUCI`] nella forma che si traduce: vedi
/// [`crate::motivo`].
fn motivo_luce(cosa: &'static str) -> crate::motivo::Motivo {
    match cosa {
        "l'opacità" => motivo!("luce.opacita", "l'opacità"),
        "la sfocatura" => motivo!("luce.sfocatura", "la sfocatura"),
        "la luminosità" => motivo!("luce.luminosita", "la luminosità"),
        "la saturazione" => motivo!("luce.saturazione", "la saturazione"),
        "la tinta" => motivo!("luce.tinta", "la tinta"),
        "il colore di fondo" => motivo!("luce.fondo", "il colore di fondo"),
        "il colore del testo" => motivo!("luce.testo", "il colore del testo"),
        "il colore del bordo" => motivo!("luce.bordo", "il colore del bordo"),
        "l'ombra" => motivo!("luce.ombra", "l'ombra"),
        // Una luce nuova in `LUCI` senza la sua riga qui: si legge in
        // italiano, e la prova delle lingue dello Studio non la vede. La
        // prova `ogni_luce_ha_il_suo_motivo` qui sotto sì.
        altro => crate::motivo::Motivo::from(altro),
    }
}

/// La durata che il foglio userà, come fattore: `essential` la dimezza.
///
/// `maximum` la allunga, e non si conta — lo Studio fa scorrere le animazioni
/// alla loro durata scritta, e il conto deve valere anche lì. `none` le ferma
/// tutte, e nemmeno questo si conta: una scala a zero non ferma un ciclo, lo
/// rende infinitamente veloce, e a fermarlo è una regola del foglio che non deve
/// essere l'unica cosa fra una skin e un lampeggio.
fn scala_delle_durate(motion: &SkinMotion) -> f64 {
    match motion.intensity.scale() {
        s if s > 0.0 => s.min(1.0),
        _ => 1.0,
    }
}

/// Un'animazione che lampeggia su una delle tracce misurate.
struct Lampo<'m> {
    /// Il nome dell'animazione.
    nome: &'m str,
    /// Quale proprietà: «l'opacità», «la scala»…
    cosa: &'static str,
    /// Il mezzo ciclo più breve, in millisecondi.
    mezzo: f64,
    /// «fa 5 lampi in un giro solo», o «si ripete all'infinito».
    quanto: crate::motivo::Motivo,
    /// È una corsa che si ferma entro [`RIPETIZIONI_SENZA_LAMPEGGIO`] giri.
    breve: bool,
}

/// Le animazioni che lampeggiano su una delle `tracce`, col conto descritto in
/// «Il lampeggio» in cima a questo file.
fn lampi<'m>(motion: &'m SkinMotion, tracce: &[Tracciato]) -> Vec<Lampo<'m>> {
    let scala = scala_delle_durate(motion);
    let mut trovati = Vec::new();
    for (nome, animazione) in &motion.animations {
        let alterna = matches!(
            animazione.direction,
            AnimDirection::Alternate | AnimDirection::AlternateReverse
        );
        let durata = animazione.duration.ms * scala;
        let curva = curva_che_rimbalza(motion, &animazione.easing);
        let giri = tracce.iter().filter_map(|(cosa, leggi, taglio)| {
            giro(
                &animazione.frames,
                durata,
                *leggi,
                curva.as_deref(),
                *taglio,
            )
            .map(|giro| (*cosa, giro))
        });

        let lampo = match animazione.iterations {
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
                let quanto = if volte == 1 {
                    motivo!(
                        "lampi.unGiro",
                        "fa {lampi} lampi in un giro solo",
                        lampi = lampi
                    )
                } else {
                    motivo!(
                        "lampi.giri",
                        "fa {lampi} lampi in {volte} giri",
                        lampi = lampi,
                        volte = volte
                    )
                };
                Lampo {
                    nome,
                    cosa,
                    mezzo,
                    quanto,
                    breve: true,
                }
            }
            // Un ciclo, o quasi: decide il lampo più breve.
            iterazioni => {
                let peggiore = giri
                    .filter_map(|(cosa, giro)| {
                        mezzo_ciclo(&giro, durata, alterna).map(|m| (cosa, m))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let Some((cosa, mezzo)) = peggiore.filter(|(_, m)| *m < MEZZO_CICLO_MINIMO_MS)
                else {
                    continue;
                };
                Lampo {
                    nome,
                    cosa,
                    mezzo,
                    quanto: match iterazioni {
                        Iterazioni::Infinite => {
                            motivo!("ripete.infinito", "si ripete all'infinito")
                        }
                        Iterazioni::Volte(n) => {
                            motivo!("ripete.volte", "si ripete {volte} volte", volte = n)
                        }
                    },
                    breve: false,
                }
            }
        };
        trovati.push(lampo);
    }
    trovati
}

/// Le animazioni che lampeggiano, come errori sulla loro durata.
pub(super) fn lampeggi(motion: &SkinMotion) -> Vec<SkinIssue> {
    let scala = scala_delle_durate(motion);
    lampi(motion, LUCI)
        .into_iter()
        .map(|lampo| {
            let Lampo {
                nome,
                cosa,
                mezzo,
                quanto,
                breve,
            } = lampo;
            let consiglio = if breve {
                motivo!(
                    "consiglio.ferma",
                    "o anima una trasformazione invece della luce: più di {massimo} lampi \
                     rapidi non passano nemmeno in un'animazione che si ferma.",
                    massimo = RIPETIZIONI_SENZA_LAMPEGGIO
                )
            } else {
                motivo!(
                    "consiglio.ciclo",
                    "anima una trasformazione invece della luce, o fermala a {massimo} \
                     ripetizioni.",
                    massimo = RIPETIZIONI_SENZA_LAMPEGGIO
                )
            };
            // Arrotondati prima: il modello di `motivo!` non ha formati, perché
            // sono gli stessi segnaposto del catalogo delle lingue.
            let mezzo = format!("{mezzo:.0}");
            let minimo = format!("{MEZZO_CICLO_MINIMO_MS:.0}");
            let cosa = motivo_luce(cosa);
            let motivo = if scala < 1.0 {
                motivo!(
                    "lampeggiaIntensita",
                    "«{nome}» lampeggia: {cosa} cambia ogni {mezzo} ms e {quanto}, sotto i \
                     {minimo} ms per mezzo ciclo. Una luce che cambia così in fretta e si ripete \
                     può scatenare una crisi in chi soffre di epilessia fotosensibile (WCAG \
                     2.3.1). Allunga la durata, togli i fotogrammi che vanno e vengono, \
                     {consiglio} Con l'intensità «{intensita}» il foglio moltiplica ogni durata \
                     per {scala}, e il conto si fa sulla durata che il foglio userà.",
                    nome = nome,
                    cosa = cosa,
                    mezzo = mezzo,
                    quanto = quanto,
                    minimo = minimo,
                    consiglio = consiglio,
                    intensita = motion.intensity.as_str(),
                    scala = scala
                )
            } else {
                motivo!(
                    "lampeggia",
                    "«{nome}» lampeggia: {cosa} cambia ogni {mezzo} ms e {quanto}, sotto i \
                     {minimo} ms per mezzo ciclo. Una luce che cambia così in fretta e si ripete \
                     può scatenare una crisi in chi soffre di epilessia fotosensibile (WCAG \
                     2.3.1). Allunga la durata, togli i fotogrammi che vanno e vengono, \
                     {consiglio}",
                    nome = nome,
                    cosa = cosa,
                    mezzo = mezzo,
                    quanto = quanto,
                    minimo = minimo,
                    consiglio = consiglio
                )
            };
            problema(
                &giu(&giu(&giu("motion", "animations"), nome), "duration"),
                motivo,
            )
        })
        .collect()
}

// ── Il lampeggio della forma ────────────────────────────────────────────────
//
// Le trasformazioni e il ritaglio stanno fuori da [`LUCI`] perché spostano la
// parte invece di cambiarne la luce. Tre casi però, per chi guarda, sono un
// cambio di luce di tutta l'area: una scala che arriva a zero, un ritaglio che
// la chiude tutta, uno spostamento che la porta fuori dal suo posto. Animati
// in fretta e ripetuti, la parte appare e sparisce: `page-content` su `hover`
// cinque volte al secondo era una skin valida (R09-9). Si misurano con lo
// stesso conto della luce, ma su una traccia di due soli valori — la parte c'è
// o non c'è — e non sono un errore: sono una domanda prima di indossarla
// (`Esame::lampi`, D-49), perché la misura non sa quanto è grande la parte.

/// La parte c'è (1) o non c'è (0), su un fotogramma.
const IN_VISTA: Taglio = (0.0, 1.0);

/// Una scala sotto un centesimo è una parte che non si vede.
fn per_scala(scala: f64) -> f64 {
    if scala.abs() <= 0.01 { 0.0 } else { 1.0 }
}

/// Uno spostamento di tutta la sua misura, o più, porta la parte fuori dal suo
/// posto. In pixel non si sa quanto è grande la parte, e non si conta.
fn per_spostamento(spostamento: Length) -> f64 {
    if spostamento.unit == LengthUnit::Percent && spostamento.value.abs() >= 100.0 {
        0.0
    } else {
        1.0
    }
}

/// Un ritaglio che non lascia niente: due lati opposti che si incontrano, o un
/// cerchio senza raggio.
fn per_ritaglio(ritaglio: &Ritaglio) -> f64 {
    let percento = |l: &Length| (l.unit == LengthUnit::Percent).then_some(l.value);
    let vuoto = match ritaglio {
        Ritaglio::Inset {
            top,
            right,
            bottom,
            left,
        } => {
            let chiude = |a: &Length, b: &Length| {
                percento(a)
                    .zip(percento(b))
                    .is_some_and(|(a, b)| a + b >= 100.0)
            };
            chiude(top, bottom) || chiude(left, right)
        }
        Ritaglio::Circle { radius, .. } => radius.value <= 0.0,
        Ritaglio::Polygon(_) => false,
    };
    if vuoto { 0.0 } else { 1.0 }
}

/// Le tracce della forma. Vedi «Il lampeggio della forma».
const FORME: &[Tracciato] = &[
    (
        "la scala",
        |f| f.scale.map(|v| Luce::Numero(per_scala(v))),
        IN_VISTA,
    ),
    (
        "la scala orizzontale",
        |f| f.scale_x.map(|v| Luce::Numero(per_scala(v))),
        IN_VISTA,
    ),
    (
        "la scala verticale",
        |f| f.scale_y.map(|v| Luce::Numero(per_scala(v))),
        IN_VISTA,
    ),
    (
        "lo spostamento orizzontale",
        |f| f.translate_x.map(|l| Luce::Numero(per_spostamento(l))),
        IN_VISTA,
    ),
    (
        "lo spostamento verticale",
        |f| f.translate_y.map(|l| Luce::Numero(per_spostamento(l))),
        IN_VISTA,
    ),
    (
        "il ritaglio",
        |f| f.clip.as_ref().map(|r| Luce::Numero(per_ritaglio(r))),
        IN_VISTA,
    ),
];

/// Le animazioni che fanno apparire e sparire la parte troppo in fretta, con
/// la forma invece che con la luce: i loro nomi, in `motion.animations`. Vedi
/// «Il lampeggio della forma».
pub(crate) fn lampi_di_forma(motion: &SkinMotion) -> Vec<String> {
    lampi(motion, FORME)
        .into_iter()
        .map(|lampo| lampo.nome.to_owned())
        .collect()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn ogni_luce_ha_il_suo_motivo() {
        // Una luce aggiunta a `LUCI` senza la sua riga in `motivo_luce` si
        // leggerebbe in italiano anche nell'interfaccia inglese.
        for (cosa, _, _) in LUCI {
            assert!(
                !motivo_luce(cosa).chiave.is_empty(),
                "«{cosa}» senza chiave"
            );
        }
    }
}
