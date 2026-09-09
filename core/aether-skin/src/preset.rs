//! I preset: un blocco di valori scritto dentro la skin che si sta già modificando.
//!
//! # Perché non è un secondo sistema
//!
//! Una skin **è già** un preset: è un documento completo, installabile,
//! esportabile, versionato e validato. Quel che manca non è un formato — è la
//! scorciatoia per scrivere in un colpo solo i quindici numeri coerenti che
//! fanno «una scena profonda», invece di quindici trascinamenti di cursore,
//! ognuno dei quali attraversa una zona in cui l'immagine è peggiore di quella
//! da cui si era partiti. Un preset è quindi una **scrittura**, non un oggetto:
//! si applica, e da quel momento non esiste più. Chi lo ha applicato ha una
//! skin, e quella skin la può salvare, spedire e ricevere come qualunque altra.
//!
//! Ne discende la cosa più importante di questo modulo, cioè quello che **non**
//! contiene: nessun campo `preset` nel documento, nessun riferimento da
//! risolvere al caricamento, nessuna eredità. Una skin che ha applicato
//! «Profondo» e una skin in cui qualcuno ha scritto a mano gli stessi sei numeri
//! sono lo stesso file, byte per byte, e nessuna parte del sistema può
//! distinguerle. Un livello di indirezione che non esiste non può divergere dal
//! valore che promette.
//!
//! # Perché il valore è un frammento JSON come stringa
//!
//! Perché è **letteralmente** ciò che finisce nel documento. La sorgente di
//! verità dello Studio è il testo JSON scritto dall'autore, non un modello
//! deserializzato: [`crate::SkinDocument`] non deriva `Serialize`, e non è una
//! svista — sta scritto sopra la sua definizione in [`crate::document`] e
//! ripetuto in `studio/patch.ts`. Un colore validato è una quaterna di canali, e
//! riscriverlo produrrebbe un manifest che quella stessa validazione rifiuta.
//!
//! Applicare un preset vuol dire quindi innestare del testo dentro un altro
//! testo, e un valore che è già testo non ha bisogno che nessuno lo converta:
//! `"0.42"` si scrive `0.42`, `"\"#e8e2ff\""` si scrive `"#e8e2ff"`,
//! `"{\"$token\":\"color.accent\"}"` si scrive com'è. La forma alternativa — un
//! enum di valori tipizzati da riserializzare al momento dell'uso — sarebbe un
//! secondo scrittore di JSON accanto a quello che questo crate ha deciso di non
//! avere, e per funzionare dovrebbe sapere che `{"$token": …}` esiste, che un
//! colore si scrive col cancelletto e non con `rgb()`, e che `1` non va scritto
//! `1.0`. Sono tutte cose che il formato sa già, e che questa tabella non ha
//! nessun motivo di imparare una seconda volta.
//!
//! Il prezzo è che una riga qui dentro può essere un frammento JSON sbagliato, o
//! nominare un token che non c'è, o portare un numero fuori dai suoi estremi. È
//! esattamente il prezzo che la prova qui sotto paga per intero.
//!
//! # Perché la tabella sta in Rust e non in TypeScript
//!
//! Perché così **il validatore esistente la possiede**. La striscia dei preset
//! è una cosa che si vede nello Studio, e la tentazione naturale è scriverla
//! dove si vede. Ma un preset è un insieme di valori di token, e chi sa se un
//! valore di token è buono è [`crate::check_skin`], che gira qui. Scritta di là,
//! la tabella sarebbe sorvegliata da `tsc`, che di `canvas.viz.haze` sa soltanto
//! che è una stringa; scritta di qua, è sorvegliata da
//! `ogni_preset_applicato_a_plain_da_una_skin_valida_e_senza_avvisi`, che fonde
//! ogni preset in [`crate::PLAIN_SOURCE`] e lo fa passare dalle guardie vere.
//!
//! Una tabella di preset marcisce in due modi, e sono i due modi in cui marcisce
//! qualunque elenco di nomi tenuto a mano: un token rinominato o tolto, e un
//! numero che era buono finché qualcuno non ha stretto i `limiti` del suo token.
//! Quella prova li prende tutti e due, e li prende quando si compila la suite —
//! non davanti a un autore che si vede rifiutare il salvataggio per un valore
//! che non ha scelto lui.

