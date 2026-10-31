//! Le parti: aspetto, stati, livelli e movimento di ogni superficie
//! ridisegnabile, con i richiami a motivi, animazioni e curve già letti.

use serde_json::Value;

use super::valori::{
    colore, curva, durata, effetto, lunghezza, lunghezza_facoltativa, lunghezza_non_negativa,
    misura_fra, non_negativa_facoltativa, ombra_parte,
};
use super::{
    CHIAVI_ANELLO, CHIAVI_ANGOLI, CHIAVI_ASPETTO, CHIAVI_LIVELLO, CHIAVI_MOVIMENTO_PARTE,
    CHIAVI_STRUTTURA_PARTE, CHIAVI_TRANSIZIONE, Esito, SkinIssue, campo, giu, lista_fra,
    numero_fra, oggetto, parola, problema, richiesto, solo_chiavi, testo,
};
use crate::effects::{Effect, EffectTarget, FiltroElemento, Paint};
use crate::movimento::{
    AnimTrigger, Animation, AnimationRef, Iterazioni, MAX_CASCATA_MS, MAX_DURATA_MS, PartMotion,
    Transizione,
};
use crate::parts::{
    AnelloFuoco, BorderStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, PartAppearance, PartDef, PartLayer,
    PartStates, PartStyle, Raggio, TextTransform, nearest_parts, part, ritirata,
};
use crate::stati::PartState;
use crate::values::{Easing, FontKind};
use crate::vicini::{forse, vicini};

// ── Le parti ────────────────────────────────────────────────────────────────

/// Quel che una parte può richiamare per nome, letto prima delle parti.
///
/// Un richiamo si risolve solo se quel che richiama esiste già: motivi,
/// animazioni e curve si leggono prima, e le parti li trovano qui.
pub(super) struct Richiamabili<'a> {
    pub(super) motivi: &'a [(String, Effect)],
    pub(super) animazioni: &'a [(String, Animation)],
    pub(super) curve: &'a [(String, Easing)],
}