use crate::tokens::TokenGroup;

/// Un preset: un nome, il gruppo su cui agisce, e i valori che scrive.
///
/// Non è una skin e non è un frammento di skin: è un elenco di scritture da
/// applicare al documento aperto. Il saggio del modulo dice perché.
#[derive(Debug, Clone, Copy)]
pub struct PresetDef {
    /// L'identificatore stabile: minuscolo, senza spazi, e non si traduce.
    ///
    /// È quel che viaggia sul filo verso lo Studio e che l'interfaccia rimanda
    /// indietro quando qualcuno preme il bottone. Cambiarlo è cambiare il nome
    /// di un comando.
    pub id: &'static str,
    /// Il nome che si legge sul bottone.
    pub nome: &'static str,
    /// Il gruppo di token che questo preset riscrive.
    ///
    /// Serve a una cosa sola, ed è la ragione per cui il campo esiste: lo Studio
    /// disegna la striscia **dal gruppo del token selezionato**, così
    /// `Token.tsx` continua a non nominare nessun token per nome. Un preset di
    /// un altro gruppo, il giorno che ci sarà, comparirà da solo dove deve.
    pub group: TokenGroup,
    /// Le voci da scrivere: l'id del token, e il suo valore come frammento JSON.
    ///
    /// La stringa è il testo che finisce nel documento — `0.42`, `"#e8e2ff"`,
    /// `{"$token":"color.accent"}` — virgola e chiave escluse. Nessuno la
    /// riserializza, perché non c'è niente da riserializzare.
    pub valori: &'static [(&'static str, &'static str)],
}

/// Ogni preset, nell'ordine in cui si mostra.
///
/// L'ordine è quello di una scelta e non l'alfabetico per caso: prima le due
/// destinazioni che si raggiungono più spesso — quella di ieri e quella di serie
/// — poi le due che cambiano davvero l'immagine.
pub static PRESETS: &[PresetDef] = &[
    // Il meno ovvio dei quattro, e vale la pena dire due cose: perché due sole
    // voci bastino a riportare la tavolozza della 2.2.x, e perché di quella
    // versione non riportino — né debbano riportare — proprio tutto.
    //
    // Nella 2.2.x i tre colori della scena **erano lo stesso colore**:
    // `--viz-primary` e `--viz-secondary` compilavano entrambe a `var(--accent)`,
    // e `tinta()` scartava l'alfa di `--accent-glow`, che rientrava quindi come
    // lo stesso RGB. Le due `mix()` dello shader erano due no-op, e la scena si
    // riduceva ad «accento per luce». Rimettere i tre colori sull'accento fa
    // ricollassare quelle due `mix()` esattamente come allora.
    //
    // Tutto il resto — larghezza, profondità, altezza, riempimento, obiettivo,
    // occhio, foschia, riflesso, ambiente, oscillazione, colpo, salita, discesa,
    // fondo scala — è già al valore di ieri, perché i valori di serie del
    // registro sono stati **misurati** sul renderer 2.2.x, non scelti. Per
    // questo «Classico» non ha bisogno di ripeterli, e per questo non deve: una
    // voce ripetuta qui sarebbe una seconda copia del valore di serie, cioè la
    // prima a divergere il giorno che quel valore cambia.
    //
    // Quel che «Classico» rimette sono quindi i colori, e con i colori tutta la
    // geometria e la luce che erano già dov'erano: **non** la 2.2.x byte per
    // byte. Una cosa, sotto a tutte le skin, è cambiata e resta cambiata, ed è
    // il riflesso. Là era un'alfa piatta, uguale dal pavimento alla cima; qui
    // sfuma salendo — con `canvas.viz.reflection` al valore di serie parte da
    // 0,30 sul pavimento e si spegne a 0,05 allontanandosene — perché un
    // riflesso vero si comporta così, e quello piatto sembrava una seconda scena
    // appesa sotto la prima.
    //
    // Non è un valore che qualcuno ha spostato e che un preset possa rimettere a
    // posto: è il renderer che dipinge il riflesso in un altro modo, e il token
    // continua a dire soltanto quanto è forte. La leva per tornare all'alfa
    // piatta non esiste, e non è una dimenticanza — sarebbe un token che
    // descrive una versione invece di descrivere un'immagine, cioè esattamente
    // il genere di manopola che questo registro non tiene. Ed è la stessa
    // ragione per cui non deve esistere: quella sfumatura è l'immagine che
    // questa versione ha deciso di spedire di serie, e un preset che la
    // disfacesse trasformerebbe «Classico» da scorciatoia per una tavolozza in
    // un interruttore che riporta indietro il motore.
    PresetDef {
        id: "classico",
        nome: "Classico",
        group: TokenGroup::Canvas,
        valori: &[
            ("canvas.viz.secondary", r#"{"$token":"color.accent"}"#),
            ("canvas.viz.tip", r#"{"$token":"color.accent"}"#),
        ],
    },
    // La via del ritorno. Sono le diciotto voci di `skins/plain.json` scritte per
    // esteso, ed è l'unico preset che le scrive tutte: dopo mezz'ora di
    // esperimenti nessuno ricorda quali manopole ha toccato, e «rimetti tutto
    // com'era» deve poter riscrivere anche quelle che sono ancora al loro posto.
    //
    // Che questi numeri combacino con `plain.json` non è affidato all'attenzione
    // di nessuno: c'è una prova che li confronta uno per uno, in tutti e due i
    // versi.
    PresetDef {
        id: "curato",
        nome: "Curato",
        group: TokenGroup::Canvas,
        valori: &[
            ("canvas.viz.primary", r#"{"$token":"color.accent"}"#),
            ("canvas.viz.secondary", r##""#1d1b3a""##),
            ("canvas.viz.tip", r##""#e8e2ff""##),
            ("canvas.viz.glow", "20"),
            ("canvas.viz.width", "4.6"),
            ("canvas.viz.depth", "5.2"),
            ("canvas.viz.height", "0.62"),
            ("canvas.viz.fill", "0.7"),
            ("canvas.viz.lens", "52"),
            ("canvas.viz.eye", "0.72"),
            ("canvas.viz.haze", "0.55"),
            ("canvas.viz.reflection", "0.22"),
            ("canvas.viz.ambient", "0.42"),
            ("canvas.viz.sway", "1"),
            ("canvas.viz.beat", "1"),
            ("canvas.viz.attack", "41"),
            ("canvas.viz.release", "258"),
            ("canvas.viz.floor", "-70"),
        ],
    },
    // Nessuna profondità e nessun movimento: una fila di barre illuminata di
    // piatto, che è quel che si vuole quando la scena sta dietro del testo, o
    // quando la macchina è quella che non deve accorgersene. Le tre voci che
    // costano davvero — il riflesso, l'oscillazione e l'alone — vanno a zero
    // insieme, ed è deliberato che restino tre token e non un interruttore: chi
    // vuole il riflesso e non l'oscillazione parte da qui e ne rialza uno.
    PresetDef {
        id: "piatto",
        nome: "Piatto",
        group: TokenGroup::Canvas,
        valori: &[
            ("canvas.viz.reflection", "0"),
            ("canvas.viz.sway", "0"),
            ("canvas.viz.beat", "0.3"),
            ("canvas.viz.ambient", "1"),
            ("canvas.viz.glow", "0"),
            ("canvas.viz.haze", "0.75"),
            ("canvas.viz.lens", "40"),
            ("canvas.viz.eye", "0.45"),
            ("canvas.viz.height", "0.5"),
            ("canvas.viz.fill", "0.86"),
        ],
    },
    // L'opposto: una stanza lunga, guardata da un obiettivo largo e da un occhio
    // alto, con la foschia che comincia presto e il pavimento che riflette. Sei
    // voci sole, tutte di geometria e di luce, e nessun colore — perché la
    // profondità non è una tavolozza, e un preset che cambiasse anche i colori
    // porterebbe via a chi lo applica la skin che stava costruendo.
    PresetDef {
        id: "profondo",
        nome: "Profondo",
        group: TokenGroup::Canvas,
        valori: &[
            ("canvas.viz.depth", "12"),
            ("canvas.viz.lens", "70"),
            ("canvas.viz.eye", "1.1"),
            ("canvas.viz.haze", "0.35"),
            ("canvas.viz.reflection", "0.4"),
            ("canvas.viz.width", "5.6"),
        ],
    },
];

/// I preset che agiscono su un gruppo di token.
///
/// È così che lo Studio costruisce la striscia: partendo dal gruppo del token
/// che si sta modificando, senza nominare né i preset né i token.
pub fn presets_del_gruppo(group: TokenGroup) -> impl Iterator<Item = &'static PresetDef> {
    PRESETS.iter().filter(move |def| def.group == group)
}

/// Il preset con questo id, se esiste.
#[must_use]
pub fn preset(id: &str) -> Option<&'static PresetDef> {
    PRESETS.iter().find(|def| def.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PLAIN_SOURCE, check_skin, parse_skin_json, tokens::token};
    use serde_json::Value;
    use std::collections::HashSet;

    /// `plain` con le voci di un preset fuse dentro il suo oggetto `tokens`.
    ///
    /// Si passa da `serde_json::Value` e non da una sostituzione sul testo
    /// perché un preset **sovrascrive** voci che quasi sempre ci sono già, e due
    /// chiavi uguali nello stesso oggetto JSON sono un caso ambiguo: qui si
    /// vuole provare il documento che l'autore si ritrova, non uno che gli
    /// somiglia.
    fn plain_con(def: &PresetDef) -> String {
        let mut documento: Value =
            serde_json::from_str(PLAIN_SOURCE).expect("plain.json è JSON valido");
        let tokens = documento
            .get_mut("tokens")
            .and_then(Value::as_object_mut)
            .expect("plain.json ha un oggetto «tokens»");
        for (id, frammento) in def.valori {
            let valore: Value = serde_json::from_str(frammento).unwrap_or_else(|err| {
                panic!(
                    "«{id}» del preset «{}» non è un frammento JSON: {err}",
                    def.id
                )
            });
            tokens.insert((*id).to_owned(), valore);
        }
        serde_json::to_string(&documento).expect("un Value si riscrive sempre")
    }

    #[test]
    fn ogni_preset_applicato_a_plain_da_una_skin_valida_e_senza_avvisi() {
        // È la prova che tiene vera la tabella per sempre, e prende i due modi
        // in cui marcisce: un token rinominato o tolto, che qui diventa «token
        // inesistente» — un errore duro — e un numero fuori dai `limiti` del suo
        // token, che diventa «va fra x e y». Senza di lei il difetto si vede per
        // la prima volta addosso a un autore, sotto forma di un preset che si
        // rifiuta di applicarsi.
        for def in PRESETS {
            let sorgente = plain_con(def);
            let skin = parse_skin_json(&sorgente).unwrap_or_else(|err| {
                panic!("il preset «{}» produce una skin non valida: {err}", def.id)
            });
            let avvisi = check_skin(&skin);
            assert!(
                avvisi.is_empty(),
                "il preset «{}» produce avvisi: {:?}",
                def.id,
                avvisi.iter().map(|a| &a.message).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn ogni_preset_ha_un_id_suo_e_nomina_solo_token_che_esistono() {
        let mut visti = HashSet::new();
        for def in PRESETS {
            assert!(visti.insert(def.id), "id ripetuto: {}", def.id);
            assert!(!def.nome.is_empty(), "«{}» non ha un nome", def.id);
            // Un preset vuoto sarebbe un bottone che non fa niente, che è peggio
            // di un bottone che manca: si preme due volte prima di dubitarne.
            assert!(!def.valori.is_empty(), "«{}» non scrive niente", def.id);

            let mut scritti = HashSet::new();
            for (id, _) in def.valori {
                let Some(voce) = token(id) else {
                    panic!("il preset «{}» nomina «{id}», che non è un token", def.id);
                };
                // Due voci sullo stesso token sono una che vince e una che non
                // si vede, e quale delle due dipenderebbe dall'ordine di
                // scrittura.
                assert!(
                    scritti.insert(*id),
                    "il preset «{}» scrive «{id}» due volte",
                    def.id
                );
                // Un preset che uscisse dal proprio gruppo comparirebbe sotto un
                // token e ne cambierebbe un altro, che nell'albero sta da
                // un'altra parte: si vedrebbe muoversi qualcosa che non si stava
                // guardando.
                assert_eq!(
                    voce.group, def.group,
                    "il preset «{}» scrive «{id}», che non è del suo gruppo",
                    def.id
                );
            }
        }
    }

    #[test]
    fn classico_rimette_i_tre_colori_sull_accento() {
        // È l'unica promessa del preset «Classico», ed è tutta la promessa: con
        // i tre colori uguali le due `mix()` dello shader tornano due no-op, e
        // la tavolozza è quella della 2.2.x. La sua scena intera no — il
        // riflesso sfuma comunque, e il commento sulla tabella dice perché non
        // tocchi a un preset rimetterlo piatto. `primary` non è nella tabella
        // perché il suo valore di serie **è già** l'accento.
        let def = preset("classico").expect("«classico» esiste");
        let accento = r#"{"$token":"color.accent"}"#;
        for id in ["canvas.viz.secondary", "canvas.viz.tip"] {
            assert_eq!(
                def.valori.iter().find(|(k, _)| *k == id).map(|(_, v)| *v),
                Some(accento),
                "«{id}» non torna sull'accento"
            );
        }
        assert_eq!(def.valori.len(), 2, "«Classico» scrive più del necessario");
    }

    #[test]
    fn curato_dice_esattamente_quel_che_dice_plain() {
        // «Curato» è la via del ritorno: se una sua voce si allontanasse dal
        // valore di serie, «rimetti tutto com'era» rimetterebbe qualcos'altro.
        let def = preset("curato").expect("«curato» esiste");
        let documento: Value = serde_json::from_str(PLAIN_SOURCE).expect("plain.json è JSON");
        let tokens = documento
            .get("tokens")
            .and_then(Value::as_object)
            .expect("plain.json ha un oggetto «tokens»");
        for (id, frammento) in def.valori {
            let atteso = tokens
                .get(*id)
                .unwrap_or_else(|| panic!("plain.json non dichiara «{id}»"));
            let nostro: Value = serde_json::from_str(frammento).expect("frammento JSON");
            assert_eq!(&nostro, atteso, "«{id}» si è allontanato da plain.json");
        }
        // E il verso opposto: ogni `canvas.viz.*` di plain deve stare qui
        // dentro, altrimenti il ritorno lascerebbe indietro proprio la manopola
        // aggiunta per ultima.
        for chiave in tokens.keys().filter(|k| k.starts_with("canvas.viz.")) {
            assert!(
                def.valori.iter().any(|(id, _)| id == chiave),
                "«Curato» non rimette «{chiave}»"
            );
        }
    }

    #[test]
    fn i_preset_del_gruppo_sono_quelli_del_gruppo() {
        let tela: Vec<_> = presets_del_gruppo(TokenGroup::Canvas)
            .map(|def| def.id)
            .collect();
        assert_eq!(tela, ["classico", "curato", "piatto", "profondo"]);
        assert_eq!(presets_del_gruppo(TokenGroup::Motion).count(), 0);
        assert!(preset("inventato").is_none());
    }
}