/// Uno strato di pittura: `{"$pattern": "nome"}`, oppure un effetto per esteso.
///
/// Un **motivo** nel posto sbagliato produce un nome di variabile che non
/// esiste — `--skin-x-filter` richiesto come `var(--skin-x)` — cioè un
/// riferimento che risolve nel nulla senza che nessuno lo dica. È l'unico caso
/// in cui l'errore è invisibile, ed è per questo che è l'unico controllato.
fn pittura(
    value: &Value,
    path: &str,
    motivi: &[(String, Effect)],
    dove: EffectTarget,
) -> Esito<Paint> {
    let map = oggetto(value, path)?;
    let Some(riferimento) = campo(map, "$pattern") else {
        return effetto(value, path).map(Paint::Inline);
    };

    solo_chiavi(map, path, &["$pattern"])?;
    let percorso = giu(path, "$pattern");
    let nome = testo(riferimento, &percorso)?;
    let Some((_, def)) = motivi.iter().find(|(n, _)| n == nome) else {
        let candidati = vicini(nome, motivi.iter().map(|(n, _)| n.as_str()));
        return Err(problema(
            &percorso,
            format!("motivo inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };

    if def.target() != dove {
        return Err(problema(
            &percorso,
            format!(
                "il motivo «{nome}» è un {}, e qui ci va un {}",
                nome_destinazione(def.target()),
                nome_destinazione(dove)
            ),
        ));
    }

    Ok(Paint::Pattern {
        name: nome.to_owned(),
        def: def.clone(),
    })
}

/// Come si chiama una destinazione nel messaggio d'errore.
pub(super) const fn nome_destinazione(target: EffectTarget) -> &'static str {
    match target {
        EffectTarget::Background => "sfondo",
        EffectTarget::ClipPath => "ritaglio",
        EffectTarget::Filter => "filtro di quel che sta dietro",
    }
}

fn livelli(
    value: &Value,
    path: &str,
    min: usize,
    max: usize,
    motivi: &[(String, Effect)],
) -> Esito<Vec<Paint>> {
    let voci = lista_fra(value, path, min, max)?;
    let mut strati = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        strati.push(pittura(
            grezzo,
            &giu(path, &indice.to_string()),
            motivi,
            EffectTarget::Background,
        )?);
    }
    Ok(strati)
}

/// Il raggio: una lunghezza, o un oggetto con gli angoli.
fn raggio(value: &Value, path: &str) -> Esito<Raggio> {
    let Some(map) = value.as_object() else {
        return lunghezza_non_negativa(value, path).map(Raggio::Uguale);
    };
    solo_chiavi(map, path, CHIAVI_ANGOLI)?;
    if map.is_empty() {
        return Err(problema(
            path,
            "un raggio per angolo deve nominare almeno un angolo",
        ));
    }
    Ok(Raggio::Angoli {
        top_left: non_negativa_facoltativa(map, path, "topLeft")?,
        top_right: non_negativa_facoltativa(map, path, "topRight")?,
        bottom_right: non_negativa_facoltativa(map, path, "bottomRight")?,
        bottom_left: non_negativa_facoltativa(map, path, "bottomLeft")?,
    })
}

/// I filtri sull'elemento: una lista di oggetti a una chiave sola.
///
/// La forma è `[{ "blur": "2px" }, { "saturate": 1.4 }]`, e non un oggetto con
/// tante chiavi, perché l'ordine dei filtri cambia il risultato — una
/// sfocatura prima della saturazione non è una saturazione prima della
/// sfocatura — e l'ordine delle chiavi di un oggetto non è qualcosa su cui
/// fare affidamento.
fn filtri_elemento(value: &Value, path: &str) -> Esito<Vec<FiltroElemento>> {
    if value.is_object() {
        return Err(problema(
            path,
            "qui ci va una lista di filtri sull'elemento, come [{ \"blur\": \"2px\" }]. Il filtro \
             di quel che sta dietro — la sfocatura del vetro — si chiama `backdrop`",
        ));
    }
    let voci = lista_fra(value, path, 1, 6)?;
    let mut filtri = Vec::with_capacity(voci.len());
    for (indice, grezzo) in voci.iter().enumerate() {
        let dentro = giu(path, &indice.to_string());
        let map = oggetto(grezzo, &dentro)?;
        let mut chiavi = map.keys();
        let (Some(nome), None) = (chiavi.next(), chiavi.next()) else {
            return Err(problema(
                &dentro,
                "ogni filtro è un oggetto con una chiave sola: il nome del filtro",
            ));
        };
        let valore = map.get(nome).unwrap_or(&Value::Null);
        let qui = giu(&dentro, nome);
        filtri.push(match nome.as_str() {
            "blur" => FiltroElemento::Blur(misura_fra(valore, &qui, 0.0, 40.0)?),
            "brightness" => FiltroElemento::Brightness(numero_fra(valore, &qui, 0.0, 3.0)?),
            "contrast" => FiltroElemento::Contrast(numero_fra(valore, &qui, 0.0, 3.0)?),
            "saturate" => FiltroElemento::Saturate(numero_fra(valore, &qui, 0.0, 3.0)?),
            "grayscale" => FiltroElemento::Grayscale(numero_fra(valore, &qui, 0.0, 1.0)?),
            "hueRotate" => FiltroElemento::HueRotate(numero_fra(valore, &qui, -360.0, 360.0)?),
            "dropShadow" => {
                let ombra = oggetto(valore, &qui)?;
                solo_chiavi(ombra, &qui, &["x", "y", "blur", "color"])?;
                FiltroElemento::DropShadow {
                    x: lunghezza(richiesto(ombra, &qui, "x")?, &giu(&qui, "x"))?,
                    y: lunghezza(richiesto(ombra, &qui, "y")?, &giu(&qui, "y"))?,
                    blur: lunghezza_non_negativa(
                        richiesto(ombra, &qui, "blur")?,
                        &giu(&qui, "blur"),
                    )?,
                    color: colore(richiesto(ombra, &qui, "color")?, &giu(&qui, "color"))?,
                }
            }
            altro => {
                let candidati = vicini(altro, FiltroElemento::NAMES.iter().copied());
                return Err(problema(
                    &qui,
                    format!("filtro sconosciuto: «{altro}».{}", forse(&candidati)),
                ));
            }
        });
    }
    Ok(filtri)
}

fn aspetto(value: &Value, path: &str, motivi: &[(String, Effect)]) -> Esito<PartAppearance> {
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_ASPETTO)?;
    Ok(PartAppearance {
        background: match campo(map, "background") {
            None => Vec::new(),
            Some(grezzo) => livelli(grezzo, &giu(path, "background"), 0, 4, motivi)?,
        },
        text_color: campo(map, "textColor")
            .map(|v| colore(v, &giu(path, "textColor")))
            .transpose()?,
        border_color: campo(map, "borderColor")
            .map(|v| colore(v, &giu(path, "borderColor")))
            .transpose()?,
        border_width: non_negativa_facoltativa(map, path, "borderWidth")?,
        border_style: campo(map, "borderStyle")
            .map(|v| {
                let nomi: Vec<(&str, BorderStyle)> =
                    BorderStyle::ALL.iter().map(|s| (s.as_str(), *s)).collect();
                parola(v, &giu(path, "borderStyle"), &nomi)
            })
            .transpose()?,
        radius: campo(map, "radius")
            .map(|v| raggio(v, &giu(path, "radius")))
            .transpose()?,
        shadow: campo(map, "shadow")
            .map(|v| ombra_parte(v, &giu(path, "shadow")))
            .transpose()?,
        focus_ring: campo(map, "focusRing")
            .map(|v| {
                let dentro = giu(path, "focusRing");
                let anello = oggetto(v, &dentro)?;
                solo_chiavi(anello, &dentro, CHIAVI_ANELLO)?;
                Ok(AnelloFuoco {
                    color: colore(richiesto(anello, &dentro, "color")?, &giu(&dentro, "color"))?,
                    width: misura_fra(
                        richiesto(anello, &dentro, "width")?,
                        &giu(&dentro, "width"),
                        0.0,
                        8.0,
                    )?,
                    // In pixel, come lo spessore: fino alla 3.2.0 di prova si confrontava il
                    // numero nudo, e `10rem` — 160 pixel — passava il tetto di
                    // 12 lasciando l'anello lontano dal comando che indica.
                    offset: campo(anello, "offset")
                        .map(|o| misura_fra(o, &giu(&dentro, "offset"), -12.0, 12.0))
                        .transpose()?,
                })
            })
            .transpose()?,
        clip: campo(map, "clip")
            .map(|v| pittura(v, &giu(path, "clip"), motivi, EffectTarget::ClipPath))
            .transpose()?,
        filter: match campo(map, "filter") {
            None => Vec::new(),
            Some(grezzo) => filtri_elemento(grezzo, &giu(path, "filter"))?,
        },
        backdrop: campo(map, "backdrop")
            .map(|v| pittura(v, &giu(path, "backdrop"), motivi, EffectTarget::Filter))
            .transpose()?,
        opacity: campo(map, "opacity")
            .map(|v| numero_fra(v, &giu(path, "opacity"), 0.0, 1.0))
            .transpose()?,
        font: campo(map, "font")
            .map(|v| {
                parola(
                    v,
                    &giu(path, "font"),
                    &[
                        ("sans", FontKind::Sans),
                        ("mono", FontKind::Mono),
                        ("display", FontKind::Display),
                    ],
                )
            })
            .transpose()?,
        font_size: campo(map, "fontSize")
            .map(|v| misura_fra(v, &giu(path, "fontSize"), FONT_SIZE_MIN, FONT_SIZE_MAX))
            .transpose()?,
        line_height: campo(map, "lineHeight")
            .map(|v| numero_fra(v, &giu(path, "lineHeight"), 0.8, 3.0))
            .transpose()?,
        letter_spacing: lunghezza_facoltativa(map, path, "letterSpacing")?,
        text_transform: campo(map, "textTransform")
            .map(|v| {
                let nomi: Vec<(&str, TextTransform)> = TextTransform::ALL
                    .iter()
                    .map(|t| (t.as_str(), *t))
                    .collect();
                parola(v, &giu(path, "textTransform"), &nomi)
            })
            .transpose()?,
        font_weight: campo(map, "fontWeight")
            .map(|v| numero_fra(v, &giu(path, "fontWeight"), 1.0, 1000.0))
            .transpose()?,
    })
}

/// Un livello su `::before` o `::after`, se la parte ne ha uno libero.
fn livello(
    def: &PartDef,
    quale: &str,
    libero: bool,
    value: &Value,
    path: &str,
    motivi: &[(String, Effect)],
) -> Esito<PartLayer> {
    if !libero {
        // Non è un capriccio: lo pseudo-elemento di questa parte è già usato
        // dal componente, o la parte è un'immagine e non ne ha. Una skin che ci
        // scrivesse sopra non otterrebbe il suo livello — e toglierebbe quel che
        // c'era, o non disegnerebbe niente.
        return Err(problema(
            path,
            format!(
                "la parte «{}» non ha `::{quale}` libero per un livello",
                def.name
            ),
        ));
    }
    let map = oggetto(value, path)?;
    solo_chiavi(map, path, CHIAVI_LIVELLO)?;
    Ok(PartLayer {
        background: livelli(
            richiesto(map, path, "background")?,
            &giu(path, "background"),
            1,
            3,
            motivi,
        )?,
        opacity: campo(map, "opacity")
            .map(|v| numero_fra(v, &giu(path, "opacity"), 0.0, 1.0))
            .transpose()?,
    })
}

/// Il movimento di una parte: `parts.<p>.motion`.
fn movimento_parte(
    def: &'static PartDef,
    value: &Value,
    path: &str,
    richiamabili: &Richiamabili<'_>,
) -> Esito<PartMotion> {
    let map = oggetto(value, path)?;
    let mut motion = PartMotion::default();

    for (chiave, grezzo) in map {
        let percorso = giu(path, chiave);
        match chiave.as_str() {
            "stagger" => {
                let passo = durata(grezzo, &percorso)?;
                if passo.ms > MAX_CASCATA_MS {
                    return Err(problema(
                        &percorso,
                        format!("il passo della cascata va fino a {MAX_CASCATA_MS}ms"),
                    ));
                }
                motion.stagger = Some(passo);
            }
            "transition" => {
                let blocco = oggetto(grezzo, &percorso)?;
                solo_chiavi(blocco, &percorso, CHIAVI_TRANSIZIONE)?;
                let tempo = durata(
                    richiesto(blocco, &percorso, "duration")?,
                    &giu(&percorso, "duration"),
                )?;
                if tempo.ms > MAX_DURATA_MS {
                    return Err(problema(
                        &giu(&percorso, "duration"),
                        format!("al massimo {MAX_DURATA_MS}ms"),
                    ));
                }
                motion.transition = Some(Transizione {
                    duration: tempo,
                    easing: campo(blocco, "easing")
                        .map(|v| curva(v, &giu(&percorso, "easing"), richiamabili.curve))
                        .transpose()?
                        .unwrap_or_default(),
                });
            }
            altro => {
                let Some(trigger) = AnimTrigger::parse(altro) else {
                    let nomi: Vec<&str> = AnimTrigger::nomi()
                        .into_iter()
                        .chain(CHIAVI_MOVIMENTO_PARTE.iter().copied())
                        .collect();
                    let candidati = vicini(altro, nomi.iter().copied());
                    let dopo = if altro == "active" {
                        " Nel formato 2 lo stato che si chiamava «active» si chiama «current»."
                    } else {
                        ""
                    };
                    return Err(problema(
                        &percorso,
                        format!(
                            "innesco sconosciuto: «{altro}».{}{dopo} Ammessi: {}.",
                            forse(&candidati),
                            nomi.join(", ")
                        ),
                    ));
                };
                if trigger == AnimTrigger::Exit && !def.presenza {
                    return Err(problema(
                        &percorso,
                        format!(
                            "«{}» non entra ed esce dal documento come un'unità, quindi non ha \
                             un'uscita da animare: l'uscita esiste solo sulle presenze — menù, \
                             modali, notifiche, la colonna, la coda, il contenuto della pagina",
                            def.name
                        ),
                    ));
                }
                if trigger == AnimTrigger::Share && !def.condivisa {
                    return Err(problema(
                        &percorso,
                        format!(
                            "«{}» non vola fra due schermate: il volo condiviso esiste solo sulle \
                             copertine",
                            def.name
                        ),
                    ));
                }
                let richiamata = testo(grezzo, &percorso)?;
                let Some((_, animazione)) = richiamabili
                    .animazioni
                    .iter()
                    .find(|(nome, _)| nome == richiamata)
                else {
                    let candidati = vicini(
                        richiamata,
                        richiamabili
                            .animazioni
                            .iter()
                            .map(|(nome, _)| nome.as_str()),
                    );
                    return Err(problema(
                        &percorso,
                        format!(
                            "animazione inesistente: «{richiamata}». \
                             Le animazioni si dichiarano in `motion.animations`.{}",
                            forse(&candidati)
                        ),
                    ));
                };
                if animazione.iterations == Iterazioni::Infinite && !trigger.ammette_ciclo() {
                    return Err(problema(
                        &percorso,
                        format!(
                            "«{richiamata}» si ripete all'infinito, e «{altro}» non è uno stato: \
                             un ciclo dura quanto lo stato che lo accende, e un'entrata, \
                             un'uscita o un cambio non finirebbero mai. Mettilo su uno stato — \
                             «playing», «hover», «current» — o dagli un numero di ripetizioni"
                        ),
                    ));
                }
                motion.set(
                    trigger,
                    AnimationRef {
                        name: richiamata.to_owned(),
                        def: animazione.clone(),
                    },
                );
            }
        }
    }

    Ok(motion)
}

/// Una parte, nella base o nella variante.
///
/// Nella variante il movimento non c'è: un tema cambia i colori e le superfici,
/// non come le cose si muovono, e una parte che si animasse in modo diverso a
/// seconda del tema sarebbe una seconda skin nascosta dentro la prima.
fn stile_parte(
    nome: &str,
    value: &Value,
    path: &str,
    richiamabili: &Richiamabili<'_>,
    nella_variante: bool,
) -> Esito<Option<PartStyle>> {
    let Some(def) = part(nome) else {
        // Una parte ritirata si accetta e non fa niente: il nome è pubblico, e
        // un nome sconosciuto qui **ferma il caricamento** dell'intera skin.
        if ritirata(nome).is_some() {
            return Ok(None);
        }
        let candidati = nearest_parts(nome);
        return Err(problema(
            path,
            format!("parte inesistente: «{nome}».{}", forse(&candidati)),
        ));
    };
    let map = oggetto(value, path)?;

    let ammesse: Vec<&str> = CHIAVI_ASPETTO
        .iter()
        .chain(CHIAVI_STRUTTURA_PARTE.iter())
        .copied()
        .collect();
    solo_chiavi(map, path, &ammesse)?;
    if nella_variante && campo(map, "motion").is_some() {
        return Err(problema(
            &giu(path, "motion"),
            "il movimento non cambia col tema: dichiaralo sulla parte della skin, fuori da `variant`",
        ));
    }

    // L'aspetto e i blocchi strutturali stanno nello stesso oggetto: si toglie
    // quel che non è aspetto e si valida il resto con la stessa funzione degli
    // stati, così base e stati non possono divergere.
    let mut solo_aspetto = map.clone();
    for chiave in CHIAVI_STRUTTURA_PARTE {
        solo_aspetto.remove(*chiave);
    }
    let appearance = aspetto(&Value::Object(solo_aspetto), path, richiamabili.motivi)?;

    let before = campo(map, "before")
        .map(|v| {
            livello(
                def,
                "before",
                def.before,
                v,
                &giu(path, "before"),
                richiamabili.motivi,
            )
        })
        .transpose()?;
    let after = campo(map, "after")
        .map(|v| {
            livello(
                def,
                "after",
                def.after,
                v,
                &giu(path, "after"),
                richiamabili.motivi,
            )
        })
        .transpose()?;

    let mut stati = Vec::new();
    if let Some(grezzo) = campo(map, "states") {
        let dentro = giu(path, "states");
        let blocco = oggetto(grezzo, &dentro)?;
        for (chiave, valore) in blocco {
            let Some(stato) = PartState::parse(chiave) else {
                let candidati = vicini(chiave, PartState::ALL.iter().map(|s| s.as_str()));
                let dopo = if chiave == "active" {
                    " Nel formato 2 lo stato che si chiamava «active» si chiama «current»."
                } else {
                    ""
                };
                return Err(problema(
                    &giu(&dentro, chiave),
                    format!(
                        "stato sconosciuto: «{chiave}».{}{dopo} Ammessi: {}.",
                        forse(&candidati),
                        PartState::nomi().join(", ")
                    ),
                ));
            };
            stati.push((
                stato,
                aspetto(valore, &giu(&dentro, chiave), richiamabili.motivi)?,
            ));
        }
    }

    let motion = match campo(map, "motion") {
        None => PartMotion::default(),
        Some(grezzo) => movimento_parte(def, grezzo, &giu(path, "motion"), richiamabili)?,
    };

    Ok(Some(PartStyle {
        def,
        appearance,
        before,
        after,
        states: PartStates::new(stati),
        motion,
    }))
}

pub(super) fn superfici(
    value: &Value,
    path: &str,
    richiamabili: &Richiamabili<'_>,
    nella_variante: bool,
    problemi: &mut Vec<SkinIssue>,
) -> Esito<Vec<PartStyle>> {
    let map = oggetto(value, path)?;
    let mut stili = Vec::with_capacity(map.len());
    for (nome, grezzo) in map {
        match stile_parte(nome, grezzo, &giu(path, nome), richiamabili, nella_variante) {
            Ok(Some(stile)) => stili.push(stile),
            // Una parte ritirata: letta, accettata, e lasciata cadere qui.
            Ok(None) => {}
            Err(guasto) => problemi.push(guasto),
        }
    }
    // Ordine di registro e non alfabetico: nel CSS l'ordine decide la cascata a
    // parità di specificità, e il registro va dal generale al particolare.
    stili.sort_by_key(|stile| crate::parts::posto(stile.def.name));
    Ok(stili)
}
